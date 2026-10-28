# 2. Hexagonal architecture with ports and adapters

Date: 2026-10-28

## Status

Accepted

## Context

Praxis needs to evaluate traces, store strategies, and record rewards. Each of these
capabilities has multiple possible implementations (in-memory vs file-backed, stub
vs LLM-backed, etc.). We needed a way to swap implementations without changing
the improvement loop logic.

## Decision

Use hexagonal (ports-and-adapters) architecture. Core domain crates define trait
ports; adapter crates implement them:

- `praxis-core`: defines `Evaluator`, `StrategyStore`, `RewardAccumulator`,
  `StrategyPlanner`, `TraceSource`, `IngestionLedger` traits
- `praxis-store`: implements `StrategyStore` (`FileStrategyStore`), `RewardAccumulator`
  (`InMemoryRewardStore`, `SqliteRewardStore`), `TraceSource` (`FileTraceSource`),
  `IngestionLedger` (`FileIngestionLedger`)
- `praxis-eval`: implements `Evaluator` (`StubEvaluator`, `MetricsEvaluator`,
  `LlmEvaluator`) and `StrategyPlanner` (`DeterministicStrategyPlanner`)
- `praxis`: orchestrates via `ImprovementLoop`, injecting adapters through trait
  objects

## Consequences

- The improvement loop is decoupled from storage and evaluation implementations
- Tests use in-memory adapters for speed and isolation
- New adapters (e.g., SQLite rewards, LLM evaluator) can be added without
  changing the loop logic
- Dependency direction is inward: adapters depend on core, never the reverse
