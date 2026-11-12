//! The api end to end: a Linear stub, real Postgres, NATS and Dex, the relay, and a real Connect
//! client.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write;
use std::time::Duration;

use api::status::Recorder;
use api::{intake_once, link_once};
use async_nats::jetstream::message::PublishMessage;
use buffa::Message;
use connectrpc::ErrorCode;
use connectrpc::client::HttpClient;
use relay::Relay;
use rl_app::{SystemClock, UuidV7, change_state};
use rl_config::SecretRef;
use rl_domain::{INTAKE_LABEL, SessionId, State, Ticket, Tracker, Upstream};
use rl_proto::rangelight::runner::v1alpha1::__buffa::oneof::envelope::Body;
use rl_proto::rangelight::runner::v1alpha1::{DesiredState, Envelope, Phase, SessionStatus};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::watch_sessions_response::Kind as Watched;
use rl_proto::rangelight::session::v1alpha1::{
    Event, GetSessionRequest, ListSessionsRequest, SessionServiceClient, Snapshot,
    WatchSessionRequest, WatchSessionsRequest,
};
use rl_store::Store;
use rl_testkit::{FakeTickets, LinearStub, Reply, Seed, ticket};
use rl_tickets_linear::Linear;
use tokio_util::sync::CancellationToken;

const KEY: &str = "lin_api_APITESTKEY9876543210";

fn org() -> String {
    format!("t{}", uuid::Uuid::now_v7().simple())
}

/// One intake round in the test workspace; returns how many sessions it created.
async fn intake(store: &Store, source: &impl rl_app::TicketSource) -> usize {
    intake_once(
        store,
        source,
        &rl_testkit::workspace(),
        &SystemClock,
        &UuidV7,
    )
    .await
    .unwrap()
}

fn linear(stub: &LinearStub, key: &tempfile::NamedTempFile) -> Linear {
    Linear::new(
        stub.url.clone(),
        "RAN".into(),
        SecretRef::file(key.path()),
        &[],
    )
    .unwrap()
}

fn key_file() -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{KEY}").unwrap();
    file
}

/// AC3 and AC6: one labelled issue gives one session, re-polling is a no-op, an unlabelled
/// issue gives nothing, and the key is in no event.
#[tokio::test]
async fn linear_intake_creates_one_session_per_labelled_issue() {
    let store = rl_testkit::store("api::intake").await;
    let key = key_file();
    let issues = vec![ticket(1, &[INTAKE_LABEL]), ticket(2, &["bug"])];
    let stub = LinearStub::start(Reply::Tickets(issues)).await;
    let linear = linear(&stub, &key);

    assert_eq!(intake(&store, &linear).await, 1);
    assert_eq!(intake(&store, &linear).await, 0, "re-poll");

    let sessions = store.list_sessions(10).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        (
            sessions[0].ticket_id.as_str(),
            sessions[0].ticket_key.as_str()
        ),
        ("issue-1", "RAN-1")
    );
    let chain = store.chain(sessions[0].id).await.unwrap();
    assert_eq!(chain.len(), 1);
    assert!(
        !String::from_utf8_lossy(&chain[0].payload).contains(KEY),
        "key in an event"
    );
}

/// The api, and a session API client signed in through Dex.
async fn served(store: &Store, org: &str) -> (common::Served, SessionServiceClient<HttpClient>) {
    let served = common::served(store, org).await;
    let browser = served.signed_in("kilgore@example.com").await;
    let client = served.sessions(&browser.cookie_header());
    (served, client)
}

