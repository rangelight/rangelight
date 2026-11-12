//! The api's config (ADR 0049 §6), loaded by `rl-config` from `configs/api.default.toml`.

use std::net::SocketAddr;
use std::path::PathBuf;

use rl_auth::{Registration, Url};
use rl_config::{ConfigError, SecretRef};
use rl_domain::{Upstream, Workspace, subject};
use rl_tickets_linear::Linear;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use serde::Deserialize;

/// What the api is configured with.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// `tracing` filter directives.
    pub log: String,
    /// The Connect-RPC listener.
    pub listen: SocketAddr,
    /// The Postgres URL, in a secret.
    pub database_url: SecretRef,
    /// The NATS server: `tls://`, or `nats://` inside the cluster (`rl_nats::check_url`).
    pub nats_url: String,
    /// The api's projected token for NATS (audience `rangelight-nats`), read at every connect.
    pub nats_token_file: PathBuf,
    /// The api as NATS's auth callout.
    pub nats_auth: NatsAuth,
    /// The org's subject token.
    pub org: String,
    /// The runner every session runs at, until admission places sessions (ADR 0018): the
    /// relay's `runner`. Lanes sign in only from that runner's cluster.
    pub runner: String,
    /// Linear's GraphQL endpoint.
    pub linear_url: String,
    /// The key of the Linear team whose issues are polled, for example `RAN`.
    pub linear_team: String,
    /// The Linear API key, in a secret.
    pub linear_api_key: SecretRef,
    /// Where browsers reach the Workbench and this api, `https://<host>[:<port>]`: the only
    /// origin a state-changing request or WebSocket upgrade may come from.
    pub public_url: Url,
    /// The OIDC issuer: `<public_url>/dex` for bundled Dex, or your own provider.
    pub oidc_issuer: Url,
    /// The api's client id at the issuer.
    pub oidc_client_id: String,
    /// The api's client secret at the issuer, in a secret.
    pub oidc_client_secret: SecretRef,
    /// The ID-token claim that names the member. With your own provider it must be one the
    /// provider sets and keeps unique, never one users edit.
    pub oidc_member_claim: String,
    /// Seconds between a long-lived stream's checks that its login still lives.
    pub login_recheck_seconds: u64,
    /// JetStream replicas of the runners' status stream.
    pub stream_replicas: usize,
    /// The git mirror's name for the repo the team's sessions work on.
    pub repo: String,
    /// That repo's upstream, `https://github.com/<owner>/<repo>`: where its pull requests are.
    /// Empty: none, and no session keeps a pull request.
    pub upstream: String,
    /// That repo's approved base branch.
    pub base_ref: String,
    /// Whose Claude home the team's lanes mount.
    pub member: String,
    /// A PEM bundle of CA certificates the api's HTTPS (the issuer, Linear) trusts beside the
    /// platform's roots: for an issuer or a Linear proxy behind your own CA.
    pub extra_ca_file: Option<PathBuf>,
}

/// The api as NATS's auth callout (ADR 0021 amendment): it verifies each client's projected
/// token and answers with a user JWT holding the identity's exact permissions.
#[derive(Debug, Clone, Deserialize)]
pub struct NatsAuth {
    /// The callout issuer's nkey seed, in a secret: it signs every user JWT. Its public key is
    /// the server's `auth_callout.issuer`.
    pub issuer_seed: SecretRef,
    /// The api's own sign-in as the callout's auth user, in a secret: an nkey seed (the bundled
    /// server), or a `.creds` file (your own server in operator mode).
    pub auth_user: SecretRef,
    /// The account the control plane's and the runners' user JWTs are for: `RL` on the bundled
    /// server, or the account's public key in operator mode.
    pub account: String,
    /// The lanes' account: `LANE` on the bundled server, or its public key in operator mode.
    pub lane_account: String,
    /// The lane account's signing key seed, in a secret, in operator mode; else the issuer's
    /// signs the lanes' user JWTs too.
    #[serde(default)]
    pub lane_issuer_seed: Option<SecretRef>,
    /// The ServiceAccount token issuers clients may sign in with.
    pub issuers: Vec<Issuer>,
}

