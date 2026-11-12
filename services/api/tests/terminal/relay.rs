//! How the api carries frames between the browser and the agent.

use std::time::Duration;

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::{lane, next, received};

#[tokio::test]
async fn input_and_resize_reach_the_agent_and_its_output_reaches_the_browser() {
    let lane = lane("api::terminal_bridge").await;
    let mut input = lane.agent_input().await;
    let mut socket = lane.terminal().await;
    let typed = r#"{"type":"input","data":"bHMNCg=="}"#;
    let resize = r#"{"type":"resize","cols":120,"rows":40}"#;
    socket.send(Message::text(typed)).await.unwrap();
    socket.send(Message::text(resize)).await.unwrap();
    assert_eq!(received(&mut input).await, typed, "forwarded unchanged");
    assert_eq!(received(&mut input).await, resize);

    // Only a valid output frame from the lane reaches the browser; the rest is dropped.
    let output = r#"{"type":"output","data":"aGk="}"#;
    lane.agent_writes(&[r#"{"type":"input","data":"aGk="}"#, "garbage", output])
        .await;
    assert_eq!(next(&mut socket).await, Ok(output.to_owned()));
}

/// Review 2 and 3: a browser that stops reading costs the api a bounded queue, not the lane's whole
/// output (past 64 queued messages it is dropped), and after 10 s without taking a frame its
/// terminal closes with 1011.
#[tokio::test]
async fn a_browser_that_stops_reading_loses_output_and_is_closed() {
    const WRITTEN: usize = 600;
    let lane = lane("api::terminal_stalled").await;
    let mut socket = lane.terminal().await;
    // 600 frames of 48 KiB, about 39 MB: far more than the connection's buffers hold.
    let frame = format!(r#"{{"type":"output","data":"{}"}}"#, "A".repeat(65_536));
    lane.agent_writes(&[frame.as_str(); WRITTEN]).await;
    // The send stalls once the buffers are full; its 10 s run out, then the api sends its close
    // frame (also within 10 s), which this browser takes by reading again.
    tokio::time::sleep(Duration::from_secs(15)).await;
    let mut got = 0;
    let end = loop {
        match next(&mut socket).await {
            Ok(_) => got += 1,
            Err(code) => break code,
        }
    };
    assert!(got < WRITTEN, "nothing was dropped: {got} frames arrived");
    assert_eq!(end, Some(CloseCode::Error));
}
