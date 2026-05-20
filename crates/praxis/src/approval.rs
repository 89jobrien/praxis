use std::io::Write;

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
    async fn review(&self, improvement: &Improvement) -> ApprovalDecision;
}

/// Auto-approves everything. Default gate when no human review is needed.
pub struct AutoApproveGate;

#[async_trait]
impl ApprovalGate for AutoApproveGate {
    async fn review(&self, _: &Improvement) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

/// Interactive CLI approval gate. Prints improvement details and reads y/n/d.
///
/// Use `CliApprovalGate::new()` for real stdin/stdout, or
/// `CliApprovalGate::with_io(reader, writer)` for testing.
pub struct CliApprovalGate {
    reader: std::sync::Mutex<Box<dyn std::io::BufRead + Send>>,
    writer: std::sync::Mutex<Box<dyn std::io::Write + Send>>,
}

impl CliApprovalGate {
    /// Create a gate that reads from stdin and writes to stdout.
    pub fn new() -> Self {
        Self {
            reader: std::sync::Mutex::new(Box::new(std::io::BufReader::new(std::io::stdin()))),
            writer: std::sync::Mutex::new(Box::new(std::io::stdout())),
        }
    }

    /// Create a gate with custom I/O for testing.
    pub fn with_io(
        reader: Box<dyn std::io::BufRead + Send>,
        writer: Box<dyn std::io::Write + Send>,
    ) -> Self {
        Self {
            reader: std::sync::Mutex::new(reader),
            writer: std::sync::Mutex::new(writer),
        }
    }
}

impl Default for CliApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApprovalGate for CliApprovalGate {
    async fn review(&self, improvement: &Improvement) -> ApprovalDecision {
        let mut writer = self.writer.lock().expect("writer lock poisoned");
        writeln!(writer, "Improvement proposed: {:?}", improvement.kind).ok();
        writeln!(writer, "  target: {}", improvement.target).ok();
        writeln!(writer, "  confidence: {:.2}", improvement.confidence).ok();
        writeln!(writer, "  evidence: {:?}", improvement.evidence).ok();
        write!(writer, "  approve? [y/n/d]: ").ok();
        writer.flush().ok();

        let mut input = String::new();
        let mut reader = self.reader.lock().expect("reader lock poisoned");
        reader.read_line(&mut input).ok();
        match input.trim() {
            "y" | "Y" | "yes" => ApprovalDecision::Approved,
            "n" | "N" | "no" => ApprovalDecision::Rejected,
            _ => ApprovalDecision::Deferred,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cruxx_improve::{CruxId, ImprovementKind, StrategyDiff};

    fn dummy_improvement() -> Improvement {
        Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::PromptTemplate,
            target: "test".into(),
            confidence: 0.9,
            diff: StrategyDiff::default(),
            evidence: vec![],
            proposed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn auto_approve_always_approves() {
        let gate = AutoApproveGate;
        let decision = gate.review(&dummy_improvement()).await;
        assert_eq!(decision, ApprovalDecision::Approved);
    }

    #[tokio::test]
    async fn cli_gate_approves_on_y() {
        let input = std::io::Cursor::new(b"y\n".to_vec());
        let output: Vec<u8> = Vec::new();
        let gate = CliApprovalGate::with_io(Box::new(input), Box::new(output));
        let decision = gate.review(&dummy_improvement()).await;
        assert_eq!(decision, ApprovalDecision::Approved);
    }

    #[tokio::test]
    async fn cli_gate_rejects_on_n() {
        let input = std::io::Cursor::new(b"n\n".to_vec());
        let output: Vec<u8> = Vec::new();
        let gate = CliApprovalGate::with_io(Box::new(input), Box::new(output));
        let decision = gate.review(&dummy_improvement()).await;
        assert_eq!(decision, ApprovalDecision::Rejected);
    }

    #[tokio::test]
    async fn cli_gate_defers_on_d() {
        let input = std::io::Cursor::new(b"d\n".to_vec());
        let output: Vec<u8> = Vec::new();
        let gate = CliApprovalGate::with_io(Box::new(input), Box::new(output));
        let decision = gate.review(&dummy_improvement()).await;
        assert_eq!(decision, ApprovalDecision::Deferred);
    }

    #[tokio::test]
    async fn cli_gate_defers_on_unknown_input() {
        let input = std::io::Cursor::new(b"maybe\n".to_vec());
        let output: Vec<u8> = Vec::new();
        let gate = CliApprovalGate::with_io(Box::new(input), Box::new(output));
        let decision = gate.review(&dummy_improvement()).await;
        assert_eq!(decision, ApprovalDecision::Deferred);
    }
}
