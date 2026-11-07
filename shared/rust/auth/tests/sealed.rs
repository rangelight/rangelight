//! The sealed sign-in cookie (security review M1 of RAN-37): only a cookie this client sealed,
//! unaltered, unexpired and for this state opens. No provider is reached.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::time::{Duration, SystemTime};

use rl_auth::{AuthError, Provider, SIGN_IN_TTL, Started, Url};
use rl_config::SecretRef;
use secrecy::ExposeSecret;

fn provider(secret: &str) -> (Provider, tempfile::NamedTempFile) {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "{secret}").unwrap();
    let url = Url::parse("https://rangelight.test/dex").unwrap();
    let provider = Provider::new(
        url.clone(),
        "rangelight-api".into(),
        SecretRef::file(file.path()),
        url.join("/auth/callback").unwrap(),
        "preferred_username".into(),
    )
    .unwrap();
    (provider, file)
}

fn started() -> Started {
    Started {
        url: Url::parse("https://rangelight.test/dex/auth").unwrap(),
        state: "the-state".to_owned().into(),
        nonce: "the-nonce".to_owned().into(),
        pkce_verifier: "v".repeat(43).into(),
    }
}

fn refused(result: Result<rl_auth::Attempt, AuthError>) -> &'static str {
    match result {
        Err(AuthError::Cookie(why)) => why,
        Err(other) => panic!("not a cookie refusal: {other}"),
        Ok(_) => panic!("opened"),
    }
}

#[test]
fn a_sealed_sign_in_opens_and_says_nothing_in_clear() {
    let (provider, _file) = provider("the-client-secret-of-at-least-32-bytes");
    let now = SystemTime::now();
    let sealed = provider.seal(&started(), now).unwrap();
    for clear in ["the-state", "the-nonce", "vvvv"] {
        assert!(!sealed.expose_secret().contains(clear), "{clear} in clear");
    }
    let attempt = provider
        .open(sealed.expose_secret(), "the-state", now)
        .unwrap();
    assert_eq!(attempt.nonce.expose_secret(), "the-nonce");
    assert_eq!(attempt.pkce_verifier.expose_secret(), &"v".repeat(43));
    // A fresh random nonce each time: two seals of the same sign-in differ.
    let again = provider.seal(&started(), now).unwrap();
    assert_ne!(sealed.expose_secret(), again.expose_secret());
}

#[test]
fn a_tampered_cookie_is_refused() {
    let (provider, _file) = provider("the-client-secret-of-at-least-32-bytes");
    let now = SystemTime::now();
    let sealed = provider.seal(&started(), now).unwrap();
    let sealed = sealed.expose_secret();
    for at in [0, sealed.len() / 2, sealed.len() - 2] {
        let mut chars: Vec<char> = sealed.chars().collect();
        chars[at] = if chars[at] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        refused(provider.open(&tampered, "the-state", now));
    }
    for junk in ["", "not base64!", "AAAA", &"A".repeat(2000)] {
        refused(provider.open(junk, "the-state", now));
    }
}

#[test]
fn a_cookie_sealed_under_another_client_secret_is_refused() {
    let (ours, _a) = provider("the-client-secret-of-at-least-32-bytes");
    let (theirs, _b) = provider("another-client-secret-of-32-bytes-or-more");
    let now = SystemTime::now();
    let sealed = theirs.seal(&started(), now).unwrap();
    assert_eq!(
        refused(ours.open(sealed.expose_secret(), "the-state", now)),
        "does not open"
    );
}

#[test]
fn an_expired_or_future_cookie_is_refused() {
    let (provider, _file) = provider("the-client-secret-of-at-least-32-bytes");
    let issued = SystemTime::now();
    let sealed = provider.seal(&started(), issued).unwrap();
    let sealed = sealed.expose_secret();
    provider
        .open(
            sealed,
            "the-state",
            issued + SIGN_IN_TTL - Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(
        refused(provider.open(sealed, "the-state", issued + SIGN_IN_TTL)),
        "expired"
    );
    assert_eq!(
        refused(provider.open(sealed, "the-state", issued - Duration::from_secs(120))),
        "expired"
    );
}

#[test]
fn a_cookie_for_another_state_is_refused() {
    let (provider, _file) = provider("the-client-secret-of-at-least-32-bytes");
    let now = SystemTime::now();
    let sealed = provider.seal(&started(), now).unwrap();
    for state in ["another-state", "the-stat", "the-state ", ""] {
        assert_eq!(
            refused(provider.open(sealed.expose_secret(), state, now)),
            "another state"
        );
    }
}

/// Security review round 2, item 5: the client secret keys the cookie, so it must hold at least
/// 32 bytes. The api checks at startup (`check_secret`), and every seal or open checks again.
#[test]
fn a_client_secret_under_32_bytes_is_refused() {
    let (short, _file) = provider(&"s".repeat(31));
    let weak = short.check_secret().unwrap_err();
    assert!(matches!(weak, AuthError::WeakSecret(_)), "{weak}");
    assert!(
        weak.to_string().contains("openssl rand -base64 48"),
        "{weak}"
    );
    assert!(matches!(
        short.seal(&started(), SystemTime::now()),
        Err(AuthError::WeakSecret(_))
    ));
    let (long, _file) = provider(&"s".repeat(32));
    long.check_secret().unwrap();
}
