//! Trace discovery and processed-trace ledger ports.

use crux_improve::{Crux, CruxId};

/// A validated trace and the source it was loaded from.
#[derive(Debug, Clone)]
pub struct DiscoveredTrace {
    /// Human-readable origin of the trace.
    pub source: String,
    /// Fully validated raw trace.
    pub trace: Crux<serde_json::Value>,
}

/// A trace-like input that could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedTrace {
    /// Human-readable origin of the rejected input.
    pub source: String,
    /// Reason the input could not be loaded as a raw trace.
    pub reason: String,
}

/// Results from scanning a trace source.
#[derive(Debug, Clone, Default)]
pub struct TraceDiscovery {
    /// Number of JSON files inspected.
    pub scanned_files: usize,
    /// Number of valid JSON files that did not resemble traces.
    pub ignored_files: usize,
    /// Valid raw traces found by the source.
    pub traces: Vec<DiscoveredTrace>,
    /// Trace-like files that failed validation.
    pub rejected: Vec<RejectedTrace>,
}

/// Errors that prevent a trace source from completing discovery.
#[derive(Debug, thiserror::Error)]
pub enum TraceSourceError {
    /// Discovery could not complete.
    #[error("trace discovery failed: {0}")]
    Discovery(String),
}

/// Errors that prevent processed trace IDs from being persisted.
#[derive(Debug, thiserror::Error)]
pub enum IngestionLedgerError {
    /// Ledger state could not be loaded or persisted.
    #[error("ingestion ledger error: {0}")]
    Store(String),
}

/// Discovers validated traces from an external source.
pub trait TraceSource: Send + Sync {
    /// Returns all valid traces and non-fatal rejections found by this source.
    ///
    /// # Errors
    ///
    /// Returns [`TraceSourceError`] when discovery cannot safely complete.
    fn discover(&self) -> Result<TraceDiscovery, TraceSourceError>;
}

/// Tracks trace IDs that have already completed ingestion.
pub trait IngestionLedger: Send + Sync {
    /// Reports whether a trace ID has completed ingestion.
    fn contains(&self, trace_id: &CruxId) -> bool;

    /// Marks a trace ID as successfully ingested.
    ///
    /// # Errors
    ///
    /// Returns [`IngestionLedgerError`] when the ID cannot be persisted.
    fn record(&mut self, trace_id: &CruxId) -> Result<(), IngestionLedgerError>;
}

#[cfg(test)]
mod tests {
    use super::{IngestionLedgerError, TraceDiscovery, TraceSourceError};

    #[test]
    fn trace_discovery_defaults_to_empty() {
        let discovery = TraceDiscovery::default();

        assert_eq!(discovery.scanned_files, 0);
        assert_eq!(discovery.ignored_files, 0);
        assert!(discovery.traces.is_empty());
        assert!(discovery.rejected.is_empty());
    }

    #[test]
    fn ingestion_errors_include_context() {
        let source = TraceSourceError::Discovery("missing root".into());
        let ledger = IngestionLedgerError::Store("read-only file".into());

        assert!(source.to_string().contains("missing root"));
        assert!(ledger.to_string().contains("read-only file"));
    }
}
