#![allow(clippy::unwrap_used)]

use rl_testkit::{FakeIssuer, NATS_AUDIENCE, now};
use serde_json::json;

use std::time::Instant;

use super::super::keys::REFETCH_AFTER;
use super::*;

fn issuer(fake: &FakeIssuer) -> Issuer {
    Issuer {
        url: fake.url.clone(),
        jwks_url: fake.jwks_url.clone(),
        jwks_token_file: None,
        jwks_ca_file: None,
        control_plane: true,
        runner: "r1".into(),
        runner_service_account: "rangelight-runner:runner".into(),
    }
}

fn claims(fake: &FakeIssuer, aud: &str, exp: u64) -> serde_json::Value {
    json!({
        "iss": fake.url,
        "aud": [aud],
        "sub": "system:serviceaccount:rl-ran-7:default",
        "exp": exp,
        "nbf": now(),
    })
}

#[tokio::test]
async fn a_token_of_a_known_issuer_names_its_service_account() {
    let fake = FakeIssuer::start().await;
    let tokens = Verifier::new(vec![issuer(&fake)]).unwrap();
    let verified = tokens
        .verify(&fake.token("rl-ran-7", "default"), now())
        .await
        .unwrap();
    assert_eq!(
        (verified.namespace.as_str(), verified.account.as_str()),
        ("rl-ran-7", "default")
    );
    assert!(verified.expires > now());
    assert_eq!(verified.issuer.runner, "r1");
}

#[tokio::test]
async fn audience_time_subject_issuer_and_signature_are_checked() {
    let fake = FakeIssuer::start().await;
    let verifier = Verifier::new(vec![issuer(&fake)]).unwrap();
    let verify = |token: String| {
        let verifier = &verifier;
        async move { verifier.verify(&token, now()).await }
    };
    let other_audience = fake.sign(&claims(&fake, "kubernetes", now() + 60));
    assert_eq!(verify(other_audience).await, Err(TokenError::Audience));
    let expired = fake.sign(&claims(&fake, NATS_AUDIENCE, now() - 1));
    assert_eq!(verify(expired).await, Err(TokenError::Time));
    let mut early = claims(&fake, NATS_AUDIENCE, now() + 600);
    early["nbf"] = (now() + 300).into();
    assert_eq!(verify(fake.sign(&early)).await, Err(TokenError::Time));
    let mut user = claims(&fake, NATS_AUDIENCE, now() + 60);
    user["sub"] = "alice".into();
    assert_eq!(verify(fake.sign(&user)).await, Err(TokenError::Subject));
    let mut elsewhere = claims(&fake, NATS_AUDIENCE, now() + 60);
    elsewhere["iss"] = "https://evil.example.com".into();
    assert_eq!(verify(fake.sign(&elsewhere)).await, Err(TokenError::Issuer));

    // Another identity's claims under this signature.
    let token = fake.token("rl-ran-7", "default");
    let (signed, signature) = token.rsplit_once('.').unwrap();
    let (header, _) = signed.split_once('.').unwrap();
    let forged = json!({"iss": fake.url, "aud": [NATS_AUDIENCE], "exp": now() + 60,
        "sub": "system:serviceaccount:rangelight-system:rangelight-api"});
    let forged = format!(
        "{header}.{}.{signature}",
        URL_SAFE_NO_PAD.encode(forged.to_string())
    );
    assert_eq!(verify(forged).await, Err(TokenError::Signature));
    // The same signature claimed for another algorithm than the key's.
    let rs256 = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","kid":"key-1"}"#);
    let (_, rest) = token.split_once('.').unwrap();
    assert_eq!(
        verify(format!("{rs256}.{rest}")).await,
        Err(TokenError::Key)
    );
    assert_eq!(verify("a.b".into()).await, Err(TokenError::Malformed));
}

#[tokio::test]
async fn an_unknown_kid_fetches_the_keys_again_at_most_every_ten_seconds() {
    let fake = FakeIssuer::start().await;
    let verifier = Verifier::new(vec![issuer(&fake)]).unwrap();
    verifier
        .verify(&fake.token("ns", "sa"), now())
        .await
        .unwrap();
    assert_eq!(fake.fetches(), 1, "the first token fetches the keys");
    verifier
        .verify(&fake.token("ns", "sa"), now())
        .await
        .unwrap();
    assert_eq!(fake.fetches(), 1, "a known kid is cached");

    // The issuer rotated its key, just after the last fetch: too soon to fetch again.
    fake.rotate();
    let rotated = fake.token("ns", "sa");
    assert_eq!(verifier.verify(&rotated, now()).await, Err(TokenError::Key));
    assert_eq!(fake.fetches(), 1);
    // Ten seconds on, the unknown kid fetches the keys again.
    let ten_seconds_ago = || Some(Instant::now().checked_sub(REFETCH_AFTER).unwrap());
    verifier.issuers[0].cache().fetched = ten_seconds_ago();
    verifier.verify(&rotated, now()).await.unwrap();
    assert_eq!(fake.fetches(), 2);

    // Callers of one unknown kid at once share one fetch.
    fake.rotate();
    verifier.issuers[0].cache().fetched = ten_seconds_ago();
    let rotated = fake.token("ns", "sa");
    let (a, b) = tokio::join!(
        verifier.verify(&rotated, now()),
        verifier.verify(&rotated, now())
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(fake.fetches(), 3, "one fetch for both");
}
