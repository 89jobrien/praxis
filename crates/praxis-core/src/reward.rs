//! Reward history, trend data, and the reward accumulator port.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use crux_improve::CruxId;
use serde::{Deserialize, Serialize};

/// A recorded reward for an agent's trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    /// The trace that was evaluated.
    pub trace_id: CruxId,
    /// The agent that produced the trace.
    pub agent: String,
    /// The evaluation score.
    pub score: f32,
    /// When the reward was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// Direction of an agent's score trend over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    /// Scores are improving.
    Improving,
    /// Scores are declining.
    Declining,
    /// Scores are stable.
    Stable,
}

/// Score trend for an agent across multiple evaluations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    /// The agent whose trend is being tracked.
    pub agent: String,
    /// Overall trend direction.
    pub direction: TrendDirection,
    /// Linear regression slope of scores over time.
    pub slope: f32,
    /// Number of samples included in the trend.
    pub sample_count: usize,
}

/// Errors that can occur in reward storage.
#[derive(Debug, thiserror::Error)]
pub enum RewardError {
    /// Reward store error.
    #[error("reward store error: {0}")]
    Store(String),
}

#[async_trait]
#[allow(clippy::double_must_use)] // `async_trait` adds a redundant `#[must_use]`.
pub trait RewardAccumulator: Send + Sync {
    /// Records an agent's evaluation score for a trace.
    async fn record(
        &mut self,
        trace_id: CruxId,
        agent: &str,
        score: f32,
    ) -> Result<(), RewardError>;

    /// Returns rewards for an agent, optionally limited to a recent window.
    async fn query(
        &self,
        agent: &str,
        window: Option<Duration>,
    ) -> Result<Vec<Reward>, RewardError>;

    /// Computes the score trend across an agent's recorded rewards.
    async fn trend(&self, agent: &str) -> Result<Trend, RewardError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_direction_serializes() {
        let d = TrendDirection::Improving;
        assert_eq!(serde_json::to_string(&d).unwrap(), r#""improving""#);
    }
}
