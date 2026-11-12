//! The api's config (ADR 0049 §6), loaded by `rl-config` from `configs/api.default.toml`.

use std::net::SocketAddr;
use std::path::PathBuf;

use rl_auth::{Registration, Url};
use rl_config::{ConfigError, SecretRef};
use rl_domain::{Workspace, subject};
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
    /// The NATS server.
    pub nats_url: String,
    /// The org's subject token.
    pub org: String,
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
    /// That repo's approved base branch.
    pub base_ref: String,
    /// Whose Claude home the team's lanes mount.
    pub member: String,
    /// A PEM bundle of CA certificates the api's HTTPS (the issuer, Linear) trusts beside the
    /// platform's roots: for an issuer or a Linear proxy behind your own CA.
    pub extra_ca_file: Option<PathBuf>,
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

    fn validate(self) -> Result<Config, ConfigError> {
        subject::token(&self.org).map_err(|e| ConfigError(e.to_string()))?;
        self.workspace()?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const REQUIRED: &str = "
linear_team = \"RAN\"
public_url = \"https://rangelight.example.com\"
oidc_issuer = \"https://rangelight.example.com/dex\"
repo = \"shop\"
member = \"ariel\"
";

    fn defaults() -> Config {
        let toml = [include_str!("../../../configs/api.default.toml"), REQUIRED].concat();
        rl_config::load("api", &toml).unwrap()
    }

    #[test]
    fn defaults_with_the_required_keys_are_valid() {
        defaults().validate().unwrap();
    }

    #[test]
    fn the_team_is_required_and_the_org_checked() {
        let defaults_only = include_str!("../../../configs/api.default.toml");
        assert!(
            rl_config::load::<Config>("api", defaults_only).is_err(),
            "no default team"
        );
        for team in ["", "RAN.*", "a-very-long-team-key"] {
            assert!(
                Config {
                    linear_team: team.into(),
                    ..defaults()
                }
                .validate()
                .is_err(),
                "{team:?}"
            );
        }
        assert!(
            Config {
                org: "a b".into(),
                ..defaults()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn sign_in_needs_https_and_an_origin() {
        let url = |s: &str| Url::parse(s).unwrap();
        for public_url in [
            "http://rangelight.example.com",
            "https://rangelight.example.com/workbench",
            "https://rangelight.example.com/?a=b",
            "https://user@rangelight.example.com",
        ] {
            let config = Config {
                public_url: url(public_url),
                ..defaults()
            };
            assert!(config.validate().is_err(), "{public_url}");
        }
        let plain_issuer = Config {
            oidc_issuer: url("http://dex.example.com/dex"),
            ..defaults()
        };
        assert!(plain_issuer.validate().is_err());
        let no_client = Config {
            oidc_client_id: String::new(),
            ..defaults()
        };
        assert!(no_client.validate().is_err());
        let no_claim = Config {
            oidc_member_claim: String::new(),
            ..defaults()
        };
        assert!(no_claim.validate().is_err());
        let no_recheck = Config {
            login_recheck_seconds: 0,
            ..defaults()
        };
        assert!(no_recheck.validate().is_err());
    }

    #[test]
    fn the_workspace_and_replicas_are_checked() {
        for bad in [
            Config {
                repo: "shop.git".into(),
                ..defaults()
            },
            Config {
                member: String::new(),
                ..defaults()
            },
            Config {
                stream_replicas: 0,
                ..defaults()
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }

    fn roots_from(pem: &str) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, pem.as_bytes()).unwrap();
        Config {
            extra_ca_file: Some(file.path().to_owned()),
            ..defaults()
        }
        .extra_roots()
    }

    #[tokio::test]
    async fn the_extra_ca_file_is_every_certificate_in_it() {
        assert!(defaults().extra_roots().unwrap().is_empty(), "unset: none");
        let ca = rl_testkit::HttpsStub::start(|_| String::new())
            .await
            .ca_pem
            .clone();
        assert_eq!(roots_from(&[ca.as_str(), &ca].concat()).unwrap().len(), 2);
    }

    #[test]
    fn an_extra_ca_file_that_doesnt_parse_stops_the_api() {
        for pem in [
            "",
            "not a certificate",
            "-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n",
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
        ] {
            let error = roots_from(pem).unwrap_err().to_string();
            assert!(error.contains("extra_ca_file /"), "{pem:?}: {error}");
        }
        let missing = Config {
            extra_ca_file: Some("/nonexistent/ca.crt".into()),
            ..defaults()
        };
        let error = missing.extra_roots().unwrap_err().to_string();
        assert!(error.contains("/nonexistent/ca.crt"), "{error}");
    }

    #[test]
    fn the_linear_key_only_travels_over_https() {
        let plain = Config {
            linear_url: "http://api.linear.app/graphql".into(),
            ..defaults()
        };
        assert!(plain.validate().is_err());
    }
}