/// One cluster's ServiceAccount token issuer and who signs in from it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issuer {
    /// The issuer, as tokens name it (`iss`), https.
    pub url: String,
    /// Its signing keys (JWKS), https.
    pub jwks_url: String,
    /// A bearer token for the JWKS, read at each fetch: the api's own token for the in-cluster
    /// API server. None for a public JWKS.
    #[serde(default)]
    pub jwks_token_file: Option<PathBuf>,
    /// The CA the JWKS's certificate chains to, when it isn't a public one: the cluster's.
    #[serde(default)]
    pub jwks_ca_file: Option<PathBuf>,
    /// The api and the relay run in this cluster: only its tokens can be them.
    #[serde(default)]
    pub control_plane: bool,
    /// The runner of this cluster.
    pub runner: String,
    /// That runner's ServiceAccount, `<namespace>:<name>`.
    pub runner_service_account: String,
}

impl Config {
    /// Loads and validates the config.
    ///
    /// # Errors
    /// [`ConfigError`] for a missing or invalid value.
    pub fn load() -> Result<Config, ConfigError> {
        let config: Config =
            rl_config::load("api", include_str!("../../../configs/api.default.toml"))?;
        config.validate()
    }

    /// The workspace every session of the team starts in.
    ///
    /// # Errors
    /// [`ConfigError`] when `repo`, `base_ref` or `member` is invalid.
    pub fn workspace(&self) -> Result<Workspace, ConfigError> {
        Workspace::new(
            self.repo.clone(),
            self.base_ref.clone(),
            self.member.clone(),
        )
        .map_err(|e| ConfigError(e.to_string()))
    }

