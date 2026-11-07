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
        let rows = sqlx::query_file!("../../../db/queries/outbox/unsent.sql", i64::from(limit))
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Unsent {
                    outbox_id: row.outbox_id,
                    event: EventId(row.event_id),
                    session: SessionId(row.session_id),
                    seq: to_u64(row.seq, "seq")?,
                    payload: row.payload,
                    at: SystemTime::from(row.at),
                    namespace: row.namespace,
                })
            })
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
