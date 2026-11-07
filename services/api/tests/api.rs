//! The api end to end: a Linear stub, real Postgres and NATS, the relay, and a real Connect
//! client.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::time::Duration;

use api::{SessionApi, intake_once, serve};
use buffa::Message;
use connectrpc::ErrorCode;
use connectrpc::client::{ClientConfig, HttpClient};
use relay::Relay;
use rl_app::{SystemClock, UuidV7, change_state};
use rl_config::SecretRef;
use rl_domain::{INTAKE_LABEL, SessionId, State, Ticket, Tracker};
use rl_proto::rangelight::runner::v1alpha1::DesiredState;
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_proto::rangelight::session::v1alpha1::{
    Event, ListSessionsRequest, SessionServiceClient, WatchSessionRequest,
};
use rl_store::Store;
use rl_testkit::{FakeTickets, LinearStub, Reply, Seed, ticket};
use rl_tickets_linear::Linear;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const KEY: &str = "lin_api_APITESTKEY9876543210";

fn org() -> String {
    format!("t{}", uuid::Uuid::now_v7().simple())
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
    let linear = Linear::new(stub.url.clone(), "RAN".into(), SecretRef::file(key.path())).unwrap();

    assert_eq!(
        intake_once(&store, &linear, &SystemClock, &UuidV7)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        intake_once(&store, &linear, &SystemClock, &UuidV7)
            .await
            .unwrap(),
        0,
        "re-poll"
    );

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

struct Served {
    client: SessionServiceClient<HttpClient>,
    cancel: CancellationToken,
}

async fn served(store: &Store, org: &str) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let cancel = CancellationToken::new();
    let api = SessionApi::new(
        store.clone(),
        rl_testkit::nats().await,
        org.into(),
        cancel.clone(),
    );
    tokio::spawn(serve(listener, api, cancel.clone()));
    Served {
        client: SessionServiceClient::new(HttpClient::plaintext(), ClientConfig::new(uri)),
        cancel,
    }
}

/// AC5: `ListSessions`, and `WatchSession` fed by the relay's NATS fan-out.
#[tokio::test]
async fn list_and_watch_with_a_real_client() {
    let (store, org) = (rl_testkit::store("api::watch").await, org());
    let stub = LinearStub::start(Reply::Tickets(vec![ticket(7, &[INTAKE_LABEL])])).await;
    let key = key_file();
    let linear = Linear::new(stub.url.clone(), "RAN".into(), SecretRef::file(key.path())).unwrap();
    intake_once(&store, &linear, &SystemClock, &UuidV7)
        .await
        .unwrap();
    let served = served(&store, &org).await;

    let listed = served
        .client
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

    let request = WatchSessionRequest {
        session_id: session.id.clone(),
        ..Default::default()
    };
    let mut watch = served.client.watch_session(request).await.unwrap();
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
    served.cancel.cancel();
}

#[tokio::test]
async fn watch_rejects_an_id_that_isnt_a_uuid() {
    let (store, org) = (rl_testkit::store("api::invalid").await, org());
    let served = served(&store, &org).await;
    let request = WatchSessionRequest {
        session_id: "*".into(),
        ..Default::default()
    };
    // A server stream reports its error on the first read.
    let error = match served.client.watch_session(request).await {
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
        )
        .unwrap(),
        labels: vec![INTAKE_LABEL.into()],
    };
    let source = FakeTickets(vec![
        ticket(1, &[INTAKE_LABEL]),
        same_key,
        ticket(2, &[INTAKE_LABEL]),
    ]);
    assert_eq!(
        intake_once(&store, &source, &SystemClock, &UuidV7)
            .await
            .unwrap(),
        2
    );
    assert_eq!(store.list_sessions(10).await.unwrap().len(), 2);
}

/// Review 1: a gap in the fan-out (a slow consumer lost events) ends the stream with an error
/// instead of skipping silently.
#[tokio::test]
async fn watch_ends_with_an_error_on_a_seq_gap() {
    let (store, org) = (rl_testkit::store("api::gap").await, org());
    let served = served(&store, &org).await;
    let id = uuid::Uuid::now_v7();
    let request = WatchSessionRequest {
        session_id: id.to_string(),
        ..Default::default()
    };
    let mut watch = served.client.watch_session(request).await.unwrap();
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
}

/// Review 1: shutdown doesn't wait for an intake round stuck on the tracker.
#[tokio::test]
async fn shutdown_doesnt_wait_for_a_stuck_round() {
    let store = rl_testkit::store("api::shutdown").await;
    let cancel = CancellationToken::new();
    let intake = tokio::spawn(api::intake_loop(
        store,
        Hangs,
        SystemClock,
        UuidV7,
        cancel.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), intake)
        .await
        .expect("the loop stopped")
        .unwrap();
}
