//! File marking/selection system with persistent storage.
//!
//! Provides a `MarkStore` trait with SQLite and CSV implementations for
//! tracking marked files across sessions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Abstraction for mark storage backends.
pub trait MarkStore: Send + Sync {
    /// Mark a file path.
    fn mark(&self, path: &Path) -> anyhow::Result<()>;
    /// Unmark a file path.
    fn unmark(&self, path: &Path) -> anyhow::Result<()>;
    /// Check if a path is marked.
    fn is_marked(&self, path: &Path) -> bool;
    /// Get all marked paths.
    fn marked_paths(&self) -> anyhow::Result<Vec<PathBuf>>;
    /// Clear all marks. Returns the number cleared.
    fn clear_all(&self) -> anyhow::Result<usize>;
    /// Number of currently marked files.
    fn mark_count(&self) -> usize;
}

/// SQLite-backed mark store wrapping `Database` mark methods.
pub struct SqliteMarkStore {
    db_path: PathBuf,
}

impl SqliteMarkStore {
    /// Create a new SQLite mark store.
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }
}

impl MarkStore for SqliteMarkStore {
    fn mark(&self, path: &Path) -> anyhow::Result<()> {
        let db = super::sqlite::Database::open(&self.db_path)?;
        db.mark_path(&path.to_string_lossy())
    }

    fn unmark(&self, path: &Path) -> anyhow::Result<()> {
        let db = super::sqlite::Database::open(&self.db_path)?;
        db.unmark_path(&path.to_string_lossy())
    }

    fn is_marked(&self, path: &Path) -> bool {
        let db = match super::sqlite::Database::open(&self.db_path) {
            Ok(db) => db,
            Err(_) => return false,
        };
        db.is_marked(&path.to_string_lossy()).unwrap_or(false)
    }

    fn marked_paths(&self) -> anyhow::Result<Vec<PathBuf>> {
        let db = super::sqlite::Database::open(&self.db_path)?;
        db.marked_paths()
    }

    fn clear_all(&self) -> anyhow::Result<usize> {
        let db = super::sqlite::Database::open(&self.db_path)?;
        db.clear_all_marks()
    }

    fn mark_count(&self) -> usize {
        self.marked_paths().map(|v| v.len()).unwrap_or(0)
    }
}

/// CSV-backed mark store: in-memory `HashSet` flushed to a CSV file.
pub struct CsvMarkStore {
    path: PathBuf,
    marks: Mutex<HashSet<PathBuf>>,
}

impl CsvMarkStore {
    /// Create or load a CSV mark store from the given file path.
    pub fn new(path: PathBuf) -> Self {
        let marks = Self::load_from_file(&path);
        Self {
            path,
            marks: Mutex::new(marks),
        }
    }

    /// Load marks from a CSV file (one path per line).
    fn load_from_file(path: &Path) -> HashSet<PathBuf> {
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return HashSet::new(),
        };
        contents
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect()
    }

    /// Flush marks to the CSV file.
    fn flush(&self, marks: &HashSet<PathBuf>) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut lines: Vec<String> = marks
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        lines.sort();
        std::fs::write(&self.path, lines.join("\n") + "\n")?;
        Ok(())
    }
}

impl MarkStore for CsvMarkStore {
    fn mark(&self, path: &Path) -> anyhow::Result<()> {
        let mut marks = self.marks.lock().unwrap();
        marks.insert(path.to_path_buf());
        self.flush(&marks)
    }

    fn unmark(&self, path: &Path) -> anyhow::Result<()> {
        let mut marks = self.marks.lock().unwrap();
        marks.remove(path);
        self.flush(&marks)
    }

    fn is_marked(&self, path: &Path) -> bool {
        self.marks.lock().unwrap().contains(path)
    }

    fn marked_paths(&self) -> anyhow::Result<Vec<PathBuf>> {
        let marks = self.marks.lock().unwrap();
        let mut paths: Vec<PathBuf> = marks.iter().cloned().collect();
        paths.sort();
        Ok(paths)
    }

    fn clear_all(&self) -> anyhow::Result<usize> {
        let mut marks = self.marks.lock().unwrap();
        let count = marks.len();
        marks.clear();
        self.flush(&marks)?;
        Ok(count)
    }

    fn mark_count(&self) -> usize {
        self.marks.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_mark_store_roundtrip() {
        let dir = std::env::temp_dir().join("riffgrep_test_csv_marks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("marks.csv");

        let store = CsvMarkStore::new(path.clone());
        store.mark(Path::new("/a.wav")).unwrap();
        store.mark(Path::new("/b.wav")).unwrap();
        assert!(store.is_marked(Path::new("/a.wav")));
        assert!(!store.is_marked(Path::new("/c.wav")));
        assert_eq!(store.mark_count(), 2);

        store.unmark(Path::new("/a.wav")).unwrap();
        assert!(!store.is_marked(Path::new("/a.wav")));
        assert_eq!(store.mark_count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_csv_mark_store_persistence() {
        let dir = std::env::temp_dir().join("riffgrep_test_csv_persist");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("marks.csv");

        {
            let store = CsvMarkStore::new(path.clone());
            store.mark(Path::new("/x.wav")).unwrap();
            store.mark(Path::new("/y.wav")).unwrap();
        }

        // Reload from file.
        let store2 = CsvMarkStore::new(path);
        assert!(store2.is_marked(Path::new("/x.wav")));
        assert!(store2.is_marked(Path::new("/y.wav")));
        assert_eq!(store2.mark_count(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_mark_store_clear_all() {
        let dir = std::env::temp_dir().join("riffgrep_test_csv_clear");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("marks.csv");

        let store = CsvMarkStore::new(path);
        store.mark(Path::new("/a.wav")).unwrap();
        store.mark(Path::new("/b.wav")).unwrap();
        let cleared = store.clear_all().unwrap();
        assert_eq!(cleared, 2);
        assert_eq!(store.mark_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
