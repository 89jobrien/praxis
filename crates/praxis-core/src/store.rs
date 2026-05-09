use cruxx_improve::{Strategy, StrategyDiff};

pub trait StrategyStore: Send + Sync {
    fn current(&self) -> Strategy;
    fn apply(&mut self, diff: &StrategyDiff) -> Strategy;
    fn history(&self) -> Vec<Strategy>;
    fn rollback(&mut self, version: u64);
}
