//! Who may do what on NATS (RAN-43, ADR 0021 amendment), end to end: a real nats-server with the
//! bundled chart's sign-in config, the api's auth callout, fake cluster token issuers (the home
//! cluster with runner r1, a remote one with runner r2), and real Postgres.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod control_plane;
mod events;
mod lane;
mod runner;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use api::config::Issuer;
use api::nats_auth::{Callout, Identity, Settings, Verifier};
use async_nats::jetstream;
use async_nats::{Client, ConnectOptions, Event, ServerError};
use rl_app::{SystemClock, UuidV7};
use rl_domain::{EventId, INTAKE_LABEL, Phase, ReportPhase, SessionId, subject};
use rl_store::Store;
use rl_testkit::{AuthNats, FakeIssuer, FakeTickets, NATS_AUDIENCE, now, ticket};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};
use uuid::Uuid;

const ORG: &str = "acme";

struct World {
    nats: AuthNats,
    home: FakeIssuer,
    remote: FakeIssuer,
    store: Store,
    callout: Arc<Callout>,
    _stop: DropGuard,
}

fn issuer(fake: &FakeIssuer, control_plane: bool, runner: &str) -> Issuer {
    Issuer {
        url: fake.url.clone(),
        jwks_url: fake.jwks_url.clone(),
        jwks_token_file: None,
        jwks_ca_file: None,
        control_plane,
        runner: runner.into(),
        runner_service_account: "rangelight-runner:runner".into(),
    }
}

/// The server, the issuers, a database, and the callout answering until the world is dropped.
async fn world(test: &'static str) -> World {
    let (nats, home, remote) = (
        AuthNats::start().await,
        FakeIssuer::start().await,
        FakeIssuer::start().await,
    );
    let store = rl_testkit::store(test).await;
    let issuers = vec![issuer(&home, true, "r1"), issuer(&remote, false, "r2")];
    let client = Callout::connect(&nats.url, &nats.auth_user_seed.clone().into())
        .await
        .unwrap();
    let settings = Settings {
        org: ORG.into(),
        sessions_runner: "r1".into(),
        account: "RL".into(),
        lane_account: "LANE".into(),
        issuer_seed: nats.issuer_seed.clone().into(),
        lane_issuer_seed: None,
    };
    let verifier = Verifier::new(issuers).unwrap();
    let callout = Arc::new(Callout::new(client, store.clone(), verifier, settings).unwrap());
    let requests = callout.listen().await.unwrap();
    let stop = CancellationToken::new();
    tokio::spawn(Arc::clone(&callout).serve(requests, stop.clone()));
    World {
        nats,
        home,
        remote,
        store,
        callout,
        _stop: stop.drop_guard(),
    }
}

/// A client signed in with `token`, its replies under `inbox`, and the errors the server sends it.
struct Signed {
    client: Client,
    errors: mpsc::UnboundedReceiver<String>,
}

async fn sign_in(url: &str, token: Option<String>, inbox: &str) -> Option<Signed> {
    let (errors, received) = mpsc::unbounded_channel();
    let mut options = ConnectOptions::new()
        .custom_inbox_prefix(subject::inbox(inbox))
        .event_callback(move |event| {
            let errors = errors.clone();
            async move {
                if let Event::ServerError(ServerError::Other(error)) = event {
                    let _ = errors.send(error);
                }
            }
        });
    if let Some(token) = token {
        options = options.token(token);
    }
    let client = options.connect(url).await.ok()?;
    Some(Signed {
        client,
        errors: received,
    })
}

impl World {
    async fn api(&self) -> Signed {
        let token = self.home.token("rangelight-system", "rangelight-api");
        sign_in(&self.nats.url, Some(token), "api").await.unwrap()
    }

    async fn relay(&self) -> Signed {
        let token = self.home.token("rangelight-system", "rangelight-relay");
        sign_in(&self.nats.url, Some(token), "relay").await.unwrap()
    }

    /// Runner r1 at home, or r2 in the remote cluster.
    async fn runner(&self, home: bool) -> Signed {
        let (issuer, inbox) = if home {
            (&self.home, "runner-r1")
        } else {
            (&self.remote, "runner-r2")
        };
        let token = issuer.token("rangelight-runner", "runner");
        sign_in(&self.nats.url, Some(token), inbox).await.unwrap()
    }

    async fn lane(&self, namespace: &str) -> Option<Signed> {
        let token = self.home.pod_token(namespace, "default", "agent");
        sign_in(&self.nats.url, Some(token), "lane").await
    }

    /// A hot session from ticket `n`, in the namespace rl-ran-<n>.
    async fn session(&self, n: u32) -> SessionId {
        let tickets = FakeTickets(vec![ticket(n, &[INTAKE_LABEL])]);
        let workspace = rl_testkit::workspace();
        api::intake_once(&self.store, &tickets, &workspace, &SystemClock, &UuidV7)
            .await
            .unwrap();
        let namespace = format!("rl-ran-{n}");
        let row = self.store.session_by_namespace(&namespace).await.unwrap();
        row.unwrap().id
    }

    /// [`World::session`], reported running by its runner: a lane may sign in.
    async fn started_session(&self, n: u32) -> SessionId {
        let session = self.session(n).await;
        self.report(session, Phase::Running).await;
        session
    }

    /// Records `phase` as the runner's latest report on `session`.
    async fn report(&self, session: SessionId, phase: Phase) {
        let report = ReportPhase {
            session,
            event: EventId(Uuid::now_v7()),
            at: SystemTime::now(),
            message_id: Uuid::now_v7(),
            sent_at: None,
            phase,
            message: String::new(),
            observed_generation: 1,
            pull_request: None,
        };
        assert!(self.store.report_phase(&report).await.unwrap());
    }
}

