//! The runners' reports (ADR 0021): a durable JetStream consumer of every runner's
//! `rl.<org>.runner.<runner>.status`. Each `SessionStatus` becomes one chained event in one
//! transaction (`Store::report_phase`), which the relay then fans out to `WatchSession`.

use std::time::{Duration, SystemTime};

use async_nats::jetstream::{self, AckKind, consumer::pull, stream};
use buffa::Message;
use futures_util::StreamExt;
use rl_app::{Clock, IdGen, SystemClock, UuidV7};
use rl_domain::{EventId, ReportPhase, SessionId, subject};
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::Envelope;
use rl_store::{Store, StoreError};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The runner protocol version the api accepts.
const PROTOCOL: &str = "v1alpha1";
/// The durable consumer's name.
const CONSUMER: &str = "api";
/// How long JetStream drops a repeated `Nats-Msg-Id`; Postgres dedupes on `message_id` beyond it.
const DUPLICATE_WINDOW: Duration = Duration::from_secs(120);
/// How long a report waits in the stream while the api is away.
const MAX_AGE: Duration = Duration::from_hours(7 * 24);
/// The pause before a report the store couldn't take is redelivered.
const RETRY: Duration = Duration::from_secs(1);
/// How long a delivered report is the api's before JetStream redelivers it.
const ACK_WAIT: Duration = Duration::from_secs(30);
/// The bound on recording one report, below [`ACK_WAIT`].
const RECORD_TIMEOUT: Duration = Duration::from_secs(20);
/// How many deliveries of one report are worth a warning.
const SLOW_DELIVERIES: i64 = 5;
/// The longest pause between attempts to reach the stream.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// What a report came to: what the consumer acknowledges.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// One new event.
    Recorded,
    /// Its message was already recorded: nothing written.
    Duplicate,
    /// It can never be recorded (the reason is logged): terminated, never redelivered.
    Refused(&'static str),
}

/// The command a status message asks for, or why it can't be one. `runner` is the runner named
/// by the subject it came on.
fn command(
    payload: &[u8],
    runner: &str,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<ReportPhase, &'static str> {
    let envelope = Envelope::decode_from_slice(payload).map_err(|_| "undecodable")?;
    if envelope.protocol_version != PROTOCOL {
        return Err("unsupported_protocol");
    }
    if envelope.runner_id != runner {
        return Err("runner_mismatch");
    }
    let message_id = Uuid::parse_str(&envelope.message_id).map_err(|_| "message_id")?;
    let sent_at = envelope
        .sent_at
        .into_option()
        .and_then(|t| SystemTime::try_from(t).ok());
    let Some(Body::Status(status)) = envelope.body else {
        return Err("not_a_status");
    };
    let session = Uuid::parse_str(&status.session_id).map_err(|_| "session_id")?;
    let phase = status
        .phase
        .as_known()
        .and_then(rl_chain::phase)
        .ok_or("phase")?;
    Ok(ReportPhase {
        session: SessionId(session),
        event: EventId(ids.new_id()),
        at: clock.now(),
        message_id,
        sent_at,
        phase,
        message: status.message,
        observed_generation: status.observed_generation,
    })
}

