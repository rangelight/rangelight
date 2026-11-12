//! A local HTTP stand-in for Linear's GraphQL endpoint: a few lines of HTTP/1.1, no framework.

use std::sync::{Arc, Mutex};

use crate::Seed;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the stub answers.
#[derive(Debug, Clone)]
pub enum Reply {
    /// The seeded tickets carrying the request's `label` variable, as Linear filters them,
    /// paged by its `first` and `after` variables; and success to every `attachmentCreate`.
    Tickets(Vec<Seed>),
    /// This GraphQL answer, verbatim, with status 200.
    Answer(Value),
    /// This HTTP status, with a body that echoes the request's `Authorization` header where
    /// `data` should be, so a test can prove the caller never surfaces response bodies.
    Status(u16),
}

/// A request the stub received.
#[derive(Debug, Clone)]
pub struct StubRequest {
    /// The `Authorization` header.
    pub authorization: Option<String>,
    /// The GraphQL body.
    pub body: Value,
}

/// A running stub. It stops when dropped.
#[derive(Debug)]
pub struct LinearStub {
    /// Its GraphQL URL.
    pub url: String,
    requests: Arc<Mutex<Vec<StubRequest>>>,
    task: tokio::task::JoinHandle<()>,
}

impl LinearStub {
    /// Starts a stub on a free local port.
    pub async fn start(reply: Reply) -> LinearStub {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/graphql", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                serve(stream, &reply, &seen).await;
            }
        });
        LinearStub {
            url,
            requests,
            task,
        }
    }

    /// The requests so far.
    #[must_use]
    pub fn requests(&self) -> Vec<StubRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for LinearStub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, reply: &Reply, seen: &Mutex<Vec<StubRequest>>) {
    let request = read_request(&mut stream).await;
    let (status, body) = match reply {
        Reply::Tickets(_)
            if request.body["query"]
                .as_str()
                .is_some_and(|q| q.contains("attachmentCreate")) =>
        {
            (
                200,
                json!({ "data": { "attachmentCreate": { "success": true } } }),
            )
        }
        Reply::Tickets(tickets) => (200, page(tickets, &request.body)),
        Reply::Answer(answer) => (200, answer.clone()),
        Reply::Status(status) => {
            let echo = request.authorization.clone().unwrap_or_default();
            (
                *status,
                json!({ "data": echo, "errors": [{ "message": echo }] }),
            )
        }
    };
    seen.lock().unwrap().push(request);
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 {status} Stub\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
}

async fn read_request(stream: &mut TcpStream) -> StubRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed mid-request");
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(i) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let length: usize = header("content-length").map_or(0, |v| v.parse().unwrap());
    while buffer.len() < head_end + length {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed mid-body");
        buffer.extend_from_slice(&chunk[..n]);
    }
    let body = serde_json::from_slice(&buffer[head_end..head_end + length]).unwrap();
    StubRequest {
        authorization: header("authorization"),
        body,
    }
}

/// One page of `issues`, as Linear shapes it. The cursor is the index of the next ticket.
fn page(seeds: &[Seed], body: &Value) -> Value {
    let variables = &body["variables"];
    let label = variables["label"].as_str().unwrap();
    let tickets: Vec<_> = seeds
        .iter()
        .filter(|seed| seed.labels.iter().any(|l| l == label))
        .collect();
    let first = variables["first"]
        .as_u64()
        .map_or(50, |n| usize::try_from(n).unwrap());
    let start = variables["after"]
        .as_str()
        .map_or(0, |cursor| cursor.parse().unwrap());
    let end = (start + first).min(tickets.len());
    let nodes: Vec<Value> = tickets[start..end]
        .iter()
        .map(|seed| {
            let t = &seed.ticket;
            json!({ "id": t.id(), "identifier": t.key(), "title": t.title(), "description": t.description(), "url": t.url() })
        })
        .collect();
    json!({ "data": { "issues": {
        "nodes": nodes,
        "pageInfo": { "hasNextPage": end < tickets.len(), "endCursor": end.to_string() },
    } } })
}
