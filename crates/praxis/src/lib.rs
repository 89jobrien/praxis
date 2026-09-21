//! Runtime orchestration for trace ingestion and iterative strategy improvement.

pub mod approval;
pub mod ingest;
pub mod loop_runner;
pub mod strategy_export;

pub use approval::{ApprovalDecision, ApprovalGate, AutoApproveGate, CliApprovalGate};
pub use ingest::{IngestionError, IngestionFailure, IngestionReport, ingest_traces};
pub use loop_runner::{BatchResult, CycleResult, ImprovementLoop, LoopConfig, LoopError};
pub use strategy_export::{export_strategy, load_strategy};
