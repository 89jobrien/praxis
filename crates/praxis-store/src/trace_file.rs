use crux_improve::Crux;
use praxis_core::{DiscoveredTrace, RejectedTrace, TraceDiscovery, TraceSource, TraceSourceError};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_TRACE_BYTES: u64 = 16 * 1024 * 1024;

/// Recursively discovers raw Crux JSON traces under explicit roots.
#[derive(Debug, Clone)]
pub struct FileTraceSource {
    roots: Vec<PathBuf>,
    max_file_bytes: u64,
}

impl FileTraceSource {
    /// Creates a source that scans the supplied files and directories.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            max_file_bytes: DEFAULT_MAX_TRACE_BYTES,
        }
    }

    /// Overrides the maximum accepted trace file size.
    #[must_use]
    pub fn with_max_file_bytes(mut self, max_file_bytes: u64) -> Self {
        self.max_file_bytes = max_file_bytes;
        self
    }

    fn visit(&self, path: &Path, discovery: &mut TraceDiscovery) -> Result<(), TraceSourceError> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|error| TraceSourceError::Discovery(format!("{}: {error}", path.display())))?;

        if metadata.file_type().is_symlink() {
            return Ok(());
        }

        if metadata.is_dir() {
            let mut entries = std::fs::read_dir(path)
                .map_err(|error| {
                    TraceSourceError::Discovery(format!("{}: {error}", path.display()))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    TraceSourceError::Discovery(format!("{}: {error}", path.display()))
                })?;
            entries.sort_by_key(std::fs::DirEntry::path);

            for entry in entries {
                self.visit(&entry.path(), discovery)?;
            }
            return Ok(());
        }

        if metadata.is_file() && is_json(path) {
            self.inspect_file(path, metadata.len(), discovery);
        }

        Ok(())
    }

    fn inspect_file(&self, path: &Path, file_bytes: u64, discovery: &mut TraceDiscovery) {
        discovery.scanned_files += 1;
        let source = path.display().to_string();

        if file_bytes > self.max_file_bytes {
            discovery.rejected.push(RejectedTrace {
                source,
                reason: format!("file exceeds size limit of {} bytes", self.max_file_bytes),
            });
            return;
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                discovery.rejected.push(RejectedTrace {
                    source,
                    reason: format!("failed to read file: {error}"),
                });
                return;
            }
        };

        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                discovery.rejected.push(RejectedTrace {
                    source,
                    reason: format!("invalid JSON: {error}"),
                });
                return;
            }
        };

        let Some(object) = value.as_object() else {
            discovery.ignored_files += 1;
            return;
        };
        let has_markers = ["id", "agent", "steps"]
            .iter()
            .all(|key| object.contains_key(*key));
        if !has_markers {
            discovery.ignored_files += 1;
            return;
        }
        if !object.contains_key("value") && object.contains_key("status") {
            discovery.rejected.push(RejectedTrace {
                source,
                reason: "presentation trace is not replayable raw Crux JSON".into(),
            });
            return;
        }

        match serde_json::from_value::<Crux<serde_json::Value>>(value) {
            Ok(trace) => discovery.traces.push(DiscoveredTrace { source, trace }),
            Err(error) => discovery.rejected.push(RejectedTrace {
                source,
                reason: format!("failed to deserialize raw Crux trace: {error}"),
            }),
        }
    }
}

impl TraceSource for FileTraceSource {
    fn discover(&self) -> Result<TraceDiscovery, TraceSourceError> {
        if self.roots.is_empty() {
            return Err(TraceSourceError::Discovery(
                "at least one scan root is required".into(),
            ));
        }

        let mut discovery = TraceDiscovery::default();
        for root in &self.roots {
            self.visit(root, &mut discovery)?;
        }
        Ok(discovery)
    }
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use crux_improve::{Crux, CruxId};
    use praxis_core::TraceSource as _;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    use super::FileTraceSource;

    fn write_trace(path: &std::path::Path, agent: &str) {
        let trace = Crux {
            id: CruxId::new(),
            agent: agent.into(),
            value: Ok(json!({"complete": true})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        };
        fs::write(path, serde_json::to_vec(&trace).unwrap()).unwrap();
    }

    #[test]
    fn discovers_nested_raw_trace_and_ignores_unrelated_files() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        write_trace(&nested.join("run.json"), "agent-a");
        fs::write(dir.path().join("other.json"), br#"{"name":"config"}"#).unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

        let discovery = FileTraceSource::new(vec![dir.path().to_path_buf()])
            .discover()
            .unwrap();

        assert_eq!(discovery.scanned_files, 2);
        assert_eq!(discovery.ignored_files, 1);
        assert_eq!(discovery.traces.len(), 1);
        assert_eq!(discovery.traces[0].trace.agent, "agent-a");
    }

    #[test]
    fn rejects_presentation_malformed_and_oversized_candidates() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("presentation.json"),
            serde_json::to_vec(&json!({
                "id": "crux_presentation",
                "agent": "demo",
                "status": "ok",
                "steps": []
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.path().join("malformed.json"),
            serde_json::to_vec(&json!({
                "id": "crux_malformed",
                "agent": "demo",
                "value": {"Ok": null},
                "steps": []
            }))
            .unwrap(),
        )
        .unwrap();
        write_trace(&dir.path().join("oversized.json"), "large");

        let discovery = FileTraceSource::new(vec![dir.path().to_path_buf()])
            .with_max_file_bytes(100)
            .discover()
            .unwrap();

        assert_eq!(discovery.traces.len(), 0);
        assert_eq!(discovery.rejected.len(), 3);
        assert!(
            discovery
                .rejected
                .iter()
                .any(|item| item.reason.contains("presentation"))
        );
        assert!(
            discovery
                .rejected
                .iter()
                .any(|item| item.reason.contains("deserialize"))
        );
        assert!(
            discovery
                .rejected
                .iter()
                .any(|item| item.reason.contains("size limit"))
        );
    }

    #[test]
    fn missing_root_returns_discovery_error() {
        let dir = TempDir::new().unwrap();
        let error = FileTraceSource::new(vec![dir.path().join("missing")])
            .discover()
            .unwrap_err();

        assert!(error.to_string().contains("missing"));
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symbolic_links() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        write_trace(&outside.path().join("linked.json"), "linked-agent");
        symlink(outside.path(), dir.path().join("linked-dir")).unwrap();

        let discovery = FileTraceSource::new(vec![dir.path().to_path_buf()])
            .discover()
            .unwrap();

        assert!(discovery.traces.is_empty());
        assert_eq!(discovery.scanned_files, 0);
    }
}
