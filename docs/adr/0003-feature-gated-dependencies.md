# 3. Feature-gated optional dependencies

Date: 2026-10-28

## Status

Accepted

## Context

Praxis has optional capabilities that require heavy external dependencies:

- LLM-backed evaluation needs `reqwest` and `serde`
- SQLite reward storage needs `rusqlite`

Including these as mandatory dependencies would force all users to compile and
link them, even if they only use the deterministic evaluator and in-memory store.

## Decision

Gate optional dependencies behind Cargo features:

- `praxis-eval` has an `llm` feature (default off) that enables `reqwest` and
  `serde` for the `LlmEvaluator` adapter
- `praxis-store` has a `sqlite` feature (default off) that enables `rusqlite`
  for the `SqliteRewardStore` adapter
- Modules are conditionally compiled with `#[cfg(feature = "...")]`
- Re-exports are also gated to avoid exposing unavailable types

## Consequences

- Default builds are lightweight (no HTTP client or SQLite dependency)
- Users opt in to heavy features with `--features llm` or `--features sqlite`
- CI runs tests with both features enabled to catch regressions
- The workspace `Cargo.toml` retains `reqwest` as a workspace dependency for
  convenience, even though it's only used via the feature gate
