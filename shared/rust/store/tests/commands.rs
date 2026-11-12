//! Commands against real Postgres: state, chained event and outbox row in one transaction.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant, SystemTime};

use rl_app::{IdGen, change_state, intake};
use rl_chain::{Link, Record};
use rl_domain::{
    EventId, INTAKE_LABEL, NewSession, Phase, ReportPhase, SessionId, State, Upstream,
};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_store::{ChainEvent, SessionRow, Store, StoreError};
use rl_testkit::{Database, FakeTickets, FixedClock, SeqIds, ticket};

/// A time with nanoseconds, which the chain truncates to microseconds.
fn clock() -> FixedClock {
    FixedClock(SystemTime::UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789))
}

async fn new_session(n: u32, ids: &SeqIds) -> NewSession {
    let source = FakeTickets(vec![ticket(n, &[INTAKE_LABEL])]);
    intake(&source, &rl_testkit::workspace(), &clock(), ids)
        .await
        .unwrap()
        .sessions
        .remove(0)
}

async fn create(store: &Store, command: &NewSession) -> bool {
    store.create_session(command).await.unwrap()
}

async fn move_to(
    store: &Store,
    session: SessionId,
    to: State,
    ids: &impl IdGen,
) -> Result<u64, StoreError> {
    store
        .change_state(&change_state(session, to, &clock(), ids))
        .await
}

fn links(chain: &[ChainEvent]) -> impl Iterator<Item = Link<'_>> {
    chain.iter().map(|e| Link {
        prev_hash: &e.prev_hash,
        record: Record {
            id: e.id,
            at: rl_chain::micros(e.at),
            payload: &e.payload,
        },
        hash: &e.hash,
    })
}

/// ADR 0031: replaying a session's events gives its state row.
fn replay(chain: &[ChainEvent]) -> (State, u64) {
    let mut state = None;
    for event in chain {
        let to = match rl_chain::decode(&event.payload).unwrap().kind.unwrap() {
            Kind::Created(created) => created.state,
            Kind::StateChanged(changed) => changed.state,
            Kind::PhaseChanged(_) => continue,
        };
        state = Some(to.as_known().unwrap());
    }
    let state = match state.unwrap() {
        rl_proto::rangelight::runner::v1alpha1::DesiredState::DESIRED_STATE_HOT => State::Hot,
        rl_proto::rangelight::runner::v1alpha1::DesiredState::DESIRED_STATE_WARM => State::Warm,
        rl_proto::rangelight::runner::v1alpha1::DesiredState::DESIRED_STATE_COLD => State::Cold,
        rl_proto::rangelight::runner::v1alpha1::DesiredState::DESIRED_STATE_UNSPECIFIED => {
            unreachable!("unspecified state")
        }
    };
    (state, chain.last().unwrap().seq)
}

async fn assert_replays(store: &Store) {
    for row in store.list_sessions(100).await.unwrap() {
        let SessionRow {
            id,
            state,
            generation,
            ..
        } = row;
        let chain = store.chain(id).await.unwrap();
        rl_chain::verify(links(&chain)).unwrap();
        assert_eq!(
            replay(&chain),
            (state, generation),
            "events replay to the state row"
        );
    }
}

#[tokio::test]
async fn create_writes_state_event_and_outbox() {
    let store = rl_testkit::store("store::create").await;
    let command = new_session(1, &SeqIds::default()).await;
    assert!(create(&store, &command).await);

    let sessions = store.list_sessions(10).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!((sessions[0].state, sessions[0].generation), (State::Hot, 1));
    assert_eq!(sessions[0].namespace, "rl-ran-1");
    let chain = store.chain(command.id).await.unwrap();
    assert_eq!(
        (chain[0].id, chain[0].seq, chain[0].kind.as_str()),
        (command.event, 1, "session.created")
    );
    // Stored at the hashed precision: microseconds.
    assert_eq!(chain[0].at, rl_chain::time(rl_chain::micros(command.at)));
    assert_ne!(chain[0].at, command.at);
    let unsent = store.unsent(10).await.unwrap();
    assert_eq!(
        (unsent[0].event, unsent[0].session, unsent[0].seq),
        (command.event, command.id, 1)
    );
    assert_replays(&store).await;
}

