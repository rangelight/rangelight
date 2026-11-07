#![allow(clippy::unwrap_used)]

use super::*;

fn ticket(key: &str) -> Result<Ticket, DomainError> {
    Ticket::new(Tracker::Linear, "id-1".into(), key.into(), "Title".into())
}

#[test]
fn state_machine_allows_hibernate_wake_and_teardown() {
    assert_eq!(State::Hot.change_to(State::Warm), Ok(State::Warm));
    assert_eq!(State::Warm.change_to(State::Hot), Ok(State::Hot));
    assert_eq!(State::Hot.change_to(State::Cold), Ok(State::Cold));
    assert_eq!(State::Warm.change_to(State::Cold), Ok(State::Cold));
}

#[test]
fn state_machine_rejects_no_op_and_leaving_cold() {
    for (from, to) in [
        (State::Hot, State::Hot),
        (State::Cold, State::Hot),
        (State::Cold, State::Warm),
    ] {
        assert_eq!(
            from.change_to(to),
            Err(DomainError::Transition { from, to })
        );
    }
}

#[test]
fn state_names_round_trip_and_unknown_fails() {
    for state in [State::Hot, State::Warm, State::Cold] {
        assert_eq!(State::parse(state.as_str()), Ok(state));
    }
    assert!(State::parse("lukewarm").is_err());
}

#[test]
fn ticket_rejects_keys_that_cant_name_a_namespace() {
    for key in ["", "-RAN", "RAN-", "RAN 1", "RAN.1", "R/1", &"A".repeat(41)] {
        assert_eq!(
            ticket(key),
            Err(DomainError::InvalidTicket("key")),
            "{key:?}"
        );
    }
    let empty_id = Ticket::new(
        Tracker::Linear,
        String::new(),
        "RAN-1".into(),
        String::new(),
    );
    assert_eq!(empty_id, Err(DomainError::InvalidTicket("id")));
    let long_title = Ticket::new(
        Tracker::Linear,
        "i".into(),
        "RAN-1".into(),
        "t".repeat(1025),
    );
    assert_eq!(long_title, Err(DomainError::InvalidTicket("title")));
}

#[test]
fn new_session_is_hot_in_its_ticket_namespace() {
    let command = NewSession {
        id: SessionId(uuid::Uuid::nil()),
        event: EventId(uuid::Uuid::nil()),
        at: std::time::SystemTime::UNIX_EPOCH,
        ticket: ticket("RAN-34").unwrap(),
    };
    assert_eq!(command.namespace(), "rl-ran-34");
    assert_eq!(command.change().state(), State::Hot);
    assert_eq!(command.change().kind(), "session.created");
}

#[test]
fn subject_tokens_cant_widen_a_subject() {
    assert_eq!(subject::token("acme-1"), Ok("acme-1"));
    for bad in ["", "a.b", "*", ">", "a b", "a\n"] {
        assert!(subject::token(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_member_is_the_lowercased_login_or_nothing() {
    assert_eq!(
        Member::from_username("ArielLahiany").unwrap().as_str(),
        "ariellahiany"
    );
    assert_eq!(Member::from_username("a-1").unwrap().as_str(), "a-1");
    assert_eq!(
        Member::from_username(&"a".repeat(63))
            .unwrap()
            .as_str()
            .len(),
        63
    );
    for bad in [
        "",
        "-a",
        "a-",
        "a.b",
        "a_b",
        "a@example.com",
        "a b",
        "ı",
        &"a".repeat(64),
    ] {
        assert_eq!(
            Member::from_username(bad),
            Err(DomainError::InvalidMember),
            "{bad:?}"
        );
    }
}
