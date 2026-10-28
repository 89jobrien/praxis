# 6. Auto-rollback on regression detection

Date: 2026-10-28

## Status

Accepted

## Context

The improvement loop applies strategy changes, but some changes make agent
behavior worse. Without rollback, a bad improvement persists and degrades all
subsequent runs. We needed automatic rollback when regression is detected.

## Decision

- Add `auto_rollback: bool` field to `LoopConfig` (defaults to `false`)
- After each improvement cycle, compare the new trace against the previous
  one using `replay_compare()` which returns a `Verdict` (Improved/Regressed/Neutral)
- If `auto_rollback` is enabled and verdict is `Regressed`, roll back the
  strategy to the previous version (`current_version - 1`)
- The feature was originally added in commit `9888b96` but deleted in `dd63531`;
  it has been restored with tests `regression_triggers_rollback` and
  `no_rollback_when_disabled`

## Consequences

- Auto-rollback is opt-in (safe default for CI/demos)
- Rollback is immediate and automatic when enabled
- Only rolls back one version at a time (doesn't cascade)
- The feature requires `Verdict::Regressed` comparison which depends on
  trace metrics being available
