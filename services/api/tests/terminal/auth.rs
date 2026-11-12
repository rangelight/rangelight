//! Who may open a terminal, and for how long.

use std::time::Duration;

use rl_testkit::PUBLIC_URL;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use crate::common::{self, RECHECK};
use crate::{PROTOCOL, connect, lane, lane_with, next, refused};

/// No live login is 401 and a foreign Origin is 403 (RAN-37's checks, the only auth path), a client
/// without the subprotocol is refused, and so is a session that doesn't exist.
#[tokio::test]
async fn the_upgrade_needs_a_login_the_origin_the_subprotocol_and_a_session() {
    let lane = lane("api::terminal_refused").await;
    let id = lane.session.0.to_string();
    let (cookie, origin, p) = (Some(lane.cookie.as_str()), Some(PUBLIC_URL), Some(PROTOCOL));
    let forged = Some("__Host-rl_session=forged");
    assert_eq!(refused(lane.open(&id, None, origin, p).await), 401);
    assert_eq!(refused(lane.open(&id, forged, origin, p).await), 401);
    let evil = Some("https://evil.test");
    assert_eq!(refused(lane.open(&id, cookie, evil, p).await), 403);
    assert_eq!(refused(lane.open(&id, cookie, None, p).await), 403);
    assert_eq!(refused(lane.open(&id, cookie, origin, None).await), 400);
    let chat = Some("chat");
    assert_eq!(refused(lane.open(&id, cookie, origin, chat).await), 400);
    let unknown = uuid::Uuid::now_v7().to_string();
    assert_eq!(refused(lane.open(&unknown, cookie, origin, p).await), 404);
    assert_eq!(refused(lane.open("rl.%3E", cookie, origin, p).await), 400);
}

/// Only the session's own member may open its terminal (review 1): kilgore's mixed-case login
/// `Kilgore` opens it (every other test too), another signed-in member of the org gets 403.
#[tokio::test]
async fn another_member_cant_open_the_terminal() {
    let lane = lane("api::terminal_other_member").await;
    // Under Dex's `name` claim impostor is member `impostor` (under `preferred_username` it would
    // claim `kilgore`, which kilgore's account owns).
    let by_name = common::served_with(&lane.db.store().await, &lane.org, "name", RECHECK).await;
    let cookie = by_name
        .signed_in("impostor@example.com")
        .await
        .cookie_header();
    let id = lane.session.0.to_string();
    let opened = connect(
        &by_name.base,
        &id,
        Some(&cookie),
        Some(PUBLIC_URL),
        Some(PROTOCOL),
    );
    assert_eq!(refused(opened.await), 403);
}

/// A login holds at most four terminals at once, 429 past them (review 2); closing one frees it.
#[tokio::test]
async fn a_login_holds_at_most_four_terminals() {
    let lane = lane("api::terminal_per_login").await;
    let mut open = Vec::new();
    for _ in 0..4 {
        open.push(lane.terminal().await);
    }
    assert_eq!(refused(lane.try_terminal().await), 429);
    let mut first = open.remove(0);
    first.close(None).await.unwrap();
    assert!(next(&mut first).await.is_err(), "the api answers the close");
    // The slot frees once the api's side of the socket has ended, just after its close frame.
    let mut reopened = false;
    for _ in 0..50 {
        reopened = lane.try_terminal().await.is_ok();
        if reopened {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(reopened, "the closed terminal's slot was never freed");
}

/// An open terminal closes once its login is gone (RAN-37 review M1: every long-lived response).
#[tokio::test]
async fn the_terminal_closes_when_its_login_is_gone() {
    let mut lane = lane_with("api::terminal_signed_out", Duration::from_millis(300)).await;
    let mut socket = lane.terminal().await;
    assert_eq!(lane.browser.post("/auth/logout").await.status.as_u16(), 204);
    assert_eq!(next(&mut socket).await, Err(Some(CloseCode::Policy)));
}
