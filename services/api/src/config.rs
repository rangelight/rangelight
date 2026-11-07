//! The api's config (ADR 0049 §6), loaded by `rl-config` from `configs/api.default.toml`.

use std::net::SocketAddr;

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
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const TEAM: &str = "\nlinear_team = \"RAN\"\n";

    fn defaults() -> Config {
        let toml = [include_str!("../../../configs/api.default.toml"), TEAM].concat();
        rl_config::load("api", &toml).unwrap()
    }

    #[test]
    fn defaults_with_a_team_are_valid() {
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
    fn the_linear_key_only_travels_over_https() {
        let plain = Config {
            linear_url: "http://api.linear.app/graphql".into(),
            ..defaults()
        };
        assert!(plain.validate().is_err());
    }
}
