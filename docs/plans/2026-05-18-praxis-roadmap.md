# Plan: Praxis Roadmap — Six Features

## Goal

Evolve praxis from a demo-ready prototype into a production-capable
self-improving runtime with persistent storage, LLM-backed evaluation,
human approval gating, braid integration, automated rollback, and
comprehensive test coverage.

## Context Map

### Files to Modify

| File                                       | Purpose              | Changes Needed                                |
| ------------------------------------------ | -------------------- | --------------------------------------------- |
| `crates/praxis-store/src/lib.rs`           | Store re-exports     | Add `SqliteRewardStore` export                |
| `crates/praxis-store/src/reward_sqlite.rs` | NEW                  | SQLite-backed `RewardAccumulator`             |
| `crates/praxis-store/Cargo.toml`           | Store deps           | Add `rusqlite`                                |
| `crates/praxis-eval/src/lib.rs`            | Eval re-exports      | Add `LlmEvaluator` export                     |
| `crates/praxis-eval/src/llm_eval.rs`       | NEW                  | LLM-backed `Evaluator` adapter                |
| `crates/praxis-eval/Cargo.toml`            | Eval deps            | Add `reqwest`, `serde_json`                   |
| `crates/praxis-core/src/evaluator.rs`      | Evaluation type      | Add `approval` field                          |
| `crates/praxis/src/loop_runner.rs`         | ImprovementLoop      | Add rollback-on-regression, approval callback |
| `crates/praxis/src/lib.rs`                 | Orchestrator exports | Add `ApprovalGate` re-export                  |
| `crates/praxis/src/approval.rs`            | NEW                  | `ApprovalGate` trait + `CliApprovalGate`      |
| `crates/praxis/src/strategy_export.rs`     | NEW                  | Braid-compatible strategy JSON export         |
| `crates/praxis/Cargo.toml`                 | Orchestrator deps    | Add optional `rusqlite`, `reqwest`            |

### Dependencies (may need updates)

| File                            | Relationship                                     |
| ------------------------------- | ------------------------------------------------ |
| `crates/praxis-core/src/lib.rs` | Re-exports `Evaluation` — additive field is safe |
| `crates/praxis/src/main.rs`     | Demo binary — update to showcase new adapters    |
| `Cargo.toml` (workspace)        | Add `rusqlite`, `reqwest` to workspace deps      |

### Test Coverage

| Test File                                  | Covers                                               |
| ------------------------------------------ | ---------------------------------------------------- |
| `crates/praxis-store/src/reward_memory.rs` | `InMemoryRewardStore` — record, query, trend         |
| `crates/praxis-store/src/strategy_file.rs` | `FileStrategyStore` — roundtrip, rollback, history   |
| `crates/praxis-eval/src/metrics_eval.rs`   | `MetricsEvaluator` — healthy + failing traces        |
| `crates/praxis-eval/src/deterministic.rs`  | `DeterministicStrategyPlanner` — low/high score      |
| `crates/praxis-eval/src/stub.rs`           | `StubEvaluator` — neutral score                      |
| `crates/praxis/src/loop_runner.rs`         | Full cycle, comparison, batch, clone sharing         |
| **GAP**                                    | No test for regression-triggered rollback            |
| **GAP**                                    | No test for deferred improvement approval flow       |
| **GAP**                                    | No integration test exercising all adapters together |

### Reference Patterns

| File                                       | Pattern to Follow               |
| ------------------------------------------ | ------------------------------- |
| `crates/praxis-store/src/reward_memory.rs` | `RewardAccumulator` impl shape  |
| `crates/praxis-eval/src/metrics_eval.rs`   | `Evaluator` impl shape          |
| `crates/praxis-store/src/strategy_file.rs` | File-backed persistence pattern |

### Risk

- [x] `Evaluation` is `pub` + `Serialize/Deserialize` — adding a field
      with `#[serde(default)]` is backwards-compatible
- [ ] `StrategyStore` trait is sync (not async) — SQLite reward store
      only applies to `RewardAccumulator` (which is async), not strategy
- [ ] `ImprovementLoop::run_cycle` is the critical path — rollback logic
      must not break existing tests
- [ ] LLM evaluator introduces network dependency — must be behind a
      feature flag to keep `cargo test` fast

---

## Architecture

- **Crates affected:** `praxis-core` (minor), `praxis-eval` (new adapter),
  `praxis-store` (new adapter), `praxis` (approval gate, rollback, export)
- **New traits/types:**
  - `ApprovalGate` trait in `praxis/src/approval.rs`
  - `SqliteRewardStore` in `praxis-store/src/reward_sqlite.rs`
  - `LlmEvaluator` in `praxis-eval/src/llm_eval.rs`
  - `StrategyExporter` in `praxis/src/strategy_export.rs`
