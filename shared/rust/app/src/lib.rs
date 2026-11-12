//! Rangelight's use cases and the seams they consume (ADR 0049 §2.1, §2.2). Async; no I/O of its
//! own. Postgres is not a seam: a use case returns the command, and the binary hands it to
//! `rl-store`, which applies it in one transaction (ADR 0031).
#![deny(missing_docs)]

use std::fmt::Write as _;
use std::future::Future;
use std::time::SystemTime;

use rl_domain::{
    ChangeState, DomainError, EventId, NewSession, SessionId, State, Ticket, Tracker, Workspace,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Where tickets come from: Linear now, GitHub Issues next (ADR 0049 §2.2). Every implementation
/// passes `rl_testkit::ticket_source_contract`.
pub trait TicketSource: Send + Sync {
    /// Every ticket in scope that carries [`rl_domain::INTAKE_LABEL`]: those that keep the ticket
    /// rule, and those that don't, with why. The tracker filters; callers don't check again.
    fn tickets(&self) -> impl Future<Output = Result<Listing, TicketError>> + Send;

    /// Links the pull request at `url` to the ticket the tracker knows as `ticket_id`. Linking
    /// one already linked changes nothing. [`TicketError::Refused`] when the tracker won't, ever.
    fn link_pull_request(
        &self,
        ticket_id: &str,
        url: &str,
    ) -> impl Future<Output = Result<(), TicketError>> + Send;

    /// Posts `body` (Markdown) as a comment on the ticket the tracker knows as `ticket_id`.
    /// [`TicketError::Refused`] when the tracker won't, ever.
    fn comment(
        &self,
        ticket_id: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), TicketError>> + Send;
}

/// What a [`TicketSource`] lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The tickets that keep the ticket rule.
    pub tickets: Vec<Ticket>,
    /// The labelled tickets that don't, which get no session.
    pub skipped: Vec<Skipped>,
}

/// A labelled ticket intake can't take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The tracker it came from.
    pub tracker: Tracker,
    /// The tracker's id.
    pub ticket_id: String,
    /// What it was skipped for, hashed: its title, description and reason. Only an edit to those
    /// is a new version; a reply, a status change or the comment itself is not.
    pub version: String,
    /// The rule it breaks.
    pub reason: DomainError,
}

impl Skipped {
    /// The skip of the ticket the tracker knows as `ticket_id`, with this `title` and
    /// `description`, for `reason`.
    #[must_use]
    pub fn new(
        tracker: Tracker,
        ticket_id: String,
        title: &str,
        description: &str,
        reason: DomainError,
    ) -> Skipped {
        let mut hash = Sha256::new();
        // Each part after its length, so no two different tickets hash alike.
        for part in [title, description, &reason.to_string()] {
            hash.update(part.len().to_be_bytes());
            hash.update(part);
        }
        let version = hash.finalize().iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
        Skipped {
            tracker,
            ticket_id,
            version,
            reason,
        }
    }

    /// The comment that tells the ticket's author why it got no session, and what to do.
    #[must_use]
    pub fn comment(&self) -> String {
        let why = match self.reason {
            DomainError::InvalidTicket("criteria") => {
                "no acceptance criteria found (or too many, or one too long). Add a numbered list \
                 under an 'Acceptance criteria' heading, or lines that start with AC1, AC2, …"
            }
            DomainError::InvalidTicket("title") => "its title is empty or too long.",
            DomainError::InvalidTicket("description") => "its description is too long.",
            DomainError::InvalidTicket("key") => "its key can't name a lane's namespace.",
            _ => "it breaks the ticket rule.",
        };
        format!("Rangelight skipped this ticket: {why}")
    }
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

/// Intake: one [`NewSession`] in `workspace` for each ticket the source lists, and the labelled
/// tickets it skips. Tickets that already have a session are left to the store's unique
/// constraint, so re-polling is a no-op.
///
/// # Errors
/// The source's [`TicketError`].
pub async fn intake(
    source: &impl TicketSource,
    workspace: &Workspace,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<Intake, TicketError> {
    let Listing { tickets, skipped } = source.tickets().await?;
    let sessions = tickets
        .into_iter()
        .map(|ticket| NewSession {
            id: SessionId(ids.new_id()),
            event: EventId(ids.new_id()),
            at: clock.now(),
            ticket,
            workspace: workspace.clone(),
        })
        .collect();
    Ok(Intake { sessions, skipped })
}

/// What one intake round makes of the source's listing.
#[derive(Debug, Clone)]
pub struct Intake {
    /// A new session for each ticket that keeps the ticket rule.
    pub sessions: Vec<NewSession>,
    /// The labelled tickets that don't.
    pub skipped: Vec<Skipped>,
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
