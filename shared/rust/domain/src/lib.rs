//! Rangelight's domain: session ids, the session state machine, tickets and the commands that
//! change sessions. Sync, no I/O, no clock, no environment (ADR 0049 §2.1).
#![deny(missing_docs)]

mod session;
pub mod subject;
mod ticket;

pub use session::{Change, ChangeState, EventId, NewSession, SessionId, State};
pub use ticket::{INTAKE_LABEL, Ticket, Tracker};

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
    /// A ticket field is empty, too long or has characters it can't have.
    #[error("invalid ticket {0}")]
    InvalidTicket(&'static str),
    /// A NATS subject token is empty or holds `.`, `*`, `>` or whitespace.
    #[error("invalid subject token {0:?}")]
    InvalidToken(String),
}

#[cfg(test)]
mod tests;
