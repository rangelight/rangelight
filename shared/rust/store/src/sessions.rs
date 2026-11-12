use std::collections::HashMap;
use std::time::SystemTime;

use rl_domain::{Change, ChangeState, NewSession, Phase, ReportPhase, SessionId, State};
use time::OffsetDateTime;
use uuid::Uuid;

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
    /// The ticket's page in its tracker; empty for sessions created before it was recorded.
    pub ticket_url: String,
    /// The session's namespace.
    pub namespace: String,
    /// The git mirror repo it works in.
    pub repo: String,
    /// The member it works for (ADR 0004): only they may open its terminal.
    pub member: String,
    /// The desired state.
    pub state: State,
    /// The phase its runner last reported, if it has.
    pub phase: Option<Phase>,
    /// The lane's pull request, once its runner reported one.
    pub pull_request_url: Option<String>,
    /// The seq of the session's last event.
    pub generation: u64,
    /// When the session was created.
    pub created_at: SystemTime,
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
            command.ticket.description(),
            command.ticket.criteria(),
            command.ticket.url(),
            command.namespace(),
            command.workspace.repo(),
            command.workspace.base_ref(),
            command.workspace.member(),
            // NewSession::change records it: sessions start hot.
            State::Hot.as_str(),
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
                message_id: None,
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
        let current = sqlx::query_file!("../../../db/queries/sessions/lock.sql", command.session.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        let to = State::parse(&current.state)?.change_to(command.to)?;
        let seq = events::append(
            &mut tx,
            NewEvent {
                id: command.event,
                session: command.session,
                change: &change,
                at,
                message_id: None,
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

    /// Records a runner's status: locks the session's row, and unless the status's message is
    /// already recorded, appends its event, sets the phase (unless a later report already did)
    /// and the session's first pull request ([`ReportPhase::change`] decides), and inserts the
    /// outbox row, in one transaction. Returns false, writing nothing, for a message already
    /// recorded.
    ///
    /// # Errors
    /// [`StoreError::NotFound`], or [`StoreError::Db`] or [`StoreError::Timeout`]; nothing is
    /// written.
    #[tracing::instrument(skip_all, fields(session_id = %command.session.0, message_id = %command.message_id))]
    pub async fn report_phase(&self, command: &ReportPhase) -> Result<bool, StoreError> {
        let at = rl_chain::micros(command.at);
        let mut tx = self.pool.begin().await?;
        let session = sqlx::query_file!("../../../db/queries/sessions/lock.sql", command.session.0)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        // The row lock serializes a session's reports; UNIQUE (message_id) backs this check.
        let recorded = sqlx::query_file_scalar!(
            "../../../db/queries/events/recorded.sql",
            command.message_id
        )
        .fetch_one(&mut *tx)
        .await?;
        if recorded {
            return Ok(false);
        }
        let change = command.change(&session.repo, session.pull_request_url.as_deref());
        if command
            .pull_request
            .as_ref()
            .is_some_and(|pr| pr.repo() != session.repo)
        {
            tracing::warn!("ignored a pull request of another repo's upstream");
        }
        let pull_request_url = match &change {
            Change::PhaseChanged {
                pull_request_url, ..
            } => pull_request_url.as_deref(),
            _ => None,
        };
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
        sqlx::query_file!(
            "../../../db/queries/sessions/set_phase.sql",
            command.session.0,
            command.phase.as_str(),
            seq,
            timestamp(at),
            command.message_id,
            pull_request_url,
        )
        .execute(&mut *tx)
        .await?;
        outbox::insert(&mut tx, command.event).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// The newest sessions first, at most `limit`.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state or phase that isn't one.
    pub async fn list_sessions(&self, limit: u16) -> Result<Vec<SessionRow>, StoreError> {
        sqlx::query_file_as!(
            Record,
            "../../../db/queries/sessions/list.sql",
            i64::from(limit)
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(SessionRow::try_from)
        .collect()
    }

    /// One session, or `None` when there's no such session.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state or phase that isn't one.
    pub async fn get_session(&self, id: SessionId) -> Result<Option<SessionRow>, StoreError> {
        sqlx::query_file_as!(Record, "../../../db/queries/sessions/get.sql", id.0)
            .fetch_optional(&self.pool)
            .await?
            .map(SessionRow::try_from)
            .transpose()
    }

    /// The session whose namespace is `namespace`, or `None`.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state or phase that isn't one.
    pub async fn session_by_namespace(
        &self,
        namespace: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        sqlx::query_file_as!(
            Record,
            "../../../db/queries/sessions/by_namespace.sql",
            namespace
        )
        .fetch_optional(&self.pool)
        .await?
        .map(SessionRow::try_from)
        .transpose()
    }

    /// What a `WatchSessions` stream starts from: the newest sessions to show (at most `limit`) and
    /// every session's generation, read in one repeatable-read transaction so they agree.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state or phase that isn't one.
    pub async fn snapshot(&self, limit: u16) -> Result<Snapshot, StoreError> {
        let mut tx = self
            .pool
            .begin_with(include_str!(
                "../../../../db/queries/sessions/begin_snapshot.sql"
            ))
            .await?;
        let generations = sqlx::query_file!("../../../db/queries/sessions/generations.sql")
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|row| Ok((SessionId(row.id), to_u64(row.generation, "generation")?)))
            .collect::<Result<_, StoreError>>()?;
        let sessions = sqlx::query_file_as!(
            Record,
            "../../../db/queries/sessions/list.sql",
            i64::from(limit)
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(SessionRow::try_from)
        .collect::<Result<_, _>>()?;
        tx.commit().await?;
        Ok(Snapshot {
            sessions,
            generations,
        })
    }
}

/// The sessions to show and every session's generation, at one instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The newest sessions first.
    pub sessions: Vec<SessionRow>,
    /// Every session's generation, shown or not.
    pub generations: HashMap<SessionId, u64>,
}

/// A row of `list.sql` or `get.sql`.
struct Record {
    id: Uuid,
    tracker: String,
    ticket_id: String,
    ticket_key: String,
    title: String,
    ticket_url: String,
    namespace: String,
    repo: String,
    member: String,
    state: String,
    phase: Option<String>,
    pull_request_url: Option<String>,
    generation: i64,
    created_at: OffsetDateTime,
}

impl TryFrom<Record> for SessionRow {
    type Error = StoreError;

    fn try_from(row: Record) -> Result<SessionRow, StoreError> {
        Ok(SessionRow {
            id: SessionId(row.id),
            tracker: row.tracker,
            ticket_id: row.ticket_id,
            ticket_key: row.ticket_key,
            title: row.title,
            ticket_url: row.ticket_url,
            namespace: row.namespace,
            repo: row.repo,
            member: row.member,
            state: State::parse(&row.state)?,
            phase: row.phase.as_deref().map(Phase::parse).transpose()?,
            pull_request_url: row.pull_request_url,
            generation: to_u64(row.generation, "generation")?,
            created_at: SystemTime::from(row.created_at),
        })
    }
}

mod pull_requests;
