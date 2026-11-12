//! NATS sign-in for tests: a nats-server running the bundled chart's sign-in config
//! (`deployments/helm/rangelight/files/nats/auth.conf`) with fresh keys, and a stand-in for a
//! cluster's ServiceAccount token issuer (an ES256 key, its JWKS over HTTP, the tokens it signs).

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The bundled server's sign-in config.
const AUTH_CONF: &str =
    include_str!("../../../../deployments/helm/rangelight/files/nats/auth.conf");

/// A nats-server with the bundled sign-in config. It stops when dropped.
#[derive(Debug)]
pub struct AuthNats {
    /// Its client URL.
    pub url: String,
    /// The callout issuer's seed (`issuer-seed` in the chart's Secret).
    pub issuer_seed: String,
    /// The api's auth-user seed (`auth-user-seed`).
    pub auth_user_seed: String,
    server: Child,
    _dir: tempfile::TempDir,
}

impl AuthNats {
    /// Starts one on a free local port, with JetStream, under fresh keys.
    pub async fn start() -> AuthNats {
        let dir = tempfile::tempdir().unwrap();
        let issuer = nkeys::KeyPair::new_account();
        let auth_user = nkeys::KeyPair::new_user();
        std::fs::write(dir.path().join("auth.conf"), AUTH_CONF).unwrap();
        let conf = format!(
            "listen: \"127.0.0.1:-1\"\nports_file_dir: \"{dir}\"\njetstream {{ store_dir: \"{dir}/js\" }}\ninclude \"auth.conf\"\n",
            dir = dir.path().display(),
        );
        std::fs::write(dir.path().join("nats.conf"), conf).unwrap();
        let server = Command::new("nats-server")
            .arg("-c")
            .arg(dir.path().join("nats.conf"))
            .env("RL_NATS_ISSUER", issuer.public_key())
            .env("RL_NATS_AUTH_USER", auth_user.public_key())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("nats-server: run the tests via scripts/test/services.sh");
        let ports = dir
            .path()
            .join(format!("nats-server_{}.ports", server.id()));
        let mut url = None;
        for _ in 0..300 {
            if let Ok(text) = std::fs::read_to_string(&ports)
                && let Ok(ports) = serde_json::from_str::<Value>(&text)
            {
                url = ports["nats"][0].as_str().map(str::to_owned);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        AuthNats {
            url: url.expect("nats-server didn't start"),
            issuer_seed: issuer.seed().unwrap(),
            auth_user_seed: auth_user.seed().unwrap(),
            server,
            _dir: dir,
        }
    }
}

impl Drop for AuthNats {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

/// The audience every NATS token carries.
pub const NATS_AUDIENCE: &str = "rangelight-nats";

/// A cluster's ServiceAccount token issuer: its JWKS at `jwks_url`. It stops when dropped.
#[derive(Debug)]
pub struct FakeIssuer {
    /// Its issuer, the tokens' `iss`.
    pub url: String,
    /// Its JWKS.
    pub jwks_url: String,
    keys: Arc<Mutex<Keys>>,
    fetches: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Debug)]
struct Keys {
    kid: String,
    signing: EcdsaKeyPair,
    jwks: Vec<Value>,
}

impl Keys {
    fn new(n: usize) -> Keys {
        let kid = format!("key-{n}");
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .unwrap();
        let signing = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            pkcs8.as_ref(),
            &SystemRandom::new(),
        )
        .unwrap();
        let point = signing.public_key().as_ref();
        let jwk = json!({
            "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": kid,
            "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
        });
        Keys {
            kid,
            signing,
            jwks: vec![jwk],
        }
    }
}

impl FakeIssuer {
    /// Starts one on a free local port.
    pub async fn start() -> FakeIssuer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let keys = Arc::new(Mutex::new(Keys::new(1)));
        let fetches = Arc::new(AtomicUsize::new(0));
        let (served, counted) = (Arc::clone(&keys), Arc::clone(&fetches));
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                counted.fetch_add(1, Ordering::SeqCst);
                let body = json!({ "keys": served.lock().unwrap().jwks }).to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            }
        });
        FakeIssuer {
            jwks_url: format!("{url}/openid/v1/jwks"),
            url,
            keys,
            fetches,
            task,
        }
    }

    /// A token for the ServiceAccount `namespace:account`, for NATS, valid for an hour.
    #[must_use]
    pub fn token(&self, namespace: &str, account: &str) -> String {
        self.sign(&self.claims(namespace, account))
    }

    /// [`FakeIssuer::token`], bound to the pod `pod` as the kubelet's projected tokens are.
    #[must_use]
    pub fn pod_token(&self, namespace: &str, account: &str, pod: &str) -> String {
        let mut claims = self.claims(namespace, account);
        claims["kubernetes.io"] = json!({"namespace": namespace, "pod": {"name": pod},
            "serviceaccount": {"name": account}});
        self.sign(&claims)
    }

    fn claims(&self, namespace: &str, account: &str) -> Value {
        let now = now();
        json!({
            "iss": self.url,
            "aud": [NATS_AUDIENCE],
            "sub": format!("system:serviceaccount:{namespace}:{account}"),
            "iat": now,
            "nbf": now,
            "exp": now + 3600,
        })
    }

    /// `claims`, signed with the current key (ES256, its `kid`).
    #[must_use]
    pub fn sign(&self, claims: &Value) -> String {
        let keys = self.keys.lock().unwrap();
        let header = json!({"alg": "ES256", "kid": keys.kid, "typ": "JWT"});
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = keys
            .signing
            .sign(&SystemRandom::new(), input.as_bytes())
            .unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
    }

    /// Signs with a new key from now on, and serves only that one.
    pub fn rotate(&self) {
        let mut keys = self.keys.lock().unwrap();
        let next = keys
            .kid
            .trim_start_matches("key-")
            .parse::<usize>()
            .unwrap()
            + 1;
        *keys = Keys::new(next);
    }

    /// How many times its JWKS was fetched.
    #[must_use]
    pub fn fetches(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

impl Drop for FakeIssuer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Seconds since the epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
