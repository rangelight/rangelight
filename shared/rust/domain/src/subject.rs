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

/// The JetStream subject of one session's desired state at its runner (api/README.md). The stream
/// keeps one message per subject, so it holds each session's latest desired state, and a cold one
/// is its tombstone.
#[must_use]
pub fn runner_desired(org: &str, runner: &str, session: SessionId) -> String {
    format!("rl.{org}.runner.{runner}.desired.{}", session.0)
}

/// The subjects of every session's desired state at every runner in an org: what the stream
/// captures.
#[must_use]
pub fn runners_desired(org: &str) -> String {
    format!("rl.{org}.runner.*.desired.*")
}

/// The subjects of every session's committed events in an org: what `WatchSessions` follows.
#[must_use]
pub fn sessions_events(org: &str) -> String {
    format!("rl.{org}.session.*.events")
}

/// The core NATS subject a session's committed events fan out on (ADR 0021, ADR 0033).
#[must_use]
pub fn session_events(org: &str, session: SessionId) -> String {
    format!("rl.{org}.session.{}.events", session.0)
}

/// The JetStream subject a runner reports its sessions' status on (api/README.md).
#[must_use]
pub fn runner_status(org: &str, runner: &str) -> String {
    format!("rl.{org}.runner.{runner}.status")
}

/// The subjects of every runner's status in an org: what the status stream captures.
#[must_use]
pub fn runners_status(org: &str) -> String {
    format!("rl.{org}.runner.*.status")
}
