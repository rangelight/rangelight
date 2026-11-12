//! The api: the Linear intake loop, the link loop (lanes' pull requests onto their tickets),
//! the runners' status consumer, the Connect-RPC session API, and sign-in (ADR 0021, ADR 0028,
//! ADR 0031, ADR 0033). `main.rs` only loads config and wires these.
#![deny(missing_docs)]
// connectrpc's generated traits return `impl Encodable<M>`; handlers name the concrete message.
#![allow(refining_impl_trait_internal, refining_impl_trait_reachable)]

pub mod auth;
pub mod config;
pub mod nats_auth;
mod sessions;
pub mod status;

use std::sync::Arc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rl_app::{Clock, IdGen, SystemClock, TicketError, TicketSource, UuidV7, intake};
use rl_domain::Workspace;
use rl_store::{SessionRow, Store, StoreError};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Instrument;

pub use auth::Auth;
pub use sessions::{SessionApi, viewers_client};

use config::Config;
use nats_auth::{Callout, CalloutError, Settings, Verifier};
use rl_config::SecretRef;

/// The pause between two intake polls.
const POLL: Duration = Duration::from_secs(30);
/// The most pull requests one round links.
const LINKS: u16 = 10;
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

/// Links pull requests every 30 seconds until `cancel`, with the system clock: its own task, so a
/// tracker that hangs on links never holds up intake. Rounds are dropped at `cancel` as in
/// [`intake_loop`]; a link dropped mid-way is tried again, and linking is idempotent.
pub async fn link_loop(store: Store, source: impl TicketSource, cancel: CancellationToken) {
    every_poll(&cancel, |round| {
        async {
            if let Err(error) = link_once(&store, &source, &SystemClock).await {
                tracing::warn!(%error, "link round failed");
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

/// The api's own NATS clients, each signed in as the api: `reports` consumes the runners'
/// status, `viewers` follows sessions for the Workbench (`viewers_client`).
#[derive(Debug, Clone)]
pub struct Nats {
    /// For the status consumer.
    pub reports: async_nats::Client,
    /// For the session streams.
    pub viewers: async_nats::Client,
}

/// Starts the NATS auth callout, which answers until `cancel`, then signs the api's own clients
/// in through it: every NATS client but the callout's own connection signs in there.
///
/// # Errors
/// [`CalloutError`] when a secret, a key or NATS fails.
pub async fn connect_nats(
    config: &Config,
    store: Store,
    cancel: CancellationToken,
) -> Result<(Nats, tokio::task::JoinHandle<()>), CalloutError> {
    let auth = &config.nats_auth;
    let callout_nats = Callout::connect(&config.nats_url, &auth.auth_user.read()?).await?;
    let settings = Settings {
        org: config.org.clone(),
        sessions_runner: config.runner.clone(),
        account: auth.account.clone(),
        lane_account: auth.lane_account.clone(),
        issuer_seed: auth.issuer_seed.read()?,
        lane_issuer_seed: auth
            .lane_issuer_seed
            .as_ref()
            .map(SecretRef::read)
            .transpose()?,
    };
    let verifier = Verifier::new(auth.issuers.clone())?;
    let callout = Callout::new(callout_nats, store, verifier, settings)?;
    let requests = callout.listen().await?;
    let task = tokio::spawn(std::sync::Arc::new(callout).serve(requests, cancel));
    let options = || rl_nats::options(config.nats_token_file.clone(), "api");
    let nats = |e: async_nats::ConnectError| CalloutError::Nats(e.to_string());
    let reports = options().connect(&config.nats_url).await.map_err(nats)?;
    let viewers = viewers_client(&config.nats_url, options())
        .await
        .map_err(nats)?;
    Ok((Nats { reports, viewers }, task))
}

/// Serves sign-in and, behind it, the session and auth APIs (Connect, and gRPC-Web) over HTTP/1.1
/// on `listener` until `cancel`, then lets open connections finish for up to 10 seconds.
/// `GET /healthz` answers 200 without a login: the api is ready once it serves, after its
/// migrations, Postgres and NATS.
///
/// HTTP/1.1 only: the Workbench speaks Connect over it and the terminal WebSocket needs it;
/// nothing speaks HTTP/2 to the api. A client gets 10 seconds to send a request's headers, from
/// the moment it connects and between requests on a kept-alive connection; a silent one is closed.
pub async fn serve(listener: TcpListener, api: SessionApi, auth: Auth, cancel: CancellationToken) {
    let rpc = connectrpc::Router::new()
        .add_service(Arc::new(api))
        .add_service(Arc::new(auth::WhoAmI))
        .into_axum_router();
    let app = axum::Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .merge(auth::router(auth, rpc));
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
