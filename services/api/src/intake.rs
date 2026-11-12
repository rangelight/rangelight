//! Intake, and the tracker write-backs: each session's pull request linked to its ticket, why a
//! skipped ticket got no session, and the label check at start.

use std::time::Duration;

use rl_app::{
    Clock, IdGen, Intake, Skipped, SystemClock, TicketError, TicketSource, UuidV7, intake,
};
use rl_domain::{NewSession, Workspace};
use rl_store::{SessionRow, SkipRow, Store, StoreError};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::ApiError;

/// The pause between two intake polls.
const POLL: Duration = Duration::from_secs(30);
/// The most pull requests one round links, and the most skip comments it posts.
const LINKS: u16 = 10;

/// One intake round: lists the tickets, creates a session in `workspace` for each that has none
/// yet, records each labelled ticket it skips, for the link loop to say why on the ticket, and
/// drops the skips of tickets it doesn't skip any more.
/// A command that fails (say, a namespace another ticket already holds) is logged and skipped;
/// the round goes on. Returns how many sessions it created.
///
/// # Errors
/// [`ApiError::Tickets`] when the source can't list tickets.
pub async fn intake_once(
    store: &Store,
    source: &impl TicketSource,
    workspace: &Workspace,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<usize, ApiError> {
    let mut created = 0;
    let Intake { sessions, skipped } = intake(source, workspace, clock, ids).await?;
    keep_skips(store, &sessions, &skipped).await;
    record_skips(store, &skipped).await;
    for command in sessions {
        match store.create_session(&command).await {
            Ok(true) => created += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, ticket = command.ticket.key(), "skipped a ticket this round");
            }
        }
    }
    Ok(created)
}

/// One link round: attaches each session's pull request to its ticket, once; at most 10, the
/// least recently tried first. A link the tracker refuses for good is recorded and never tried
/// again; another failure is recorded and tried again in a later round. The stored pull request
/// stays either way. Returns how many it linked.
///
/// # Errors
/// [`ApiError::Store`] when the sessions can't be read.
pub async fn link_once(
    store: &Store,
    source: &impl TicketSource,
    clock: &impl Clock,
) -> Result<usize, ApiError> {
    let mut linked = 0;
    for session in store.unlinked_pull_requests(LINKS).await? {
        let id = session.id;
        match link(store, source, clock, session).await {
            Ok(true) => linked += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, session_id = %id.0, "couldn't record a link attempt; trying again next round");
            }
        }
    }
    Ok(linked)
}

/// One comment round: posts on each skipped ticket why intake skipped it, once per version of the
/// ticket; at most 10, the least recently tried first. A comment the tracker refuses for good is
/// recorded and never tried again; another failure is tried again in a later round. Returns how
/// many it posted.
///
/// # Errors
/// [`ApiError::Store`] when the skips can't be read.
pub async fn comment_once(
    store: &Store,
    source: &impl TicketSource,
    clock: &impl Clock,
) -> Result<usize, ApiError> {
    let mut posted = 0;
    for skip in store.uncommented_skips(LINKS).await? {
        match comment(store, source, clock, &skip).await {
            Ok(true) => posted += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, ticket_id = skip.ticket_id, "couldn't record a comment attempt; trying again next round");
            }
        }
    }
    Ok(posted)
}

/// Keeps only the skips of the tickets listed as skipped this round: a ticket fixed while its
/// comment was still to post never hears it was skipped, and a closed or unlabelled one's row
/// goes. One statement a round: a source is one tracker.
async fn keep_skips(store: &Store, sessions: &[NewSession], skipped: &[Skipped]) {
    // ponytail: a round that lists nothing names no tracker, so its stale rows wait for one that does.
    let tracker = sessions
        .first()
        .map(|s| s.ticket.tracker())
        .or_else(|| skipped.first().map(|s| s.tracker));
    let Some(tracker) = tracker else {
        return;
    };
    let ids: Vec<String> = skipped.iter().map(|s| s.ticket_id.clone()).collect();
    if let Err(error) = store.keep_skips(tracker.as_str(), &ids).await {
        tracing::warn!(%error, "couldn't drop the skips of tickets no longer skipped; trying again next round");
    }
}

/// Records each skipped ticket, for [`comment_once`]. One that fails to record is tried again by
/// the next round, which lists it again.
async fn record_skips(store: &Store, skipped: &[Skipped]) {
    for skip in skipped {
        let (tracker, body) = (skip.tracker.as_str(), skip.comment());
        if let Err(error) = store
            .record_skip(tracker, &skip.ticket_id, &skip.version, &body)
            .await
        {
            tracing::warn!(%error, ticket_id = skip.ticket_id, "couldn't record a skipped ticket; trying again next round");
        }
    }
}

