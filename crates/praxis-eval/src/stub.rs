use async_trait::async_trait;
use chrono::Utc;
use cruxx_improve::{Crux, TraceMetrics};
use praxis_core::evaluator::{Evaluation, EvaluationError, Evaluator};

pub struct StubEvaluator;

#[async_trait]
impl Evaluator for StubEvaluator {
    async fn evaluate(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<Evaluation, EvaluationError> {
        let metrics = TraceMetrics::extract(trace);
        Ok(Evaluation {
            trace_id: trace.id.clone(),
            agent: trace.agent.clone(),
            score: metrics.score,
            findings: vec![],
            metrics,
            evaluated_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_improve::CruxId;

    #[tokio::test]
    async fn stub_returns_metrics_score() {
        let eval = StubEvaluator;
        let trace = Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok(serde_json::json!({})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        };
        let result = eval.evaluate(&trace).await.unwrap();
        assert!((result.score - 0.5).abs() < f32::EPSILON);
        assert!(result.findings.is_empty());
    }
}
