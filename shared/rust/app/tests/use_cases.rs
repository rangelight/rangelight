//! The use cases, against the seams' fakes.
#![allow(clippy::unwrap_used)]

use std::time::SystemTime;

use rl_app::{change_state, intake};
use rl_domain::{INTAKE_LABEL, SessionId, State};
use rl_testkit::{FakeTickets, FixedClock, SeqIds, ticket, ticket_source_contract};
use uuid::Uuid;

#[tokio::test]
async fn intake_creates_one_command_per_listed_ticket() {
    let source = FakeTickets(vec![ticket(1, &[INTAKE_LABEL]), ticket(2, &["bug"])]);
    let clock = FixedClock(SystemTime::UNIX_EPOCH);
    let commands = intake(
        &source,
        &rl_testkit::workspace(),
        &clock,
        &SeqIds::default(),
    )
    .await
    .unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].ticket.id(), "issue-1");
    assert_eq!(commands[0].at, SystemTime::UNIX_EPOCH);
    assert_ne!(commands[0].id.0, commands[0].event.0);
}

#[tokio::test]
async fn change_state_takes_its_event_id_and_time_from_the_seams() {
    let ids = SeqIds::default();
    let command = change_state(
        SessionId(Uuid::nil()),
        State::Warm,
        &FixedClock(SystemTime::UNIX_EPOCH),
        &ids,
    );
    assert_eq!(command.event.0, Uuid::from_u128(1));
    assert_eq!(command.to, State::Warm);
}

#[tokio::test]
async fn the_fake_passes_the_ticket_source_contract() {
    ticket_source_contract(|tickets| async move { FakeTickets(tickets) }).await;
}
