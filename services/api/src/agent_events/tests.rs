#![allow(clippy::unwrap_used)]

use rl_testkit::{FixedClock, SeqIds};

use super::*;

const SESSION: &str = "0190f3a0-0000-7000-8000-000000000001";

fn session() -> SessionId {
    SessionId(Uuid::parse_str(SESSION).unwrap())
}

fn parse(bytes: &[u8]) -> Result<ReportAgentEvent, &'static str> {
    let clock = FixedClock(SystemTime::UNIX_EPOCH);
    command(bytes, session(), &clock, &SeqIds::default())
}

fn event(r#type: &str, payload: &serde_json::Value) -> Vec<u8> {
    serde_json::json!({
        "messageId": "0190f3a0-0000-7000-8000-0000000000aa",
        "sessionId": SESSION,
        "at": "2026-11-13T10:00:00.000Z",
        "type": r#type,
        "payload": payload,
    })
    .to_string()
    .into_bytes()
}

/// The schema's examples pass or fail here as they do against the schema, with their session
/// as the subject's.
#[test]
fn the_schema_s_examples_hold() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../api/examples");
    for (dir, valid) in [
        (examples.to_owned(), true),
        (format!("{examples}/invalid"), false),
    ] {
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if !name.starts_with("agent-event.") {
                continue;
            }
            let text = std::fs::read(&path).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&text).unwrap();
            let own = value["sessionId"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok());
            let own = SessionId(own.unwrap_or_else(Uuid::nil));
            let clock = FixedClock(SystemTime::UNIX_EPOCH);
            let made = command(&text, own, &clock, &SeqIds::default());
            assert_eq!(made.is_ok(), valid, "{}", path.display());
            seen += 1;
        }
        assert!(seen > 0, "no agent-event examples in {dir}");
    }
}

#[test]
fn an_ask_opens_the_question_and_a_progress_closes_it() {
    let ask = parse(&event(
        "ask",
        &serde_json::json!({ "question": "Sign in?" }),
    ))
    .unwrap();
    assert_eq!(ask.session, session());
    assert_eq!(ask.question(), rl_domain::Question::Ask("Sign in?"));
    let turn = serde_json::json!({ "stage": "turn", "note": "Claude is working" });
    assert_eq!(
        parse(&event("progress", &turn)).unwrap().question(),
        rl_domain::Question::Clear
    );
}

#[test]
fn events_hold_to_the_schema_and_their_session() {
    let long = "é".repeat(2001);
    let bad = [
        event("ask", &serde_json::json!({ "question": "" })),
        event("ask", &serde_json::json!({ "question": long })),
        event(
            "ask",
            &serde_json::json!({ "question": "q", "options": ["only one"] }),
        ),
        event(
            "ask",
            &serde_json::json!({ "question": "q", "options": ["a", "a"] }),
        ),
        event(
            "ask",
            &serde_json::json!({ "question": "q", "to": "another lane" }),
        ),
        event(
            "progress",
            &serde_json::json!({ "stage": "Tests", "note": "n" }),
        ),
        event(
            "worker_done",
            &serde_json::json!({ "summary": "s", "headSha": "ABC" }),
        ),
        event(
            "compacted",
            &serde_json::json!({ "trigger": "sometimes", "summary": "s" }),
        ),
        event("chat", &serde_json::json!({ "text": "hi" })),
        // Security review H2: Postgres text can't hold a NUL; it would fail on every delivery.
        event("ask", &serde_json::json!({ "question": "a\u{0}b" })),
        event(
            "progress",
            &serde_json::json!({ "stage": "turn", "note": "\u{1b}[2J" }),
        ),
        br#"{"messageId":"x"}"#.to_vec(),
        b"not json".to_vec(),
    ];
    for bytes in bad {
        assert_eq!(
            parse(&bytes).err(),
            Some("invalid"),
            "{}",
            String::from_utf8_lossy(&bytes)
        );
    }
    let mut other = serde_json::from_slice::<serde_json::Value>(&event(
        "ask",
        &serde_json::json!({ "question": "q" }),
    ))
    .unwrap();
    other["sessionId"] = "0190f3a0-0000-7000-8000-000000000002".into();
    assert_eq!(
        parse(other.to_string().as_bytes()).err(),
        Some("another_session")
    );
    let huge = vec![b' '; MAX_BYTES + 1];
    assert_eq!(parse(&huge).err(), Some("too_large"));
}

#[test]
fn a_newline_and_a_tab_are_text() {
    let ask = event("ask", &serde_json::json!({ "question": "One:\n\tTwo?" }));
    assert!(parse(&ask).is_ok());
}

/// AC4: a session sends at most RATE events a window; others aren't held back, and the next
/// window starts again.
#[test]
fn a_session_sends_at_most_rate_a_window() {
    let mut rate = Rate::default();
    let (now, other) = (Instant::now(), SessionId(Uuid::nil()));
    for _ in 0..RATE {
        assert!(rate.allow(session(), now, 1));
    }
    assert!(!rate.allow(session(), now, 1), "past the rate");
    assert!(rate.allow(other, now, 1), "another session");
    assert!(rate.allow(session(), now + WINDOW, 1), "the next window");
}

/// Security review M3: a redelivery (the store was away) doesn't count, so a database blip never
/// drops a lane's sign-in ask as a flood.
#[test]
fn a_redelivery_doesnt_count() {
    let mut rate = Rate::default();
    let now = Instant::now();
    for delivery in 1..=100 {
        assert!(rate.allow(session(), now, delivery), "delivery {delivery}");
    }
    for _ in 1..RATE {
        assert!(rate.allow(session(), now, 1));
    }
    assert!(
        !rate.allow(session(), now, 1),
        "only first deliveries filled the window"
    );
}
