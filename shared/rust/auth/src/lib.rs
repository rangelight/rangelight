//! OIDC sign-in (ADR 0028, ADR 0033): the authorization-code flow with PKCE (S256), `state` and
//! `nonce`, against bundled Dex or any OIDC provider. A trust boundary: the ID token is believed
//! only after `openidconnect` has checked its signature (keys from discovery), issuer, audience,
//! expiry and nonce.
//!
//! The client secret is read from its secret file at each code exchange and is never logged;
//! errors never carry it.
#![deny(missing_docs)]

mod sealed;

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreIdTokenClaims, CoreProviderMetadata,
};
use openidconnect::{
    AsyncHttpClient, AuthorizationCode, ClaimsVerificationError, ClientId, ClientSecret, CsrfToken,
    HttpRequest, HttpResponse, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, SignatureVerificationError, TokenResponse,
};
use rl_config::SecretRef;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Mutex;

pub use openidconnect::url::Url;
pub use sealed::{Attempt, SIGN_IN_TTL};

/// How long discovery (endpoints and signing keys) is reused before it is fetched again.
const DISCOVERY_TTL: Duration = Duration::from_secs(300);
/// How long a failed discovery is answered from the cache.
const FAILURE_TTL: Duration = Duration::from_secs(5);
/// Every call to the provider gives up after this long (ADR 0049 §3).
const TIMEOUT: Duration = Duration::from_secs(10);
/// The most the api reads of a provider's answer (discovery, JWKS, token) before parsing it.
const MAX_BODY: usize = 1 << 20;

/// Why a sign-in step failed. Never holds a secret, a code or a token.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The provider can't be reached, or its discovery document or keys don't check.
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    /// The client secret can't be read.
    #[error(transparent)]
    Secret(#[from] rl_config::SecretError),
    /// The provider refused the code: unknown, used, expired, or issued to another client.
    #[error("code refused: {0}")]
    Code(String),
    /// The ID token failed a check: signature, issuer, audience, expiry or nonce.
    #[error("id token refused: {0}")]
    Token(String),
    /// The client secret is too short to key the sign-in cookie. Names the file, never the value.
    #[error(
        "the client secret in {} is shorter than 32 bytes; generate one with `openssl rand -base64 48`",
        .0.display()
    )]
    WeakSecret(std::path::PathBuf),
    /// The sign-in cookie is malformed, tampered with, sealed with another key, expired, or for
    /// another state.
    #[error("sign-in cookie refused: {0}")]
    Cookie(&'static str),
}

/// The start of a sign-in: where to send the browser, and what the callback must match. Its
/// `Debug` shows the secrets redacted.
#[derive(Debug)]
pub struct Started {
    /// The provider's authorization URL, with the PKCE challenge, state and nonce.
    pub url: Url,
    /// The `state` the callback must carry.
    pub state: SecretString,
    /// The `nonce` the ID token must carry.
    pub nonce: SecretString,
    /// The PKCE verifier the code exchange must send.
    pub pkce_verifier: SecretString,
}

/// An ID token as the provider issued it, not yet verified. Its `Debug` hides the token.
pub struct IdToken(CoreIdToken);

impl std::fmt::Debug for IdToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdToken([REDACTED])")
    }
}

/// Who signed in, from a verified ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The token's `iss`.
    pub issuer: String,
    /// The token's `sub`: stable for one account at one issuer.
    pub subject: String,
    /// The value of the member claim (see [`Provider::new`]), if the token has it as a string.
    pub member: Option<String>,
}

/// A fresh random token of 256 bits, base64url: a login cookie's value.
#[must_use]
pub fn random_token() -> SecretString {
    // The same CSPRNG as `state`, `nonce` and the PKCE verifier.
    CsrfToken::new_random_len(32).into_secret().into()
}

