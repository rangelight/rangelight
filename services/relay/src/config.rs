//! The relay's config (ADR 0049 §6), loaded by `rl-config` from `configs/relay.default.toml`.

use std::net::SocketAddr;

use rl_config::{ConfigError, SecretRef};
use rl_domain::subject;
use serde::Deserialize;

/// What the relay is configured with.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// `tracing` filter directives.
    pub log: String,
    /// The `/healthz` listener.
    pub listen: SocketAddr,
    /// The Postgres URL, in a secret.
    pub database_url: SecretRef,
    /// The NATS server.
    pub nats_url: String,
    /// The org's subject token.
    pub org: String,
    /// The runner sessions go to.
    pub runner: String,
    /// JetStream replicas of the desired-state stream.
    pub stream_replicas: usize,
}

impl Config {
    /// Loads and validates the config.
    ///
    /// # Errors
    /// [`ConfigError`] for a missing or invalid value.
    pub fn load() -> Result<Config, ConfigError> {
        let config: Config =
            rl_config::load("relay", include_str!("../../../configs/relay.default.toml"))?;
        config.validate()
    }

    fn validate(self) -> Result<Config, ConfigError> {
        for token in [&self.org, &self.runner] {
            subject::token(token).map_err(|e| ConfigError(e.to_string()))?;
        }
        if !matches!(self.stream_replicas, 1..=5) {
            return Err(ConfigError("stream_replicas must be 1 to 5".into()));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn defaults() -> Config {
        rl_config::load("relay", include_str!("../../../configs/relay.default.toml")).unwrap()
    }

    #[test]
    fn defaults_are_valid() {
        defaults().validate().unwrap();
    }

    #[test]
    fn subject_tokens_and_replicas_are_checked() {
        let bad_org = Config {
            org: "a.>".into(),
            ..defaults()
        };
        assert!(bad_org.validate().is_err());
        let bad_runner = Config {
            runner: String::new(),
            ..defaults()
        };
        assert!(bad_runner.validate().is_err());
        let bad_replicas = Config {
            stream_replicas: 0,
            ..defaults()
        };
        assert!(bad_replicas.validate().is_err());
    }
}
