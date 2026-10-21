//! Evaluator and strategy-planner adapters for Praxis.

pub mod deterministic;
pub mod metrics_eval;
pub mod stub;

#[cfg(feature = "llm")]
pub mod llm_eval;

pub use deterministic::DeterministicStrategyPlanner;
#[cfg(feature = "llm")]
pub use llm_eval::{LlmEvaluator, LlmEvaluatorConfig};
pub use metrics_eval::MetricsEvaluator;
pub use stub::StubEvaluator;
