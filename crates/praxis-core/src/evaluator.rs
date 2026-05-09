use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cruxx_improve::{Crux, CruxId, TraceMetrics};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub trace_id: CruxId,
    pub agent: String,
    pub score: f32,
    pub findings: Vec<String>,
    pub metrics: TraceMetrics,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("evaluation failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait Evaluator: Send + Sync {
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