/// An OIDC provider and this client's registration with it.
pub struct Provider {
    http: Http,
    issuer: IssuerUrl,
    client_id: ClientId,
    client_secret: SecretRef,
    redirect: RedirectUrl,
    member_claim: String,
    discovery: Mutex<Option<(Instant, Result<CoreProviderMetadata, String>)>>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("issuer", &self.issuer.as_str())
            .field("client_id", &self.client_id.as_str())
            .finish_non_exhaustive()
    }
}

impl Provider {
    /// The provider at `issuer`, for the client `client_id` whose secret is in `client_secret`,
    /// with the browser coming back to `redirect`; `member_claim` names the ID-token claim that
    /// names the member (`preferred_username` for Dex with GitHub). Nothing is fetched until the
    /// first sign-in.
    ///
    /// # Errors
    /// [`AuthError::Unavailable`] if the HTTP client can't be built.
    pub fn new(
        issuer: Url,
        client_id: String,
        client_secret: SecretRef,
        redirect: Url,
        member_claim: String,
    ) -> Result<Provider, AuthError> {
        // reqwest's TLS needs a process-wide rustls provider; an Err means one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            // A provider must not bounce the api's requests elsewhere (openidconnect's SSRF advice).
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AuthError::Unavailable(chain(&e)))?;
        Ok(Provider {
            http: Http(client),
            issuer: IssuerUrl::from_url(issuer),
            client_id: ClientId::new(client_id),
            client_secret,
            redirect: RedirectUrl::from_url(redirect),
            member_claim,
            discovery: Mutex::new(None),
        })
    }

    /// Starts a sign-in: a fresh state, nonce and PKCE (S256) verifier, and the URL that asks the
    /// provider for `openid profile`.
    ///
    /// # Errors
    /// [`AuthError::Unavailable`] if discovery fails.
    pub async fn start(&self) -> Result<Started, AuthError> {
        let client = CoreClient::from_provider_metadata(
            self.discover(false).await?,
            self.client_id.clone(),
            None,
        )
        .set_redirect_uri(self.redirect.clone());
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(Started {
            url,
            state: state.into_secret().into(),
            nonce: nonce.secret().clone().into(),
            pkce_verifier: verifier.into_secret().into(),
        })
    }

    /// Exchanges an authorization code (with its PKCE verifier) for the ID token. The token is
    /// not verified yet: pass it to [`Provider::verify`].
    ///
    /// # Errors
    /// [`AuthError::Code`] if the provider refuses the code, [`AuthError::Secret`] if the client
    /// secret can't be read, [`AuthError::Unavailable`] if the provider can't be reached.
    pub async fn exchange(
        &self,
        code: &str,
        pkce_verifier: &SecretString,
    ) -> Result<IdToken, AuthError> {
        let secret = ClientSecret::new(self.client_secret.read()?.expose_secret().to_owned());
        let client = CoreClient::from_provider_metadata(
            self.discover(false).await?,
            self.client_id.clone(),
            Some(secret),
        )
        .set_redirect_uri(self.redirect.clone());
        let response = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|e| AuthError::Unavailable(chain(&e)))?
            .set_pkce_verifier(PkceCodeVerifier::new(
                pkce_verifier.expose_secret().to_owned(),
            ))
            .request_async(&self.http)
            .await
            .map_err(|e| AuthError::Code(chain(&e)))?;
        let token = response
            .id_token()
            .ok_or_else(|| AuthError::Token("the provider returned no ID token".into()))?;
        Ok(IdToken(token.clone()))
    }

    /// Verifies an ID token: signature against the provider's published keys, issuer, audience
    /// (this client), expiry, and `nonce`. Keys are fetched again once if the token's key is
    /// unknown, in case the provider rotated them.
    ///
    /// # Errors
    /// [`AuthError::Token`] if any check fails, [`AuthError::Unavailable`] if the keys can't be
    /// fetched.
    pub async fn verify(
        &self,
        token: &IdToken,
        nonce: &SecretString,
    ) -> Result<Identity, AuthError> {
        let nonce = Nonce::new(nonce.expose_secret().to_owned());
        let checked = match self.check(self.discover(false).await?, token, &nonce) {
            Err(ClaimsVerificationError::SignatureVerification(
                SignatureVerificationError::NoMatchingKey,
            )) => self.check(self.discover(true).await?, token, &nonce),
            checked => checked,
        };
        let claims = checked.map_err(|e| AuthError::Token(chain(&e)))?;
        Ok(Identity {
            issuer: claims.issuer().to_string(),
            subject: claims.subject().to_string(),
            member: string_claim(token, &self.member_claim),
        })
    }

    fn check(
        &self,
        metadata: CoreProviderMetadata,
        token: &IdToken,
        nonce: &Nonce,
    ) -> Result<CoreIdTokenClaims, ClaimsVerificationError> {
        let client = CoreClient::from_provider_metadata(metadata, self.client_id.clone(), None);
        token.0.claims(&client.id_token_verifier(), nonce).cloned()
    }

    /// The provider's discovery document and keys, cached for [`DISCOVERY_TTL`]; `fresh` asks
    /// for a copy fetched after the call. One fetch at a time: callers wait on the lock, then use
    /// what the fetch before them got (single flight). A failure is cached too, for
    /// [`FAILURE_TTL`] and for everyone who asked before it came, so a hung provider costs one
    /// fetch, not one per waiter. `discover_async` refuses a document whose issuer isn't exactly
    /// ours.
    async fn discover(&self, fresh: bool) -> Result<CoreProviderMetadata, AuthError> {
        let asked = Instant::now();
        let mut cache = self.discovery.lock().await;
        if let Some((at, cached)) = cache.as_ref() {
            let usable = match cached {
                Ok(_) if fresh => *at >= asked,
                Ok(_) => at.elapsed() < DISCOVERY_TTL,
                Err(_) => *at >= asked || at.elapsed() < FAILURE_TTL,
            };
            if usable {
                return cached.clone().map_err(AuthError::Unavailable);
            }
        }
        let fetched = CoreProviderMetadata::discover_async(self.issuer.clone(), &self.http)
            .await
            .map_err(|e| chain(&e));
        *cache = Some((Instant::now(), fetched.clone()));
        fetched.map_err(AuthError::Unavailable)
    }
}

