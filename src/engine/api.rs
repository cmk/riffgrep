//! Headless programmatic API for riffgrep.
//!
//! Provides the core search, similarity, metadata, and indexing operations
//! without CLI argument parsing or stdout formatting. Designed for use by
//! MCP tool wrappers (driver-rfg in stdio) and integration tests.
//!
//! These functions are consumed by external crates (driver-rfg) via the
//! lib crate, not by the binary — allow dead_code at module level.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::similarity::SimilarityResult;
use super::sqlite::Database;
use super::{
    SearchQuery, UnifiedMetadata, filesystem::FilesystemFinder, pq, read_metadata, similarity,
};

/// Statistics returned from an indexing operation.
#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    /// Number of files successfully indexed.
    pub files_indexed: usize,
    /// Number of files that failed to index.
    pub errors: usize,
    /// Path to the database file.
    pub db_path: PathBuf,
}

/// Search for audio files matching a query.
///
/// Uses the SQLite index if `db_path` exists, otherwise falls back to
/// a filesystem walk over `roots`.
///
/// Returns up to `limit` matching results (0 = unlimited).
pub fn search(
    query: &SearchQuery,
    db_path: Option<&Path>,
    roots: &[PathBuf],
    limit: usize,
) -> anyhow::Result<Vec<UnifiedMetadata>> {
    let db_path = match db_path {
        Some(p) if p.exists() => Some(p),
        _ => None,
    };

    let (tx, rx) = crossbeam_channel::bounded(2048);

    if let Some(db_path) = db_path {
        let db = Database::open(db_path)?;
        db.search(query, &tx);
        drop(tx); // Close channel so rx.iter() terminates.
    } else {
        anyhow::ensure!(!roots.is_empty(), "no search roots provided");
        let finder = FilesystemFinder::new(roots.to_vec(), 0);
        finder.walk(query, tx); // walk() consumes tx, closing the channel.
    }

    let mut results: Vec<UnifiedMetadata> = rx.iter().collect();
    if limit > 0 {
        results.truncate(limit);
    }
    Ok(results)
}

/// Find files similar to a reference file using embedding similarity.
///
/// Tries PQ-accelerated search if a codebook is available in the DB,
/// otherwise falls back to brute-force L2.
pub fn similar(
    db_path: &Path,
    query_path: &Path,
    limit: usize,
) -> anyhow::Result<Vec<SimilarityResult>> {
    let db = Database::open(db_path)?;

    let query_path = query_path
        .canonicalize()
        .unwrap_or_else(|_| query_path.to_path_buf());
    let query_str = query_path.to_string_lossy();
    let query_embedding = db
        .load_embedding(&query_str)?
        .ok_or_else(|| anyhow::anyhow!("file not embedded: {query_str}"))?;

    // Try PQ-accelerated search first.
    if let Ok(Some(codebook_blob)) = db.get_metadata("pq_codebook") {
        let pq = pq::ProductQuantizer::from_bytes(&codebook_blob)?;
        let all_embeddings = db.load_all_embeddings()?;
        let codes: Vec<(i64, [u8; pq::M])> = all_embeddings
            .iter()
            .map(|(id, _, vec)| (*id, pq.encode(vec)))
            .collect();

        let scored = pq.search(&query_embedding, &codes, limit + 1);

        let path_map: std::collections::HashMap<i64, &Path> = all_embeddings
            .iter()
            .map(|(id, p, _)| (*id, p.as_path()))
            .collect();

        let query_id = all_embeddings
            .iter()
            .find(|(_, p, _)| p.as_os_str() == query_path.as_os_str())
            .map(|(id, _, _)| *id)
            .unwrap_or(-1);

        let max_dist = scored.last().map(|(_, d)| d.sqrt()).unwrap_or(0.0);
        let mut results = Vec::with_capacity(scored.len() + 1);

        results.push(SimilarityResult {
            id: query_id,
            path: query_path.clone(),
            dist: 0.0,
            sim: 1.0,
        });

        for (id, sq_dist) in &scored {
            if *id == query_id {
                continue;
            }
            let dist = sq_dist.sqrt();
            let sim = similarity::similarity_score(dist, max_dist);
            let path = path_map
                .get(id)
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            results.push(SimilarityResult {
                id: *id,
                path,
                dist,
                sim,
            });
        }
        results.truncate(limit);
        Ok(results)
    } else {
        // Brute-force L2 fallback.
        let candidates = db.load_all_embeddings()?;
        let query_id = candidates
            .iter()
            .find(|(_, p, _)| p.as_os_str() == query_path.as_os_str())
            .map(|(id, _, _)| *id)
            .unwrap_or(-1);
        Ok(similarity::search_similar(
            query_id,
            &query_embedding,
            &candidates,
            limit,
        ))
    }
}

