#![allow(clippy::unwrap_used)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use proptest::prelude::*;
use uuid::Uuid;

use super::*;

/// One event as stored: `prev_hash`, id, at, payload, hash.
type Stored = (Vec<u8>, EventId, i64, Vec<u8>, Vec<u8>);

/// Events as an append makes them, from (id, at, payload).
fn build(events: &[(u128, i64, Vec<u8>)]) -> Vec<Stored> {
    let mut prev = GENESIS.to_vec();
    events
        .iter()
        .map(|(id, at, payload)| {
            let id = EventId(Uuid::from_u128(*id));
            let hash = link(
                &prev,
                &Record {
                    id,
                    at: *at,
                    payload,
                },
            )
            .to_vec();
            let stored = (prev.clone(), id, *at, payload.clone(), hash.clone());
            prev = hash;
            stored
        })
        .collect()
}

fn links(chain: &[Stored]) -> impl Iterator<Item = Link<'_>> {
    chain.iter().map(|(prev_hash, id, at, payload, hash)| Link {
        prev_hash,
        record: Record {
            id: *id,
            at: *at,
            payload,
        },
        hash,
    })
}

fn events() -> impl Strategy<Value = Vec<(u128, i64, Vec<u8>)>> {
    prop::collection::vec(
        (
            any::<u128>(),
            any::<i64>(),
            prop::collection::vec(any::<u8>(), 0..64),
        ),
        1..16,
    )
}

proptest! {
    #[test]
    fn a_built_chain_verifies(events in events()) {
        prop_assert_eq!(verify(links(&build(&events))), Ok(()));
    }

    #[test]
    fn a_changed_id_time_or_payload_is_caught(
        events in events(),
        pick in any::<prop::sample::Index>(),
        field in 0..3u8,
        flip in 1u8..,
    ) {
        let mut chain = build(&events);
        let index = pick.index(chain.len());
        let event = &mut chain[index];
        match field {
            0 => event.1 = EventId(Uuid::from_u128(event.1.0.as_u128() ^ u128::from(flip))),
            1 => event.2 ^= i64::from(flip),
            _ => event.3.push(flip),
        }
        prop_assert_eq!(verify(links(&chain)), Err(ChainError::Tampered(index)));
    }

    #[test]
    fn a_removed_event_is_caught(events in events(), pick in any::<prop::sample::Index>()) {
        prop_assume!(events.len() >= 2);
        let mut chain = build(&events);
        let index = pick.index(chain.len() - 1);
        chain.remove(index);
        prop_assert_eq!(verify(links(&chain)), Err(ChainError::Unlinked(index)));
    }

    #[test]
    fn micros_round_trip(at in any::<i64>()) {
        prop_assert_eq!(micros(time(at)), at);
    }

    #[test]
    fn micros_round_toward_the_past(secs in -4_000_000_000i64..4_000_000_000, nanos in 0u32..1_000_000_000) {
        let offset = Duration::new(secs.unsigned_abs(), nanos);
        let at = if secs >= 0 { UNIX_EPOCH + offset } else { UNIX_EPOCH - offset };
        let truncated = time(micros(at));
        prop_assert!(truncated <= at);
        prop_assert!(at.duration_since(truncated).unwrap() < Duration::from_micros(1));
    }
}

#[test]
fn the_canonical_encoding_is_fixed() {
    // A known vector: changing the encoding breaks every stored chain, so it must fail here first.
    let record = Record {
        id: EventId(Uuid::from_u128(1)),
        at: 1_700_000_000_000_000,
        payload: b"change",
    };
    let hash = link(&GENESIS, &record);
    let mut expected = Sha256::new();
    expected.update([0u8; 32]);
    expected.update([0u8; 15]);
    expected.update([1u8]);
    expected.update(1_700_000_000_000_000i64.to_be_bytes());
    expected.update(b"change");
    assert_eq!(hash, <[u8; 32]>::from(expected.finalize()));
    assert_eq!(micros(SystemTime::UNIX_EPOCH - Duration::from_nanos(1)), -1);
}

#[test]
fn payload_is_deterministic_and_decodes() {
    let change = Change::Created {
        ticket_id: "issue-1".into(),
        ticket_key: "RAN-1".into(),
        title: "A title".into(),
        namespace: "rl-ran-1".into(),
        state: State::Hot,
    };
    assert_eq!(payload(&change), payload(&change.clone()));
    assert_eq!(decode(&payload(&change)).unwrap(), to_proto(&change));
    assert!(decode(&[0xff, 0xff]).is_err());
}
