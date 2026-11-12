//! A lane's agent events (`db/queries/sessions/set_question.sql`): each one chained, and an ask
//! opening the session's question, in one transaction.

use rl_domain::{Question, ReportAgentEvent, State};

use crate::events::NewEvent;
use crate::{Store, StoreError, events, outbox, timestamp};

/// What recording an agent event came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEventRecorded {
    /// One new event.
    New,
    /// The session already has an event for this message: nothing written.
    Duplicate,
    /// The session isn't hot, so it has no lane to speak for it: nothing written.
    NotLive,
}

impl Store {
    /// Records a lane's agent event: locks the session's row, and while the session is hot and
    /// unless it already has an event for this message, appends its event, sets or clears the
    /// session's question ([`ReportAgentEvent::question`]) and inserts the outbox row, in one
    /// transaction.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], [`StoreError::Domain`] for a stored state that isn't one, or
    /// [`StoreError::Db`] or [`StoreError::Timeout`]; nothing is written.
    #[tracing::instrument(skip_all, fields(session_id = %command.session.0, message_id = %command.message_id))]
    pub async fn report_agent_event(
        &self,
        command: &ReportAgentEvent,
    ) -> Result<AgentEventRecorded, StoreError> {
        let at = rl_chain::micros(command.at);
        let change = command.change();
        let mut tx = self.pool.begin().await?;
        let session = sqlx::query_file!("../../../db/queries/sessions/lock.sql", command.session.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        // A closed session's lane may still be connected, or its events held in the stream.
        if State::parse(&session.state)? != State::Hot {
            return Ok(AgentEventRecorded::NotLive);
        }
        // The row lock serializes a session's events; UNIQUE (message_id) backs this check.
        let recorded = sqlx::query_file_scalar!(
            "../../../db/queries/events/recorded.sql",
            command.session.0,
            command.message_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if recorded {
            return Ok(AgentEventRecorded::Duplicate);
        }
        let seq = events::append(
            &mut tx,
            NewEvent {
                id: command.event,
                session: command.session,
                change: &change,
                at,
                message_id: Some(command.message_id),
            },
        )
        .await?;
        let (changes, question) = match command.question() {
            Question::Ask(question) => (true, Some(question)),
            Question::Clear => (true, None),
            Question::Keep => (false, None),
        };
        sqlx::query_file!(
            "../../../db/queries/sessions/set_question.sql",
            command.session.0,
            changes,
            question,
            seq,
            timestamp(at),
        )
        .execute(&mut *tx)
        .await?;
        outbox::insert(&mut tx, command.event).await?;
        tx.commit().await?;
        Ok(AgentEventRecorded::New)
    }
}
