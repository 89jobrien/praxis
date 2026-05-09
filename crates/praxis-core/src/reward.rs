use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use cruxx_improve::CruxId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub trace_id: CruxId,
    pub agent: String,
    pub score: f32,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Improving,
    Declining,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trend {
    pub agent: String,
    pub direction: TrendDirection,
    pub slope: f32,
    pub sample_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum RewardError {
    #[error("reward store error: {0}")]
    Store(String),
}

#[async_trait]
pub trait RewardAccumulator: Send + Sync {
    async fn record(
        &mut self,
        trace_id: CruxId,
        agent: &str,
        score: f32,
    ) -> Result<(), RewardError>;

    async fn query(
        &self,
        agent: &str,
        window: Option<Duration>,
    ) -> Result<Vec<Reward>, RewardError>;

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
