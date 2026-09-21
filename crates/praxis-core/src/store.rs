//! Persistence port for versioned agent strategies.

use crux_improve::{Strategy, StrategyDiff};

pub trait StrategyStore: Send + Sync {
    /// Returns the latest strategy snapshot.
    fn current(&self) -> Strategy;
    /// Applies a diff and stores the resulting strategy snapshot.
    fn apply(&mut self, diff: &StrategyDiff) -> Strategy;
    /// Returns all stored strategy snapshots in version order.
    fn history(&self) -> Vec<Strategy>;
    /// Discards snapshots newer than the requested version.
    fn rollback(&mut self, version: u64);
}
