use std::fmt;
use std::time::SystemTime;

use uuid::Uuid;

use crate::{DomainError, Ticket, Workspace};

/// A session's id, a UUIDv7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub Uuid);

/// An event's id, a UUIDv7. It is also the `Nats-Msg-Id` of every message published for the
/// event, so consumers dedupe on it (ADR 0031).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(pub Uuid);

/// Where a session's runtime should be (ADR 0007): `Hot` runs pods, `Warm` keeps the namespace
/// with zero pods, `Cold` keeps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Pods running.
    Hot,
    /// Hibernated: namespace kept, zero pods, rebuildable (ADR 0010).
    Warm,
    /// Torn down. Final.
    Cold,
}

impl State {
    /// The stored name: `hot`, `warm` or `cold`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            State::Hot => "hot",
            State::Warm => "warm",
            State::Cold => "cold",
        }
    }

    /// Parses a stored name.
    ///
    /// # Errors
    /// [`DomainError::UnknownState`] for anything but `hot`, `warm` or `cold`.
    pub fn parse(name: &str) -> Result<State, DomainError> {
        match name {
            "hot" => Ok(State::Hot),
            "warm" => Ok(State::Warm),
            "cold" => Ok(State::Cold),
            _ => Err(DomainError::UnknownState(name.to_owned())),
        }
    }

    /// The state machine: hot and warm move to each other (hibernate, wake) and to cold
    /// (teardown); cold is final; staying in the same state is not a change.
    ///
    /// # Errors
    /// [`DomainError::Transition`] when the move isn't allowed.
    pub fn change_to(self, to: State) -> Result<State, DomainError> {
        match (self, to) {
            (State::Hot, State::Warm | State::Cold) | (State::Warm, State::Hot | State::Cold) => {
                Ok(to)
            }
            _ => Err(DomainError::Transition { from: self, to }),
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The command that creates a session for a ticket. Sessions start hot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    /// The new session's id.
    pub id: SessionId,
    /// The id of its first event.
    pub event: EventId,
    /// When the command was issued.
    pub at: SystemTime,
    /// The ticket asking for the session.
    pub ticket: Ticket,
    /// Where the session works, and for whom.
    pub workspace: Workspace,
}

impl NewSession {
    /// The session's namespace, `rl-<ticket key>` (ADR 0051).
    #[must_use]
    pub fn namespace(&self) -> String {
        format!("rl-{}", self.ticket.key().to_ascii_lowercase())
    }

    /// What this command records.
    #[must_use]
    pub fn change(&self) -> Change {
        Change::Created {
            ticket_id: self.ticket.id().to_owned(),
            ticket_key: self.ticket.key().to_owned(),
            title: self.ticket.title().to_owned(),
            ticket_url: self.ticket.url().to_owned(),
            repo: self.workspace.repo().to_owned(),
            namespace: self.namespace(),
            state: State::Hot,
        }
    }
}

/// The command that moves a session to another state. The store checks it against the current
/// state, inside the command's transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeState {
    /// The session to change.
    pub session: SessionId,
    /// The id of the event it appends.
    pub event: EventId,
    /// When the command was issued.
    pub at: SystemTime,
    /// The state to move to.
    pub to: State,
}

impl ChangeState {
    /// What this command records.
    #[must_use]
    pub fn change(&self) -> Change {
        Change::StateChanged { state: self.to }
    }
}

/// Where a session's pod is, as its runner reports it (`runner.v1alpha1.Phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Waiting to start.
    Queued,
    /// The namespace exists; the pod isn't ready yet.
    Starting,
    /// The agent pod is ready.
    Running,
    /// Going warm.
    Hibernating,
    /// Warm: namespace kept, zero pods.
    Hibernated,
    /// Going hot again.
    Waking,
    /// Being torn down.
    Terminating,
    /// Torn down.
    Gone,
    /// Failed; the runner said why.
    Failed,
}

