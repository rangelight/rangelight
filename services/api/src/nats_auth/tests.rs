#![allow(clippy::unwrap_used)]

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rl_domain::SessionId;
use serde_json::{Value, json};

use super::jwt::{self, Request};
use super::permissions::{Account, grant};
use super::*;

fn claims(jwt: &str) -> Value {
    let part = jwt.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

fn request() -> Request {
    Request {
        user_nkey: KeyPair::new_user().public_key(),
        server_id: "NSERVER".into(),
        token: Some("t".into()),
    }
}

#[test]
fn the_servers_request_gives_its_user_server_and_token() {
    let body = json!({"nats": {"user_nkey": "UABC", "server_id": {"id": "NXYZ", "name": "n"},
        "connect_opts": {"auth_token": "the-token", "lang": "rust"}, "type": "authorization_request"}});
    let jwt = format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(body.to_string()));
    let request = jwt::request(jwt.as_bytes()).unwrap();
    assert_eq!(
        (request.user_nkey.as_str(), request.server_id.as_str()),
        ("UABC", "NXYZ")
    );
    assert_eq!(request.token.as_deref(), Some("the-token"));
    let anonymous =
        json!({"nats": {"user_nkey": "U", "server_id": {"id": "N"}, "connect_opts": {}}});
    let jwt = format!("e30.{}.c2ln", URL_SAFE_NO_PAD.encode(anonymous.to_string()));
    assert_eq!(jwt::request(jwt.as_bytes()).unwrap().token, None);
    assert!(jwt::request(b"not a jwt").is_err());
}

#[test]
fn a_user_jwt_is_signed_by_the_issuer_with_the_grant_and_expiry() {
    let issuer = KeyPair::new_account();
    let request = request();
    let lane = grant("acme", &Identity::Lane(SessionId(uuid::Uuid::now_v7())));
    let user = jwt::user(&issuer, "LANE", &request, &lane, 100..200).unwrap();
    let (input, signature) = user.rsplit_once('.').unwrap();
    issuer
        .verify(
            input.as_bytes(),
            &URL_SAFE_NO_PAD.decode(signature).unwrap(),
        )
        .unwrap();
    let signed = claims(&user);
    assert_eq!(signed["iss"], issuer.public_key());
    assert_eq!(signed["sub"], request.user_nkey);
    assert_eq!(
        (signed["aud"].as_str(), signed["exp"].as_u64()),
        (Some("LANE"), Some(200))
    );
    assert_eq!(
        signed["nats"]["pub"]["allow"],
        json!(lane.permissions.publish)
    );
    assert_eq!(
        signed["nats"]["sub"]["allow"],
        json!(lane.permissions.subscribe)
    );
    let limits = (
        signed["nats"]["subs"].as_i64(),
        signed["nats"]["payload"].as_i64(),
    );
    assert_eq!(limits, (Some(-1), Some(-1)), "the account's limits apply");
    assert!(
        signed["nats"].get("issuer_account").is_none(),
        "config mode"
    );

    // Operator mode: the account is a public key, and the issuer one of its signing keys.
    let account = KeyPair::new_account().public_key();
    let user = jwt::user(&issuer, &account, &request, &lane, 100..200).unwrap();
    assert_eq!(claims(&user)["nats"]["issuer_account"], account);
}

#[test]
fn a_response_carries_the_user_or_the_reason_for_this_server_and_user() {
    let issuer = KeyPair::new_account();
    let request = request();
    let ok = claims(&jwt::response(&issuer, &request, Ok("the.user.jwt"), 100).unwrap());
    assert_eq!(
        (ok["aud"].as_str(), ok["sub"].as_str()),
        (Some("NSERVER"), Some(request.user_nkey.as_str()))
    );
    assert_eq!(ok["nats"]["jwt"], "the.user.jwt");
    let refused = claims(&jwt::response(&issuer, &request, Err("no_token"), 100).unwrap());
    assert_eq!(refused["nats"]["error"], "no_token");
    assert!(refused["nats"].get("jwt").is_none());
}

#[test]
fn only_a_lane_joins_the_lane_account() {
    for identity in [
        Identity::Api,
        Identity::Relay,
        Identity::Runner("r1".into()),
    ] {
        assert_eq!(
            grant("acme", &identity).account,
            Account::Main,
            "{identity:?}"
        );
    }
    let lane = grant("acme", &Identity::Lane(SessionId(uuid::Uuid::now_v7())));
    assert_eq!(lane.account, Account::Lane);
    assert_eq!(
        grant("acme", &Identity::Runner("r1".into())).name,
        "runner-r1"
    );
}
