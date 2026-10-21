//! File and in-memory adapters for Praxis persistence ports.

pub mod ingestion_ledger_file;
pub mod reward_memory;
pub mod strategy_file;
pub mod trace_file;

#[cfg(feature = "sqlite")]
pub mod reward_sqlite;

pub use ingestion_ledger_file::FileIngestionLedger;
pub use reward_memory::InMemoryRewardStore;
#[cfg(feature = "sqlite")]
pub use reward_sqlite::SqliteRewardStore;
pub use strategy_file::FileStrategyStore;
pub use trace_file::FileTraceSource;
