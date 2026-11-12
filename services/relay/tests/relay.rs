//! The relay against real Postgres and a real NATS server.
#![allow(clippy::unwrap_used, clippy::expect_used)]

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
    let command = intake(&source, &rl_testkit::workspace(), &SystemClock, &UuidV7)
        .await
        .unwrap()
        .sessions
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
        .get_stream(subject::desired_stream(org))
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
        subject::runner_desired(&org, "r1", command.id)
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
    // RAN-35: the whole session, as the runner's Session resource holds it.
    let Some(Body::Desired(whole)) = Envelope::decode_from_slice(&message.payload).unwrap().body
    else {
        unreachable!("not a SessionDesired")
    };
    let ticket = whole.ticket.into_option().unwrap();
    assert_eq!(
        (
            ticket.key.as_str(),
            ticket.title.as_str(),
            ticket.criteria.as_slice(),
            ticket.url.as_str()
        ),
        (
            "RAN-1",
            "Ticket 1",
            ["AC1 one".to_owned(), "AC2 two".to_owned()].as_slice(),
            "https://linear.app/example/issue/RAN-1"
        )
    );
    let worktree = &whole.worktrees[0];
    assert_eq!(
        (
            worktree.mirror_repo.as_str(),
            worktree.base_ref.as_str(),
            whole.member.as_str()
        ),
        ("shop", "main", "ariel")
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

    // One message per session's subject (review B1): the stream holds its latest desired state.
    let kept: Vec<_> = consume(&org).await.iter().map(desired).collect();
    assert_eq!(
        kept,
        [(2, DesiredState::DESIRED_STATE_WARM, "rl-ran-1".to_owned())]
    );
    for (seq, id) in [(1, command.event), (2, warm.event)] {
        let event = Event::decode_from_slice(&watch.next().await.unwrap().payload).unwrap();
        assert_eq!((event.seq, event.id), (seq, id.0.to_string()));
    }
}

/// RAN-43: a desired stream that lost its messages (a wiped volume, or the move into the RL account)
/// gets each session's last published desired state back from Postgres, tombstones included,
/// before anything newer; what wasn't sent yet follows from the outbox, in order.
#[tokio::test]
async fn an_empty_desired_stream_is_reseeded_from_postgres() {
    let (store, org) = (rl_testkit::store("relay::reseed").await, org());
    let tickets = FakeTickets(vec![ticket(1, &[INTAKE_LABEL]), ticket(2, &[INTAKE_LABEL])]);
    let commands = intake(&tickets, &rl_testkit::workspace(), &SystemClock, &UuidV7)
        .await
        .unwrap()
        .sessions;
    for command in &commands {
        assert!(store.create_session(command).await.unwrap());
    }
    let (one, two) = (commands[0].id, commands[1].id);
    let closed = change_state(one, State::Cold, &SystemClock, &UuidV7);
    store.change_state(&closed).await.unwrap();
    assert_eq!(relay(&store, &org).await.run_once().await.unwrap(), 3);
    let unsent = change_state(two, State::Warm, &SystemClock, &UuidV7);
    store.change_state(&unsent).await.unwrap();

    jetstream::new(rl_testkit::nats().await)
        .delete_stream(subject::desired_stream(&org))
        .await
        .unwrap();
    let restarted = relay(&store, &org).await;
    let mut kept: Vec<_> = consume(&org).await.iter().map(desired).collect();
    kept.sort_by(|a, b| a.2.cmp(&b.2));
    assert_eq!(
        kept,
        [
            (2, DesiredState::DESIRED_STATE_COLD, "rl-ran-1".to_owned()),
            (1, DesiredState::DESIRED_STATE_HOT, "rl-ran-2".to_owned()),
        ],
        "the last published state of each, the tombstone included"
    );
    assert_eq!(restarted.run_once().await.unwrap(), 1);
    let mut kept: Vec<_> = consume(&org).await.iter().map(desired).collect();
    kept.sort_by(|a, b| a.2.cmp(&b.2));
    assert_eq!(
        kept[1],
        (2, DesiredState::DESIRED_STATE_WARM, "rl-ran-2".to_owned())
    );
}

