# Changelog

All notable changes to Praxis are documented in this file.

## Unreleased

### Features

- Add one-shot ingestion of raw local `Crux<Value>` traces with explicit recursive scan roots.
- Persist processed trace IDs so repeated ingestion runs are idempotent while failures remain retryable.
- Add human approval flow for strategy improvements.
- Add concurrent batch evaluation and task processing.
- Add the metrics evaluator, demo binary, and workspace `xtask` commands.
- Scaffold the Praxis workspace and its core, evaluator, storage, and runtime crates.

### Fixes

- Update stale `cruxx-*` dependency names to current Crux crate names.
- Resolve Clippy and rustfmt findings.

### Refactoring

- Extract improvement-cycle helpers, constants, and safety documentation.
- Split evaluator and store logic and replace magic numbers with named constants.
- Remove the `praxis-task` crate after moving concurrent batching into `ImprovementLoop`.

### Documentation

- Document real trace ingestion, concurrent batch APIs, workspace commands, and handoff state.

### Maintenance

- Add project scaffolding based on workspace conventions.
- Update ignore rules, Rust metadata, and handoff naming.

### Non-Conventional Commits

- `244cd77` Delete target directory (missing conventional commit type and separator).
