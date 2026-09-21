//! Ordered, deduplicated ingestion of discovered traces into the improvement loop.

use crate::ImprovementLoop;
use crux_improve::CruxId;
use praxis_core::{
    IngestionLedger, IngestionLedgerError, RejectedTrace, TraceSource, TraceSourceError,
};
use std::collections::HashSet;

/// A trace that could not complete an improvement cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionFailure {
    /// Human-readable origin of the trace.
    pub source: String,
    /// ID of the trace that failed.
    pub trace_id: CruxId,
    /// Improvement-loop error message.
    pub error: String,
}

/// Aggregate outcome of one trace ingestion scan.
#[derive(Debug, Clone, Default)]
pub struct IngestionReport {
    /// Number of JSON files inspected.
    pub scanned_files: usize,
    /// Number of unrelated JSON files ignored.
    pub ignored_files: usize,
    /// Number of valid traces found, including duplicate IDs.
    pub discovered_traces: usize,
    /// Number of traces successfully processed during this run.
    pub ingested_traces: usize,
    /// Number of IDs skipped because an earlier run processed them.
    pub previously_ingested: usize,
    /// Number of repeated IDs found within this discovery result.
    pub duplicate_ids: usize,
    /// Trace-like files rejected during discovery.
    pub rejected: Vec<RejectedTrace>,
    /// Valid traces whose improvement cycle failed.
    pub failures: Vec<IngestionFailure>,
}

