//! The `SessionService` handlers.

use std::collections::HashMap;
use std::pin::Pin;

use buffa::Message;
use connectrpc::{
    ConnectError, ErrorCode, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use futures_util::{Stream, StreamExt};
use rl_domain::{SessionId, subject};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::watch_sessions_response;
use rl_proto::rangelight::session::v1alpha1::{
    Event, GetSessionRequest, GetSessionResponse, ListSessionsRequest, ListSessionsResponse,
    Session, SessionService, Snapshot, WatchSessionRequest, WatchSessionResponse,
    WatchSessionsRequest, WatchSessionsResponse,
};
use rl_store::{SessionRow, Store};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::Auth;

/// The most sessions `ListSessions` and a `WatchSessions` snapshot return.
const LIST_LIMIT: u16 = 1000;

/// How many events a viewer's stream may fall behind before it ends with ABORTED: the subscription
/// capacity of the viewers' NATS client.
const VIEWER_CAPACITY: usize = 1024;

/// The NATS client for viewers' streams, separate from the api's other subscriptions (runner
/// reports): its subscription capacity bounds how far a viewer may lag ([`VIEWER_CAPACITY`]).
/// `options` sign it in.
///
/// # Errors
/// NATS's, when it can't connect.
pub async fn viewers_client(
    url: &str,
    options: async_nats::ConnectOptions,
) -> Result<async_nats::Client, async_nats::ConnectError> {
    options
        .subscription_capacity(VIEWER_CAPACITY)
        .connect(url)
        .await
}

/// How many messages a lane terminal's output may queue in the api: past it, output is dropped
/// (the subscription capacity of the terminals' NATS client), so a lane that writes faster than
/// its browser reads can't pin the api's memory.
const TERMINAL_CAPACITY: usize = 64;

/// The NATS client for lane terminals, separate from the viewers' one: its subscription capacity
/// bounds what a terminal may queue ([`TERMINAL_CAPACITY`]). `options` sign it in.
///
/// # Errors
/// NATS's, when it can't connect.
pub async fn terminals_client(
    url: &str,
    options: async_nats::ConnectOptions,
) -> Result<async_nats::Client, async_nats::ConnectError> {
    options
        .subscription_capacity(TERMINAL_CAPACITY)
        .connect(url)
        .await
}

/// The session API: reads from Postgres, streams from NATS fan-out.
#[derive(Debug, Clone)]
pub struct SessionApi {
    store: Store,
    nats: async_nats::Client,
    terminals: async_nats::Client,
    open: terminal::Open,
    org: String,
    auth: Auth,
    cancel: CancellationToken,
}

impl SessionApi {
    /// The API for one org, on the api's `viewers` and `terminals` clients. Open streams end when
    /// `cancel` fires, or when their login is gone (`auth`).
    #[must_use]
    pub fn new(
        store: Store,
        nats: &crate::Nats,
        org: String,
        auth: Auth,
        cancel: CancellationToken,
    ) -> SessionApi {
        SessionApi {
            store,
            nats: nats.viewers.clone(),
            terminals: nats.terminals.clone(),
            open: terminal::Open::default(),
            org,
            auth,
            cancel,
        }
    }
}

/// Logs the cause and answers with a stable code and reason, never internal details.
fn internal(reason: &'static str, error: &dyn std::fmt::Display) -> ConnectError {
    tracing::error!(%error, reason, "request failed");
    ConnectError::new(ErrorCode::Unavailable, reason)
}

fn to_proto(row: SessionRow) -> Session {
    Session {
        id: row.id.0.to_string(),
        tracker: row.tracker,
        ticket_id: row.ticket_id,
        ticket_key: row.ticket_key,
        title: row.title,
        namespace: row.namespace,
        state: rl_chain::desired_state(row.state).into(),
        phase: row
            .phase
            .map(rl_chain::proto_phase)
            .unwrap_or_default()
            .into(),
        generation: row.generation,
        repo: row.repo,
        created_at: buffa_types::google::protobuf::Timestamp::from(row.created_at).into(),
        ticket_url: row.ticket_url,
        pull_request_url: row.pull_request_url.unwrap_or_default(),
        ..Default::default()
    }
}

/// What a watch does with the next event, given the seq of the last one it sent.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The next in order, or the first: send it.
    Send,
    /// A republish of one already sent: drop it (receivers dedupe on the event id anyway).
    Skip,
    /// Events went missing (a slow consumer, or a fan-out lost on the way): end the stream.
    Gap,
}

