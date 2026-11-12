//! Rangelight's domain: session ids, the session state machine and phases, tickets, members,
//! workspaces, pull requests, lane agents' events and the commands that change sessions. Sync, no I/O, no clock, no environment
//! (ADR 0049 §2.1).
#![deny(missing_docs)]

mod agent_event;
mod member;
mod pull_request;
mod session;
pub mod subject;
pub(crate) mod ticket;
mod workspace;

pub use agent_event::{AgentEvent, Question, ReportAgentEvent};
pub use member::Member;
pub use pull_request::{PullRequest, Upstream};
pub use session::{
    Change, ChangeState, EventId, NewSession, PHASE_MESSAGE_MAX, Phase, ReportPhase, SessionId,
    State,
};
pub use ticket::{INTAKE_LABEL, Ticket, Tracker};
pub use workspace::Workspace;

/// A rule of the domain was broken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    /// A session can't move from its state to the asked one.
    #[error("a {from} session can't become {to}")]
    Transition {
        /// The current state.
        from: State,
        /// The asked state.
        to: State,
    },
    /// A stored or received state name isn't one of hot, warm, cold.
    #[error("unknown session state {0:?}")]
    UnknownState(String),
    /// A stored or reported phase name isn't one of [`Phase`]'s.
    #[error("unknown session phase {0:?}")]
    UnknownPhase(String),
    /// A ticket field breaks the ticket rule (`api/jsonschema/ticket.v1alpha1.schema.json`).
    #[error("invalid ticket {0}")]
    InvalidTicket(&'static str),
    /// A workspace field isn't a repo name, branch name or member name.
    #[error("invalid workspace {0}")]
    InvalidWorkspace(&'static str),
    /// A NATS subject token is empty or holds `.`, `*`, `>` or whitespace.
    #[error("invalid subject token {0:?}")]
    InvalidToken(String),
    /// A repo upstream that isn't `https://github.com/<owner>/<repo>`.
    #[error("the upstream must be https://github.com/<owner>/<repo>")]
    InvalidUpstream,
    /// A username that isn't a member name (see [`Member`]). Holds nothing from the input.
    #[error("the username is not a valid member name")]
    InvalidMember,
}

#[cfg(test)]
mod tests;
