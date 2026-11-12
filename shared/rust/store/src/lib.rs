//! The only Postgres access (ADR 0049 §2.4): sessions, events, outbox, sign-in, and skipped tickets. Every command
//! writes its state, its chained event and its outbox row in one transaction (ADR 0031). SQL lives in `db/queries/<area>/*.sql`, one
//! module per area, compile-time checked against `.sqlx/`.
#![deny(missing_docs)]

use std::str::FromStr;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use time::OffsetDateTime;

mod auth;
mod events;
mod outbox;
mod sessions;
mod skips;

pub use auth::NewLogin;
pub use events::ChainEvent;
pub use outbox::Unsent;
pub use sessions::{SessionRow, Snapshot};
pub use skips::SkipRow;

/// The schema, from `db/migrations/`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../../db/migrations");

/// Why a store call failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Postgres failed or refused the statement; the transaction was rolled back.
    #[error("database: {0}")]
    Db(sqlx::Error),
    /// A statement ran past `statement_timeout` or waited past `lock_timeout`; the transaction
    /// was rolled back. Worth retrying.
    #[error("database timeout: {0}")]
    Timeout(sqlx::Error),
    /// The migrations didn't apply.
    #[error("migrations: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// No session has that id.
    #[error("session not found")]
    NotFound,
    /// The command breaks a domain rule, for example a state change the state machine forbids.
    #[error(transparent)]
    Domain(#[from] rl_domain::DomainError),
    /// A stored number is out of range; the row is corrupt.
    #[error("stored {0} out of range")]
    OutOfRange(&'static str),
}

impl From<sqlx::Error> for StoreError {
    fn from(error: sqlx::Error) -> StoreError {
        // 57014 query_canceled (statement_timeout), 55P03 lock_not_available (lock_timeout).
        let code = error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code);
        match code.as_deref() {
            Some("57014" | "55P03") => StoreError::Timeout(error),
            _ => StoreError::Db(error),
        }
    }
}

/// Every statement's bound (ADR 0049 §3: every outbound call has a timeout).
const STATEMENT_TIMEOUT: &str = "5s";
/// How long a command waits for another's row lock before giving up.
const LOCK_TIMEOUT: &str = "2s";

/// A Postgres connection pool.
#[derive(Debug, Clone)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Connects, waiting at most 5 seconds for a connection.
    ///
    /// # Errors
    /// [`StoreError::Db`] if the URL is invalid or Postgres can't be reached.
    pub async fn connect(url: &SecretString) -> Result<Store, StoreError> {
        Store::connect_with(PgConnectOptions::from_str(url.expose_secret())?).await
    }

    /// Connects with these options, waiting at most 5 seconds for a connection. Every session
    /// gets `statement_timeout` 5s and `lock_timeout` 2s.
    ///
    /// # Errors
    /// [`StoreError::Db`] if Postgres can't be reached.
    pub async fn connect_with(options: PgConnectOptions) -> Result<Store, StoreError> {
        let options = options.options([
            ("statement_timeout", STATEMENT_TIMEOUT),
            ("lock_timeout", LOCK_TIMEOUT),
        ]);
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;
        Ok(Store { pool })
    }

    /// Applies the migrations that haven't run yet.
    ///
    /// # Errors
    /// [`StoreError::Migrate`] if one fails.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
}

fn to_u64(value: i64, what: &'static str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::OutOfRange(what))
}

/// A chain time (`rl_chain::micros`) as Postgres stores it: exactly, at microsecond precision.
fn timestamp(micros: i64) -> OffsetDateTime {
    OffsetDateTime::from(rl_chain::time(micros))
}
