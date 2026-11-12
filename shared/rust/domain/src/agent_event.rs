use std::time::SystemTime;

use uuid::Uuid;

use crate::{Change, EventId, SessionId};

/// One event a lane's agent sent (`api/jsonschema/agent-event.v1alpha1.schema.json`), its fields
/// already held to the schema at the edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// The lane needs its member: the session waits for them.
    Ask {
        /// What it asks.
        question: String,
        /// Choices to offer, if any.
        options: Vec<String>,
    },
    /// Feed only; also the start of each turn (stage `turn`).
    Progress {
        /// A short slug, for example `tests`.
        stage: String,
        /// What happened.
        note: String,
    },
    /// The agent says it is done: a claim, never proof (ADR 0045 §3).
    WorkerDone {
        /// What it says it did.
        summary: String,
        /// The commit it pushed.
        head_sha: String,
    },
    /// The agent session's context was compacted.
    Compacted {
        /// `auto` or `manual`.
        trigger: String,
        /// The compaction's summary.
        summary: String,
    },
}

/// What an agent event does to the session's question (the member is "waiting for you" while it
/// has one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question<'a> {
    /// An ask: this is the open question now.
    Ask(&'a str),
    /// The lane went on (a turn started, or it says it is done): nothing is open.
    Clear,
    /// No change.
    Keep,
}

/// The command that records an agent event. The store records each `message_id` once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportAgentEvent {
    /// The session it came from: the subject's, never the payload's.
    pub session: SessionId,
    /// The id of the event it appends.
    pub event: EventId,
    /// When the control plane received it.
    pub at: SystemTime,
    /// The event's `messageId`.
    pub message_id: Uuid,
    /// The event's `at`: when the agent emitted it.
    pub sent_at: SystemTime,
    /// The event.
    pub agent: AgentEvent,
}

impl ReportAgentEvent {
    /// What this command records.
    #[must_use]
    pub fn change(&self) -> Change {
        Change::AgentEvent {
            message_id: self.message_id,
            sent_at: self.sent_at,
            event: self.agent.clone(),
        }
    }

    /// What it does to the session's question: an ask opens it, a progress or a `worker_done`
    /// closes it, a compaction leaves it.
    #[must_use]
    pub fn question(&self) -> Question<'_> {
        match &self.agent {
            AgentEvent::Ask { question, .. } => Question::Ask(question),
            AgentEvent::Progress { .. } | AgentEvent::WorkerDone { .. } => Question::Clear,
            AgentEvent::Compacted { .. } => Question::Keep,
        }
    }
}
