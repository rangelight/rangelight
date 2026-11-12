//! A workload's NATS connection with its own identity (ADR 0021 amendment). The workload signs in
//! with its projected ServiceAccount token, which the api's auth callout verifies and maps to
//! exact permissions. The token is read from its file at every connect and reconnect, so the
//! kubelet's rotation and the one-hour user JWT never end a long-lived connection.
#![deny(missing_docs)]

use std::path::PathBuf;
use std::time::Duration;

use async_nats::{Auth, AuthError, Client, ConnectError, ConnectErrorKind, ConnectOptions};

/// A NATS URL a token may not be sent to.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "nats_url must be tls://<host>[:<port>], or nats:// to a Service in this cluster by its full name \
     (<name>.<namespace>.svc.cluster.local: lanes can't resolve a bare .svc) or on loopback, without \
     credentials"
)]
pub struct UrlError;

/// Checks a NATS URL a token travels to: `tls://`, or `nats://` only to a cluster-local Service
/// by its full name (`<name>.<namespace>.svc.cluster.local`) or loopback, so the token never crosses
/// a network in clear text. Not a bare `.svc`: it is the URL the lanes get too, and they resolve only
/// exact names. One server, no credentials.
///
/// # Errors
/// [`UrlError`] otherwise.
pub fn check_url(url: &str) -> Result<(), UrlError> {
    let (tls, rest) = match url.split_once("://") {
        Some(("tls", rest)) => (true, rest),
        Some(("nats", rest)) => (false, rest),
        _ => return Err(UrlError),
    };
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() || authority.contains(['@', '/', ',', '?', '#']) {
        return Err(UrlError);
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.parse::<u16>().is_ok_and(|p| p != 0) => host,
        Some(_) => return Err(UrlError),
        None => authority,
    };
    let service = host
        .strip_suffix(".svc.cluster.local")
        .is_some_and(|name| !name.is_empty());
    let local = service || matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if host.is_empty() || !(tls || local) {
        return Err(UrlError);
    }
    Ok(())
}

/// Connect options that sign in with the token in `token_file`, read again at every connect,
/// and take replies only under the identity's inbox, `rl_domain::subject::inbox(id)`: the one
/// prefix its permissions let it subscribe to.
#[must_use]
pub fn options(token_file: PathBuf, id: &str) -> ConnectOptions {
    ConnectOptions::with_auth_callback(move |_nonce| {
        let path = token_file.clone();
        async move {
            let token = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| AuthError::new(format!("{}: {e}", path.display())))?;
            let mut auth = Auth::new();
            auth.token = Some(token.trim().to_owned());
            Ok(auth)
        }
    })
    .custom_inbox_prefix(rl_domain::subject::inbox(id))
}

/// The longest pause between two tries to reach NATS at start.
pub const MAX_RETRY: Duration = Duration::from_secs(5);

/// The pause after `failures` tries that couldn't reach NATS: 100 ms, doubling, at most [`MAX_RETRY`].
#[must_use]
pub fn retry_pause(failures: u32) -> Duration {
    Duration::from_millis(100)
        .saturating_mul(2u32.saturating_pow(failures))
        .min(MAX_RETRY)
}

