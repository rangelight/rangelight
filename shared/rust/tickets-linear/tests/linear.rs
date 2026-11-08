//! The Linear adapter against a local stub.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use rl_app::{TicketError, TicketSource};
use rl_config::SecretRef;
use rl_domain::INTAKE_LABEL;
use rl_testkit::{LinearStub, Reply, ticket, ticket_source_contract};
use rl_tickets_linear::Linear;

const KEY: &str = "lin_api_TESTKEY0123456789abcdef";

fn key_file() -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{KEY}").unwrap();
    file
}

fn linear(stub: &LinearStub, key: &tempfile::NamedTempFile) -> Linear {
    Linear::new(stub.url.clone(), "RAN".into(), SecretRef::file(key.path())).unwrap()
}

#[tokio::test]
async fn passes_the_ticket_source_contract() {
    let key = key_file();
    let stubs = Arc::new(Mutex::new(Vec::new()));
    ticket_source_contract(|tickets| {
        let (stubs, path) = (Arc::clone(&stubs), key.path().to_owned());
        async move {
            let stub = LinearStub::start(Reply::Tickets(tickets)).await;
            let source =
                Linear::new(stub.url.clone(), "RAN".into(), SecretRef::file(path)).unwrap();
            stubs.lock().unwrap().push(stub);
            source
        }
    })
    .await;
}

#[tokio::test]
async fn asks_for_the_team_and_label_with_the_key_as_authorization() {
    let key = key_file();
    let stub = LinearStub::start(Reply::Tickets(vec![ticket(1, &[INTAKE_LABEL])])).await;
    linear(&stub, &key).tickets().await.unwrap();
    let request = &stub.requests()[0];
    assert_eq!(request.authorization.as_deref(), Some(KEY));
    assert_eq!(request.body["variables"]["team"], "RAN");
    assert_eq!(request.body["variables"]["label"], INTAKE_LABEL);
    assert!(
        request.body["query"]
            .as_str()
            .unwrap()
            .contains("labels: { name: { eq: $label } }")
    );
}

#[tokio::test]
async fn refused_key_is_unauthorized_and_other_failures_are_unavailable() {
    let key = key_file();
    let refused = LinearStub::start(Reply::Status(401)).await;
    assert!(matches!(
        linear(&refused, &key).tickets().await,
        Err(TicketError::Unauthorized)
    ));
    let broken = LinearStub::start(Reply::Status(500)).await;
    assert!(matches!(
        linear(&broken, &key).tickets().await,
        Err(TicketError::Unavailable(_))
    ));
}

#[tokio::test]
async fn a_missing_key_file_is_a_credential_error() {
    let stub = LinearStub::start(Reply::Tickets(vec![])).await;
    let source = Linear::new(
        stub.url.clone(),
        "RAN".into(),
        SecretRef::file("/nonexistent/key"),
    )
    .unwrap();
    assert!(matches!(
        source.tickets().await,
        Err(TicketError::Credential(_))
    ));
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn an_answer_that_isnt_a_page_is_malformed() {
    let stub = LinearStub::start(Reply::Status(200)).await;
    let key = key_file();
    assert!(matches!(
        linear(&stub, &key).tickets().await,
        Err(TicketError::Malformed(_))
    ));
}

/// AC6: the key never appears in logs or errors, even when Linear echoes it back.
#[tokio::test]
async fn linear_key_never_reaches_logs_or_errors() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = {
        let logs = Arc::clone(&logs);
        move || Capture(Arc::clone(&logs))
    };
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(writer)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let key = key_file();
    let mut errors = Vec::new();
    for status in [401, 403, 500, 200] {
        let stub = LinearStub::start(Reply::Status(status)).await;
        errors.push(linear(&stub, &key).tickets().await.unwrap_err());
    }
    let ok = LinearStub::start(Reply::Tickets(vec![ticket(1, &[INTAKE_LABEL])])).await;
    linear(&ok, &key).tickets().await.unwrap();
    // https, as in production: TLS is compiled in, and a failed connection is only Unavailable.
    let unreachable = Linear::new(
        "https://127.0.0.1:9/graphql".into(),
        "RAN".into(),
        SecretRef::file(key.path()),
    )
    .unwrap();
    let error = unreachable.tickets().await.unwrap_err();
    assert!(matches!(error, TicketError::Unavailable(_)), "{error:?}");
    errors.push(error);

    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    assert!(!logs.is_empty(), "the test captured nothing");
    assert!(!logs.contains(KEY), "key in logs");
    for error in errors {
        assert!(
            !format!("{error} {error:?}").contains(KEY),
            "key in error {error:?}"
        );
    }
}

struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Review 1, and RAN-35 AC6: an issue that breaks the ticket rule (a bad key, no acceptance
/// criteria) is skipped, never reaching a runner; the others still come through.
#[tokio::test]
async fn an_invalid_issue_is_skipped_not_fatal() {
    let node = |id: &str, key: &str, description: serde_json::Value| serde_json::json!({ "id": id, "identifier": key, "title": "t", "description": description, "url": format!("https://linear.app/example/issue/{key}") });
    let brief = serde_json::json!("Acceptance\n- AC1 it works");
    let answer = serde_json::json!({ "data": { "issues": {
        "nodes": [
            node("issue-1", "RAN-1", brief.clone()),
            node("issue-2", "not a key!", brief.clone()),
            node("issue-3", "RAN-3", brief),
            node("issue-4", "RAN-4", serde_json::json!("no criteria")),
            node("issue-5", "RAN-5", serde_json::Value::Null),
        ],
        "pageInfo": { "hasNextPage": false, "endCursor": null },
    } } });
    let stub = LinearStub::start(Reply::Answer(answer)).await;
    let key = key_file();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = {
        let logs = Arc::clone(&logs);
        move || Capture(Arc::clone(&logs))
    };
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(writer)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let source = linear(&stub, &key);
    source.tickets().await.unwrap();
    let tickets = source.tickets().await.unwrap();
    let warnings = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    assert_eq!(
        warnings.matches("skipped a Linear issue").count(),
        3,
        "each unusable issue is warned about once, over two polls: {warnings}"
    );
    let ids: Vec<_> = tickets.iter().map(|t| t.id().to_owned()).collect();
    assert_eq!(ids, ["issue-1", "issue-3"]);
}
