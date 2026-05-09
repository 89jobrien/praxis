use cruxx_improve::{Strategy, StrategyDiff};
use praxis_core::store::StrategyStore;
use std::path::PathBuf;

pub struct FileStrategyStore {
    pub(crate) path: PathBuf,
    snapshots: Vec<Strategy>,
}

impl FileStrategyStore {
    pub fn new(path: PathBuf) -> Self {
        let snapshots = if path.exists() {
            let data = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str::<Vec<Strategy>>(&data)
                .unwrap_or_else(|_| vec![Strategy::default()])
        } else {
            vec![Strategy::default()]
        };
        Self { path, snapshots }
    }

    fn persist(&self) {
        let json = serde_json::to_string_pretty(&self.snapshots).expect("strategy serialization");
        std::fs::write(&self.path, json).expect("strategy file write");
    }
}

impl StrategyStore for FileStrategyStore {
    fn current(&self) -> Strategy {
        self.snapshots.last().cloned().unwrap_or_default()
    }

    fn apply(&mut self, diff: &StrategyDiff) -> Strategy {
        let mut next = self.current();
        next.apply(diff);
        self.snapshots.push(next.clone());
        self.persist();
        next
    }

    fn history(&self) -> Vec<Strategy> {
        self.snapshots.clone()
    }

    fn rollback(&mut self, version: u64) {
        if let Some(idx) = self.snapshots.iter().position(|s| s.version == version) {
            self.snapshots.truncate(idx + 1);
            self.persist();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn roundtrips_strategy() {
        let dir = TempDir::new().unwrap();
        let mut store = FileStrategyStore::new(dir.path().join("s.json"));
        assert_eq!(store.current().version, 0);

        let diff = StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        };
        let updated = store.apply(&diff);
        assert_eq!(updated.version, 1);
        assert_eq!(updated.tool_preferences["rg"], 5);

        let store2 = FileStrategyStore::new(store.path.clone());
        assert_eq!(store2.current().version, 1);
    }

    #[test]
    fn rollback_restores_previous() {
        let dir = TempDir::new().unwrap();
        let mut store = FileStrategyStore::new(dir.path().join("s.json"));
        store.apply(&StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        });
        store.apply(&StrategyDiff {
            tool_preferences: vec![("fd".into(), 3)],
            ..Default::default()
        });
        assert_eq!(store.current().version, 2);

        store.rollback(1);
        assert_eq!(store.current().version, 1);
        assert!(!store.current().tool_preferences.contains_key("fd"));
    }

    #[test]
    fn history_returns_all_versions() {
        let dir = TempDir::new().unwrap();
        let mut store = FileStrategyStore::new(dir.path().join("s.json"));
        store.apply(&StrategyDiff::default());
        store.apply(&StrategyDiff::default());
        assert_eq!(store.history().len(), 3);
    }
}