/// The string claim `name` of a token `claims()` has just verified. It is read from the same
/// payload bytes the signature covers (openidconnect keeps them verbatim), because the claim may
/// be one its typed claims don't model.
fn string_claim(token: &IdToken, name: &str) -> Option<String> {
    let jwt = token.0.to_string();
    let payload = URL_SAFE_NO_PAD.decode(jwt.split('.').nth(1)?).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims.get(name)?.as_str().map(str::to_owned)
}

/// An error and its causes, `outer: inner: …`: openidconnect's own messages ("Request failed")
/// leave out why.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        text = format!("{text}: {inner}");
        cause = inner.source();
    }
    text
}

/// `openidconnect`'s HTTP client, on the workspace's reqwest.
struct Http(reqwest::Client);

/// An HTTP failure talking to the provider.
#[derive(Debug, thiserror::Error)]
#[error("http: {0}")]
struct HttpError(String);

impl<'c> AsyncHttpClient<'c> for Http {
    type Error = HttpError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, HttpError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            let request =
                reqwest::Request::try_from(request).map_err(|e| HttpError(e.to_string()))?;
            let mut response = self
                .0
                .execute(request)
                .await
                .map_err(|e| HttpError(e.without_url().to_string()))?;
            let status = response.status();
            let headers = response.headers().clone();
            let too_large = || HttpError(format!("the answer is over {MAX_BODY} bytes"));
            if response
                .content_length()
                .is_some_and(|n| n > MAX_BODY as u64)
            {
                return Err(too_large());
            }
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| HttpError(e.to_string()))?
            {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(too_large());
                }
                body.extend_from_slice(&chunk);
            }
            let mut out = HttpResponse::new(body);
            *out.status_mut() = status;
            *out.headers_mut() = headers;
            Ok(out)
        })
    }
}
