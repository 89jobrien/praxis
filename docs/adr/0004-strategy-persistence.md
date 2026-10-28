# 4. Strategy persistence and versioning

Date: 2026-10-28

## Status

Accepted

## Context

Agent strategies evolve over time through the improvement loop. We need to
persist strategies across runs, track their evolution, and enable rollback to
any previous version. Losing strategy history means losing the agent's learned
behavior.

## Decision

- Strategies are persisted as a JSON array of snapshots, one per version
- `FileStrategyStore` writes the full history on every change (append-only
  semantics — never overwrites earlier versions)
- Each `Strategy` snapshot includes `version`, `tool_preferences`,
  `confidence_thresholds`, `prompt_patches`, `resource_hints`, etc.
- Version numbers are monotonically increasing starting from 0
- `rollback(version)` restores any prior snapshot by discarding newer ones
- A companion ledger file (`<strategy>.ingested.json`) tracks processed
  trace IDs for deduplication across runs

## Consequences

- Full audit trail of strategy evolution is preserved
- Rollback to any prior version is O(1) (just truncate the array)
- The strategy file grows linearly with the number of applied improvements
- File-based storage is simple but not suitable for concurrent writers
- The ledger file uses a clean naming convention (`.ingested.json` extension)
