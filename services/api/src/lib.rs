//! The api: the Linear intake loop and the Connect-RPC session API (ADR 0031, ADR 0033).
//! `main.rs` only loads config and wires these.
#![deny(missing_docs)]
// connectrpc's generated traits return `impl Encodable<M>`; handlers name the concrete message.
#![allow(refining_impl_trait_internal, refining_impl_trait_reachable)]

pub mod config;
mod sessions;

use std::sync::Arc;
use std::time::Duration;

use rl_app::{Clock, IdGen, TicketError, TicketSource, intake};
use rl_store::{Store, StoreError};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

pub use sessions::SessionApi;

/// The pause between two intake polls.
const POLL: Duration = Duration::from_secs(30);

/// Why the api or one of its rounds failed.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The ticket tracker failed.
    #[error(transparent)]
    Tickets(#[from] TicketError),
    /// Postgres failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The Connect server failed.
    #[error("server: {0}")]
    Serve(String),
}

/// One intake round: lists the tickets, and creates a session for each that has none yet.
/// A command that fails (say, a namespace another ticket already holds) is logged and skipped;
/// the round goes on. Returns how many sessions it created.
///
/// # Errors
/// [`ApiError::Tickets`] when the source can't list tickets.
pub async fn intake_once(
    store: &Store,
    source: &impl TicketSource,
    clock: &impl Clock,
    ids: &impl IdGen,
) -> Result<usize, ApiError> {
    let mut created = 0;
    for command in intake(source, clock, ids).await? {
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

/// Polls every 30 seconds until `cancel`. A failed round is logged and retried next poll. A
/// round in flight is dropped at `cancel`, so shutdown never waits on Linear or Postgres; each
/// command is one transaction, so a dropped round leaves nothing half-written.
pub async fn intake_loop(
    store: Store,
    source: impl TicketSource,
    clock: impl Clock,
    ids: impl IdGen,
    cancel: CancellationToken,
) {
    for round in 1u64.. {
        let once = async {
            if let Err(error) = intake_once(&store, &source, &clock, &ids).await {
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

/// Serves the session API (Connect, gRPC and gRPC-Web) on `listener` until `cancel`.
///
/// # Errors
/// [`ApiError::Serve`] if the server fails.
pub async fn serve(
    listener: TcpListener,
    api: SessionApi,
    cancel: CancellationToken,
) -> Result<(), ApiError> {
    let router = connectrpc::Router::new().add_service(Arc::new(api));
    connectrpc::Server::from_listener(listener)
        .serve_with_graceful_shutdown(router, cancel.cancelled_owned())
        .await
        .map_err(|e| ApiError::Serve(e.to_string()))
}
