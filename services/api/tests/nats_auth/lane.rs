//! A lane reaches only its own terminal, once, while its session lives.

use std::time::Duration;

use futures_util::StreamExt;
use rl_app::{SystemClock, UuidV7, change_state};
use rl_domain::{Phase, State, subject};

use super::{ORG, sign_in, world};

/// AC2: a lane receives its own terminal input and sends its own output; it can't subscribe to
/// anything else (every subject, the JetStream API, another session's terminal) or send elsewhere.
#[tokio::test]
async fn ac2_a_lane_reaches_only_its_own_terminal() {
    let world = world("nats_auth::lane").await;
    let (session, other) = (
        world.started_session(7).await,
        world.started_session(8).await,
    );
    let api = world.api().await;
    let mut lane = world
        .lane("rl-ran-7")
        .await
        .expect("the live session's lane");
    for forbidden in ["rl.>", "$JS.API.>", &subject::pty_in(ORG, other)] {
        let _ = lane.client.subscribe(forbidden.to_owned()).await.unwrap();
        lane.refused(forbidden).await;
    }
    let mut input = lane
        .client
        .subscribe(subject::pty_in(ORG, session))
        .await
        .unwrap();
    lane.client.flush().await.unwrap();
    let mut output = api
        .client
        .subscribe(format!("rl.{ORG}.pty.*.out"))
        .await
        .unwrap();
    api.client.flush().await.unwrap();
    let typed = subject::pty_in(ORG, session);
    api.client.publish(typed, "ls\n".into()).await.unwrap();
    let typed = tokio::time::timeout(Duration::from_secs(5), input.next()).await;
    assert_eq!(&typed.unwrap().unwrap().payload[..], b"ls\n");
    let shown = subject::pty_out(ORG, session);
    lane.client.publish(shown, "file\n".into()).await.unwrap();
    let shown = tokio::time::timeout(Duration::from_secs(5), output.next()).await;
    assert_eq!(&shown.unwrap().unwrap().payload[..], b"file\n");
    for forbidden in [subject::pty_out(ORG, other), "$JS.API.INFO".to_owned()] {
        lane.client
            .publish(forbidden.clone(), "x".into())
            .await
            .unwrap();
        lane.refused(&forbidden).await;
    }
}

/// AC2 (decision 8): a lane holds one subscription and sends at most 128 KiB, its account's limits.
#[tokio::test]
async fn ac2_a_lane_holds_one_subscription_and_sends_at_most_128_kib() {
    let world = world("nats_auth::lane_limits").await;
    let session = world.started_session(7).await;
    let mut lane = world.lane("rl-ran-7").await.unwrap();
    let input = lane
        .client
        .subscribe(subject::pty_in(ORG, session))
        .await
        .unwrap();
    let _ = lane
        .client
        .subscribe(subject::pty_in(ORG, session))
        .await
        .unwrap();
    lane.told("maximum subscriptions exceeded").await;
    let too_big = vec![0u8; 128 * 1024 + 1];
    let sent = lane
        .client
        .publish(subject::pty_out(ORG, session), too_big.into())
        .await;
    assert!(sent.is_err(), "over 128 KiB");
    drop(input);
}

/// A lane signs in once, only while its session is live, and only from the sessions' runner's
/// cluster.
#[tokio::test]
async fn a_second_or_closed_or_foreign_lane_is_refused() {
    let world = world("nats_auth::lanes").await;
    let session = world.started_session(9).await;
    let first = world.lane("rl-ran-9").await.unwrap();
    let input = first
        .client
        .subscribe(subject::pty_in(ORG, session))
        .await
        .unwrap();
    first.client.flush().await.unwrap();
    assert!(
        world.lane("rl-ran-9").await.is_none(),
        "a second connection"
    );
    drop((input, first));
    let mut again = None;
    for _ in 0..50 {
        again = world.lane("rl-ran-9").await;
        if again.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(again.is_some(), "once the first is gone, it signs in again");
    drop(again);

    let cold = change_state(session, State::Cold, &SystemClock, &UuidV7);
    world.store.change_state(&cold).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        world.lane("rl-ran-9").await.is_none(),
        "a closed session's lane"
    );
    assert!(world.lane("rl-ran-404").await.is_none(), "no session");
    let token = world.remote.pod_token("rl-ran-9", "default", "agent");
    assert!(
        sign_in(&world.nats.url, Some(token), "lane")
            .await
            .is_none(),
        "another cluster"
    );
}

/// RAN-43 review 1: a lane's connection counts whether it subscribes or not, so a lane can't fill
/// the lane account with idle connections; two sign-ins at once let one in.
#[tokio::test]
async fn a_lane_connects_once_subscribed_or_not_and_once_at_a_time() {
    let world = world("nats_auth::lane_once").await;
    world.started_session(11).await;
    let idle = world.lane("rl-ran-11").await.expect("the first connection");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(
        world.lane("rl-ran-11").await.is_none(),
        "a second connection beside one that never subscribed"
    );
    drop(idle);

    world.started_session(12).await;
    let (a, b) = tokio::join!(world.lane("rl-ran-12"), world.lane("rl-ran-12"));
    assert_eq!(
        usize::from(a.is_some()) + usize::from(b.is_some()),
        1,
        "two at once: one in"
    );
}

/// RAN-43 review 2, H1: a hot session's lane signs in only once its runner reported it Starting or
/// Running, which it does only for a namespace it made for that Session. Before any report, while
/// queued, or failed (say, `namespace_taken`: someone made the namespace in advance), its lane is
/// refused.
#[tokio::test]
async fn a_lane_signs_in_only_once_its_runner_started_it() {
    let world = world("nats_auth::lane_started").await;
    let session = world.started_session(13).await;
    for phase in [Phase::Queued, Phase::Failed] {
        world.report(session, phase).await;
        assert!(world.lane("rl-ran-13").await.is_none(), "{phase:?}");
    }
    // A session the runner never reported on.
    world.session(14).await;
    assert!(world.lane("rl-ran-14").await.is_none(), "no report yet");
    world.report(session, Phase::Starting).await;
    assert!(world.lane("rl-ran-13").await.is_some(), "starting");
}
