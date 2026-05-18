use async_trait::async_trait;
use chrono::Utc;
use cruxx_improve::Crux;
use praxis_core::evaluator::{Evaluation, EvaluationError, Evaluator};
use serde::{Deserialize, Serialize};

use crate::metrics_eval::MetricsEvaluator;

/// Configuration for the LLM-backed evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEvaluatorConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub max_findings: usize,
}

/// An evaluator that calls an LLM API to score traces and extract findings.
///
/// On any network or parse error, falls back to [`MetricsEvaluator`].
pub struct LlmEvaluator {
    config: LlmEvaluatorConfig,
    client: reqwest::Client,
    fallback: MetricsEvaluator,
}

#[derive(Debug, Deserialize)]
struct LlmResponse {
    score: f32,
    findings: Vec<String>,
}

impl LlmEvaluator {
    pub fn new(config: LlmEvaluatorConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            fallback: MetricsEvaluator,
        }
    }

    pub fn config(&self) -> &LlmEvaluatorConfig {
        &self.config
    }
}

#[async_trait]
impl Evaluator for LlmEvaluator {
    async fn evaluate(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<Evaluation, EvaluationError> {
        let metrics = cruxx_improve::TraceMetrics::extract(trace);

        let step_summaries: Vec<serde_json::Value> = trace
            .steps
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "status": format!("{:?}", s.status),
                    "confidence": s.confidence,
                })
            })
            .collect();

        let prompt = serde_json::json!({
            "model": self.config.model,
            "messages": [{
                "role": "user",
                "content": format!(
                    "Evaluate this agent trace. Return JSON with `score` (0.0-1.0) and \
                     `findings` (list of strings, max {}).\n\nMetrics: {:?}\n\nSteps: {}",
                    self.config.max_findings,
                    metrics,
                    serde_json::to_string_pretty(&step_summaries).unwrap_or_default()
                )
            }]
        });

        let result = self
            .client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&prompt)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let text = resp.text().await.unwrap_or_default();
                match serde_json::from_str::<LlmResponse>(&text) {
                    Ok(llm) => {
                        let findings = llm
                            .findings
                            .into_iter()
                            .take(self.config.max_findings)
                            .collect();
                        Ok(Evaluation {
                            trace_id: trace.id.clone(),
                            agent: trace.agent.clone(),
                            score: llm.score.clamp(0.0, 1.0),
                            findings,
                            metrics,
                            evaluated_at: Utc::now(),
                        })
                    }
                    Err(_) => self.fallback.evaluate(trace).await,
                }
            }
            Err(_) => self.fallback.evaluate(trace).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_improve::{CruxId, Step, StepKind, StepStatus};

    fn test_config() -> LlmEvaluatorConfig {
        LlmEvaluatorConfig {
            api_url: "http://127.0.0.1:1".into(),
            api_key: "test-key".into(),
            model: "test-model".into(),
            max_findings: 5,
        }
    }

    fn test_trace() -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test-agent".into(),
            value: Ok(serde_json::json!({})),
            steps: vec![Step {
                name: "step1".into(),
                kind: StepKind::Plain,
                status: StepStatus::Ok,
                confidence: 0.9,
                started_at: Utc::now(),
                duration_ms: 100,
                input_hash: 0,
                content_hash: None,
                output: None,
                error: None,
                attempt: 1,
                events: vec![],
                metadata: Default::default(),
            }],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn llm_evaluator_falls_back_on_error() {
        let evaluator = LlmEvaluator::new(test_config());
        let trace = test_trace();
        let result = evaluator.evaluate(&trace).await;
        assert!(result.is_ok(), "should fall back gracefully, not error");
        let eval = result.unwrap();
        assert_eq!(eval.agent, "test-agent");
        assert!(eval.score >= 0.0 && eval.score <= 1.0);
    }

    #[test]
    fn llm_evaluator_config_roundtrip() {
        let config = test_config();
        let evaluator = LlmEvaluator::new(config);
        let c = evaluator.config();
        assert_eq!(c.api_url, "http://127.0.0.1:1");
        assert_eq!(c.model, "test-model");
        assert_eq!(c.max_findings, 5);
    }
}
