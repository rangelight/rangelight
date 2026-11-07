#![allow(clippy::unwrap_used)]

use std::time::SystemTime;

use rl_proto::rangelight::runner::v1alpha1::{Phase, SessionStatus};
use rl_testkit::{FixedClock, SeqIds};

use super::*;

fn envelope(edit: impl FnOnce(&mut Envelope, &mut SessionStatus)) -> Vec<u8> {
    let mut status = SessionStatus {
        session_id: Uuid::nil().to_string(),
        phase: Phase::PHASE_RUNNING.into(),
        observed_generation: 2,
        ..Default::default()
    };
    let mut envelope = Envelope {
        message_id: Uuid::from_u128(9).to_string(),
        protocol_version: PROTOCOL.into(),
        runner_id: "r1".into(),
        ..Default::default()
    };
    edit(&mut envelope, &mut status);
    envelope.body = Some(Body::from(status));
    envelope.encode_to_vec()
}

#[test]
fn a_status_becomes_a_report_and_anything_else_is_refused() {
    let clock = FixedClock(SystemTime::UNIX_EPOCH);
    let parse = |payload: &[u8]| command(payload, "r1", &clock, &SeqIds::default());
    let report = parse(&envelope(|_, _| {})).unwrap();
    assert_eq!(report.message_id, Uuid::from_u128(9));
    assert_eq!(report.observed_generation, 2);
    let cases: [(Vec<u8>, &str); 6] = [
        (vec![0xff], "undecodable"),
        (
            envelope(|e, _| e.protocol_version = "v9".into()),
            "unsupported_protocol",
        ),
        (
            envelope(|e, _| e.runner_id = "r2".into()),
            "runner_mismatch",
        ),
        (envelope(|e, _| e.message_id = "x".into()), "message_id"),
        (envelope(|_, s| s.session_id = "*".into()), "session_id"),
        (
            envelope(|_, s| s.phase = Phase::PHASE_UNSPECIFIED.into()),
            "phase",
        ),
    ];
    for (payload, reason) in cases {
        assert_eq!(parse(&payload), Err(reason));
    }
    let not_a_status = Envelope {
        message_id: Uuid::nil().to_string(),
        protocol_version: PROTOCOL.into(),
        runner_id: "r1".into(),
        ..Default::default()
    };
    assert_eq!(parse(&not_a_status.encode_to_vec()), Err("not_a_status"));
}
