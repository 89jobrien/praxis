use cruxx_improve::{
    Comparison, Crux, Improvement, Strategy, StrategyPolicy, StrategyViolation, Verdict,
    replay_compare,
};
use praxis_core::evaluator::{Evaluation, Evaluator};
use praxis_core::reward::RewardAccumulator;
use praxis_core::store::StrategyStore;
use praxis_core::strategy::StrategyPlanner;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

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

/// Configuration for the improvement loop.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Max concurrent evaluation cycles in `run_batch`.
    pub concurrency: usize,
    /// Automatically rollback strategy on regression detection.
    pub auto_rollback: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            auto_rollback: false,
        }
    }
}

/// Batch result: one CycleResult per trace, in submission order.
pub struct BatchResult {
    pub results: Vec<Result<CycleResult, LoopError>>,
}

impl BatchResult {
    pub fn succeeded(&self) -> usize {
        self.results.iter().filter(|r| r.is_ok()).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| r.is_err()).count()
    }
}

/// Self-improving agent runtime loop.
///
/// Thread-safe and cloneable. Supports both sequential (`run_cycle`)
/// and concurrent (`run_batch`) trace evaluation with configurable
/// concurrency.
#[derive(Clone)]
pub struct ImprovementLoop {
    evaluator: Arc<dyn Evaluator>,
    planner: Arc<dyn StrategyPlanner>,
    store: Arc<Mutex<Box<dyn StrategyStore>>>,
    rewards: Arc<Mutex<Box<dyn RewardAccumulator>>>,
    policy: Arc<dyn StrategyPolicy>,
    last_traces: Arc<Mutex<std::collections::HashMap<String, Crux<serde_json::Value>>>>,
    config: LoopConfig,
}

impl ImprovementLoop {
    pub fn new(
        evaluator: Box<dyn Evaluator>,
        planner: Box<dyn StrategyPlanner>,
        store: Box<dyn StrategyStore>,
        rewards: Box<dyn RewardAccumulator>,
        policy: Box<dyn StrategyPolicy>,
    ) -> Self {
        Self::with_config(
            evaluator,
            planner,
            store,
            rewards,
            policy,
            LoopConfig::default(),
        )
    }

    pub fn with_config(
        evaluator: Box<dyn Evaluator>,
        planner: Box<dyn StrategyPlanner>,
        store: Box<dyn StrategyStore>,
        rewards: Box<dyn RewardAccumulator>,
        policy: Box<dyn StrategyPolicy>,
        config: LoopConfig,
    ) -> Self {
        Self {
            evaluator: Arc::from(evaluator),
            planner: Arc::from(planner),
            store: Arc::new(Mutex::new(store)),
            rewards: Arc::new(Mutex::new(rewards)),
            policy: Arc::from(policy),
            last_traces: Arc::new(Mutex::new(std::collections::HashMap::new())),
            config,
        }
    }

    pub async fn current_strategy(&self) -> Strategy {
        self.store.lock().await.current()
    }

