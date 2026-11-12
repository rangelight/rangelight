//! The relay and the api each do only their own part, as they really run.

use std::path::Path;
use std::time::Duration;

use async_nats::jetstream::{consumer::pull, stream};
use buffa::Message;
use futures_util::StreamExt;
use rl_domain::{SessionId, subject};
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::{Envelope, Phase, SessionStatus};
use tokio_util::sync::CancellationToken;

use super::{ORG, World, world};

/// AC2: the relay publishes desired state and events but can't consume the runners' status; the
/// api consumes status but can't publish desired state.
#[tokio::test]
async fn ac2_the_relay_cannot_consume_status_nor_the_api_publish_desired() {
    let world = world("nats_auth::control_plane").await;
    let api = world.api().await;
    api.jetstream()
        .create_or_update_stream(stream::Config {
            name: subject::status_stream(ORG),
            subjects: vec![subject::runners_status(ORG)],
            ..Default::default()
        })
        .await
        .unwrap();
    let mut relay = world.relay().await;
    let consumer = relay
        .jetstream()
        .create_consumer_on_stream(
            pull::Config {
                durable_name: Some(subject::STATUS_CONSUMER.into()),
                filter_subject: subject::runners_status(ORG),
                ..Default::default()
            },
            subject::status_stream(ORG),
        )
        .await;
    assert!(consumer.is_err(), "the relay consumes status");
    let status = subject::runners_status(ORG);
    let _ = relay.client.subscribe(status.clone()).await.unwrap();
    relay.refused(&status).await;

    let mut api = api;
    let desired = subject::runner_desired(ORG, "r1", SessionId(uuid::Uuid::now_v7()));
    api.client
        .publish(desired.clone(), "x".into())
        .await
        .unwrap();
    api.refused(&desired).await;
}

/// A client of `world`'s server signed in by the token file of `namespace:account`, as the
/// components connect (`rl_nats::options`).
async fn component(world: &World, dir: &Path, account: &str, inbox: &str) -> async_nats::Client {
    let path = dir.join(account);
    std::fs::write(&path, world.home.token("rangelight-system", account)).unwrap();
    rl_nats::options(path, inbox)
        .connect(&world.nats.url)
        .await
        .unwrap()
}

/// Runner r1 takes `session`'s desired state through its own durable consumer, and reports it
/// running.
async fn run_at_r1(world: &World, session: SessionId) {
    let runner = world.runner(true).await;
    let js = runner.jetstream();
    let stream = js
        .get_stream_no_info(subject::desired_stream(ORG))
        .await
        .unwrap();
    let consumer = stream
        .create_consumer(pull::Config {
            durable_name: Some(subject::desired_consumer("r1")),
            filter_subject: format!("rl.{ORG}.runner.r1.desired.*"),
            ..Default::default()
        })
        .await
        .unwrap();
    let mut batch = consumer.fetch().max_messages(1).messages().await.unwrap();
    let desired = batch.next().await.unwrap().unwrap();
    assert_eq!(
        desired.subject.as_str(),
        subject::runner_desired(ORG, "r1", session)
    );
    desired.ack().await.unwrap();
    let report = Envelope {
        message_id: uuid::Uuid::now_v7().to_string(),
        protocol_version: "v1alpha1".into(),
        runner_id: "r1".into(),
        body: Some(Body::from(SessionStatus {
            session_id: session.0.to_string(),
            phase: Phase::PHASE_RUNNING.into(),
            ..Default::default()
        })),
        ..Default::default()
    };
    let ack = js.publish(
        subject::runner_status(ORG, "r1"),
        report.encode_to_vec().into(),
    );
    ack.await.unwrap().await.unwrap();
}

/// AC1 and AC2: the relay, the api's status consumer and the runner protocol, as they really run,
/// each signed in as itself and within its permissions: a session's desired state reaches the
/// runner, and its report reaches Postgres.
#[tokio::test]
async fn ac1_each_component_signs_in_as_itself_and_runs_within_its_permissions() {
    let world = world("nats_auth::components").await;
    let dir = tempfile::tempdir().unwrap();
    let relay_nats = component(&world, dir.path(), "rangelight-relay", "relay").await;
    let api_nats = component(&world, dir.path(), "rangelight-api", "api").await;
    let session = world.session(11).await;
    let relay = relay::Relay::new(world.store.clone(), relay_nats, ORG.into(), "r1".into(), 1);
    assert_eq!(relay.await.unwrap().run_once().await.unwrap(), 1);
    let stop = CancellationToken::new();
    let recorder = api::status::Recorder::new(world.store.clone(), None);
    let reports = tokio::spawn(api::status::consume(
        recorder,
        api_nats,
        ORG.into(),
        1,
        stop.clone(),
    ));

    run_at_r1(&world, session).await;
    let mut phase = None;
    for _ in 0..100 {
        phase = world
            .store
            .get_session(session)
            .await
            .unwrap()
            .unwrap()
            .phase;
        if phase.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        phase,
        Some(rl_domain::Phase::Running),
        "the report reached Postgres"
    );
    stop.cancel();
    reports.await.unwrap();
}
