//! Sign-in end to end (RAN-37): the api, real Postgres, NATS and Dex, a browser and real Connect
//! clients. Every way in without a live login is refused alike.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::http::header::{CONNECTION, CONTENT_TYPE, ORIGIN, UPGRADE};
use axum::http::{HeaderMap, Method, StatusCode};
use connectrpc::ErrorCode;
use rl_auth::{Provider, Started, Url};
use rl_config::SecretRef;
use rl_proto::rangelight::auth::v1alpha1::WhoAmIRequest;
use rl_proto::rangelight::session::v1alpha1::{ListSessionsRequest, WatchSessionRequest};
use rl_testkit::{Browser, Database, PUBLIC_URL};
use secrecy::ExposeSecret;

const KILGORE: &str = "kilgore@example.com";

fn org() -> String {
    format!("t{}", uuid::Uuid::now_v7().simple())
}

async fn app(test: &'static str) -> (Database, common::Served) {
    let db = Database::new(test).await;
    let served = common::served(&db.store().await, &org()).await;
    (db, served)
}

/// Every RPC refuses `cookie` with UNAUTHENTICATED and a message that says nothing more.
async fn refused(served: &common::Served, cookie: &str) {
    let errors = [
        served
            .sessions(cookie)
            .list_sessions(ListSessionsRequest::default())
            .await
            .err(),
        served
            .auth(cookie)
            .who_am_i(WhoAmIRequest::default())
            .await
            .err(),
        match served
            .sessions(cookie)
            .watch_session(WatchSessionRequest {
                session_id: uuid::Uuid::now_v7().to_string(),
                ..Default::default()
            })
            .await
        {
            // A wrongly accepted watch would wait for events for ever: give up after 5s.
            Ok(mut stream) => tokio::time::timeout(Duration::from_secs(5), stream.message())
                .await
                .expect("refused, not streaming")
                .err(),
            Err(error) => Some(error),
        },
    ];
    for error in errors {
        let error = error.expect("refused");
        assert_eq!(error.code, ErrorCode::Unauthenticated, "{error:?}");
        assert_eq!(error.message.as_deref(), Some("unauthenticated"));
    }
}

async fn who(served: &common::Served, browser: &Browser) -> String {
    let reply = served
        .auth(&browser.cookie_header())
        .who_am_i(WhoAmIRequest::default())
        .await
        .unwrap();
    reply.into_owned().member
}

/// AC2 and AC5: Dex signs kilgore in; the cookie is host-only, `HttpOnly`, `Secure` and `Lax`; the
/// member is the lowercased login; the RPCs answer.
#[tokio::test]
async fn signs_in_through_dex() {
    let (_db, served) = app("api::auth_sign_in").await;
    let mut browser = served.browser();
    let page = browser.sign_in(KILGORE).await;
    assert_eq!(page.status, StatusCode::SEE_OTHER, "{}", page.body);
    assert_eq!(page.location.as_deref(), Some("/"));
    let session = page
        .set_cookies
        .iter()
        .find(|c| c.starts_with("__Host-rl_session="))
        .unwrap();
    assert!(
        session.ends_with("; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800"),
        "{session}"
    );
    // 256 random bits, base64url.
    assert_eq!(browser.cookies["__Host-rl_session"].len(), 43);
    assert!(
        !browser.cookies.contains_key("__Host-rl_login"),
        "the sign-in cookie is cleared"
    );
    assert_eq!(who(&served, &browser).await, "kilgore");
    served
        .sessions(&browser.cookie_header())
        .list_sessions(ListSessionsRequest::default())
        .await
        .unwrap();
}

/// AC3: no cookie, an unknown login, and an empty cookie are refused by every RPC.
#[tokio::test]
async fn rpcs_refuse_a_missing_or_unknown_login() {
    let (_db, served) = app("api::auth_unknown").await;
    refused(&served, "").await;
    refused(
        &served,
        "__Host-rl_session=bm90LWEtbG9naW4tdG9rZW4tYXQtYWxsLW5vdC1ldmVu",
    )
    .await;
    refused(&served, "__Host-rl_session=").await;
    refused(&served, "rl_session=x; __Host-rl_login=y").await;
}

/// AC3: a login idle past 8 hours, and one past its absolute end, are refused.
#[tokio::test]
async fn expired_logins_are_refused() {
    let (db, served) = app("api::auth_expired").await;
    let idle = served.signed_in(KILGORE).await;
    db.execute(include_str!("idle_logins.sql")).await;
    refused(&served, &idle.cookie_header()).await;

    let old = served.signed_in(KILGORE).await;
    assert_eq!(who(&served, &old).await, "kilgore");
    db.execute(include_str!("expire_logins.sql")).await;
    refused(&served, &old.cookie_header()).await;
}