- **Data flow:** Trace -> Evaluator -> RewardAccumulator -> Planner ->
  ApprovalGate -> StrategyStore -> StrategyExporter (braid JSON)

## Tech Stack

- Rust edition 2024, MSRV 1.85
- `rusqlite` (bundled feature) for SQLite reward persistence
- `reqwest` (optional, feature-gated) for LLM API calls
- All new deps behind cargo features to keep default build lean

---

## Phase 1: Persistent Reward Store (SQLite)

### Task 1.1: Add rusqlite workspace dependency

**Crate**: workspace root
**File(s)**: `Cargo.toml`, `crates/praxis-store/Cargo.toml`

1. Add to workspace deps:
   ```toml
   rusqlite = { version = "0.34", features = ["bundled"] }
   ```
2. Add to praxis-store:
   ```toml
   rusqlite = { workspace = true, optional = true }
   ```
3. Add feature:
   ```toml
   [features]
   default = []
   sqlite = ["dep:rusqlite"]
   ```
4. Verify: `cargo check -p praxis-store --features sqlite`
5. Commit: `chore(praxis-store): add rusqlite dependency`

### Task 1.2: Implement SqliteRewardStore

**Crate**: `praxis-store`
**File(s)**: `crates/praxis-store/src/reward_sqlite.rs`
**Run**: `cargo nextest run -p praxis-store --features sqlite`

1. Write failing test:

   ```rust
   #[cfg(feature = "sqlite")]
   mod sqlite_tests {
       use super::*;

       #[tokio::test]
       async fn sqlite_record_and_query() {
           let store = SqliteRewardStore::in_memory().unwrap();
           // ... record, query, assert
       }

       #[tokio::test]
       async fn sqlite_trend_ascending() {
           // ... same pattern as InMemoryRewardStore test
       }

       #[tokio::test]
       async fn sqlite_persists_across_instances() {
           let path = tempfile::NamedTempFile::new().unwrap();
           // store1 records, drop, store2 reads back
       }
   }
   ```

2. Implement `SqliteRewardStore`:
   - `new(path: PathBuf)` — opens/creates DB, runs migration
   - `in_memory()` — `:memory:` for tests
   - Schema: `CREATE TABLE IF NOT EXISTS rewards (
  id INTEGER PRIMARY KEY,
  trace_id TEXT NOT NULL,
  agent TEXT NOT NULL,
  score REAL NOT NULL,
  recorded_at TEXT NOT NULL
)`
   - `RewardAccumulator` impl: INSERT for record, SELECT with
     optional window filter for query, linear regression for trend

3. Verify:

   ```
   cargo nextest run -p praxis-store --features sqlite
   cargo clippy -p praxis-store --features sqlite -- -D warnings
   ```

4. Commit: `feat(praxis-store): add SqliteRewardStore`

### Task 1.3: Export and register SqliteRewardStore

**Crate**: `praxis-store`
**File(s)**: `crates/praxis-store/src/lib.rs`

1. Add:

   ```rust
   #[cfg(feature = "sqlite")]
   pub mod reward_sqlite;
   #[cfg(feature = "sqlite")]
   pub use reward_sqlite::SqliteRewardStore;
   ```

2. Verify: `cargo check -p praxis-store --features sqlite`
3. Commit: `feat(praxis-store): export SqliteRewardStore`

---

## Phase 2: LLM-Backed Evaluator

### Task 2.1: Add reqwest workspace dependency (feature-gated)

**Crate**: workspace root
**File(s)**: `Cargo.toml`, `crates/praxis-eval/Cargo.toml`

1. Add to workspace deps:
   ```toml
   reqwest = { version = "0.12", features = ["json"] }
   ```
2. Add to praxis-eval:

   ```toml
   reqwest = { workspace = true, optional = true }

   [features]
   default = []
   llm = ["dep:reqwest"]
   ```

3. Verify: `cargo check -p praxis-eval --features llm`
4. Commit: `chore(praxis-eval): add reqwest dependency for LLM evaluator`

### Task 2.2: Implement LlmEvaluator

**Crate**: `praxis-eval`
**File(s)**: `crates/praxis-eval/src/llm_eval.rs`
**Run**: `cargo nextest run -p praxis-eval --features llm`

1. Design: `LlmEvaluator` wraps an HTTP client and sends trace
   metrics + step summaries to an LLM API endpoint. Returns
   structured `Evaluation` with LLM-generated findings.

2. Configuration:

   ```rust
   pub struct LlmEvaluatorConfig {
       pub api_url: String,
       pub api_key: String,
       pub model: String,
       pub max_findings: usize,
   }
   ```

