//! The relay (ADR 0031): publishes each committed event in the outbox to NATS, then marks it
//! sent. Two messages per event:
//!
//! - the session's `SessionDesired`, in an `Envelope`, to JetStream on
//!   `rl.<org>.runner.<runner>.desired`, with `Nats-Msg-Id` = the event id. The row is marked
//!   sent only after JetStream acknowledges, so a crash in between republishes; a quick restart
//!   falls inside JetStream's duplicate window, and consumers dedupe on the event id and
//!   `generation` for anything later;
//! - the `Event`, on the core NATS subject `rl.<org>.session.<id>.events`, which the api fans
//!   out to `WatchSession` streams. Watchers dedupe on the event id.
#![deny(missing_docs)]

pub mod config;

use std::time::Duration;

use async_nats::jetstream::{self, message::PublishMessage, stream};
use buffa::Message;
use rl_domain::subject;
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::{Envelope, SessionDesired};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_proto::rangelight::session::v1alpha1::Event;
use rl_store::{Store, StoreError, Unsent};
use tokio_util::sync::CancellationToken;

/// The runner protocol version the relay speaks.
const PROTOCOL: &str = "v1alpha1";
/// Events published per round.
const BATCH: u16 = 100;
/// The pause between rounds that found nothing, and the first pause after a failed round.
const IDLE: Duration = Duration::from_millis(500);
/// The longest pause after failed rounds: each failure doubles the pause up to this.
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// How long JetStream drops a repeated `Nats-Msg-Id`. It covers a quick crash and restart, not
/// every outage: consumers still dedupe on the event id and `generation`.
const DUPLICATE_WINDOW: Duration = Duration::from_secs(120);
/// How long a desired state stays in the stream for a runner that is away.
const MAX_AGE: Duration = Duration::from_hours(7 * 24);

/// Why a relay round failed. The rows it didn't mark stay unsent and go out next round.
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    /// Postgres failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// NATS refused or didn't acknowledge.
    #[error("nats: {0}")]
    Nats(String),
    /// A stored payload isn't a session change this relay knows; the row is set aside.
    #[error("event {0}: payload isn't a session change")]
    Payload(uuid::Uuid),
}

/// Publishes the outbox of one org to one runner.
#[derive(Debug, Clone)]
pub struct Relay {
    store: Store,
    nats: async_nats::Client,
    jetstream: jetstream::Context,
    org: String,
    runner: String,
}

impl Relay {
    /// A relay; creates the org's desired-state stream if it doesn't exist. `org` and `runner`
    /// are checked subject tokens (`rl_domain::subject::token`).
    ///
    /// # Errors
    /// [`RelayError::Nats`] if the stream can't be created.
    pub async fn new(
        store: Store,
        nats: async_nats::Client,
        org: String,
        runner: String,
        replicas: usize,
    ) -> Result<Relay, RelayError> {
        let jetstream = jetstream::new(nats.clone());
        jetstream
            .get_or_create_stream(stream::Config {
                name: format!("rl-{org}-desired"),
                subjects: vec![subject::runners_desired(&org)],
                retention: stream::RetentionPolicy::Limits,
                max_age: MAX_AGE,
                num_replicas: replicas,
                duplicate_window: DUPLICATE_WINDOW,
                ..Default::default()
            })
            .await
            .map_err(|e| RelayError::Nats(e.to_string()))?;
        Ok(Relay {
            store,
            nats,
            jetstream,
            org,
            runner,
        })
    }

