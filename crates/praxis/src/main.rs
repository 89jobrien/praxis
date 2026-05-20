use chrono::Utc;
use cruxx_improve::{Crux, CruxId, DefaultStrategyPolicy, Step, StepKind, StepStatus, Verdict};
use praxis::{AutoApproveGate, ImprovementLoop, LoopConfig};
use praxis_core::StrategyStore as _;
use praxis_eval::{DeterministicStrategyPlanner, MetricsEvaluator};
use praxis_store::{FileStrategyStore, InMemoryRewardStore};
use std::{env, path::PathBuf};

fn make_trace(agent: &str, steps: Vec<(&str, StepStatus, f32)>) -> Crux<serde_json::Value> {
    Crux {
        id: CruxId::new(),
        agent: agent.into(),
        value: Ok(serde_json::json!({"status": "complete"})),
        steps: steps
            .into_iter()
            .map(|(name, status, confidence)| Step {
                name: name.into(),
                kind: StepKind::Plain,
                status,
                confidence,
                started_at: Utc::now(),
                duration_ms: 150,
                input_hash: 0,
                content_hash: None,
                output: None,
                error: None,
                attempt: 1,
                events: vec![],
                metadata: Default::default(),
            })
            .collect(),
        children: vec![],
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoMode {
    Standard,
    Live,
}

#[tokio::main]
async fn main() {
    let mode = demo_mode_from_args();

    match mode {
        DemoMode::Standard => println!("praxis -- self-improving agent runtime demo\n"),
        DemoMode::Live => {
            println!("praxis -- live self-improving agent runtime demo\n");
            print_live_intro();
        }
    }

    let strategy_path = PathBuf::from("/tmp/praxis-demo-strategy.json");
    let _ = std::fs::remove_file(&strategy_path);

    let loop_runner = ImprovementLoop::with_config(
        Box::new(MetricsEvaluator),
        Box::new(DeterministicStrategyPlanner {
            low_score_threshold: 0.6,
            improvement_confidence: 0.7,
        }),
        Box::new(FileStrategyStore::new(strategy_path.clone())),
        Box::new(InMemoryRewardStore::new()),
        Box::new(DefaultStrategyPolicy::default()),
        LoopConfig {
            concurrency: 4,
            ..Default::default()
        },
        Box::new(AutoApproveGate),
    );

    // === Sequential demo: 5 sessions with improving traces ===
    println!("=== Sequential improvement loop ===\n");

    let sessions = vec![
        (
            "session-1: struggling agent",
            vec![
                ("fetch-data", StepStatus::Ok, 0.3),
                ("parse-response", StepStatus::Err, 0.2),
                ("retry-parse", StepStatus::Err, 0.1),
            ],
        ),
        (
            "session-2: partial recovery",
            vec![
                ("fetch-data", StepStatus::Ok, 0.5),
                ("parse-response", StepStatus::Ok, 0.4),
                ("validate", StepStatus::Err, 0.3),
            ],
        ),
        (
            "session-3: getting better",
            vec![
                ("fetch-data", StepStatus::Ok, 0.7),
                ("parse-response", StepStatus::Ok, 0.6),
                ("validate", StepStatus::Ok, 0.5),
            ],
        ),
        (
            "session-4: confident execution",
            vec![
                ("fetch-data", StepStatus::Ok, 0.9),
                ("parse-response", StepStatus::Ok, 0.8),
                ("validate", StepStatus::Ok, 0.7),
                ("deploy", StepStatus::Ok, 0.85),
            ],
        ),
        (
            "session-5: regression!",
            vec![
                ("fetch-data", StepStatus::Ok, 0.6),
                ("parse-response", StepStatus::Err, 0.3),
                ("validate", StepStatus::Err, 0.2),
            ],
        ),
    ];

    for (label, steps) in sessions {
        println!("--- {label} ---");
        if mode == DemoMode::Live {
            print_live_trace_input(&steps);
        }

        let trace = make_trace("demo-agent", steps);
        let result = loop_runner.run_cycle(&trace).await.unwrap();

        print_result(&result);
        if mode == DemoMode::Live {
            print_live_analysis(&result);
        }
        println!();
    }

    // === Batch demo: 6 agents evaluated concurrently ===
    println!("=== Batch evaluation (concurrency: 4) ===\n");

    let traces: Vec<_> = vec![
        make_trace(
            "fetch-agent",
            vec![
                ("http-get", StepStatus::Ok, 0.9),
                ("parse-json", StepStatus::Ok, 0.85),
            ],
        ),
        make_trace(
            "broken-agent",
            vec![
                ("init", StepStatus::Err, 0.1),
                ("retry", StepStatus::Err, 0.1),
            ],
        ),
        make_trace(
            "deploy-agent",
            vec![
                ("build", StepStatus::Ok, 0.8),
                ("test", StepStatus::Ok, 0.75),
                ("push", StepStatus::Ok, 0.9),
            ],
        ),
        make_trace(
            "review-agent",
            vec![
                ("diff", StepStatus::Ok, 0.7),
                ("analyze", StepStatus::Ok, 0.65),
            ],
        ),
        make_trace(
            "search-agent",
            vec![
                ("index", StepStatus::Ok, 0.6),
                ("query", StepStatus::Err, 0.3),
            ],
        ),
        make_trace(
            "test-agent",
            vec![
                ("discover", StepStatus::Ok, 0.85),
                ("run", StepStatus::Ok, 0.9),
                ("report", StepStatus::Ok, 0.88),
            ],
        ),
    ];

    let batch = loop_runner.run_batch(&traces).await;
    println!(
        "  {} succeeded, {} failed\n",
        batch.succeeded(),
        batch.failed()
    );

    for (i, result) in batch.results.iter().enumerate() {
        match result {
            Ok(r) => {
                println!(
                    "  [{}] {} -- score: {:.2}",
                    i, r.evaluation.agent, r.evaluation.score
                );
            }
            Err(e) => {
                println!("  [{}] FAILED: {}", i, e);
            }
        }
    }

    // === Strategy history ===
    println!("\n--- strategy history ---");
    let store = FileStrategyStore::new(strategy_path);
    for s in store.history() {
        println!(
            "  v{}: {} tool_prefs, {} thresholds",
            s.version,
            s.tool_preferences.len(),
            s.confidence_thresholds.len(),
        );
    }
}

fn demo_mode_from_args() -> DemoMode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => DemoMode::Standard,
        Some("live") | Some("live-demo") | Some("--live") => DemoMode::Live,
        Some(arg) => {
            eprintln!("unknown demo argument: {arg}");
            eprintln!("usage: praxis [live-demo]");
            std::process::exit(2);
        }
    }
}

