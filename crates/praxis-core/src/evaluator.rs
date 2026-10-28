//! Trace evaluation results, errors, and the evaluator port.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use crux_improve::{Crux, CruxId, TraceMetrics};
use serde::{Deserialize, Serialize};

/// Evaluation result for a single trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    /// The trace that was evaluated.
    pub trace_id: CruxId,
    /// The agent that produced the trace.
    pub agent: String,
    /// Overall quality score (0.0–1.0).
    pub score: f32,
    /// Actionable findings extracted from the trace.
    pub findings: Vec<String>,
    /// Structured metrics extracted from the trace.
    pub metrics: TraceMetrics,
    /// When the evaluation was performed.
    pub evaluated_at: DateTime<Utc>,
}

/// Errors that can occur during trace evaluation.
#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    /// Evaluation failed.
    #[error("evaluation failed: {0}")]
    Failed(String),
}

#[async_trait]
#[allow(clippy::double_must_use)] // `async_trait` adds a redundant `#[must_use]`.
pub trait Evaluator: Send + Sync {
    /// Scores a trace and returns its metrics and actionable findings.
    async fn evaluate(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<Evaluation, EvaluationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_roundtrips_json() {
        let m = TraceMetrics::extract(&Crux {
            id: CruxId::new(),
            agent: "t".into(),
            pipeline_version: None,
            value: Ok(serde_json::json!({})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        });
        let e = Evaluation {
            trace_id: CruxId::new(),
            agent: "test".into(),
            score: 0.75,
            findings: vec!["good".into()],
            metrics: m,
            evaluated_at: Utc::now(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Evaluation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent, "test");
    }
}
