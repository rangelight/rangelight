#![allow(clippy::unwrap_used)]

use super::*;

/// AC3: a client logs at most `PER_MINUTE` lines a minute; the next line after says how many were
/// left out; another client isn't held back.
#[test]
fn a_looping_client_can_t_flood_the_log() {
    let (lines, now) = (Lines::default(), Instant::now());
    for _ in 0..PER_MINUTE {
        assert_eq!(lines.allow("10.0.0.1", now), Some(0));
    }
    for _ in 0..5 {
        assert_eq!(lines.allow("10.0.0.1", now), None);
    }
    assert_eq!(lines.allow("10.0.0.2", now), Some(0), "another client");
    assert_eq!(
        lines.allow("10.0.0.1", now + WINDOW),
        Some(5),
        "the next minute says how many"
    );
}

/// A refusal's reason is a code from its Connect error, and nothing else of the body.
#[test]
fn a_reason_is_a_code_and_never_free_text() {
    let body = |code: &str, message: &str| {
        serde_json::json!({ "code": code, "message": message })
            .to_string()
            .into_bytes()
    };
    assert_eq!(
        reason(&body("permission_denied", "cross_origin")),
        "cross_origin"
    );
    assert_eq!(
        reason(&body("not_found", "no such session: abc")),
        "not_found"
    );
    assert_eq!(reason(&body("Weird Code", "__Host-rl_session=secret")), "");
    assert_eq!(reason(b"Sign-in failed."), "");
}
