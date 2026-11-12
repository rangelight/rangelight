use std::time::SystemTime;

use rl_domain::{EventId, SessionId};
use sqlx::{Postgres, Transaction};

use crate::{Store, StoreError, to_u64};

/// An event the relay hasn't published yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsent {
    /// The outbox row, for [`Store::mark_sent`].
    pub outbox_id: i64,
    /// The event's id.
    pub event: EventId,
    /// Its session.
    pub session: SessionId,
    /// Its seq, which is also the session's generation after it.
    pub seq: u64,
    /// The canonical payload.
    pub payload: Vec<u8>,
    /// When it happened.
    pub at: SystemTime,
    /// Its session's namespace.
    pub namespace: String,
    /// Its session's ticket key.
    pub ticket_key: String,
    /// Its session's ticket title.
    pub title: String,
    /// Its session's ticket description.
    pub description: String,
    /// Its session's acceptance criteria, in order.
    pub criteria: Vec<String>,
    /// Its session's repo, by its git mirror name.
    pub repo: String,
    /// Its session's base branch.
    pub base_ref: String,
    /// Its session's member.
    pub member: String,
}

/// A row of `unsent.sql` or `last_sent_desired.sql`.
struct Row {
    outbox_id: i64,
    event_id: uuid::Uuid,
    session_id: uuid::Uuid,
    seq: i64,
    payload: Vec<u8>,
    at: time::OffsetDateTime,
    namespace: String,
    ticket_key: String,
    title: String,
    description: String,
    criteria: Vec<String>,
    repo: String,
    base_ref: String,
    member: String,
}

impl TryFrom<Row> for Unsent {
    type Error = StoreError;

    fn try_from(row: Row) -> Result<Unsent, StoreError> {
        Ok(Unsent {
            outbox_id: row.outbox_id,
            event: EventId(row.event_id),
            session: SessionId(row.session_id),
            seq: to_u64(row.seq, "seq")?,
            payload: row.payload,
            at: SystemTime::from(row.at),
            namespace: row.namespace,
            ticket_key: row.ticket_key,
            title: row.title,
            description: row.description,
            criteria: row.criteria,
            repo: row.repo,
            base_ref: row.base_ref,
            member: row.member,
        })
    }
}

pub(crate) async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    event: EventId,
) -> Result<(), StoreError> {
    sqlx::query_file!("../../../db/queries/outbox/insert.sql", event.0)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

impl Store {
    /// The oldest unsent events, at most `limit`, leaving out every session with a failed row.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn unsent(&self, limit: u16) -> Result<Vec<Unsent>, StoreError> {
        sqlx::query_file_as!(
            Row,
            "../../../db/queries/outbox/unsent.sql",
            i64::from(limit)
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Unsent::try_from)
        .collect()
    }

    /// Each session's last published desired state, to put back into a desired stream that lost
    /// its messages.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn last_sent_desired(&self) -> Result<Vec<Unsent>, StoreError> {
        sqlx::query_file_as!(Row, "../../../db/queries/outbox/last_sent_desired.sql")
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(Unsent::try_from)
            .collect()
    }

    /// Sets aside a row the relay can't publish. Its session's later rows stop going out (they
    /// would arrive out of order); other sessions are unaffected.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn mark_failed(&self, outbox_id: i64) -> Result<(), StoreError> {
        sqlx::query_file!("../../../db/queries/outbox/mark_failed.sql", outbox_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Marks an outbox row sent. Call it only after NATS acknowledged the publish.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn mark_sent(&self, outbox_id: i64) -> Result<(), StoreError> {
        sqlx::query_file!("../../../db/queries/outbox/mark_sent.sql", outbox_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
