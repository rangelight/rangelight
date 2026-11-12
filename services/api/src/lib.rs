//! The api: the Linear intake loop, the link loop (lanes' pull requests, and why intake skipped a
//! ticket, onto their tickets),
//! the runners' status consumer, the Connect-RPC session API, and sign-in (ADR 0021, ADR 0028,
//! ADR 0031, ADR 0033). `main.rs` only loads config and wires these.
#![deny(missing_docs)]
// connectrpc's generated traits return `impl Encodable<M>`; handlers name the concrete message.
#![allow(refining_impl_trait_internal, refining_impl_trait_reachable)]

pub mod agent_events;
pub mod auth;
pub mod config;
mod intake;
pub mod nats_auth;
mod sessions;
pub mod status;

use std::sync::Arc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rl_app::TicketError;
use rl_store::{Store, StoreError};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub use auth::Auth;
pub use intake::{
    check_intake_label, comment_once, intake_loop, intake_once, link_loop, link_once,
};
pub use sessions::{SessionApi, terminals_client, viewers_client};

use config::Config;
use nats_auth::{Callout, CalloutError, Settings, Verifier};
use rl_config::SecretRef;

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

/// The api's own NATS clients, each signed in as the api: `reports` consumes the runners'
/// status and the lanes' agent events, `viewers` follows sessions for the Workbench (`viewers_client`), `terminals` bridges
/// the lane terminals (`terminals_client`).
#[derive(Debug, Clone)]
pub struct Nats {
    /// For the status and agent event consumers.
    pub reports: async_nats::Client,
    /// For the session streams.
    pub viewers: async_nats::Client,
    /// For the lane terminals.
    pub terminals: async_nats::Client,
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
    let terminals = terminals_client(&config.nats_url, options())
        .await
        .map_err(nats)?;
    Ok((
        Nats {
            reports,
            viewers,
            terminals,
        },
        task,
    ))
}

/// Serves sign-in and, behind it, the session and auth APIs (Connect, and gRPC-Web) and the lane
/// terminal's WebSocket over HTTP/1.1
/// on `listener` until `cancel`, then lets open connections finish for up to 10 seconds.
/// `GET /healthz` answers 200 without a login: the api is ready once it serves, after its
/// migrations, Postgres and NATS.
///
/// HTTP/1.1 only: the Workbench speaks Connect over it and the terminal WebSocket needs it;
/// nothing speaks HTTP/2 to the api. A client gets 10 seconds to send a request's headers, from
/// the moment it connects and between requests on a kept-alive connection; a silent one is closed.
pub async fn serve(listener: TcpListener, api: SessionApi, auth: Auth, cancel: CancellationToken) {
    let terminal = sessions::terminal::router(api.clone());
    let rpc = connectrpc::Router::new()
        .add_service(Arc::new(api))
        .add_service(Arc::new(auth::WhoAmI))
        .into_axum_router()
        .merge(terminal);
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