impl Phase {
    /// Every phase, for parsing.
    pub(crate) const ALL: [Phase; 9] = [
        Phase::Queued,
        Phase::Starting,
        Phase::Running,
        Phase::Hibernating,
        Phase::Hibernated,
        Phase::Waking,
        Phase::Terminating,
        Phase::Gone,
        Phase::Failed,
    ];

    /// The stored name, for example `running`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Queued => "queued",
            Phase::Starting => "starting",
            Phase::Running => "running",
            Phase::Hibernating => "hibernating",
            Phase::Hibernated => "hibernated",
            Phase::Waking => "waking",
            Phase::Terminating => "terminating",
            Phase::Gone => "gone",
            Phase::Failed => "failed",
        }
    }

    /// Parses a stored name.
    ///
    /// # Errors
    /// [`DomainError::UnknownPhase`] for anything else.
    pub fn parse(name: &str) -> Result<Phase, DomainError> {
        Phase::ALL
            .into_iter()
            .find(|phase| phase.as_str() == name)
            .ok_or_else(|| DomainError::UnknownPhase(name.to_owned()))
    }
}

/// The command that records a runner's `SessionStatus`. The store records each `message_id` once,
/// so a redelivered or republished status changes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportPhase {
    /// The session it reports on.
    pub session: SessionId,
    /// The id of the event it appends.
    pub event: EventId,
    /// When the control plane received it.
    pub at: SystemTime,
    /// The status's `Envelope.message_id`.
    pub message_id: Uuid,
    /// The status's `Envelope.sent_at`, if it had one.
    pub sent_at: Option<SystemTime>,
    /// The reported phase.
    pub phase: Phase,
    /// The runner's reason, at most [`PHASE_MESSAGE_MAX`] characters.
    pub message: String,
    /// The `SessionDesired.generation` the runner had applied.
    pub observed_generation: u64,
}

/// The longest phase message kept, in characters: the runner's own cap (the CRD's
/// `status.message`).
pub const PHASE_MESSAGE_MAX: usize = 2000;

impl ReportPhase {
    /// What this command records.
    #[must_use]
    pub fn change(&self) -> Change {
        Change::PhaseChanged {
            phase: self.phase,
            message: self.message.chars().take(PHASE_MESSAGE_MAX).collect(),
            observed_generation: self.observed_generation,
            message_id: self.message_id,
            sent_at: self.sent_at,
        }
    }
}

/// What a command changed: an event's payload, before its canonical encoding (ADR 0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A session was created for a ticket.
    Created {
        /// The tracker's id of the ticket.
        ticket_id: String,
        /// The human key, for example `RAN-34`.
        ticket_key: String,
        /// The ticket's title when the session was created.
        title: String,
        /// The ticket's page in its tracker.
        ticket_url: String,
        /// The git mirror repo the session works in.
        repo: String,
        /// The session's namespace.
        namespace: String,
        /// The first state, always hot.
        state: State,
    },
    /// A session moved to another state.
    StateChanged {
        /// The new state.
        state: State,
    },
    /// The runner reported a new phase.
    PhaseChanged {
        /// The phase.
        phase: Phase,
        /// The runner's reason.
        message: String,
        /// The generation the runner had applied.
        observed_generation: u64,
        /// The status message it records.
        message_id: Uuid,
        /// When the runner sent it.
        sent_at: Option<SystemTime>,
    },
}

impl Change {
    /// The event type stored next to the payload.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Change::Created { .. } => "session.created",
            Change::StateChanged { .. } => "session.state_changed",
            Change::PhaseChanged { .. } => "session.phase_changed",
        }
    }

    /// The desired state this change sets, if it sets one.
    #[must_use]
    pub fn state(&self) -> Option<State> {
        match self {
            Change::Created { state, .. } | Change::StateChanged { state } => Some(*state),
            Change::PhaseChanged { .. } => None,
        }
    }
}
