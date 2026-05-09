use crate::status::{TaskId, TaskRecord, TaskStatus};
use chrono::Utc;
use cruxx_improve::Crux;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

/// A submitted trace waiting to be processed.
pub struct Submission {
    pub task_id: TaskId,
    pub trace: Crux<serde_json::Value>,
}

/// Concurrent task queue with status tracking.
///
/// Submit traces for improvement cycle processing, track their status,
/// and query results. Thread-safe — cloneable handle backed by Arc.
#[derive(Clone)]
pub struct TaskQueue {
    records: Arc<Mutex<HashMap<TaskId, TaskRecord>>>,
    tx: mpsc::Sender<Submission>,
    rx: Arc<Mutex<mpsc::Receiver<Submission>>>,
    capacity: usize,
}

impl TaskQueue {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            tx,
            rx: Arc::new(Mutex::new(rx)),
            capacity,
        }
    }

    /// Submit a trace for evaluation. Returns the task ID for tracking.
    pub async fn submit(
        &self,
        agent: impl Into<String>,
        trace: Crux<serde_json::Value>,
    ) -> Result<TaskId, QueueError> {
        let mut record = TaskRecord::new(agent);
        record.trace_id = Some(trace.id.clone());
        let task_id = record.id.clone();

        {
            let mut records = self.records.lock().await;
            records.insert(task_id.clone(), record);
        }

        self.tx
            .send(Submission {
                task_id: task_id.clone(),
                trace,
            })
            .await
            .map_err(|_| QueueError::Full)?;

        Ok(task_id)
    }

    /// Receive the next submission (used by workers).
    pub async fn recv(&self) -> Option<Submission> {
        let mut rx = self.rx.lock().await;
        rx.recv().await
    }

    /// Mark a task as running.
    pub async fn mark_running(&self, id: &TaskId) {
        let mut records = self.records.lock().await;
        if let Some(r) = records.get_mut(id) {
            r.status = TaskStatus::Running;
            r.started_at = Some(Utc::now());
        }
    }

    /// Mark a task as done with a score.
    pub async fn mark_done(&self, id: &TaskId, score: f32) {
        let mut records = self.records.lock().await;
        if let Some(r) = records.get_mut(id) {
            r.status = TaskStatus::Done;
            r.score = Some(score);
            r.finished_at = Some(Utc::now());
        }
    }

    /// Mark a task as failed with an error message.
    pub async fn mark_failed(&self, id: &TaskId, error: String) {
        let mut records = self.records.lock().await;
        if let Some(r) = records.get_mut(id) {
            r.status = TaskStatus::Failed;
            r.error = Some(error);
            r.finished_at = Some(Utc::now());
        }
    }

    /// Cancel a pending task.
    pub async fn cancel(&self, id: &TaskId) -> bool {
        let mut records = self.records.lock().await;
        if let Some(r) = records.get_mut(id) {
            if r.status == TaskStatus::Pending {
                r.status = TaskStatus::Cancelled;
                r.finished_at = Some(Utc::now());
                return true;
            }
        }
        false
    }

    /// Get a task's current record.
    pub async fn get(&self, id: &TaskId) -> Option<TaskRecord> {
        let records = self.records.lock().await;
        records.get(id).cloned()
    }

    /// List all task records, optionally filtered by status.
    pub async fn list(&self, status: Option<TaskStatus>) -> Vec<TaskRecord> {
        let records = self.records.lock().await;
        records
            .values()
            .filter(|r| status.is_none_or(|s| r.status == s))
            .cloned()
            .collect()
    }

    /// Count tasks by status.
    pub async fn stats(&self) -> QueueStats {
        let records = self.records.lock().await;
        let mut stats = QueueStats::default();
        for r in records.values() {
            match r.status {
                TaskStatus::Pending => stats.pending += 1,
                TaskStatus::Running => stats.running += 1,
                TaskStatus::Done => stats.done += 1,
                TaskStatus::Failed => stats.failed += 1,
                TaskStatus::Cancelled => stats.cancelled += 1,
            }
        }
        stats.capacity = self.capacity;
        stats
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct QueueStats {
    pub pending: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub capacity: usize,
}

impl QueueStats {
    pub fn total(&self) -> usize {
        self.pending + self.running + self.done + self.failed + self.cancelled
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("queue is full")]
    Full,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_improve::CruxId;

    fn dummy_trace(agent: &str) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: agent.into(),
            value: Ok(serde_json::json!({})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn submit_and_get() {
        let q = TaskQueue::new(10);
        let id = q.submit("agent-a", dummy_trace("agent-a")).await.unwrap();
        let record = q.get(&id).await.unwrap();
        assert_eq!(record.status, TaskStatus::Pending);
        assert_eq!(record.agent, "agent-a");
    }

    #[tokio::test]
    async fn lifecycle_pending_running_done() {
        let q = TaskQueue::new(10);
        let id = q.submit("a", dummy_trace("a")).await.unwrap();

        q.mark_running(&id).await;
        let r = q.get(&id).await.unwrap();
        assert_eq!(r.status, TaskStatus::Running);
        assert!(r.started_at.is_some());

        q.mark_done(&id, 0.85).await;
        let r = q.get(&id).await.unwrap();
        assert_eq!(r.status, TaskStatus::Done);
        assert_eq!(r.score, Some(0.85));
        assert!(r.finished_at.is_some());
        assert!(r.duration_ms().unwrap() < 1000);
    }

    #[tokio::test]
    async fn lifecycle_failed() {
        let q = TaskQueue::new(10);
        let id = q.submit("a", dummy_trace("a")).await.unwrap();
        q.mark_running(&id).await;
        q.mark_failed(&id, "timeout".into()).await;

        let r = q.get(&id).await.unwrap();
        assert_eq!(r.status, TaskStatus::Failed);
        assert_eq!(r.error.as_deref(), Some("timeout"));
    }

    #[tokio::test]
    async fn cancel_pending() {
        let q = TaskQueue::new(10);
        let id = q.submit("a", dummy_trace("a")).await.unwrap();
        assert!(q.cancel(&id).await);

        let r = q.get(&id).await.unwrap();
        assert_eq!(r.status, TaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn cancel_running_fails() {
        let q = TaskQueue::new(10);
        let id = q.submit("a", dummy_trace("a")).await.unwrap();
        q.mark_running(&id).await;
        assert!(!q.cancel(&id).await);
    }

    #[tokio::test]
    async fn recv_gets_submission() {
        let q = TaskQueue::new(10);
        let id = q.submit("a", dummy_trace("a")).await.unwrap();
        let sub = q.recv().await.unwrap();
        assert_eq!(sub.task_id, id);
    }

    #[tokio::test]
    async fn stats_counts_correctly() {
        let q = TaskQueue::new(10);
        let id1 = q.submit("a", dummy_trace("a")).await.unwrap();
        let id2 = q.submit("b", dummy_trace("b")).await.unwrap();
        let _id3 = q.submit("c", dummy_trace("c")).await.unwrap();

        q.mark_running(&id1).await;
        q.mark_done(&id1, 0.9).await;
        q.mark_running(&id2).await;
        q.mark_failed(&id2, "err".into()).await;

        let stats = q.stats().await;
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.done, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.total(), 3);
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let q = TaskQueue::new(10);
        q.submit("a", dummy_trace("a")).await.unwrap();
        let id2 = q.submit("b", dummy_trace("b")).await.unwrap();
        q.mark_running(&id2).await;
        q.mark_done(&id2, 0.8).await;

        let done = q.list(Some(TaskStatus::Done)).await;
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].agent, "b");

        let all = q.list(None).await;
        assert_eq!(all.len(), 2);
    }
}
