use async_trait::async_trait;
use cruxx_improve::{Improvement, Strategy};

use crate::evaluator::Evaluation;
use crate::reward::Trend;

#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("planner error: {0}")]
    Failed(String),
}

#[async_trait]
pub trait StrategyPlanner: Send + Sync {
    async fn propose(
        &self,
        evaluation: &Evaluation,
        trend: &Trend,
        current: &Strategy,
    ) -> Result<Vec<Improvement>, PlannerError>;
}
