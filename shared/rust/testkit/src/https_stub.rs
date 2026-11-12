//! A local HTTPS server whose certificate a throwaway CA signs: what a provider behind your own
//! PKI looks like to the api.

use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A running stub. It stops when dropped.
#[derive(Debug)]
pub struct HttpsStub {
    /// Its base URL, `https://127.0.0.1:<port>`.
    pub url: String,
    /// The certificate of the CA that signed its certificate, trusted nowhere else.
    pub ca: CertificateDer<'static>,
    /// The same, in PEM.
    pub ca_pem: String,
    task: tokio::task::JoinHandle<()>,
}

impl HttpsStub {
    /// Starts a stub on a free local port that answers every request with 200 and the JSON
    /// `body(url)`.
    pub async fn start(body: fn(&str) -> String) -> HttpsStub {
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let (ca, ca_pem) = (ca_cert.der().clone(), ca_cert.pem());
        let key = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["127.0.0.1".to_owned()])
            .unwrap()
            .signed_by(&key, &Issuer::new(ca_params, ca_key))
            .unwrap();
        // Idempotent: an Err means a provider is installed already.
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(std::sync::Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let served = url.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let body = body(&served);
                tokio::spawn(async move {
                    // A client that refuses the certificate ends the handshake: nothing to serve.
                    let Ok(mut tls) = acceptor.accept(stream).await else {
                        return;
                    };
                    let mut request = [0u8; 4096];
                    let _ = tls.read(&mut request).await;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = tls.write_all(head.as_bytes()).await;
                    let _ = tls.write_all(body.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        HttpsStub {
            url,
            ca,
            ca_pem,
            task,
        }
    }
}

impl Drop for HttpsStub {
    fn drop(&mut self) {
        self.task.abort();
    }
}
