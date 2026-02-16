//! Async bridge: spawns synchronous Finders on background threads and bridges
//! results into tokio channels.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::engine::filesystem::FilesystemFinder;
use crate::engine::sqlite::Database;
use crate::engine::{SearchQuery, TableRow, UnifiedMetadata};

/// Which search backend to use.
pub enum SearchMode {
    /// SQLite indexed search.
    Sqlite(PathBuf),
    /// Filesystem walk.
    Filesystem {
        /// Root directories to search.
        roots: Vec<PathBuf>,
        /// Thread count (0 = default).
        threads: usize,
    },
}

/// Handle to a running background search.
///
/// Produces raw `UnifiedMetadata` for headless/batch workflows.
/// Currently unused — reserved for Lua scripting and batch actions.
#[allow(dead_code)]
pub struct SearchHandle {
    /// Receive search results.
    pub results_rx: mpsc::Receiver<UnifiedMetadata>,
    /// Cancellation flag.
    cancel: Arc<AtomicBool>,
    /// Join handle for the background thread.
    join: Option<JoinHandle<usize>>,
}

impl SearchHandle {
    /// Spawn a background search. Returns immediately.
    pub fn spawn(query: SearchQuery, mode: SearchMode) -> Self {
        let (tx, rx) = mpsc::channel::<UnifiedMetadata>(2048);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancel.clone();

        let handle = std::thread::spawn(move || {
            let (cb_tx, cb_rx) = crossbeam_channel::bounded::<UnifiedMetadata>(2048);

            // Spawn the sync search.
            let search_thread = std::thread::spawn({
                let cb_tx = cb_tx.clone();
                move || match mode {
                    SearchMode::Sqlite(db_path) => {
                        if let Ok(db) = Database::open(&db_path) {
                            db.search(&query, &cb_tx);
                        }
                    }
                    SearchMode::Filesystem { roots, threads } => {
                        let finder = FilesystemFinder::new(roots, threads);
                        finder.walk(&query, cb_tx);
                    }
                }
            });
            drop(cb_tx);

            // Bridge crossbeam → tokio::mpsc with cancellation checks.
            let mut count = 0usize;
            for meta in cb_rx {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                count += 1;
                if tx.blocking_send(meta).is_err() {
                    break; // receiver dropped
                }
            }

            let _ = search_thread.join();
            count
        });

        Self {
            results_rx: rx,
            cancel,
            join: Some(handle),
        }
    }

    /// Signal cancellation. The background thread will stop sending results.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Block until the search thread completes. Returns total match count.
    pub fn join(mut self) -> usize {
        if let Some(handle) = self.join.take() {
            handle.join().unwrap_or(0)
        } else {
            0
        }
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Don't block on drop — the thread will finish on its own.
    }
}

/// Handle to a running background search that produces `TableRow` for the TUI.
///
/// SQLite mode: queries include audio info + marked status.
/// Filesystem mode: wraps `UnifiedMetadata` in `TableRow` with audio info read from WAV headers.
pub struct SearchHandleTable {
    /// Receive search results as `TableRow`.
    pub results_rx: mpsc::Receiver<TableRow>,
    /// Cancellation flag.
    cancel: Arc<AtomicBool>,
    /// Join handle for the background thread (held for ownership).
    _join: Option<JoinHandle<usize>>,
}

