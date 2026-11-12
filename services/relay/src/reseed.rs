//! Putting the desired stream back from Postgres, at every start (RAN-43).

use rl_store::Unsent;

use crate::{Relay, RelayError, decode};

impl Relay {
    /// Puts each session's last published desired state back wherever the desired stream lost
    /// it (a wiped volume, a new NATS account, a reseed cut short), at every start: each only onto
    /// its session's empty subject, so it never replaces what's there, newer or not. A row that
    /// can't be decoded is logged and skipped. The outbox then publishes what's newer, in order.
    /// Returns how many it put back.
    /// ponytail: every session's subject is checked at each start, tombstones included. Ceiling:
    /// start time at hundreds of thousands of sessions. Upgrade: reseed only on a stream that
    /// holds fewer subjects than Postgres has sessions.
    ///
    /// # Errors
    /// [`RelayError::Store`] or [`RelayError::Nats`]: the next start goes on where this stopped.
    pub async fn reseed(&self) -> Result<usize, RelayError> {
        let sessions = self.store.last_sent_desired().await?;
        let mut put = 0;
        for event in &sessions {
            put += usize::from(self.reseed_one(event).await?);
        }
        tracing::info!(
            sessions = sessions.len(),
            put,
            "reseeded the desired stream"
        );
        Ok(put)
    }

    /// Puts `event`'s desired state onto its session's subject if that is empty; whether it did.
    async fn reseed_one(&self, event: &Unsent) -> Result<bool, RelayError> {
        match decode(event) {
            Ok((_, Some(state))) => self.send_desired(event, state, true).await,
            Ok((_, None)) => Ok(false),
            Err(_) => {
                tracing::error!(session_id = %event.session.0, "can't reseed a session's desired state; skipped");
                Ok(false)
            }
        }
    }
}
