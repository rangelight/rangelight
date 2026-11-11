//! The relay binary: loads config, starts telemetry, wires the store and NATS, then serves
//! /healthz and runs until SIGTERM or Ctrl-C.

use anyhow::Context;
use relay::Relay;
use relay::config::Config;
use rl_store::Store;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    rl_telemetry::init(&config.log)?;
    let store = Store::connect(&config.database_url.read()?)
        .await
        .context("postgres")?;
    let nats = async_nats::connect(&config.nats_url)
        .await
        .context("nats")?;
    let relay = Relay::new(
        store,
        nats,
        config.org,
        config.runner,
        config.stream_replicas,
    )
    .await?;
    let listener = TcpListener::bind(config.listen).await?;

    let cancel = CancellationToken::new();
    let mut terminate = signal(SignalKind::terminate())?;
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
        stop.cancel();
    });
    let health = tokio::spawn(relay::serve_health(listener, cancel.clone()));
    tracing::info!(listen = %config.listen, "relay started");
    relay.run(cancel).await;
    health.await??;
    Ok(())
}