    /// The Linear source for `linear_team`, trusting `extra_roots` beside the platform's roots.
    ///
    /// # Errors
    /// [`rl_app::TicketError::Unavailable`] if its HTTP client can't be built.
    pub fn linear(
        &self,
        extra_roots: &[CertificateDer<'static>],
    ) -> Result<Linear, rl_app::TicketError> {
        Linear::new(
            self.linear_url.clone(),
            self.linear_team.clone(),
            self.linear_api_key.clone(),
            extra_roots,
        )
    }

    /// The api's registration at the issuer, coming back to `<public_url>/auth/callback`.
    ///
    /// # Errors
    /// [`ConfigError`] if `public_url` can't take that path.
    pub fn registration(&self) -> Result<Registration, ConfigError> {
        Ok(Registration {
            client_id: self.oidc_client_id.clone(),
            client_secret: self.oidc_client_secret.clone(),
            redirect: self
                .public_url
                .join("/auth/callback")
                .map_err(|e| ConfigError(format!("public_url: {e}")))?,
        })
    }

    /// Every certificate in `extra_ca_file`, read once for all the api's HTTPS clients; none
    /// when it is unset.
    ///
    /// # Errors
    /// [`ConfigError`] naming the file when it can't be read, holds no certificate, or holds one
    /// that doesn't parse: never a silent fallback to the platform's roots alone.
    pub fn extra_roots(&self) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
        let Some(file) = &self.extra_ca_file else {
            return Ok(Vec::new());
        };
        let bad = |why: &dyn std::fmt::Display| {
            ConfigError(format!("extra_ca_file {}: {why}", file.display()))
        };
        let ders = CertificateDer::pem_file_iter(file)
            .map_err(|e| bad(&e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| bad(&e))?;
        if ders.is_empty() {
            return Err(bad(&"no PEM certificate in it"));
        }
        // The check the TLS client makes of a root, made here so a bad one names the file.
        let mut store = rustls::RootCertStore::empty();
        for der in &ders {
            store.add(der.clone()).map_err(|e| bad(&e))?;
        }
        Ok(ders)
    }

    /// The upstream of the team's repo, if one is configured.
    ///
    /// # Errors
    /// [`ConfigError`] when `upstream` isn't empty or a GitHub repo.
    pub fn upstream(&self) -> Result<Option<Upstream>, ConfigError> {
        if self.upstream.is_empty() {
            return Ok(None);
        }
        Upstream::new(&self.repo, &self.upstream)
            .map(Some)
            .map_err(|e| ConfigError(format!("upstream: {e}")))
    }

    fn validate(self) -> Result<Config, ConfigError> {
        for token in [&self.org, &self.runner] {
            subject::token(token).map_err(|e| ConfigError(e.to_string()))?;
        }
        rl_nats::check_url(&self.nats_url).map_err(|e| ConfigError(e.to_string()))?;
        self.nats_auth.validate()?;
        self.workspace()?;
        self.upstream()?;
        if !matches!(self.stream_replicas, 1..=5) {
            return Err(ConfigError("stream_replicas must be 1 to 5".into()));
        }
        let team_ok = !self.linear_team.is_empty()
            && self.linear_team.len() <= 16
            && self.linear_team.bytes().all(|b| b.is_ascii_alphanumeric());
        if !team_ok {
            return Err(ConfigError(
                "linear_team must be a Linear team key, for example RAN".into(),
            ));
        }
        // The Linear key travels in a header: never in clear text.
        if !self.linear_url.starts_with("https://") {
            return Err(ConfigError("linear_url must be an https:// URL".into()));
        }
        // Login cookies are Secure, and the client secret and tokens travel to the issuer.
        let origin_only = self.public_url.path() == "/"
            && self.public_url.query().is_none()
            && self.public_url.username().is_empty()
            && self.public_url.password().is_none();
        if self.public_url.scheme() != "https" || !origin_only {
            return Err(ConfigError(
                "public_url must be https://<host>[:<port>], without a path".into(),
            ));
        }
        if self.oidc_issuer.scheme() != "https" {
            return Err(ConfigError("oidc_issuer must be an https:// URL".into()));
        }
        if self.oidc_client_id.is_empty() || self.oidc_member_claim.is_empty() {
            return Err(ConfigError(
                "oidc_client_id and oidc_member_claim must not be empty".into(),
            ));
        }
        if self.login_recheck_seconds == 0 {
            return Err(ConfigError("login_recheck_seconds must be above 0".into()));
        }
        Ok(self)
    }
}

impl NatsAuth {
    fn validate(&self) -> Result<(), ConfigError> {
        let bad = |what: &str| Err(ConfigError(format!("nats_auth: {what}")));
        // Each names an account, and is a subject token in the lane's CONNZ request.
        for account in [&self.account, &self.lane_account] {
            subject::token(account).map_err(|e| ConfigError(e.to_string()))?;
        }
        if self.account == self.lane_account {
            return bad("lanes need an account of their own (lane_account)");
        }
        if self.issuers.iter().filter(|i| i.control_plane).count() != 1 {
            return bad("exactly one issuer must be the control_plane's");
        }
        let mut urls: Vec<&str> = self.issuers.iter().map(|i| i.url.as_str()).collect();
        urls.sort_unstable();
        urls.dedup();
        if urls.len() != self.issuers.len() {
            return bad("each issuer url once");
        }
        for issuer in &self.issuers {
            // A token's signature is only as good as the keys: never fetched in clear text.
            if !issuer.url.starts_with("https://") || !issuer.jwks_url.starts_with("https://") {
                return bad("an issuer's url and jwks_url must be https://");
            }
            subject::token(&issuer.runner).map_err(|e| ConfigError(e.to_string()))?;
            let account_ok =
                issuer
                    .runner_service_account
                    .split_once(':')
                    .is_some_and(|(ns, name)| {
                        !ns.is_empty() && !name.is_empty() && !name.contains(':')
                    });
            if !account_ok {
                return bad("runner_service_account must be <namespace>:<name>");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