/// Review 2: a snapshot shows at most `limit` sessions, the newest, but carries every session's
/// generation, so a watch can check changes to sessions it doesn't show.
#[tokio::test]
async fn snapshot_shows_the_newest_and_counts_every_session() {
    let store = rl_testkit::store("store::snapshot").await;
    let ids = SeqIds::default();
    let (older, newer) = (new_session(1, &ids).await, new_session(2, &ids).await);
    create(&store, &older).await;
    create(&store, &newer).await;
    move_to(&store, older.id, State::Warm, &ids).await.unwrap();
    let snapshot = store.snapshot(1).await.unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.generations.len(), 2);
    assert_eq!(snapshot.generations[&older.id], 2);
    assert_eq!(snapshot.generations[&newer.id], 1);
}

/// RAN-36: a listed session carries its ticket's url, its creation time and its repo's name.
#[tokio::test]
async fn list_carries_ticket_url_created_at_and_repo() {
    let store = rl_testkit::store("store::list").await;
    let command = new_session(1, &SeqIds::default()).await;
    create(&store, &command).await;
    let listed = store.list_sessions(10).await.unwrap();
    assert_eq!(
        listed[0].ticket_url,
        "https://linear.app/example/issue/RAN-1"
    );
    assert_eq!(
        listed[0].created_at,
        rl_chain::time(rl_chain::micros(command.at))
    );
    assert_eq!(listed[0].repo, command.workspace.repo());
    let got = store.get_session(command.id).await.unwrap();
    assert_eq!(got.as_ref(), listed.first());
    assert_eq!(
        store
            .get_session(SessionId(uuid::Uuid::now_v7()))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn one_ticket_gets_one_session_by_constraint() {
    let store = rl_testkit::store("store::unique").await;
    let ids = SeqIds::default();
    assert!(create(&store, &new_session(1, &ids).await).await);
    // A second command for the same ticket, with new ids, as a re-poll makes.
    assert!(!create(&store, &new_session(1, &ids).await).await);
    assert_eq!(store.list_sessions(10).await.unwrap().len(), 1);
    assert_eq!(store.unsent(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn state_changes_follow_the_state_machine_and_extend_the_chain() {
    let store = rl_testkit::store("store::changes").await;
    let ids = SeqIds::default();
    let command = new_session(1, &ids).await;
    create(&store, &command).await;
    create(&store, &new_session(2, &ids).await).await;

    assert_eq!(
        move_to(&store, command.id, State::Warm, &ids)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        move_to(&store, command.id, State::Cold, &ids)
            .await
            .unwrap(),
        3
    );
    let refused = move_to(&store, command.id, State::Hot, &ids).await;
    assert!(matches!(refused, Err(StoreError::Domain(_))), "{refused:?}");
    let missing = move_to(
        &store,
        SessionId(uuid::Uuid::from_u128(999)),
        State::Warm,
        &ids,
    )
    .await;
    assert!(matches!(missing, Err(StoreError::NotFound)), "{missing:?}");

    let chain = store.chain(command.id).await.unwrap();
    assert_eq!(chain.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(store.unsent(10).await.unwrap().len(), 4);
    assert_replays(&store).await;
}

/// AC2: a command that fails after its event insert leaves nothing behind.
#[tokio::test]
async fn a_failed_command_leaves_nothing_visible() {
    let database = Database::new("store::rollback").await;
    let store = database.store().await;
    let ids = SeqIds::default();
    let first = new_session(1, &ids).await;
    create(&store, &first).await;
    database.execute(include_str!("fail_outbox.sql")).await;

    let second = new_session(2, &ids).await;
    assert!(store.create_session(&second).await.is_err());
    assert!(move_to(&store, first.id, State::Warm, &ids).await.is_err());

    let sessions = store.list_sessions(10).await.unwrap();
    assert_eq!(sessions.len(), 1, "no second session");
    assert_eq!(
        (sessions[0].state, sessions[0].generation),
        (State::Hot, 1),
        "state unchanged"
    );
    assert!(
        store.chain(second.id).await.unwrap().is_empty(),
        "no event for the failed create"
    );
    assert_eq!(
        store.chain(first.id).await.unwrap().len(),
        1,
        "no event for the failed change"
    );
    assert_eq!(store.unsent(10).await.unwrap().len(), 1);
    assert_replays(&store).await;
}

/// Review 1: a command waiting on another's row lock gives up with a typed, retryable error.
#[tokio::test]
async fn a_held_row_lock_times_out_with_a_typed_error() {
    let database = Database::new("store::lock").await;
    let store = database.store().await;
    let ids = SeqIds::default();
    let command = new_session(1, &ids).await;
    create(&store, &command).await;

    let held = database.hold(include_str!("lock_sessions.sql")).await;
    let started = Instant::now();
    let waited = tokio::time::timeout(
        Duration::from_secs(10),
        move_to(&store, command.id, State::Warm, &ids),
    )
    .await
    .expect("lock_timeout bounds the wait");
    assert!(matches!(waited, Err(StoreError::Timeout(_))), "{waited:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(held);
    assert_eq!(
        move_to(&store, command.id, State::Warm, &ids)
            .await
            .unwrap(),
        2,
        "retry succeeds"
    );
}

#[tokio::test]
async fn events_are_append_only_and_mark_sent_hides_a_row() {
    let database = Database::new("store::append_only").await;
    let store = database.store().await;
    let command = new_session(1, &SeqIds::default()).await;
    create(&store, &command).await;
    assert!(
        database
            .try_execute(include_str!("tamper_events.sql"))
            .await
            .unwrap_err()
            .contains("append-only")
    );

    let unsent = store.unsent(10).await.unwrap();
    store.mark_sent(unsent[0].outbox_id).await.unwrap();
    assert!(
        store.unsent(10).await.unwrap().is_empty(),
        "sent rows aren't unsent"
    );
}

#[tokio::test]
async fn a_failed_row_holds_back_only_its_session() {
    let store = rl_testkit::store("store::failed").await;
    let ids = SeqIds::default();
    let (first, second) = (new_session(1, &ids).await, new_session(2, &ids).await);
    create(&store, &first).await;
    create(&store, &second).await;
    move_to(&store, first.id, State::Warm, &ids).await.unwrap();

    let unsent = store.unsent(10).await.unwrap();
    store.mark_failed(unsent[0].outbox_id).await.unwrap();
    let left: Vec<_> = store
        .unsent(10)
        .await
        .unwrap()
        .iter()
        .map(|u| u.session)
        .collect();
    assert_eq!(
        left,
        [second.id],
        "the first session's later event waits too"
    );
}

/// Review M3: a late report (an older `message_id`) is recorded once, as an event, but doesn't undo
/// the phase a newer report set; the generation still moves on.
#[tokio::test]
async fn a_late_report_never_undoes_a_newer_phase() {
    let store = rl_testkit::store("store::late_report").await;
    let ids = SeqIds::default();
    let command = new_session(1, &ids).await;
    assert!(create(&store, &command).await);
    let (earlier, later) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    let report = |message_id, phase| ReportPhase {
        session: command.id,
        event: EventId(ids.new_id()),
        at: clock().0,
        message_id,
        sent_at: None,
        phase,
        message: String::new(),
        observed_generation: 1,
        pull_request: None,
    };
    assert!(
        store
            .report_phase(&report(later, Phase::Running))
            .await
            .unwrap()
    );
    assert!(
        store
            .report_phase(&report(earlier, Phase::Starting))
            .await
            .unwrap()
    );
    assert!(
        !store
            .report_phase(&report(earlier, Phase::Starting))
            .await
            .unwrap(),
        "once"
    );
    let row = store.list_sessions(1).await.unwrap().remove(0);
    assert_eq!((row.phase, row.generation), (Some(Phase::Running), 3));
    assert_eq!(
        store.chain(command.id).await.unwrap().len(),
        3,
        "both reports are events"
    );
    assert_replays(&store).await;
}

/// RAN-56: the first pull request of the session's own repo is stored and stays; one on another
/// repo's upstream isn't; the session is unlinked until its ticket links that very pull request.
#[tokio::test]
async fn a_session_keeps_its_first_pull_request_until_linked() {
    let store = rl_testkit::store("store::pull_request").await;
    let ids = SeqIds::default();
    let command = new_session(1, &ids).await;
    assert!(create(&store, &command).await);
    let report = |repo: &str, url: &str| ReportPhase {
        session: command.id,
        event: EventId(ids.new_id()),
        at: clock().0,
        message_id: uuid::Uuid::now_v7(),
        sent_at: None,
        phase: Phase::Running,
        message: String::new(),
        observed_generation: 1,
        pull_request: Upstream::new(repo, "https://github.com/acme/shop")
            .unwrap()
            .pull_request(url),
    };
    let (first, second) = (
        "https://github.com/acme/shop/pull/1",
        "https://github.com/acme/shop/pull/2",
    );
    let pull_request = || async {
        store.list_sessions(1).await.unwrap()[0]
            .pull_request_url
            .clone()
    };
    store.report_phase(&report("other", first)).await.unwrap();
    assert_eq!(pull_request().await, None, "another repo's");
    store.report_phase(&report("shop", first)).await.unwrap();
    store.report_phase(&report("shop", second)).await.unwrap();
    assert_eq!(
        pull_request().await.as_deref(),
        Some(first),
        "the first stays"
    );
    assert_replays(&store).await;

    let unlinked = store.unlinked_pull_requests(10).await.unwrap();
    assert_eq!(
        unlinked.iter().map(|s| s.id).collect::<Vec<_>>(),
        [command.id]
    );
    store
        .pull_request_linked(command.id, second, clock().0)
        .await
        .unwrap();
    assert_eq!(
        store.unlinked_pull_requests(10).await.unwrap().len(),
        1,
        "not its PR"
    );
    store
        .pull_request_linked(command.id, first, clock().0)
        .await
        .unwrap();
    assert_eq!(store.unlinked_pull_requests(10).await.unwrap(), []);
}

/// RAN-56 review M1: a failed link goes behind the untried ones, and a refused one is never listed
/// again.
#[tokio::test]
async fn a_failed_link_waits_its_turn_and_a_refused_one_ends() {
    let store = rl_testkit::store("store::link_failed").await;
    let ids = SeqIds::default();
    let upstream = Upstream::new("shop", "https://github.com/acme/shop").unwrap();
    let mut sessions = Vec::new();
    for n in 1..=2 {
        let command = new_session(n, &ids).await;
        assert!(create(&store, &command).await);
        let url = format!("https://github.com/acme/shop/pull/{n}");
        let report = ReportPhase {
            session: command.id,
            event: EventId(ids.new_id()),
            at: clock().0,
            message_id: uuid::Uuid::now_v7(),
            sent_at: None,
            phase: Phase::Running,
            message: String::new(),
            observed_generation: 1,
            pull_request: upstream.pull_request(&url),
        };
        store.report_phase(&report).await.unwrap();
        sessions.push((command.id, url));
    }
    let listed = || async {
        let rows = store.unlinked_pull_requests(10).await.unwrap();
        rows.into_iter().map(|s| s.id).collect::<Vec<_>>()
    };
    let [(first, first_url), (second, second_url)] = <[_; 2]>::try_from(sessions).unwrap();
    assert_eq!(listed().await, [first, second], "oldest first");
    store
        .pull_request_link_failed(first, &first_url, clock().0, false)
        .await
        .unwrap();
    assert_eq!(listed().await, [second, first], "the tried one waits");
    store
        .pull_request_link_failed(second, &second_url, clock().0, true)
        .await
        .unwrap();
    assert_eq!(listed().await, [first], "the refused one ends");
}
