use crate::queue::TaskQueue;
use cruxx_improve::StrategyPolicy;
use praxis_core::evaluator::Evaluator;
use praxis_core::reward::RewardAccumulator;
use praxis_core::store::StrategyStore;
use praxis_core::strategy::StrategyPlanner;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// A worker that pulls traces from the queue and runs improvement cycles.
///
/// Multiple workers can run concurrently against the same queue for
/// parallel processing. Each worker owns its own evaluator but shares
/// the queue, reward store, strategy store, and policy.
pub struct Worker {
    queue: TaskQueue,
    evaluator: Arc<dyn Evaluator>,
    planner: Arc<dyn StrategyPlanner>,
    store: Arc<Mutex<Box<dyn StrategyStore>>>,
    rewards: Arc<Mutex<Box<dyn RewardAccumulator>>>,
    policy: Arc<dyn StrategyPolicy>,
}

impl Worker {
    pub fn new(
        queue: TaskQueue,
        evaluator: Arc<dyn Evaluator>,
        planner: Arc<dyn StrategyPlanner>,
        store: Arc<Mutex<Box<dyn StrategyStore>>>,
        rewards: Arc<Mutex<Box<dyn RewardAccumulator>>>,
        policy: Arc<dyn StrategyPolicy>,
    ) -> Self {
        Self {
            queue,
            evaluator,
            planner,
            store,
            rewards,
            policy,
        }
    }