    /// Publishes one event: JetStream first, waiting for its acknowledgement, then the fan-out.
    /// Doesn't mark it sent.
    ///
    /// # Errors
    /// [`RelayError::Payload`] or [`RelayError::Nats`].
    #[tracing::instrument(skip_all, fields(session_id = %event.session.0, event_id = %event.event.0, seq = event.seq))]
    pub async fn publish(&self, event: &Unsent) -> Result<(), RelayError> {
        let change =
            rl_chain::decode(&event.payload).map_err(|_| RelayError::Payload(event.event.0))?;
        let state = match &change.kind {
            Some(Kind::Created(created)) => created.state,
            Some(Kind::StateChanged(changed)) => changed.state,
            None => return Err(RelayError::Payload(event.event.0)),
        };
        let id = event.event.0.to_string();
        let desired = SessionDesired {
            session_id: event.session.0.to_string(),
            generation: event.seq,
            state,
            namespace: event.namespace.clone(),
            ..Default::default()
        };
        let envelope = Envelope {
            message_id: id.clone(),
            protocol_version: PROTOCOL.into(),
            runner_id: self.runner.clone(),
            // The event's time, so a republish is byte-for-byte the same message.
            sent_at: buffa_types::google::protobuf::Timestamp::from(event.at).into(),
            body: Some(Body::from(desired)),
            ..Default::default()
        };
        let publish = PublishMessage::build()
            .message_id(id.as_str())
            .payload(envelope.encode_to_vec().into());
        let nats = |e: &dyn std::fmt::Display| RelayError::Nats(e.to_string());
        let subject = subject::runner_desired(&self.org, &self.runner);
        let ack = self
            .jetstream
            .send_publish(subject, publish)
            .await
            .map_err(|e| nats(&e))?;
        ack.await.map_err(|e| nats(&e))?;

        let fanout = Event {
            id,
            session_id: event.session.0.to_string(),
            seq: event.seq,
            at: buffa_types::google::protobuf::Timestamp::from(event.at).into(),
            change: change.into(),
            ..Default::default()
        };
        self.nats
            .publish(
                subject::session_events(&self.org, event.session),
                fanout.encode_to_vec().into(),
            )
            .await
            .map_err(|e| nats(&e))
    }

    /// One round: publishes the oldest unsent events in order and marks each sent after its
    /// acknowledgement. An event whose payload this relay can't decode (say, a newer release's
    /// change during a rolling deploy) is set aside with `failed_at` and logged, and only its own
    /// session's later events wait behind it. Any other failure ends the round. Returns how many
    /// it sent.
    ///
    /// # Errors
    /// The first [`RelayError`] other than [`RelayError::Payload`].
    pub async fn run_once(&self) -> Result<usize, RelayError> {
        let batch = self.store.unsent(BATCH).await?;
        let mut sent = 0;
        for event in &batch {
            match self.publish(event).await {
                Ok(()) => {
                    self.store.mark_sent(event.outbox_id).await?;
                    sent += 1;
                }
                Err(error @ RelayError::Payload(_)) => {
                    tracing::error!(%error, session_id = %event.session.0, "set aside an event; its session waits");
                    self.store.mark_failed(event.outbox_id).await?;
                }
                Err(error) => return Err(error),
            }
        }
        self.nats
            .flush()
            .await
            .map_err(|e| RelayError::Nats(e.to_string()))?;
        Ok(sent)
    }

    /// Runs rounds until `cancel`, pausing as [`next_pause`] says.
    pub async fn run(&self, cancel: CancellationToken) {
        let mut pause = Duration::ZERO;
        while !cancel.is_cancelled() {
            let round = self.run_once().await;
            if let Err(error) = &round {
                tracing::warn!(%error, "relay round failed");
            }
            pause = next_pause(pause, &round);
            tokio::select! {
                () = cancel.cancelled() => {}
                () = tokio::time::sleep(pause) => {}
            }
        }
    }
}

/// The pause after a round: none after a full batch (more is waiting), [`IDLE`] after a partial
/// one, and after a failure double the previous pause, from [`IDLE`] up to [`MAX_BACKOFF`].
#[must_use]
pub fn next_pause(previous: Duration, round: &Result<usize, RelayError>) -> Duration {
    match round {
        Ok(sent) if *sent == usize::from(BATCH) => Duration::ZERO,
        Ok(_) => IDLE,
        Err(_) => previous.saturating_mul(2).clamp(IDLE, MAX_BACKOFF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_rounds_back_off_to_a_cap_and_success_resets() {
        let failed = Err(RelayError::Nats("down".into()));
        let mut pause = Duration::ZERO;
        let mut pauses = Vec::new();
        for _ in 0..9 {
            pause = next_pause(pause, &failed);
            pauses.push(pause.as_millis());
        }
        assert_eq!(
            pauses,
            [500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
        );
        assert_eq!(next_pause(pause, &Ok(3)), IDLE);
        assert_eq!(next_pause(pause, &Ok(usize::from(BATCH))), Duration::ZERO);
    }
}
