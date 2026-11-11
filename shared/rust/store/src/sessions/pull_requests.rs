//! A lane's pull request, linked on its ticket (`db/queries/sessions/unlinked.sql`, `linked.sql`,
//! `link_failed.sql`).

use std::time::SystemTime;

use rl_domain::SessionId;

use super::{Record, SessionRow};
use crate::{Store, StoreError, timestamp};

impl Store {
    /// The sessions whose pull request is still to link (neither linked nor refused), the least
    /// recently tried first, at most `limit`.
    ///
    /// # Errors
    /// [`StoreError::Db`], or [`StoreError::Domain`] for a stored state or phase that isn't one.
    pub async fn unlinked_pull_requests(&self, limit: u16) -> Result<Vec<SessionRow>, StoreError> {
        sqlx::query_file_as!(
            Record,
            "../../../db/queries/sessions/unlinked.sql",
            i64::from(limit)
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(SessionRow::try_from)
        .collect()
    }

    /// Records that the session's ticket links its pull request `url` since `at`; a no-op when
    /// the session's pull request isn't `url`.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn pull_request_linked(
        &self,
        session: SessionId,
        url: &str,
        at: SystemTime,
    ) -> Result<(), StoreError> {
        sqlx::query_file!(
            "../../../db/queries/sessions/linked.sql",
            session.0,
            url,
            timestamp(rl_chain::micros(at)),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records that linking the session's pull request `url` failed at `at`, for good when
    /// `refused`: a refused one is never listed again, another is listed after those tried
    /// longer ago. A no-op when the session's pull request isn't `url`.
    ///
    /// # Errors
    /// [`StoreError::Db`] or [`StoreError::Timeout`].
    pub async fn pull_request_link_failed(
        &self,
        session: SessionId,
        url: &str,
        at: SystemTime,
        refused: bool,
    ) -> Result<(), StoreError> {
        sqlx::query_file!(
            "../../../db/queries/sessions/link_failed.sql",
            session.0,
            url,
            timestamp(rl_chain::micros(at)),
            refused,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