fn step(last: Option<u64>, seq: u64) -> Step {
    match last {
        Some(last) if seq <= last => Step::Skip,
        Some(last) if seq > last.saturating_add(1) => Step::Gap,
        _ => Step::Send,
    }
}

/// The stream's end after a gap: the client rereads the session and watches again.
fn missed() -> ConnectError {
    ConnectError::new(
        ErrorCode::Aborted,
        "events_missed: reread the session and watch again",
    )
}

/// A session id from a request: a UUID can't hold `.`, `*` or `>`, so a subject built from it can't
/// be widened.
fn session_id(raw: &str) -> Result<SessionId, ConnectError> {
    Uuid::parse_str(raw)
        .map(SessionId)
        .map_err(|_| ConnectError::new(ErrorCode::InvalidArgument, "session_id must be a UUID"))
}

impl SessionService for SessionApi {
    #[tracing::instrument(name = "list_sessions", skip_all)]
    async fn list_sessions(
        &self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, ListSessionsRequest>,
    ) -> ServiceResult<ListSessionsResponse> {
        let rows = self
            .store
            .list_sessions(LIST_LIMIT)
            .await
            .map_err(|e| internal("store_unavailable", &e))?;
        Response::ok(ListSessionsResponse {
            sessions: rows.into_iter().map(to_proto).collect(),
            ..Default::default()
        })
    }

    #[tracing::instrument(name = "get_session", skip_all)]
    async fn get_session(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, GetSessionRequest>,
    ) -> ServiceResult<GetSessionResponse> {
        let id = session_id(request.session_id)?;
        let row = self
            .store
            .get_session(id)
            .await
            .map_err(|e| internal("store_unavailable", &e))?
            .ok_or_else(|| ConnectError::new(ErrorCode::NotFound, "session_not_found"))?;
        Response::ok(GetSessionResponse {
            session: to_proto(row).into(),
            ..Default::default()
        })
    }

    async fn watch_session(
        &self,
        ctx: RequestContext,
        request: ServiceRequest<'_, WatchSessionRequest>,
    ) -> ServiceResult<ServiceStream<WatchSessionResponse>> {
        let id = session_id(request.session_id)?;
        let span = tracing::info_span!("watch_session", session_id = %id.0);
        let subscription = self
            .subscribe(subject::session_events(&self.org, id), span)
            .await?;
        // The stream starts with the next event, whatever its seq: there is nothing to check it
        // against until one arrives.
        let events = ordered(subscription, HashMap::new(), None).map(|event| {
            event.map(|event| WatchSessionResponse {
                event: event.into(),
                ..Default::default()
            })
        });
        Response::stream_ok(self.auth.while_signed_in(&ctx, events)?)
    }

    async fn watch_sessions(
        &self,
        ctx: RequestContext,
        _request: ServiceRequest<'_, WatchSessionsRequest>,
    ) -> ServiceResult<ServiceStream<WatchSessionsResponse>> {
        let span = tracing::info_span!("watch_sessions");
        let subscription = self
            .subscribe(subject::sessions_events(&self.org), span)
            .await?;
        let store = self.store.clone();
        let stream = futures_util::stream::once(async move {
            let snapshot = match read_snapshot(&store, &subscription).await {
                Ok(snapshot) => snapshot,
                Err(e) => return Box::pin(futures_util::stream::iter([Err(e)])) as Changes,
            };
            let seen = snapshot
                .generations
                .into_iter()
                .map(|(id, generation)| (id.0.to_string(), generation))
                .collect();
            let first = WatchSessionsResponse {
                kind: Some(watch_sessions_response::Kind::from(Snapshot {
                    sessions: snapshot.sessions.into_iter().map(to_proto).collect(),
                    ..Default::default()
                })),
                ..Default::default()
            };
            // A session the snapshot didn't hold is new: its first change must be seq 1.
            let changes = ordered(subscription, seen, Some(0)).map(|event| {
                event.map(|event| WatchSessionsResponse {
                    kind: Some(watch_sessions_response::Kind::from(event)),
                    ..Default::default()
                })
            });
            Box::pin(futures_util::stream::iter([Ok(first)]).chain(changes)) as Changes
        })
        .flatten();
        Response::stream_ok(self.auth.while_signed_in(&ctx, stream)?)
    }
}

