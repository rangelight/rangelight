//! The test Dex from `scripts/test/services.sh`: its issuer, the client secret of its two clients
//! (`rangelight-api` and `rangelight-other`), and a browser's walk through its password login.

use reqwest::{StatusCode, Url, header::LOCATION, redirect::Policy};
use rl_config::SecretRef;

/// Where the tests' Workbench is: the origin of the callback Dex redirects to.
pub const PUBLIC_URL: &str = "https://rangelight.test";
/// Dex's password for every test account.
const PASSWORD: &str = "password";

/// The test Dex.
#[derive(Debug, Clone)]
pub struct Dex {
    /// Its issuer, `http://127.0.0.1:<port>/dex`.
    pub issuer: Url,
    /// The client secret of both clients, in a file.
    pub secret: SecretRef,
    /// The redirect URI both clients have, `<PUBLIC_URL>/auth/callback`.
    pub redirect: Url,
}

/// The test Dex that `scripts/test/services.sh` started.
#[must_use]
pub fn dex() -> Dex {
    #[derive(serde::Deserialize)]
    struct Env {
        #[serde(rename = "dex_issuer")]
        issuer: String,
        #[serde(rename = "dex_secret")]
        secret: String,
        #[serde(rename = "dex_redirect")]
        redirect: String,
    }
    let env: Env =
        rl_config::load("testkit", "").expect("Dex: run the tests via scripts/test/services.sh");
    Dex {
        issuer: Url::parse(&env.issuer).unwrap(),
        secret: SecretRef::file(env.secret),
        redirect: Url::parse(&env.redirect).unwrap(),
    }
}

/// Signs `email` in at Dex the way a browser does, from the provider's authorization URL to the
/// redirect back: returns that redirect (the callback URL with `code` and `state`), unvisited.
pub async fn sign_in_at_dex(authorization: &Url, email: &str, redirect: &Url) -> Url {
    let http = reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .unwrap();
    // Follow Dex to its password form, post it, and follow on to the redirect URI.
    let mut url = authorization.clone();
    let mut response = http.get(url.clone()).send().await.unwrap();
    while response.status().is_redirection() {
        url = next(&url, &response);
        response = http.get(url.clone()).send().await.unwrap();
    }
    assert_eq!(response.status(), StatusCode::OK, "Dex's login form");
    let form = [("login", email), ("password", PASSWORD)];
    response = http.post(url.clone()).form(&form).send().await.unwrap();
    loop {
        assert!(
            response.status().is_redirection(),
            "Dex answered {} for {email}",
            response.status()
        );
        url = next(&url, &response);
        if url.as_str().starts_with(redirect.as_str()) {
            return url;
        }
        response = http.get(url.clone()).send().await.unwrap();
    }
}

/// Signs `email` in at Dex as [`sign_in_at_dex`] does, but returns the body of the page Dex shows
/// after the password form: its approval screen under `prompt=consent`, for one (RAN-82). Only for
/// a flow that stops at Dex: it follows redirects, so one back to the callback would fail.
pub async fn page_after_dex_login(authorization: &Url, email: &str) -> String {
    let http = reqwest::Client::builder()
        .redirect(Policy::limited(10))
        .build()
        .unwrap();
    let form = http.get(authorization.clone()).send().await.unwrap();
    assert_eq!(form.status(), StatusCode::OK, "Dex's login form");
    let url = form.url().clone();
    let page = http
        .post(url)
        .form(&[("login", email), ("password", PASSWORD)])
        .send()
        .await
        .unwrap();
    page.text().await.unwrap()
}

fn next(url: &Url, response: &reqwest::Response) -> Url {
    let location = response.headers()[LOCATION].to_str().unwrap();
    url.join(location).unwrap()
}
