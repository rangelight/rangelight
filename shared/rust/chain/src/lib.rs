//! The per-session event chain (ADR 0029).
//!
//! An event's payload is the deterministic protobuf encoding of its
//! `rangelight.session.v1alpha1.Change` ([`payload`]). Its hash covers the canonical record of the
//! event, chained to the one before ([`link`]):
//!
//! ```text
//! hash = SHA-256( prev_hash   32 bytes: the previous event's hash, or GENESIS (zeros)
//!               ‖ event_id    16 bytes: the UUID, in its RFC 9562 byte order
//!               ‖ at           8 bytes: i64 big-endian, microseconds since 1970-01-01T00:00:00Z
//!               ‖ payload     the rest: the encoded Change )
//! ```
//!
//! Every field but the last has a fixed length, so the concatenation is unambiguous. `at` is
//! hashed at microsecond precision, Postgres's `timestamptz` precision, and stored as exactly that
//! value ([`micros`], [`time`]). This encoding is fixed: changing it breaks every stored chain.
//! `rl-store` computes the links as it appends; [`verify`] recomputes a whole chain.
#![deny(missing_docs)]

use buffa::Message;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use buffa_types::google::protobuf::Timestamp;
use rl_domain::{AgentEvent, Change, EventId, Phase, State};
use rl_proto::rangelight::runner::v1alpha1::{DesiredState, Phase as ProtoPhase};
use rl_proto::rangelight::session::v1alpha1 as proto;
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::agent_event::Kind as AgentKind;
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use sha2::{Digest, Sha256};

/// The `prev_hash` of a session's first event.
pub const GENESIS: [u8; 32] = [0; 32];

/// A chain doesn't verify.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChainError {
    /// The event at this index doesn't point at the hash before it.
    #[error("event {0} doesn't link to the event before it")]
    Unlinked(usize),
    /// The event at this index doesn't hash to its stored hash: its id, time, payload or hash
    /// changed.
    #[error("event {0} doesn't match its hash")]
    Tampered(usize),
    /// A payload isn't a `Change`.
    #[error("payload isn't a session change: {0}")]
    Payload(String),
}

/// What an event's hash covers, besides the previous hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record<'a> {
    /// The event's id.
    pub id: EventId,
    /// When it happened, in microseconds since the Unix epoch ([`micros`]).
    pub at: i64,
    /// The canonical payload ([`payload`]).
    pub payload: &'a [u8],
}

/// One stored link of a chain, in order.
#[derive(Debug, Clone, Copy)]
pub struct Link<'a> {
    /// The previous event's hash.
    pub prev_hash: &'a [u8],
    /// The event.
    pub record: Record<'a>,
    /// The stored hash.
    pub hash: &'a [u8],
}

/// A time at the chain's precision: whole microseconds since the Unix epoch, UTC, rounded
/// toward the past; saturates beyond ±292,000 years.
#[must_use]
pub fn micros(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_micros()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_micros()).unwrap_or(i64::MAX);
            let partial = i64::from(before.subsec_nanos() % 1000 != 0);
            whole.saturating_add(partial).saturating_neg()
        }
    }
}

/// The time [`micros`] stands for.
#[must_use]
pub fn time(micros: i64) -> SystemTime {
    let offset = Duration::from_micros(micros.unsigned_abs());
    let at = if micros >= 0 {
        UNIX_EPOCH.checked_add(offset)
    } else {
        UNIX_EPOCH.checked_sub(offset)
    };
    // Every i64 of microseconds fits a 64-bit SystemTime.
    at.unwrap_or(UNIX_EPOCH)
}

/// The runner protocol's name for a state.
#[must_use]
pub fn desired_state(state: State) -> DesiredState {
    match state {
        State::Hot => DesiredState::DESIRED_STATE_HOT,
        State::Warm => DesiredState::DESIRED_STATE_WARM,
        State::Cold => DesiredState::DESIRED_STATE_COLD,
    }
}

/// The runner protocol's name for a phase.
#[must_use]
pub fn proto_phase(phase: Phase) -> ProtoPhase {
    match phase {
        Phase::Queued => ProtoPhase::PHASE_QUEUED,
        Phase::Starting => ProtoPhase::PHASE_STARTING,
        Phase::Running => ProtoPhase::PHASE_RUNNING,
        Phase::Hibernating => ProtoPhase::PHASE_HIBERNATING,
        Phase::Hibernated => ProtoPhase::PHASE_HIBERNATED,
        Phase::Waking => ProtoPhase::PHASE_WAKING,
        Phase::Terminating => ProtoPhase::PHASE_TERMINATING,
        Phase::Gone => ProtoPhase::PHASE_GONE,
        Phase::Failed => ProtoPhase::PHASE_FAILED,
    }
}

