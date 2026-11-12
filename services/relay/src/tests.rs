#![allow(clippy::unwrap_used)]

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

#[tokio::test]
async fn healthz_answers_200_until_cancelled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let served = tokio::spawn(serve_health(listener, cancel.clone()));
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: relay\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    cancel.cancel();
    served.await.unwrap().unwrap();
}

#[test]
fn failed_rounds_back_off_to_a_cap_and_success_resets() {
    let failed = Err(RelayError::Nats("down".into()));
    let mut pause = Duration::ZERO;
    let mut pauses = Vec::new();
    for _ in 0..9 {
        pause = next_pause(pause, &failed);
        pauses.push(pause.as_millis());
    }
    assert_eq!(
        pauses,
        [500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
    );
    assert_eq!(next_pause(pause, &Ok(3)), IDLE);
    assert_eq!(next_pause(pause, &Ok(usize::from(BATCH))), Duration::ZERO);
}
