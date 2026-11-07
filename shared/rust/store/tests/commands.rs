//! Commands against real Postgres: state, chained event and outbox row in one transaction.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant, SystemTime};

use rl_app::{IdGen, change_state, intake};
use rl_chain::{Link, Record};
use rl_domain::{INTAKE_LABEL, NewSession, SessionId, State};
use rl_proto::rangelight::session::v1alpha1::__buffa::oneof::change::Kind;
use rl_store::{ChainEvent, SessionRow, Store, StoreError};
use rl_testkit::{Database, FakeTickets, FixedClock, SeqIds, ticket};

/// A time with nanoseconds, which the chain truncates to microseconds.
fn clock() -> FixedClock {
    FixedClock(SystemTime::UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789))
}

async fn new_session(n: u32, ids: &SeqIds) -> NewSession {
    let source = FakeTickets(vec![ticket(n, &[INTAKE_LABEL])]);
    intake(&source, &clock(), ids).await.unwrap().remove(0)
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
