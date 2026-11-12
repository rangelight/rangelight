//! A runner reaches only its own sessions and its own status.

use async_nats::jetstream::{consumer::pull, stream};
use futures_util::StreamExt;
use rl_domain::{SessionId, subject};

use super::{ORG, world};

/// AC4: runner A reports its own status, never runner B's.
#[tokio::test]
async fn ac4_runner_a_cannot_publish_runner_bs_status() {
    let world = world("nats_auth::status").await;
    let api = world.api().await;
    let stream = subject::status_stream(ORG);
    api.jetstream()
        .create_or_update_stream(stream::Config {
            name: stream.clone(),
            subjects: vec![subject::runners_status(ORG)],
            ..Default::default()
        })
        .await
        .unwrap();
    let mut a = world.runner(true).await;
    let js = a.jetstream();
    let own = js.publish(subject::runner_status(ORG, "r1"), "a".into());
    own.await.unwrap().await.unwrap();
    let theirs = subject::runner_status(ORG, "r2");
    a.client
        .publish(theirs.clone(), "forged".into())
        .await
        .unwrap();
    a.refused(&theirs).await;
    let info = api
        .jetstream()
        .get_stream(&stream)
        .await
        .unwrap()
        .info()
        .await
        .unwrap()
        .clone();
    assert_eq!(info.state.messages, 1, "only runner A's own report");
}

/// AC4: runner A consumes only its own desired sessions, through its own durable consumer created
/// by its filtered subject, and reads no other runner's inbox.
#[tokio::test]
async fn ac4_runner_a_cannot_consume_runner_bs_desired_or_read_its_inbox() {
    let world = world("nats_auth::desired").await;
    let relay = world.relay().await;
    let name = subject::desired_stream(ORG);
    let js = relay.jetstream();
    js.create_or_update_stream(stream::Config {
        name: name.clone(),
        subjects: vec![subject::runners_desired(ORG)],
        max_messages_per_subject: 1,
        ..Default::default()
    })
    .await
    .unwrap();
    for runner in ["r1", "r2"] {
        let session = SessionId(uuid::Uuid::now_v7());
        let desired = subject::runner_desired(ORG, runner, session);
        js.publish(desired, runner.into())
            .await
            .unwrap()
            .await
            .unwrap();
    }
    let mut a = world.runner(true).await;
    let stream = a.jetstream().get_stream_no_info(&name).await.unwrap();
    let consumer = |durable: &str, filter: String| pull::Config {
        durable_name: Some(durable.into()),
        filter_subject: filter,
        ..Default::default()
    };
    let own = format!("rl.{ORG}.runner.r1.desired.*");
    let theirs = format!("rl.{ORG}.runner.r2.desired.*");
    let mine = stream
        .create_consumer(consumer("runner-r1", own))
        .await
        .unwrap();
    let mut batch = mine.fetch().max_messages(10).messages().await.unwrap();
    let message = batch.next().await.unwrap().unwrap();
    assert_eq!(&message.payload[..], b"r1");
    message.ack().await.unwrap();
    assert!(
        batch.next().await.is_none(),
        "runner B's desired never reaches A"
    );
    for (durable, filter) in [
        ("runner-r1", theirs.clone()),
        ("runner-r2", theirs.clone()),
        // Unfiltered, named or not.
        ("runner-r1", String::new()),
    ] {
        let created = stream
            .create_consumer(consumer(durable, filter.clone()))
            .await;
        assert!(created.is_err(), "{durable} on {filter:?}");
    }
    let unnamed = stream.create_consumer(pull::Config::default()).await;
    assert!(unnamed.is_err(), "an ephemeral, unfiltered consumer");
    let inbox = format!("{}.>", subject::inbox("runner-r2"));
    let _ = a.client.subscribe(inbox.clone()).await.unwrap();
    a.refused(&inbox).await;
}
