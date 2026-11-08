//! The api on a random port, with sign-in through the test Dex (`scripts/test/services.sh`).
// Each test binary uses part of it.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use api::{Auth, SessionApi, serve};
use connectrpc::client::{ClientConfig, HttpClient};
use rl_auth::{Provider, Url};
use rl_proto::rangelight::auth::v1alpha1::AuthServiceClient;
use rl_proto::rangelight::session::v1alpha1::SessionServiceClient;
use rl_store::Store;
use rl_testkit::{Browser, PUBLIC_URL, dex};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// A running api.
pub(crate) struct Served {
    /// Where the test reaches it.
    pub(crate) base: Url,
    pub(crate) cancel: CancellationToken,
}

impl Drop for Served {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The defaults of `configs/api.default.toml`.
pub(crate) const RECHECK: Duration = Duration::from_secs(60);

/// Serves the api for `org` on `store`, signing in at the test Dex as the client
/// `rangelight-api`, for browsers at [`PUBLIC_URL`].
pub(crate) async fn served(store: &Store, org: &str) -> Served {
    served_with(store, org, "preferred_username", RECHECK).await
}

/// [`served`], with this member claim and re-check interval.
pub(crate) async fn served_with(
    store: &Store,
    org: &str,
    member_claim: &str,
    recheck: Duration,
) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let cancel = CancellationToken::new();
    let dex = dex();
    let provider = Provider::new(
        dex.issuer,
        "rangelight-api".into(),
        dex.secret,
        dex.redirect,
        member_claim.into(),
    )
    .unwrap();
    let auth = Auth::new(
        store.clone(),
        provider,
        &Url::parse(PUBLIC_URL).unwrap(),
        recheck,
    );
    let api = SessionApi::new(
        store.clone(),
        rl_testkit::nats().await,
        org.into(),
        auth.clone(),
        cancel.clone(),
    );
    tokio::spawn(serve(listener, api, auth, cancel.clone()));
    Served { base, cancel }
}

impl Served {
    /// A browser with no cookies.
    pub(crate) fn browser(&self) -> Browser {
        Browser::new(self.base.clone())
    }

    /// A browser signed in as `email`.
    pub(crate) async fn signed_in(&self, email: &str) -> Browser {
        let mut browser = self.browser();
        let page = browser.sign_in(email).await;
        assert_eq!(page.status, 303, "{}", page.body);
        browser
    }

    fn config(&self, cookie: &str) -> ClientConfig {
        let mut config = ClientConfig::new(self.base.as_str().parse().unwrap())
            .with_default_header("origin", PUBLIC_URL);
        if !cookie.is_empty() {
            config = config.with_default_header("cookie", cookie);
        }
        config
    }

    /// A session API client sending `cookie` (none if empty) from the public origin.
    pub(crate) fn sessions(&self, cookie: &str) -> SessionServiceClient<HttpClient> {
        SessionServiceClient::new(HttpClient::plaintext(), self.config(cookie))
    }

    /// An auth API client sending `cookie` (none if empty) from the public origin.
    pub(crate) fn auth(&self, cookie: &str) -> AuthServiceClient<HttpClient> {
        AuthServiceClient::new(HttpClient::plaintext(), self.config(cookie))
    }
}
