#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;

#[test]
fn a_watch_sends_in_order_skips_repeats_and_stops_at_a_gap() {
    assert_eq!(step(None, 7), Step::Send);
    assert_eq!(step(Some(7), 8), Step::Send);
    assert_eq!(step(Some(7), 7), Step::Skip);
    assert_eq!(step(Some(7), 3), Step::Skip);
    assert_eq!(step(Some(7), 9), Step::Gap);
}

fn event(session: &str, seq: u64) -> Event {
    Event {
        session_id: session.into(),
        seq,
        ..Default::default()
    }
}

/// The seqs `ordered` sends from `events`, and whether it ended with ABORTED.
async fn sent(
    events: Vec<Event>,
    seen: &[(&str, u64)],
    unknown: Option<u64>,
) -> (Vec<(String, u64)>, bool) {
    let subscription = Subscription(Box::pin(futures_util::stream::iter(events)));
    let seen = seen.iter().map(|(id, g)| ((*id).to_owned(), *g)).collect();
    let mut out = Vec::new();
    let mut stream = std::pin::pin!(ordered(subscription, seen, unknown));
    while let Some(item) = stream.next().await {
        match item {
            Ok(e) => out.push((e.session_id, e.seq)),
            Err(e) => return (out, e.code == ErrorCode::Aborted),
        }
    }
    (out, false)
}

/// Review 2: seeded with every session's generation, `WatchSessions` drops what the snapshot
/// holds, continues a session the snapshot only counted (beyond its display limit), and wants
/// seq 1 from a session it never saw.
#[tokio::test]
async fn watch_sessions_checks_every_change_against_the_seed() {
    let seed = [("shown", 2), ("beyond-limit", 7)];
    let (out, aborted) = sent(
        vec![
            event("shown", 2),
            event("shown", 3),
            event("beyond-limit", 8),
        ],
        &seed,
        Some(0),
    )
    .await;
    assert_eq!(out, [("shown".into(), 3), ("beyond-limit".into(), 8)]);
    assert!(!aborted);
    let (out, _) = sent(vec![event("new", 1), event("new", 2)], &seed, Some(0)).await;
    assert_eq!(out, [("new".into(), 1), ("new".into(), 2)]);
    let (out, aborted) = sent(vec![event("lost-creation", 2)], &seed, Some(0)).await;
    assert!(out.is_empty() && aborted, "seq 1 was lost: ABORTED");
}

/// Review 2: a viewer that falls further behind than its subscription's capacity loses events
/// (async-nats drops them); the gap ends the stream with ABORTED, the reconnect path.
#[tokio::test]
async fn a_viewer_past_its_capacity_ends_with_aborted() {
    let nats = async_nats::ConnectOptions::new()
        .subscription_capacity(2)
        .connect(rl_testkit::nats_url())
        .await
        .unwrap();
    let session = Uuid::now_v7();
    let subject = subject::session_events("capacity", SessionId(session));
    let subscription = subscribe(
        &nats,
        &CancellationToken::new(),
        subject.clone(),
        tracing::Span::none(),
    )
    .await
    .unwrap();
    let publisher = rl_testkit::nats().await;
    let publish = |seq: u64| {
        let event = Event {
            session_id: session.to_string(),
            seq,
            ..Default::default()
        };
        publisher.publish(subject.clone(), event.encode_to_vec().into())
    };
    // Nobody reads while ten arrive: two fit, the rest are dropped.
    for seq in 1..=10 {
        publish(seq).await.unwrap();
    }
    publisher.flush().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    publish(11).await.unwrap();
    publisher.flush().await.unwrap();
    let mut stream = std::pin::pin!(ordered(subscription, HashMap::new(), None));
    let mut seqs = Vec::new();
    let ended = loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
            Ok(Some(Ok(e))) => seqs.push(e.seq),
            Ok(Some(Err(e))) => break e.code,
            other => panic!("{other:?} after {seqs:?}"),
        }
    };
    assert_eq!(ended, ErrorCode::Aborted, "after {seqs:?}");
    assert_eq!(seqs, [1, 2]);
}
