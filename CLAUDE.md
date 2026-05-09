# CLAUDE.md

This file provides guidance to Claude Code when working with the praxis
codebase.

## Overview

Praxis is a self-improving agent runtime that closes the loop between
execution traces, evaluation, and strategy evolution. It depends on
`cruxx-improve` from the crux workspace as its single bridge dependency.

## Build Commands

```bash
just ci                    # Full gate: fmt + clippy + nextest
just test                  # cargo nextest run
just lint                  # cargo clippy --all-targets -- -D warnings
just fmt                   # cargo fmt --all
just build                 # cargo build --all-targets
cargo nextest run -p praxis-core   # Test a single crate
```

Always use `cargo nextest run` instead of `cargo test`.

## Workspace Structure

```
crates/
  praxis-core/     # Port traits (Evaluator, StrategyPlanner, StrategyStore,
                   # RewardAccumulator). Zero adapters, zero async runtime.
  praxis-eval/     # Evaluator + planner adapters (StubEvaluator,
                   # DeterministicStrategyPlanner)
  praxis-store/    # Storage adapters (InMemoryRewardStore, FileStrategyStore)
  praxis/          # ImprovementLoop orchestrator
```

## Architecture

Hexagonal (ports/adapters). All domain logic in praxis-core as traits.
Adapters in praxis-eval and praxis-store. The orchestrator in praxis
wires them together.

### Dependency Direction

```
praxis -> cruxx-improve -> cruxx-core, cruxx-types, cruxx-planner
```

Praxis never imports cruxx-core, cruxx-types, or cruxx-planner directly.
`cruxx-improve` is the single entry point for all crux types.

### Key Types (from cruxx-improve)

- `Crux<T>` — execution trace fused with result
- `TraceMetrics` — extracted metrics (success rate, confidence, depth)
- `Comparison` / `Verdict` — trace comparison results
- `ImprovementKind` / `StrategyDiff` / `Strategy` — improvement protocol
- `StrategyPolicy` — validates strategy changes
- `SafetyPolicy` — validates harness changes

### Key Traits (praxis-core)

- `Evaluator` — scores a trace, produces findings
- `StrategyPlanner` — proposes improvements from evaluation + trend
- `StrategyStore` — persists strategy snapshots with rollback
- `RewardAccumulator` — records and queries reward history with trends

### The Improvement Loop

```
Session N:
  Agent runs -> Crux<T> trace
  -> Evaluator scores trace
  -> RewardAccumulator records reward
  -> StrategyPlanner proposes improvements
  -> StrategyPolicy validates (auto-approve or defer)
  -> StrategyStore applies accepted diffs

Session N+1:
  Agent runs with updated strategy
  -> replay_compare(old_trace, new_trace)
  -> Regression? rollback + negative reward
  -> Improvement? positive reward, reinforce
```

## Rust Conventions

- Edition 2024, MSRV 1.85
- `unsafe_code = "deny"` (inherited from crux patterns)
- Clippy pedantic with `-D warnings`
- All tests via `cargo nextest run`
