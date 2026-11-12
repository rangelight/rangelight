//! RAN-63 AC5: a lane's agent events reach the api through its own subject, and only there.

use std::time::Duration;

use rl_domain::{SessionId, subject};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{ORG, world};

/// One agent event of `session`, as the lane's emitter writes it, with a new message id.
fn event(session: SessionId, r#type: &str, payload: &serde_json::Value) -> String {
    sent(uuid::Uuid::now_v7(), session, r#type, payload)
}

/// One agent event of `session` with `message_id`.
fn sent(
    message_id: uuid::Uuid,
    session: SessionId,
    r#type: &str,
    payload: &serde_json::Value,
) -> String {
    json!({
        "messageId": message_id.to_string(),
        "sessionId": session.0.to_string(),
        "at": "2026-11-13T10:00:00.000Z",
        "type": r#type,
        "payload": payload,
    })
    .to_string()
}

/// A real lane identity sends its sign-in ask on its own subject: the api records it, and the
/// session waits for its member; another lane's subject is refused, an invalid event is
/// dropped, and the next turn's progress ends the wait.
#[tokio::test]
async fn ac5_a_lane_s_ask_makes_its_session_wait_for_you() {
    let world = world("nats_auth::agent_events").await;
    let (session, other) = (
        world.started_session(21).await,
        world.started_session(22).await,
    );
    let api = world.api().await;
    let cancel = CancellationToken::new();
    let consumer = tokio::spawn(api::agent_events::consume(
        world.store.clone(),
        api.client.clone(),
        ORG.into(),
        1,
        cancel.clone(),
    ));
    let mut lane = world.lane("rl-ran-21").await.expect("the lane");
    let question = async || {
        world
            .store
            .get_session(session)
            .await
            .unwrap()
            .unwrap()
            .question
    };
    let own = subject::agent_events(ORG, session);
    let ask = json!({ "question": "Please sign in to Claude." });
    // The api creates the stream as it starts: send the ask, one message, until it lands; the
    // api records it once however often it arrives.
    let ask_id = uuid::Uuid::now_v7();
    for _ in 0..100 {
        let sent = sent(ask_id, session, "ask", &ask);
        lane.client.publish(own.clone(), sent.into()).await.unwrap();
        lane.client.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        if question().await.is_some() {
            break;
        }
    }
    assert_eq!(
        question().await.as_deref(),
        Some("Please sign in to Claude.")
    );

    let foreign = subject::agent_events(ORG, other);
    let sent = event(other, "ask", &ask);
    lane.client
        .publish(foreign.clone(), sent.into())
        .await
        .unwrap();
    lane.refused(&foreign).await;
    invalid_dropped_then_turn_ends_the_wait(&world, &lane, session).await;
    let others = world.store.get_session(other).await.unwrap().unwrap();
    assert_eq!(
        others.question, None,
        "the other lane's session doesn't wait"
    );
    cancel.cancel();
    consumer.await.unwrap();
}

/// Once any ask still in flight is in: an invalid event is dropped, and the next turn's
/// progress ends the wait.
async fn invalid_dropped_then_turn_ends_the_wait(
    world: &super::World,
    lane: &super::Signed,
    session: SessionId,
) {
    let question = async || {
        let row = world.store.get_session(session).await.unwrap();
        row.unwrap().question
    };
    let own = subject::agent_events(ORG, session);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = world.store.chain(session).await.unwrap().len();
    let invalid = event(session, "ask", &json!({ "question": "" }));
    lane.client
        .publish(own.clone(), invalid.into())
        .await
        .unwrap();
    let turn = event(
        session,
        "progress",
        &json!({ "stage": "turn", "note": "working" }),
    );
    lane.client.publish(own.clone(), turn.into()).await.unwrap();
    lane.client.flush().await.unwrap();
    for _ in 0..50 {
        if question().await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(question().await, None, "the next turn ends the wait");
    let after = world.store.chain(session).await.unwrap().len();
    assert_eq!(after, before + 1, "the invalid event was dropped");
}
