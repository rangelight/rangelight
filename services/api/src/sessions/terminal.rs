//! The lane terminal (ADR 0040 §1): a WebSocket at `/v1alpha1/sessions/<id>/terminal`, behind the
//! same sign-in as every stream, bridged to the lane agent's PTY over core NATS
//! (`rl.<org>.pty.<session>.in` and `.out`, api/README.md "Terminal").
//!
//! Every frame is held to `api/jsonschema/terminal.v1alpha1.schema.json`: one that isn't an
//! `input` or `resize` closes the socket with 1008, and an `output` from the agent that isn't one
//! is dropped. Frames go through unchanged and are never logged, at any level: while `/login`
//! runs they carry the sign-in code (ADR 0046 §3 step 4).
//!
//! Only the session's own member may open its terminal (the agent acts for them, ADR 0004), at
//! most [`PER_LOGIN`] at once per login. What the api holds for a terminal is bounded: its output
//! subscription queues at most `TERMINAL_CAPACITY` messages (more are dropped), and a browser that
//! takes longer than [`SEND_TIMEOUT`] to take a frame is closed with 1011.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{Extension, Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use connectrpc::{ConnectError, ErrorCode};
use futures_util::StreamExt;
use rl_domain::{Member, SessionId, subject};
use serde::Deserialize;

use super::{SessionApi, session_id};
use crate::access::Access;
use crate::auth::LoginHash;

/// The WebSocket subprotocol of the terminal protocol; a client must offer it.
pub(crate) const PROTOCOL: &str = "terminal.v1alpha1.rangelight.dev";

/// The schema's limits: base64 `data` of 4 to 65 536 characters, sizes of 1 to 1000 cells. A test
/// holds these to the schema.
const DATA_MIN: usize = 4;
const DATA_MAX: usize = 65_536;
const CELLS: std::ops::RangeInclusive<u16> = 1..=1000;
/// The largest message the socket reads: twice the largest valid frame, so an oversized frame is
/// read whole and refused with 1008 rather than buffered without bound.
const MESSAGE_MAX: usize = 2 * DATA_MAX;

/// The most terminals one login holds open at once.
const PER_LOGIN: usize = 4;
/// How long a browser gets to take one frame before its terminal closes with 1011.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Open terminals per login, held to [`PER_LOGIN`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Open(Arc<Mutex<HashMap<LoginHash, usize>>>);

impl Open {
    /// A slot for one more terminal of `login`, freed when dropped; `None` at [`PER_LOGIN`].
    fn take(&self, login: LoginHash) -> Option<Slot> {
        let mut open = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(login).or_default();
        if *count >= PER_LOGIN {
            return None;
        }
        *count += 1;
        Some(Slot {
            open: self.clone(),
            login,
        })
    }
}

/// One open terminal of a login, counted in [`Open`] until dropped.
#[derive(Debug)]
struct Slot {
    open: Open,
    login: LoginHash,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut open = self.open.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(&self.login) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.login);
            }
        }
    }
}

/// One frame of the terminal protocol, as the schema has it.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum Frame {
    Input { data: String },
    Output { data: String },
    Resize { cols: u16, rows: u16 },
}

impl Frame {
    /// The frame `text` holds, or `None` if it breaks the schema.
    pub(crate) fn parse(text: &[u8]) -> Option<Frame> {
        let frame: Frame = serde_json::from_slice(text).ok()?;
        let valid = match &frame {
            Frame::Input { data } | Frame::Output { data } => base64(data),
            Frame::Resize { cols, rows } => CELLS.contains(cols) && CELLS.contains(rows),
        };
        valid.then_some(frame)
    }
}

