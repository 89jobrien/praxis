# praxis

Self-improving agent runtime. Wires together the port traits from
`praxis-core` with adapters from `praxis-eval` and `praxis-store` into
the `ImprovementLoop` orchestrator.

## ImprovementLoop

The core runtime loop. Each cycle:

1. Evaluates a `Crux<T>` trace via the `Evaluator`
2. Records the reward score via the `RewardAccumulator`
3. Computes the agent's performance trend
4. Asks the `StrategyPlanner` to propose improvements
5. Validates improvements against `StrategyPolicy`
6. Routes improvements through the `ApprovalGate` (approve/reject/defer)
7. Applies accepted diffs to the `StrategyStore`
8. Compares against the previous trace for regression detection

Supports both sequential (`run_cycle`) and concurrent (`run_batch`)
evaluation with configurable concurrency via `LoopConfig`.

## Approval Gates

| Gate              | Behavior                                             |
| ----------------- | ---------------------------------------------------- |
| `AutoApproveGate` | Approves everything (default)                        |
| `CliApprovalGate` | Interactive y/n/d prompt; injectable I/O for testing |

Deferred improvements accumulate in a queue. Call `resubmit_deferred()`
to re-evaluate them after swapping the gate.

## Strategy Export

`export_strategy` / `load_strategy` serialize the current `Strategy` as
JSON for consumption by external systems (e.g., braid). Auto-export is
available via `LoopConfig::export_path`.

## Usage

```rust
use praxis::{ImprovementLoop, LoopConfig, AutoApproveGate};
use praxis_eval::{StubEvaluator, DeterministicStrategyPlanner};
use praxis_store::{FileStrategyStore, InMemoryRewardStore};
use cruxx_improve::DefaultStrategyPolicy;

let runner = ImprovementLoop::with_config(
    Box::new(StubEvaluator),
    Box::new(DeterministicStrategyPlanner::default()),
    Box::new(FileStrategyStore::new("strategy.json".into())),
    Box::new(InMemoryRewardStore::new()),
    Box::new(DefaultStrategyPolicy::default()),
    LoopConfig { concurrency: 4, export_path: None },
    Box::new(AutoApproveGate),
);

// let result = runner.run_cycle(&trace).await?;
// let batch = runner.run_batch(&traces).await;
```