/// The phase a runner reported, or `None` for `PHASE_UNSPECIFIED`.
#[must_use]
pub fn phase(reported: ProtoPhase) -> Option<Phase> {
    match reported {
        ProtoPhase::PHASE_UNSPECIFIED => None,
        ProtoPhase::PHASE_QUEUED => Some(Phase::Queued),
        ProtoPhase::PHASE_STARTING => Some(Phase::Starting),
        ProtoPhase::PHASE_RUNNING => Some(Phase::Running),
        ProtoPhase::PHASE_HIBERNATING => Some(Phase::Hibernating),
        ProtoPhase::PHASE_HIBERNATED => Some(Phase::Hibernated),
        ProtoPhase::PHASE_WAKING => Some(Phase::Waking),
        ProtoPhase::PHASE_TERMINATING => Some(Phase::Terminating),
        ProtoPhase::PHASE_GONE => Some(Phase::Gone),
        ProtoPhase::PHASE_FAILED => Some(Phase::Failed),
    }
}

/// The change as its protobuf message.
#[must_use]
pub fn to_proto(change: &Change) -> proto::Change {
    let kind = match change {
        Change::Created {
            ticket_id,
            ticket_key,
            title,
            ticket_url,
            repo,
            namespace,
            state,
        } => Kind::from(proto::Created {
            ticket_id: ticket_id.clone(),
            ticket_key: ticket_key.clone(),
            title: title.clone(),
            ticket_url: ticket_url.clone(),
            repo: repo.clone(),
            namespace: namespace.clone(),
            state: desired_state(*state).into(),
            ..Default::default()
        }),
        Change::StateChanged { state } => Kind::from(proto::StateChanged {
            state: desired_state(*state).into(),
            ..Default::default()
        }),
        Change::PhaseChanged {
            phase,
            message,
            observed_generation,
            message_id,
            sent_at,
            pull_request_url,
        } => Kind::from(proto::PhaseChanged {
            phase: proto_phase(*phase).into(),
            message: message.clone(),
            observed_generation: *observed_generation,
            message_id: message_id.to_string(),
            sent_at: sent_at.map(Timestamp::from).into(),
            pull_request_url: pull_request_url.clone().unwrap_or_default(),
            ..Default::default()
        }),
        Change::AgentEvent {
            message_id,
            sent_at,
            event,
        } => Kind::from(proto::AgentEvent {
            message_id: message_id.to_string(),
            sent_at: Some(Timestamp::from(*sent_at)).into(),
            kind: Some(agent_kind(event)),
            ..Default::default()
        }),
    };
    proto::Change {
        kind: Some(kind),
        ..Default::default()
    }
}

fn agent_kind(event: &AgentEvent) -> AgentKind {
    match event {
        AgentEvent::Ask { question, options } => AgentKind::from(proto::Ask {
            question: question.clone(),
            options: options.clone(),
            ..Default::default()
        }),
        AgentEvent::Progress { stage, note } => AgentKind::from(proto::Progress {
            stage: stage.clone(),
            note: note.clone(),
            ..Default::default()
        }),
        AgentEvent::WorkerDone { summary, head_sha } => AgentKind::from(proto::WorkerDone {
            summary: summary.clone(),
            head_sha: head_sha.clone(),
            ..Default::default()
        }),
        AgentEvent::Compacted { trigger, summary } => AgentKind::from(proto::Compacted {
            trigger: trigger.clone(),
            summary: summary.clone(),
            ..Default::default()
        }),
    }
}

/// The canonical payload: the deterministic encoding of [`to_proto`]. Fields are written in
/// field-number order and the message has no maps, so equal changes give equal bytes.
#[must_use]
pub fn payload(change: &Change) -> Vec<u8> {
    to_proto(change).encode_to_vec()
}

/// Decodes a stored payload.
///
/// # Errors
/// [`ChainError::Payload`] if it isn't a `Change`.
pub fn decode(payload: &[u8]) -> Result<proto::Change, ChainError> {
    proto::Change::decode_from_slice(payload).map_err(|e| ChainError::Payload(e.to_string()))
}

/// The hash of `record` chained to `prev_hash`, in the canonical encoding above.
#[must_use]
pub fn link(prev_hash: &[u8], record: &Record<'_>) -> [u8; 32] {
    Sha256::new()
        .chain_update(prev_hash)
        .chain_update(record.id.0.as_bytes())
        .chain_update(record.at.to_be_bytes())
        .chain_update(record.payload)
        .finalize()
        .into()
}

/// Verifies a session's whole chain, in order: the first event follows [`GENESIS`], each event
/// points at the hash before it, and each hash is [`link`] of its own fields.
///
/// # Errors
/// The first [`ChainError`] found.
pub fn verify<'a>(chain: impl IntoIterator<Item = Link<'a>>) -> Result<(), ChainError> {
    let mut expected_prev: &[u8] = &GENESIS;
    for (index, event) in chain.into_iter().enumerate() {
        if event.prev_hash != expected_prev {
            return Err(ChainError::Unlinked(index));
        }
        if link(event.prev_hash, &event.record) != event.hash {
            return Err(ChainError::Tampered(index));
        }
        expected_prev = event.hash;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
