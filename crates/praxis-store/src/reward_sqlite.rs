use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use cruxx_improve::CruxId;
use praxis_core::reward::{Reward, RewardAccumulator, RewardError, Trend, TrendDirection};
use rusqlite::Connection;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;

const IMPROVING_SLOPE_THRESHOLD: f32 = 0.01;
const DECLINING_SLOPE_THRESHOLD: f32 = -0.01;

pub struct SqliteRewardStore {
    conn: Mutex<Connection>,
}

impl SqliteRewardStore {
    pub fn new(path: PathBuf) -> Result<Self, RewardError> {
        let conn = Connection::open(path).map_err(|e| RewardError::Store(e.to_string()))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, RewardError> {
        let conn = Connection::open_in_memory().map_err(|e| RewardError::Store(e.to_string()))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), RewardError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RewardError::Store(e.to_string()))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS rewards (
                id INTEGER PRIMARY KEY,
                trace_id TEXT NOT NULL,
                agent TEXT NOT NULL,
                score REAL NOT NULL,
                recorded_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| RewardError::Store(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl RewardAccumulator for SqliteRewardStore {
    async fn record(
        &mut self,
        trace_id: CruxId,
        agent: &str,
        score: f32,
    ) -> Result<(), RewardError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RewardError::Store(e.to_string()))?;
        conn.execute(
            "INSERT INTO rewards (trace_id, agent, score, recorded_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                trace_id.to_string(),
                agent,
                score as f64,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| RewardError::Store(e.to_string()))?;
        Ok(())
    }

    async fn query(
        &self,
        agent: &str,
        window: Option<Duration>,
    ) -> Result<Vec<Reward>, RewardError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RewardError::Store(e.to_string()))?;
        let cutoff = window.map(|w| (Utc::now() - w).to_rfc3339());

        let mut stmt = if cutoff.is_some() {
            conn.prepare(
                "SELECT trace_id, agent, score, recorded_at FROM rewards WHERE agent = ?1 AND recorded_at >= ?2 ORDER BY id",
            )
            .map_err(|e| RewardError::Store(e.to_string()))?
        } else {
            conn.prepare(
                "SELECT trace_id, agent, score, recorded_at FROM rewards WHERE agent = ?1 ORDER BY id",
            )
            .map_err(|e| RewardError::Store(e.to_string()))?
        };

        let rows = if let Some(ref c) = cutoff {
            stmt.query_map(rusqlite::params![agent, c], |row| {
                Ok(RowData {
                    trace_id: row.get(0)?,
                    agent: row.get(1)?,
                    score: row.get::<_, f64>(2)?,
                    recorded_at: row.get::<_, String>(3)?,
                })
            })
        } else {
            stmt.query_map(rusqlite::params![agent], |row| {
                Ok(RowData {
                    trace_id: row.get(0)?,
                    agent: row.get(1)?,
                    score: row.get::<_, f64>(2)?,
                    recorded_at: row.get::<_, String>(3)?,
                })
            })
        }
        .map_err(|e| RewardError::Store(e.to_string()))?;

        let mut rewards = Vec::new();
        for row in rows {
            let r = row.map_err(|e| RewardError::Store(e.to_string()))?;
            let trace_id =
                CruxId::from_str(&r.trace_id).map_err(|e| RewardError::Store(e.to_string()))?;
            let recorded_at = DateTime::parse_from_rfc3339(&r.recorded_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| RewardError::Store(e.to_string()))?;
            rewards.push(Reward {
                trace_id,
                agent: r.agent,
                score: r.score as f32,
                recorded_at,
            });
        }
        Ok(rewards)
    }

    async fn trend(&self, agent: &str) -> Result<Trend, RewardError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RewardError::Store(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT score FROM rewards WHERE agent = ?1 ORDER BY id")
            .map_err(|e| RewardError::Store(e.to_string()))?;

        let scores: Vec<f32> = stmt
            .query_map(rusqlite::params![agent], |row| {
                row.get::<_, f64>(0).map(|v| v as f32)
            })
            .map_err(|e| RewardError::Store(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| RewardError::Store(e.to_string()))?;

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

        let direction = if slope > IMPROVING_SLOPE_THRESHOLD {
            TrendDirection::Improving
        } else if slope < DECLINING_SLOPE_THRESHOLD {
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

struct RowData {
    trace_id: String,
    agent: String,
    score: f64,
    recorded_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_record_and_query() {
        let mut store = SqliteRewardStore::in_memory().unwrap();
        store.record(CruxId::new(), "a", 0.8).await.unwrap();
        store.record(CruxId::new(), "a", 0.6).await.unwrap();
        store.record(CruxId::new(), "b", 0.9).await.unwrap();
        let results = store.query("a", None).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!((results[0].score - 0.8).abs() < 0.01);
        assert!((results[1].score - 0.6).abs() < 0.01);
    }

    #[tokio::test]
    async fn sqlite_trend_ascending() {
        let mut store = SqliteRewardStore::in_memory().unwrap();
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
    async fn sqlite_persists_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("rewards.db");

        let id = CruxId::new();
        {
            let mut store1 = SqliteRewardStore::new(db_path.clone()).unwrap();
            store1.record(id.clone(), "a", 0.7).await.unwrap();
        }

        let store2 = SqliteRewardStore::new(db_path).unwrap();
        let results = store2.query("a", None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!((results[0].score - 0.7).abs() < 0.01);
    }

    #[tokio::test]
    async fn sqlite_unknown_agent_stable() {
        let store = SqliteRewardStore::in_memory().unwrap();
        let t = store.trend("nonexistent").await.unwrap();
        assert_eq!(t.direction, TrendDirection::Stable);
        assert_eq!(t.sample_count, 0);
    }
}
