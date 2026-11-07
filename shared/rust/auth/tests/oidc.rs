//! The OIDC flow against the real (test) Dex: what `rl-auth` itself refuses.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;

use rl_auth::{AuthError, Identity, Provider, Started};
use rl_config::SecretRef;
use rl_testkit::{Dex, dex, sign_in_at_dex};
use secrecy::{ExposeSecret, SecretString};

fn provider(dex: &Dex, client: &str, secret: SecretRef) -> Provider {
    with_claim(dex, client, secret, "preferred_username")
}

fn with_claim(dex: &Dex, client: &str, secret: SecretRef, member_claim: &str) -> Provider {
    Provider::new(
        dex.issuer.clone(),
        client.into(),
        secret,
        dex.redirect.clone(),
        member_claim.into(),
    )
    .unwrap()
}

/// Starts a sign-in with `provider` and walks Dex as kilgore: the code it comes back with.
async fn code(provider: &Provider, dex: &Dex) -> (Started, String) {
    let started = provider.start().await.unwrap();
    let back = sign_in_at_dex(&started.url, "kilgore@example.com", &dex.redirect).await;
    let param = |name: &str| {
        back.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap()
    };
    assert_eq!(param("state"), started.state.expose_secret());
    (started, param("code"))
}

#[tokio::test]
async fn the_authorization_url_asks_for_s256_pkce_state_and_nonce() {
    let dex = dex();
    let started = provider(&dex, "rangelight-api", dex.secret.clone())
        .start()
        .await
        .unwrap();
    let pairs: Vec<(String, String)> = started.url.query_pairs().into_owned().collect();
    let get = |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("response_type"), Some("code"));
    assert_eq!(get("code_challenge_method"), Some("S256"));
    assert_eq!(get("state"), Some(started.state.expose_secret()));
    assert_eq!(get("nonce"), Some(started.nonce.expose_secret()));
    assert_eq!(get("scope"), Some("openid profile"));
    assert!(
        started.state.expose_secret().len() >= 22,
        "128 bits or more"
    );
}

#[tokio::test]
async fn a_verified_token_names_the_account() {
    let dex = dex();
    let api = provider(&dex, "rangelight-api", dex.secret.clone());
    let (started, code) = code(&api, &dex).await;
    let token = api.exchange(&code, &started.pkce_verifier).await.unwrap();
    let identity = api.verify(&token, &started.nonce).await.unwrap();
    assert_eq!(
        identity,
        Identity {
            issuer: dex.issuer.to_string(),
            subject: identity.subject.clone(),
            member: Some("Kilgore".into()),
        }
    );
    assert_ne!(identity.subject, "");
}

/// Review M2: the member is read from the configured claim, and is absent when the token has none.
#[tokio::test]
async fn the_member_comes_from_the_configured_claim() {
    let dex = dex();
    for (claim, member) in [
        ("preferred_username", Some("Kilgore")),
        ("name", Some("kilgore")),
        ("nickname", None),
    ] {
        let api = with_claim(&dex, "rangelight-api", dex.secret.clone(), claim);
        let (started, code) = code(&api, &dex).await;
        let token = api.exchange(&code, &started.pkce_verifier).await.unwrap();
        let identity = api.verify(&token, &started.nonce).await.unwrap();
        assert_eq!(identity.member.as_deref(), member, "{claim}");
    }
}

/// AC3: an ID token issued to another client is refused by the api's audience check.
#[tokio::test]
async fn a_token_for_another_audience_is_refused() {
    let dex = dex();
    let other = provider(&dex, "rangelight-other", dex.secret.clone());
    let (started, code) = code(&other, &dex).await;
    let token = other.exchange(&code, &started.pkce_verifier).await.unwrap();
    other.verify(&token, &started.nonce).await.unwrap();

    let api = provider(&dex, "rangelight-api", dex.secret.clone());
    let refused = api.verify(&token, &started.nonce).await.unwrap_err();
    assert!(
        matches!(&refused, AuthError::Token(why) if why.contains("audience")),
        "{refused}"
    );
}

/// AC3: a token whose nonce isn't the sign-in's is refused.
#[tokio::test]
async fn a_token_with_another_nonce_is_refused() {
    let dex = dex();
    let api = provider(&dex, "rangelight-api", dex.secret.clone());
    let (started, code) = code(&api, &dex).await;
    let token = api.exchange(&code, &started.pkce_verifier).await.unwrap();
    let refused = api
        // Another sign-in's nonce: the real one, changed (a fixed literal would be a hard-coded nonce).
        .verify(&token, &SecretString::from(format!("{}x", started.nonce.expose_secret())))
        .await
        .unwrap_err();
    assert!(
        matches!(&refused, AuthError::Token(why) if why.contains("once")),
        "{refused}"
    );
}

/// AC3: a code works once, and only with its own PKCE verifier.
#[tokio::test]
async fn a_code_is_refused_when_replayed_or_without_its_verifier() {
    let dex = dex();
    let api = provider(&dex, "rangelight-api", dex.secret.clone());
    let (_, code) = code(&api, &dex).await;
    let wrong = SecretString::from("x".repeat(64));
    assert!(matches!(
        api.exchange(&code, &wrong).await,
        Err(AuthError::Code(_))
    ));

    let (started_again, code_again) = self::code(&api, &dex).await;
    api.exchange(&code_again, &started_again.pkce_verifier)
        .await
        .unwrap();
    assert!(matches!(
        api.exchange(&code_again, &started_again.pkce_verifier)
            .await,
        Err(AuthError::Code(_))
    ));
}

/// AC7: the client secret is in no error and no `Debug`, also when the provider refuses it.
#[tokio::test]
async fn the_client_secret_is_in_no_error() {
    let dex = dex();
    let secret = dex.secret.read().unwrap();
    let mut wrong_file = tempfile::NamedTempFile::new().unwrap();
    write!(wrong_file, "{}-wrong", secret.expose_secret()).unwrap();
    let wrong = provider(&dex, "rangelight-api", SecretRef::file(wrong_file.path()));
    let (started, code) = code(&wrong, &dex).await;
    let refused = wrong
        .exchange(&code, &started.pkce_verifier)
        .await
        .unwrap_err();
    for text in [
        refused.to_string(),
        format!("{refused:?}"),
        format!("{wrong:?}"),
        format!("{:?}", started.pkce_verifier),
    ] {
        assert!(!text.contains(secret.expose_secret()), "{text}");
    }
    assert!(
        !format!("{:?}", started.pkce_verifier).contains(started.pkce_verifier.expose_secret())
    );
}
