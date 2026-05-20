use async_trait::async_trait;
use chrono::Utc;
use cruxx_improve::{Crux, StepStatus, TraceMetrics};
use praxis_core::evaluator::{Evaluation, EvaluationError, Evaluator};

const LOW_SUCCESS_RATE_THRESHOLD: f32 = 0.5;
const LOW_CONFIDENCE_THRESHOLD: f32 = 0.4;
const HIGH_ERROR_RATE_THRESHOLD: f32 = 0.3;
const LOW_SPECULATION_HIT_RATE_THRESHOLD: f32 = 0.3;

/// Evaluator that generates findings from trace metrics.
///
/// Produces actionable findings when it detects problems in the trace:
/// low success rate, low confidence, high error count, etc.
pub struct MetricsEvaluator;

#[async_trait]
impl Evaluator for MetricsEvaluator {
    async fn evaluate(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<Evaluation, EvaluationError> {
        let metrics = TraceMetrics::extract(trace);
        let mut findings = Vec::new();

        if metrics.success_rate < LOW_SUCCESS_RATE_THRESHOLD {
            let failed: Vec<&str> = trace
                .steps
                .iter()
                .filter(|s| s.status == StepStatus::Err)
                .map(|s| s.name.as_str())
                .collect();
            findings.push(format!(
                "low success rate ({:.0}%): failing steps: {}",
                metrics.success_rate * 100.0,
                failed.join(", ")
            ));
        }

        if metrics.avg_confidence < LOW_CONFIDENCE_THRESHOLD {
            findings.push(format!(
                "low average confidence ({:.2}): agent is uncertain",
                metrics.avg_confidence
            ));
        }

        if metrics.error_count > 0 && metrics.step_count > 0 {
            let error_rate = metrics.error_count as f32 / metrics.step_count as f32;
            if error_rate > HIGH_ERROR_RATE_THRESHOLD {
                findings.push(format!(
                    "high error rate ({:.0}%): {} errors in {} steps",
                    error_rate * 100.0,
                    metrics.error_count,
                    metrics.step_count
                ));
            }
        }

        if metrics.speculation_count > 0
            && metrics.speculation_hit_rate < LOW_SPECULATION_HIT_RATE_THRESHOLD
        {
            findings.push(format!(
                "speculation hit rate low ({:.0}%): speculative branches mostly fail",
                metrics.speculation_hit_rate * 100.0
            ));
        }

        Ok(Evaluation {
            trace_id: trace.id.clone(),
            agent: trace.agent.clone(),
            score: metrics.score,
            findings,
            metrics,
            evaluated_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_improve::{CruxId, Step, StepKind};

    fn step(name: &str, status: StepStatus, confidence: f32) -> Step {
        Step {
            name: name.into(),
            kind: StepKind::Plain,
            status,
            confidence,
            started_at: Utc::now(),
            duration_ms: 100,
            input_hash: 0,
            content_hash: None,
            output: None,
            error: None,
            attempt: 1,
            events: vec![],
            metadata: Default::default(),
        }
    }

    fn trace(steps: Vec<Step>) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok(serde_json::json!({})),
            steps,
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn healthy_trace_no_findings() {
        let eval = MetricsEvaluator;
        let t = trace(vec![
            step("a", StepStatus::Ok, 0.9),
            step("b", StepStatus::Ok, 0.8),
        ]);
        let result = eval.evaluate(&t).await.unwrap();
        assert!(result.findings.is_empty());
        assert!(result.score > 0.8);
    }

    #[tokio::test]
    async fn failing_trace_produces_findings() {
        let eval = MetricsEvaluator;
        let t = trace(vec![
            step("fetch", StepStatus::Ok, 0.3),
            step("parse", StepStatus::Err, 0.2),
            step("validate", StepStatus::Err, 0.1),
        ]);
        let result = eval.evaluate(&t).await.unwrap();
        assert!(!result.findings.is_empty());
        assert!(result.findings.iter().any(|f| f.contains("success rate")));
        assert!(result.findings.iter().any(|f| f.contains("confidence")));
    }
}
