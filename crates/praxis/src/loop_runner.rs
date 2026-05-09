use cruxx_improve::{Comparison, Crux, StrategyPolicy, StrategyViolation, replay_compare};
use cruxx_improve::{Improvement, Strategy};
use praxis_core::evaluator::{Evaluation, Evaluator};
use praxis_core::reward::RewardAccumulator;
use praxis_core::store::StrategyStore;
use praxis_core::strategy::StrategyPlanner;

#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    #[error("evaluation failed: {0}")]
    Evaluation(#[from] praxis_core::evaluator::EvaluationError),
    #[error("planner failed: {0}")]
    Planner(#[from] praxis_core::strategy::PlannerError),
    #[error("reward store failed: {0}")]
    Reward(#[from] praxis_core::reward::RewardError),
    #[error("strategy validation failed: {0}")]
    Policy(#[from] StrategyViolation),
}

pub struct CycleResult {
    pub evaluation: Evaluation,
    pub applied: Vec<Improvement>,
    pub deferred: Vec<Improvement>,
    pub strategy: Strategy,
    pub comparison: Option<Comparison>,
}

pub struct ImprovementLoop {
    evaluator: Box<dyn Evaluator>,
    planner: Box<dyn StrategyPlanner>,
    store: Box<dyn StrategyStore>,
    rewards: Box<dyn RewardAccumulator>,
    policy: Box<dyn StrategyPolicy>,
    last_trace: Option<Crux<serde_json::Value>>,
}

impl ImprovementLoop {
    pub fn new(
        evaluator: Box<dyn Evaluator>,
        planner: Box<dyn StrategyPlanner>,
        store: Box<dyn StrategyStore>,
        rewards: Box<dyn RewardAccumulator>,
        policy: Box<dyn StrategyPolicy>,
    ) -> Self {
        Self {
            evaluator,
            planner,
            store,
            rewards,
            policy,
            last_trace: None,
        }
    }

    pub fn current_strategy(&self) -> Strategy {
        self.store.current()
    }

    pub async fn run_cycle(
        &mut self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<CycleResult, LoopError> {
        // 1. Evaluate
        let evaluation = self.evaluator.evaluate(trace).await?;

        // 2. Record reward
        self.rewards
            .record(trace.id.clone(), &trace.agent, evaluation.score)
            .await?;

        // 3. Get trend
        let trend = self.rewards.trend(&trace.agent).await?;

        // 4. Propose improvements
        let current = self.store.current();
        let improvements = self.planner.propose(&evaluation, &trend, &current).await?;

        // 5. Validate and partition: applied vs deferred (needs approval)
        let mut applied = Vec::new();
        let mut deferred = Vec::new();
        let mut strategy = current;

        for improvement in improvements {
            self.policy.validate_strategy(&improvement.diff)?;

            if self.policy.requires_strategy_approval(&improvement.diff) {
                deferred.push(improvement);
            } else {
                strategy = self.store.apply(&improvement.diff);
                applied.push(improvement);
            }
        }

        // 6. Compare with previous trace
        let comparison = self
            .last_trace
            .as_ref()
            .map(|old| replay_compare(old, trace));

        // 7. Store for next comparison
        self.last_trace = Some(trace.clone());

        Ok(CycleResult {
            evaluation,
            applied,
            deferred,
            strategy,
            comparison,
        })
    }

    pub fn rollback(&mut self, version: u64) {
        self.store.rollback(version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cruxx_improve::{CruxId, DefaultStrategyPolicy, Step, StepKind, StepStatus};
    use praxis_eval::{DeterministicStrategyPlanner, StubEvaluator};
    use praxis_store::{FileStrategyStore, InMemoryRewardStore};
    use tempfile::TempDir;

    fn make_trace(confidence: f32, status: StepStatus) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test-agent".into(),
            value: Ok(serde_json::json!({})),
            steps: vec![Step {
                name: "step-1".into(),
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
            }],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn full_cycle_evaluates_and_records() {
        let dir = TempDir::new().unwrap();
        let mut runner = ImprovementLoop::new(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
        );
        let trace = make_trace(0.5, StepStatus::Ok);
        let result = runner.run_cycle(&trace).await.unwrap();
        assert!(result.evaluation.score > 0.0);
        assert!(result.comparison.is_none());
    }

    #[tokio::test]
    async fn second_cycle_produces_comparison() {
        let dir = TempDir::new().unwrap();
        let mut runner = ImprovementLoop::new(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
        );
        runner
            .run_cycle(&make_trace(0.5, StepStatus::Ok))
            .await
            .unwrap();

        let result = runner
            .run_cycle(&make_trace(0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(result.comparison.is_some());
    }
}
