//! Sign-in (ADR 0028, ADR 0033): `/auth/login`, `/auth/callback` and `/auth/logout`, the login
//! check in front of every Connect RPC and WebSocket upgrade, the same-origin check in front of
//! every state-changing request, and `AuthService.WhoAmI`.
//!
//! Fail closed: a request without a live login gets 401 `unauthenticated`, whatever the reason
//! (no cookie, unknown, expired, signed out); the reason goes to the log only. If Postgres can't
//! answer, the request is refused too (503), never let through.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use connectrpc::{ConnectError, RequestContext, ServiceRequest, ServiceResult};
use rl_auth::{AuthError, Provider, SIGN_IN_TTL, Url};
use rl_domain::Member;
use rl_proto::rangelight::auth::v1alpha1::{AuthService, WhoAmIRequest, WhoAmIResponse};
use rl_store::{NewLogin, Store, StoreError};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The login cookie. `__Host-`: the browser takes it only Secure, from this host, for Path=/.
pub const SESSION_COOKIE: &str = "__Host-rl_session";
/// The sign-in cookie: state, nonce and PKCE verifier, sealed, from /auth/login to /auth/callback.
pub const SIGN_IN_COOKIE: &str = "__Host-rl_login";
/// A login unused this long ends.
pub const IDLE: Duration = Duration::from_hours(8);
/// A login ends this long after sign-in, however active.
pub const ABSOLUTE: Duration = Duration::from_hours(7 * 24);
const CLEAR_SESSION: HeaderValue = HeaderValue::from_static(
    "__Host-rl_session=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0",
);
const CLEAR_SIGN_IN: HeaderValue =
    HeaderValue::from_static("__Host-rl_login=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0");

/// Sign-in for one public URL against one OIDC provider.
#[derive(Debug, Clone)]
pub struct Auth(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    store: Store,
    provider: Provider,
    origin: String,
    /// How often a long-lived stream checks that its login still lives.
    recheck: Duration,
}

/// The login a request came with: the SHA-256 of its token, for re-checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LoginHash([u8; 32]);

impl Auth {
    /// Sign-in for browsers at `public_url`, against `provider`; long-lived streams re-check
    /// their login every `recheck`.
    #[must_use]
    pub fn new(store: Store, provider: Provider, public_url: &Url, recheck: Duration) -> Auth {
        Auth(Arc::new(Inner {
            store,
            provider,
            origin: public_url.origin().ascii_serialization(),
            recheck,
        }))
    }
}

/// The api's routes: sign-in, and `rpc` (the Connect services) behind the login check.
pub(crate) fn router(auth: Auth, rpc: axum::Router) -> axum::Router {
    let guarded = rpc.layer(middleware::from_fn_with_state(auth.clone(), require_login));
    axum::Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .with_state(auth.clone())
        .merge(guarded)
        .layer(middleware::from_fn_with_state(auth, same_origin))
}

/// CSRF, on top of SameSite=Lax: anything but a plain GET or HEAD (so every RPC POST, the
/// sign-out, and every upgrade, WebSocket included) must come from the public origin. A request
/// without an `Origin` header is refused too.
async fn same_origin(State(auth): State<Auth>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let reads = matches!(*request.method(), Method::GET | Method::HEAD)
        && !headers.contains_key(header::UPGRADE);
    let origin = headers.get(header::ORIGIN).map(HeaderValue::as_bytes);
    if !reads && origin != Some(auth.0.origin.as_bytes()) {
        tracing::info!(method = %request.method(), path = request.uri().path(), "refused a cross-origin request");
        return ConnectError::permission_denied("cross_origin").into_response();
    }
    next.run(request).await
}

