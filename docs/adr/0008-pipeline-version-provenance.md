# 8. Pipeline version provenance in traces

Date: 2026-10-28

## Status

Accepted

## Context

Crux traces are produced by specific pipeline definitions. Without recording
which pipeline version produced a trace, it's impossible to attribute strategy
improvements to specific pipeline changes or compare traces across versions.

## Decision

- Add `pipeline_version: Option<String>` field to `LoopConfig`
- Propagate the version through all trace construction sites via `make_trace()`
- Store the version in the `Crux` trace's `pipeline_version` field
- Expose a `config()` accessor on `ImprovementLoop` so callers can read the
  configured version
- Default to `None` for backward compatibility

## Consequences

- Traces carry pipeline identity for replay and version comparison
- The version is configurable per `ImprovementLoop` instance
- Existing code that doesn't set the version continues to work
- The `make_trace` helper function now requires an additional parameter,
  which is a breaking change for callers (mitigated by passing `None`)