/// RAN-43 review 2: a reseed cut short (the stream holds only some sessions) is finished by the
/// next start, and never replaces what a subject already holds, even an older state than
/// Postgres's (here a newer one, published after the reseed began).
#[tokio::test]
async fn a_reseed_cut_short_is_finished_at_the_next_start_and_never_overwrites() {
    let (store, org) = (rl_testkit::store("relay::reseed_partial").await, org());
    let tickets = FakeTickets(vec![ticket(1, &[INTAKE_LABEL]), ticket(2, &[INTAKE_LABEL])]);
    let commands = intake(&tickets, &rl_testkit::workspace(), &SystemClock, &UuidV7)
        .await
        .unwrap()
        .sessions;
    for command in &commands {
        assert!(store.create_session(command).await.unwrap());
    }
    let first = relay(&store, &org).await;
    assert_eq!(first.run_once().await.unwrap(), 2);
    let kept = consume(&org).await;
    let one = kept
        .iter()
        .find(|m| desired(m).2 == "rl-ran-1")
        .unwrap()
        .clone();

    // The stream lost everything; a reseed put back session 1 only, as a later state, then stopped.
    let nats = jetstream::new(rl_testkit::nats().await);
    let stream = nats
        .get_stream(subject::desired_stream(&org))
        .await
        .unwrap();
    stream.purge().await.unwrap();
    let mut newer = Envelope::decode_from_slice(&one.payload).unwrap();
    if let Some(Body::Desired(desired)) = &mut newer.body {
        desired.generation = 99;
    }
    let ack = nats
        .publish(one.subject.clone(), newer.encode_to_vec().into())
        .await
        .unwrap();
    ack.await.unwrap();

    let restarted = relay(&store, &org).await;
    assert_eq!(
        restarted.reseed().await.unwrap(),
        0,
        "nothing left to put back"
    );
    let mut kept: Vec<_> = consume(&org).await.iter().map(desired).collect();
    kept.sort_by(|a, b| a.2.cmp(&b.2));
    assert_eq!(
        kept,
        [
            (99, DesiredState::DESIRED_STATE_HOT, "rl-ran-1".to_owned()),
            (1, DesiredState::DESIRED_STATE_HOT, "rl-ran-2".to_owned()),
        ],
        "session 2 put back, session 1's state kept"
    );
}

/// RAN-43 review 2: a session whose last desired state can't be decoded is skipped by the reseed,
/// which puts back every other session.
#[tokio::test]
async fn a_reseed_skips_an_undecodable_row_and_puts_back_the_rest() {
    let database = rl_testkit::Database::new("relay::reseed_poison").await;
    let (store, org) = (database.store().await, org());
    let tickets = FakeTickets(vec![ticket(1, &[INTAKE_LABEL]), ticket(2, &[INTAKE_LABEL])]);
    for command in intake(&tickets, &rl_testkit::workspace(), &SystemClock, &UuidV7)
        .await
        .unwrap()
        .sessions
    {
        assert!(store.create_session(&command).await.unwrap());
    }
    assert_eq!(relay(&store, &org).await.run_once().await.unwrap(), 2);
    database.execute(include_str!("reseed_poison.sql")).await;

    let stream = jetstream::new(rl_testkit::nats().await)
        .get_stream(subject::desired_stream(&org))
        .await
        .unwrap();
    stream.purge().await.unwrap();
    let restarted = relay(&store, &org).await;
    let mut kept: Vec<_> = consume(&org).await.iter().map(desired).collect();
    kept.sort_by(|a, b| a.2.cmp(&b.2));
    let namespaces: Vec<_> = kept.iter().map(|k| k.2.as_str()).collect();
    assert_eq!(namespaces, ["rl-ran-1", "rl-ran-2"], "the rest put back");
    assert_eq!(restarted.reseed().await.unwrap(), 0);
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

/// Review B1: the stream is the desired-state table, one message per session, kept however long.
#[tokio::test]
async fn the_desired_stream_keeps_one_message_per_session() {
    let (store, org) = (rl_testkit::store("relay::table").await, org());
    relay(&store, &org).await;
    let config = jetstream::new(rl_testkit::nats().await)
        .get_stream(subject::desired_stream(&org))
        .await
        .unwrap()
        .cached_info()
        .config
        .clone();
    assert_eq!(
        (
            config.max_messages_per_subject,
            config.max_age,
            config.subjects
        ),
        (
            1,
            Duration::ZERO,
            vec![format!("rl.{org}.runner.*.desired.*")]
        )
    );
}

/// RAN-79: the relay's start (`main`: `rl_nats::connect`, then `Relay::new`) waits while NATS is
/// away and finishes within one retry pause once it is back, so its /healthz answers.
#[tokio::test]
async fn the_relay_waits_for_nats_at_start() {
    let store = rl_testkit::store("relay::waits_for_nats").await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("nats://127.0.0.1:{port}");
    let dir = tempfile::tempdir().unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, "a token").unwrap();
    let starting = tokio::spawn({
        let (store, url) = (store.clone(), url.clone());
        async move {
            let nats = rl_nats::connect(|| rl_nats::options(token.clone(), "relay"), &url)
                .await
                .unwrap();
            Relay::new(store, nats, "waits".into(), "r1".into(), 1)
                .await
                .unwrap()
        }
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !starting.is_finished(),
        "the relay gave up while NATS was away"
    );
    let mut server = std::process::Command::new("nats-server")
        .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-js", "-sd"])
        .arg(dir.path().join("js"))
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("nats-server: run the tests via scripts/test/services.sh");
    let started = tokio::time::timeout(rl_nats::MAX_RETRY + Duration::from_secs(2), starting).await;
    let _ = server.kill();
    let _ = server.wait();
    started.expect("started within one retry pause").unwrap();
}
