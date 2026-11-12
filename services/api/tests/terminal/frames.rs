//! What frames pass: the schema, and nothing of them in the logs.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::{lane, next, received};

#[tokio::test]
async fn a_frame_that_breaks_the_schema_closes_with_1008() {
    let lane = lane("api::terminal_invalid").await;
    let too_big = format!(r#"{{"type":"input","data":"{}"}}"#, "A".repeat(65_540));
    let bad = [
        Message::text(r#"{"type":"input","data":"not base64!"}"#),
        Message::text(r#"{"type":"output","data":"aGk="}"#),
        Message::text(r#"{"type":"resize","cols":0,"rows":1}"#),
        Message::text(too_big),
        // Past the socket's read limit: refused before it is buffered.
        Message::text("A".repeat(200_000)),
        Message::binary(vec![1, 2, 3]),
    ];
    for frame in bad {
        let mut input = lane.agent_input().await;
        let mut socket = lane.terminal().await;
        socket.send(frame).await.unwrap();
        assert_eq!(next(&mut socket).await, Err(Some(CloseCode::Policy)));
        let nothing = tokio::time::timeout(Duration::from_millis(200), input.next()).await;
        assert!(nothing.is_err(), "an invalid frame reached the agent");
    }
}

/// A `tracing` writer into memory.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// AC5 (ADR 0046 §3.4): a session that types a sign-in code, and gets one echoed, leaves it in no
/// log line at any level, valid frames or invalid, the NATS and WebSocket crates' own included.
#[tokio::test]
async fn ac5_terminal_contents_are_never_logged() {
    let logs = Logs::default();
    let writer = logs.clone();
    // The services' own log setup, at its most verbose.
    rl_telemetry::init_to("trace", move || writer.clone()).unwrap();
    let lane = lane("api::terminal_logs").await;
    // "sk-ant-oat01-FAKECODE" in base64, typed, echoed, and sent once more as a broken frame.
    let code = "c2stYW50LW9hdDAxLUZBS0VDT0RF";
    let mut input = lane.agent_input().await;
    let mut socket = lane.terminal().await;
    let typed = format!(r#"{{"type":"input","data":"{code}"}}"#);
    socket.send(Message::text(typed.clone())).await.unwrap();
    received(&mut input).await;
    let echoed = format!(r#"{{"type":"output","data":"{code}"}}"#);
    lane.agent_writes(&[&echoed, &typed]).await;
    assert!(next(&mut socket).await.unwrap().contains(code));
    let broken = format!(r#"{{"type":"input","data":"{code}","x":1}}"#);
    socket.send(Message::text(broken)).await.unwrap();
    assert_eq!(next(&mut socket).await, Err(Some(CloseCode::Policy)));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("terminal closed"), "logging works: {logs}");
    for secret in [code, "sk-ant-oat01-FAKECODE", "FAKECODE"] {
        let leaks: Vec<_> = logs.lines().filter(|l| l.contains(secret)).collect();
        assert!(leaks.is_empty(), "terminal contents in the logs: {leaks:?}");
    }
}
