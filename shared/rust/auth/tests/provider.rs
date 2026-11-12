//! What `rl-auth` does with a provider that misbehaves, against a raw HTTP stub: answers over
//! 1 MiB are refused before parsing, and discovery is fetched once however many ask at once
//! (security review hardening). And one behind a throwaway CA, over HTTPS.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rl_auth::{AuthError, Provider, Url};
use rl_config::SecretRef;
use rl_testkit::HttpsStub;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves `body(issuer)` for every request (with a Content-Length or, if `chunked`, without one,
/// closing the connection), counting the requests. Returns the issuer URL.
async fn stub(body: fn(&str) -> String, with_length: bool, count: Arc<AtomicUsize>) -> Url {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}/dex", listener.local_addr().unwrap());
    let served = issuer.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            let body = body(&served);
            tokio::spawn(async move {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                // Slow enough that concurrent callers overlap.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let head = if with_length {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n"
                        .to_owned()
                };
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    Url::parse(&issuer).unwrap()
}

fn provider(issuer: &Url) -> Provider {
    with_roots(issuer, &[])
}

fn with_roots(
    issuer: &Url,
    extra_roots: &[rustls::pki_types::CertificateDer<'static>],
) -> Provider {
    Provider::new(
        issuer.clone(),
        rl_auth::Registration {
            client_id: "rangelight-api".into(),
            client_secret: SecretRef::file("/nonexistent"),
            redirect: issuer.join("/auth/callback").unwrap(),
        },
        "preferred_username".into(),
        extra_roots,
    )
    .unwrap()
}

/// A valid discovery document whose JWKS (the same URL, so the same body) is also a JWKS.
fn discovery(issuer: &str) -> String {
    format!(
        r#"{{"issuer":"{issuer}","authorization_endpoint":"{issuer}/auth","token_endpoint":"{issuer}/token","jwks_uri":"{issuer}/keys","response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"keys":[]}}"#
    )
}

fn huge(_: &str) -> String {
    format!(r#"{{"pad":"{}"}}"#, "x".repeat((1 << 20) + 1))
}

/// RAN-54: an issuer behind your own CA is refused until its CA is among the extra roots.
#[tokio::test]
async fn an_issuer_behind_your_own_ca_needs_the_extra_root() {
    let stub = HttpsStub::start(|url| discovery(&format!("{url}/dex"))).await;
    let issuer = Url::parse(&format!("{}/dex", stub.url)).unwrap();
    let error = provider(&issuer).start(false).await.unwrap_err();
    assert!(matches!(error, AuthError::Unavailable(_)), "{error}");
    with_roots(&issuer, std::slice::from_ref(&stub.ca))
        .start(false)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_answer_over_one_mib_is_refused() {
    for with_length in [true, false] {
        let issuer = stub(huge, with_length, Arc::default()).await;
        let error = provider(&issuer).start(false).await.unwrap_err();
        assert!(
            matches!(&error, AuthError::Unavailable(why) if why.contains("over 1048576 bytes")),
            "{error}"
        );
    }
}

#[tokio::test]
async fn discovery_is_fetched_once_for_many_callers() {
    let count = Arc::new(AtomicUsize::new(0));
    let issuer = stub(discovery, true, count.clone()).await;
    let provider = Arc::new(provider(&issuer));
    let starts = (0..20).map(|_| {
        let provider = provider.clone();
        tokio::spawn(async move { provider.start(false).await.map(drop) })
    });
    for start in starts {
        start.await.unwrap().unwrap();
    }
    // One discovery document and one JWKS.
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// Security review round 2, item 2: a provider that never answers costs one fetch (10 s timeout),
/// not one per waiter: everyone queued behind it gets its failure, and so does anyone in the 5 s
/// after.
#[tokio::test]
async fn a_hung_provider_is_asked_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = Url::parse(&format!("http://{}/dex", listener.local_addr().unwrap())).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let accepted = count.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            accepted.fetch_add(1, Ordering::SeqCst);
            held.push(socket);
        }
    });
    let provider = Arc::new(provider(&issuer));
    let starts: Vec<_> = (0..20)
        .map(|_| {
            let provider = provider.clone();
            tokio::spawn(async move { provider.start(false).await.map(drop) })
        })
        .collect();
    let all = async {
        for start in starts {
            assert!(matches!(
                start.await.unwrap(),
                Err(AuthError::Unavailable(_))
            ));
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(25), all)
        .await
        .expect("every waiter failed with the one fetch");
    assert!(matches!(
        provider.start(false).await,
        Err(AuthError::Unavailable(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