    /// Spawn N workers processing from the shared queue.
    /// Returns join handles for all workers.
    pub fn spawn(self, concurrency: usize) -> Vec<JoinHandle<()>> {
        (0..concurrency)
            .map(|_| {
                let queue = self.queue.clone();
                let evaluator = self.evaluator.clone();
                let planner = self.planner.clone();
                let store = self.store.clone();
                let rewards = self.rewards.clone();
                let policy = self.policy.clone();

                tokio::spawn(async move {
                    while let Some(submission) = queue.recv().await {
                        let task_id = submission.task_id;
                        queue.mark_running(&task_id).await;

                        // 1. Evaluate
                        let evaluation = match evaluator.evaluate(&submission.trace).await {
                            Ok(e) => e,
                            Err(e) => {
                                queue.mark_failed(&task_id, e.to_string()).await;
                                continue;
                            }
                        };

                        let score = evaluation.score;

                        // 2. Record reward
                        {
                            let mut rewards = rewards.lock().await;
                            if let Err(e) = rewards
                                .record(submission.trace.id.clone(), &submission.trace.agent, score)
                                .await
                            {
                                queue.mark_failed(&task_id, e.to_string()).await;
                                continue;
                            }
                        }

                        // 3. Get trend + propose
                        let trend = {
                            let rewards = rewards.lock().await;
                            match rewards.trend(&submission.trace.agent).await {
                                Ok(t) => t,
                                Err(e) => {
                                    queue.mark_failed(&task_id, e.to_string()).await;
                                    continue;
                                }
                            }
                        };

                        let current = {
                            let store = store.lock().await;
                            store.current()
                        };

                        let improvements =
                            match planner.propose(&evaluation, &trend, &current).await {
                                Ok(i) => i,
                                Err(e) => {
                                    queue.mark_failed(&task_id, e.to_string()).await;
                                    continue;
                                }
                            };

                        // 4. Validate and apply
                        {
                            let mut store = store.lock().await;
                            for improvement in &improvements {
                                if policy.validate_strategy(&improvement.diff).is_err() {
                                    continue;
                                }
                                if !policy.requires_strategy_approval(&improvement.diff) {
                                    store.apply(&improvement.diff);
                                }
                            }
                        }

                        queue.mark_done(&task_id, score).await;
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::TaskQueue;
    use crate::status::TaskStatus;
    use async_trait::async_trait;
    use chrono::Utc;
    use cruxx_improve::{
        Crux, CruxId, DefaultStrategyPolicy, Step, StepKind, StepStatus, Strategy, StrategyDiff,
        TraceMetrics,
    };
    use praxis_core::evaluator::{Evaluation, EvaluationError};
    use praxis_core::reward::{Reward, RewardError, Trend, TrendDirection};
    use praxis_core::store::StrategyStore;
    use praxis_core::strategy::PlannerError;

    // Inline test doubles to avoid depending on praxis-eval/praxis-store

    struct TestEvaluator;

    #[async_trait]
    impl Evaluator for TestEvaluator {
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

    struct TestPlanner;

    #[async_trait]
    impl StrategyPlanner for TestPlanner {
        async fn propose(
            &self,
            _evaluation: &Evaluation,
            _trend: &Trend,
            _current: &Strategy,
        ) -> Result<Vec<cruxx_improve::Improvement>, PlannerError> {
            Ok(vec![])
        }
    }

    struct TestRewardStore {
        rewards: Vec<Reward>,
    }

    impl TestRewardStore {
        fn new() -> Self {
            Self { rewards: vec![] }
        }
    }

    #[async_trait]
    impl RewardAccumulator for TestRewardStore {
        async fn record(
            &mut self,
            trace_id: CruxId,
            agent: &str,
            score: f32,
        ) -> Result<(), RewardError> {
            self.rewards.push(Reward {
                trace_id,
                agent: agent.into(),
                score,
                recorded_at: Utc::now(),
            });
            Ok(())
        }

        async fn query(
            &self,
            _agent: &str,
            _window: Option<chrono::Duration>,
        ) -> Result<Vec<Reward>, RewardError> {
            Ok(self.rewards.clone())
        }

        async fn trend(&self, agent: &str) -> Result<Trend, RewardError> {
            Ok(Trend {
                agent: agent.into(),
                direction: TrendDirection::Stable,
                slope: 0.0,
                sample_count: self.rewards.len(),
            })
        }
    }

    struct TestStrategyStore;

    impl StrategyStore for TestStrategyStore {
        fn current(&self) -> Strategy {
            Strategy::default()
        }
        fn apply(&mut self, _diff: &StrategyDiff) -> Strategy {
            Strategy::default()
        }
        fn history(&self) -> Vec<Strategy> {
            vec![Strategy::default()]
        }
        fn rollback(&mut self, _version: u64) {}
    }

    fn dummy_trace(agent: &str) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: agent.into(),
            value: Ok(serde_json::json!({})),
            steps: vec![Step {
                name: "s".into(),
                kind: StepKind::Plain,
                status: StepStatus::Ok,
                confidence: 0.8,
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
    async fn worker_processes_submissions() {
        let queue = TaskQueue::new(10);
        let id1 = queue.submit("a", dummy_trace("a")).await.unwrap();
        let id2 = queue.submit("b", dummy_trace("b")).await.unwrap();

        let worker = Worker::new(
            queue.clone(),
            Arc::new(TestEvaluator),
            Arc::new(TestPlanner),
            Arc::new(Mutex::new(
                Box::new(TestStrategyStore) as Box<dyn StrategyStore>
            )),
            Arc::new(Mutex::new(
                Box::new(TestRewardStore::new()) as Box<dyn RewardAccumulator>
            )),
            Arc::new(DefaultStrategyPolicy::default()),
        );

        let _handles = worker.spawn(2);

        // Give workers time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let r1 = queue.get(&id1).await.unwrap();
        let r2 = queue.get(&id2).await.unwrap();
        assert_eq!(r1.status, TaskStatus::Done);
        assert_eq!(r2.status, TaskStatus::Done);
        assert!(r1.score.is_some());
        assert!(r2.score.is_some());
    }

    #[tokio::test]
    async fn concurrent_workers_share_queue() {
        let queue = TaskQueue::new(100);

        // Submit 10 tasks
        let mut ids = Vec::new();
        for i in 0..10 {
            let id = queue
                .submit(format!("agent-{i}"), dummy_trace(&format!("agent-{i}")))
                .await
                .unwrap();
            ids.push(id);
        }

        let worker = Worker::new(
            queue.clone(),
            Arc::new(TestEvaluator),
            Arc::new(TestPlanner),
            Arc::new(Mutex::new(
                Box::new(TestStrategyStore) as Box<dyn StrategyStore>
            )),
            Arc::new(Mutex::new(
                Box::new(TestRewardStore::new()) as Box<dyn RewardAccumulator>
            )),
            Arc::new(DefaultStrategyPolicy::default()),
        );

        let _handles = worker.spawn(3);

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let stats = queue.stats().await;
        assert_eq!(stats.done, 10);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.running, 0);
    }
}