/// The schema's `bytes`: standard base64 with padding, 4 to 65 536 characters.
fn base64(data: &str) -> bool {
    let bytes = data.as_bytes();
    let padding = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    (DATA_MIN..=DATA_MAX).contains(&bytes.len())
        && bytes.len().is_multiple_of(4)
        && padding <= 2
        && bytes[..bytes.len() - padding]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// The terminal's route. `require_login` and the same-origin check run in front of it.
pub(crate) fn router(api: SessionApi) -> axum::Router {
    axum::Router::new()
        .route("/v1alpha1/sessions/{id}/terminal", get(open))
        .with_state(api)
}

/// Upgrades a signed-in member's request for their own session, offering [`PROTOCOL`]: 404 for a
/// session that doesn't exist, 403 for another member's, 429 past [`PER_LOGIN`].
async fn open(
    State(api): State<SessionApi>,
    Path(id): Path<String>,
    // What the login check and the access log put on the request.
    (Extension(login), Extension(member), Extension(access)): (
        Extension<LoginHash>,
        Extension<Member>,
        Extension<Access>,
    ),
    ws: WebSocketUpgrade,
) -> Response {
    let ws = ws
        .protocols([PROTOCOL])
        .max_message_size(MESSAGE_MAX)
        .max_frame_size(MESSAGE_MAX);
    if ws.selected_protocol().is_none() {
        return ConnectError::invalid_argument("subprotocol_required").into_response();
    }
    let session = match session_id(&id) {
        Ok(session) => session,
        Err(error) => return error.into_response(),
    };
    match api.store.get_session(session).await {
        // `Member` is the login, lowercased; a session's member is `api.member`, set to it.
        Ok(Some(row)) if row.member == member.as_str() => {}
        Ok(Some(_)) => return ConnectError::permission_denied("not_your_session").into_response(),
        Ok(None) => {
            return ConnectError::new(ErrorCode::NotFound, "session_not_found").into_response();
        }
        Err(error) => return super::internal("store_unavailable", &error).into_response(),
    }
    let Some(slot) = api.open.take(login) else {
        return ConnectError::new(ErrorCode::ResourceExhausted, "too_many_terminals")
            .into_response();
    };
    // Subscribed before the upgrade answers, so nothing the agent writes after it is missed.
    let output = match api
        .terminals
        .subscribe(subject::pty_out(&api.org, session))
        .await
    {
        Ok(output) => output,
        Err(error) => return super::internal("nats_unavailable", &error).into_response(),
    };
    ws.on_upgrade(move |socket| async move {
        if let Some(left_out) = access.allow() {
            tracing::info!(session = %session.0, member = member.as_str(), client = access.client, left_out, "terminal opened");
        }
        let reason = bridge(api, session, login, output, socket).await;
        drop(slot);
        if let Some(left_out) = access.allow() {
            tracing::info!(session = %session.0, client = access.client, left_out, reason, "terminal closed");
        }
    })
}

/// Why a terminal closed: its close code and reason, the only thing logged about it.
type Closed = (u16, &'static str);

/// Carries frames both ways until either side ends, the login ends, or the api shuts down, then
/// closes with why, and returns why: the only thing logged about it.
async fn bridge(
    api: SessionApi,
    session: SessionId,
    login: LoginHash,
    output: async_nats::Subscriber,
    mut socket: WebSocket,
) -> &'static str {
    let (code, reason) = relay(&api, session, login, output, &mut socket).await;
    let close = CloseFrame {
        code,
        reason: Utf8Bytes::from_static(reason),
    };
    // Bounded too: a browser that stopped reading never takes it.
    let _ = tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Close(Some(close)))).await;
    reason
}

/// The frames of [`bridge`], until why it closes.
async fn relay(
    api: &SessionApi,
    session: SessionId,
    login: LoginHash,
    mut output: async_nats::Subscriber,
    socket: &mut WebSocket,
) -> Closed {
    let input = subject::pty_in(&api.org, session);
    let mut ended = std::pin::pin!(api.auth.clone().ended(login));
    loop {
        let step = tokio::select! {
            () = api.cancel.cancelled() => Err((close_code::AWAY, "shutting_down")),
            _ = &mut ended => Err((close_code::POLICY, "unauthenticated")),
            message = output.next() => to_browser(socket, session, message).await,
            frame = socket.recv() => to_agent(&api.nats, &input, frame).await,
        };
        if let Err(closed) = step {
            return closed;
        }
    }
}

/// Sends the agent's `message` to the browser if it is a valid output frame: the lane is untrusted,
/// anything else is dropped.
async fn to_browser(
    socket: &mut WebSocket,
    session: SessionId,
    message: Option<async_nats::Message>,
) -> Result<(), Closed> {
    let message = message.ok_or((close_code::ERROR, "nats_unavailable"))?;
    let text = std::str::from_utf8(&message.payload).ok();
    let Some(text) =
        text.filter(|t| matches!(Frame::parse(t.as_bytes()), Some(Frame::Output { .. })))
    else {
        tracing::debug!(session = %session.0, "dropped an agent frame that isn't output");
        return Ok(());
    };
    match tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(text.into()))).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err((close_code::AWAY, "browser_gone")),
        Err(_) => Err((close_code::ERROR, "browser_too_slow")),
    }
}

/// Publishes the browser's `frame` to the agent if it is an `input` or `resize`; anything else
/// closes the terminal.
async fn to_agent(
    nats: &async_nats::Client,
    input: &str,
    frame: Option<Result<Message, axum::Error>>,
) -> Result<(), Closed> {
    match frame {
        Some(Ok(Message::Text(text))) => {
            if !matches!(
                Frame::parse(text.as_bytes()),
                Some(Frame::Input { .. } | Frame::Resize { .. })
            ) {
                return Err((close_code::POLICY, "invalid_frame"));
            }
            nats.publish(input.to_owned(), text.into())
                .await
                .map_err(|_| (close_code::ERROR, "nats_unavailable"))
        }
        // Oversized, binary or broken: the protocol is text frames of the schema only.
        Some(Ok(Message::Binary(_)) | Err(_)) => Err((close_code::POLICY, "invalid_frame")),
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(()),
        Some(Ok(Message::Close(_))) | None => Err((close_code::NORMAL, "browser_closed")),
    }
}

#[cfg(test)]
mod tests;
