//! NATS JWTs: the server's authorization request, which the callout reads, and the user and
//! response JWTs it signs with the callout issuer's nkey (`ed25519-nkey`, as nats-server checks
//! them).

use std::ops::Range;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nkeys::KeyPair;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::permissions::Grant;

/// What the server asks the callout about one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Request {
    /// The connection's user nkey, which the server made for it: the user JWT's subject.
    pub(crate) user_nkey: String,
    /// The server's id: the response's audience.
    pub(crate) server_id: String,
    /// The token the client presented, if any.
    pub(crate) token: Option<String>,
}

/// Bytes that aren't a NATS JWT, or a key that can't sign.
#[derive(Debug, thiserror::Error)]
#[error("malformed NATS JWT")]
pub(crate) struct Malformed;

#[derive(Deserialize)]
struct RequestClaims {
    nats: RequestBody,
}

#[derive(Deserialize)]
struct RequestBody {
    user_nkey: String,
    server_id: ServerId,
    #[serde(default)]
    connect_opts: ConnectOpts,
}

#[derive(Deserialize)]
struct ServerId {
    id: String,
}

#[derive(Default, Deserialize)]
struct ConnectOpts {
    #[serde(default)]
    auth_token: Option<String>,
}

/// Reads the server's authorization request. Its signature isn't what makes it trusted: only the
/// server can publish on `$SYS.REQ.USER.AUTH` (no identity's permissions allow it), and the token
/// it carries is verified on its own.
pub(crate) fn request(jwt: &[u8]) -> Result<Request, Malformed> {
    let jwt = std::str::from_utf8(jwt).map_err(|_| Malformed)?;
    let mut parts = jwt.split('.');
    let (Some(_), Some(claims), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Malformed);
    };
    let claims = URL_SAFE_NO_PAD.decode(claims).map_err(|_| Malformed)?;
    let claims: RequestClaims = serde_json::from_slice(&claims).map_err(|_| Malformed)?;
    Ok(Request {
        user_nkey: claims.nats.user_nkey,
        server_id: claims.nats.server_id.id,
        token: claims
            .nats
            .connect_opts
            .auth_token
            .filter(|t| !t.is_empty()),
    })
}

/// The user JWT for `request`'s connection: `grant`'s permissions, in `account`, valid for
/// `lifetime` (seconds since the epoch: from issue to expiry). In operator mode (`account` is an account's public
/// key, and `issuer` one of its signing keys) it names the account it's for. No limits of its
/// own (`-1`): the server applies the account's to a callout user.
pub(crate) fn user(
    issuer: &KeyPair,
    account: &str,
    request: &Request,
    grant: &Grant,
    lifetime: Range<u64>,
) -> Result<String, Malformed> {
    let mut claims = json!({
        "iat": lifetime.start,
        "exp": lifetime.end,
        "iss": issuer.public_key(),
        "sub": request.user_nkey,
        "aud": account,
        "name": grant.name,
        "nats": {
            "pub": {"allow": grant.permissions.publish},
            "sub": {"allow": grant.permissions.subscribe},
            "subs": -1,
            "payload": -1,
            "data": -1,
            "type": "user",
            "version": 2,
        },
    });
    if account != issuer.public_key() && KeyPair::from_public_key(account).is_ok() {
        claims["nats"]["issuer_account"] = account.into();
    }
    sign(issuer, claims)
}

/// The response to `request`: the user JWT, or why the connection is refused.
pub(crate) fn response(
    issuer: &KeyPair,
    request: &Request,
    answer: Result<&str, &str>,
    now: u64,
) -> Result<String, Malformed> {
    let mut nats = json!({"type": "authorization_response", "version": 2});
    match answer {
        Ok(user) => nats["jwt"] = user.into(),
        Err(reason) => nats["error"] = reason.into(),
    }
    let claims = json!({
        "iat": now,
        "iss": issuer.public_key(),
        "sub": request.user_nkey,
        "aud": request.server_id,
        "nats": nats,
    });
    sign(issuer, claims)
}

/// `claims` as a signed NATS JWT, with `jti` the SHA-256 of the claims without it.
fn sign(issuer: &KeyPair, mut claims: Value) -> Result<String, Malformed> {
    let digest = Sha256::digest(claims.to_string().as_bytes());
    claims["jti"] = URL_SAFE_NO_PAD.encode(digest).into();
    let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ed25519-nkey"}"#);
    let body = URL_SAFE_NO_PAD.encode(claims.to_string());
    let input = format!("{header}.{body}");
    let signature = issuer.sign(input.as_bytes()).map_err(|_| Malformed)?;
    Ok(format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