/// AC5: `ListSessions`, and `WatchSession` fed by the relay's NATS fan-out.
#[tokio::test]
async fn list_and_watch_with_a_real_client() {
    let (store, org) = (rl_testkit::store("api::watch").await, org());
    let stub = LinearStub::start(Reply::Tickets(vec![ticket(7, &[INTAKE_LABEL])])).await;
    let key = key_file();
    let linear = linear(&stub, &key);
    intake(&store, &linear).await;
    let (_served, client) = served(&store, &org).await;

    let listed = client
        .list_sessions(ListSessionsRequest::default())
        .await
        .unwrap()
        .into_owned();
    assert_eq!(listed.sessions.len(), 1);
    let session = &listed.sessions[0];
    assert_eq!(
        (session.ticket_key.as_str(), session.namespace.as_str()),
        ("RAN-7", "rl-ran-7")
    );
    assert_eq!(
        session.state.as_known(),
        Some(DesiredState::DESIRED_STATE_HOT)
    );
    assert_eq!(session.ticket_url, "https://linear.app/example/issue/RAN-7");
    assert!(session.created_at.is_set(), "created_at is filled");
    assert_eq!(session.repo, rl_testkit::workspace().repo());

    let request = WatchSessionRequest {
        session_id: session.id.clone(),
        ..Default::default()
    };
    let mut watch = client.watch_session(request).await.unwrap();
    let id = SessionId(uuid::Uuid::parse_str(&session.id).unwrap());
    let warm = change_state(id, State::Warm, &SystemClock, &UuidV7);
    store.change_state(&warm).await.unwrap();
    let relay = Relay::new(
        store.clone(),
        rl_testkit::nats().await,
        org.clone(),
        "r1".into(),
        1,
    )
    .await
    .unwrap();
    assert_eq!(relay.run_once().await.unwrap(), 2);

    let mut seen = Vec::new();
    for _ in 0..2 {
        let message = tokio::time::timeout(Duration::from_secs(5), watch.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let event = message.to_owned_message().event.into_option().unwrap();
        seen.push((event.seq, event.change.into_option().unwrap().kind));
    }
    assert!(matches!(seen[0], (1, Some(Kind::Created(_)))), "{seen:?}");
    assert!(
        matches!(&seen[1], (2, Some(Kind::StateChanged(changed))) if changed.state.as_known() == Some(DesiredState::DESIRED_STATE_WARM))
    );
}

/// The next `WatchSessions` message's kind, within 5s.
macro_rules! next_watched {
    ($watch:expr) => {
        tokio::time::timeout(Duration::from_secs(5), $watch.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .to_owned_message()
            .kind
            .unwrap()
    };
}

fn snapshot(kind: Watched) -> Option<Box<Snapshot>> {
    match kind {
        Watched::Snapshot(snapshot) => Some(snapshot),
        Watched::Change(_) => None,
    }
}

fn change(kind: Watched) -> Option<Box<Event>> {
    match kind {
        Watched::Change(event) => Some(event),
        Watched::Snapshot(_) => None,
    }
}

/// Review 1 and 2: a change commits and fans out while the stream reads its snapshot, so it lands
/// on one side of the snapshot or the other. Either way the client gets the snapshot and then
/// every later seq exactly once, in order: nothing is lost or repeated. The filtering itself is
/// pinned by the unit tests of `ordered`.
#[tokio::test]
async fn watch_sessions_sends_each_change_after_the_snapshot_once() {
    let (store, org) = (rl_testkit::store("api::watch_all_race").await, org());
    intake(&store, &FakeTickets(vec![ticket(1, &[INTAKE_LABEL])])).await;
    assert_eq!(relay_once(&store, &org).await, 1, "the session's creation");
    let id = store.list_sessions(1).await.unwrap()[0].id;
    let (_served, client) = served(&store, &org).await;

    let mut watch = tokio::time::timeout(
        Duration::from_secs(5),
        client.watch_sessions(WatchSessionsRequest::default()),
    )
    .await
    .unwrap()
    .unwrap();
    store
        .change_state(&change_state(id, State::Warm, &SystemClock, &UuidV7))
        .await
        .unwrap();
    assert_eq!(relay_once(&store, &org).await, 1, "seq 2 fans out");

    let snapshot = snapshot(next_watched!(watch)).expect("the snapshot comes first");
    assert_eq!(snapshot.sessions.len(), 1);
    let from = snapshot.sessions[0].generation;
    assert!(matches!(from, 1 | 2), "{from}");
    store
        .change_state(&change_state(id, State::Cold, &SystemClock, &UuidV7))
        .await
        .unwrap();
    assert_eq!(relay_once(&store, &org).await, 1, "seq 3 fans out");
    let mut seqs = Vec::new();
    while seqs.last() != Some(&3) {
        seqs.push(change(next_watched!(watch)).expect("changes").seq);
    }
    assert_eq!(seqs, ((from + 1)..=3).collect::<Vec<_>>());
}

/// Review 1: a session created while the Workbench watches appears, with its ticket link and repo.
#[tokio::test]
async fn watch_sessions_shows_a_new_session() {
    let (store, org) = (rl_testkit::store("api::watch_all_new").await, org());
    let (_served, client) = served(&store, &org).await;
    let mut watch = client
        .watch_sessions(WatchSessionsRequest::default())
        .await
        .unwrap();
    let snapshot = snapshot(next_watched!(watch)).expect("the snapshot comes first");
    assert_eq!(snapshot.sessions.len(), 0);

    intake(&store, &FakeTickets(vec![ticket(4, &[INTAKE_LABEL])])).await;
    assert_eq!(relay_once(&store, &org).await, 1);
    let change = change(next_watched!(watch)).expect("a change");
    let Some(Kind::Created(created)) = change.change.into_option().unwrap().kind else {
        unreachable!("a creation")
    };
    assert_eq!(
        (
            created.ticket_key.as_str(),
            created.ticket_url.as_str(),
            created.repo.as_str()
        ),
        (
            "RAN-4",
            "https://linear.app/example/issue/RAN-4",
            rl_testkit::workspace().repo()
        )
    );
}

/// Review 2: a session whose creation never reached this stream (its seq 1 lost) can't appear
/// half-known: its first change, seq 2, ends the stream with ABORTED, and a new watch snapshots it.
#[tokio::test]
async fn watch_sessions_aborts_when_a_creation_was_lost() {
    let (store, org) = (rl_testkit::store("api::watch_all_lost").await, org());
    let (_served, client) = served(&store, &org).await;
    let mut watch = client
        .watch_sessions(WatchSessionsRequest::default())
        .await
        .unwrap();
    snapshot(next_watched!(watch)).expect("the snapshot comes first");
    let session = SessionId(uuid::Uuid::now_v7());
    let lost = Event {
        id: uuid::Uuid::now_v7().to_string(),
        session_id: session.0.to_string(),
        seq: 2,
        ..Default::default()
    };
    let nats = rl_testkit::nats().await;
    nats.publish(
        rl_domain::subject::session_events(&org, session),
        lost.encode_to_vec().into(),
    )
    .await
    .unwrap();
    nats.flush().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Aborted);
}

/// Review 2: `GetSession` serves one session, `NOT_FOUND` for an unknown id, `INVALID_ARGUMENT` for
/// one that isn't a UUID.
#[tokio::test]
async fn get_session_finds_one_or_says_not_found() {
    let (store, org) = (rl_testkit::store("api::get").await, org());
    intake(&store, &FakeTickets(vec![ticket(5, &[INTAKE_LABEL])])).await;
    let id = store.list_sessions(1).await.unwrap()[0].id;
    let (_served, client) = served(&store, &org).await;
    let get = |session_id: String| GetSessionRequest {
        session_id,
        ..Default::default()
    };
    let found = client
        .get_session(get(id.0.to_string()))
        .await
        .unwrap()
        .into_owned();
    let session = found.session.into_option().unwrap();
    assert_eq!(
        (session.ticket_key.as_str(), session.ticket_url.as_str()),
        ("RAN-5", "https://linear.app/example/issue/RAN-5")
    );
    let missing = client
        .get_session(get(uuid::Uuid::now_v7().to_string()))
        .await
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    let invalid = client.get_session(get("*".into())).await.unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn watch_rejects_an_id_that_isnt_a_uuid() {
    let (store, org) = (rl_testkit::store("api::invalid").await, org());
    let (_served, client) = served(&store, &org).await;
    let request = WatchSessionRequest {
        session_id: "*".into(),
        ..Default::default()
    };
    // A server stream reports its error on the first read.
    let error = match client.watch_session(request).await {
        Ok(mut stream) => stream.message().await.err().unwrap(),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

/// Review 1: one command that fails (here a namespace taken by another ticket) doesn't end the
/// round; the tickets after it still get sessions.
#[tokio::test]
async fn a_failing_command_doesnt_stop_the_intake_round() {
    let store = rl_testkit::store("api::collision").await;
    let same_key = Seed {
        ticket: Ticket::new(
            Tracker::Linear,
            "issue-x".into(),
            "RAN-1".into(),
            "t".into(),
            "- AC1 one".into(),
        )
        .unwrap(),
        labels: vec![INTAKE_LABEL.into()],
    };
    let source = FakeTickets(vec![
        ticket(1, &[INTAKE_LABEL]),
        same_key,
        ticket(2, &[INTAKE_LABEL]),
    ]);
    assert_eq!(intake(&store, &source).await, 2);
    assert_eq!(store.list_sessions(10).await.unwrap().len(), 2);
}

/// Review 1: a gap in the fan-out (a slow consumer lost events) ends the stream with an error
/// instead of skipping silently.
#[tokio::test]
async fn watch_ends_with_an_error_on_a_seq_gap() {
    let (store, org) = (rl_testkit::store("api::gap").await, org());
    let (_served, client) = served(&store, &org).await;
    let id = uuid::Uuid::now_v7();
    let request = WatchSessionRequest {
        session_id: id.to_string(),
        ..Default::default()
    };
    let mut watch = client.watch_session(request).await.unwrap();
    let nats = rl_testkit::nats().await;
    for seq in [1, 3] {
        let event = Event {
            id: uuid::Uuid::now_v7().to_string(),
            session_id: id.to_string(),
            seq,
            ..Default::default()
        };
        let subject = rl_domain::subject::session_events(&org, SessionId(id));
        nats.publish(subject, event.encode_to_vec().into())
            .await
            .unwrap();
    }
    nats.flush().await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(first.to_owned_message().event.into_option().unwrap().seq, 1);
    let gap = tokio::time::timeout(Duration::from_secs(5), watch.message())
        .await
        .unwrap();
    assert_eq!(gap.err().map(|e| e.code), Some(ErrorCode::Aborted));
}

/// A tracker that never answers.
struct Hangs;

impl rl_app::TicketSource for Hangs {
    fn tickets(&self) -> impl Future<Output = Result<Vec<Ticket>, rl_app::TicketError>> + Send {
        std::future::pending()
    }

    fn link_pull_request(
        &self,
        _ticket_id: &str,
        _url: &str,
    ) -> impl Future<Output = Result<(), rl_app::TicketError>> + Send {
        std::future::pending()
    }
}

/// Review 1: shutdown doesn't wait for an intake round stuck on the tracker.
#[tokio::test]
async fn shutdown_doesnt_wait_for_a_stuck_round() {
    let store = rl_testkit::store("api::shutdown").await;
    let cancel = CancellationToken::new();
    let intake = tokio::spawn(api::intake_loop(
        store,
        Hangs,
        rl_testkit::workspace(),
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), intake)
        .await
        .expect("the loop stopped")
        .unwrap();
}

/// Security review L3: a client that never finishes its request headers is disconnected after the
/// 10-second header-read timeout, not held for ever.
#[tokio::test]
async fn a_stalled_request_is_closed() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (store, org) = (rl_testkit::store("api::stalled").await, org());
    let served = common::served(&store, &org).await;
    let mut socket = tokio::net::TcpStream::connect(served.base.socket_addrs(|| None).unwrap()[0])
        .await
        .unwrap();
    socket
        .write_all(b"GET /auth/login HTTP/1.1\r\nHost: rangelight.test\r\n")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let mut rest = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(20), socket.read_to_end(&mut rest)).await;
    assert!(read.is_ok(), "the connection was still open after 20s");
    let waited = started.elapsed();
    assert!(
        (Duration::from_secs(8)..Duration::from_secs(15)).contains(&waited),
        "closed after {waited:?}"
    );
}

/// Security review round 2, item 1: a connection that never sends a byte is closed within the
/// header-read timeout too (the server speaks HTTP/1.1 only: no protocol sniff waiting for ever).
#[tokio::test]
async fn a_silent_connection_is_closed() {
    use tokio::io::AsyncReadExt;

    let (store, org) = (rl_testkit::store("api::silent").await, org());
    let served = common::served(&store, &org).await;
    let mut socket = tokio::net::TcpStream::connect(served.base.socket_addrs(|| None).unwrap()[0])
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let mut rest = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(20), socket.read_to_end(&mut rest)).await;
    assert!(read.is_ok(), "the connection was still open after 20s");
    let waited = started.elapsed();
    assert!(
        (Duration::from_secs(8)..Duration::from_secs(15)).contains(&waited),
        "closed after {waited:?}"
    );
}

/// A runner's status for `session`, naming `pull_request_url` ("" for none), as the runner sends it.
fn status(session: &str, message_id: uuid::Uuid, pull_request_url: &str) -> Vec<u8> {
    let status = SessionStatus {
        session_id: session.into(),
        observed_generation: 1,
        phase: Phase::PHASE_RUNNING.into(),
        pull_request_url: pull_request_url.into(),
        ..Default::default()
    };
    Envelope {
        message_id: message_id.to_string(),
        protocol_version: "v1alpha1".into(),
        runner_id: "r1".into(),
        body: Some(Body::from(status)),
        ..Default::default()
    }
    .encode_to_vec()
}

/// Publishes a status on the runner's subject with its own `Nats-Msg-Id`, retrying until the
/// consumer has created the stream.
async fn publish_status(org: &str, payload: Vec<u8>) {
    let jetstream = async_nats::jetstream::new(rl_testkit::nats().await);
    let publish = PublishMessage::build()
        .message_id(uuid::Uuid::now_v7().to_string().as_str())
        .payload(payload.into());
    let subject = rl_domain::subject::runner_status(org, "r1");
    loop {
        if let Ok(ack) = jetstream
            .send_publish(subject.clone(), publish.clone())
            .await
            && ack.await.is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// One relay round for runner `r1`; returns how many events it sent.
async fn relay_once(store: &Store, org: &str) -> usize {
    let nats = rl_testkit::nats().await;
    let relay = Relay::new(store.clone(), nats, org.into(), "r1".into(), 1);
    relay.await.unwrap().run_once().await.unwrap()
}

/// How many messages the org's desired stream holds.
async fn desired_messages(org: &str) -> u64 {
    let jetstream = async_nats::jetstream::new(rl_testkit::nats().await);
    let mut stream = jetstream
        .get_stream(rl_domain::subject::desired_stream(org))
        .await
        .unwrap();
    stream.info().await.unwrap().state.messages
}

/// The session's phase, once the status consumer has set one.
async fn reported_phase(store: &Store) -> Option<rl_domain::Phase> {
    for _ in 0..100 {
        if let Some(phase) = store.list_sessions(1).await.unwrap()[0].phase {
            tokio::time::sleep(Duration::from_millis(300)).await; // time for a second one too
            return Some(phase);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

/// RAN-35 AC2 and AC5: a runner's status, delivered twice (past JetStream's duplicate window:
/// another Nats-Msg-Id) and then a third time, becomes exactly one chained event and the session's
/// phase; the relay fans it out to `WatchSession` and sends the runner nothing back for it.
#[tokio::test]
async fn a_status_becomes_exactly_one_chained_event() {
    let (store, org) = (rl_testkit::store("api::status").await, org());
    intake(&store, &FakeTickets(vec![ticket(8, &[INTAKE_LABEL])])).await;
    let id = store.list_sessions(1).await.unwrap()[0].id;
    let cancel = CancellationToken::new();
    let recorder = Recorder::new(store.clone(), None);
    let consumer = tokio::spawn(api::status::consume(
        recorder.clone(),
        rl_testkit::nats().await,
        org.clone(),
        1,
        cancel.clone(),
    ));
    let (_served, client) = served(&store, &org).await;
    let request = WatchSessionRequest {
        session_id: id.0.to_string(),
        ..Default::default()
    };
    let mut watch = client.watch_session(request).await.unwrap();

    let message_id = uuid::Uuid::now_v7();
    let payload = status(&id.0.to_string(), message_id, "");
    publish_status(&org, payload.clone()).await;
    publish_status(&org, payload.clone()).await;
    assert_eq!(
        reported_phase(&store).await,
        Some(rl_domain::Phase::Running)
    );
    let third = recorder.record(&payload, "r1", &SystemClock, &UuidV7).await;
    assert_eq!(third.unwrap(), api::status::Outcome::Duplicate);
    assert_eq!(
        store.chain(id).await.unwrap().len(),
        2,
        "created, then one phase change"
    );

    assert_eq!(relay_once(&store, &org).await, 2);
    let mut last = None;
    for _ in 0..2 {
        let message = tokio::time::timeout(Duration::from_secs(5), watch.message()).await;
        let event = message.unwrap().unwrap().unwrap().to_owned_message().event;
        last = event
            .into_option()
            .unwrap()
            .change
            .into_option()
            .unwrap()
            .kind;
    }
    assert!(
        matches!(&last, Some(Kind::PhaseChanged(p)) if p.phase.as_known() == Some(Phase::PHASE_RUNNING) && p.message_id == message_id.to_string()),
        "{last:?}"
    );
    assert_eq!(
        desired_messages(&org).await,
        1,
        "only the created event goes to the runner"
    );
    cancel.cancel();
    consumer.await.unwrap();
}

/// RAN-56 AC1, AC2 and AC4: from the runner's status stream, a foreign pull request is ignored and
/// the session's own is stored and served.
#[tokio::test]
async fn a_pull_request_is_stored_and_served() {
    let (store, org) = (rl_testkit::store("api::pull_request").await, org());
    intake(&store, &FakeTickets(vec![ticket(9, &[INTAKE_LABEL])])).await;
    let id = store.list_sessions(1).await.unwrap()[0].id.0.to_string();
    let upstream = Upstream::new("shop", "https://github.com/acme/shop.git").unwrap();
    let cancel = CancellationToken::new();
    let consumer = tokio::spawn(api::status::consume(
        Recorder::new(store.clone(), Some(upstream)),
        rl_testkit::nats().await,
        org.clone(),
        1,
        cancel.clone(),
    ));
    let ours = "https://github.com/acme/shop/pull/7";
    // Review H1: one the database's check would refuse, in case Rust's check ever let it through.
    for bad in [
        "https://github.com/evil/shop/pull/7",
        "HTTPS://GitHub.com/acme/shop/pull/7",
    ] {
        publish_status(&org, status(&id, uuid::Uuid::now_v7(), bad)).await;
    }
    publish_status(&org, status(&id, uuid::Uuid::now_v7(), ours)).await;
    let mut stored = None;
    for _ in 0..100 {
        stored = store.list_sessions(1).await.unwrap()[0]
            .pull_request_url
            .clone();
        if stored.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(stored.as_deref(), Some(ours));
    let session = SessionId(id.parse().unwrap());
    assert_eq!(
        store.chain(session).await.unwrap().len(),
        4,
        "created, and each report's phase: a bad URL never blocks its report"
    );
    let (_served, client) = served(&store, &org).await;
    let request = GetSessionRequest {
        session_id: id.clone(),
        ..Default::default()
    };
    let served = client.get_session(request).await.unwrap().into_owned();
    assert_eq!(served.session.into_option().unwrap().pull_request_url, ours);
    cancel.cancel();
    consumer.await.unwrap();
}

const OURS: &str = "https://github.com/acme/shop/pull/7";

/// A session for ticket `n` whose runner reported [`OURS`].
async fn with_pull_request(store: &Store, n: u32) {
    intake(store, &FakeTickets(vec![ticket(n, &[INTAKE_LABEL])])).await;
    let id = store.list_sessions(1).await.unwrap()[0].id.0.to_string();
    let upstream = Upstream::new("shop", "https://github.com/acme/shop").unwrap();
    let report = status(&id, uuid::Uuid::now_v7(), OURS);
    Recorder::new(store.clone(), Some(upstream))
        .record(&report, "r1", &SystemClock, &UuidV7)
        .await
        .unwrap();
}

/// RAN-56 AC3 and AC4: a Linear failure keeps the stored pull request and is retried; then it is
/// attached to the ticket exactly once.
#[tokio::test]
async fn a_pull_request_is_attached_to_its_ticket_once() {
    let store = rl_testkit::store("api::pull_request_link").await;
    with_pull_request(&store, 9).await;
    let ours = OURS;
    let key = key_file();
    let linear = |stub: &LinearStub| linear(stub, &key);
    let down = LinearStub::start(Reply::Status(500)).await;
    assert_eq!(
        link_once(&store, &linear(&down), &SystemClock)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        store.unlinked_pull_requests(10).await.unwrap().len(),
        1,
        "kept for a retry"
    );
    let up = LinearStub::start(Reply::Tickets(Vec::new())).await;
    assert_eq!(
        link_once(&store, &linear(&up), &SystemClock).await.unwrap(),
        1
    );
    assert_eq!(
        link_once(&store, &linear(&up), &SystemClock).await.unwrap(),
        0,
        "once"
    );
    let requests = up.requests();
    assert_eq!(requests.len(), 1);
    let variables = &requests[0].body["variables"];
    assert_eq!(
        (variables["issue"].as_str(), variables["url"].as_str()),
        (Some("issue-9"), Some(ours))
    );
    assert!(
        requests[0].body["query"]
            .as_str()
            .unwrap()
            .contains("attachmentCreate")
    );
}

/// RAN-56 review M1: Linear refusing a link (a GraphQL error: the issue is gone) is recorded, and
/// that link is never tried again.
#[tokio::test]
async fn a_refused_link_is_never_tried_again() {
    let store = rl_testkit::store("api::pull_request_refused").await;
    with_pull_request(&store, 10).await;
    let key = key_file();
    let gone = serde_json::json!({ "data": null, "errors": [{ "message": "Entity not found" }] });
    let refusing = LinearStub::start(Reply::Answer(gone)).await;
    assert_eq!(
        link_once(&store, &linear(&refusing, &key), &SystemClock)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        link_once(&store, &linear(&refusing, &key), &SystemClock)
            .await
            .unwrap(),
        0
    );
    assert_eq!(refusing.requests().len(), 1, "tried once");
    assert_eq!(store.unlinked_pull_requests(10).await.unwrap(), []);
    assert_eq!(
        store.list_sessions(1).await.unwrap()[0]
            .pull_request_url
            .as_deref(),
        Some(OURS),
        "the URL stays"
    );
}

/// A tracker that lists its tickets but never answers a link.
struct LinksHang(FakeTickets);

impl rl_app::TicketSource for LinksHang {
    fn tickets(&self) -> impl Future<Output = Result<Vec<Ticket>, rl_app::TicketError>> + Send {
        self.0.tickets()
    }

    fn link_pull_request(
        &self,
        _ticket_id: &str,
        _url: &str,
    ) -> impl Future<Output = Result<(), rl_app::TicketError>> + Send {
        std::future::pending()
    }
}

/// RAN-56 review M2: links run in their own loop, so a tracker hanging on one never holds up
/// intake; shutdown stops both.
#[tokio::test]
async fn a_hanging_link_doesnt_hold_up_intake() {
    let store = rl_testkit::store("api::link_hangs").await;
    with_pull_request(&store, 11).await;
    let source = || LinksHang(FakeTickets(vec![ticket(12, &[INTAKE_LABEL])]));
    let cancel = CancellationToken::new();
    let links = tokio::spawn(api::link_loop(store.clone(), source(), cancel.clone()));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let intake = tokio::spawn(api::intake_loop(
        store.clone(),
        source(),
        rl_testkit::workspace(),
        cancel.clone(),
    ));
    let mut sessions = 0;
    for _ in 0..40 {
        sessions = store.list_sessions(10).await.unwrap().len();
        if sessions == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(sessions, 2, "intake went on while a link hung");
    cancel.cancel();
    for task in [links, intake] {
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("the loop stopped")
            .unwrap();
    }
}
