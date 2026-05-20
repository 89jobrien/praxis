use cruxx_improve::Strategy;
use std::path::Path;

/// Exports the current strategy as a JSON file that braid can consume.
/// The format is the raw `Strategy` struct serialized as pretty JSON.
pub fn export_strategy(strategy: &Strategy, path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(strategy).map_err(json_to_io_error)?;
    std::fs::write(path, json)
}

/// Loads a strategy from a JSON file (e.g., exported by praxis for braid).
pub fn load_strategy(path: &Path) -> std::io::Result<Strategy> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data).map_err(json_to_io_error)
}

fn json_to_io_error(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn strategy_export_roundtrips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("strategy.json");
        let mut s = Strategy::default();
        s.tool_preferences.insert("rg".into(), 5);
        s.confidence_thresholds.insert("speculate".into(), 0.7);
        export_strategy(&s, &path).unwrap();
        let loaded = load_strategy(&path).unwrap();
        assert_eq!(loaded.tool_preferences["rg"], 5);
        assert!((loaded.confidence_thresholds["speculate"] - 0.7).abs() < f32::EPSILON);
        assert_eq!(loaded.version, s.version);
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let result = load_strategy(Path::new("/tmp/nonexistent-praxis-strategy.json"));
        assert!(result.is_err());
    }

    #[test]
    fn export_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.json");
        assert!(!path.exists());
        export_strategy(&Strategy::default(), &path).unwrap();
        assert!(path.exists());
    }
}
