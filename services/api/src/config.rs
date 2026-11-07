//! The api's config (ADR 0049 §6), loaded by `rl-config` from `configs/api.default.toml`.

use std::net::SocketAddr;

use rl_auth::Url;
use rl_config::{ConfigError, SecretRef};
use rl_domain::subject;
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

    fn validate(self) -> Result<Config, ConfigError> {
        subject::token(&self.org).map_err(|e| ConfigError(e.to_string()))?;
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
    fn the_linear_key_only_travels_over_https() {
        let plain = Config {
            linear_url: "http://api.linear.app/graphql".into(),
            ..defaults()
        };
        assert!(plain.validate().is_err());
    }
}
