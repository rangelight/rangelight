use std::time::SystemTime;

use crate::{Store, StoreError, timestamp};

/// A skipped ticket whose comment is still to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipRow {
    /// The tracker, for example `linear`.
    pub tracker: String,
    /// The tracker's id of the ticket.
    pub ticket_id: String,
    /// The ticket's version when it was skipped.
    pub version: String,
    /// The comment.
    pub body: String,
}

impl Store {
    /// Records that intake skipped the ticket at `version`, saying `body`: a new version is to
    /// comment on again; the same one changes nothing.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn record_skip(
        &self,
        tracker: &str,
        ticket_id: &str,
        version: &str,
        body: &str,
    ) -> Result<(), StoreError> {
        sqlx::query_file!(
            "../../../db/queries/skips/record.sql",
            tracker,
            ticket_id,
            version,
            body
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// At most `limit` skips still to comment on, the least recently tried first.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn uncommented_skips(&self, limit: u16) -> Result<Vec<SkipRow>, StoreError> {
        Ok(sqlx::query_file_as!(
            SkipRow,
            "../../../db/queries/skips/uncommented.sql",
            i64::from(limit)
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Keeps only the skips of `tracker`'s tickets `skipped`, the ones it lists as skipped now: a
    /// ticket now valid never hears it was skipped, and a closed one's row goes.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn keep_skips(&self, tracker: &str, skipped: &[String]) -> Result<(), StoreError> {
        sqlx::query_file!("../../../db/queries/skips/keep.sql", tracker, skipped)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records the skip's comment posted at `at`. A no-op when the ticket was edited since
    /// `skip.version`.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn skip_commented(&self, skip: &SkipRow, at: SystemTime) -> Result<(), StoreError> {
        sqlx::query_file!(
            "../../../db/queries/skips/commented.sql",
            skip.tracker,
            skip.ticket_id,
            skip.version,
            timestamp(rl_chain::micros(at)),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records that commenting on the skip failed at `at`, for good when `refused`: a refused one
    /// is never listed again, another is listed after those tried longer ago.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn skip_comment_failed(
        &self,
        skip: &SkipRow,
        at: SystemTime,
        refused: bool,
    ) -> Result<(), StoreError> {
        sqlx::query_file!(
            "../../../db/queries/skips/comment_failed.sql",
            skip.tracker,
            skip.ticket_id,
            skip.version,
            timestamp(rl_chain::micros(at)),
            refused,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