    /// Run a single improvement cycle for one trace.
    pub async fn run_cycle(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<CycleResult, LoopError> {
        // 1. Evaluate (stateless, no lock needed)
        let evaluation = self.evaluator.evaluate(trace).await?;

        // 2. Record reward
        {
            let mut rewards = self.rewards.lock().await;
            rewards
                .record(trace.id.clone(), &trace.agent, evaluation.score)
                .await?;
        }

        // 3. Get trend
        let trend = {
            let rewards = self.rewards.lock().await;
            rewards.trend(&trace.agent).await?
        };

        // 4. Propose improvements
        let current = { self.store.lock().await.current() };
        let improvements = self.planner.propose(&evaluation, &trend, &current).await?;

        // 5. Validate and partition: applied vs deferred
        let mut applied = Vec::new();
        let mut deferred = Vec::new();
        let mut strategy = current;

        for improvement in improvements {
            self.policy.validate_strategy(&improvement.diff)?;

            if self.policy.requires_strategy_approval(&improvement.diff) {
                deferred.push(improvement);
            } else {
                let mut store = self.store.lock().await;
                strategy = store.apply(&improvement.diff);
                applied.push(improvement);
            }
        }

        // 6. Compare with previous trace for this agent
        let comparison = {
            let traces = self.last_traces.lock().await;
            traces
                .get(&trace.agent)
                .map(|old| replay_compare(old, trace))
        };

        // 7. Auto-rollback on regression
        if self.config.auto_rollback {
            if let Some(ref cmp) = comparison {
                if cmp.verdict == Verdict::Regressed {
                    let current_version = { self.store.lock().await.current().version };
                    if current_version > 0 {
                        self.store.lock().await.rollback(current_version - 1);
                        strategy = self.store.lock().await.current();
                    }
                }
            }
        }

        // 8. Store for next comparison (per-agent)
        {
            let mut traces = self.last_traces.lock().await;
            traces.insert(trace.agent.clone(), trace.clone());
        }

        Ok(CycleResult {
            evaluation,
            applied,
            deferred,
            strategy,
            comparison,
        })
    }

    /// Run improvement cycles for multiple traces concurrently.
    ///
    /// Concurrency is bounded by `config.concurrency`. Results are
    /// returned in submission order. Each trace is evaluated
    /// independently; strategy updates are serialized through the
    /// shared store lock.
    pub async fn run_batch(&self, traces: &[Crux<serde_json::Value>]) -> BatchResult {
        let semaphore = Arc::new(Semaphore::new(self.config.concurrency));

        let handles: Vec<_> = traces
            .iter()
            .map(|trace| {
                let sem = semaphore.clone();
                let loop_clone = self.clone();
                let trace = trace.clone();

                tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    loop_clone.run_cycle(&trace).await
                })
            })
            .collect();

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(LoopError::Evaluation(
                    praxis_core::evaluator::EvaluationError::Failed(e.to_string()),
                ))),
            }
        }

        BatchResult { results }
    }

    pub async fn rollback(&self, version: u64) {
        self.store.lock().await.rollback(version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cruxx_improve::{CruxId, DefaultStrategyPolicy, Step, StepKind, StepStatus, Verdict};
    use praxis_eval::{DeterministicStrategyPlanner, StubEvaluator};
    use praxis_store::{FileStrategyStore, InMemoryRewardStore};
    use tempfile::TempDir;

    fn make_trace(agent: &str, confidence: f32, status: StepStatus) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: agent.into(),
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

    fn make_loop(dir: &TempDir) -> ImprovementLoop {
        ImprovementLoop::new(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
        )
    }

    #[tokio::test]
    async fn full_cycle_evaluates_and_records() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir);
        let trace = make_trace("test-agent", 0.5, StepStatus::Ok);
        let result = runner.run_cycle(&trace).await.unwrap();
        assert!(result.evaluation.score > 0.0);
        assert!(result.comparison.is_none());
    }

    #[tokio::test]
    async fn second_cycle_produces_comparison() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir);
        runner
            .run_cycle(&make_trace("test-agent", 0.5, StepStatus::Ok))
            .await
            .unwrap();

        let result = runner
            .run_cycle(&make_trace("test-agent", 0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(result.comparison.is_some());
    }

    #[tokio::test]
    async fn per_agent_comparison_tracking() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir);

        // Agent A: two cycles -> comparison on second
        runner
            .run_cycle(&make_trace("agent-a", 0.5, StepStatus::Ok))
            .await
            .unwrap();
        let r = runner
            .run_cycle(&make_trace("agent-a", 0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(r.comparison.is_some());

        // Agent B: first cycle -> no comparison (independent)
        let r = runner
            .run_cycle(&make_trace("agent-b", 0.6, StepStatus::Ok))
            .await
            .unwrap();
        assert!(r.comparison.is_none());
    }

    #[tokio::test]
    async fn batch_processes_all_traces() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir);

        let traces: Vec<_> = (0..5)
            .map(|i| make_trace(&format!("agent-{i}"), 0.7, StepStatus::Ok))
            .collect();

        let batch = runner.run_batch(&traces).await;
        assert_eq!(batch.succeeded(), 5);
        assert_eq!(batch.failed(), 0);
    }

    #[tokio::test]
    async fn batch_respects_concurrency() {
        let dir = TempDir::new().unwrap();
        let runner = ImprovementLoop::with_config(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
            LoopConfig {
                concurrency: 2,
                ..Default::default()
            },
        );

        let traces: Vec<_> = (0..10)
            .map(|i| make_trace(&format!("agent-{i}"), 0.7, StepStatus::Ok))
            .collect();

        let batch = runner.run_batch(&traces).await;
        assert_eq!(batch.succeeded(), 10);
    }

    #[tokio::test]
    async fn regression_triggers_rollback() {
        let dir = TempDir::new().unwrap();
        let runner = ImprovementLoop::with_config(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
            LoopConfig {
                concurrency: 4,
                auto_rollback: true,
            },
        );

        // Good trace first -- establishes baseline strategy
        let good = make_trace("agent", 0.9, StepStatus::Ok);
        let r1 = runner.run_cycle(&good).await.unwrap();
        let v_after_good = r1.strategy.version;

        // Bad trace triggers regression
        let bad = make_trace("agent", 0.1, StepStatus::Err);
        let r2 = runner.run_cycle(&bad).await.unwrap();

        // Should detect regression
        assert!(r2.comparison.is_some());
        assert_eq!(r2.comparison.as_ref().unwrap().verdict, Verdict::Regressed);
        // After rollback, version should be <= v_after_good
        assert!(r2.strategy.version <= v_after_good);
    }

    #[tokio::test]
    async fn no_rollback_when_disabled() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir); // default config: auto_rollback: false

        // Good trace
        let good = make_trace("agent", 0.9, StepStatus::Ok);
        let r1 = runner.run_cycle(&good).await.unwrap();
        let v_after_good = r1.strategy.version;

        // Bad trace
        let bad = make_trace("agent", 0.1, StepStatus::Err);
        let r2 = runner.run_cycle(&bad).await.unwrap();

        // Regression detected but no rollback
        assert!(r2.comparison.is_some());
        assert_eq!(r2.comparison.as_ref().unwrap().verdict, Verdict::Regressed);
        // Version should be >= v_after_good (no rollback happened)
        assert!(r2.strategy.version >= v_after_good);
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let dir = TempDir::new().unwrap();
        let runner = make_loop(&dir);
        let runner2 = runner.clone();

        runner
            .run_cycle(&make_trace("agent-a", 0.5, StepStatus::Ok))
            .await
            .unwrap();

        // Cloned runner sees the same reward history
        let result = runner2
            .run_cycle(&make_trace("agent-a", 0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(result.comparison.is_some());
    }
}
