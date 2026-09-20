use praxis_core::{IngestionLedger, IngestionLedgerError};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Write as _,
    path::{Path, PathBuf},
};

const LEDGER_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct LedgerDocument {
    version: u32,
    trace_ids: Vec<String>,
}

/// JSON-backed set of trace IDs that completed ingestion.
#[derive(Debug)]
pub struct FileIngestionLedger {
    path: PathBuf,
    trace_ids: HashSet<String>,
}

impl FileIngestionLedger {
    /// Opens an existing ledger or creates an empty versioned ledger.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent directory is absent, existing state is
    /// invalid, or the initial ledger cannot be persisted.
    pub fn open(path: PathBuf) -> Result<Self, IngestionLedgerError> {
        if path.exists() {
            let bytes = std::fs::read(&path).map_err(|error| ledger_error(&path, error))?;
            let document: LedgerDocument = serde_json::from_slice(&bytes).map_err(|error| {
                IngestionLedgerError::Store(format!(
                    "{} contains invalid ledger JSON: {error}",
                    path.display()
                ))
            })?;
            if document.version != LEDGER_VERSION {
                return Err(IngestionLedgerError::Store(format!(
                    "{} uses unsupported ledger version {}",
                    path.display(),
                    document.version
                )));
            }
            return Ok(Self {
                path,
                trace_ids: document.trace_ids.into_iter().collect(),
            });
        }

        let parent = parent_directory(&path);
        if !parent.is_dir() {
            return Err(IngestionLedgerError::Store(format!(
                "parent directory {} is missing",
                parent.display()
            )));
        }

        let ledger = Self {
            path,
            trace_ids: HashSet::new(),
        };
        ledger.persist()?;
        Ok(ledger)
    }

    fn persist(&self) -> Result<(), IngestionLedgerError> {
        let mut trace_ids: Vec<_> = self.trace_ids.iter().cloned().collect();
        trace_ids.sort_unstable();
        let document = LedgerDocument {
            version: LEDGER_VERSION,
            trace_ids,
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
            IngestionLedgerError::Store(format!("failed to serialize ledger: {error}"))
        })?;
        let temporary = temporary_path(&self.path);
        let result = (|| {
            let mut file = std::fs::File::create(&temporary)
                .map_err(|error| ledger_error(&temporary, error))?;
            file.write_all(&bytes)
                .map_err(|error| ledger_error(&temporary, error))?;
            file.sync_all()
                .map_err(|error| ledger_error(&temporary, error))?;
            std::fs::rename(&temporary, &self.path).map_err(|error| ledger_error(&self.path, error))
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

impl IngestionLedger for FileIngestionLedger {
    fn contains(&self, trace_id: &crux_improve::CruxId) -> bool {
        self.trace_ids.contains(trace_id.as_str())
    }

    fn record(&mut self, trace_id: &crux_improve::CruxId) -> Result<(), IngestionLedgerError> {
        if !self.trace_ids.insert(trace_id.to_string()) {
            return Ok(());
        }

        if let Err(error) = self.persist() {
            self.trace_ids.remove(trace_id.as_str());
            return Err(error);
        }
        Ok(())
    }
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("praxis-ingestion-ledger");
    path.with_file_name(format!(".{file_name}.tmp"))
}

fn ledger_error(path: &Path, error: std::io::Error) -> IngestionLedgerError {
    IngestionLedgerError::Store(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use crux_improve::CruxId;
    use praxis_core::IngestionLedger as _;
    use tempfile::TempDir;

    use super::FileIngestionLedger;

    #[test]
    fn ledger_records_and_reopens_trace_ids() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ingested.json");
        let id = CruxId::new();

        let mut ledger = FileIngestionLedger::open(path.clone()).unwrap();
        assert!(path.exists());
        assert!(!ledger.contains(&id));
        ledger.record(&id).unwrap();
        ledger.record(&id).unwrap();

        let reopened = FileIngestionLedger::open(path).unwrap();
        assert!(reopened.contains(&id));
    }

    #[test]
    fn ledger_rejects_malformed_existing_state() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ingested.json");
        std::fs::write(&path, "not json").unwrap();

        let error = FileIngestionLedger::open(path).unwrap_err();

        assert!(error.to_string().contains("invalid"));
    }

    #[test]
    fn ledger_requires_existing_parent_directory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing").join("ingested.json");

        let error = FileIngestionLedger::open(path).unwrap_err();

        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn ledger_atomic_write_leaves_no_temporary_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ingested.json");
        let mut ledger = FileIngestionLedger::open(path).unwrap();
        ledger.record(&CruxId::new()).unwrap();

        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("ingested.json")]);
    }
}