/// Fatal errors that prevent an ingestion run from continuing safely.
#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    /// Trace discovery failed.
    #[error(transparent)]
    Source(#[from] TraceSourceError),

    /// Processed-ID persistence failed.
    #[error(transparent)]
    Ledger(#[from] IngestionLedgerError),
}

/// Discovers and evaluates each previously unseen trace in chronological order.
///
/// # Errors
///
/// Returns [`IngestionError`] when discovery fails or a successfully processed
/// trace ID cannot be persisted. Individual improvement-cycle failures are
/// captured in [`IngestionReport::failures`] instead.
pub async fn ingest_traces(
    runner: &ImprovementLoop,
    source: &dyn TraceSource,
    ledger: &mut dyn IngestionLedger,
) -> Result<IngestionReport, IngestionError> {
    let mut discovery = source.discover()?;
    discovery.traces.sort_by(|left, right| {
        left.trace
            .started_at
            .cmp(&right.trace.started_at)
            .then_with(|| left.trace.id.as_str().cmp(right.trace.id.as_str()))
    });

    let mut report = IngestionReport {
        scanned_files: discovery.scanned_files,
        ignored_files: discovery.ignored_files,
        discovered_traces: discovery.traces.len(),
        rejected: discovery.rejected,
        ..IngestionReport::default()
    };
    let mut seen = HashSet::new();

    for candidate in discovery.traces {
        if !seen.insert(candidate.trace.id.to_string()) {
            report.duplicate_ids += 1;
            continue;
        }
        if ledger.contains(&candidate.trace.id) {
            report.previously_ingested += 1;
            continue;
        }

        match runner.run_cycle(&candidate.trace).await {
            Ok(_) => {
                ledger.record(&candidate.trace.id)?;
                report.ingested_traces += 1;
            }
            Err(error) => report.failures.push(IngestionFailure {
                source: candidate.source,
                trace_id: candidate.trace.id,
                error: error.to_string(),
            }),
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use crux_improve::{Crux, CruxId, DefaultStrategyPolicy};
    use praxis_core::{
        DiscoveredTrace, Evaluation, EvaluationError, Evaluator, IngestionLedger,
        IngestionLedgerError, TraceDiscovery, TraceSource, TraceSourceError,
    };
    use praxis_eval::{DeterministicStrategyPlanner, MetricsEvaluator};
    use praxis_store::{FileStrategyStore, InMemoryRewardStore};
    use std::collections::HashSet;
    use tempfile::TempDir;

    use super::{IngestionError, ingest_traces};
    use crate::ImprovementLoop;

    #[derive(Clone)]
    struct FakeSource {
        discovery: TraceDiscovery,
    }

    impl TraceSource for FakeSource {
        fn discover(&self) -> Result<TraceDiscovery, TraceSourceError> {
            Ok(self.discovery.clone())
        }
    }

    struct FailingSource;

    impl TraceSource for FailingSource {
        fn discover(&self) -> Result<TraceDiscovery, TraceSourceError> {
            Err(TraceSourceError::Discovery("source unavailable".into()))
        }
    }

    #[derive(Default)]
    struct FakeLedger {
        existing: HashSet<String>,
        recorded: Vec<String>,
        fail_record: bool,
    }

    impl IngestionLedger for FakeLedger {
        fn contains(&self, trace_id: &CruxId) -> bool {
            self.existing.contains(trace_id.as_str())
        }

        fn record(&mut self, trace_id: &CruxId) -> Result<(), IngestionLedgerError> {
            if self.fail_record {
                return Err(IngestionLedgerError::Store("ledger unavailable".into()));
            }
            self.existing.insert(trace_id.to_string());
            self.recorded.push(trace_id.to_string());
            Ok(())
        }
    }

    fn trace(agent: &str, started_at: chrono::DateTime<Utc>) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: agent.into(),
            value: Ok(serde_json::Value::Null),
            steps: vec![],
            children: vec![],
            started_at,
            finished_at: Some(started_at),
        }
    }

    fn runner(evaluator: Box<dyn Evaluator>) -> (ImprovementLoop, TempDir) {
        let dir = TempDir::new().unwrap();
        let runner = ImprovementLoop::new(
            evaluator,
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("strategy.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
        );
        (runner, dir)
    }

    #[tokio::test]
    async fn ingestion_orders_deduplicates_and_records_successes() {
        let now = Utc::now();
        let oldest = trace("agent", now - Duration::seconds(2));
        let newest = trace("agent", now);
        let source = FakeSource {
            discovery: TraceDiscovery {
                scanned_files: 4,
                ignored_files: 1,
                traces: vec![
                    DiscoveredTrace {
                        source: "new.json".into(),
                        trace: newest.clone(),
                    },
                    DiscoveredTrace {
                        source: "old.json".into(),
                        trace: oldest.clone(),
                    },
                    DiscoveredTrace {
                        source: "old-copy.json".into(),
                        trace: oldest.clone(),
                    },
                ],
                rejected: vec![],
            },
        };
        let mut ledger = FakeLedger::default();
        let (runner, _dir) = runner(Box::new(MetricsEvaluator));

        let report = ingest_traces(&runner, &source, &mut ledger).await.unwrap();

        assert_eq!(report.scanned_files, 4);
        assert_eq!(report.ignored_files, 1);
        assert_eq!(report.discovered_traces, 3);
        assert_eq!(report.ingested_traces, 2);
        assert_eq!(report.duplicate_ids, 1);
        assert_eq!(
            ledger.recorded,
            vec![oldest.id.to_string(), newest.id.to_string()]
        );
    }

    #[tokio::test]
    async fn ingestion_skips_previously_processed_trace() {
        let trace = trace("agent", Utc::now());
        let source = FakeSource {
            discovery: TraceDiscovery {
                traces: vec![DiscoveredTrace {
                    source: "trace.json".into(),
                    trace: trace.clone(),
                }],
                ..TraceDiscovery::default()
            },
        };
        let mut ledger = FakeLedger::default();
        ledger.existing.insert(trace.id.to_string());
        let (runner, _dir) = runner(Box::new(MetricsEvaluator));

        let report = ingest_traces(&runner, &source, &mut ledger).await.unwrap();

        assert_eq!(report.previously_ingested, 1);
        assert_eq!(report.ingested_traces, 0);
        assert!(ledger.recorded.is_empty());
    }

    struct RejectingEvaluator;

    #[async_trait]
    impl Evaluator for RejectingEvaluator {
        async fn evaluate(
            &self,
            _trace: &Crux<serde_json::Value>,
        ) -> Result<Evaluation, EvaluationError> {
            Err(EvaluationError::Failed("rejected fixture".into()))
        }
    }

    #[tokio::test]
    async fn failed_cycle_is_reported_and_remains_retryable() {
        let trace = trace("agent", Utc::now());
        let source = FakeSource {
            discovery: TraceDiscovery {
                traces: vec![DiscoveredTrace {
                    source: "failed.json".into(),
                    trace: trace.clone(),
                }],
                ..TraceDiscovery::default()
            },
        };
        let mut ledger = FakeLedger::default();
        let (runner, _dir) = runner(Box::new(RejectingEvaluator));

        let report = ingest_traces(&runner, &source, &mut ledger).await.unwrap();

        assert_eq!(report.failures.len(), 1);
        assert!(!ledger.contains(&trace.id));
    }

    #[tokio::test]
    async fn source_and_ledger_errors_are_fatal() {
        let (runner, _dir) = runner(Box::new(MetricsEvaluator));
        let mut ledger = FakeLedger::default();
        let source_error = ingest_traces(&runner, &FailingSource, &mut ledger)
            .await
            .unwrap_err();
        assert!(matches!(source_error, IngestionError::Source(_)));

        let source = FakeSource {
            discovery: TraceDiscovery {
                traces: vec![DiscoveredTrace {
                    source: "trace.json".into(),
                    trace: trace("agent", Utc::now()),
                }],
                ..TraceDiscovery::default()
            },
        };
        ledger.fail_record = true;
        let ledger_error = ingest_traces(&runner, &source, &mut ledger)
            .await
            .unwrap_err();
        assert!(matches!(ledger_error, IngestionError::Ledger(_)));
    }
}
