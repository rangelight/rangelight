//! Verifies a projected ServiceAccount token (ADR 0018 amendment): its signature against its
//! issuer's keys (`keys`), its audience and its time.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::keys::{IssuerKeys, verify_signature};
use crate::config::Issuer;

/// The audience of every token a NATS client presents.
pub const AUDIENCE: &str = "rangelight-nats";
/// How far a token's `nbf` may be ahead of this clock.
const SKEW: u64 = 30;

/// Why a token was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Not a JWT, or not one of the expected shape.
    #[error("malformed")]
    Malformed,
    /// Its `iss` isn't a configured issuer.
    #[error("unknown issuer")]
    Issuer,
    /// Its `kid` isn't one of the issuer's keys, or its `alg` not that key's.
    #[error("unknown signing key")]
    Key,
    /// The signature doesn't verify.
    #[error("bad signature")]
    Signature,
    /// Its audience isn't [`AUDIENCE`].
    #[error("wrong audience")]
    Audience,
    /// Expired, or not valid yet.
    #[error("outside its validity")]
    Time,
    /// Its subject isn't a ServiceAccount.
    #[error("not a ServiceAccount")]
    Subject,
    /// The issuer's keys couldn't be fetched.
    #[error("can't fetch the issuer's keys: {0}")]
    Jwks(String),
}

impl TokenError {
    /// The machine-readable reason a sign-in gets.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            TokenError::Malformed => "malformed_token",
            TokenError::Issuer => "unknown_issuer",
            TokenError::Key => "unknown_key",
            TokenError::Signature => "bad_signature",
            TokenError::Audience => "wrong_audience",
            TokenError::Time => "expired",
            TokenError::Subject => "not_a_service_account",
            TokenError::Jwks(_) => "issuer_unavailable",
        }
    }
}

/// A verified token: which issuer, which ServiceAccount, and until when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The issuer that signed it.
    pub issuer: Issuer,
    /// The ServiceAccount's namespace.
    pub namespace: String,
    /// The ServiceAccount's name.
    pub account: String,
    /// The pod the token is bound to, if any (`kubernetes.io.pod.name`).
    pub pod: Option<String>,
    /// When it expires, in seconds since the epoch.
    pub expires: u64,
}

/// Every configured issuer and its cached keys.
#[derive(Debug)]
pub struct Verifier {
    pub(crate) issuers: Vec<IssuerKeys>,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    sub: String,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(default, rename = "kubernetes.io")]
    kubernetes: Option<Bound>,
}

/// What a projected token is bound to.
#[derive(Deserialize)]
struct Bound {
    #[serde(default)]
    pod: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Verifier {
    /// A verifier for `issuers`; reads each one's CA file now.
    ///
    /// # Errors
    /// [`TokenError::Jwks`] when a CA file can't be read or an HTTP client can't be built.
    pub fn new(issuers: Vec<Issuer>) -> Result<Verifier, TokenError> {
        let issuers = issuers
            .into_iter()
            .map(IssuerKeys::new)
            .collect::<Result<_, _>>()?;
        Ok(Verifier { issuers })
    }

    /// Verifies `token` at `now` (seconds since the epoch).
    ///
    /// # Errors
    /// The [`TokenError`] that refused it.
    pub async fn verify(&self, token: &str, now: u64) -> Result<Verified, TokenError> {
        let mut parts = token.split('.');
        let (Some(header), Some(claims), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };
        let header: Header = decode_json(header)?;
        let claims_json: Claims = decode_json(claims)?;
        // Nothing in the claims is trusted before the signature: `iss` only picks the keys.
        let keys = self
            .issuers
            .iter()
            .find(|keys| keys.issuer.url == claims_json.iss)
            .ok_or(TokenError::Issuer)?;
        let key = keys.key(&header.kid).await?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| TokenError::Malformed)?;
        let input = &token[..header_and_claims_len(token)];
        verify_signature(&key, &header.alg, input.as_bytes(), &signature)?;
        check(keys.issuer.clone(), &claims_json, now)
    }
}

/// The audience, the time and the subject of signed claims.
fn check(issuer: Issuer, claims: &Claims, now: u64) -> Result<Verified, TokenError> {
    let audience_ok = match &claims.aud {
        Audience::One(aud) => aud == AUDIENCE,
        Audience::Many(auds) => auds.iter().any(|aud| aud == AUDIENCE),
    };
    if !audience_ok {
        return Err(TokenError::Audience);
    }
    if claims.exp <= now || claims.nbf.is_some_and(|nbf| nbf > now + SKEW) {
        return Err(TokenError::Time);
    }
    let (namespace, account) = claims
        .sub
        .strip_prefix("system:serviceaccount:")
        .and_then(|rest| rest.split_once(':'))
        .filter(|(ns, name)| !ns.is_empty() && !name.is_empty() && !name.contains(':'))
        .ok_or(TokenError::Subject)?;
    Ok(Verified {
        issuer,
        namespace: namespace.to_owned(),
        account: account.to_owned(),
        pod: claims
            .kubernetes
            .as_ref()
            .and_then(|bound| bound.pod.as_ref())
            .map(|pod| pod.name.clone()),
        expires: claims.exp,
    })
}

fn decode_json<T: serde::de::DeserializeOwned>(part: &str) -> Result<T, TokenError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| TokenError::Malformed)?;
    serde_json::from_slice(&bytes).map_err(|_| TokenError::Malformed)
}

/// The length of `header.claims`, the signed part of a three-part token.
fn header_and_claims_len(token: &str) -> usize {
    token.rfind('.').unwrap_or(0)
}

#[cfg(test)]
mod tests;
