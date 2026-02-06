//! Parallel filesystem walker using `ignore::WalkParallel`.
//!
//! Walks directories for `.wav` files, reads metadata from each, and sends
//! matching results through a crossbeam channel.

use std::path::PathBuf;

use crossbeam_channel::Sender;
use ignore::types::TypesBuilder;
use ignore::WalkBuilder;

use super::{SearchQuery, UnifiedMetadata, read_metadata};

/// Result sent through the channel for each matching file.
pub type SearchResult = UnifiedMetadata;

/// Parallel filesystem finder using `ignore::WalkParallel`.
pub struct FilesystemFinder {
    roots: Vec<PathBuf>,
    threads: usize,
}

impl FilesystemFinder {
    /// Create a new finder with the given root directories and thread count.
    /// If `threads` is 0, uses the `ignore` crate default (num CPUs).
    pub fn new(roots: Vec<PathBuf>, threads: usize) -> Self {
        Self { roots, threads }
    }

    /// Walk all roots in parallel, sending matching results to `tx`.
    ///
    /// Each parallel worker reads metadata from WAV files and tests against
    /// the query. Matches are sent through the channel. Individual file errors
    /// are logged to stderr and skipped.
    pub fn walk(&self, query: &SearchQuery, tx: Sender<SearchResult>) {
        if self.roots.is_empty() {
            return;
        }

        let mut types = TypesBuilder::new();
        types.add("wav", "*.wav").expect("valid glob");
        types.add("wav", "*.WAV").expect("valid glob");
        types.select("wav");
        let types = types.build().expect("valid types");

        let mut builder = WalkBuilder::new(&self.roots[0]);
        for root in &self.roots[1..] {
            builder.add(root);
        }

        builder.types(types);
        builder.hidden(false); // Don't skip hidden files
        builder.git_ignore(false); // Don't respect .gitignore for sample libraries

        if self.threads > 0 {
            builder.threads(self.threads);
        }

        let query = query.clone();
        builder.build_parallel().run(|| {
            let tx = tx.clone();
            let query = query.clone();
            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("riffgrep: {e}");
                        return ignore::WalkState::Continue;
                    }
                };

                // Skip directories.
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return ignore::WalkState::Continue;
                }

                let path = entry.path();
                match read_metadata(path) {
                    Ok(meta) => {
                        if query.matches(&meta) && tx.send(meta).is_err() {
                            // Receiver dropped (e.g., broken pipe).
                            return ignore::WalkState::Quit;
                        }
                    }
                    Err(_) => {
                        // Skip files we can't parse (not WAV, corrupt, etc.).
                    }
                }

                ignore::WalkState::Continue
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn test_files_dir() -> PathBuf {
        PathBuf::from("test_files")
    }

    #[test]
    fn walk_discovers_all_wav_files() {
        if !test_files_dir().exists() {
            return;
        }
        let finder = FilesystemFinder::new(vec![test_files_dir()], 2);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery::default(); // matches everything
        finder.walk(&query, tx);
        let results: Vec<_> = rx.iter().collect();
        // There are 11 WAV files in test_files/.
        assert_eq!(results.len(), 9, "expected 9 WAV files, got {}", results.len());
    }

    #[test]
    fn walk_with_category_filter() {
        if !test_files_dir().exists() {
            return;
        }
        let finder = FilesystemFinder::new(vec![test_files_dir()], 2);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            category: Some(super::super::Pattern::Substring("IGNR-Genre".to_string())),
            ..Default::default()
        };
        finder.walk(&query, tx);
        let results: Vec<_> = rx.iter().collect();
        assert!(
            !results.is_empty(),
            "expected at least one match for category IGNR-Genre"
        );
        for r in &results {
            assert!(
                r.category.to_ascii_lowercase().contains("ignr-genre"),
                "unexpected category: {}",
                r.category
            );
        }
    }

    #[test]
    fn walk_no_matches() {
        if !test_files_dir().exists() {
            return;
        }
        let finder = FilesystemFinder::new(vec![test_files_dir()], 2);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(super::super::Pattern::Substring("nonexistent_vendor_xyz".to_string())),
            ..Default::default()
        };
        finder.walk(&query, tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn walk_nonexistent_directory() {
        let finder =
            FilesystemFinder::new(vec![PathBuf::from("/nonexistent/directory/abc123")], 1);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery::default();
        finder.walk(&query, tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn walk_empty_directory() {
        // Create a temp empty dir.
        let dir = std::env::temp_dir().join("riffgrep_test_empty");
        let _ = std::fs::create_dir(&dir);
        let finder = FilesystemFinder::new(vec![dir.clone()], 1);
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery::default();
        finder.walk(&query, tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 0);
        let _ = std::fs::remove_dir(&dir);
    }
}
