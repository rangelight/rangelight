use rl_domain::{ChangeState, NewSession, SessionId, State};

use crate::events::NewEvent;
use crate::{Store, StoreError, events, outbox, timestamp, to_u64};

/// A session as the state table holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// The id.
    pub id: SessionId,
    /// The tracker its ticket is in, for example `linear`.
    pub tracker: String,
    /// The tracker's id of the ticket.
    pub ticket_id: String,
    /// The ticket's key, for example `RAN-34`.
    pub ticket_key: String,
    /// The ticket's title when the session was created.
    pub title: String,
    /// The session's namespace.
    pub namespace: String,
    /// The desired state.
    pub state: State,
    /// The seq of the session's last event.
    pub generation: u64,
}

impl Store {
    /// Creates the session, its first event and its outbox row in one transaction. Returns
    /// false, writing nothing, when the ticket already has a session.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`]; nothing is written.
    #[tracing::instrument(skip_all, fields(session_id = %command.id.0, event_id = %command.event.0))]
    pub async fn create_session(&self, command: &NewSession) -> Result<bool, StoreError> {
        let at = rl_chain::micros(command.at);
        let change = command.change();
        let mut tx = self.pool.begin().await?;
        let created = sqlx::query_file_scalar!(
            "../../../db/queries/sessions/insert.sql",
            command.id.0,
            command.ticket.tracker().as_str(),
            command.ticket.id(),
            command.ticket.key(),
            command.ticket.title(),
            command.namespace(),
            change.state().as_str(),
            timestamp(at),
        )
        .fetch_optional(&mut *tx)
        .await?;
        if created.is_none() {
            return Ok(false);
        }
        events::append(
            &mut tx,
            NewEvent {
                id: command.event,
                session: command.id,
                change: &change,
                at,
            },
        )
        .await?;
        outbox::insert(&mut tx, command.event).await?;
        tx.commit().await?;
        tracing::info!(ticket = command.ticket.key(), "session created");
        Ok(true)
    }

    /// Moves the session to another state: locks its row, checks the move against the state
    /// machine, appends the event, updates the state and inserts the outbox row, in one
    /// transaction. Returns the new generation.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], [`StoreError::Domain`] for a forbidden move, or
    /// [`StoreError::Db`] or [`StoreError::Timeout`]; nothing is written.
    #[tracing::instrument(skip_all, fields(session_id = %command.session.0, event_id = %command.event.0))]
    pub async fn change_state(&self, command: &ChangeState) -> Result<u64, StoreError> {
        let at = rl_chain::micros(command.at);
        let change = command.change();
        let mut tx = self.pool.begin().await?;
        let current =
            sqlx::query_file_scalar!("../../../db/queries/sessions/lock.sql", command.session.0)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(StoreError::NotFound)?;
        let to = State::parse(&current)?.change_to(command.to)?;
        let seq = events::append(
            &mut tx,
            NewEvent {
                id: command.event,
                session: command.session,
                change: &change,
                at,
            },
        )
        .await?;
        sqlx::query_file!(
            "../../../db/queries/sessions/set_state.sql",
            command.session.0,
            to.as_str(),
            seq,
            timestamp(at),
        )
        .execute(&mut *tx)
        .await?;
        outbox::insert(&mut tx, command.event).await?;
        tx.commit().await?;
        to_u64(seq, "seq")
    }

    /// The newest sessions first, at most `limit`.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state that isn't one.
    pub async fn list_sessions(&self, limit: u16) -> Result<Vec<SessionRow>, StoreError> {
        let rows = sqlx::query_file!("../../../db/queries/sessions/list.sql", i64::from(limit))
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(SessionRow {
                    id: SessionId(row.id),
                    tracker: row.tracker,
                    ticket_id: row.ticket_id,
                    ticket_key: row.ticket_key,
                    title: row.title,
                    namespace: row.namespace,
                    state: State::parse(&row.state)?,
                    generation: to_u64(row.generation, "generation")?,
                })
            })
            .collect()
    }
}
