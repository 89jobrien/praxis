//! Domain types and ports for evaluating traces and evolving agent strategies.

// TODO(docs): public API doc coverage is 66%. Document the port traits and the error enums
// here to lift the workspace health score above its threshold. (#14)

pub mod evaluator;
pub mod reward;
pub mod store;
pub mod strategy;
pub mod trace_source;

pub use evaluator::{Evaluation, EvaluationError, Evaluator};
pub use reward::{Reward, RewardAccumulator, RewardError, Trend, TrendDirection};
pub use store::StrategyStore;
pub use strategy::{PlannerError, StrategyPlanner};
pub use trace_source::{
    DiscoveredTrace, IngestionLedger, IngestionLedgerError, RejectedTrace, TraceDiscovery,
    TraceSource, TraceSourceError,
};