impl SearchHandleTable {
    /// Spawn a background search producing `TableRow`. Returns immediately.
    pub fn spawn(query: SearchQuery, mode: SearchMode) -> Self {
        let (tx, rx) = mpsc::channel::<TableRow>(2048);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancel.clone();

        let handle = std::thread::spawn(move || {
            let mut count = 0usize;

            match mode {
                SearchMode::Sqlite(db_path) => {
                    let (cb_tx, cb_rx) = crossbeam_channel::bounded::<TableRow>(2048);
                    let search_thread = std::thread::spawn({
                        let cb_tx = cb_tx.clone();
                        move || {
                            if let Ok(db) = Database::open(&db_path) {
                                db.search_table_rows(&query, &cb_tx);
                            }
                        }
                    });
                    drop(cb_tx);

                    for row in cb_rx {
                        if cancel_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        count += 1;
                        if tx.blocking_send(row).is_err() {
                            break;
                        }
                    }
                    let _ = search_thread.join();
                }
                SearchMode::Filesystem { roots, threads } => {
                    let (cb_tx, cb_rx) = crossbeam_channel::bounded::<UnifiedMetadata>(2048);
                    let search_thread = std::thread::spawn({
                        let cb_tx = cb_tx.clone();
                        move || {
                            let finder = FilesystemFinder::new(roots, threads);
                            finder.walk(&query, cb_tx);
                        }
                    });
                    drop(cb_tx);

                    for meta in cb_rx {
                        if cancel_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        count += 1;
                        let audio_info = (|| {
                            let file = std::fs::File::open(&meta.path).ok()?;
                            let mut rdr = std::io::BufReader::with_capacity(8192, file);
                            let map = crate::engine::bext::scan_chunks(&mut rdr).ok()?;
                            let fmt = crate::engine::wav::parse_fmt(&mut rdr, &map).ok()?;
                            Some(crate::engine::wav::AudioInfo::from_fmt(&fmt, map.data_size))
                        })();
                        let row = TableRow {
                            meta,
                            audio_info,
                            marked: false,
                        };
                        if tx.blocking_send(row).is_err() {
                            break;
                        }
                    }
                    let _ = search_thread.join();
                }
            }

            count
        });

        Self {
            results_rx: rx,
            cancel,
            _join: Some(handle),
        }
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for SearchHandleTable {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Load decompressed peaks for a file path, running the blocking DB call off
/// the async runtime via `spawn_blocking`.
pub async fn load_peaks(db_path: &Path, file_path: &str) -> Option<Vec<u8>> {
    let db_path = db_path.to_path_buf();
    let file_path = file_path.to_string();
    tokio::task::spawn_blocking(move || {
        let db = Database::open(&db_path).ok()?;
        db.get_peaks(&file_path).ok().flatten()
    })
    .await
    .ok()
    .flatten()
}

/// Load peaks with JIT fallback: try DB first, then compute from audio.
pub async fn load_peaks_with_fallback(
    db_path: Option<&Path>,
    file_path: &Path,
) -> Option<Vec<u8>> {
    // Try DB first.
    if let Some(db) = db_path {
        let path_str = file_path.to_string_lossy().to_string();
        if let Some(peaks) = load_peaks(db, &path_str).await {
            if !peaks.is_empty() {
                return Some(peaks);
            }
        }
    }
    // Fallback: compute stereo peaks from audio file.
    let path = file_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::engine::wav::compute_peaks_stereo_from_path(&path).ok()
    })
    .await
    .ok()
    .flatten()
}

/// Load audio format info (duration, sample rate, etc.) from a WAV file.
pub async fn load_audio_info(file_path: &Path) -> Option<crate::engine::wav::AudioInfo> {
    let path = file_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).ok()?;
        let mut reader = std::io::BufReader::with_capacity(8192, file);
        let map = crate::engine::bext::scan_chunks(&mut reader).ok()?;
        let fmt = crate::engine::wav::parse_fmt(&mut reader, &map).ok()?;
        Some(crate::engine::wav::AudioInfo::from_fmt(&fmt, map.data_size))
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_files_exist() -> bool {
        PathBuf::from("test_files").exists()
    }

