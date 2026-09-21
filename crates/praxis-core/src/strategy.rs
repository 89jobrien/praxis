//! Strategy planning errors and the planner port.

use async_trait::async_trait;
use crux_improve::{Improvement, Strategy};

use crate::evaluator::Evaluation;
use crate::reward::Trend;

#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("planner error: {0}")]
    Failed(String),
}

#[async_trait]
pub trait StrategyPlanner: Send + Sync {
    /// Proposes improvements from an evaluation, reward trend, and current strategy.
    async fn propose(
        &self,
        evaluation: &Evaluation,
        trend: &Trend,
        current: &Strategy,
    ) -> Result<Vec<Improvement>, PlannerError>;
}