/// Read metadata for a single audio file.
pub fn metadata(path: &Path) -> anyhow::Result<UnifiedMetadata> {
    read_metadata(path).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Build or update the search index.
///
/// Scans `roots` for audio files, reads metadata, and inserts into the
/// SQLite database at `db_path`.
pub fn index(roots: &[PathBuf], db_path: &Path) -> anyhow::Result<IndexStats> {
    use super::source::AudioRegistry;

    anyhow::ensure!(!roots.is_empty(), "no index roots provided");

    let db = Database::open(db_path)?;
    let registry = AudioRegistry::new();

    let mut files_indexed = 0usize;
    let mut errors = 0usize;

    for root in roots {
        let types = super::filesystem::build_audio_types(&registry);
        let mut builder = ignore::WalkBuilder::new(root);
        builder.types(types);
        builder.hidden(false);
        builder.git_ignore(false);

        for entry in builder.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    errors += 1;
                    continue;
                }
            };

            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();
            match read_metadata(path) {
                Ok(meta) => {
                    let mtime = super::sqlite::file_mtime(path).unwrap_or(0);
                    if db.insert_batch(&[(meta, mtime, None)]).is_err() {
                        errors += 1;
                    } else {
                        files_indexed += 1;
                    }
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }
    }

    Ok(IndexStats {
        files_indexed,
        errors,
        db_path: db_path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_files_exist() -> bool {
        Path::new("test_files").exists()
    }

    #[test]
    fn metadata_reads_wav() {
        if !test_files_exist() {
            return;
        }
        let meta = metadata(Path::new("test_files/clean_base.wav")).unwrap();
        assert!(!meta.path.as_os_str().is_empty());
    }

    #[test]
    fn metadata_errors_on_missing() {
        let result = metadata(Path::new("/nonexistent/file.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn search_filesystem_returns_results() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let roots = vec![PathBuf::from("test_files")];
        let results = search(&query, None, &roots, 0).unwrap();
        assert!(!results.is_empty(), "should find WAV files in test_files/");
    }

    #[test]
    fn search_with_limit() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let roots = vec![PathBuf::from("test_files")];
        let results = search(&query, None, &roots, 3).unwrap();
        assert!(results.len() <= 3);
    }

    #[test]
    fn search_no_roots_errors() {
        let query = SearchQuery::default();
        let result = search(&query, None, &[], 0);
        assert!(result.is_err());
    }

    #[test]
    fn index_and_search_roundtrip() {
        if !test_files_exist() {
            return;
        }
        let db_path = std::env::temp_dir().join("riffgrep_api_test_index.db");
        let _ = std::fs::remove_file(&db_path);

        let roots = vec![PathBuf::from("test_files")];
        let stats = index(&roots, &db_path).unwrap();
        assert!(stats.files_indexed > 0, "should index some files");

        let query = SearchQuery::default();
        let results = search(&query, Some(&db_path), &[], 0).unwrap();
        assert_eq!(results.len(), stats.files_indexed);

        let _ = std::fs::remove_file(&db_path);
    }
}
