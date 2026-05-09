use async_trait::async_trait;
use chrono::Utc;
use cruxx_improve::{CruxId, Improvement, ImprovementKind, Strategy, StrategyDiff};
use praxis_core::evaluator::Evaluation;
use praxis_core::reward::Trend;
use praxis_core::strategy::{PlannerError, StrategyPlanner};

#[derive(Debug, Clone)]
pub struct DeterministicStrategyPlanner {
    pub low_score_threshold: f32,
    pub improvement_confidence: f32,
}

impl Default for DeterministicStrategyPlanner {
    fn default() -> Self {
        Self {
            low_score_threshold: 0.5,
            improvement_confidence: 0.6,
        }
    }
}

#[async_trait]
impl StrategyPlanner for DeterministicStrategyPlanner {
    async fn propose(
        &self,
        evaluation: &Evaluation,
        trend: &Trend,
        _current: &Strategy,
    ) -> Result<Vec<Improvement>, PlannerError> {
        let mut improvements = Vec::new();

        if evaluation.score >= self.low_score_threshold && trend.slope >= 0.0 {
            return Ok(improvements);
        }

        if evaluation.score < self.low_score_threshold && !evaluation.findings.is_empty() {
            improvements.push(Improvement {
                id: CruxId::new(),
                kind: ImprovementKind::ConfidenceThreshold,
                target: evaluation.agent.clone(),
                diff: StrategyDiff {
                    confidence_thresholds: vec![(
                        "speculate_threshold".into(),
                        (evaluation.score + 0.1).min(1.0),
                    )],
                    ..Default::default()
                },
                confidence: self.improvement_confidence,
                evidence: evaluation.findings.clone(),
                proposed_at: Utc::now(),
            });
        }

        Ok(improvements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_improve::TraceMetrics;
    use praxis_core::reward::TrendDirection;

    fn dummy_metrics(score: f32) -> TraceMetrics {
        TraceMetrics {
            step_count: 5,
            success_rate: score,
            error_count: 0,
            avg_confidence: score,
            total_duration_ms: 500,
            delegation_count: 0,
            delegation_depth: 0,
            speculation_count: 0,
            speculation_hit_count: 0,
            speculation_hit_rate: 0.0,
            score,
        }
    }

    #[tokio::test]
    async fn low_score_proposes_improvements() {
        let planner = DeterministicStrategyPlanner::default();
        let eval = Evaluation {
            trace_id: CruxId::new(),
            agent: "test".into(),
            score: 0.3,
            findings: vec!["failures".into()],
            metrics: dummy_metrics(0.3),
            evaluated_at: Utc::now(),
        };
        let trend = Trend {
            agent: "test".into(),
            direction: TrendDirection::Declining,
            slope: -0.05,
            sample_count: 10,
        };
        let imps = planner
            .propose(&eval, &trend, &Strategy::default())
            .await
            .unwrap();
        assert!(!imps.is_empty());
    }

    #[tokio::test]
    async fn high_score_proposes_nothing() {
        let planner = DeterministicStrategyPlanner::default();
        let eval = Evaluation {
            trace_id: CruxId::new(),
            agent: "test".into(),
            score: 0.95,
            findings: vec![],
            metrics: dummy_metrics(0.95),
            evaluated_at: Utc::now(),
        };
        let trend = Trend {
            agent: "test".into(),
            direction: TrendDirection::Improving,
            slope: 0.02,
            sample_count: 10,
        };
        let imps = planner
            .propose(&eval, &trend, &Strategy::default())
            .await
            .unwrap();
        assert!(imps.is_empty());
    }
}