impl Signed {
    /// Waits for the server to report a permission violation naming `subject`.
    async fn refused(&mut self, subject: &str) {
        let violation = |e: &str| e.contains("Permissions Violation") && e.contains(subject);
        self.told_where(violation, subject).await;
    }

    /// Waits for the server to say `text`, in any case.
    async fn told(&mut self, text: &str) {
        self.told_where(|e: &str| e.to_lowercase().contains(text), text)
            .await;
    }

    async fn told_where(&mut self, matches: impl Fn(&str) -> bool, what: &str) {
        let wait = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(error) = self.errors.recv().await {
                if matches(&error) {
                    return;
                }
            }
        });
        assert!(wait.await.is_ok(), "the server never said {what}");
    }

    fn jetstream(&self) -> jetstream::Context {
        let mut context = jetstream::new(self.client.clone());
        context.set_timeout(Duration::from_secs(2));
        context
    }
}

/// AC4: no token, a token for another audience, an expired one, one that isn't signed, and one of
/// a ServiceAccount that is no one are all refused.
#[tokio::test]
async fn ac4_a_client_without_credentials_is_refused() {
    let world = world("nats_auth::refused").await;
    let url = &world.nats.url;
    assert!(sign_in(url, None, "x").await.is_none(), "anonymous");
    assert!(
        sign_in(url, Some("not-a-token".into()), "x")
            .await
            .is_none()
    );
    let claims = |aud: &str, exp: u64| {
        json!({"iss": world.home.url, "aud": [aud], "exp": exp,
            "sub": "system:serviceaccount:rangelight-system:rangelight-api"})
    };
    let kubernetes = world
        .home
        .sign(&claims("https://kubernetes.default.svc", now() + 60));
    assert!(
        sign_in(url, Some(kubernetes), "api").await.is_none(),
        "wrong audience"
    );
    let expired = world.home.sign(&claims(NATS_AUDIENCE, now() - 1));
    assert!(
        sign_in(url, Some(expired), "api").await.is_none(),
        "expired"
    );
    let web = world.home.token("rangelight-system", "rangelight-web");
    assert!(sign_in(url, Some(web), "x").await.is_none(), "no one");
    // The control plane's names, from a runner's cluster.
    let remote_api = world.remote.token("rangelight-system", "rangelight-api");
    assert!(sign_in(url, Some(remote_api), "api").await.is_none());
    assert!(
        sign_in(
            url,
            Some(world.home.token("rangelight-system", "rangelight-api")),
            "api"
        )
        .await
        .is_some()
    );
}

/// A user JWT lives until its token expires, an hour at most.
#[tokio::test]
async fn a_user_jwt_lives_no_longer_than_its_token_or_an_hour() {
    let world = world("nats_auth::lifetime").await;
    let token = |exp: u64| {
        world.home.sign(
            &json!({"iss": world.home.url, "aud": [NATS_AUDIENCE], "exp": exp,
            "sub": "system:serviceaccount:rangelight-runner:runner"}),
        )
    };
    let (soon, later) = (now() + 60, now() + 7200);
    let short = world
        .callout
        .authorize(Some(&token(soon)), now())
        .await
        .unwrap();
    assert_eq!(short, (Identity::Runner("r1".into()), soon));
    let (_, expires) = world
        .callout
        .authorize(Some(&token(later)), now())
        .await
        .unwrap();
    assert!(expires <= now() + 3600, "an hour at most");
}

/// A client reads its token file at every connect: a rotated token is the one it signs in with
/// next, and a bad one keeps it out until the file holds a good one again.
#[tokio::test]
async fn the_token_file_is_read_at_every_connect() {
    let world = world("nats_auth::rotation").await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("token");
    std::fs::write(
        &path,
        world.home.token("rangelight-system", "rangelight-relay"),
    )
    .unwrap();
    let client = rl_nats::options(path.clone(), "relay")
        .retry_on_initial_connect()
        .connect(&world.nats.url)
        .await
        .unwrap();
    std::fs::write(&path, "expired").unwrap();
    client.force_reconnect().await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let connected = || client.connection_state() == async_nats::connection::State::Connected;
    assert!(!connected(), "kept out with a bad token");
    std::fs::write(
        &path,
        world.home.token("rangelight-system", "rangelight-relay"),
    )
    .unwrap();
    let mut back = false;
    for _ in 0..300 {
        back = connected();
        if back {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(back, "back with the rotated token");
}

/// RAN-79: the api's first NATS connect at start, the callout's, waits while NATS is away and
/// connects within one retry pause once it is back (its other clients use the same
/// `rl_nats::connect`). A refused sign-in still fails at once.
#[tokio::test]
async fn ran79_the_api_waits_for_nats_at_start_but_not_for_a_bad_key() {
    let mut nats = AuthNats::start().await;
    nats.stop();
    let (url, seed) = (nats.url.clone(), nats.auth_user_seed.clone());
    let connect = tokio::spawn(async move { Callout::connect(&url, &seed.into()).await });
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !connect.is_finished(),
        "the api gave up while NATS was away"
    );
    nats.start_again().await;
    let client = tokio::time::timeout(rl_nats::MAX_RETRY + Duration::from_secs(2), connect)
        .await
        .expect("connected within one retry pause")
        .unwrap()
        .unwrap();
    assert_eq!(
        client.connection_state(),
        async_nats::connection::State::Connected
    );

    let stranger = nkeys::KeyPair::new_user().seed().unwrap();
    let refused = tokio::time::timeout(
        Duration::from_secs(10),
        Callout::connect(&nats.url, &stranger.into()),
    )
    .await
    .expect("a refused sign-in isn't retried");
    assert!(refused.is_err());
}
