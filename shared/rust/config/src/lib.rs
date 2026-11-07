//! The config loader and secret references (ADR 0049 §6). The only module that reads the
//! environment. Each service declares its own `Config` in its `config.rs` and loads it here:
//! the service's `configs/<service>.default.toml` (compiled in), then the mounted file
//! `/etc/rangelight/<service>.toml` if present, then `RANGELIGHT_<SERVICE>_*` variables.
#![deny(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Toml};
use secrecy::SecretString;
use serde::de::DeserializeOwned;

/// Where `secret://<name>#<key>` resolves: the mounted Secret file `<SECRETS>/<name>/<key>`.
pub const SECRETS: &str = "/var/run/rangelight/secrets";

/// The config is missing a value or has one of the wrong type.
#[derive(Debug, thiserror::Error)]
#[error("invalid config: {0}")]
pub struct ConfigError(pub String);

/// Loads a service's config: `defaults` (the service's `configs/<service>.default.toml`), then
/// `/etc/rangelight/<service>.toml` if mounted, then `RANGELIGHT_<SERVICE>_<KEY>` variables.
///
/// # Errors
/// [`ConfigError`] naming the bad key.
pub fn load<T: DeserializeOwned>(service: &str, defaults: &str) -> Result<T, ConfigError> {
    load_from(
        service,
        defaults,
        &Path::new("/etc/rangelight").join(format!("{service}.toml")),
    )
}

fn load_from<T: DeserializeOwned>(
    service: &str,
    defaults: &str,
    file: &Path,
) -> Result<T, ConfigError> {
    let prefix = format!("RANGELIGHT_{}_", service.to_ascii_uppercase());
    Figment::from(Toml::string(defaults))
        .merge(Toml::file(file))
        .merge(Env::prefixed(&prefix))
        .extract()
        .map_err(|e| ConfigError(e.to_string()))
}

/// A reference to a secret, never its value: `secret://<name>#<key>` in config. The value is
/// read at use with [`SecretRef::read`] and held in a [`SecretString`], which never prints.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct SecretRef {
    path: PathBuf,
}

impl SecretRef {
    /// Parses `secret://<name>#<key>`. Name and key are letters, digits, `.`, `_` and `-`, and
    /// neither may be `.` or `..`, so a reference can't leave its Secret's directory.
    ///
    /// # Errors
    /// [`ConfigError`] for anything else.
    pub fn parse(reference: &str) -> Result<SecretRef, ConfigError> {
        let invalid = || ConfigError(format!("{reference:?} is not secret://<name>#<key>"));
        let (name, key) = reference
            .strip_prefix("secret://")
            .and_then(|rest| rest.split_once('#'))
            .ok_or_else(invalid)?;
        let part_ok = |part: &str| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        if !part_ok(name) || !part_ok(key) {
            return Err(invalid());
        }
        Ok(SecretRef {
            path: Path::new(SECRETS).join(name).join(key),
        })
    }

    /// A secret in a file at an explicit path, for tests and local runs.
    #[must_use]
    pub fn file(path: impl Into<PathBuf>) -> SecretRef {
        SecretRef { path: path.into() }
    }

    /// The file the secret is read from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the secret now, without its trailing line break.
    ///
    /// # Errors
    /// [`SecretError`], which names the file but never holds its content.
    pub fn read(&self) -> Result<SecretString, SecretError> {
        let value = std::fs::read_to_string(&self.path).map_err(|source| SecretError::Read {
            path: self.path.clone(),
            source,
        })?;
        let value = value.trim_end_matches(['\r', '\n']);
        if value.is_empty() {
            return Err(SecretError::Empty {
                path: self.path.clone(),
            });
        }
        Ok(SecretString::from(value))
    }
}

impl TryFrom<String> for SecretRef {
    type Error = ConfigError;

    fn try_from(reference: String) -> Result<SecretRef, ConfigError> {
        SecretRef::parse(&reference)
    }
}

/// A secret couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The file is missing or unreadable.
    #[error("can't read secret file {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The file is empty.
    #[error("secret file {} is empty", path.display())]
    Empty {
        /// The file.
        path: PathBuf,
    },
}

#[cfg(test)]
mod tests;