fn print_result(result: &praxis::CycleResult) {
    println!(
        "  score: {:.2}  |  success_rate: {:.0}%  |  avg_confidence: {:.2}",
        result.evaluation.score,
        result.evaluation.metrics.success_rate * 100.0,
        result.evaluation.metrics.avg_confidence,
    );

    for imp in &result.applied {
        println!(
            "  [applied] {:?} -> {} (confidence: {:.2})",
            imp.kind, imp.target, imp.confidence,
        );
    }
    for imp in &result.deferred {
        println!(
            "  [deferred - needs approval] {:?} -> {}",
            imp.kind, imp.target,
        );
    }

    if let Some(cmp) = &result.comparison {
        let arrow = match cmp.verdict {
            Verdict::Improved => "^^ IMPROVED",
            Verdict::Regressed => "vv REGRESSED",
            Verdict::Neutral => "== NEUTRAL",
        };
        println!("  {arrow} (delta: {:+.3})", cmp.delta);
    }

    println!(
        "  strategy v{}: {} tool prefs, {} thresholds",
        result.strategy.version,
        result.strategy.tool_preferences.len(),
        result.strategy.confidence_thresholds.len(),
    );
}

fn print_live_intro() {
    println!("Live scoring model:");
    println!("  score = 0.60 * success_rate + 0.40 * avg_confidence");
    println!("  verdict = improved/regressed when score delta crosses +/-0.05");
    println!("  planner applies a ConfidenceThreshold when score < 0.60 and findings exist\n");
}

fn print_live_trace_input(steps: &[(&str, StepStatus, f32)]) {
    println!("  input trace:");
    for (name, status, confidence) in steps {
        println!(
            "    - {name}: {} (confidence {:.2})",
            status_label(*status),
            confidence
        );
    }
}

fn print_live_analysis(result: &praxis::CycleResult) {
    if !result.evaluation.findings.is_empty() {
        println!("  findings:");
        for finding in &result.evaluation.findings {
            println!("    - {finding}");
        }
    }

    if let Some(threshold) = result
        .strategy
        .confidence_thresholds
        .get("speculate_threshold")
    {
        println!("  active threshold: speculate_threshold={threshold:.2}");
    }

    if let Some(cmp) = &result.comparison {
        println!(
            "  score math: {:.3} -> {:.3} ({:+.3})",
            cmp.old_metrics.score, cmp.new_metrics.score, cmp.delta
        );
        println!(
            "  success_rate: {:.0}% -> {:.0}% | avg_confidence: {:.2} -> {:.2}",
            cmp.old_metrics.success_rate * 100.0,
            cmp.new_metrics.success_rate * 100.0,
            cmp.old_metrics.avg_confidence,
            cmp.new_metrics.avg_confidence
        );

        if cmp.delta < -0.05 {
            println!(
                "  regression cause: the new trace has fewer successful steps and lower confidence than the previous trace"
            );
        }
    }
}

fn status_label(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Ok => "ok",
        StepStatus::Err => "err",
        StepStatus::Rejected => "rejected",
        StepStatus::Skipped => "skipped",
    }
}
