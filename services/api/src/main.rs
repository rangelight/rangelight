//! The api binary: loads config, starts telemetry, migrates, starts the NATS auth callout, wires
//! Postgres, NATS, Linear and OIDC, and runs the intake and link loops, the status consumer and the
//! server until SIGTERM or Ctrl-C.

use anyhow::Context;
use api::{Auth, SessionApi, config::Config, connect_nats, intake_loop, link_loop, serve, status};
use rl_store::Store;
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    rl_telemetry::init(&config.log)?;
    let (workspace, upstream) = (config.workspace()?, config.upstream()?);
    let roots = config.extra_roots()?;
    let store = Store::connect(&config.database_url.read()?)
        .await
        .context("postgres")?;
    store.migrate().await?;
    let cancel = CancellationToken::new();
    let (nats, callout) = connect_nats(&config, store.clone(), cancel.clone()).await?;
    let linear = config.linear(&roots)?;
    let provider = rl_auth::Provider::new(
        config.oidc_issuer.clone(),
        config.registration()?,
        config.oidc_member_claim.clone(),
        &roots,
    )?;
    // The client secret keys the sign-in cookie: a weak one stops the api here.
    provider.check_secret()?;
    let recheck = std::time::Duration::from_secs(config.login_recheck_seconds);
    let auth = Auth::new(store.clone(), provider, &config.public_url, recheck);
    let listener = tokio::net::TcpListener::bind(config.listen).await?;

    let mut terminate = signal(SignalKind::terminate())?;
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
        stop.cancel();
    });
    let links = tokio::spawn(link_loop(store.clone(), linear.clone(), cancel.clone()));
    let intake = tokio::spawn(intake_loop(
        store.clone(),
        linear,
        workspace,
        cancel.clone(),
    ));
    let reports = tokio::spawn(status::consume(
        status::Recorder::new(store.clone(), upstream),
        nats.reports.clone(),
        config.org.clone(),
        config.stream_replicas,
        cancel.clone(),
    ));
    tracing::info!(listen = %config.listen, "api started");
    let api = SessionApi::new(store, &nats, config.org, auth.clone(), cancel.clone());
    serve(listener, api, auth, cancel).await;
    intake.await?;
    links.await?;
    reports.await?;
    callout.await?;
    Ok(())
}