/// AC3 and AC4: sign-out deletes the login (its cookie no longer works) and clears the cookie;
/// it needs the public origin.
#[tokio::test]
async fn sign_out_ends_the_login_and_needs_the_origin() {
    let (_db, served) = app("api::auth_sign_out").await;
    let mut browser = served.signed_in(KILGORE).await;
    let cookie = browser.cookie_header();

    for origin in [
        None,
        Some("https://evil.example"),
        Some("http://rangelight.test"),
    ] {
        let mut headers = HeaderMap::new();
        if let Some(origin) = origin {
            headers.insert(ORIGIN, origin.parse().unwrap());
        }
        let page = browser.send(Method::POST, "/auth/logout", headers).await;
        assert_eq!(page.status, StatusCode::FORBIDDEN, "{origin:?}");
    }
    assert_eq!(who(&served, &browser).await, "kilgore", "still signed in");

    let page = browser.post("/auth/logout").await;
    assert_eq!(page.status, StatusCode::NO_CONTENT);
    assert!(
        page.set_cookies
            .iter()
            .any(|c| c.starts_with("__Host-rl_session=;"))
    );
    assert!(browser.cookies.is_empty());
    refused(&served, &cookie).await;
}

/// AC4: an RPC from another origin is refused even with a live login.
#[tokio::test]
async fn rpcs_need_the_public_origin() {
    let (_db, served) = app("api::auth_origin").await;
    let mut browser = served.signed_in(KILGORE).await;
    let path = "/rangelight.auth.v1alpha1.AuthService/WhoAmI";
    // An empty body is an empty WhoAmIRequest in protobuf.
    let call = |origin: Option<&str>| {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/proto".parse().unwrap());
        if let Some(origin) = origin {
            headers.insert(ORIGIN, origin.parse().unwrap());
        }
        headers
    };
    let page = browser
        .send(Method::POST, path, call(Some(PUBLIC_URL)))
        .await;
    assert_eq!(page.status, StatusCode::OK);
    for origin in [
        None,
        Some("https://evil.example"),
        Some("https://rangelight.test:8443"),
    ] {
        let page = browser.send(Method::POST, path, call(origin)).await;
        assert_eq!(page.status, StatusCode::FORBIDDEN, "{origin:?}");
        assert!(page.body.contains("permission_denied"), "{}", page.body);
    }
}

/// AC3 and AC4: a WebSocket upgrade needs a live login and the public origin.
#[tokio::test]
async fn websocket_upgrades_need_a_login_and_the_origin() {
    let (_db, served) = app("api::auth_websocket").await;
    let upgrade = |origin: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(UPGRADE, "websocket".parse().unwrap());
        headers.insert(CONNECTION, "Upgrade".parse().unwrap());
        headers.insert("sec-websocket-version", "13".parse().unwrap());
        headers.insert(
            "sec-websocket-key",
            "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap(),
        );
        headers.insert(ORIGIN, origin.parse().unwrap());
        headers
    };
    let mut anonymous = served.browser();
    let page = anonymous
        .send(Method::GET, "/terminal", upgrade(PUBLIC_URL))
        .await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);

    let mut browser = served.signed_in(KILGORE).await;
    let page = browser
        .send(Method::GET, "/terminal", upgrade("https://evil.example"))
        .await;
    assert_eq!(page.status, StatusCode::FORBIDDEN);
    // Past both checks there is no WebSocket endpoint yet (RAN-41).
    let page = browser
        .send(Method::GET, "/terminal", upgrade(PUBLIC_URL))
        .await;
    assert!(
        ![StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&page.status),
        "{}",
        page.status
    );
}

/// AC3: the callback refuses a state that isn't this browser's, a callback replayed after it
/// succeeded, and a code replayed into a new sign-in.
#[tokio::test]
async fn the_callback_refuses_bad_state_and_replays() {
    let (_db, served) = app("api::auth_callback").await;

    // Another browser's callback (login CSRF): this browser's state differs.
    let mut victim = served.browser();
    victim.start_sign_in(KILGORE).await;
    let mut attacker = served.browser();
    let attackers = attacker.start_sign_in(KILGORE).await;
    let page = victim.get(&attackers).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert!(!victim.cookies.contains_key("__Host-rl_session"));
    // No sign-in cookie at all.
    let page = served.browser().get(&attackers).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);

    // The right browser, once; the same callback again is refused.
    let mut browser = served.browser();
    let callback = browser.start_sign_in(KILGORE).await;
    let state = browser.cookies["__Host-rl_login"].clone();
    assert_eq!(browser.get(&callback).await.status, StatusCode::SEE_OTHER);
    browser.cookies.insert("__Host-rl_login".into(), state);
    assert_eq!(
        browser.get(&callback).await.status,
        StatusCode::UNAUTHORIZED
    );

    // The used code, with a fresh state of this browser's own.
    let mut replay = served.browser();
    let fresh = replay.start_sign_in(KILGORE).await;
    let code = callback
        .split_once("code=")
        .unwrap()
        .1
        .split('&')
        .next()
        .unwrap();
    let fresh_state = fresh
        .split_once("state=")
        .unwrap()
        .1
        .split('&')
        .next()
        .unwrap();
    let page = replay
        .get(&format!("/auth/callback?code={code}&state={fresh_state}"))
        .await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert!(!replay.cookies.contains_key("__Host-rl_session"));
}

