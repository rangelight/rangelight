//! The Linear adapter for `TicketSource` (ADR 0049 §2.2): lists a team's open issues labelled
//! `rangelight` through Linear's GraphQL API, 50 per page.
//!
//! The API key is read from its secret file at each poll and sent only in the `Authorization`
//! header. It is never logged, and errors never carry it or a response body.
#![deny(missing_docs)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderValue};
use rl_app::{TicketError, TicketSource};
use rl_config::SecretRef;
use rl_domain::{INTAKE_LABEL, Ticket, Tracker};
use rustls::pki_types::CertificateDer;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;

/// Linear's GraphQL endpoint.
pub const API_URL: &str = "https://api.linear.app/graphql";

const PAGE: u32 = 50;
/// A runaway cursor can't keep a poll going for ever: 200 pages are 10,000 issues.
const MAX_PAGES: u32 = 200;

const QUERY: &str = "query Intake($team: String!, $label: String!, $first: Int!, $after: String) {
  issues(first: $first, after: $after, filter: {
    team: { key: { eq: $team } },
    labels: { name: { eq: $label } },
    state: { type: { nin: [\"completed\", \"canceled\"] } }
  }) {
    nodes { id identifier title description url }
    pageInfo { hasNextPage endCursor }
  }
}";

/// A Linear team's issues as a [`TicketSource`].
#[derive(Debug, Clone)]
pub struct Linear {
    http: reqwest::Client,
    url: String,
    team: String,
    api_key: SecretRef,
    /// The issues already warned about, so a skipped issue is logged once, not every poll.
    /// ponytail: grows by one id per unusable issue for the process's life.
    warned: Arc<Mutex<HashSet<String>>>,
}

impl Linear {
    /// A source for the team with key `team` (for example `RAN`), at `url` (normally
    /// [`API_URL`]), authenticating with the key in `api_key`. HTTPS trusts the platform's roots
    /// and `extra_roots`. Each request times out after 10s.
    ///
    /// # Errors
    /// [`TicketError::Unavailable`] if the HTTP client can't be built.
    pub fn new(
        url: String,
        team: String,
        api_key: SecretRef,
        extra_roots: &[CertificateDer<'static>],
    ) -> Result<Linear, TicketError> {
        // reqwest's TLS needs a process-wide rustls provider; an Err means one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let extra_roots = extra_roots
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| TicketError::Unavailable(e.to_string()))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .tls_certs_merge(extra_roots)
            .build()
            .map_err(|e| TicketError::Unavailable(e.to_string()))?;
        Ok(Linear {
            http,
            url,
            team,
            api_key,
            warned: Arc::default(),
        })
    }

    async fn page(&self, after: Option<&str>) -> Result<Issues, TicketError> {
        let key = self
            .api_key
            .read()
            .map_err(|e| TicketError::Credential(e.to_string()))?;
        let body = json!({
            "query": QUERY,
            "variables": { "team": self.team, "label": INTAKE_LABEL, "first": PAGE, "after": after },
        });
        let mut authorization = HeaderValue::from_str(key.expose_secret())
            .map_err(|_| TicketError::Credential("the key isn't a valid header value".into()))?;
        // Kept out of reqwest's and hyper's debug output.
        authorization.set_sensitive(true);
        let response = self
            .http
            .post(&self.url)
            .header(AUTHORIZATION, authorization)
            .json(&body)
            .send()
            .await
            // Without the URL: it is config, not secret, but errors stay short and uniform.
            .map_err(|e| TicketError::Unavailable(e.without_url().to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(TicketError::Unauthorized);
        }
        if !status.is_success() {
            // The body is never surfaced: it is untrusted and may echo the request.
            return Err(TicketError::Unavailable(format!(
                "HTTP {}",
                status.as_u16()
            )));
        }
        // No parser detail: serde quotes the offending value, which may echo the request.
        let answer: Answer = response
            .json()
            .await
            .map_err(|_| TicketError::Malformed("the answer isn't a page of issues".into()))?;
        answer
            .data
            .map(|data| data.issues)
            .ok_or_else(|| TicketError::Malformed("no data in the answer".into()))
    }
}

impl TicketSource for Linear {
    async fn tickets(&self) -> Result<Vec<Ticket>, TicketError> {
        let mut tickets = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let issues = self.page(after.as_deref()).await?;
            tickets.extend(
                issues
                    .nodes
                    .into_iter()
                    .filter_map(|node| self.usable(node)),
            );
            match (issues.page_info.has_next_page, issues.page_info.end_cursor) {
                (true, Some(cursor)) => after = Some(cursor),
                (true, None) => {
                    return Err(TicketError::Malformed("next page without a cursor".into()));
                }
                (false, _) => {
                    tracing::debug!(team = %self.team, count = tickets.len(), "listed Linear issues");
                    return Ok(tickets);
                }
            }
        }
        Err(TicketError::Malformed(format!(
            "more than {MAX_PAGES} pages"
        )))
    }
}

impl Linear {
    /// The node as a ticket. One issue Rangelight can't use (it breaks the ticket rule, say, no
    /// acceptance criteria) is skipped, never sent to a runner, and not fatal: the team's other
    /// issues still get sessions. It is logged the first time only.
    fn usable(&self, node: Node) -> Option<Ticket> {
        let ticket = Ticket::new(
            Tracker::Linear,
            node.id.clone(),
            node.identifier,
            node.title,
            node.description.unwrap_or_default(),
        )
        .and_then(|ticket| ticket.with_url(node.url));
        if let Err(error) = &ticket
            && self
                .warned
                .lock()
                .is_ok_and(|mut warned| warned.insert(node.id.clone()))
        {
            tracing::warn!(issue = %node.id, %error, "skipped a Linear issue");
        }
        ticket.ok()
    }
}

#[derive(Deserialize)]
struct Answer {
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    issues: Issues,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Issues {
    nodes: Vec<Node>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    identifier: String,
    title: String,
    /// Markdown; Linear sends null for an empty one.
    description: Option<String>,
    /// The issue's page.
    url: String,
}
