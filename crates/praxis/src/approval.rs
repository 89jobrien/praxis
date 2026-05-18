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

/// Interactive CLI approval gate. Prints improvement details to stdout
/// and reads y/n/d from stdin.
pub struct CliApprovalGate;

#[async_trait]
impl ApprovalGate for CliApprovalGate {
    async fn review(&self, improvement: &Improvement) -> ApprovalDecision {
        println!("Improvement proposed: {:?}", improvement.kind);
        println!("  target: {}", improvement.target);
        println!("  confidence: {:.2}", improvement.confidence);
        println!("  evidence: {:?}", improvement.evidence);
        print!("  approve? [y/n/d]: ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
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
}
