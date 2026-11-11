//! Tracing setup: JSON logs to stdout, one line per event, with the current span's fields
//! (`session_id`, `org_id`, `trace_id`, ADR 0049 §3).
#![deny(missing_docs)]

use tracing_subscriber::filter::{FilterExt, LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

/// The filter is invalid, or a subscriber is already installed.
#[derive(Debug, thiserror::Error)]
#[error("telemetry: {0}")]
pub struct TelemetryError(String);

/// Crates whose debug and trace events carry message payloads: NATS messages and WebSocket frames,
/// a lane terminal's keystrokes among them. Held to INFO whatever the filter says (ADR 0046 §3.4).
const PAYLOAD_LOGGERS: [&str; 3] = ["async_nats", "tungstenite", "tokio_tungstenite"];

/// Installs the global subscriber. `filter` uses `tracing`'s directive syntax, for example
/// `info` or `info,rl_store=debug`; it comes from the service's config, not the environment.
///
/// # Errors
/// [`TelemetryError`] for an invalid filter or a second call.
pub fn init(filter: &str) -> Result<(), TelemetryError> {
    init_to(filter, std::io::stdout)
}

/// [`init`], writing to `writer` instead of stdout.
///
/// # Errors
/// [`TelemetryError`] for an invalid filter or a second call.
pub fn init_to<W>(filter: &str, writer: W) -> Result<(), TelemetryError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_new(filter).map_err(|e| TelemetryError(e.to_string()))?;
    let cap = PAYLOAD_LOGGERS.iter().fold(
        Targets::new().with_default(LevelFilter::TRACE),
        |cap, target| cap.with_target(*target, LevelFilter::INFO),
    );
    let logs = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true)
        .with_writer(writer)
        .with_filter(filter.and(cap));
    Registry::default()
        .with(logs)
        .try_init()
        .map_err(|e| TelemetryError(e.to_string()))
}
