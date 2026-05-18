# praxis

Self-improving agent runtime for the [cruxx](https://github.com/89jobrien/cruxx) agentic DSL.
Closes the loop between execution traces, evaluation, and strategy evolution so agents get better at their job across sessions.

```text
RUN -> TRACE -> EVALUATE -> PROPOSE -> VALIDATE -> APPLY
 ^                                                   |
 +---------------------------------------------------+
```

## What it does

Every time an agent runs, cruxx captures a `Crux<T>` trace — a full causal record of every step, delegation, and speculation. Praxis takes that trace and:

1. **Evaluates** it — extracts metrics (success rate, confidence, error distribution, delegation depth, speculation hit rate) and generates findings
2. **Records** the reward score and computes trend direction over time
3. **Proposes** strategy improvements backed by evidence
4. **Validates** each proposal against a safety policy (auto-approve low-risk, defer high-risk for human approval)
5. **Applies** accepted changes to the agent's strategy
6. **Compares** the new trace against the previous one and detects regressions

If a strategy change causes a regression, it can be rolled back.

## Demo

```
$ cargo xtask demo

praxis -- self-improving agent runtime demo

=== Sequential improvement loop ===

--- session-1: struggling agent ---
  score: 0.28  |  success_rate: 33%  |  avg_confidence: 0.20
  [applied] ConfidenceThreshold -> demo-agent (confidence: 0.70)
  strategy v1: 0 tool prefs, 1 thresholds

--- session-2: partial recovery ---
  score: 0.56  |  success_rate: 67%  |  avg_confidence: 0.40
  [applied] ConfidenceThreshold -> demo-agent (confidence: 0.70)
  ^^ IMPROVED (delta: +0.280)
  strategy v2: 0 tool prefs, 1 thresholds

--- session-3: getting better ---
  score: 0.84  |  success_rate: 100%  |  avg_confidence: 0.60
  ^^ IMPROVED (delta: +0.280)
  strategy v2: 0 tool prefs, 1 thresholds

--- session-4: confident execution ---
  score: 0.93  |  success_rate: 100%  |  avg_confidence: 0.81
  ^^ IMPROVED (delta: +0.085)
  strategy v2: 0 tool prefs, 1 thresholds

--- session-5: regression! ---
  score: 0.35  |  success_rate: 33%  |  avg_confidence: 0.37
  [applied] ConfidenceThreshold -> demo-agent (confidence: 0.70)
  vv REGRESSED (delta: -0.578)
  strategy v3: 0 tool prefs, 1 thresholds

=== Batch evaluation (concurrency: 4) ===

  6 succeeded, 0 failed

  [0] fetch-agent -- score: 0.95
  [1] broken-agent -- score: 0.04
  [2] deploy-agent -- score: 0.93
  [3] review-agent -- score: 0.87
  [4] search-agent -- score: 0.48
  [5] test-agent -- score: 0.95
```

For a narrated version that prints the input trace, findings, active threshold,
and score math for each cycle:

```bash
cargo xtask live-demo
```

### Regression finding

Session 5 is an intentional regression fixture in the demo trace data. The
score comes from `TraceMetrics`:

```text
score = 0.60 * success_rate + 0.40 * avg_confidence
```

Session 4 has four successful steps and high confidence:

- success rate: `100%`
- average confidence: `0.81`
- score: `0.925`

Session 5 then drops to one successful step out of three, with lower
confidence:

- success rate: `33%`
- average confidence: `0.37`
- score: `0.347`

The comparison delta is `0.347 - 0.925 = -0.578`, which crosses the
regression threshold of `-0.05`, so `replay_compare` reports
`vv REGRESSED`. The low score also causes the deterministic planner to apply a
new `ConfidenceThreshold` because the score is below `0.60` and the evaluator
produces findings for low success rate, low confidence, and high error rate.

## Architecture

Hexagonal (ports/adapters). Domain logic as traits, adapters are swappable. The `ImprovementLoop` is thread-safe, cloneable, and supports both sequential and concurrent trace evaluation.

```
praxis/
  crates/
    praxis-core/       port traits (zero async, zero adapters)
      Evaluator           scores a trace, produces findings
      StrategyPlanner     proposes improvements from eval + trend
      StrategyStore       persists strategy with rollback
      RewardAccumulator   records rewards, computes trends

    praxis-eval/       evaluator + planner adapters
      MetricsEvaluator              findings from trace metrics
      StubEvaluator                 neutral scores (testing)
      DeterministicStrategyPlanner  rule-based proposals

    praxis-store/      storage adapters
      InMemoryRewardStore           in-process reward tracking
      FileStrategyStore             JSON file with snapshot history

    praxis/            orchestrator
      ImprovementLoop               sequential + concurrent cycles
      LoopConfig                    concurrency settings
      BatchResult                   aggregated batch outcomes

  xtask/               workspace build tasks
```

### Dependency direction

```
praxis -> cruxx-improve -> cruxx-core, cruxx-types, cruxx-planner
```

Praxis never imports cruxx internals directly. `cruxx-improve` is the single bridge crate providing:

- Re-exports: `Crux<T>`, `Step`, `CruxId`, `SafetyPolicy`, `HarnessDiff`
- `TraceMetrics` — structured extraction from traces (crux-domain knowledge)
- `replay_compare` / `Verdict` / `Comparison` — trace comparison logic
- `ImprovementKind` / `StrategyDiff` / `Strategy` — shared vocabulary
- `StrategyPolicy` — validates strategy changes (extends `SafetyPolicy`)
- `evolution_to_strategy_diff` — bridges crux's `EvolutionPlanner`

## Improvement kinds

| Kind                    | What changes                 | Example                  |
| ----------------------- | ---------------------------- | ------------------------ |
| `Resource`              | Memory, timeout, concurrency | OOM -> bump 50%          |
| `ToolPreference`        | Tool selection order         | "Prefer rg over grep"    |
| `DecompositionStrategy` | Problem breakdown approach   | "Parallelize by crate"   |
| `DelegationPolicy`      | When to delegate vs inline   | "Delegate if >3 steps"   |
| `PromptTemplate`        | System/user prompt wording   | "Add format spec"        |
| `ConfidenceThreshold`   | Routing thresholds           | "Lower speculate to 0.6" |

Low-risk changes (thresholds, tool preferences) are auto-applied.
High-risk changes (prompt patches, delegation rules) are deferred for human approval.

## Safety

Every proposed improvement passes through a `StrategyPolicy` before
application:

- **Validation**: rejects changes that exceed configured limits (e.g., too many simultaneous changes)
- **Approval gating**: prompt patches and delegation rules require explicit approval; threshold tweaks auto-approve
- **Rollback**: `loop.rollback(version)` restores any previous strategy snapshot
- **Regression detection**: `replay_compare` detects when a strategy change makes things worse

## Usage

### Sequential

```rust
use praxis::ImprovementLoop;
use praxis_eval::{MetricsEvaluator, DeterministicStrategyPlanner};
use praxis_store::{InMemoryRewardStore, FileStrategyStore};
use cruxx_improve::DefaultStrategyPolicy;

let loop_runner = ImprovementLoop::new(
    Box::new(MetricsEvaluator),
    Box::new(DeterministicStrategyPlanner::default()),
    Box::new(FileStrategyStore::new("strategy.json".into())),
    Box::new(InMemoryRewardStore::new()),
    Box::new(DefaultStrategyPolicy::default()),
);

// After each agent run, feed the trace:
let result = loop_runner.run_cycle(&trace).await?;

// result.evaluation   -- score + findings + metrics
// result.applied      -- improvements that were auto-applied
// result.deferred     -- improvements needing human approval
// result.comparison   -- verdict vs previous trace (if any)
// result.strategy     -- current strategy state
```

### Concurrent batch

```rust
use praxis::{ImprovementLoop, LoopConfig};

let loop_runner = ImprovementLoop::with_config(
    evaluator, planner, store, rewards, policy,
    LoopConfig { concurrency: 8 },
);

// Evaluate multiple traces concurrently (semaphore-bounded):
let batch = loop_runner.run_batch(&traces).await;

println!("{} succeeded, {} failed", batch.succeeded(), batch.failed());

for result in &batch.results {
    match result {
        Ok(r) => println!("{}: {:.2}", r.evaluation.agent, r.evaluation.score),
        Err(e) => println!("FAILED: {e}"),
    }
}
```

The loop is `Clone` — cloned instances share state (rewards, strategy
store, per-agent comparison tracking). Safe to pass across tasks.

## Building

```bash
cargo xtask ci       # fmt-check + clippy + nextest
cargo xtask test     # cargo nextest run
cargo xtask lint     # cargo clippy --all-targets -- -D warnings
cargo xtask demo     # run the demo
cargo xtask live-demo # run the narrated live demo
cargo xtask build    # cargo build --all-targets
```

Requires Rust 1.85+ (edition 2024).

## Related projects

| Project                                         | Role                                                 |
| ----------------------------------------------- | ---------------------------------------------------- |
| [cruxx](https://github.com/89jobrien/cruxx)     | Agentic DSL: traces, replay, evolution, safety       |
| cruxx-improve                                   | Bridge crate: shared vocabulary + trace metrics      |
| [devloop](https://github.com/89jobrien/devloop) | Council analysis (future `CouncilEvaluator` adapter) |
| [magi](https://github.com/89jobrien/magi)       | Two-tier RL (future `RLEvaluator` adapter)           |
| [braid](https://github.com/89jobrien/braid)     | Agent engine (consumes strategy from praxis)         |

## License

MIT OR Apache-2.0
