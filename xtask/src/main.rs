use clap::{Parser, Subcommand};
use std::process::{Command, ExitCode, Stdio};

#[derive(Parser)]
#[command(name = "xtask", about = "praxis workspace tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the full CI gate: fmt-check + clippy + nextest
    Ci,
    /// Run cargo nextest
    Test {
        /// Package to test (default: all)
        #[arg(short, long)]
        package: Option<String>,
        /// Test name filter
        filter: Option<String>,
    },
    /// Run clippy with deny warnings
    Lint,
    /// Run cargo fmt --all
    Fmt,
    /// Check formatting without modifying
    FmtCheck,
    /// Build all targets
    Build,
    /// Run the demo binary
    Demo,
    /// Pre-commit gate: fmt-check + clippy + test
    PreCommit,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.cmd {
        Cmd::Ci => ci(),
        Cmd::Test { package, filter } => test(package, filter),
        Cmd::Lint => lint(),
        Cmd::Fmt => fmt(),
        Cmd::FmtCheck => fmt_check(),
        Cmd::Build => build(),
        Cmd::Demo => demo(),
        Cmd::PreCommit => pre_commit(),
    };

    if result.is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn ci() -> Result<(), ()> {
    step("fmt-check", fmt_check)?;
    step("lint", lint)?;
    step("test", || test(None, None))?;
    Ok(())
}

fn pre_commit() -> Result<(), ()> {
    step("fmt-check", fmt_check)?;
    step("lint", lint)?;
    step("test", || test(None, None))?;
    Ok(())
}

fn test(package: Option<String>, filter: Option<String>) -> Result<(), ()> {
    let mut args = vec!["nextest", "run"];
    if let Some(ref p) = package {
        args.push("-p");
        args.push(p);
    }
    if let Some(ref f) = filter {
        args.push("--");
        args.push(f);
    }
    cargo(&args)
}

fn lint() -> Result<(), ()> {
    cargo(&["clippy", "--all-targets", "--", "-D", "warnings"])
}

fn fmt() -> Result<(), ()> {
    cargo(&["fmt", "--all"])
}

fn fmt_check() -> Result<(), ()> {
    cargo(&["fmt", "--all", "--", "--check"])
}

fn build() -> Result<(), ()> {
    cargo(&["build", "--all-targets"])
}

fn demo() -> Result<(), ()> {
    cargo(&["run", "-p", "praxis"])
}

fn step(name: &str, f: impl FnOnce() -> Result<(), ()>) -> Result<(), ()> {
    eprintln!("\n--- {name} ---");
    f()
}

fn cargo(args: &[&str]) -> Result<(), ()> {
    let status = Command::new("cargo")
        .args(args)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| eprintln!("failed to run cargo: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        eprintln!("cargo {} failed with {status}", args.join(" "));
        Err(())
    }
}