3. Implementation:
   - Serialize trace metrics + step names/statuses into a prompt
   - POST to API, parse structured response
   - Fall back to `MetricsEvaluator` on network error (graceful
     degradation)

4. Tests: mock-based (no real API calls in CI):

   ```rust
   #[tokio::test]
   async fn llm_evaluator_falls_back_on_error() {
       // LlmEvaluator with unreachable URL -> returns
       // MetricsEvaluator-equivalent score, no panic
   }
   ```

5. Commit: `feat(praxis-eval): add LlmEvaluator with fallback`

### Task 2.3: Export LlmEvaluator

**Crate**: `praxis-eval`
**File(s)**: `crates/praxis-eval/src/lib.rs`

1. Add:

   ```rust
   #[cfg(feature = "llm")]
   pub mod llm_eval;
   #[cfg(feature = "llm")]
   pub use llm_eval::{LlmEvaluator, LlmEvaluatorConfig};
   ```

2. Verify: `cargo check -p praxis-eval --features llm`
3. Commit: `feat(praxis-eval): export LlmEvaluator`

---

## Phase 3: Human Approval Flow

### Task 3.1: Define ApprovalGate trait

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/approval.rs`
**Run**: `cargo nextest run -p praxis`

1. Define:

   ```rust
   use async_trait::async_trait;
   use cruxx_improve::Improvement;

   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum ApprovalDecision {
       Approved,
       Rejected,
       Deferred,
   }

   #[async_trait]
   pub trait ApprovalGate: Send + Sync {
       async fn review(&self, improvement: &Improvement)
           -> ApprovalDecision;
   }

   /// Auto-approves everything. Used when no human gate is needed.
   pub struct AutoApproveGate;

   #[async_trait]
   impl ApprovalGate for AutoApproveGate {
       async fn review(&self, _: &Improvement) -> ApprovalDecision {
           ApprovalDecision::Approved
       }
   }
   ```

2. Test:

   ```rust
   #[tokio::test]
   async fn auto_approve_always_approves() {
       let gate = AutoApproveGate;
       // ... assert Approved for any improvement
   }
   ```

3. Commit: `feat(praxis): add ApprovalGate trait + AutoApproveGate`

### Task 3.2: Wire ApprovalGate into ImprovementLoop

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/loop_runner.rs`

1. Add `approval_gate: Arc<dyn ApprovalGate>` field to
   `ImprovementLoop`.

2. Update constructors (`new`, `with_config`) to accept an optional
   gate. Default to `AutoApproveGate` to maintain backwards
   compatibility.

3. In `run_cycle`, replace the current `requires_strategy_approval`
   check (lines 150-156) with:

   ```rust
   if self.policy.requires_strategy_approval(&improvement.diff) {
       match self.approval_gate.review(&improvement).await {
           ApprovalDecision::Approved => {
               let mut store = self.store.lock().await;
               strategy = store.apply(&improvement.diff);
               applied.push(improvement);
           }
           ApprovalDecision::Rejected => { /* drop it */ }
           ApprovalDecision::Deferred => {
               deferred.push(improvement);
           }
       }
   }
   ```

4. All existing tests pass unchanged (they use `AutoApproveGate`
   implicitly via `new()`).

5. Add test:

   ```rust
   #[tokio::test]
   async fn approval_gate_can_reject() {
       // RejectAllGate -> applied is empty, deferred is empty
   }
   ```

6. Commit: `feat(praxis): wire ApprovalGate into ImprovementLoop`

### Task 3.3: Add CliApprovalGate (optional)

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/approval.rs`

1. Implement `CliApprovalGate` that prints the improvement to stdout
   and reads y/n/d from stdin.

2. This is not unit-testable (interactive) — document it, test
   `AutoApproveGate` and a mock `RejectAllGate` instead.

3. Commit: `feat(praxis): add CliApprovalGate for interactive use`

---

## Phase 4: Regression Rollback Automation

### Task 4.1: Add auto-rollback to run_cycle

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/loop_runner.rs`
**Run**: `cargo nextest run -p praxis`

1. After the comparison step (line 160-165), check verdict:

   ```rust
   if let Some(ref cmp) = comparison {
       if cmp.verdict == Verdict::Regressed {
           let store = self.store.lock().await;
           let current_version = store.current().version;
           if current_version > 0 {
               drop(store);
               self.rollback(current_version - 1).await;
               // Update strategy in result to reflect rollback
               strategy = self.store.lock().await.current();
           }
       }
   }
   ```

2. Add `auto_rollback: bool` to `LoopConfig` (default `false` for
   backwards compat).

3. Write test:

   ```rust
   #[tokio::test]
   async fn regression_triggers_rollback() {
       // Run good trace -> run bad trace -> assert strategy
       // version went backwards
   }
   ```

4. Verify all existing tests pass.
5. Commit: `feat(praxis): auto-rollback on regression detection`

