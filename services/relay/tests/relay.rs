//! The relay against real Postgres and a real NATS server.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use async_nats::jetstream::{self, consumer::pull};
use buffa::Message;
use futures_util::StreamExt;
use relay::Relay;
use rl_app::{SystemClock, UuidV7, change_state, intake};
use rl_domain::{INTAKE_LABEL, NewSession, State, subject};
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::{DesiredState, Envelope};
use rl_proto::rangelight::session::v1alpha1::Event;
use rl_store::Store;
use rl_testkit::{FakeTickets, ticket};

/// Each test gets its own org, so its subjects and stream are its own.
fn org() -> String {
    format!("t{}", uuid::Uuid::now_v7().simple())
}

async fn session(store: &Store) -> NewSession {
    let source = FakeTickets(vec![ticket(1, &[INTAKE_LABEL])]);
    let command = intake(&source, &SystemClock, &UuidV7)
        .await
        .unwrap()
        .remove(0);
    assert!(store.create_session(&command).await.unwrap());
    command
}

async fn relay(store: &Store, org: &str) -> Relay {
    Relay::new(
        store.clone(),
        rl_testkit::nats().await,
        org.into(),
        "r1".into(),
        1,
    )
    .await
    .unwrap()
}

/// Everything in the org's stream, as one consumer sees it.
async fn consume(org: &str) -> Vec<jetstream::Message> {
    let stream = jetstream::new(rl_testkit::nats().await)
        .get_stream(format!("rl-{org}-desired"))
        .await
        .unwrap();
    let consumer = stream
        .create_consumer(pull::Config::default())
        .await
        .unwrap();
    let batch = consumer
        .fetch()
        .max_messages(10)
        .expires(Duration::from_millis(500))
        .messages()
        .await
        .unwrap();
    batch.map(Result::unwrap).collect().await
}

fn desired(message: &jetstream::Message) -> (u64, DesiredState, String) {
    let envelope = Envelope::decode_from_slice(&message.payload).unwrap();
    let desired = match envelope.body {
        Some(Body::Desired(desired)) => desired,
        other => unreachable!("not a SessionDesired: {other:?}"),
    };
    (
        desired.generation,
        desired.state.as_known().unwrap(),
        desired.namespace,
    )
}

/// AC4: a restart between publish and mark republishes, and JetStream's dedupe on
/// Nats-Msg-Id means the consumer sees the event once.
#[tokio::test]
async fn a_restart_between_publish_and_mark_is_seen_once() {
    let (store, org) = (rl_testkit::store("relay::restart").await, org());
    let command = session(&store).await;

    let before_crash = relay(&store, &org).await;
    let unsent = store.unsent(10).await.unwrap();
    before_crash.publish(&unsent[0]).await.unwrap();
    drop(before_crash); // the crash: published, never marked

    let restarted = relay(&store, &org).await;
    assert_eq!(restarted.run_once().await.unwrap(), 1);
    assert!(
        store.unsent(10).await.unwrap().is_empty(),
        "marked sent after the ack"
    );

    let messages = consume(&org).await;
    assert_eq!(messages.len(), 1, "seen once");
    let message = &messages[0];
    assert_eq!(
        message.subject.as_str(),
        subject::runner_desired(&org, "r1")
    );
    let msg_id = message
        .headers
        .as_ref()
        .unwrap()
        .get("Nats-Msg-Id")
        .unwrap();
    assert_eq!(msg_id.as_str(), command.event.0.to_string());
    assert_eq!(
        desired(message),
        (1, DesiredState::DESIRED_STATE_HOT, "rl-ran-1".into())
    );
}

#[tokio::test]
async fn state_changes_go_out_in_order_and_fan_out() {
    let (store, org) = (rl_testkit::store("relay::order").await, org());
    let command = session(&store).await;
    let warm = change_state(command.id, State::Warm, &SystemClock, &UuidV7);
    store.change_state(&warm).await.unwrap();

    let nats = rl_testkit::nats().await;
    let mut watch = nats
        .subscribe(subject::session_events(&org, command.id))
        .await
        .unwrap();
    nats.flush().await.unwrap();
    let relay = relay(&store, &org).await;
    assert_eq!(relay.run_once().await.unwrap(), 2);
    assert_eq!(relay.run_once().await.unwrap(), 0, "nothing left");

    let generations: Vec<_> = consume(&org).await.iter().map(|m| desired(m).0).collect();
    assert_eq!(generations, [1, 2]);
    let last = desired(&consume(&org).await[1]);
    assert_eq!(
        (last.1, last.2.as_str()),
        (DesiredState::DESIRED_STATE_WARM, "rl-ran-1")
    );
    for (seq, id) in [(1, command.event), (2, warm.event)] {
        let event = Event::decode_from_slice(&watch.next().await.unwrap().payload).unwrap();
        assert_eq!((event.seq, event.id), (seq, id.0.to_string()));
    }
}

/// Review 1: an undecodable row is set aside and only its session waits; others keep flowing.
#[tokio::test]
async fn an_undecodable_event_holds_back_only_its_own_session() {
    let database = rl_testkit::Database::new("relay::poison").await;
    let (store, org) = (database.store().await, org());
    database.execute(include_str!("poison.sql")).await;
    let good = session(&store).await;

    let relay = relay(&store, &org).await;
    assert_eq!(
        relay.run_once().await.unwrap(),
        1,
        "the good session's event went out"
    );
    assert!(
        store.unsent(10).await.unwrap().is_empty(),
        "the poison session's rows wait"
    );

    let messages = consume(&org).await;
    assert_eq!(messages.len(), 1);
    let msg_id = messages[0]
        .headers
        .as_ref()
        .unwrap()
        .get("Nats-Msg-Id")
        .unwrap();
    assert_eq!(msg_id.as_str(), good.event.0.to_string());
}
