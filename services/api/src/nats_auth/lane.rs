//! A lane's sign-in: its session must be live, and no other connection may be its lane.

use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use rl_domain::{Phase, SessionId, State};
use serde::Deserialize;

use super::{Callout, Identity, LOOKUP_TIMEOUT, permissions};

/// How long after a lane's sign-in its connection may still be missing from the server's list
/// (the server registers it once it has the answer): a second sign-in meanwhile is refused.
const SETTLE: Duration = Duration::from_secs(2);

/// One session's lane sign-ins, one at a time: when the last one was let in.
pub(super) type Slot = tokio::sync::Mutex<Option<Instant>>;

impl Callout {
    /// The live session whose lane runs in `namespace`, if no other connection is its lane. One
    /// sign-in per session at a time, so two at once can't both find none.
    pub(super) async fn live_lane(&self, namespace: &str) -> Result<SessionId, &'static str> {
        let row = tokio::time::timeout(LOOKUP_TIMEOUT, self.store.session_by_namespace(namespace))
            .await
            .map_err(|_| "store_timeout")?
            .map_err(|error| {
                tracing::warn!(%error, "can't look a lane's session up");
                "store_unavailable"
            })?
            .ok_or("no_session")?;
        // A closed session's lane can't come back.
        if row.state != State::Hot {
            return Err("session_not_live");
        }
        // Only a lane its runner brought up: the runner reports Starting only once the namespace is
        // the one it made for this Session, so a namespace someone made in advance (the runner
        // fails the session) never gets the lane's identity.
        if !matches!(row.phase, Some(Phase::Starting | Phase::Running)) {
            return Err("lane_not_started");
        }
        let slot = self.lane_slot(row.id);
        let mut let_in = slot.lock().await;
        if let_in.is_some_and(|at| at.elapsed() < SETTLE) || self.lane_connected(row.id).await? {
            return Err("lane_already_connected");
        }
        *let_in = Some(Instant::now());
        Ok(row.id)
    }

    /// `session`'s sign-in slot; forgets the slots no sign-in holds or settles.
    fn lane_slot(&self, session: SessionId) -> Arc<Slot> {
        let mut lanes = self.lanes.lock().unwrap_or_else(PoisonError::into_inner);
        lanes.retain(|_, slot| {
            Arc::strong_count(slot) > 1
                || slot
                    .try_lock()
                    .is_ok_and(|at| at.is_some_and(|at| at.elapsed() < SETTLE))
        });
        Arc::clone(lanes.entry(session).or_default())
    }

    /// Whether a connection is already `session`'s lane: the lane account's connections of its
    /// user, subscribed or not.
    /// ponytail: the first server's answer, and slots per api replica. Ceiling: a clustered NATS
    /// (`scale`), or two replicas answering one lane's two sign-ins within [`SETTLE`] (it can only
    /// reach its own terminal either way). Upgrade: gather every server's answer, and a lock in
    /// Postgres.
    async fn lane_connected(&self, session: SessionId) -> Result<bool, &'static str> {
        #[derive(Deserialize)]
        struct Reply {
            data: Connz,
        }
        #[derive(Deserialize)]
        struct Connz {
            num_connections: u64,
        }
        let user = permissions::grant(&self.org, &Identity::Lane(session)).name;
        let filter = serde_json::json!({ "user": user });
        let connz = format!("$SYS.REQ.ACCOUNT.{}.CONNZ", self.lane_account);
        let reply = tokio::time::timeout(
            LOOKUP_TIMEOUT,
            self.nats.request(connz, filter.to_string().into()),
        )
        .await
        .map_err(|_| "connz_timeout")?
        .map_err(|_| "connz_unavailable")?;
        let reply: Reply = serde_json::from_slice(&reply.payload).map_err(|_| "connz_invalid")?;
        Ok(reply.data.num_connections > 0)
    }
}