---

## Phase 5: Braid Integration (Strategy Export)

### Task 5.1: Define strategy export format

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/strategy_export.rs`
**Run**: `cargo nextest run -p praxis`

1. Implement:

   ```rust
   use cruxx_improve::Strategy;
   use std::path::Path;

   /// Exports the current strategy as a JSON file that braid can
   /// consume. The format is the raw `Strategy` struct serialized
   /// as JSON — braid reads `tool_preferences` and
   /// `confidence_thresholds` directly.
   pub fn export_strategy(
       strategy: &Strategy,
       path: &Path,
   ) -> std::io::Result<()> {
       let json = serde_json::to_string_pretty(strategy)
           .expect("strategy serialization");
       std::fs::write(path, json)
   }

   pub fn load_strategy(path: &Path)
       -> std::io::Result<Strategy> {
       let data = std::fs::read_to_string(path)?;
       serde_json::from_str(&data)
           .map_err(|e| std::io::Error::new(
               std::io::ErrorKind::InvalidData, e))
   }
   ```

2. Test roundtrip:

   ```rust
   #[test]
   fn strategy_export_roundtrips() {
       let dir = tempfile::TempDir::new().unwrap();
       let path = dir.path().join("strategy.json");
       let mut s = Strategy::default();
       s.tool_preferences.insert("rg".into(), 5);
       export_strategy(&s, &path).unwrap();
       let loaded = load_strategy(&path).unwrap();
       assert_eq!(loaded.tool_preferences["rg"], 5);
   }
   ```

3. Commit: `feat(praxis): add strategy export for braid consumption`

### Task 5.2: Add export step to ImprovementLoop

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/loop_runner.rs`

1. Add optional `export_path: Option<PathBuf>` to `LoopConfig`.
2. After strategy is updated in `run_cycle`, if `export_path` is
   set, call `export_strategy(&strategy, &path)`.
3. Test: run cycle with export_path -> file exists with valid JSON.
4. Commit: `feat(praxis): auto-export strategy after each cycle`

---

## Phase 6: Test Coverage Expansion

### Task 6.1: Integration test — full loop with all adapters

**Crate**: `praxis`
**File(s)**: `crates/praxis/tests/integration.rs` (NEW)
**Run**: `cargo nextest run -p praxis`

1. Test that wires `MetricsEvaluator` + `DeterministicStrategyPlanner`
   - `FileStrategyStore` + `InMemoryRewardStore` + `AutoApproveGate`
     and runs 3 cycles:
   * Cycle 1: low score -> improvement proposed + applied
   * Cycle 2: higher score -> improvement trend
   * Cycle 3: regression -> rollback (if auto_rollback enabled)

2. Commit: `test(praxis): add integration test for full loop`

### Task 6.2: Edge case tests for existing adapters

**Crate**: `praxis-store`, `praxis-eval`
**File(s)**: existing test modules
**Run**: `cargo nextest run`

1. `InMemoryRewardStore`:
   - Empty query returns empty vec
   - Trend with 1 sample is Stable
   - Window filtering works correctly

2. `FileStrategyStore`:
   - Rollback to nonexistent version is no-op
   - Apply with empty diff increments version

3. `MetricsEvaluator`:
   - Empty trace (no steps) produces neutral score
   - All-error trace produces maximum findings

4. `DeterministicStrategyPlanner`:
   - Declining trend with high score still proposes nothing
   - Exactly-at-threshold score behavior

5. Commit: `test: add edge case coverage for adapters`

### Task 6.3: Loop error handling tests

**Crate**: `praxis`
**File(s)**: `crates/praxis/src/loop_runner.rs` (test module)
**Run**: `cargo nextest run -p praxis`

1. Test with a `FailingEvaluator` that returns `EvaluationError`:

   ```rust
   #[tokio::test]
   async fn cycle_propagates_evaluator_error() {
       // ... assert run_cycle returns Err(LoopError::Evaluation(_))
   }
   ```

2. Test batch with mix of passing and failing traces.

3. Commit: `test(praxis): add error propagation tests`

---

## Dependency Order

```
Phase 1 (SQLite store) ----+
Phase 2 (LLM evaluator) ---+---> Phase 6 (tests use all adapters)
Phase 3 (approval gate) ---+
Phase 4 (rollback) --------+
Phase 5 (braid export) ----+
```

Phases 1-5 are independent of each other and can be parallelized.
Phase 6 depends on all previous phases being complete.

## Estimated Task Count

- Phase 1: 3 tasks
- Phase 2: 3 tasks
- Phase 3: 3 tasks
- Phase 4: 1 task
- Phase 5: 2 tasks
- Phase 6: 3 tasks
- **Total: 15 tasks**
