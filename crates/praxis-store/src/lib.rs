//! File and in-memory adapters for Praxis persistence ports.

// TODO(feature): `reward_sqlite.rs` (263 lines) is not in the module tree, so it is never
// compiled, linted or tested. Add a `sqlite` feature gate with `dep:rusqlite` and declare the
// module. (#8)

pub mod ingestion_ledger_file;
pub mod reward_memory;
pub mod strategy_file;
pub mod trace_file;

pub use ingestion_ledger_file::FileIngestionLedger;
pub use reward_memory::InMemoryRewardStore;
pub use strategy_file::FileStrategyStore;
pub use trace_file::FileTraceSource;
