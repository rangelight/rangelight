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

/// The JetStream stream of every runner's status in an org; the api owns it.
#[must_use]
pub fn status_stream(org: &str) -> String {
    format!("rl-{org}-status")
}

/// The JetStream stream of every session's desired state in an org; the relay owns it.
#[must_use]
pub fn desired_stream(org: &str) -> String {
    format!("rl-{org}-desired")
}

/// The api's durable consumer of the status stream.
pub const STATUS_CONSUMER: &str = "api";

/// A runner's durable consumer of the desired stream.
#[must_use]
pub fn desired_consumer(runner: &str) -> String {
    format!("runner-{runner}")
}

/// The core NATS subject a session's terminal input goes to its lane on (api/README.md).
#[must_use]
pub fn pty_in(org: &str, session: SessionId) -> String {
    format!("rl.{org}.pty.{}.in", session.0)
}

/// The core NATS subject a lane's terminal output comes back on.
#[must_use]
pub fn pty_out(org: &str, session: SessionId) -> String {
    format!("rl.{org}.pty.{}.out", session.0)
}

/// The inbox prefix of the NATS identity `id`: its replies arrive under `<prefix>.>`, which only
/// it may subscribe to, so no client reads another's replies or pulled messages.
#[must_use]
pub fn inbox(id: &str) -> String {
    format!("_INBOX_{id}")
}

/// What one NATS identity may publish and subscribe to: subjects or NATS wildcards, and nothing
/// else (ADR 0021 amendment). `test/nats-permissions.json` pins every list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permissions {
    /// The subjects it may publish to.
    pub publish: Vec<String>,
    /// The subjects it may subscribe to.
    pub subscribe: Vec<String>,
}

/// The JetStream API a stream's owner calls: create, update (`create_or_update_stream`) and info.
fn owned_stream(stream: &str) -> [String; 3] {
    ["CREATE", "UPDATE", "INFO"].map(|verb| format!("$JS.API.STREAM.{verb}.{stream}"))
}

/// The JetStream API of one durable pull consumer: created only in the filtered form, so its
/// subject names the filter and NATS refuses a body with any other; its info, its next
/// messages, and its acknowledgements.
fn pull_consumer(stream: &str, durable: &str, filter: &str) -> [String; 4] {
    [
        format!("$JS.API.CONSUMER.CREATE.{stream}.{durable}.{filter}"),
        format!("$JS.API.CONSUMER.INFO.{stream}.{durable}"),
        format!("$JS.API.CONSUMER.MSG.NEXT.{stream}.{durable}"),
        format!("$JS.ACK.{stream}.{durable}.>"),
    ]
}

/// The api: consumes every runner's status, follows every session's events, and bridges every
/// lane's terminal.
#[must_use]
pub fn api(org: &str) -> Permissions {
    let stream = status_stream(org);
    let mut publish = owned_stream(&stream).to_vec();
    publish.extend(pull_consumer(
        &stream,
        STATUS_CONSUMER,
        &runners_status(org),
    ));
    publish.push(format!("rl.{org}.pty.*.in"));
    Permissions {
        publish,
        subscribe: vec![
            sessions_events(org),
            format!("rl.{org}.pty.*.out"),
            format!("{}.>", inbox("api")),
        ],
    }
}

/// The relay: owns the desired stream and publishes every session's desired state and events.
#[must_use]
pub fn relay(org: &str) -> Permissions {
    let mut publish = owned_stream(&desired_stream(org)).to_vec();
    publish.extend([runners_desired(org), sessions_events(org)]);
    Permissions {
        publish,
        subscribe: vec![format!("{}.>", inbox("relay"))],
    }
}

/// The runner `runner`: consumes only its own sessions' desired state, through its own durable
/// consumer, and reports only its own status.
#[must_use]
pub fn runner(org: &str, runner: &str) -> Permissions {
    let own = format!("rl.{org}.runner.{runner}.desired.*");
    let mut publish = pull_consumer(&desired_stream(org), &desired_consumer(runner), &own).to_vec();
    publish.push(runner_status(org, runner));
    Permissions {
        publish,
        subscribe: vec![format!("{}.>", inbox(&desired_consumer(runner)))],
    }
}

/// The lane of `session`: its own terminal, nothing else. No inbox: a lane makes no requests.
#[must_use]
pub fn lane(org: &str, session: SessionId) -> Permissions {
    Permissions {
        publish: vec![pty_out(org, session)],
        subscribe: vec![pty_in(org, session)],
    }
}