/// AC3: an ID token whose nonce isn't the sign-in's is refused.
#[tokio::test]
async fn a_bad_nonce_is_refused() {
    let (_db, served) = app("api::auth_nonce").await;
    let mut browser = served.browser();
    let callback = browser.start_sign_in(KILGORE).await;
    // Re-seal the browser's sign-in with the client secret, swapping in a nonce Dex never saw.
    let provider = dex_provider();
    let state = query_param(&callback, "state");
    let sealed = &browser.cookies["__Host-rl_login"];
    let attempt = provider.open(sealed, &state, SystemTime::now()).unwrap();
    let swapped = Started {
        url: Url::parse(PUBLIC_URL).unwrap(),
        state: state.clone().into(),
        nonce: "not-the-nonce-dex-signed".to_owned().into(),
        pkce_verifier: attempt.pkce_verifier,
    };
    let resealed = provider.seal(&swapped, SystemTime::now()).unwrap();
    browser.cookies.insert(
        "__Host-rl_login".into(),
        resealed.expose_secret().to_owned(),
    );
    let page = browser.get(&callback).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert_eq!(page.body, "Sign-in failed. Start again at /auth/login.");
}

/// Security review M1: a sign-in cookie that was tampered with, or sealed under another key, is
/// refused with nothing more than "failed".
#[tokio::test]
async fn a_tampered_or_foreign_sign_in_cookie_is_refused() {
    let (_db, served) = app("api::auth_tampered").await;
    let mut browser = served.browser();
    let callback = browser.start_sign_in(KILGORE).await;
    let sealed = browser.cookies["__Host-rl_login"].clone();
    // Flip one character in the ciphertext.
    let mut chars: Vec<char> = sealed.chars().collect();
    let k = chars.len() / 2;
    chars[k] = if chars[k] == 'A' { 'B' } else { 'A' };
    browser
        .cookies
        .insert("__Host-rl_login".into(), chars.into_iter().collect());
    let page = browser.get(&callback).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert_eq!(page.body, "Sign-in failed. Start again at /auth/login.");

    // The same sign-in sealed under another client secret.
    let mut other = served.browser();
    let callback = other.start_sign_in(KILGORE).await;
    let mut secret = tempfile::NamedTempFile::new().unwrap();
    write!(secret, "another-client-secret-of-32-bytes-or-more").unwrap();
    let foreign = Provider::new(
        rl_testkit::dex().issuer,
        "rangelight-api".into(),
        SecretRef::file(secret.path()),
        rl_testkit::dex().redirect,
        "preferred_username".into(),
    )
    .unwrap();
    let started = Started {
        url: Url::parse(PUBLIC_URL).unwrap(),
        state: query_param(&callback, "state").into(),
        nonce: "n".to_owned().into(),
        pkce_verifier: "v".repeat(43).into(),
    };
    let forged = foreign.seal(&started, SystemTime::now()).unwrap();
    other
        .cookies
        .insert("__Host-rl_login".into(), forged.expose_secret().to_owned());
    let page = other.get(&callback).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert_eq!(page.body, "Sign-in failed. Start again at /auth/login.");
}

/// Security review M1: /auth/login writes nothing, so a flood of them locks no one out.
#[tokio::test]
async fn a_flood_of_sign_ins_writes_nothing() {
    let (db, served) = app("api::auth_flood").await;
    for _ in 0..200 {
        let page = served.browser().get("/auth/login").await;
        assert_eq!(page.status, StatusCode::SEE_OTHER);
    }
    db.try_execute(include_str!("nothing_written.sql"))
        .await
        .expect("no row written");
    let page = served.browser().sign_in(KILGORE).await;
    assert_eq!(
        page.status,
        StatusCode::SEE_OTHER,
        "a real sign-in still works"
    );
}

/// Security review L1: signing in again ends the login the browser had.
#[tokio::test]
async fn signing_in_again_ends_the_previous_login() {
    let (_db, served) = app("api::auth_again").await;
    let mut browser = served.signed_in(KILGORE).await;
    let old = browser.cookie_header();
    let page = browser.sign_in(KILGORE).await;
    assert_eq!(page.status, StatusCode::SEE_OTHER);
    assert_ne!(browser.cookie_header(), old);
    assert_eq!(who(&served, &browser).await, "kilgore");
    refused(&served, &old).await;
}