/// Records one status message from `runner`, its transaction bounded by 20s (below the
/// consumer's 30s ack wait, so a stuck one can't outlive its delivery).
///
/// # Errors
/// [`StoreError`] other than [`StoreError::NotFound`], or `None` on the time limit: worth
/// redelivering.
pub async fn record(
    store: &Store,
    payload: &[u8],
    runner: &str,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<Outcome, Option<StoreError>> {
    let command = match command(payload, runner, clock, ids) {
        Ok(command) => command,
        Err(reason) => return Ok(Outcome::Refused(reason)),
    };
    let Ok(recorded) = tokio::time::timeout(RECORD_TIMEOUT, store.report_phase(&command)).await
    else {
        return Err(None);
    };
    match recorded {
        Ok(true) => Ok(Outcome::Recorded),
        Ok(false) => Ok(Outcome::Duplicate),
        Err(StoreError::NotFound) => Ok(Outcome::Refused("unknown_session")),
        Err(error) => Err(Some(error)),
    }
}

/// Consumes every runner's status in `org` until `cancel`: creates the org's status stream (with
/// `replicas`) and the api's durable consumer, then records each message and acknowledges it
/// only after its transaction commits, with the system clock and UUIDv7 ids. NATS failures are
/// logged and retried, backing off to 30s.
pub async fn consume(
    store: Store,
    nats: async_nats::Client,
    org: String,
    replicas: usize,
    cancel: CancellationToken,
) {
    let jetstream = jetstream::new(nats);
    let mut pause = RETRY;
    while !cancel.is_cancelled() {
        let round = async {
            let mut messages = messages(&jetstream, &org, replicas).await?;
            pause = RETRY;
            while let Some(message) = messages.next().await {
                let message = message.map_err(|e| e.to_string())?;
                handle(&store, &message).await;
            }
            Ok::<(), String>(())
        };
        if let Some(Err(error)) = cancel.run_until_cancelled(round).await {
            tracing::warn!(%error, "status stream unavailable; retrying");
        }
        let _ = cancel.run_until_cancelled(tokio::time::sleep(pause)).await;
        pause = pause.saturating_mul(2).min(MAX_BACKOFF);
    }
}

/// The api's durable consumer of the org's status stream, the stream created or updated to this
/// config (say, a changed `replicas`). One message in flight: a session's reports are recorded in the order the runner sent them.
/// ponytail: one in flight for all sessions. Ceiling: report throughput at one transaction at a
/// time. Upgrade: a consumer per runner, or per-session ordering keyed on the subject.
async fn messages(
    jetstream: &jetstream::Context,
    org: &str,
    replicas: usize,
) -> Result<pull::Stream, String> {
    let name = format!("rl-{org}-status");
    jetstream
        .create_or_update_stream(stream::Config {
            name: name.clone(),
            subjects: vec![subject::runners_status(org)],
            max_age: MAX_AGE,
            num_replicas: replicas,
            duplicate_window: DUPLICATE_WINDOW,
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let stream = jetstream
        .get_stream(name)
        .await
        .map_err(|e| e.to_string())?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER,
            pull::Config {
                durable_name: Some(CONSUMER.into()),
                ack_wait: ACK_WAIT,
                max_ack_pending: 1,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    consumer.messages().await.map_err(|e| e.to_string())
}

/// Records one message and acknowledges it: ack when recorded or a duplicate, term when it can
/// never be recorded, nak (redelivered after [`RETRY`]) when the store failed.
#[tracing::instrument(skip_all, fields(subject = %message.subject))]
async fn handle(store: &Store, message: &jetstream::Message) {
    // rl.<org>.runner.<runner>.status: the stream captures nothing else.
    let runner = message.subject.split('.').nth(3).unwrap_or_default();
    if let Ok(info) = message.info()
        && info.delivered > SLOW_DELIVERIES
    {
        let session = match Envelope::decode_from_slice(&message.payload).map(|e| e.body) {
            Ok(Some(Body::Status(status))) => status.session_id,
            _ => String::new(),
        };
        tracing::warn!(delivered = info.delivered, session_id = %session, "a status message keeps failing");
    }
    let ack = match record(store, &message.payload, runner, &SystemClock, &UuidV7).await {
        Ok(Outcome::Recorded | Outcome::Duplicate) => AckKind::Ack,
        Ok(Outcome::Refused(reason)) => {
            tracing::warn!(reason, "refused a status message");
            AckKind::Term
        }
        Err(error) => {
            let error = error.map_or_else(|| "timed out".to_owned(), |e| e.to_string());
            tracing::warn!(%error, "couldn't record a status message; redelivering");
            AckKind::Nak(Some(RETRY))
        }
    };
    if let Err(error) = message.ack_with(ack).await {
        tracing::warn!(%error, "couldn't acknowledge a status message");
    }
}

#[cfg(test)]
mod tests;