/// Lets a request through only with a live login, and hands its [`Member`] to the handler.
async fn require_login(State(auth): State<Auth>, mut request: Request, next: Next) -> Response {
    let found = match cookie(request.headers(), SESSION_COOKIE) {
        Some(token) => auth
            .0
            .store
            .login_member(&sha256(token), IDLE)
            .await
            .map(|member| member.map(|member| (member, LoginHash(sha256(token))))),
        None => Ok(None),
    };
    match found {
        Ok(Some((member, login))) => {
            request.extensions_mut().insert(member);
            request.extensions_mut().insert(login);
            next.run(request).await
        }
        Ok(None) => unauthenticated(),
        Err(error) => store_unavailable(&error),
    }
}

/// The one answer to a request without a live login (no cookie, or one unknown, expired or
/// signed out): nothing says which.
fn unauthenticated() -> Response {
    tracing::debug!("refused a request without a live login");
    ConnectError::unauthenticated("unauthenticated").into_response()
}

fn store_unavailable(error: &StoreError) -> Response {
    tracing::error!(%error, "login check failed");
    ConnectError::unavailable("store_unavailable").into_response()
}

/// What `/auth/login` takes: only `prompt=consent`, from the Workbench's Sign in after a sign-out.
#[derive(Debug, Deserialize)]
struct Login {
    prompt: Option<String>,
}

/// GET /auth/login: starts a sign-in and sends the browser to the provider. `?prompt=consent` has
/// the provider ask the member to confirm (RAN-82); any other `prompt` is refused, never passed on.
async fn login(State(auth): State<Auth>, Query(query): Query<Login>) -> Response {
    let confirm = match query.prompt.as_deref() {
        None => false,
        Some("consent") => true,
        Some(_) => return ConnectError::invalid_argument("prompt").into_response(),
    };
    match start(&auth, confirm).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn start(auth: &Auth, confirm: bool) -> Result<Response, SignInError> {
    let provider = &auth.0.provider;
    let started = provider.start(confirm).await?;
    // Nothing is stored: state, nonce and PKCE verifier travel sealed in the cookie (review M1).
    let sealed = provider.seal(&started, SystemTime::now())?;
    let cookie = set_cookie(SIGN_IN_COOKIE, &sealed, SIGN_IN_TTL)?;
    let location = HeaderValue::from_str(started.url.as_str())
        .map_err(|_| SignInError::Refused("authorization URL is not a header value"))?;
    Ok((
        StatusCode::SEE_OTHER,
        [(header::LOCATION, location), (header::SET_COOKIE, cookie)],
    )
        .into_response())
}

/// What the provider sends the browser back with.
#[derive(Debug, Deserialize)]
struct Callback {
    code: Option<String>,
    state: Option<String>,
}

/// GET /auth/callback: finishes a sign-in, sets the login cookie and sends the browser home.
/// The sign-in cookie is cleared whatever the outcome: a sign-in is tried once.
async fn callback(
    State(auth): State<Auth>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    let mut response = match finish(&auth, &headers, query).await {
        Ok((member, token)) => {
            tracing::info!(member = member.as_str(), "signed in");
            match set_cookie(SESSION_COOKIE, &token, ABSOLUTE) {
                Ok(cookie) => (
                    StatusCode::SEE_OTHER,
                    [
                        (header::LOCATION, HeaderValue::from_static("/")),
                        (header::SET_COOKIE, cookie),
                    ],
                )
                    .into_response(),
                Err(error) => error.into_response(),
            }
        }
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .append(header::SET_COOKIE, CLEAR_SIGN_IN);
    response
}

async fn finish(
    auth: &Auth,
    headers: &HeaderMap,
    query: Callback,
) -> Result<(Member, SecretString), SignInError> {
    let (Some(code), Some(state)) = (query.code, query.state) else {
        return Err(SignInError::Refused("the provider sent no code"));
    };
    // The sign-in this browser started, sealed by us, unexpired, for this state: no login CSRF,
    // no replayed or forged sign-in.
    let sealed =
        cookie(headers, SIGN_IN_COOKIE).ok_or(SignInError::Refused("no sign-in cookie"))?;
    let attempt = auth.0.provider.open(sealed, &state, SystemTime::now())?;
    let provider = &auth.0.provider;
    let token = provider.exchange(&code, &attempt.pkce_verifier).await?;
    let identity = provider.verify(&token, &attempt.nonce).await?;
    let username = identity
        .member
        .ok_or(SignInError::Refused("the ID token has no member claim"))?;
    let member = Member::from_username(&username)
        .map_err(|_| SignInError::Refused("the member claim is not a member name"))?;
    let login = rl_auth::random_token();
    let new = NewLogin {
        token_hash: sha256(login.expose_secret()),
        member: &member,
        issuer: &identity.issuer,
        subject: &identity.subject,
        absolute: ABSOLUTE,
        idle: IDLE,
    };
    // Signing in again ends the login this browser had (review L1).
    if let Some(old) = cookie(headers, SESSION_COOKIE) {
        auth.0.store.sign_out(&sha256(old)).await?;
    }
    if !auth.0.store.sign_in(&new).await? {
        return Err(SignInError::Refused(
            "another account already owns this member",
        ));
    }
    Ok((member, login))
}

/// POST /auth/logout: deletes the login and clears its cookie.
async fn logout(State(auth): State<Auth>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie(&headers, SESSION_COOKIE)
        && let Err(error) = auth.0.store.sign_out(&sha256(token)).await
    {
        // Still signed in: say so rather than clear the cookie over a live login.
        return SignInError::from(error).into_response();
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, CLEAR_SESSION)],
    )
        .into_response()
}

