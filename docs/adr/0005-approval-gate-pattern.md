# 5. Approval gate pattern for human-in-the-loop

Date: 2026-10-28

## Status

Accepted

## Context

The improvement loop proposes strategy changes, some of which are high-risk
(prompt patches, delegation policy changes). Auto-applying these without review
could degrade agent behavior. We needed a way to route high-risk proposals
through human approval while auto-applying low-risk ones.

## Decision

- Define an `ApprovalGate` trait with `review(improvement) -> ApprovalDecision`
- `ApprovalDecision` is `Approved`, `Rejected`, or `Deferred`
- `AutoApproveGate` approves everything (default, for CI/demo)
- `CliApprovalGate` reads from stdin and writes to stdout (interactive)
- `StrategyPolicy::requires_strategy_approval()` determines which improvements
  need approval (prompt patches and delegation rules require it; thresholds
  auto-approve)
- The gate is selectable via `--approval-gate auto|cli` CLI flag

## Consequences

- Human-in-the-loop is opt-in via CLI flag
- The default (auto) is safe for CI and demos
- Deferred improvements accumulate and can be resubmitted with a different gate
- The CLI gate is reachable from the binary despite namespace constraints
  (implemented inline in the binary to avoid Rust module resolution issues)
