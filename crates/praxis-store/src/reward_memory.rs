use async_trait::async_trait;
use chrono::{Duration, Utc};
use cruxx_improve::CruxId;
use praxis_core::reward::{Reward, RewardAccumulator, RewardError, Trend, TrendDirection};

#[derive(Debug, Default)]
pub struct InMemoryRewardStore {
    rewards: Vec<Reward>,
}

impl InMemoryRewardStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RewardAccumulator for InMemoryRewardStore {
    async fn record(
        &mut self,
        trace_id: CruxId,
        agent: &str,
        score: f32,
    ) -> Result<(), RewardError> {
        self.rewards.push(Reward {
            trace_id,
            agent: agent.to_string(),
            score,
            recorded_at: Utc::now(),
        });
        Ok(())
    }

    async fn query(
        &self,
        agent: &str,
        window: Option<Duration>,
    ) -> Result<Vec<Reward>, RewardError> {
        let cutoff = window.map(|w| Utc::now() - w);
        Ok(self
            .rewards
            .iter()
            .filter(|r| r.agent == agent && cutoff.is_none_or(|c| r.recorded_at >= c))
            .cloned()
            .collect())
    }

    async fn trend(&self, agent: &str) -> Result<Trend, RewardError> {
        let scores: Vec<f32> = self
            .rewards
            .iter()
            .filter(|r| r.agent == agent)
            .map(|r| r.score)
            .collect();

        if scores.len() < 2 {
            return Ok(Trend {
                agent: agent.to_string(),
                direction: TrendDirection::Stable,
                slope: 0.0,
                sample_count: scores.len(),
            });
        }

        let n = scores.len() as f32;
        let sum_x: f32 = (0..scores.len()).map(|i| i as f32).sum();
        let sum_y: f32 = scores.iter().sum();
        let sum_xy: f32 = scores.iter().enumerate().map(|(i, y)| i as f32 * y).sum();
        let sum_xx: f32 = (0..scores.len()).map(|i| (i as f32) * (i as f32)).sum();

        let denom = n * sum_xx - sum_x * sum_x;
        let slope = if denom.abs() < f32::EPSILON {
            0.0
        } else {
            (n * sum_xy - sum_x * sum_y) / denom
        };

        let direction = if slope > 0.01 {
            TrendDirection::Improving
        } else if slope < -0.01 {
            TrendDirection::Declining
        } else {
            TrendDirection::Stable
        };

        Ok(Trend {
            agent: agent.to_string(),
            direction,
            slope,
            sample_count: scores.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_and_query() {
        let mut store = InMemoryRewardStore::new();
        store.record(CruxId::new(), "a", 0.8).await.unwrap();
        store.record(CruxId::new(), "a", 0.6).await.unwrap();
        store.record(CruxId::new(), "b", 0.9).await.unwrap();
        assert_eq!(store.query("a", None).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn trend_ascending_is_improving() {
        let mut store = InMemoryRewardStore::new();
        for i in 0..5 {
            store
                .record(CruxId::new(), "a", 0.5 + (i as f32) * 0.1)
                .await
                .unwrap();
        }
        let t = store.trend("a").await.unwrap();
        assert_eq!(t.direction, TrendDirection::Improving);
    }

    #[tokio::test]
    async fn unknown_agent_is_stable() {
        let store = InMemoryRewardStore::new();
        let t = store.trend("x").await.unwrap();
        assert_eq!(t.direction, TrendDirection::Stable);
    }
}