/// Posts one skip's comment and records the outcome; true when it is posted.
async fn comment(
    store: &Store,
    source: &impl TicketSource,
    clock: &impl Clock,
    skip: &SkipRow,
) -> Result<bool, StoreError> {
    match source.comment(&skip.ticket_id, &skip.body).await {
        Ok(()) => {
            store.skip_commented(skip, clock.now()).await?;
            Ok(true)
        }
        Err(error) => {
            let refused = matches!(error, TicketError::Refused(_));
            tracing::warn!(%error, ticket_id = skip.ticket_id, refused, "couldn't comment on a skipped ticket");
            store
                .skip_comment_failed(skip, clock.now(), refused)
                .await?;
            Ok(false)
        }
    }
}

/// Warns, once at start, when the Linear team has no [`rl_domain::INTAKE_LABEL`] label: intake
/// would list nothing, silently. Its own task, so a slow Linear never holds up the start.
pub async fn check_intake_label(linear: rl_tickets_linear::Linear) {
    match linear.has_intake_label().await {
        Ok(true) => {}
        Ok(false) => no_intake_label(),
        Err(error) => tracing::warn!(%error, "couldn't check the Linear team's labels"),
    }
}

fn no_intake_label() {
    tracing::warn!(
        label = rl_domain::INTAKE_LABEL,
        "the Linear team has no such label, so no ticket gets a session: create it in Linear (Settings, Labels)"
    );
}

/// Links one session's pull request and records the outcome; true when it is linked.
async fn link(
    store: &Store,
    source: &impl TicketSource,
    clock: &impl Clock,
    session: SessionRow,
) -> Result<bool, StoreError> {
    let Some(url) = session.pull_request_url else {
        return Ok(false);
    };
    match source.link_pull_request(&session.ticket_id, &url).await {
        Ok(()) => {
            store
                .pull_request_linked(session.id, &url, clock.now())
                .await?;
            Ok(true)
        }
        Err(error) => {
            let refused = matches!(error, TicketError::Refused(_));
            tracing::warn!(%error, session_id = %session.id.0, refused, "couldn't link a pull request");
            store
                .pull_request_link_failed(session.id, &url, clock.now(), refused)
                .await?;
            Ok(false)
        }
    }
}

/// Intake every 30 seconds until `cancel`, with the system clock and UUIDv7 ids. A failed round
/// is logged and retried next poll. A round in flight is dropped at `cancel`, so shutdown never
/// waits on Linear or Postgres; each command is one transaction, so a dropped round leaves nothing
/// half-written.
pub async fn intake_loop(
    store: Store,
    source: impl TicketSource,
    workspace: Workspace,
    cancel: CancellationToken,
) {
    every_poll(&cancel, |round| {
        async {
            if let Err(error) =
                intake_once(&store, &source, &workspace, &SystemClock, &UuidV7).await
            {
                tracing::warn!(%error, "intake round failed");
            }
        }
        .instrument(tracing::info_span!("intake", round))
    })
    .await;
}

/// Links pull requests and comments on skipped tickets every 30 seconds until `cancel`, with the
/// system clock: its own task, so a tracker that hangs on writes never holds up intake. Rounds are
/// dropped at `cancel` as in [`intake_loop`]; a link dropped mid-way is tried again, and linking
/// is idempotent; a comment dropped after Linear posted it may be posted once more.
pub async fn link_loop(store: Store, source: impl TicketSource, cancel: CancellationToken) {
    every_poll(&cancel, |round| {
        async {
            if let Err(error) = link_once(&store, &source, &SystemClock).await {
                tracing::warn!(%error, "link round failed");
            }
            if let Err(error) = comment_once(&store, &source, &SystemClock).await {
                tracing::warn!(%error, "comment round failed");
            }
        }
        .instrument(tracing::info_span!("link", round))
    })
    .await;
}

/// Runs `round(1)`, `round(2)`, … 30 seconds apart until `cancel`, which also drops the round in
/// flight.
async fn every_poll<F: Future<Output = ()>>(
    cancel: &CancellationToken,
    mut round: impl FnMut(u64) -> F,
) {
    for n in 1u64.. {
        if cancel.run_until_cancelled(round(n)).await.is_none()
            || cancel
                .run_until_cancelled(tokio::time::sleep(POLL))
                .await
                .is_none()
        {
            return;
        }
    }
}