/// Why a sign-in step failed. The browser learns only "failed" or "unavailable".
#[derive(Debug, thiserror::Error)]
enum SignInError {
    #[error(transparent)]
    Provider(#[from] AuthError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("refused: {0}")]
    Refused(&'static str),
}

impl IntoResponse for SignInError {
    fn into_response(self) -> Response {
        let unavailable = matches!(
            self,
            SignInError::Store(_)
                | SignInError::Provider(
                    AuthError::Unavailable(_) | AuthError::Secret(_) | AuthError::WeakSecret(_)
                )
        );
        let (status, body) = if unavailable {
            let body = "Sign-in is unavailable. Try again later.";
            (StatusCode::SERVICE_UNAVAILABLE, body)
        } else {
            (
                StatusCode::UNAUTHORIZED,
                "Sign-in failed. Start again at /auth/login.",
            )
        };
        tracing::warn!(error = %self, status = status.as_u16(), "sign-in failed");
        (status, body).into_response()
    }
}

/// The value of the cookie `name`, if the request has a non-empty one.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name && !value.is_empty()).then_some(value))
}

fn set_cookie(
    name: &str,
    value: &SecretString,
    max_age: Duration,
) -> Result<HeaderValue, SignInError> {
    let cookie = format!(
        "{name}={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        value.expose_secret(),
        max_age.as_secs()
    );
    let mut cookie = HeaderValue::from_str(&cookie)
        .map_err(|_| SignInError::Refused("a token is not a cookie value"))?;
    cookie.set_sensitive(true);
    Ok(cookie)
}

/// Tokens are stored only as their SHA-256.
fn sha256(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// `AuthService`: who is signed in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WhoAmI;

impl AuthService for WhoAmI {
    fn who_am_i(
        &self,
        ctx: RequestContext,
        _request: ServiceRequest<'_, WhoAmIRequest>,
    ) -> impl Future<Output = ServiceResult<WhoAmIResponse>> + Send {
        // `require_login` put it there; without it, refuse rather than guess.
        let member = ctx.extensions().get::<Member>();
        std::future::ready(match member {
            Some(member) => connectrpc::Response::ok(WhoAmIResponse {
                member: member.as_str().to_owned(),
                ..Default::default()
            }),
            None => Err(ConnectError::unauthenticated("unauthenticated")),
        })
    }
}

mod recheck;
#[cfg(test)]
mod tests;
