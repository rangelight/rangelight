use std::time::SystemTime;

use rl_chain::{GENESIS, Record};
use rl_domain::{Change, EventId, SessionId};
use sqlx::{Postgres, Transaction};

use crate::{Store, StoreError, timestamp, to_u64};

/// One link of a session's chain, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEvent {
    /// The event's id.
    pub id: EventId,
    /// Its position in the session, from 1.
    pub seq: u64,
    /// Its type, for example `session.created`.
    pub kind: String,
    /// The canonical payload.
    pub payload: Vec<u8>,
    /// When it happened, at the chain's microsecond precision.
    pub at: SystemTime,
    /// The previous event's hash, or 32 zero bytes.
    pub prev_hash: Vec<u8>,
    /// `rl_chain::link` of the previous hash and this event's record.
    pub hash: Vec<u8>,
}

/// An event to append.
pub(crate) struct NewEvent<'a> {
    pub(crate) id: EventId,
    pub(crate) session: SessionId,
    pub(crate) change: &'a Change,
    /// Microseconds since the epoch (`rl_chain::micros`): what is hashed is what is stored.
    pub(crate) at: i64,
    /// The runner message it records, if one.
    pub(crate) message_id: Option<uuid::Uuid>,
}

/// Appends the session's next event, chained to its last one, and returns its seq. The caller
/// holds the session's row lock; `UNIQUE (session_id, seq)` refuses a concurrent append anyway.
pub(crate) async fn append(
    tx: &mut Transaction<'_, Postgres>,
    event: NewEvent<'_>,
) -> Result<i64, StoreError> {
    let last = sqlx::query_file!("../../../db/queries/events/last.sql", event.session.0)
        .fetch_optional(&mut **tx)
        .await?;
    let (seq, prev_hash) = match last {
        Some(last) => (
            last.seq
                .checked_add(1)
                .ok_or(StoreError::OutOfRange("seq"))?,
            last.hash,
        ),
        None => (1, GENESIS.to_vec()),
    };
    let payload = rl_chain::payload(event.change);
    let hash = rl_chain::link(
        &prev_hash,
        &Record {
            id: event.id,
            at: event.at,
            payload: &payload,
        },
    );
    sqlx::query_file!(
        "../../../db/queries/events/insert.sql",
        event.id.0,
        event.session.0,
        seq,
        event.change.kind(),
        payload,
        prev_hash,
        &hash[..],
        timestamp(event.at),
        event.message_id,
    )
    .execute(&mut **tx)
    .await?;
    Ok(seq)
}

impl Store {
    /// A session's chain in order, for verification (`rl_chain::verify`), replay and receipts.
    ///
    /// # Errors
    /// [`StoreError::Db`].
    pub async fn chain(&self, session: SessionId) -> Result<Vec<ChainEvent>, StoreError> {
        let rows = sqlx::query_file!("../../../db/queries/events/list.sql", session.0)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ChainEvent {
                    id: EventId(row.id),
                    seq: to_u64(row.seq, "seq")?,
                    kind: row.r#type,
                    payload: row.payload,
                    at: SystemTime::from(row.at),
                    prev_hash: row.prev_hash,
                    hash: row.hash,
                })
            })
            .collect()
    }
}
