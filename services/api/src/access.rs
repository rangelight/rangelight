//! The api's access log (RAN-81): one line per refused request (an RPC or a terminal upgrade) with
//! its method, status, reason and member, and one line when each stream opens and ends, with why.
//! Never a cookie, a token, a request or response body, or a terminal frame: a refusal's reason is
//! the machine reason of its Connect error, kept only when it is a code like `cross_origin`.
//!
//! Each client may log [`PER_MINUTE`] lines a minute; the rest are counted and the next line says
//! how many were left out, so a client in a loop can't flood the log. A client is the last
//! `X-Forwarded-For` hop, which the proxy in front sets, or else the TCP peer.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use connectrpc::ConnectError;
use futures_util::Stream;
use rl_domain::Member;

/// The most lines one client logs a minute.
pub(crate) const PER_MINUTE: u32 = 30;
const WINDOW: Duration = Duration::from_mins(1);
/// The most clients counted at once; past it the counts start over.
const CLIENTS: usize = 10_000;
/// The largest refusal body read for its reason (a Connect error is a few dozen bytes).
const BODY_MAX: usize = 64 * 1024;

/// The TCP peer of a connection, for requests that came without a proxy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Peer(pub(crate) SocketAddr);

/// Each client's lines in its current minute.
/// ponytail: one counter per client, all cleared at [`CLIENTS`]. Ceiling: a flood from more than
/// 10,000 addresses a minute logs again after each clear. Upgrade: a sampling layer.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lines(Arc<Mutex<HashMap<String, Window>>>);

/// A client's minute: when it started, its lines in it, and the lines left out since its last one.
type Window = (Instant, u32, u32);

impl Lines {
    /// Whether `client` may log a line at `now`, and if so how many of its lines were left out
    /// since its last one.
    pub(crate) fn allow(&self, client: &str, now: Instant) -> Option<u32> {
        let mut clients = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if clients.len() >= CLIENTS && !clients.contains_key(client) {
            clients.clear();
        }
        let (start, count, left_out) = clients.entry(client.to_owned()).or_insert((now, 0, 0));
        if now.duration_since(*start) >= WINDOW {
            (*start, *count) = (now, 0);
        }
        *count += 1;
        if *count > PER_MINUTE {
            *left_out += 1;
            return None;
        }
        Some(std::mem::take(left_out))
    }
}

/// The client of a request and the limiter its lines go through: in every request's extensions,
/// for the streams' lines.
#[derive(Debug, Clone)]
pub(crate) struct Access {
    pub(crate) client: String,
    pub(crate) lines: Lines,
}

impl Access {
    /// Whether this client may log a line now (see [`Lines::allow`]).
    pub(crate) fn allow(&self) -> Option<u32> {
        self.lines.allow(&self.client, Instant::now())
    }
}

/// The client a request comes from: the last `X-Forwarded-For` hop (the proxy in front of the api
/// sets it), or the TCP peer.
fn client(request: &Request) -> String {
    let forwarded = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(str::trim)
        .filter(|hop| !hop.is_empty() && hop.len() <= 64);
    let peer = request
        .extensions()
        .get::<Peer>()
        .map(|peer| peer.0.ip().to_string());
    forwarded.map(str::to_owned).or(peer).unwrap_or_default()
}

/// A refusal's machine reason: its Connect error's message when that is a code (`cross_origin`),
/// else its Connect code, else nothing. Nothing else of the body is read.
pub(crate) fn reason(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Error {
        code: Option<String>,
        message: Option<String>,
    }
    serde_json::from_slice::<Error>(body).map_or_else(
        |_| String::new(),
        |e| reason_of(e.code.as_deref(), e.message.as_deref()),
    )
}

/// The message when it is a code, else the code, if either is one.
fn reason_of(code: Option<&str>, message: Option<&str>) -> String {
    let code_like = |s: &&str| {
        (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    };
    message
        .filter(code_like)
        .or(code.filter(code_like))
        .unwrap_or_default()
        .to_owned()
}

/// The outermost layer: hands each request its [`Access`], and logs every response of status 400
/// or more with its method, path, status, reason and member (when the login check found one).
pub(crate) async fn log_refusals(
    State(lines): State<Lines>,
    mut request: Request,
    next: Next,
) -> Response {
    let access = Access {
        client: client(&request),
        lines,
    };
    let (method, path) = (request.method().clone(), request.uri().path().to_owned());
    request.extensions_mut().insert(access.clone());
    let response = next.run(request).await;
    if response.status().as_u16() < 400 {
        return response;
    }
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, BODY_MAX)
        .await
        .unwrap_or_default();
    if let Some(left_out) = access.allow() {
        let member = parts
            .extensions
            .get::<Member>()
            .map(Member::as_str)
            .unwrap_or_default();
        tracing::info!(
            %method,
            path,
            status = parts.status.as_u16(),
            reason = reason(&body),
            member,
            client = access.client,
            left_out,
            "refused a request"
        );
    }
    Response::from_parts(parts, Body::from(body))
}

/// `stream` of the request `name` (its procedure) from `access`, for `member`, with a line when it
/// opens and one when it ends: completed, ended with an error (its reason), or the client gone.
pub(crate) fn logged<T>(
    stream: impl Stream<Item = Result<T, ConnectError>> + Send + 'static,
    name: String,
    member: String,
    access: Option<Access>,
) -> impl Stream<Item = Result<T, ConnectError>> + Send + 'static {
    let line = Line {
        name,
        member,
        access,
        ended: "client_gone".into(),
    };
    line.log(None);
    futures_util::stream::unfold(
        (Box::pin(stream), line, false),
        |(mut stream, mut line, done)| async move {
            if done {
                return None;
            }
            match futures_util::StreamExt::next(&mut stream).await {
                Some(Ok(item)) => Some((Ok(item), (stream, line, false))),
                Some(Err(error)) => {
                    line.end(reason_of(
                        Some(error.code.as_str()),
                        error.message.as_deref(),
                    ));
                    Some((Err(error), (stream, line, true)))
                }
                None => {
                    // Its end line, as `line` is dropped here.
                    line.end("completed".into());
                    None
                }
            }
        },
    )
}

/// A stream's identity for its lines; its end line is logged when it is dropped.
struct Line {
    name: String,
    member: String,
    access: Option<Access>,
    ended: String,
}

impl Line {
    /// Why it ended, for its end line.
    fn end(&mut self, why: String) {
        self.ended = why;
    }

    /// Its open line, or with `ended` its end line, if its client may log one now.
    fn log(&self, ended: Option<&str>) {
        let Some(left_out) = self.access.as_ref().map_or(Some(0), Access::allow) else {
            return;
        };
        let (stream, member) = (self.name.as_str(), self.member.as_str());
        let client = self
            .access
            .as_ref()
            .map(|a| a.client.as_str())
            .unwrap_or_default();
        let Some(reason) = ended else {
            tracing::info!(stream, member, client, left_out, "stream opened");
            return;
        };
        tracing::info!(stream, member, client, left_out, reason, "stream ended");
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        self.log(Some(&self.ended));
    }
}

#[cfg(test)]
mod tests;
