//! Evaluator and strategy-planner adapters for Praxis.

// TODO(feature): `llm_eval.rs` (196 lines) is not in the module tree, so it is never compiled,
// linted or tested. Add an `llm` feature gate with `dep:reqwest` and declare the module. (#7)

pub mod deterministic;
pub mod metrics_eval;
pub mod stub;

pub use deterministic::DeterministicStrategyPlanner;
pub use metrics_eval::MetricsEvaluator;
pub use stub::StubEvaluator;