/// Connects to `url` with the options `options` makes, trying again while NATS can't be reached
/// (DNS, I/O, a timeout): every failed try is logged, with [`retry_pause`] between tries. Any other
/// error (a refused sign-in, a bad URL, a failed TLS handshake or certificate) is returned at once. Once connected, the client
/// reconnects by itself, to `url` only: the servers NATS advertises (INFO's `connect_urls`) are
/// ignored, since only `url` was checked ([`check_url`]) and the token goes wherever it dials
/// (RAN-80). async-nats' own `retry_on_initial_connect` isn't used: it returns before the
/// connection is up and retries a refused sign-in forever.
///
/// # Errors
/// The first [`ConnectError`] that isn't NATS being unreachable.
pub async fn connect(
    options: impl Fn() -> ConnectOptions,
    url: &str,
) -> Result<Client, ConnectError> {
    let mut failures = 0;
    loop {
        match options().ignore_discovered_servers().connect(url).await {
            Ok(client) => return Ok(client),
            Err(error) if unreachable(&error) => {
                let pause = retry_pause(failures);
                tracing::warn!(%error, retry_in_ms = pause.as_millis(), "nats: not reachable, trying again");
                tokio::time::sleep(pause).await;
                failures = failures.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Whether `error` means NATS couldn't be reached (worth another try), not that it was reached and
/// refused, or that the client's own setup is wrong.
fn unreachable(error: &ConnectError) -> bool {
    match error.kind() {
        ConnectErrorKind::Dns | ConnectErrorKind::TimedOut => true,
        // async-nats reports a failed TLS handshake, a bad certificate included, as I/O too:
        // tokio-rustls wraps the rustls error in an io::Error.
        ConnectErrorKind::Io => !tls_failure(error),
        _ => false,
    }
}

/// Whether a rustls error is anywhere in `error`'s sources.
fn tls_failure(error: &ConnectError) -> bool {
    let mut source = std::error::Error::source(error);
    while let Some(e) = source {
        // An io::Error's own source() skips the error it wraps, so look inside it.
        let wrapped = e
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .is_some_and(<dyn std::error::Error + Send + Sync>::is::<async_nats::rustls::Error>);
        if wrapped || e.is::<async_nats::rustls::Error>() {
            return true;
        }
        source = e.source();
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_token_travels_only_over_tls_or_inside_the_cluster() {
        for good in [
            "tls://nats.example.com:4222",
            "tls://nats.example.com",
            "nats://rangelight-nats.rangelight-system.svc.cluster.local:4222",
            "nats://nats.ns.svc.cluster.local:4222/",
            "nats://127.0.0.1:4222",
            "nats://localhost:4222",
        ] {
            assert_eq!(check_url(good), Ok(()), "{good}");
        }
        for bad in [
            "nats://nats.example.com:4222",
            "nats://10.0.0.7:4222",
            "nats://evil.svc.example.com:4222",
            // A lane can't resolve it (ndots 1).
            "nats://rangelight-nats.rangelight-system.svc:4222",
            "nats://.svc.cluster.local:4222",
            "nats://nats.ns.svc.cluster.local:0",
            "tls://user:pass@nats.example.com",
            "tls://a:4222,tls://b:4222",
            "tls://nats.example.com:port",
            "tls://:4222",
            "http://nats.example.com",
            "nats.example.com:4222",
        ] {
            assert_eq!(check_url(bad), Err(UrlError), "{bad}");
        }
    }

    #[test]
    fn the_retry_pause_doubles_up_to_five_seconds() {
        assert_eq!(retry_pause(0), Duration::from_millis(100));
        assert_eq!(retry_pause(1), Duration::from_millis(200));
        assert_eq!(retry_pause(5), Duration::from_millis(3200));
        assert_eq!(retry_pause(6), MAX_RETRY);
        assert_eq!(retry_pause(u32::MAX), MAX_RETRY);
    }

    /// A NATS that is away is waited for; one that refuses the sign-in is not.
    #[tokio::test]
    async fn connect_waits_for_nats_but_not_for_a_refusal() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("nats://127.0.0.1:{port}");
        let waiting = tokio::spawn({
            let url = url.clone();
            async move { connect(ConnectOptions::new, &url).await }
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!waiting.is_finished());
        let mut server = std::process::Command::new("nats-server")
            .args([
                "-a",
                "127.0.0.1",
                "-p",
                &port.to_string(),
                "--auth",
                "right",
            ])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("nats-server: run the tests via scripts/test/services.sh");
        // The waiting client has no token: once the server is up it is refused, and that ends it.
        let refused = tokio::time::timeout(MAX_RETRY + Duration::from_secs(2), waiting).await;
        let signed_in = connect(|| ConnectOptions::new().token("right".into()), &url).await;
        let _ = server.kill();
        let _ = server.wait();
        let refused = refused.unwrap().unwrap();
        assert!(
            matches!(&refused, Err(e) if matches!(e.kind(), ConnectErrorKind::AuthorizationViolation)),
            "{:?}",
            refused.err()
        );
        assert!(signed_in.is_ok());
    }

    /// RAN-80: a server that advertises another address (INFO `connect_urls`) and then drops the
    /// client never gets it dialed: the client reconnects to the configured server only.
    #[tokio::test]
    async fn a_reconnect_never_dials_an_advertised_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let configured = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let advertised = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let elsewhere = advertised.local_addr().unwrap();
        let dialed = tokio::spawn(async move { advertised.accept().await.is_ok() });
        let url = format!("nats://{}", configured.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let info = format!(
                "INFO {{\"server_id\":\"fake\",\"version\":\"2.15.2\",\"max_payload\":1048576,\"connect_urls\":[\"{elsewhere}\"]}}\r\n"
            );
            // Three connections from the client: each is dropped once it is up.
            for _ in 0..3 {
                let (mut socket, _) = configured.accept().await.unwrap();
                socket.write_all(info.as_bytes()).await.unwrap();
                let mut seen = Vec::new();
                let mut buffer = [0u8; 1024];
                while !String::from_utf8_lossy(&seen).contains("PING") {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0, "the client hung up");
                    seen.extend_from_slice(&buffer[..n]);
                }
                socket.write_all(b"PONG\r\n").await.unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        let client = connect(ConnectOptions::new, &url).await.unwrap();
        tokio::time::timeout(Duration::from_secs(20), server)
            .await
            .expect("the client reconnected to the configured server, again and again")
            .unwrap();
        assert!(
            !dialed.is_finished(),
            "the client dialed the advertised server"
        );
        dialed.abort();
        drop(client);
    }

    /// RAN-79 review: a failed TLS handshake (here, a server that answers TLS with bytes that
    /// aren't TLS; a bad certificate fails the same way) is returned at once, not retried as
    /// "not reachable".
    #[tokio::test]
    async fn a_failed_tls_handshake_is_not_retried() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                socket
                    .write_all(b"INFO {\"server_id\":\"x\",\"version\":\"2.15.2\",\"proto\":1,\"max_payload\":1048576,\"tls_required\":true}\r\n")
                    .await
                    .unwrap();
                let mut hello = [0; 512];
                let _ = socket.read(&mut hello).await;
                let _ = socket.write_all(b"this is not TLS\r\n\r\n").await;
            }
        });
        let url = format!("tls://localhost:{port}");
        let result =
            tokio::time::timeout(Duration::from_secs(2), connect(ConnectOptions::new, &url)).await;
        server.abort();
        let error = result
            .expect("returned at once, not retried")
            .err()
            .unwrap();
        assert!(tls_failure(&error), "{error:?}");
    }
}
