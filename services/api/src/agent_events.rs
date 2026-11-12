//! The lanes' agent events (ADR 0045, RAN-63): a durable JetStream consumer of every lane's
//! `rl.<org>.agent.<session>.events`. Each event is held to
//! `api/jsonschema/agent-event.v1alpha1.schema.json` and its size, and becomes one chained event
//! with its effect on the session's question in one transaction (`Store::report_agent_event`).
//!
//! The session is the subject's, which the lane's NATS permissions pin to its own: never the
//! payload's (a payload naming another session is dropped). An event that breaks the schema
//! (or holds a control character, which Postgres text can't), is larger than [`MAX_BYTES`], comes
//! past a session's [`RATE`], names no session or a session that isn't hot, or that Postgres
//! refuses, is dropped and logged with the session id only: its contents come from the model,
//! untrusted. Only a store that is away is waited for: its event is redelivered, never dropped.
//!
//! The stream is a work queue: an acknowledged event leaves it. It holds at most [`STREAM_BYTES`]
//! for all lanes (the oldest go first) and [`PER_SESSION`] a session, so lanes can't fill the
//! JetStream volume the runners' status and the desired state share.
//! ponytail: no NATS-level publish rate (NATS has none per user). Ceiling: a lane can spend the
//! stream's write bandwidth up to those bounds. Upgrade: a per-lane bandwidth limit when NATS has one.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use async_nats::jetstream::{self, AckKind, stream};
use rl_app::{Clock, IdGen, SystemClock, UuidV7};
use rl_domain::{AgentEvent, EventId, ReportAgentEvent, SessionId, subject};
use rl_store::{AgentEventRecorded, Store, StoreError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::status::{Handle, RECORD_TIMEOUT, RETRY, durable};

/// The largest event kept, in bytes: the largest valid one (a compaction's 8000-character
/// summary, JSON-escaped) fits.
pub const MAX_BYTES: usize = 64 * 1024;
/// The most events one session sends per [`WINDOW`]; past it they are dropped.
pub const RATE: u32 = 30;
/// The window [`RATE`] counts in.
pub const WINDOW: Duration = Duration::from_mins(1);
/// How long an event waits in the stream while the api is away.
const MAX_AGE: Duration = Duration::from_hours(24);
/// The most events the stream holds for one session; older ones go first.
pub const PER_SESSION: i64 = 100;
/// The most bytes the stream holds for all lanes; older events go first.
pub const STREAM_BYTES: i64 = 512 * 1024 * 1024;

/// What an event came to: what the consumer acknowledges.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// One new event.
    Recorded,
    /// Its message was already recorded: nothing written.
    Duplicate,
    /// Dropped (the reason is logged): terminated, never redelivered.
    Dropped(&'static str),
}

/// The envelope (`additionalProperties: false`), with `payload` read by `type`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Envelope {
    message_id: String,
    session_id: String,
    at: String,
    r#type: String,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    question: String,
    options: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkerDone {
    summary: String,
    head_sha: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    stage: String,
    note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compacted {
    trigger: String,
    summary: String,
}

/// 1 to `max` characters (JSON Schema counts code points), and no control character but a
/// newline or a tab: Postgres text can't hold a NUL, and none of them belong in a question.
fn text(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.chars().count())
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

/// The schema's `uuidv7`: lowercase, hyphenated, version 7, RFC 9562 variant.
fn uuidv7(value: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(value).ok()?;
    let canonical = id.hyphenated().to_string() == value;
    (canonical && id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122)
        .then_some(id)
}

fn payload<T: DeserializeOwned>(value: serde_json::Value) -> Option<T> {
    serde_json::from_value(value).ok()
}

/// The event `bytes` holds, held to the schema, or `None`.
fn event(r#type: &str, value: serde_json::Value) -> Option<AgentEvent> {
    let event = match r#type {
        "ask" => {
            let Ask { question, options } = payload(value)?;
            let options = options.unwrap_or_default();
            let unique = options
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len();
            let options_ok = options.is_empty()
                || ((2..=8).contains(&options.len())
                    && unique == options.len()
                    && options.iter().all(|o| text(o, 200)));
            (text(&question, 2000) && options_ok).then_some(AgentEvent::Ask { question, options })
        }
        "worker_done" => {
            let WorkerDone { summary, head_sha } = payload(value)?;
            let sha_ok = head_sha.len() == 40
                && head_sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            (text(&summary, 600) && sha_ok).then_some(AgentEvent::WorkerDone { summary, head_sha })
        }
        "progress" => {
            let Progress { stage, note } = payload(value)?;
            let stage_ok = (1..=40).contains(&stage.len())
                && stage.starts_with(|c: char| c.is_ascii_lowercase())
                && stage
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            (stage_ok && text(&note, 200)).then_some(AgentEvent::Progress { stage, note })
        }
        "compacted" => {
            let Compacted { trigger, summary } = payload(value)?;
            (matches!(trigger.as_str(), "auto" | "manual") && text(&summary, 8000))
                .then_some(AgentEvent::Compacted { trigger, summary })
        }
        _ => None,
    }?;
    Some(event)
}

/// The command an event from `session`'s lane asks for, or why it is dropped.
fn command(
    bytes: &[u8],
    session: SessionId,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<ReportAgentEvent, &'static str> {
    if bytes.len() > MAX_BYTES {
        return Err("too_large");
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| "invalid")?;
    let message_id = uuidv7(&envelope.message_id).ok_or("invalid")?;
    if uuidv7(&envelope.session_id) != Some(session.0) {
        return Err("another_session");
    }
    let sent_at = OffsetDateTime::parse(&envelope.at, &Rfc3339).map_err(|_| "invalid")?;
    let agent = event(&envelope.r#type, envelope.payload).ok_or("invalid")?;
    Ok(ReportAgentEvent {
        session,
        event: EventId(ids.new_id()),
        at: clock.now(),
        message_id,
        sent_at: SystemTime::from(sent_at),
        agent,
    })
}

/// Each session's events in its current [`WINDOW`]: when the window started, and how many.
#[derive(Debug, Default)]
pub struct Rate(HashMap<SessionId, (Instant, u32)>);

impl Rate {
    /// Counts the `delivery`th delivery of an event of `session` at `now`; false past [`RATE`] in
    /// its window. Only a first delivery counts: one redelivered while the store was away was
    /// counted already, so a database blip never makes a lane's events look like a flood.
    /// ponytail: one entry per session that ever sent, for the process's life. Ceiling: memory
    /// at about 40 bytes per session. Upgrade: drop windows older than [`WINDOW`] now and then.
    pub fn allow(&mut self, session: SessionId, now: Instant, delivery: i64) -> bool {
        if delivery > 1 {
            return true;
        }
        let (start, count) = self.0.entry(session).or_insert((now, 0));
        if now.duration_since(*start) >= WINDOW {
            (*start, *count) = (now, 0);
        }
        *count += 1;
        *count <= RATE
    }
}

/// Records one agent event that came on `subject`: its session is the subject's. Its
/// transaction is bounded like a runner report's.
///
/// # Errors
/// [`StoreError`] other than [`StoreError::NotFound`], or `None` on the time limit: worth
/// redelivering.
async fn record(
    (store, rate): (&Store, &mut Rate),
    (subject, delivery): (&str, i64),
    bytes: &[u8],
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<Outcome, Option<StoreError>> {
    // rl.<org>.agent.<session>.events: the stream captures nothing else.
    let Some(session) = subject
        .split('.')
        .nth(3)
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return Ok(Outcome::Dropped("no_session"));
    };
    let session = SessionId(session);
    let dropped = |reason| {
        tracing::warn!(session_id = %session.0, reason, "dropped an agent event");
        Ok(Outcome::Dropped(reason))
    };
    if !rate.allow(session, Instant::now(), delivery) {
        return dropped("rate");
    }
    let command = match command(bytes, session, clock, ids) {
        Ok(command) => command,
        Err(reason) => return dropped(reason),
    };
    let recorded = store.report_agent_event(&command);
    let Ok(recorded) = tokio::time::timeout(RECORD_TIMEOUT, recorded).await else {
        return Err(None);
    };
    match recorded {
        Ok(AgentEventRecorded::New) => Ok(Outcome::Recorded),
        Ok(AgentEventRecorded::Duplicate) => Ok(Outcome::Duplicate),
        Ok(AgentEventRecorded::NotLive) => dropped("session_not_live"),
        Err(StoreError::NotFound) => dropped("unknown_session"),
        // The same statement would fail on every delivery and hold back every lane's events.
        Err(error) if error.is_refused() => dropped("refused_by_store"),
        Err(error) => Err(Some(error)),
    }
}

/// Consumes every lane's agent events in `org` until `cancel`: creates the org's agent stream
/// (with `replicas`; a work queue of at most [`MAX_BYTES`] a message, [`PER_SESSION`] a session and
/// [`STREAM_BYTES`] in all) and the api's durable
/// consumer, then records each event and acknowledges it only after its transaction commits.
pub async fn consume(
    store: Store,
    nats: async_nats::Client,
    org: String,
    replicas: usize,
    cancel: CancellationToken,
) {
    let stream = stream::Config {
        name: subject::agent_stream(&org),
        subjects: vec![subject::agents_events(&org)],
        retention: stream::RetentionPolicy::WorkQueue,
        discard: stream::DiscardPolicy::Old,
        max_age: MAX_AGE,
        max_bytes: STREAM_BYTES,
        max_message_size: i32::try_from(MAX_BYTES).unwrap_or(i32::MAX),
        max_messages_per_subject: PER_SESSION,
        num_replicas: replicas,
        ..Default::default()
    };
    let events = Events {
        store,
        rate: Rate::default(),
    };
    durable(nats, stream, subject::AGENT_CONSUMER, cancel, events).await;
}

/// The consumer's state: the store, and each session's rate.
struct Events {
    store: Store,
    rate: Rate,
}

impl Handle for Events {
    /// Records the event and acknowledges it (see [`ack`]).
    async fn handle(&mut self, message: &jetstream::Message) {
        let at = (&self.store, &mut self.rate);
        let delivery = message.info().map_or(1, |info| info.delivered);
        let recorded = record(
            at,
            (message.subject.as_str(), delivery),
            &message.payload,
            &SystemClock,
            &UuidV7,
        )
        .await;
        if let Err(error) = message.ack_with(ack(recorded)).await {
            tracing::warn!(%error, "couldn't acknowledge an agent event");
        }
    }
}

/// Ack when recorded, a duplicate or dropped; nak (redelivered after 1s) when the store failed.
fn ack(recorded: Result<Outcome, Option<StoreError>>) -> AckKind {
    match recorded {
        Ok(_) => AckKind::Ack,
        Err(error) => {
            let error = error.map_or_else(|| "timed out".to_owned(), |e| e.to_string());
            tracing::warn!(%error, "couldn't record an agent event; redelivering");
            AckKind::Nak(Some(RETRY))
        }
    }
}

#[cfg(test)]
mod tests;
