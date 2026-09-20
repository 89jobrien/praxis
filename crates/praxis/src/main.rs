use crux_improve::{DefaultStrategyPolicy, StepStatus};
use praxis::{AutoApproveGate, ImprovementLoop, IngestionReport, LoopConfig, ingest_traces};
use praxis_core::StrategyStore as _;
use praxis_eval::{DeterministicStrategyPlanner, MetricsEvaluator};
use praxis_store::{FileIngestionLedger, FileStrategyStore, FileTraceSource, InMemoryRewardStore};
use std::{
    env,
    path::{Path, PathBuf},
};

const LOW_SCORE_THRESHOLD: f32 = 0.6;
const IMPROVEMENT_CONFIDENCE: f32 = 0.7;
const BATCH_CONCURRENCY: usize = 4;

mod demo {
    use chrono::Utc;
    use crux_improve::{Crux, CruxId, Step, StepKind, StepStatus, Verdict};

    const REGRESSION_DELTA_THRESHOLD: f32 = -0.05;

    pub(crate) fn make_trace(
        agent: &str,
        steps: Vec<(&str, StepStatus, f32)>,
    ) -> Crux<serde_json::Value> {
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
                    findings: vec![],
                })
                .collect(),
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    pub(crate) fn print_result(result: &praxis::CycleResult) {
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

    pub(crate) fn print_live_intro() {
        println!("Live scoring model:");
        println!("  score = 0.60 * success_rate + 0.40 * avg_confidence");
        println!("  verdict = improved/regressed when score delta crosses +/-0.05");
        println!(
            "  planner applies a ConfidenceThreshold when score < 0.60 \
             and findings exist\n"
        );
    }

    pub(crate) fn print_live_trace_input(steps: &[(&str, StepStatus, f32)]) {
        println!("  input trace:");
        for (name, status, confidence) in steps {
            println!(
                "    - {name}: {} (confidence {:.2})",
                status_label(*status),
                confidence
            );
        }
    }

    pub(crate) fn print_live_analysis(result: &praxis::CycleResult) {
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

            if cmp.delta < REGRESSION_DELTA_THRESHOLD {
                println!(
                    "  regression cause: the new trace has fewer successful \
                     steps and lower confidence than the previous trace"
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoMode {
    Standard,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IngestArgs {
    strategy_path: PathBuf,
    roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    Run(DemoMode),
    Ingest(IngestArgs),
    Help,
}

#[tokio::main]
async fn main() {
    let action = match parse_demo_args(env::args().skip(1)) {
        Ok(action) => action,
        Err(message) => {
            eprintln!("{message}");
            eprintln!();
            print_usage();
            std::process::exit(2);
        }
    };

    match action {
        CliAction::Run(mode) => run_demo(mode).await,
        CliAction::Ingest(args) => match run_ingest(args).await {
            Ok(report) => {
                print_ingestion_report(&report);
                if ingestion_exit_failed(&report) {
                    std::process::exit(1);
                }
            }
            Err(message) => {
                eprintln!("praxis ingest failed: {message}");
                std::process::exit(1);
            }
        },
        CliAction::Help => {
            print_usage();
        }
    }
}

async fn run_demo(mode: DemoMode) {
    match mode {
        DemoMode::Standard => println!("praxis -- self-improving agent runtime demo\n"),
        DemoMode::Live => {
            println!("praxis -- live self-improving agent runtime demo\n");
            demo::print_live_intro();
        }
    }

    let strategy_path = PathBuf::from("/tmp/praxis-demo-strategy.json");
    let _ = std::fs::remove_file(&strategy_path);

    let loop_runner = ImprovementLoop::with_config(
        Box::new(MetricsEvaluator),
        Box::new(DeterministicStrategyPlanner {
            low_score_threshold: LOW_SCORE_THRESHOLD,
            improvement_confidence: IMPROVEMENT_CONFIDENCE,
        }),
        Box::new(FileStrategyStore::new(strategy_path.clone())),
        Box::new(InMemoryRewardStore::new()),
        Box::new(DefaultStrategyPolicy::default()),
        LoopConfig {
            concurrency: BATCH_CONCURRENCY,
            ..Default::default()
        },
        Box::new(AutoApproveGate),
    );

    run_sequential_demo(&loop_runner, mode).await;
    run_batch_demo(&loop_runner).await;
    print_strategy_history(&strategy_path);
}

async fn run_ingest(args: IngestArgs) -> Result<IngestionReport, String> {
    let ledger_path = ingestion_ledger_path(&args.strategy_path);
    let store = FileStrategyStore::open(args.strategy_path).map_err(|error| error.to_string())?;
    let mut ledger = FileIngestionLedger::open(ledger_path).map_err(|error| error.to_string())?;
    let source = FileTraceSource::new(args.roots);
    let loop_runner = ImprovementLoop::with_config(
        Box::new(MetricsEvaluator),
        Box::new(DeterministicStrategyPlanner {
            low_score_threshold: LOW_SCORE_THRESHOLD,
            improvement_confidence: IMPROVEMENT_CONFIDENCE,
        }),
        Box::new(store),
        Box::new(InMemoryRewardStore::new()),
        Box::new(DefaultStrategyPolicy::default()),
        LoopConfig {
            concurrency: BATCH_CONCURRENCY,
            ..Default::default()
        },
        Box::new(AutoApproveGate),
    );

    ingest_traces(&loop_runner, &source, &mut ledger)
        .await
        .map_err(|error| error.to_string())
}

fn ingestion_ledger_path(strategy_path: &Path) -> PathBuf {
    let mut path = strategy_path.as_os_str().to_os_string();
    path.push(".ingested.json");
    PathBuf::from(path)
}

fn print_ingestion_report(report: &IngestionReport) {
    for rejected in &report.rejected {
        eprintln!("[rejected] {}: {}", rejected.source, rejected.reason);
    }
    for failure in &report.failures {
        eprintln!(
            "[failed] {} ({}): {}",
            failure.source, failure.trace_id, failure.error
        );
    }
    println!(
        "scanned={} ignored={} discovered={} ingested={} previous={} duplicates={} rejected={} failed={}",
        report.scanned_files,
        report.ignored_files,
        report.discovered_traces,
        report.ingested_traces,
        report.previously_ingested,
        report.duplicate_ids,
        report.rejected.len(),
        report.failures.len(),
    );
}

fn ingestion_exit_failed(report: &IngestionReport) -> bool {
    report.discovered_traces == 0 || !report.failures.is_empty()
}

async fn run_sequential_demo(runner: &ImprovementLoop, mode: DemoMode) {
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
            demo::print_live_trace_input(&steps);
        }

        let trace = demo::make_trace("demo-agent", steps);
        let result = runner.run_cycle(&trace).await.unwrap();

        demo::print_result(&result);
        if mode == DemoMode::Live {
            demo::print_live_analysis(&result);
        }
        println!();
    }
}

async fn run_batch_demo(runner: &ImprovementLoop) {
    println!("=== Batch evaluation (concurrency: {BATCH_CONCURRENCY}) ===\n");

    let traces: Vec<_> = vec![
        demo::make_trace(
            "fetch-agent",
            vec![
                ("http-get", StepStatus::Ok, 0.9),
                ("parse-json", StepStatus::Ok, 0.85),
            ],
        ),
        demo::make_trace(
            "broken-agent",
            vec![
                ("init", StepStatus::Err, 0.1),
                ("retry", StepStatus::Err, 0.1),
            ],
        ),
        demo::make_trace(
            "deploy-agent",
            vec![
                ("build", StepStatus::Ok, 0.8),
                ("test", StepStatus::Ok, 0.75),
                ("push", StepStatus::Ok, 0.9),
            ],
        ),
        demo::make_trace(
            "review-agent",
            vec![
                ("diff", StepStatus::Ok, 0.7),
                ("analyze", StepStatus::Ok, 0.65),
            ],
        ),
        demo::make_trace(
            "search-agent",
            vec![
                ("index", StepStatus::Ok, 0.6),
                ("query", StepStatus::Err, 0.3),
            ],
        ),
        demo::make_trace(
            "test-agent",
            vec![
                ("discover", StepStatus::Ok, 0.85),
                ("run", StepStatus::Ok, 0.9),
                ("report", StepStatus::Ok, 0.88),
            ],
        ),
    ];

    let batch = runner.run_batch(&traces).await;
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
}

fn print_strategy_history(path: &Path) {
    println!("\n--- strategy history ---");
    let store = FileStrategyStore::new(path.to_path_buf());
    for s in store.history() {
        println!(
            "  v{}: {} tool_prefs, {} thresholds",
            s.version,
            s.tool_preferences.len(),
            s.confidence_thresholds.len(),
        );
    }
}

fn parse_demo_args<I, S>(args: I) -> Result<CliAction, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect();

    match args.as_slice() {
        [] => Ok(CliAction::Run(DemoMode::Standard)),
        [arg] if matches!(arg.as_str(), "-h" | "--help" | "help") => Ok(CliAction::Help),
        [command, rest @ ..] if command == "ingest" => {
            parse_ingest_args(rest).map(CliAction::Ingest)
        }
        [arg] if matches!(arg.as_str(), "live" | "live-demo" | "--live") => {
            Ok(CliAction::Run(DemoMode::Live))
        }
        [arg] => Err(format!("unknown demo argument: {arg}")),
        [first, second, ..] => Err(format!(
            "unexpected extra arguments after {first}: {second}"
        )),
    }
}

fn parse_ingest_args(args: &[String]) -> Result<IngestArgs, String> {
    let [flag, strategy, roots @ ..] = args else {
        return Err("ingest requires --strategy <file> and at least one root".into());
    };
    if flag != "--strategy" {
        return Err(format!("unknown ingest option: {flag}"));
    }
    if strategy.starts_with('-') {
        return Err("--strategy requires a file path".into());
    }
    if roots.is_empty() {
        return Err("ingest requires at least one root".into());
    }
    if let Some(option) = roots.iter().find(|root| root.starts_with('-')) {
        return Err(format!("unknown ingest option: {option}"));
    }

    Ok(IngestArgs {
        strategy_path: PathBuf::from(strategy),
        roots: roots.iter().map(PathBuf::from).collect(),
    })
}

fn print_usage() {
    println!("usage: praxis [live-demo]");
    println!("       praxis ingest --strategy <file> <root>...");
}

#[cfg(test)]
mod tests {
    use super::{CliAction, DemoMode, IngestArgs, ingestion_exit_failed, parse_demo_args};
    use praxis::IngestionReport;
    use std::path::PathBuf;

    #[test]
    fn parse_demo_args_defaults_to_standard_mode() {
        assert_eq!(
            parse_demo_args(Vec::<String>::new()),
            Ok(CliAction::Run(DemoMode::Standard))
        );
    }

    #[test]
    fn parse_demo_args_accepts_live_aliases() {
        for arg in ["live", "live-demo", "--live"] {
            assert_eq!(parse_demo_args([arg]), Ok(CliAction::Run(DemoMode::Live)));
        }
    }

    #[test]
    fn parse_demo_args_supports_help_flags() {
        for arg in ["-h", "--help", "help"] {
            assert_eq!(parse_demo_args([arg]), Ok(CliAction::Help));
        }
    }

    #[test]
    fn parse_demo_args_rejects_unknown_argument() {
        let error = parse_demo_args(["--nope"]).unwrap_err();
        assert!(error.contains("--nope"));
    }

    #[test]
    fn parse_demo_args_rejects_extra_arguments() {
        let error = parse_demo_args(["live-demo", "extra"]).unwrap_err();
        assert!(error.contains("unexpected extra arguments"));
    }

    #[test]
    fn parse_demo_args_accepts_ingest_command() {
        assert_eq!(
            parse_demo_args(["ingest", "--strategy", "state.json", "traces-a", "traces-b",]),
            Ok(CliAction::Ingest(IngestArgs {
                strategy_path: PathBuf::from("state.json"),
                roots: vec![PathBuf::from("traces-a"), PathBuf::from("traces-b")],
            }))
        );
    }

    #[test]
    fn parse_demo_args_rejects_incomplete_ingest_command() {
        for args in [
            vec!["ingest"],
            vec!["ingest", "--strategy"],
            vec!["ingest", "--strategy", "state.json"],
            vec!["ingest", "--unknown", "state.json", "traces"],
        ] {
            assert!(parse_demo_args(args).is_err());
        }
    }

    #[test]
    fn ingestion_exit_status_rejects_empty_scan() {
        assert!(ingestion_exit_failed(&IngestionReport::default()));

        let report = IngestionReport {
            discovered_traces: 1,
            ..IngestionReport::default()
        };
        assert!(!ingestion_exit_failed(&report));
    }
}
