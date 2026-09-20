# Design: Local Crux Trace Ingestion

## Goal

Add a real one-shot Praxis command that discovers raw `Crux<Value>` JSON traces under explicit local paths, evaluates each new trace, and persists strategy evolution without reprocessing prior trace IDs.

## Approved Approach

Use schema-first recursive discovery over explicit roots, persistent trace-ID deduplication, and an explicit strategy-history destination.

## Context Map

### Files To Modify

| File                                               | Purpose                                 | Changes Needed                                                        |
| -------------------------------------------------- | --------------------------------------- | --------------------------------------------------------------------- |
| `crates/praxis-core/src/trace_source.rs`           | New ingestion ports and discovery types | Define trace source and ingestion ledger contracts                    |
| `crates/praxis-core/src/lib.rs`                    | Core public exports                     | Re-export the new ports and types                                     |
| `crates/praxis-store/src/trace_file.rs`            | New filesystem trace adapter            | Recursively discover and deserialize raw Crux JSON                    |
| `crates/praxis-store/src/ingestion_ledger_file.rs` | New file ledger adapter                 | Persist processed trace IDs atomically                                |
| `crates/praxis-store/src/strategy_file.rs`         | Strategy-history persistence            | Add a fallible, validating constructor for production use             |
| `crates/praxis-store/src/lib.rs`                   | Store public exports                    | Re-export both new adapters                                           |
| `crates/praxis/src/ingest.rs`                      | New ingestion application service       | Order, deduplicate, evaluate, and report traces                       |
| `crates/praxis/src/lib.rs`                         | Runtime public exports                  | Re-export ingestion service types and function                        |
| `crates/praxis/src/main.rs`                        | Binary entry point                      | Add `praxis ingest --strategy <file> <root>...` while retaining demos |
| `README.md`                                        | User documentation                      | Document one-shot ingestion, accepted trace format, and sidecar state |

### Dependencies

| File                                                      | Relationship                                                               |
| --------------------------------------------------------- | -------------------------------------------------------------------------- |
| `crates/praxis/src/loop_runner.rs`                        | `ingest_traces` composes the existing `ImprovementLoop::run_cycle` API     |
| `crates/praxis-eval/src/metrics_eval.rs`                  | The CLI uses the existing deterministic metrics evaluator                  |
| `crates/praxis-eval/src/deterministic.rs`                 | The CLI uses the existing deterministic strategy planner                   |
| `crates/praxis-store/src/reward_memory.rs`                | The initial command continues to use the existing in-memory reward adapter |
| `/Users/joe/dev/crux/crates/crux-types/src/crux_value.rs` | Defines the raw serde shape deserialized as `Crux<Value>`                  |
| `/Users/joe/dev/crux/crates/crux-cli/src/bin/crux/run.rs` | Produces compatible raw JSON when `crux run --save-trace` is used          |

### Test Coverage

| Test Location                                      | Coverage                                                                                      |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `crates/praxis/src/main.rs`                        | Existing custom CLI parsing for demo and help modes                                           |
| `crates/praxis/src/loop_runner.rs`                 | Existing cycle evaluation, comparison, strategy application, and error behavior               |
| `crates/praxis-store/src/strategy_file.rs`         | Existing strategy persistence and rollback behavior                                           |
| `crates/praxis-store/src/trace_file.rs`            | New temporary-directory tests for recursion, schema filtering, size limits, and symlinks      |
| `crates/praxis-store/src/ingestion_ledger_file.rs` | New persistence, reopen, duplicate, malformed-state, and atomic-write tests                   |
| `crates/praxis/src/ingest.rs`                      | New fake-port tests for ordering, current-run duplicates, prior-run IDs, retries, and reports |

### Reference Patterns

| File                                       | Pattern To Follow                                      |
| ------------------------------------------ | ------------------------------------------------------ |
| `crates/praxis-core/src/evaluator.rs`      | Port trait, domain result, and typed error conventions |
| `crates/praxis-store/src/strategy_file.rs` | JSON-backed adapter shape                              |
| `crates/praxis/src/strategy_export.rs`     | Serde-to-I/O error conversion                          |
| `crates/praxis/src/main.rs`                | Existing argument parsing and inline parser tests      |

### Risk

- The new core APIs are additive and do not break existing consumers.
- The ingest command adds a CLI surface but preserves the current no-argument and `live-demo` behavior.
- Raw serde traces are accepted; presentation JSON from `Crux::to_trace_json` is not replayable and is rejected.
- `crates/praxis/src/main.rs` currently has uncommitted changes, so implementation must preserve that work.
- Reward trend history remains process-local in this version; persistent rewards are out of scope.

## Crate Ownership

- **`praxis-core`** owns the `TraceSource` and `IngestionLedger` ports plus transport-neutral discovery records.
- **`praxis-store`** owns recursive filesystem discovery and JSON ledger persistence as I/O adapters.
- **`praxis`** owns ingestion orchestration and the user-facing command because it already composes evaluators, planners, stores, policy, and approval gates.

No new crate is required. The design affects exactly the three existing architectural layers needed for the source, adapters, and application service.

## Public API

### Traits

```rust
pub trait TraceSource: Send + Sync {
    fn discover(&self) -> Result<TraceDiscovery, TraceSourceError>;
}

pub trait IngestionLedger: Send + Sync {
    fn contains(&self, trace_id: &CruxId) -> bool;

    fn record(&mut self, trace_id: &CruxId) -> Result<(), IngestionLedgerError>;
}
```

### Core Types

