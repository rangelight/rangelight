//! Rangelight's use cases and the seams they consume (ADR 0049 §2.1, §2.2). Async; no I/O of its
//! own. Postgres is not a seam: a use case returns the command, and the binary hands it to
//! `rl-store`, which applies it in one transaction (ADR 0031).
#![deny(missing_docs)]

use std::future::Future;
use std::time::SystemTime;

use rl_domain::{ChangeState, EventId, NewSession, SessionId, State, Ticket, Workspace};
use uuid::Uuid;

/// Where tickets come from: Linear now, GitHub Issues next (ADR 0049 §2.2). Every implementation
/// passes `rl_testkit::ticket_source_contract`.
pub trait TicketSource: Send + Sync {
    /// Every ticket in scope that carries [`rl_domain::INTAKE_LABEL`]. The tracker filters;
    /// callers don't check again.
    fn tickets(&self) -> impl Future<Output = Result<Vec<Ticket>, TicketError>> + Send;

    /// Links the pull request at `url` to the ticket the tracker knows as `ticket_id`. Linking
    /// one already linked changes nothing. [`TicketError::Refused`] when the tracker won't, ever.
    fn link_pull_request(
        &self,
        ticket_id: &str,
        url: &str,
    ) -> impl Future<Output = Result<(), TicketError>> + Send;
}

/// Why a [`TicketSource`] couldn't list tickets or link one. Never carries a credential.
#[derive(Debug, thiserror::Error)]
pub enum TicketError {
    /// The tracker refused the credential.
    #[error("the ticket tracker refused the credential")]
    Unauthorized,
    /// The tracker or the network failed; worth retrying on the next poll.
    #[error("the ticket tracker is unavailable: {0}")]
    Unavailable(String),
    /// The tracker answered with something that isn't a valid ticket list.
    #[error("the ticket tracker sent a malformed answer: {0}")]
    Malformed(String),
    /// The tracker answered, and refused the request for good (say, the ticket is gone):
    /// retrying won't help.
    #[error("the ticket tracker refused: {0}")]
    Refused(String),
    /// The credential couldn't be read.
    #[error("the ticket tracker credential can't be read: {0}")]
    Credential(String),
}

/// The time, so tests can fix it.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> SystemTime;
}

/// New ids, so tests can fix them.
pub trait IdGen: Send + Sync {
    /// A new UUIDv7.
    fn new_id(&self) -> Uuid;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// UUIDv7 from the system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidV7;

impl IdGen for UuidV7 {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// Intake: one [`NewSession`] in `workspace` for each ticket the source lists. Tickets that
/// already have one are left to the store's unique constraint, so re-polling is a no-op.
///
/// # Errors
/// The source's [`TicketError`].
pub async fn intake(
    source: &impl TicketSource,
    workspace: &Workspace,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<Vec<NewSession>, TicketError> {
    let tickets = source.tickets().await?;
    Ok(tickets
        .into_iter()
        .map(|ticket| NewSession {
            id: SessionId(ids.new_id()),
            event: EventId(ids.new_id()),
            at: clock.now(),
            ticket,
            workspace: workspace.clone(),
        })
        .collect())
}

/// The command that moves a session to `to`. Whether the move is allowed is decided against the
/// current state inside the store's transaction ([`State::change_to`]).
#[must_use]
pub fn change_state(
    session: SessionId,
    to: State,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> ChangeState {
    ChangeState {
        session,
        event: EventId(ids.new_id()),
        at: clock.now(),
        to,
    }
}
