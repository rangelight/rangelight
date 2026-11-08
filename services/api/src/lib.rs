//! The api: the Linear intake loop, the runners' status consumer, the Connect-RPC session API,
//! and sign-in (ADR 0021, ADR 0028, ADR 0031, ADR 0033). `main.rs` only loads config and wires
//! these.
#![deny(missing_docs)]
// connectrpc's generated traits return `impl Encodable<M>`; handlers name the concrete message.
#![allow(refining_impl_trait_internal, refining_impl_trait_reachable)]

pub mod auth;
pub mod config;
mod sessions;
pub mod status;

use std::sync::Arc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rl_app::{Clock, IdGen, SystemClock, TicketError, TicketSource, UuidV7, intake};
use rl_domain::Workspace;
use rl_store::{Store, StoreError};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Instrument;

pub use auth::Auth;
pub use sessions::{SessionApi, viewers_client};

/// The pause between two intake polls.
const POLL: Duration = Duration::from_secs(30);
/// A client's time to send a request's headers; also the shutdown grace.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the api or one of its rounds failed.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The ticket tracker failed.
    #[error(transparent)]
    Tickets(#[from] TicketError),
    /// Postgres failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// One intake round: lists the tickets, and creates a session in `workspace` for each that has
/// none yet.
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
    for command in intake(source, workspace, clock, ids).await? {
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

/// Polls every 30 seconds until `cancel`, with the system clock and UUIDv7 ids. A failed round is logged and retried next poll. A
/// round in flight is dropped at `cancel`, so shutdown never waits on Linear or Postgres; each
/// command is one transaction, so a dropped round leaves nothing half-written.
pub async fn intake_loop(
    store: Store,
    source: impl TicketSource,
    workspace: Workspace,
    cancel: CancellationToken,
) {
    for round in 1u64.. {
        let once = async {
            if let Err(error) =
                intake_once(&store, &source, &workspace, &SystemClock, &UuidV7).await
            {
                tracing::warn!(%error, "intake round failed");
            }
        };
        let once = once.instrument(tracing::info_span!("intake", round));
        if cancel.run_until_cancelled(once).await.is_none()
            || cancel
                .run_until_cancelled(tokio::time::sleep(POLL))
                .await
                .is_none()
        {
            return;
        }
    }
}

/// Serves sign-in and, behind it, the session and auth APIs (Connect, and gRPC-Web) over HTTP/1.1
/// on `listener` until `cancel`, then lets open connections finish for up to 10 seconds.
///
/// HTTP/1.1 only: the Workbench speaks Connect over it and the terminal WebSocket needs it;
/// nothing speaks HTTP/2 to the api. A client gets 10 seconds to send a request's headers, from
/// the moment it connects and between requests on a kept-alive connection; a silent one is closed.
pub async fn serve(listener: TcpListener, api: SessionApi, auth: Auth, cancel: CancellationToken) {
    let rpc = connectrpc::Router::new()
        .add_service(Arc::new(api))
        .add_service(Arc::new(auth::WhoAmI))
        .into_axum_router();
    let app = auth::router(auth, rpc);
    let mut http = http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(CLIENT_TIMEOUT)
        .keep_alive(true);
    let connections = TaskTracker::new();
    while let Some(stream) = cancel.run_until_cancelled(accept(&listener)).await {
        let service = TowerToHyperService::new(app.clone());
        let connection = http
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades();
        let cancel = cancel.clone();
        connections.spawn(async move {
            let mut connection = std::pin::pin!(connection);
            // At shutdown: finish the request in flight, then close; 10 seconds at most.
            if cancel
                .run_until_cancelled(connection.as_mut())
                .await
                .is_none()
            {
                connection.as_mut().graceful_shutdown();
                let _ = tokio::time::timeout(CLIENT_TIMEOUT, connection).await;
            }
        });
    }
    connections.close();
    connections.wait().await;
}

/// The next connection. A failed accept (out of file descriptors, say) is logged and retried
/// after a pause rather than spun on.
async fn accept(listener: &TcpListener) -> TcpStream {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => return stream,
            Err(error) => tracing::warn!(%error, "accept failed"),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
