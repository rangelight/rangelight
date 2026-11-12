//! A token issuer's signing keys: its JWKS over HTTPS (a plain HTTP client, not a Kubernetes
//! client), with a bearer token and a CA of its own when the issuer is this cluster's API server.
//! The keys are cached, and fetched again for an unknown `kid` at most every 10 seconds, so a
//! stream of made-up `kid`s can't flood the issuer. A fetch holds no lock a known `kid` waits on:
//! only the callers that need its answer wait, for that one fetch.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{self, RsaPublicKeyComponents, UnparsedPublicKey};
use serde::Deserialize;

use super::token::TokenError;
use crate::config::Issuer;

/// The least time between two fetches of one issuer's keys.
pub(crate) const REFETCH_AFTER: Duration = Duration::from_secs(10);
/// The bound on one fetch of an issuer's keys.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest JWKS accepted.
const JWKS_MAX: usize = 64 * 1024;

/// One issuer and its cached keys.
#[derive(Debug)]
pub(crate) struct IssuerKeys {
    pub(crate) issuer: Issuer,
    http: reqwest::Client,
    cache: Mutex<Cache>,
    /// Held for the one fetch in flight.
    fetching: tokio::sync::Mutex<()>,
}

/// The keys by `kid`, and when they were last fetched.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    keys: HashMap<String, Key>,
    pub(crate) fetched: Option<Instant>,
}

/// A key a signature verifies with.
#[derive(Debug, Clone)]
pub(crate) enum Key {
    /// RS256: the modulus and exponent.
    Rsa(Vec<u8>, Vec<u8>),
    /// ES256: the uncompressed P-256 point.
    P256(Vec<u8>),
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kty: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    crv: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

impl IssuerKeys {
    /// The issuer's keys, none fetched yet; reads its CA file now.
    pub(crate) fn new(issuer: Issuer) -> Result<IssuerKeys, TokenError> {
        // reqwest's TLS needs a process-wide rustls provider; an Err means one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let jwks = |e: &dyn std::fmt::Display| TokenError::Jwks(e.to_string());
        let mut http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none());
        if let Some(ca) = &issuer.jwks_ca_file {
            let pem = std::fs::read(ca).map_err(|e| jwks(&e))?;
            let ca = reqwest::Certificate::from_pem(&pem).map_err(|e| jwks(&e))?;
            http = http.add_root_certificate(ca);
        }
        Ok(IssuerKeys {
            http: http.build().map_err(|e| jwks(&e))?,
            issuer,
            cache: Mutex::default(),
            fetching: tokio::sync::Mutex::default(),
        })
    }

    /// The key `kid`, fetching the issuer's keys when it's unknown and the last fetch is old enough.
    pub(crate) async fn key(&self, kid: &str) -> Result<Key, TokenError> {
        if let Some(key) = self.cache().keys.get(kid) {
            return Ok(key.clone());
        }
        // Only callers of an unknown kid wait here, for the fetch in flight; that one may have
        // brought it, or been the one fetch allowed for now.
        let _fetching = self.fetching.lock().await;
        if let Some(key) = self.cached(kid)? {
            return Ok(key);
        }
        self.cache().fetched = Some(Instant::now());
        let keys = self.fetch().await?;
        let mut cache = self.cache();
        cache.keys = keys;
        cache.keys.get(kid).cloned().ok_or(TokenError::Key)
    }

    /// `kid` from the cache; `None` when it's unknown and a fetch is allowed.
    fn cached(&self, kid: &str) -> Result<Option<Key>, TokenError> {
        let cache = self.cache();
        if let Some(key) = cache.keys.get(kid) {
            return Ok(Some(key.clone()));
        }
        if cache.fetched.is_some_and(|at| at.elapsed() < REFETCH_AFTER) {
            return Err(TokenError::Key);
        }
        Ok(None)
    }

    /// The cache, never held across an `await`.
    pub(crate) fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn fetch(&self) -> Result<HashMap<String, Key>, TokenError> {
        let jwks = |e: &dyn std::fmt::Display| TokenError::Jwks(e.to_string());
        let mut request = self.http.get(&self.issuer.jwks_url);
        if let Some(file) = &self.issuer.jwks_token_file {
            let token = tokio::fs::read_to_string(file)
                .await
                .map_err(|e| jwks(&e))?;
            request = request.bearer_auth(token.trim());
        }
        let response = request.send().await.map_err(|e| jwks(&e))?;
        if !response.status().is_success() {
            return Err(TokenError::Jwks(format!("HTTP {}", response.status())));
        }
        let body = response.bytes().await.map_err(|e| jwks(&e))?;
        if body.len() > JWKS_MAX {
            return Err(TokenError::Jwks("JWKS too large".into()));
        }
        let set: Jwks = serde_json::from_slice(&body).map_err(|e| jwks(&e))?;
        Ok(set.keys.into_iter().filter_map(parse_jwk).collect())
    }
}

/// A JWK we can verify with, by its `kid`: RSA, or EC on P-256. Anything else is skipped.
fn parse_jwk(jwk: Jwk) -> Option<(String, Key)> {
    let b64 = |v: Option<String>| URL_SAFE_NO_PAD.decode(v?).ok();
    let key = match (jwk.kty.as_str(), jwk.crv.as_deref()) {
        ("RSA", _) => Key::Rsa(b64(jwk.n)?, b64(jwk.e)?),
        ("EC", Some("P-256")) => {
            let (x, y) = (b64(jwk.x)?, b64(jwk.y)?);
            if x.len() != 32 || y.len() != 32 {
                return None;
            }
            Key::P256([&[4][..], &x, &y].concat())
        }
        _ => return None,
    };
    Some((jwk.kid?, key))
}

/// Verifies `sig` over `input` with `key`, if `alg` is that key's algorithm.
pub(crate) fn verify_signature(
    key: &Key,
    alg: &str,
    input: &[u8],
    sig: &[u8],
) -> Result<(), TokenError> {
    let verified = match (key, alg) {
        (Key::Rsa(n, e), "RS256") => RsaPublicKeyComponents { n, e }
            .verify(&signature::RSA_PKCS1_2048_8192_SHA256, input, sig)
            .is_ok(),
        (Key::P256(point), "ES256") => {
            UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point)
                .verify(input, sig)
                .is_ok()
        }
        _ => return Err(TokenError::Key),
    };
    if verified {
        Ok(())
    } else {
        Err(TokenError::Signature)
    }
}
