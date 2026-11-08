//! The `SessionService` handlers.

use buffa::Message;
use connectrpc::{
    ConnectError, ErrorCode, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use futures_util::StreamExt;
use rl_domain::{SessionId, subject};
use rl_proto::rangelight::session::v1alpha1::{
    Event, ListSessionsRequest, ListSessionsResponse, Session, SessionService, WatchSessionRequest,
    WatchSessionResponse,
};
use rl_store::{SessionRow, Store};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::Auth;

/// The most sessions `ListSessions` returns.
const LIST_LIMIT: u16 = 1000;

/// The session API: reads from Postgres, streams from NATS fan-out.
#[derive(Debug, Clone)]
pub struct SessionApi {
    store: Store,
    nats: async_nats::Client,
    org: String,
    auth: Auth,
    cancel: CancellationToken,
}

impl SessionApi {
    /// The API for one org. Open `WatchSession` streams end when `cancel` fires, or when their
    /// login is gone (`auth`).
    #[must_use]
    pub fn new(
        store: Store,
        nats: async_nats::Client,
        org: String,
        auth: Auth,
        cancel: CancellationToken,
    ) -> SessionApi {
        SessionApi {
            store,
            nats,
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
        generation: row.generation,
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

    async fn watch_session(
        &self,
        ctx: RequestContext,
        request: ServiceRequest<'_, WatchSessionRequest>,
    ) -> ServiceResult<ServiceStream<WatchSessionResponse>> {
        // A UUID can't hold `.`, `*` or `>`, so the subject can't be widened.
        let id = Uuid::parse_str(request.session_id).map_err(|_| {
            ConnectError::new(ErrorCode::InvalidArgument, "session_id must be a UUID")
        })?;
        let span = tracing::info_span!("watch_session", session_id = %id);
        let subject = subject::session_events(&self.org, SessionId(id));
        let subscription = self
            .nats
            .subscribe(subject)
            .await
            .map_err(|e| internal("nats_unavailable", &e))?;
        // The subscription is registered before the stream starts, so no later event is missed.
        self.nats
            .flush()
            .await
            .map_err(|e| internal("nats_unavailable", &e))?;
        let decoded = subscription
            .take_until(self.cancel.clone().cancelled_owned())
            .filter_map(move |message| {
                let event = Event::decode_from_slice(&message.payload);
                if let Err(error) = &event {
                    span.in_scope(
                        || tracing::warn!(%error, "skipped an event that doesn't decode"),
                    );
                }
                std::future::ready(event.ok())
            });
        let events =
            futures_util::stream::unfold(Some((Box::pin(decoded), None)), |state| async move {
                let (mut decoded, mut last) = state?;
                while let Some(event) = decoded.next().await {
                    match step(last, event.seq) {
                        Step::Skip => {}
                        Step::Gap => return Some((Err(missed()), None)),
                        Step::Send => {
                            last = Some(event.seq);
                            let response = WatchSessionResponse {
                                event: event.into(),
                                ..Default::default()
                            };
                            return Some((Ok(response), Some((decoded, last))));
                        }
                    }
                }
                None
            });
        Response::stream_ok(self.auth.while_signed_in(&ctx, events)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_sends_in_order_skips_repeats_and_stops_at_a_gap() {
        assert_eq!(step(None, 7), Step::Send);
        assert_eq!(step(Some(7), 8), Step::Send);
        assert_eq!(step(Some(7), 7), Step::Skip);
        assert_eq!(step(Some(7), 3), Step::Skip);
        assert_eq!(step(Some(7), 9), Step::Gap);
    }
}
