# praxis-core

Port traits for the praxis self-improving runtime. This crate defines
the domain interfaces with zero adapters and zero async runtime opinion.

## Traits

| Trait               | Purpose                                                                           |
| ------------------- | --------------------------------------------------------------------------------- |
| `Evaluator`         | Scores a `Crux<T>` trace, produces an `Evaluation` with findings                  |
| `StrategyPlanner`   | Proposes `Improvement`s from an evaluation, trend, and current strategy           |
| `StrategyStore`     | Persists strategy snapshots with `apply`, `history`, and `rollback`               |
| `RewardAccumulator` | Records per-agent reward scores and computes `Trend` (improving/declining/stable) |

## Key Types

- **`Evaluation`** -- trace ID, agent name, score, findings list, extracted `TraceMetrics`
- **`Reward`** -- trace ID, agent, score, timestamp
- **`Trend`** -- agent, direction (`Improving`/`Declining`/`Stable`), slope, sample count

## Dependency

All crux types come through `cruxx-improve` (the single bridge dependency).
This crate never imports `cruxx-core`, `cruxx-types`, or `cruxx-planner` directly.

## Usage

Implement the traits in adapter crates (see `praxis-eval` and `praxis-store`),
then wire them together in the `praxis` orchestrator crate.

```rust
use praxis_core::{Evaluator, Evaluation, EvaluationError};

struct MyEvaluator;

#[async_trait::async_trait]
impl Evaluator for MyEvaluator {
    async fn evaluate(
        &self,
        trace: &cruxx_improve::Crux<serde_json::Value>,
    ) -> Result<Evaluation, EvaluationError> {
        // your evaluation logic
        todo!()
    }
}
```