```rust
#[derive(Debug, Clone)]
pub struct DiscoveredTrace {
    pub source: String,
    pub trace: Crux<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedTrace {
    pub source: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct TraceDiscovery {
    pub scanned_files: usize,
    pub ignored_files: usize,
    pub traces: Vec<DiscoveredTrace>,
    pub rejected: Vec<RejectedTrace>,
}

#[derive(Debug, thiserror::Error)]
pub enum TraceSourceError {
    #[error("trace discovery failed: {0}")]
    Discovery(String),
}

#[derive(Debug, thiserror::Error)]
pub enum IngestionLedgerError {
    #[error("ingestion ledger error: {0}")]
    Store(String),
}
```

### Store Adapter Types

```rust
#[derive(Debug, Clone)]
pub struct FileTraceSource {
    roots: Vec<PathBuf>,
    max_file_bytes: u64,
}

impl FileTraceSource {
    pub fn new(roots: Vec<PathBuf>) -> Self;

    #[must_use]
    pub fn with_max_file_bytes(self, max_file_bytes: u64) -> Self;
}

#[derive(Debug)]
pub struct FileIngestionLedger {
    path: PathBuf,
    trace_ids: HashSet<String>,
}

impl FileIngestionLedger {
    pub fn open(path: PathBuf) -> Result<Self, IngestionLedgerError>;
}

impl FileStrategyStore {
    pub fn open(path: PathBuf) -> std::io::Result<Self>;
}
```

The default file-size limit is 16 MiB. `FileTraceSource` does not follow symbolic links. `FileIngestionLedger` serializes a versioned document containing a sorted trace-ID list and replaces the destination atomically.

### Ingestion Service Types

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionFailure {
    pub source: String,
    pub trace_id: CruxId,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct IngestionReport {
    pub scanned_files: usize,
    pub ignored_files: usize,
    pub discovered_traces: usize,
    pub ingested_traces: usize,
    pub previously_ingested: usize,
    pub duplicate_ids: usize,
    pub rejected: Vec<RejectedTrace>,
    pub failures: Vec<IngestionFailure>,
}

#[derive(Debug, thiserror::Error)]
pub enum IngestionError {
    #[error(transparent)]
    Source(#[from] TraceSourceError),

    #[error(transparent)]
    Ledger(#[from] IngestionLedgerError),
}
```

### Functions

```rust
pub async fn ingest_traces(
    runner: &ImprovementLoop,
    source: &dyn TraceSource,
    ledger: &mut dyn IngestionLedger,
) -> Result<IngestionReport, IngestionError>;
```

## Data Flow

1. The CLI validates at least one explicit root plus a writable strategy and ledger destination.
2. `FileTraceSource` recursively enumerates `.json` files without following symlinks.
3. The adapter ignores unrelated JSON, rejects recognizable but invalid Crux documents, and deserializes raw documents as `Crux<Value>`.
4. `ingest_traces` sorts valid traces by `started_at` and then `CruxId`, removes duplicate IDs, and skips IDs already present in `IngestionLedger`.
5. Each eligible trace is passed sequentially to `ImprovementLoop::run_cycle`.
6. Successful cycles persist the trace ID in `FileIngestionLedger`; failed cycles remain eligible for retry.
7. `FileStrategyStore` persists any accepted strategy changes, and the CLI prints the final `IngestionReport`.

## CLI Contract

```text
praxis ingest --strategy <file> <root>...
```

- The ledger path is `<strategy-file>.ingested.json`.
- Missing roots, inaccessible roots, malformed existing strategy or ledger state, and unwritable destinations fail before ingestion.
- A scan containing no valid raw Crux traces exits nonzero.
- A scan containing only previously ingested valid traces is a successful no-op.
- Individual malformed candidates and `run_cycle` failures are reported without aborting remaining traces.
- Existing `praxis`, `praxis live-demo`, and help behavior remain available.

## Hexagonal Boundaries

- **Ports:** `TraceSource` and `IngestionLedger` in `praxis-core::trace_source`.
- **Adapters:** `FileTraceSource` and `FileIngestionLedger` in `praxis-store`.
- **Application service:** `ingest_traces` in `praxis::ingest`.
- **Composition root:** the `praxis` binary constructs the adapters and existing improvement-loop components.

## Out Of Scope

- Continuous filesystem watching or daemon mode.
- Implicit scanning of `$HOME`, `$HOME/dev`, `/tmp`, or the current directory.
- Changes to Crux trace producers or `crux run --save-trace`.
- Conversion of presentation-format traces into raw replayable traces.
- Persistent reward/trend history across Praxis processes.
- LLM-backed evaluation, interactive approval, remote traces, archives, or compressed files.

## Verification

- Run focused nextest suites for `praxis-core`, `praxis-store`, and `praxis`.
- Run `cargo xtask ci` for formatting, Clippy with denied warnings, and all workspace tests.
- Run the command against a temporary directory containing the Crux showcase fixture, unrelated JSON, malformed candidates, and duplicate IDs.
- Re-run the command with the same strategy path and verify it reports a successful no-op.

## Design Risk Checklist

- [x] Breaking API changes: no; all public additions are additive.
- [x] New external dependency: no; recursion, JSON, and persistence use existing standard-library and workspace dependencies.
- [x] Feature flag required: no; ingestion is a normal CLI capability.
- [x] Serialization change: a new versioned ledger format is introduced without altering existing strategy JSON.
- [x] Circular dependencies: none; dependency direction remains `praxis -> praxis-store -> praxis-core`.
