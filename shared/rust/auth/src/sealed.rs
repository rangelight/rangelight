//! The sign-in cookie: `state`, `nonce` and the PKCE verifier between /auth/login and the
//! callback, sealed so nothing is stored server side (review M1 of RAN-37). AES-256-GCM with a
//! random 96-bit nonce per seal; the key is HKDF-SHA256 of the client secret with the info
//! `rangelight/login/v1`, so rotating the client secret voids every sign-in in flight.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, Salt};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;

use crate::{AuthError, Provider, Started};

/// A sign-in must come back from the provider within this.
pub const SIGN_IN_TTL: Duration = Duration::from_secs(600);
/// The HKDF info, and the AEAD's associated data: a key or a cookie for anything else won't open.
const PURPOSE: &[u8] = b"rangelight/login/v1";
/// Clocks may disagree a little between api replicas.
const SKEW: Duration = Duration::from_secs(60);
/// The shortest client secret that may key the cookie.
const MIN_SECRET: usize = 32;
/// A sealed sign-in is a few hundred bytes; anything longer isn't one.
const MAX_COOKIE: usize = 1024;
const TAG_LEN: usize = 16;

/// What the callback needs from its sign-in, out of the cookie. Its `Debug` shows the secrets
/// redacted.
#[derive(Debug)]
pub struct Attempt {
    /// The nonce the ID token must carry.
    pub nonce: SecretString,
    /// The PKCE verifier the code exchange sends.
    pub pkce_verifier: SecretString,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Sealed {
    state: String,
    nonce: String,
    pkce_verifier: String,
    /// Seconds since the Unix epoch.
    issued_at: u64,
}

impl Provider {
    /// Seals `started` (state, nonce, PKCE verifier) and `now` into a cookie value.
    ///
    /// # Errors
    /// [`AuthError::Secret`] if the client secret can't be read, [`AuthError::Cookie`] if the
    /// system's randomness fails.
    pub fn seal(&self, started: &Started, now: SystemTime) -> Result<SecretString, AuthError> {
        let sealed = Sealed {
            state: started.state.expose_secret().to_owned(),
            nonce: started.nonce.expose_secret().to_owned(),
            pkce_verifier: started.pkce_verifier.expose_secret().to_owned(),
            issued_at: seconds(now),
        };
        let mut bytes =
            serde_json::to_vec(&sealed).map_err(|_| AuthError::Cookie("unserialisable"))?;
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| AuthError::Cookie("no randomness"))?;
        self.key()?
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(PURPOSE),
                &mut bytes,
            )
            .map_err(|_| AuthError::Cookie("sealing failed"))?;
        Ok(URL_SAFE_NO_PAD.encode([&nonce[..], &bytes].concat()).into())
    }

    /// Opens a sign-in cookie: it must be sealed with this client secret, issued less than
    /// [`SIGN_IN_TTL`] before `now`, and carry `state` (compared in constant time).
    ///
    /// # Errors
    /// [`AuthError::Cookie`] for a cookie that is malformed, tampered with, sealed with another
    /// key, expired, or for another state; [`AuthError::Secret`] if the secret can't be read.
    pub fn open(&self, cookie: &str, state: &str, now: SystemTime) -> Result<Attempt, AuthError> {
        if cookie.len() > MAX_COOKIE {
            return Err(AuthError::Cookie("too long"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(cookie)
            .map_err(|_| AuthError::Cookie("not base64url"))?;
        if bytes.len() < NONCE_LEN + TAG_LEN {
            return Err(AuthError::Cookie("too short"));
        }
        let (nonce, sealed) = bytes.split_at(NONCE_LEN);
        let nonce =
            Nonce::try_assume_unique_for_key(nonce).map_err(|_| AuthError::Cookie("too short"))?;
        let mut sealed = sealed.to_vec();
        let plain = self
            .key()?
            .open_in_place(nonce, Aad::from(PURPOSE), &mut sealed)
            .map_err(|_| AuthError::Cookie("does not open"))?;
        let sealed: Sealed =
            serde_json::from_slice(plain).map_err(|_| AuthError::Cookie("not a sign-in"))?;
        let now = seconds(now);
        let fresh = sealed.issued_at <= now.saturating_add(SKEW.as_secs())
            && now.saturating_sub(sealed.issued_at) < SIGN_IN_TTL.as_secs();
        if !fresh {
            return Err(AuthError::Cookie("expired"));
        }
        if !bool::from(sealed.state.as_bytes().ct_eq(state.as_bytes())) {
            return Err(AuthError::Cookie("another state"));
        }
        Ok(Attempt {
            nonce: sealed.nonce.into(),
            pkce_verifier: sealed.pkce_verifier.into(),
        })
    }

    /// Reads the client secret and checks it can key the sign-in cookie: at least 32 bytes. The
    /// api calls it at startup, so a weak secret stops it there.
    ///
    /// # Errors
    /// [`AuthError::WeakSecret`] naming the secret's file, [`AuthError::Secret`] if unreadable.
    pub fn check_secret(&self) -> Result<(), AuthError> {
        self.strong_secret().map(drop)
    }

    fn strong_secret(&self) -> Result<SecretString, AuthError> {
        let secret = self.client_secret.read()?;
        if secret.expose_secret().len() < MIN_SECRET {
            return Err(AuthError::WeakSecret(
                self.client_secret.path().to_path_buf(),
            ));
        }
        Ok(secret)
    }

    /// HKDF-SHA256(client secret, info [`PURPOSE`]) as an AES-256-GCM key; read and checked at
    /// each use, so a rotated secret takes effect at once.
    fn key(&self) -> Result<LessSafeKey, AuthError> {
        let secret = self.strong_secret()?;
        let prk = Salt::new(HKDF_SHA256, &[]).extract(secret.expose_secret().as_bytes());
        let okm = prk
            .expand(&[PURPOSE], &AES_256_GCM)
            .map_err(|_| AuthError::Cookie("key derivation failed"))?;
        Ok(LessSafeKey::new(UnboundKey::from(okm)))
    }
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
