//! The api as NATS's auth callout (ADR 0021 amendment). Every NATS client except this callout's
//! own connection signs in with a projected ServiceAccount token (audience `rangelight-nats`).
//! The server hands each sign-in to the api on `$SYS.REQ.USER.AUTH`; the api verifies the token
//! (`token`, against its issuer's `keys`), maps it to an identity (`identity`) and answers with a
//! user JWT, signed by the callout issuer's nkey, holding exactly that identity's permissions and
//! account (`permissions`, `jwt`), valid until the token expires or for one hour, whichever is
//! sooner. Anything else is refused. Every api replica answers in one queue group, so they share
//! the sign-ins.

mod identity;
mod jwt;
mod keys;
mod lane;
mod permissions;
mod token;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_nats::{Client, ConnectOptions, Message, Subscriber};
use futures_util::StreamExt;
use nkeys::KeyPair;
use rl_domain::{SessionId, subject};
use rl_store::Store;
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;

pub use identity::Identity;
pub use token::{AUDIENCE, TokenError, Verified, Verifier};

/// Where the server asks the callout about each sign-in.
const AUTH_SUBJECT: &str = "$SYS.REQ.USER.AUTH";
/// The queue group every api replica answers in.
const QUEUE: &str = "rangelight-api";
/// The callout connection's inbox id: its permissions (the server's config) allow only this one.
const INBOX: &str = "callout";
/// The longest a user JWT lives; a client signs in again with its rotated token after.
const MAX_LIFETIME: u64 = 3600;
/// Sign-ins answered at once.
const IN_FLIGHT: usize = 32;
/// The bound on each lookup a sign-in makes (Postgres, the server's connections).
pub(crate) const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the callout couldn't start.
#[derive(Debug, thiserror::Error)]
pub enum CalloutError {
    /// The issuer seed isn't an nkey seed.
    #[error("nats_auth.issuer_seed is not an nkey seed")]
    Seed,
    /// The auth user's credential isn't a seed or a `.creds` file.
    #[error("nats_auth.auth_user is not an nkey seed or a .creds file")]
    Credential,
    /// A secret couldn't be read.
    #[error(transparent)]
    Secret(#[from] rl_config::SecretError),
    /// An issuer's CA couldn't be loaded.
    #[error(transparent)]
    Token(#[from] TokenError),
    /// NATS refused the connection or the subscription.
    #[error("nats: {0}")]
    Nats(String),
}

/// Whose sign-ins a callout answers, and how it signs them.
#[derive(Debug)]
pub struct Settings {
    /// The org's subject token.
    pub org: String,
    /// The runner every session runs at: lanes sign in only from its cluster.
    pub sessions_runner: String,
    /// The account the control plane's and the runners' user JWTs are for (`nats_auth.account`).
    pub account: String,
    /// The lanes' account (`nats_auth.lane_account`).
    pub lane_account: String,
    /// The callout issuer's nkey seed: it signs every response, and every user JWT but a lane's
    /// when `lane_issuer_seed` is set.
    pub issuer_seed: SecretString,
    /// The lane account's signing key (operator mode), else the issuer's.
    pub lane_issuer_seed: Option<SecretString>,
}

/// The auth callout of one org.
#[derive(Debug)]
pub struct Callout {
    verifier: Verifier,
    store: Store,
    org: String,
    sessions_runner: String,
    issuer: KeyPair,
    lane_issuer: KeyPair,
    account: String,
    lane_account: String,
    nats: Client,
    lanes: Mutex<HashMap<SessionId, Arc<lane::Slot>>>,
}

impl Callout {
    /// Connects to `url` as the callout's auth user, with `credential`: an nkey seed, or a
    /// `.creds` file (a user JWT and its seed, operator mode).
    ///
    /// # Errors
    /// [`CalloutError::Credential`] or [`CalloutError::Nats`].
    pub async fn connect(url: &str, credential: &SecretString) -> Result<Client, CalloutError> {
        let credential = credential.expose_secret();
        let options = if credential.contains("BEGIN NATS USER JWT") {
            ConnectOptions::with_credentials(credential).map_err(|_| CalloutError::Credential)?
        } else {
            ConnectOptions::with_nkey(credential.trim().to_owned())
        };
        options
            .custom_inbox_prefix(subject::inbox(INBOX))
            .name("rangelight-api auth callout")
            .connect(url)
            .await
            .map_err(|e| CalloutError::Nats(e.to_string()))
    }

    /// The callout on `nats` (from [`Callout::connect`]), as `settings` say.
    ///
    /// # Errors
    /// [`CalloutError::Seed`].
    pub fn new(
        nats: Client,
        store: Store,
        verifier: Verifier,
        settings: Settings,
    ) -> Result<Callout, CalloutError> {
        let key = |seed: &SecretString| {
            KeyPair::from_seed(seed.expose_secret().trim()).map_err(|_| CalloutError::Seed)
        };
        let issuer = key(&settings.issuer_seed)?;
        let lane_issuer = key(settings
            .lane_issuer_seed
            .as_ref()
            .unwrap_or(&settings.issuer_seed))?;
        Ok(Callout {
            verifier,
            store,
            org: settings.org,
            sessions_runner: settings.sessions_runner,
            issuer,
            lane_issuer,
            account: settings.account,
            lane_account: settings.lane_account,
            nats,
            lanes: Mutex::default(),
        })
    }

    /// Subscribes to the server's sign-ins; once this returns, the server can sign clients in.
    ///
    /// # Errors
    /// [`CalloutError::Nats`].
    pub async fn listen(&self) -> Result<Subscriber, CalloutError> {
        let nats = |e: &dyn std::fmt::Display| CalloutError::Nats(e.to_string());
        let requests = self
            .nats
            .queue_subscribe(AUTH_SUBJECT, QUEUE.into())
            .await
            .map_err(|e| nats(&e))?;
        self.nats.flush().await.map_err(|e| nats(&e))?;
        Ok(requests)
    }

    /// Answers `requests` (from [`Callout::listen`]), 32 at a time, until `cancel`.
    pub async fn serve(self: Arc<Self>, requests: Subscriber, cancel: CancellationToken) {
        let answers = requests.for_each_concurrent(IN_FLIGHT, |request| {
            let callout = Arc::clone(&self);
            async move { callout.answer(request).await }
        });
        let _ = cancel.run_until_cancelled(answers).await;
    }

    /// Who `token` is at `now` (seconds since the epoch), and until when its user JWT lives; or
    /// the reason it's refused.
    ///
    /// # Errors
    /// A machine-readable reason.
    pub async fn authorize(
        &self,
        token: Option<&str>,
        now: u64,
    ) -> Result<(Identity, u64), &'static str> {
        let token = token.ok_or("no_token")?;
        let verified = self.verifier.verify(token, now).await.map_err(|error| {
            if let TokenError::Jwks(cause) = &error {
                tracing::warn!(%cause, "can't fetch an issuer's keys");
            }
            error.reason()
        })?;
        let claim = identity::claim(
            &verified.issuer,
            &self.sessions_runner,
            &verified.namespace,
            &verified.account,
            verified.pod.as_deref(),
        )
        .ok_or("unknown_identity")?;
        let identity = match claim {
            identity::Claim::Known(identity) => identity,
            identity::Claim::Lane(namespace) => Identity::Lane(self.live_lane(&namespace).await?),
        };
        Ok((identity, verified.expires.min(now + MAX_LIFETIME)))
    }

    /// Answers one sign-in: a user JWT, or the reason it's refused. The token never reaches a log.
    async fn answer(&self, message: Message) {
        if let Err(error) = self.try_answer(message).await {
            tracing::warn!(%error, "couldn't answer a sign-in");
        }
    }

    async fn try_answer(&self, message: Message) -> Result<(), String> {
        let reply = message
            .reply
            .ok_or("a sign-in request without a reply subject")?;
        let request = jwt::request(&message.payload).map_err(|e| e.to_string())?;
        let now = unix_now();
        let user = self.user(&request, now).await;
        let answer = user.as_deref().map_err(|reason| *reason);
        let response =
            jwt::response(&self.issuer, &request, answer, now).map_err(|e| e.to_string())?;
        let sent = self.nats.publish(reply, response.into()).await;
        sent.map_err(|e| e.to_string())
    }

    /// The user JWT for `request` at `now`, or why there is none.
    async fn user(&self, request: &jwt::Request, now: u64) -> Result<String, &'static str> {
        let (identity, expires) = self
            .authorize(request.token.as_deref(), now)
            .await
            .inspect_err(|reason| tracing::info!(reason, "refused a NATS client"))?;
        let grant = permissions::grant(&self.org, &identity);
        tracing::info!(user = %grant.name, "signed a NATS client in");
        let (signer, account) = match grant.account {
            permissions::Account::Main => (&self.issuer, &self.account),
            permissions::Account::Lane => (&self.lane_issuer, &self.lane_account),
        };
        jwt::user(signer, account, request, &grant, now..expires).map_err(|_| "signing_failed")
    }
}

/// Seconds since the epoch.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests;