/// What `WatchSessions` sends after it has read the snapshot.
type Changes = ServiceStream<WatchSessionsResponse>;

/// The decoded events of a NATS subscription that was registered (flushed) when this was made.
/// Postgres reads that must not miss an event take one by reference, so they can only run after
/// the subscription exists.
struct Subscription(Pin<Box<dyn Stream<Item = Event> + Send + Sync>>);

/// The snapshot a `WatchSessions` stream starts from. It takes the subscription so it can't be read
/// before one is open: a change committed in between is then in both, and `ordered` drops the copy.
async fn read_snapshot(
    store: &Store,
    _subscribed: &Subscription,
) -> Result<rl_store::Snapshot, ConnectError> {
    store
        .snapshot(LIST_LIMIT)
        .await
        .map_err(|e| internal("store_unavailable", &e))
}

impl SessionApi {
    async fn subscribe(
        &self,
        subject: String,
        span: tracing::Span,
    ) -> Result<Subscription, ConnectError> {
        subscribe(&self.nats, &self.cancel, subject, span).await
    }
}

/// Subscribes to `subject` and flushes, so no event committed after this returns is missed. The
/// stream ends at `cancel`. The client's subscription capacity bounds how far a viewer may lag
/// ([`VIEWER_CAPACITY`]); events beyond it are dropped, and the gap ends the stream with ABORTED.
async fn subscribe(
    nats: &async_nats::Client,
    cancel: &CancellationToken,
    subject: String,
    span: tracing::Span,
) -> Result<Subscription, ConnectError> {
    let subscription = nats
        .subscribe(subject)
        .await
        .map_err(|e| internal("nats_unavailable", &e))?;
    nats.flush()
        .await
        .map_err(|e| internal("nats_unavailable", &e))?;
    let events = subscription
        .take_until(cancel.clone().cancelled_owned())
        .filter_map(move |message| {
            let event = Event::decode_from_slice(&message.payload);
            if let Err(error) = &event {
                span.in_scope(|| tracing::warn!(%error, "skipped an event that doesn't decode"));
            }
            std::future::ready(event.ok())
        });
    Ok(Subscription(Box::pin(events)))
}

/// Each session's events in order: the next one is sent, a repeat or one at or below the
/// session's entry in `seen` is dropped, and a gap ends the stream with ABORTED. A session not in
/// `seen` starts at `unknown`: `None` takes whatever comes first, `Some(0)` wants seq 1.
fn ordered(
    subscription: Subscription,
    seen: HashMap<String, u64>,
    unknown: Option<u64>,
) -> impl Stream<Item = Result<Event, ConnectError>> + Send + 'static {
    futures_util::stream::unfold(Some((subscription.0, seen)), move |state| async move {
        let (mut events, mut seen) = state?;
        while let Some(event) = events.next().await {
            match step(seen.get(&event.session_id).copied().or(unknown), event.seq) {
                Step::Skip => {}
                Step::Gap => return Some((Err(missed()), None)),
                Step::Send => {
                    seen.insert(event.session_id.clone(), event.seq);
                    return Some((Ok(event), Some((events, seen))));
                }
            }
        }
        None
    })
}

pub(crate) mod terminal;
#[cfg(test)]
mod tests;
