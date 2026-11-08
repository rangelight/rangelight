//! The api binary: loads config, starts telemetry, migrates, wires Postgres, NATS, Linear and the
//! OIDC provider, and runs the intake loop and the server until SIGTERM or Ctrl-C.

use anyhow::Context;
use api::config::Config;
use std::time::Duration;

use api::{Auth, SessionApi, intake_loop, serve};
use rl_app::{SystemClock, UuidV7};
use rl_store::Store;
use rl_tickets_linear::Linear;
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
    store.migrate().await?;
    let nats = async_nats::connect(&config.nats_url)
        .await
        .context("nats")?;
    let linear = Linear::new(config.linear_url, config.linear_team, config.linear_api_key)?;
    let provider = rl_auth::Provider::new(
        config.oidc_issuer,
        config.oidc_client_id,
        config.oidc_client_secret,
        config.public_url.join("/auth/callback")?,
        config.oidc_member_claim,
    )?;
    // The client secret keys the sign-in cookie: a weak one stops the api here.
    provider.check_secret()?;
    let recheck = Duration::from_secs(config.login_recheck_seconds);
    let auth = Auth::new(store.clone(), provider, &config.public_url, recheck);
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

    let intake = tokio::spawn(intake_loop(
        store.clone(),
        linear,
        SystemClock,
        UuidV7,
        cancel.clone(),
    ));
    tracing::info!(listen = %config.listen, "api started");
    serve(
        listener,
        SessionApi::new(store, nats, config.org, auth.clone(), cancel.clone()),
        auth,
        cancel,
    )
    .await;
    intake.await?;
    Ok(())
}
