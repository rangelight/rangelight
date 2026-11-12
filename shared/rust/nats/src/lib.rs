//! A workload's NATS connection with its own identity (ADR 0021 amendment). The workload signs in
//! with its projected ServiceAccount token, which the api's auth callout verifies and maps to
//! exact permissions. The token is read from its file at every connect and reconnect, so the
//! kubelet's rotation and the one-hour user JWT never end a long-lived connection.
#![deny(missing_docs)]

use std::path::PathBuf;

use async_nats::{Auth, AuthError, ConnectOptions};

/// A NATS URL a token may not be sent to.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "nats_url must be tls://<host>[:<port>], or nats:// to a host in this cluster (<name>.svc, \
     <name>.svc.cluster.local) or on loopback, without credentials"
)]
pub struct UrlError;

/// Checks a NATS URL a token travels to: `tls://`, or `nats://` only to a cluster-local Service
/// or loopback, so the token never crosses a network in clear text. One server, no credentials.
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
        Some((host, port)) if port.parse::<u16>().is_ok() => host,
        Some(_) => return Err(UrlError),
        None => authority,
    };
    let service = host.ends_with(".svc.cluster.local")
        || host
            .rsplit_once('.')
            .is_some_and(|(name, last)| !name.is_empty() && last == "svc");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_travels_only_over_tls_or_inside_the_cluster() {
        for good in [
            "tls://nats.example.com:4222",
            "tls://nats.example.com",
            "nats://rangelight-nats.rangelight-system.svc:4222",
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
}
