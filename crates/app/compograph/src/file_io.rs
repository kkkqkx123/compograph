//! Graph file exchange through dialog-chosen paths with scratch fallback.
//!
//! The application resolves locations through the platform file dialogs and
//! calls the path-based helpers below. The fixed scratch paths remain as a
//! fallback when the platform reports no picker capability. The helpers stay
//! small and total: every failure surfaces as a message the status bar can
//! show, never as a panic. They move documents and text only, so the
//! application keeps owning the store and its layout.

use std::path::Path;

use cg_graph::GraphDocument;

/// Scratch JSON file carrying structure, labels, weights, and positions.
pub const JSON_PATH: &str = "/tmp/compograph-graph.json";

/// Scratch DOT file carrying structure for external tools.
pub const DOT_PATH: &str = "/tmp/compograph-graph.dot";

/// Writes a document as JSON, reporting what was stored.
pub fn export_json_file(document: &GraphDocument, path: &str) -> Result<String, String> {
    export_json_to_path(document, Path::new(path))
}

/// Reads and validates a JSON file produced by [`export_json_file`].
pub fn import_json_file(path: &str) -> Result<GraphDocument, String> {
    import_json_from_path(Path::new(path))
}

/// Writes caller-rendered text such as DOT output.
pub fn write_text_file(content: &str, path: &str) -> Result<(), String> {
    write_text_to_path(content, Path::new(path))
}

/// Writes caller-rendered bytes such as exported PPM images.
pub fn write_bytes_file(content: &[u8], path: &str) -> Result<(), String> {
    write_bytes_to_path(content, Path::new(path))
}

/// Writes a document as JSON to a dialog-chosen path.
pub fn export_json_to_path(document: &GraphDocument, path: &Path) -> Result<String, String> {
    let encoded = cg_graph::export_json(document).map_err(|error| error.message().to_string())?;
    std::fs::write(path, encoded)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(format!(
        "exported {} nodes to {}",
        document.nodes.len(),
        path.display()
    ))
}

/// Reads and validates a JSON file from a dialog-chosen path.
pub fn import_json_from_path(path: &Path) -> Result<GraphDocument, String> {
    let encoded = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    cg_graph::import_json(&encoded).map_err(|error| error.message().to_string())
}

/// Writes caller-rendered text such as DOT output to a dialog-chosen path.
pub fn write_text_to_path(content: &str, path: &Path) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Writes caller-rendered bytes such as PPM output to a dialog-chosen path.
pub fn write_bytes_to_path(content: &[u8], path: &Path) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::{EdgeEntry, NodeEntry};

    fn sample() -> GraphDocument {
        GraphDocument {
            nodes: vec![NodeEntry {
                id: 0,
                label: "kept".into(),
                position: Some([1.0, 2.0]),
            }],
            edges: vec![EdgeEntry {
                source: 0,
                target: 0,
                weight: 1.0,
            }],
        }
    }

    #[test]
    fn scratch_paths_are_absolute() {
        assert!(JSON_PATH.starts_with('/'));
        assert!(DOT_PATH.starts_with('/'));
        assert_ne!(JSON_PATH, DOT_PATH);
    }

    #[test]
    fn json_file_round_trip_through_the_scratch_path() {
        let path = "/tmp/compograph-graph-test.json";
        let document = sample();
        export_json_file(&document, path).expect("export works");
        let decoded = import_json_file(path).expect("import works");
        assert_eq!(decoded, document);
        std::fs::remove_file(path).expect("scratch file is removed");
    }

    #[test]
    fn missing_files_report_readable_errors() {
        assert!(import_json_file("/tmp/compograph-graph-absent.json").is_err());
    }

    #[test]
    fn text_files_round_trip() {
        let path = "/tmp/compograph-graph-test.txt";
        write_text_file("digraph {}", path).expect("write works");
        let back = std::fs::read_to_string(path).expect("read works");
        assert_eq!(back, "digraph {}");
        std::fs::remove_file(path).expect("scratch file is removed");
    }
}