fn dex_provider() -> Provider {
    let dex = rl_testkit::dex();
    Provider::new(
        dex.issuer,
        "rangelight-api".into(),
        dex.secret,
        dex.redirect,
        "preferred_username".into(),
    )
    .unwrap()
}

fn query_param(path_and_query: &str, name: &str) -> String {
    let url = Url::parse(&format!("{PUBLIC_URL}{path_and_query}")).unwrap();
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap()
}

/// AC5: a member belongs to the account that first signed in as it; a username that isn't a
/// member name is refused.
#[tokio::test]
async fn a_member_belongs_to_its_first_account() {
    let (_db, served) = app("api::auth_members").await;
    served.signed_in(KILGORE).await;
    for email in ["impostor@example.com", "dotted@example.com"] {
        let page = served.browser().sign_in(email).await;
        assert_eq!(page.status, StatusCode::UNAUTHORIZED, "{email}");
    }
    let again = served.signed_in(KILGORE).await;
    assert_eq!(who(&served, &again).await, "kilgore");
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

/// AC7: the client secret and the login tokens are in no log line, at any level, through
/// sign-ins that succeed and fail.
#[tokio::test]
async fn secrets_are_never_logged() {
    let logs = Logs::default();
    let writer = logs.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .init();
    let (_db, served) = app("api::auth_logs").await;
    let mut browser = served.browser();
    let callback = browser.start_sign_in(KILGORE).await;
    let state = browser.cookies["__Host-rl_login"].clone();
    assert_eq!(browser.get(&callback).await.status, StatusCode::SEE_OTHER);
    let login = browser.cookies["__Host-rl_session"].clone();
    served.browser().sign_in("impostor@example.com").await;
    browser.post("/auth/logout").await;

    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("signed in"), "logging works: {logs}");
    let secret = rl_testkit::dex().secret.read().unwrap();
    let code = callback
        .split_once("code=")
        .unwrap()
        .1
        .split('&')
        .next()
        .unwrap();
    for secret in [secret.expose_secret(), &login, &state, code] {
        assert!(!logs.contains(secret), "a secret in the logs");
    }
}

/// Review M1: an open `WatchSession` ends with UNAUTHENTICATED once its login is gone (signed out,
/// or expired), within the re-check interval; a live login's stream stays open.
#[tokio::test]
async fn a_watch_ends_when_its_login_is_gone() {
    let db = Database::new("api::auth_watch_ends").await;
    let recheck = Duration::from_millis(500);
    let served =
        common::served_with(&db.store().await, &org(), "preferred_username", recheck).await;
    let watch = |browser: &Browser| {
        let client = served.sessions(&browser.cookie_header());
        async move {
            let request = WatchSessionRequest {
                session_id: uuid::Uuid::now_v7().to_string(),
                ..Default::default()
            };
            client.watch_session(request).await.unwrap()
        }
    };

    let live = served.signed_in(KILGORE).await;
    let mut kept = watch(&live).await;
    let mut signed_out = served.signed_in(KILGORE).await;
    let mut ended = watch(&signed_out).await;
    assert_eq!(
        signed_out.post("/auth/logout").await.status,
        StatusCode::NO_CONTENT
    );
    let error = tokio::time::timeout(Duration::from_secs(5), ended.message())
        .await
        .expect("the stream ended")
        .expect_err("with an error");
    assert_eq!(error.code, ErrorCode::Unauthenticated);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), kept.message())
            .await
            .is_err(),
        "a live login's stream stays open"
    );

    db.execute(include_str!("expire_logins.sql")).await;
    let error = tokio::time::timeout(Duration::from_secs(5), kept.message())
        .await
        .expect("the stream ended")
        .expect_err("with an error");
    assert_eq!(error.code, ErrorCode::Unauthenticated);
}

/// Review M2: the member comes from the configured claim. Dex's `name` is the account's username
/// here, so impostor (refused under `preferred_username`, which kilgore owns) signs in as itself;
/// a claim the token doesn't carry (`nickname`) is refused. (Binding a member to its first
/// subject is `a_member_belongs_to_its_first_account`.)
#[tokio::test]
async fn the_member_claim_setting_is_honoured() {
    let db = Database::new("api::auth_member_claim").await;
    let store = db.store().await;
    let by_name = common::served_with(&store, &org(), "name", common::RECHECK).await;
    let kilgore = by_name.signed_in(KILGORE).await;
    assert_eq!(who(&by_name, &kilgore).await, "kilgore");
    let impostor = by_name.signed_in("impostor@example.com").await;
    assert_eq!(who(&by_name, &impostor).await, "impostor");

    let by_nickname = common::served_with(&store, &org(), "nickname", common::RECHECK).await;
    let page = by_nickname.browser().sign_in(KILGORE).await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
}
