//! NATS subject names (ADR 0021, ADR 0051: every subject starts with `rl.`).

use crate::{DomainError, SessionId};

/// Checks one subject token (an org or runner id): not empty, and no `.`, `*`, `>` or
/// whitespace, so it can't widen or split a subject.
///
/// # Errors
/// [`DomainError::InvalidToken`] otherwise.
pub fn token(value: &str) -> Result<&str, DomainError> {
    let bad = |c: char| matches!(c, '.' | '*' | '>') || c.is_whitespace() || c.is_control();
    if value.is_empty() || value.chars().any(bad) {
        return Err(DomainError::InvalidToken(value.to_owned()));
    }
    Ok(value)
}

/// The JetStream subject a runner reads its desired sessions from (api/README.md).
#[must_use]
pub fn runner_desired(org: &str, runner: &str) -> String {
    format!("rl.{org}.runner.{runner}.desired")
}

/// The subjects of every runner's desired sessions in an org: what the stream captures.
#[must_use]
pub fn runners_desired(org: &str) -> String {
    format!("rl.{org}.runner.*.desired")
}

/// The core NATS subject a session's committed events fan out on (ADR 0021, ADR 0033).
#[must_use]
pub fn session_events(org: &str, session: SessionId) -> String {
    format!("rl.{org}.session.{}.events", session.0)
}
