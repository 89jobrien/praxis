use crate::approval::{ApprovalDecision, ApprovalGate, AutoApproveGate};
use cruxx_improve::{
    Comparison, Crux, Improvement, Strategy, StrategyPolicy, StrategyViolation, replay_compare,
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
    pub rejected: Vec<Improvement>,
    pub strategy: Strategy,
    pub comparison: Option<Comparison>,
}

/// Configuration for the improvement loop.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Max concurrent evaluation cycles in `run_batch`.
    pub concurrency: usize,
    /// If set, export strategy as JSON to this path after each cycle.
    pub export_path: Option<std::path::PathBuf>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            export_path: None,
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
    approval_gate: Arc<dyn ApprovalGate>,
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
            Box::new(AutoApproveGate),
        )
    }

    pub fn with_config(
        evaluator: Box<dyn Evaluator>,
        planner: Box<dyn StrategyPlanner>,
        store: Box<dyn StrategyStore>,
        rewards: Box<dyn RewardAccumulator>,
        policy: Box<dyn StrategyPolicy>,
        config: LoopConfig,
        approval_gate: Box<dyn ApprovalGate>,
    ) -> Self {
        Self {
            evaluator: Arc::from(evaluator),
            planner: Arc::from(planner),
            store: Arc::new(Mutex::new(store)),
            rewards: Arc::new(Mutex::new(rewards)),
            policy: Arc::from(policy),
            approval_gate: Arc::from(approval_gate),
            last_traces: Arc::new(Mutex::new(std::collections::HashMap::new())),
            config,
        }
    }

    /// Replace the approval gate with a custom implementation.
    pub fn with_approval_gate(mut self, gate: Box<dyn ApprovalGate>) -> Self {
        self.approval_gate = Arc::from(gate);
        self
    }

    pub async fn current_strategy(&self) -> Strategy {
        self.store.lock().await.current()
    }

    /// Run a single improvement cycle for one trace.
    pub async fn run_cycle(
        &self,
        trace: &Crux<serde_json::Value>,
    ) -> Result<CycleResult, LoopError> {
        let evaluation = self.evaluator.evaluate(trace).await?;

        {
            let mut rewards = self.rewards.lock().await;
            rewards
                .record(trace.id.clone(), &trace.agent, evaluation.score)
                .await?;
        }

        let trend = {
            let rewards = self.rewards.lock().await;
            rewards.trend(&trace.agent).await?
        };

        let current = { self.store.lock().await.current() };
        let improvements = self.planner.propose(&evaluation, &trend, &current).await?;

        let mut applied = Vec::new();
        let mut deferred = Vec::new();
        let mut rejected = Vec::new();
        let mut strategy = current;

        for improvement in improvements {
            self.policy.validate_strategy(&improvement.diff)?;

            if self.policy.requires_strategy_approval(&improvement.diff) {
                match self.approval_gate.review(&improvement).await {
                    ApprovalDecision::Approved => {
                        let mut store = self.store.lock().await;
                        strategy = store.apply(&improvement.diff);
                        applied.push(improvement);
                    }
                    ApprovalDecision::Rejected => {
                        rejected.push(improvement);
                    }
                    ApprovalDecision::Deferred => {
                        deferred.push(improvement);
                    }
                }
            } else {
                let mut store = self.store.lock().await;
                strategy = store.apply(&improvement.diff);
                applied.push(improvement);
            }
        }

        let comparison = {
            let traces = self.last_traces.lock().await;
            traces
                .get(&trace.agent)
                .map(|old| replay_compare(old, trace))
        };

        {
            let mut traces = self.last_traces.lock().await;
            traces.insert(trace.agent.clone(), trace.clone());
        }

        // Auto-export strategy if configured
        if let Some(ref path) = self.config.export_path {
            crate::strategy_export::export_strategy(&strategy, path).map_err(|e| {
                LoopError::Evaluation(praxis_core::evaluator::EvaluationError::Failed(format!(
                    "strategy export failed: {e}"
                )))
            })?;
        }

        Ok(CycleResult {
            evaluation,
            applied,
            deferred,
            rejected,
            strategy,
            comparison,
        })
    }

    /// Run improvement cycles for multiple traces concurrently.
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
    use crate::approval::ApprovalDecision;
    use async_trait::async_trait;
    use chrono::Utc;
    use cruxx_improve::{CruxId, DefaultStrategyPolicy, Step, StepKind, StepStatus};
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
        runner
            .run_cycle(&make_trace("agent-a", 0.5, StepStatus::Ok))
            .await
            .unwrap();
        let r = runner
            .run_cycle(&make_trace("agent-a", 0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(r.comparison.is_some());
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
            Box::new(AutoApproveGate),
        );
        let traces: Vec<_> = (0..10)
            .map(|i| make_trace(&format!("agent-{i}"), 0.7, StepStatus::Ok))
            .collect();
        let batch = runner.run_batch(&traces).await;
        assert_eq!(batch.succeeded(), 10);
    }

    #[tokio::test]
    async fn auto_export_writes_strategy_file() {
        let dir = TempDir::new().unwrap();
        let export_path = dir.path().join("exported.json");
        let runner = ImprovementLoop::with_config(
            Box::new(StubEvaluator),
            Box::new(DeterministicStrategyPlanner::default()),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
            LoopConfig {
                concurrency: 4,
                export_path: Some(export_path.clone()),
            },
            Box::new(AutoApproveGate),
        );
        let trace = make_trace("test", 0.5, StepStatus::Ok);
        runner.run_cycle(&trace).await.unwrap();
        assert!(export_path.exists());
        let loaded = crate::strategy_export::load_strategy(&export_path).unwrap();
        assert_eq!(loaded.version, 0);
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
        let result = runner2
            .run_cycle(&make_trace("agent-a", 0.8, StepStatus::Ok))
            .await
            .unwrap();
        assert!(result.comparison.is_some());
    }

    struct DeferAllGate;

    #[async_trait]
    impl ApprovalGate for DeferAllGate {
        async fn review(&self, _: &Improvement) -> ApprovalDecision {
            ApprovalDecision::Deferred
        }
    }

    #[tokio::test]
    async fn with_config_accepts_approval_gate() {
        let dir = TempDir::new().unwrap();
        let runner = ImprovementLoop::with_config(
            Box::new(StubEvaluator),
            Box::new(PromptPatchPlanner),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
            LoopConfig::default(),
            Box::new(DeferAllGate),
        );
        let trace = make_trace("test-agent", 0.3, StepStatus::Ok);
        let result = runner.run_cycle(&trace).await.unwrap();
        assert!(result.applied.is_empty());
        assert!(!result.deferred.is_empty());
    }

    struct RejectAllGate;

    #[async_trait]
    impl ApprovalGate for RejectAllGate {
        async fn review(&self, _: &Improvement) -> ApprovalDecision {
            ApprovalDecision::Rejected
        }
    }

    /// Planner that always proposes a prompt_patch, triggering approval.
    struct PromptPatchPlanner;

    #[async_trait]
    impl StrategyPlanner for PromptPatchPlanner {
        async fn propose(
            &self,
            evaluation: &Evaluation,
            _trend: &praxis_core::reward::Trend,
            _current: &Strategy,
        ) -> Result<Vec<Improvement>, praxis_core::strategy::PlannerError> {
            use cruxx_improve::{ImprovementKind, PromptPatch};
            Ok(vec![Improvement {
                id: CruxId::new(),
                kind: ImprovementKind::PromptTemplate,
                target: evaluation.agent.clone(),
                diff: cruxx_improve::StrategyDiff {
                    prompt_patches: vec![PromptPatch {
                        agent: evaluation.agent.clone(),
                        section: "system".into(),
                        content: "be more helpful".into(),
                    }],
                    ..Default::default()
                },
                confidence: 0.8,
                evidence: vec!["test evidence".into()],
                proposed_at: chrono::Utc::now(),
            }])
        }
    }

    /// Helper to build a loop whose improvements require approval and get rejected.
    fn make_rejectable_loop(dir: &TempDir) -> ImprovementLoop {
        ImprovementLoop::new(
            Box::new(StubEvaluator),
            Box::new(PromptPatchPlanner),
            Box::new(FileStrategyStore::new(dir.path().join("s.json"))),
            Box::new(InMemoryRewardStore::new()),
            Box::new(DefaultStrategyPolicy::default()),
        )
        .with_approval_gate(Box::new(RejectAllGate))
    }

    #[tokio::test]
    async fn approval_gate_can_reject() {
        let dir = TempDir::new().unwrap();
        let runner = make_rejectable_loop(&dir);
        let trace = make_trace("test-agent", 0.3, StepStatus::Ok);
        let result = runner.run_cycle(&trace).await.unwrap();
        assert!(result.applied.is_empty());
        assert!(result.deferred.is_empty());
    }

    #[tokio::test]
    async fn rejected_improvements_recorded_in_cycle_result() {
        let dir = TempDir::new().unwrap();
        let runner = make_rejectable_loop(&dir);
        let trace = make_trace("test-agent", 0.3, StepStatus::Ok);
        let result = runner.run_cycle(&trace).await.unwrap();
        assert!(
            !result.rejected.is_empty(),
            "rejected improvements should be recorded, not silently dropped"
        );
        assert!(result.applied.is_empty());
        assert!(result.deferred.is_empty());
    }
}
