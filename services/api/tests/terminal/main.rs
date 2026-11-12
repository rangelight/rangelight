//! RAN-41: the lane terminal end to end: a real WebSocket client signed in through Dex, the api,
//! and real NATS, where the test plays the lane agent. By behaviour: who may open it (`auth`),
//! what frames pass (`frames`), and how it carries them (`relay`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "../common/mod.rs"]
mod common;

mod auth;
mod frames;
mod relay;

use std::time::Duration;

use futures_util::StreamExt;
use rl_app::{SystemClock, UuidV7};
use rl_domain::{INTAKE_LABEL, SessionId, Workspace, subject};
use rl_store::Store;
use rl_testkit::{Database, FakeTickets, PUBLIC_URL, ticket};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

const PROTOCOL: &str = "terminal.v1alpha1.rangelight.dev";
/// The session's member. Their login (`preferred_username` in the test Dex) is `Kilgore`: every
/// test opens the session of member `kilgore` with a mixed-case login.
const KILGORE: &str = "kilgore@example.com";
const WAIT: Duration = Duration::from_secs(5);

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A terminal to one session of kilgore's, signed in as kilgore, and the NATS side of it.
struct Lane {
    org: String,
    session: SessionId,
    served: common::Served,
    browser: rl_testkit::Browser,
    cookie: String,
    nats: async_nats::Client,
    db: Database,
}

async fn lane(test: &'static str) -> Lane {
    lane_with(test, common::RECHECK).await
}

/// [`lane`], with logins re-checked every `recheck`.
async fn lane_with(test: &'static str, recheck: Duration) -> Lane {
    let db = Database::new(test).await;
    let store: Store = db.store().await;
    let org = format!("t{}", uuid::Uuid::now_v7().simple());
    let source = FakeTickets(vec![ticket(1, &[INTAKE_LABEL])]);
    let workspace = Workspace::new("shop".into(), "main".into(), "kilgore".into()).unwrap();
    api::intake_once(&store, &source, &workspace, &SystemClock, &UuidV7)
        .await
        .unwrap();
    let session = store.list_sessions(1).await.unwrap()[0].id;
    let served = common::served_with(&store, &org, "preferred_username", recheck).await;
    let browser = served.signed_in(KILGORE).await;
    let cookie = browser.cookie_header();
    let nats = rl_testkit::nats().await;
    Lane {
        org,
        session,
        served,
        browser,
        cookie,
        nats,
        db,
    }
}

/// Opens a terminal of the api at `base` with these headers; `None` leaves one out.
async fn connect(
    base: &rl_auth::Url,
    session: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    protocol: Option<&str>,
) -> Result<Socket, Error> {
    let url = format!(
        "ws://{}/v1alpha1/sessions/{session}/terminal",
        base.authority()
    );
    let mut request = url.into_client_request().unwrap();
    let headers = request.headers_mut();
    for (name, value) in [
        ("cookie", cookie),
        ("origin", origin),
        ("sec-websocket-protocol", protocol),
    ] {
        if let Some(value) = value {
            headers.insert(name, value.parse().unwrap());
        }
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

impl Lane {
    /// Opens the terminal with these headers; `None` leaves one out.
    async fn open(
        &self,
        session: &str,
        cookie: Option<&str>,
        origin: Option<&str>,
        protocol: Option<&str>,
    ) -> Result<Socket, Error> {
        connect(&self.served.base, session, cookie, origin, protocol).await
    }

    /// Kilgore opens the session's terminal.
    async fn try_terminal(&self) -> Result<Socket, Error> {
        let id = self.session.0.to_string();
        self.open(&id, Some(&self.cookie), Some(PUBLIC_URL), Some(PROTOCOL))
            .await
    }

    async fn terminal(&self) -> Socket {
        self.try_terminal().await.unwrap()
    }

    /// What the agent reads.
    async fn agent_input(&self) -> async_nats::Subscriber {
        let input = self
            .nats
            .subscribe(subject::pty_in(&self.org, self.session))
            .await
            .unwrap();
        self.nats.flush().await.unwrap();
        input
    }

    /// The agent writes each of `frames`.
    async fn agent_writes(&self, frames: &[&str]) {
        let out = subject::pty_out(&self.org, self.session);
        for frame in frames {
            let frame = (*frame).to_owned();
            self.nats.publish(out.clone(), frame.into()).await.unwrap();
        }
        self.nats.flush().await.unwrap();
    }
}

/// The next text frame, or the close code.
async fn next(socket: &mut Socket) -> Result<String, Option<CloseCode>> {
    loop {
        match tokio::time::timeout(WAIT, socket.next())
            .await
            .expect("a frame in time")
        {
            Some(Ok(Message::Text(text))) => return Ok(text.to_string()),
            Some(Ok(Message::Close(frame))) => return Err(frame.map(|f| f.code)),
            Some(Ok(_)) => {}
            Some(Err(_)) | None => return Err(None),
        }
    }
}

/// The next message the agent reads.
async fn received(input: &mut async_nats::Subscriber) -> String {
    let message = tokio::time::timeout(WAIT, input.next())
        .await
        .expect("in time")
        .unwrap();
    String::from_utf8(message.payload.to_vec()).unwrap()
}

/// Status of a refused upgrade; 0 if it upgraded or failed some other way.
fn refused(result: Result<Socket, Error>) -> u16 {
    match result {
        Err(Error::Http(response)) => response.status().as_u16(),
        _ => 0,
    }
}
