//! The relay (ADR 0031): publishes each committed event in the outbox to NATS, then marks it
//! sent. Two messages per event:
//!
//! - for an event that sets the desired state, the session's whole `SessionDesired` (its state,
//!   ticket and workspace), in an `Envelope`, to JetStream on
//!   `rl.<org>.runner.<runner>.desired`, with `Nats-Msg-Id` = the event id. The row is marked
//!   sent only after JetStream acknowledges, so a crash in between republishes; a quick restart
//!   falls inside JetStream's duplicate window, and consumers dedupe on the event id and
//!   `generation` for anything later;
//! - the `Event`, on the core NATS subject `rl.<org>.session.<id>.events`, which the api fans
//!   out to `WatchSession` streams. Watchers dedupe on the event id.
#![deny(missing_docs)]

pub mod config;
mod reseed;

use std::time::Duration;

use async_nats::jetstream::{self, context::PublishErrorKind, message::PublishMessage, stream};
use buffa::EnumValue;
use buffa::Message;
use rl_domain::subject;
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::{
    DesiredState, Envelope, SessionDesired, Ticket, Worktree,
};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_proto::rangelight::session::v1alpha1::{Change, Event};
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
    /// A relay; creates the org's desired-state stream, or updates it to this config (say, a
    /// changed `replicas`). It keeps one message per subject, the session's latest desired state,
    /// so a runner that starts over reads only those, and a cold one is a closed session's
    /// tombstone. `org` and `runner` are checked subject tokens (`rl_domain::subject::token`).
    ///
    /// # Errors
    /// [`RelayError::Nats`] if the stream can't be created or updated.
    pub async fn new(
        store: Store,
        nats: async_nats::Client,
        org: String,
        runner: String,
        replicas: usize,
    ) -> Result<Relay, RelayError> {
        let jetstream = jetstream::new(nats.clone());
        jetstream
            .create_or_update_stream(stream::Config {
                name: subject::desired_stream(&org),
                subjects: vec![subject::runners_desired(&org)],
                // No max_age: a hot session's desired state stays however long it runs.
                // ponytail: a closed session's tombstone stays for ever, one small message each.
                // Ceiling: stream size at millions of sessions. Upgrade: purge a subject once the
                // runner reported the session gone.
                max_messages_per_subject: 1,
                retention: stream::RetentionPolicy::Limits,
                num_replicas: replicas,
                duplicate_window: DUPLICATE_WINDOW,
                ..Default::default()
            })
            .await
            .map_err(|e| RelayError::Nats(e.to_string()))?;
        let relay = Relay {
            store,
            nats,
            jetstream,
            org,
            runner,
        };
        relay.reseed().await?;
        Ok(relay)
    }

    /// Publishes one event: for a change of the desired state, JetStream first, waiting for its
    /// acknowledgement; then the fan-out. A runner's own report goes only to the fan-out. Doesn't
    /// mark it sent.
    ///
    /// # Errors
    /// [`RelayError::Payload`] or [`RelayError::Nats`].
    #[tracing::instrument(skip_all, fields(session_id = %event.session.0, event_id = %event.event.0, seq = event.seq))]
    pub async fn publish(&self, event: &Unsent) -> Result<(), RelayError> {
        let (change, state) = decode(event)?;
        if let Some(state) = state {
            self.send_desired(event, state, false).await?;
        }
        let fanout = Event {
            id: event.event.0.to_string(),
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
            .map_err(|e| RelayError::Nats(e.to_string()))
    }

    /// Publishes `event`'s desired state to JetStream and waits for the acknowledgement; `false`
    /// when `if_empty` and its session's subject already holds a message (nothing written).
    /// Otherwise `Nats-Msg-Id` is the event id.
    pub(crate) async fn send_desired(
        &self,
        event: &Unsent,
        state: EnumValue<DesiredState>,
        if_empty: bool,
    ) -> Result<bool, RelayError> {
        let nats = |e: &dyn std::fmt::Display| RelayError::Nats(e.to_string());
        let publish =
            PublishMessage::build().payload(self.desired(event, state).encode_to_vec().into());
        // Put back only onto an empty subject, and without the event's id: JetStream would take a
        // republish inside its duplicate window as a repeat and store nothing.
        let publish = if if_empty {
            publish.expected_last_subject_sequence(0)
        } else {
            publish.message_id(event.event.0.to_string().as_str())
        };
        let subject = subject::runner_desired(&self.org, &self.runner, event.session);
        let ack = self
            .jetstream
            .send_publish(subject, publish)
            .await
            .map_err(|e| nats(&e))?;
        match ack.await {
            Ok(_) => Ok(true),
            Err(e) if if_empty && e.kind() == PublishErrorKind::WrongLastSequence => Ok(false),
            Err(e) => Err(nats(&e)),
        }
    }

    /// The whole desired session as of `event`, in its envelope: what the runner's `Session`
    /// resource holds.
    fn desired(&self, event: &Unsent, state: EnumValue<DesiredState>) -> Envelope {
        let desired = SessionDesired {
            session_id: event.session.0.to_string(),
            generation: event.seq,
            state,
            namespace: event.namespace.clone(),
            worktrees: vec![Worktree {
                base_ref: event.base_ref.clone(),
                mirror_repo: event.repo.clone(),
                ..Default::default()
            }],
            ticket: Ticket {
                key: event.ticket_key.clone(),
                title: event.title.clone(),
                description: event.description.clone(),
                criteria: event.criteria.clone(),
                url: event.ticket_url.clone(),
                ..Default::default()
            }
            .into(),
            member: event.member.clone(),
            ..Default::default()
        };
        Envelope {
            message_id: event.event.0.to_string(),
            protocol_version: PROTOCOL.into(),
            runner_id: self.runner.clone(),
            // The event's time, so a republish is byte-for-byte the same message.
            sent_at: buffa_types::google::protobuf::Timestamp::from(event.at).into(),
            body: Some(Body::from(desired)),
            ..Default::default()
        }
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

/// `event`'s change, and the desired state it sets (`None` for a runner's own report).
fn decode(event: &Unsent) -> Result<(Change, Option<EnumValue<DesiredState>>), RelayError> {
    let change =
        rl_chain::decode(&event.payload).map_err(|_| RelayError::Payload(event.event.0))?;
    let state = match &change.kind {
        Some(Kind::Created(created)) => Some(created.state),
        Some(Kind::StateChanged(changed)) => Some(changed.state),
        Some(Kind::PhaseChanged(_) | Kind::AgentEvent(_)) => None,
        None => return Err(RelayError::Payload(event.event.0)),
    };
    Ok((change, state))
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

/// Answers `GET /healthz` with 200 on `listener` until `cancel`. `main` serves it only once
/// [`Relay::new`] has its Postgres, NATS and stream: a 200 means the relay started and lives.
///
/// # Errors
/// An I/O error from the listener.
pub async fn serve_health(
    listener: tokio::net::TcpListener,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let app = axum::Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));
    axum::serve(listener, app)
        .with_graceful_shutdown(cancel.cancelled_owned())
        .await
}

#[cfg(test)]
mod tests;