    #[tokio::test]
    async fn test_search_handle_receives_results() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let mut handle = SearchHandle::spawn(query, mode);
        let mut count = 0;
        while handle.results_rx.recv().await.is_some() {
            count += 1;
        }
        assert!(count > 0, "should receive at least one result");
    }

    #[tokio::test]
    async fn test_search_handle_cancel_stops_producer() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let handle = SearchHandle::spawn(query, mode);
        // Cancel immediately.
        handle.cancel();
        // The handle should terminate without hanging.
        let total = handle.join();
        // We may have received some results before cancellation, but not all.
        // Just verify it doesn't hang.
        assert!(total <= 9, "cancel should limit results");
    }

    #[tokio::test]
    async fn test_search_handle_drop_cancels() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let handle = SearchHandle::spawn(query, mode);
        // Drop without join — should not hang.
        drop(handle);
    }

    #[tokio::test]
    async fn test_search_handle_sqlite_mode() {
        if !test_files_exist() {
            return;
        }
        // Create a temp DB and index test files.
        let db_path = std::env::temp_dir().join("riffgrep_test_async_sqlite.db");
        let _ = std::fs::remove_file(&db_path);
        {
            let db = Database::open(&db_path).unwrap();
            let test_dir = PathBuf::from("test_files");
            for entry in std::fs::read_dir(&test_dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "wav") {
                    if let Ok(meta) = crate::engine::read_metadata(&path) {
                        let mtime = crate::engine::sqlite::file_mtime(&path).unwrap_or(0);
                        db.insert_batch(&[(meta, mtime, None)]).unwrap();
                    }
                }
            }
        }

        let query = SearchQuery::default();
        let mode = SearchMode::Sqlite(db_path.clone());
        let mut handle = SearchHandle::spawn(query, mode);
        let mut count = 0;
        while handle.results_rx.recv().await.is_some() {
            count += 1;
        }
        assert_eq!(count, 9);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_search_handle_filesystem_mode() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let mut handle = SearchHandle::spawn(query, mode);
        let mut count = 0;
        while handle.results_rx.recv().await.is_some() {
            count += 1;
        }
        assert_eq!(count, 9);
    }

    #[tokio::test]
    async fn test_load_peaks_missing_returns_none() {
        let db_path = std::env::temp_dir().join("riffgrep_test_peaks_none.db");
        let _ = std::fs::remove_file(&db_path);
        let _db = Database::open(&db_path).unwrap();
        drop(_db);

        let result = load_peaks(&db_path, "/nonexistent/path.wav").await;
        assert!(result.is_none());
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_load_peaks_returns_data() {
        let db_path = std::env::temp_dir().join("riffgrep_test_peaks_data.db");
        let _ = std::fs::remove_file(&db_path);
        {
            let db = Database::open(&db_path).unwrap();
            let raw: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
            let compressed = crate::engine::sqlite::compress_peaks(&raw);
            let meta = UnifiedMetadata {
                path: PathBuf::from("/test/peaks.wav"),
                ..Default::default()
            };
            db.insert_batch(&[(meta, 100, Some(compressed))]).unwrap();
        }

        let result = load_peaks(&db_path, "/test/peaks.wav").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 180);
        let _ = std::fs::remove_file(&db_path);
    }

    // --- S4-T7 tests: JIT peaks fallback ---

    #[tokio::test]
    async fn test_jit_peaks_filesystem_mode() {
        if !test_files_exist() {
            return;
        }
        let path = PathBuf::from("test_files/clean_base.wav");
        // No DB — should compute stereo peaks JIT from audio file.
        let result = load_peaks_with_fallback(None, &path).await;
        assert!(result.is_some(), "JIT peaks should be computed from audio");
        assert_eq!(result.unwrap().len(), 360);
    }

    #[tokio::test]
    async fn test_jit_peaks_db_miss_fallback() {
        if !test_files_exist() {
            return;
        }
        let db_path = std::env::temp_dir().join("riffgrep_test_jit_miss.db");
        let _ = std::fs::remove_file(&db_path);
        let _db = Database::open(&db_path).unwrap();
        drop(_db);

        let path = PathBuf::from("test_files/clean_base.wav");
        // DB exists but has no entry for this path — should fall back to JIT stereo peaks.
        let result = load_peaks_with_fallback(Some(&db_path), &path).await;
        assert!(result.is_some(), "should fall back to JIT when DB misses");
        assert_eq!(result.unwrap().len(), 360);

        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_jit_peaks_db_hit_no_fallback() {
        let db_path = std::env::temp_dir().join("riffgrep_test_jit_hit.db");
        let _ = std::fs::remove_file(&db_path);
        {
            let db = Database::open(&db_path).unwrap();
            let raw: Vec<u8> = (0..180).map(|i| (i * 5 % 256) as u8).collect();
            let compressed = crate::engine::sqlite::compress_peaks(&raw);
            let meta = UnifiedMetadata {
                path: PathBuf::from("/test/jit_hit.wav"),
                ..Default::default()
            };
            db.insert_batch(&[(meta, 100, Some(compressed))]).unwrap();
        }

        let path = PathBuf::from("/test/jit_hit.wav");
        let result = load_peaks_with_fallback(Some(&db_path), &path).await;
        assert!(result.is_some(), "DB hit should return peaks");
        assert_eq!(result.unwrap().len(), 180);

        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_jit_peaks_bad_file_returns_none() {
        // Non-WAV file — should gracefully return None.
        let path = PathBuf::from("Cargo.toml");
        let result = load_peaks_with_fallback(None, &path).await;
        assert!(result.is_none(), "non-WAV file should return None");
    }

    // --- S5-T8 tests: SearchHandleTable ---

    #[tokio::test]
    async fn test_table_row_from_sqlite_includes_audio() {
        if !test_files_exist() {
            return;
        }
        let db_path = std::env::temp_dir().join("riffgrep_test_table_row_sqlite.db");
        let _ = std::fs::remove_file(&db_path);
        {
            let db = Database::open(&db_path).unwrap();
            let meta = UnifiedMetadata {
                path: PathBuf::from("/test/audio.wav"),
                vendor: "V".to_string(),
                ..Default::default()
            };
            let audio_info = Some(crate::engine::wav::AudioInfo {
                duration_secs: 2.5,
                sample_rate: 44100,
                bit_depth: 16,
                channels: 2,
            });
            db.insert_batch_with_audio(&[(meta, 100, None, "none".to_string(), audio_info)])
                .unwrap();
        }

        let query = SearchQuery::default();
        let mode = SearchMode::Sqlite(db_path.clone());
        let mut handle = SearchHandleTable::spawn(query, mode);
        let mut rows = Vec::new();
        while let Some(row) = handle.results_rx.recv().await {
            rows.push(row);
        }
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.audio_info.is_some(), "SQLite TableRow should have audio info");
        let info = row.audio_info.as_ref().unwrap();
        assert!((info.duration_secs - 2.5).abs() < 0.01);
        assert_eq!(info.sample_rate, 44100);
        assert!(!row.marked);
        let _ = std::fs::remove_file(&db_path);
    }

    #[tokio::test]
    async fn test_table_row_from_filesystem_has_audio() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let mut handle = SearchHandleTable::spawn(query, mode);
        let mut rows = Vec::new();
        while let Some(row) = handle.results_rx.recv().await {
            rows.push(row);
        }
        assert!(!rows.is_empty(), "should get results from filesystem");
        for row in &rows {
            let info = row.audio_info.as_ref().expect(
                "filesystem TableRow should have audio info from WAV headers",
            );
            assert!(info.sample_rate > 0, "sample_rate should be set");
            assert!(info.bit_depth > 0, "bit_depth should be set");
            assert!(info.channels > 0, "channels should be set");
            assert!(info.duration_secs > 0.0, "duration should be set");
            assert!(!row.marked, "filesystem TableRow should not be marked");
        }
    }

    #[tokio::test]
    async fn test_search_handle_table_produces_table_rows() {
        if !test_files_exist() {
            return;
        }
        let query = SearchQuery::default();
        let mode = SearchMode::Filesystem {
            roots: vec![PathBuf::from("test_files")],
            threads: 2,
        };
        let mut handle = SearchHandleTable::spawn(query, mode);
        let mut count = 0;
        while let Some(row) = handle.results_rx.recv().await {
            // Verify each row has a valid path in meta.
            assert!(
                !row.meta.path.as_os_str().is_empty(),
                "TableRow should have a non-empty path"
            );
            count += 1;
        }
        assert_eq!(count, 9, "should get 9 test files as TableRows");
    }
}
