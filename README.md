# praxis

Self-improving agent runtime for the [cruxx](https://github.com/89jobrien/cruxx)
agentic DSL. Closes the loop between execution traces, evaluation, and
strategy evolution — agents get better at their job across sessions.

```
RUN -> TRACE -> EVALUATE -> PROPOSE -> VALIDATE -> APPLY
 ^                                                   |
 +---------------------------------------------------+
```

## What it does

Every time an agent runs, cruxx captures a `Crux<T>` trace — a full
causal record of every step, delegation, and speculation. Praxis takes
that trace and:

1. **Evaluates** it — extracts metrics (success rate, confidence,
   error distribution, delegation depth, speculation hit rate) and
   generates findings
2. **Records** the reward score and computes trend direction over time
3. **Proposes** strategy improvements backed by evidence
4. **Validates** each proposal against a safety policy (auto-approve
   low-risk, defer high-risk for human approval)
5. **Applies** accepted changes to the agent's strategy
6. **Compares** the new trace against the previous one and detects
   regressions

If a strategy change causes a regression, it can be rolled back.

## Demo

```
$ cargo run -p praxis

praxis -- self-improving agent runtime demo

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
```

## Architecture

Hexagonal (ports/adapters). Domain logic as traits, adapters are
swappable.

```
praxis
  |
  +-- praxis-core       port traits (zero async, zero adapters)
  |     Evaluator           scores a trace, produces findings
  |     StrategyPlanner     proposes improvements from eval + trend
  |     StrategyStore       persists strategy with rollback
  |     RewardAccumulator   records rewards, computes trends
  |
  +-- praxis-eval        evaluator + planner adapters
  |     MetricsEvaluator           generates findings from trace metrics
  |     StubEvaluator              returns neutral scores (testing)
  |     DeterministicStrategyPlanner   rule-based improvement proposals
  |
  +-- praxis-store       storage adapters
  |     InMemoryRewardStore        in-process reward tracking
  |     FileStrategyStore          JSON file with snapshot history
  |
  +-- praxis             orchestrator
        ImprovementLoop            wires everything together
```

### Dependency direction

```
praxis -> cruxx-improve -> cruxx-core, cruxx-types, cruxx-planner
```

Praxis never imports cruxx internals directly. `cruxx-improve` is the
single bridge crate providing:

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
High-risk changes (prompt patches, delegation rules) are deferred for
human approval.

## Safety

Every proposed improvement passes through a `StrategyPolicy` before
application:

- **Validation**: rejects changes that exceed configured limits
  (e.g., too many simultaneous changes)
- **Approval gating**: prompt patches and delegation rules require
  explicit approval; threshold tweaks auto-approve
- **Rollback**: `loop.rollback(version)` restores any previous
  strategy snapshot
- **Regression detection**: `replay_compare` detects when a strategy
  change makes things worse

## Usage

```rust
use praxis::ImprovementLoop;
use praxis_eval::{MetricsEvaluator, DeterministicStrategyPlanner};
use praxis_store::{InMemoryRewardStore, FileStrategyStore};
use cruxx_improve::DefaultStrategyPolicy;

let mut loop_runner = ImprovementLoop::new(
    Box::new(MetricsEvaluator),
    Box::new(DeterministicStrategyPlanner::default()),
    Box::new(FileStrategyStore::new("strategy.json".into())),
    Box::new(InMemoryRewardStore::new()),
    Box::new(DefaultStrategyPolicy::default()),
);

// After each agent run, feed the trace:
let result = loop_runner.run_cycle(&trace).await?;

// result.evaluation   — score + findings + metrics
// result.applied      — improvements that were auto-applied
// result.deferred     — improvements needing human approval
// result.comparison   — verdict vs previous trace (if any)
// result.strategy     — current strategy state
```

## Building

```bash
just ci        # fmt + clippy + nextest
just test      # cargo nextest run
just lint      # cargo clippy --all-targets -- -D warnings
cargo run      # run demo
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
