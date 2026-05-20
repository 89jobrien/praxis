# praxis-eval

Evaluator and planner adapters for praxis. Implements the `Evaluator` and
`StrategyPlanner` port traits from `praxis-core`.

## Adapters

### `StubEvaluator`

Extracts `TraceMetrics` from the trace and returns the metrics score directly.
Produces no findings. Useful for testing and as a baseline evaluator.

### `MetricsEvaluator`

Generates actionable findings by inspecting trace metrics:

- Low success rate (< 50%) -- lists failing step names
- Low average confidence (< 0.4)
- High error rate (> 30%)
- Low speculation hit rate (< 30%)

### `DeterministicStrategyPlanner`

Rule-based planner with configurable thresholds. Proposes
`ConfidenceThreshold` improvements when the evaluation score is below
`low_score_threshold` (default 0.5) and the agent has findings.
Proposes nothing when performance is adequate.

## Usage

```rust
use praxis_eval::{MetricsEvaluator, DeterministicStrategyPlanner};

let evaluator = MetricsEvaluator;
let planner = DeterministicStrategyPlanner::default();

// Wire into ImprovementLoop via praxis crate
```
