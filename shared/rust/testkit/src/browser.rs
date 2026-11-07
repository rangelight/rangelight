//! A browser for the api's sign-in routes: keeps cookies, never follows redirects, and signs in
//! through the test Dex.

use std::collections::BTreeMap;

use reqwest::header::{COOKIE, HeaderMap, LOCATION, ORIGIN, SET_COOKIE};
use reqwest::{Method, StatusCode, Url};

use crate::{PUBLIC_URL, dex, sign_in_at_dex};

/// One answer, as a test looks at it.
#[derive(Debug, Clone)]
pub struct Page {
    /// The status.
    pub status: StatusCode,
    /// `Location`, if any.
    pub location: Option<String>,
    /// Every `Set-Cookie`, as sent.
    pub set_cookies: Vec<String>,
    /// The body.
    pub body: String,
}

/// A browser on the api at `base`, which it reaches directly while the cookies and `Origin`
/// say [`PUBLIC_URL`].
#[derive(Debug)]
pub struct Browser {
    http: reqwest::Client,
    base: Url,
    /// The cookies it holds, by name.
    pub cookies: BTreeMap<String, String>,
}

impl Browser {
    /// A browser with no cookies.
    #[must_use]
    pub fn new(base: Url) -> Browser {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        Browser {
            http,
            base,
            cookies: BTreeMap::new(),
        }
    }

    /// The `Cookie` header it sends.
    #[must_use]
    pub fn cookie_header(&self) -> String {
        let pairs: Vec<String> = self
            .cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        pairs.join("; ")
    }

    /// A request to `path_and_query` with its cookies and `extra` headers, keeping what it sets.
    pub async fn send(&mut self, method: Method, path_and_query: &str, extra: HeaderMap) -> Page {
        let mut request = self
            .http
            .request(method, self.base.join(path_and_query).unwrap())
            .headers(extra);
        if !self.cookies.is_empty() {
            request = request.header(COOKIE, self.cookie_header());
        }
        let response = request.send().await.unwrap();
        let header = |value: &reqwest::header::HeaderValue| value.to_str().unwrap().to_owned();
        let set_cookies: Vec<String> = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(header)
            .collect();
        for set in &set_cookies {
            let (pair, attributes) = set.split_once(';').unwrap_or((set, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if value.is_empty() || attributes.contains("Max-Age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_owned(), value.to_owned());
            }
        }
        Page {
            status: response.status(),
            location: response.headers().get(LOCATION).map(header),
            set_cookies,
            body: response.text().await.unwrap(),
        }
    }

    /// GET, with no `Origin` (a top-level navigation).
    pub async fn get(&mut self, path_and_query: &str) -> Page {
        self.send(Method::GET, path_and_query, HeaderMap::new())
            .await
    }

    /// POST from the public origin.
    pub async fn post(&mut self, path: &str) -> Page {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, PUBLIC_URL.parse().unwrap());
        self.send(Method::POST, path, headers).await
    }

    /// /auth/login, then Dex as `email`: returns the callback path and query Dex sent the
    /// browser back with, not yet visited.
    pub async fn start_sign_in(&mut self, email: &str) -> String {
        let started = self.get("/auth/login").await;
        assert_eq!(started.status, StatusCode::SEE_OTHER, "{}", started.body);
        let authorization = Url::parse(started.location.as_deref().unwrap()).unwrap();
        let dex = dex();
        let back = sign_in_at_dex(&authorization, email, &dex.redirect).await;
        format!("{}?{}", back.path(), back.query().unwrap())
    }

    /// The whole sign-in as `email`: returns the callback's reply.
    pub async fn sign_in(&mut self, email: &str) -> Page {
        let callback = self.start_sign_in(email).await;
        self.get(&callback).await
    }
}
