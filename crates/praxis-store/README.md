# praxis-store

Storage adapters for praxis. Implements the `StrategyStore` and
`RewardAccumulator` port traits from `praxis-core`.

## Adapters

### `InMemoryRewardStore`

In-memory `RewardAccumulator`. Records rewards in a `Vec`, supports
time-windowed queries, and computes trend direction via linear regression
over score history.

### `FileStrategyStore`

File-backed `StrategyStore`. Persists strategy snapshots as a JSON array.
Supports `apply` (appends a new version), `rollback` (truncates to a
target version), and `history` (returns all snapshots). Loads existing
state from disk on construction.

## Usage

```rust
use praxis_store::{InMemoryRewardStore, FileStrategyStore};

let rewards = InMemoryRewardStore::new();
let strategies = FileStrategyStore::new("strategy.json".into());

// Wire into ImprovementLoop via praxis crate
```
