//! Tracing setup: JSON logs to stdout, one line per event, with the current span's fields
//! (`session_id`, `org_id`, `trace_id`, ADR 0049 §3).
#![deny(missing_docs)]

use tracing_subscriber::EnvFilter;

/// The filter is invalid, or a subscriber is already installed.
#[derive(Debug, thiserror::Error)]
#[error("telemetry: {0}")]
pub struct TelemetryError(String);

/// Installs the global subscriber. `filter` uses `tracing`'s directive syntax, for example
/// `info` or `info,rl_store=debug`; it comes from the service's config, not the environment.
///
/// # Errors
/// [`TelemetryError`] for an invalid filter or a second call.
pub fn init(filter: &str) -> Result<(), TelemetryError> {
    let filter = EnvFilter::try_new(filter).map_err(|e| TelemetryError(e.to_string()))?;
    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_env_filter(filter)
        .try_init()
        .map_err(|e| TelemetryError(e.to_string()))
}
