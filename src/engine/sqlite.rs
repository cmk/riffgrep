//! SQLite FTS5 Trigram indexing and search.
//!
//! Provides the `Database` struct for creating/opening an index, batch-inserting
//! metadata, building parameterized SQL queries, and executing searches. FTS5
//! Trigram tokenization enables sub-10ms substring queries across 1M+ rows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender};
use rusqlite::{Connection, params};

use super::{MatchMode, Pattern, SearchQuery, UnifiedMetadata};

/// Current schema version, tracked via `PRAGMA user_version`.
const SCHEMA_VERSION: u32 = 4;

/// SQL for inserting/replacing a sample row (28 parameters).
const INSERT_SQL: &str = "INSERT OR REPLACE INTO samples (
    path, name, parent_folder,
    vendor, library, category, sound_id,
    description, comment, key, bpm,
    rating, subcategory, genre_id, usage_id,
    umid, recid, mtime, peaks, peaks_source,
    duration, sample_rate, bit_depth, channels,
    date, take, track, item
) VALUES (
    ?1, ?2, ?3,
    ?4, ?5, ?6, ?7,
    ?8, ?9, ?10, ?11,
    ?12, ?13, ?14, ?15,
    ?16, ?17, ?18, ?19, ?20,
    ?21, ?22, ?23, ?24,
    ?25, ?26, ?27, ?28
)";

/// SQLite index database.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create a database at the given path. Applies performance pragmas,
    /// creates the schema idempotently, and migrates from older versions.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        apply_pragmas(&conn)?;
        create_schema(&conn)?;
        migrate(&conn)?;
        register_regexp_udf(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;
        create_schema(&conn)?;
        migrate(&conn)?;
        register_regexp_udf(&conn)?;
        Ok(Self { conn })
    }

    /// Insert a batch of records in a single transaction. Returns the number
    /// of rows inserted.
    pub fn insert_batch(
        &self,
        records: &[(UnifiedMetadata, i64, Option<Vec<u8>>)],
    ) -> anyhow::Result<usize> {
        self.insert_batch_with_source(records, "none")
    }

    /// Insert a batch of records with an explicit peaks_source value.
    pub fn insert_batch_with_source(
        &self,
        records: &[(UnifiedMetadata, i64, Option<Vec<u8>>)],
        peaks_source: &str,
    ) -> anyhow::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(INSERT_SQL)?;

            for (meta, mtime, peaks) in records {
                let name = meta
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let parent = meta
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path_str = meta.path.to_string_lossy();

                stmt.execute(params![
                    path_str.as_ref(),
                    name,
                    parent,
                    meta.vendor,
                    meta.library,
                    meta.category,
                    meta.sound_id,
                    meta.description,
                    meta.comment,
                    meta.key,
                    meta.bpm.map(|v| v as i32),
                    meta.rating,
                    meta.subcategory,
                    meta.genre_id,
                    meta.usage_id,
                    meta.umid,
                    meta.recid as i64,
                    mtime,
                    peaks.as_deref(),
                    peaks_source,
                    Option::<f64>::None,   // duration
                    Option::<i32>::None,   // sample_rate
                    Option::<i32>::None,   // bit_depth
                    Option::<i32>::None,   // channels
                    meta.date,
                    meta.take,
                    meta.track,
                    meta.item,
                ])?;
            }
        }
        let count = records.len();
        tx.commit()?;
        Ok(count)
    }

    /// Consume records from a channel, batch-inserting into the database.
    /// Returns the total number of records inserted.
    pub fn index_writer(
        &self,
        rx: &Receiver<(UnifiedMetadata, i64, Option<Vec<u8>>)>,
        batch_size: usize,
    ) -> anyhow::Result<usize> {
        let mut total = 0;
        let mut batch = Vec::with_capacity(batch_size);

        for item in rx {
            batch.push(item);
            if batch.len() >= batch_size {
                total += self.insert_batch(&batch)?;
                batch.clear();
            }
        }

        // Flush remaining.
        if !batch.is_empty() {
            total += self.insert_batch(&batch)?;
        }

        Ok(total)
    }

    /// Consume records with per-record peaks_source from a channel.
    /// Returns the total number of records inserted.
    pub fn index_writer_with_source(
        &self,
        rx: &Receiver<(UnifiedMetadata, i64, Option<Vec<u8>>, String)>,
        batch_size: usize,
    ) -> anyhow::Result<usize> {
        let mut total = 0;
        let mut batch: Vec<(UnifiedMetadata, i64, Option<Vec<u8>>)> = Vec::with_capacity(batch_size);
        let mut sources: Vec<String> = Vec::with_capacity(batch_size);

        for (meta, mtime, peaks, source) in rx {
            batch.push((meta, mtime, peaks));
            sources.push(source);
            if batch.len() >= batch_size {
                total += self.insert_batch_individually(&batch, &sources)?;
                batch.clear();
                sources.clear();
            }
        }

        if !batch.is_empty() {
            total += self.insert_batch_individually(&batch, &sources)?;
        }

        Ok(total)
    }

    /// Consume records with per-record peaks_source and audio info from a channel.
    /// Returns the total number of records inserted.
    pub fn index_writer_with_audio(
        &self,
        rx: &Receiver<(UnifiedMetadata, i64, Option<Vec<u8>>, String, Option<super::wav::AudioInfo>)>,
        batch_size: usize,
    ) -> anyhow::Result<usize> {
        let mut total = 0;
        let mut batch: Vec<(UnifiedMetadata, i64, Option<Vec<u8>>, String, Option<super::wav::AudioInfo>)> =
            Vec::with_capacity(batch_size);

        for item in rx {
            batch.push(item);
            if batch.len() >= batch_size {
                total += self.insert_batch_with_audio(&batch)?;
                batch.clear();
            }
        }

        if !batch.is_empty() {
            total += self.insert_batch_with_audio(&batch)?;
        }

        Ok(total)
    }

    /// Insert records with per-record peaks_source in a single transaction.
    fn insert_batch_individually(
        &self,
        records: &[(UnifiedMetadata, i64, Option<Vec<u8>>)],
        sources: &[String],
    ) -> anyhow::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(INSERT_SQL)?;

            for (i, (meta, mtime, peaks)) in records.iter().enumerate() {
                let name = meta
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let parent = meta
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path_str = meta.path.to_string_lossy();
                let source = &sources[i];

                stmt.execute(params![
                    path_str.as_ref(),
                    name,
                    parent,
                    meta.vendor,
                    meta.library,
                    meta.category,
                    meta.sound_id,
                    meta.description,
                    meta.comment,
                    meta.key,
                    meta.bpm.map(|v| v as i32),
                    meta.rating,
                    meta.subcategory,
                    meta.genre_id,
                    meta.usage_id,
                    meta.umid,
                    meta.recid as i64,
                    mtime,
                    peaks.as_deref(),
                    source.as_str(),
                    Option::<f64>::None,   // duration
                    Option::<i32>::None,   // sample_rate
                    Option::<i32>::None,   // bit_depth
                    Option::<i32>::None,   // channels
                    meta.date,
                    meta.take,
                    meta.track,
                    meta.item,
                ])?;
            }
        }
        let count = records.len();
        tx.commit()?;
        Ok(count)
    }

    /// Insert records with per-record peaks_source and audio info.
    pub fn insert_batch_with_audio(
        &self,
        records: &[(UnifiedMetadata, i64, Option<Vec<u8>>, String, Option<super::wav::AudioInfo>)],
    ) -> anyhow::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(INSERT_SQL)?;

            for (meta, mtime, peaks, source, audio_info) in records {
                let name = meta
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let parent = meta
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path_str = meta.path.to_string_lossy();

                stmt.execute(params![
                    path_str.as_ref(),
                    name,
                    parent,
                    meta.vendor,
                    meta.library,
                    meta.category,
                    meta.sound_id,
                    meta.description,
                    meta.comment,
                    meta.key,
                    meta.bpm.map(|v| v as i32),
                    meta.rating,
                    meta.subcategory,
                    meta.genre_id,
                    meta.usage_id,
                    meta.umid,
                    meta.recid as i64,
                    mtime,
                    peaks.as_deref(),
                    source.as_str(),
                    audio_info.as_ref().map(|i| i.duration_secs),
                    audio_info.as_ref().map(|i| i.sample_rate as i32),
                    audio_info.as_ref().map(|i| i.bit_depth as i32),
                    audio_info.as_ref().map(|i| i.channels as i32),
                    meta.date,
                    meta.take,
                    meta.track,
                    meta.item,
                ])?;
            }
        }
        let count = records.len();
        tx.commit()?;
        Ok(count)
    }

    /// Get all (path, mtime) pairs from the database for incremental indexing.
    pub fn get_path_mtimes(&self) -> anyhow::Result<HashMap<PathBuf, i64>> {
        let mut stmt = self.conn.prepare("SELECT path, mtime FROM samples")?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let mtime: i64 = row.get(1)?;
            Ok((PathBuf::from(path), mtime))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, mtime) = row?;
            map.insert(path, mtime);
        }
        Ok(map)
    }

    /// Delete rows whose paths are in the given set. Returns the number of
    /// rows deleted.
    pub fn delete_paths(&self, paths: &[&Path]) -> anyhow::Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut deleted = 0;
        {
            let mut stmt =
                tx.prepare_cached("DELETE FROM samples WHERE path = ?1")?;
            for path in paths {
                deleted += stmt.execute(params![path.to_string_lossy().as_ref()])?;
            }
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// Execute a search query and send results through the channel.
    pub fn search(&self, query: &SearchQuery, tx: &Sender<UnifiedMetadata>) {
        let (sql, values) = build_sql(query);

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("riffgrep: SQL error: {e}");
                return;
            }
        };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

        let rows = match stmt.query_map(param_refs.as_slice(), row_to_metadata) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("riffgrep: query error: {e}");
                return;
            }
        };

        for row in rows {
            match row {
                Ok(meta) => {
                    if tx.send(meta).is_err() {
                        return; // receiver dropped
                    }
                }
                Err(e) => {
                    eprintln!("riffgrep: row error: {e}");
                }
            }
        }
    }

    /// Execute a search query and send TableRow results through the channel.
    /// Includes audio info and marked status from the database.
    pub fn search_table_rows(
        &self,
        query: &SearchQuery,
        tx: &Sender<super::TableRow>,
    ) {
        let (sql, values) = build_sql(query);

        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("riffgrep: SQL error: {e}");
                return;
            }
        };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            values.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

        let rows = match stmt.query_map(param_refs.as_slice(), row_to_table_row) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("riffgrep: query error: {e}");
                return;
            }
        };

        for row in rows {
            match row {
                Ok(table_row) => {
                    if tx.send(table_row).is_err() {
                        return; // receiver dropped
                    }
                }
                Err(e) => {
                    eprintln!("riffgrep: row error: {e}");
                }
            }
        }
    }

    /// Get aggregate statistics about the database.
    pub fn stats(&self) -> anyhow::Result<DbStats> {
        let file_count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))?;

        let last_mtime: Option<i64> = self
            .conn
            .query_row("SELECT MAX(mtime) FROM samples", [], |r| r.get(0))?;

        let mut vendor_stmt = self.conn.prepare(
            "SELECT vendor, COUNT(*) as cnt FROM samples
             WHERE vendor != '' GROUP BY vendor ORDER BY cnt DESC LIMIT 10",
        )?;
        let vendor_rows = vendor_stmt.query_map([], |row| {
            let vendor: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((vendor, count))
        })?;
        let mut top_vendors = Vec::new();
        for row in vendor_rows {
            top_vendors.push(row?);
        }

        // Peaks breakdown by source.
        let mut peaks_stmt = self.conn.prepare(
            "SELECT peaks_source, COUNT(*) as cnt FROM samples GROUP BY peaks_source ORDER BY cnt DESC",
        )?;
        let peaks_rows = peaks_stmt.query_map([], |row| {
            let source: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((source, count))
        })?;
        let mut peaks_breakdown = Vec::new();
        for row in peaks_rows {
            peaks_breakdown.push(row?);
        }

        Ok(DbStats {
            file_count: file_count as u64,
            last_mtime,
            top_vendors,
            peaks_breakdown,
        })
    }

    /// Check if sample paths exist on the filesystem. Returns the number of
    /// stale (non-existent) paths out of the sampled set.
    pub fn check_staleness(&self, sample_size: usize) -> anyhow::Result<(usize, usize)> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM samples ORDER BY RANDOM() LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![sample_size as i64], |row| {
            let path: String = row.get(0)?;
            Ok(PathBuf::from(path))
        })?;

        let mut total = 0;
        let mut stale = 0;
        for row in rows {
            let path = row?;
            total += 1;
            if !path.exists() {
                stale += 1;
            }
        }
        Ok((stale, total))
    }

    /// Retrieve decompressed peaks for a file path. Returns None if the path
    /// is not in the database or has no peaks stored.
    pub fn get_peaks(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let blob: Option<Vec<u8>> = self.conn.query_row(
            "SELECT peaks FROM samples WHERE path = ?1",
            params![path],
            |row| row.get(0),
        ).unwrap_or(None);

        Ok(blob.map(|b| decompress_peaks(&b)))
    }

    /// Mark a file path.
    pub fn mark_path(&self, path: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE samples SET marked = 1 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// Unmark a file path.
    pub fn unmark_path(&self, path: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE samples SET marked = 0 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }

    /// Check if a path is marked.
    pub fn is_marked(&self, path: &str) -> anyhow::Result<bool> {
        let marked: i32 = self
            .conn
            .query_row(
                "SELECT marked FROM samples WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(marked != 0)
    }

    /// Get all marked paths.
    pub fn marked_paths(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM samples WHERE marked = 1 ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            Ok(PathBuf::from(path))
        })?;
        let mut paths = Vec::new();
        for row in rows {
            paths.push(row?);
        }
        Ok(paths)
    }

    /// Clear all marks. Returns the number cleared.
    pub fn clear_all_marks(&self) -> anyhow::Result<usize> {
        let count = self
            .conn
            .execute("UPDATE samples SET marked = 0 WHERE marked = 1", [])?;
        Ok(count)
    }

    /// Borrow the underlying connection (for testing).
    #[cfg(test)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Database statistics.
pub struct DbStats {
    /// Total number of indexed files.
    pub file_count: u64,
    /// Maximum mtime (most recent indexing).
    pub last_mtime: Option<i64>,
    /// Top vendors by count.
    pub top_vendors: Vec<(String, i64)>,
    /// Peaks count by source (e.g., "none", "generated", "riffgrep_u8", "bwf_reserved").
    pub peaks_breakdown: Vec<(String, i64)>,
}

// --- Schema ---

fn apply_pragmas(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA mmap_size = 2147483648;
         PRAGMA cache_size = -102400;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

fn create_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS samples (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            parent_folder TEXT NOT NULL,
            vendor TEXT NOT NULL DEFAULT '',
            library TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT '',
            sound_id TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            comment TEXT NOT NULL DEFAULT '',
            key TEXT NOT NULL DEFAULT '',
            bpm INTEGER,
            rating TEXT NOT NULL DEFAULT '',
            subcategory TEXT NOT NULL DEFAULT '',
            genre_id TEXT NOT NULL DEFAULT '',
            usage_id TEXT NOT NULL DEFAULT '',
            umid TEXT NOT NULL DEFAULT '',
            recid INTEGER NOT NULL DEFAULT 0,
            mtime INTEGER NOT NULL,
            peaks BLOB,
            peaks_source TEXT NOT NULL DEFAULT 'none',
            marked INTEGER NOT NULL DEFAULT 0,
            duration REAL,
            sample_rate INTEGER,
            bit_depth INTEGER,
            channels INTEGER,
            date TEXT NOT NULL DEFAULT '',
            take TEXT NOT NULL DEFAULT '',
            track TEXT NOT NULL DEFAULT '',
            item TEXT NOT NULL DEFAULT ''
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS samples_fts USING fts5(
            vendor, library, category, sound_id,
            description, comment, key, name,
            content='samples', content_rowid='id',
            tokenize='trigram'
        );

        -- FTS5 sync triggers
        CREATE TRIGGER IF NOT EXISTS samples_ai AFTER INSERT ON samples BEGIN
            INSERT INTO samples_fts(rowid, vendor, library, category, sound_id,
                description, comment, key, name)
            VALUES (new.id, new.vendor, new.library, new.category, new.sound_id,
                new.description, new.comment, new.key, new.name);
        END;

        CREATE TRIGGER IF NOT EXISTS samples_ad AFTER DELETE ON samples BEGIN
            INSERT INTO samples_fts(samples_fts, rowid, vendor, library, category,
                sound_id, description, comment, key, name)
            VALUES ('delete', old.id, old.vendor, old.library, old.category,
                old.sound_id, old.description, old.comment, old.key, old.name);
        END;

        CREATE TRIGGER IF NOT EXISTS samples_au AFTER UPDATE ON samples BEGIN
            INSERT INTO samples_fts(samples_fts, rowid, vendor, library, category,
                sound_id, description, comment, key, name)
            VALUES ('delete', old.id, old.vendor, old.library, old.category,
                old.sound_id, old.description, old.comment, old.key, old.name);
            INSERT INTO samples_fts(rowid, vendor, library, category, sound_id,
                description, comment, key, name)
            VALUES (new.id, new.vendor, new.library, new.category, new.sound_id,
                new.description, new.comment, new.key, new.name);
        END;",
    )?;

    Ok(())
}

/// Check if a column exists in a table.
fn has_column(conn: &Connection, column: &str) -> bool {
    conn.prepare("PRAGMA table_info(samples)")
        .ok()
        .map(|mut stmt| {
            stmt.query_map([], |row| row.get::<_, String>(1))
                .ok()
                .map(|mut rows| rows.any(|r| r.as_deref() == Ok(column)))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Idempotently add a column if it doesn't exist.
fn add_column_if_missing(conn: &Connection, column: &str, typedef: &str) -> anyhow::Result<()> {
    if !has_column(conn, column) {
        conn.execute_batch(&format!(
            "ALTER TABLE samples ADD COLUMN {column} {typedef};"
        ))?;
    }
    Ok(())
}

/// Migrate from older schema versions to the current version.
fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;

    if version < 2 {
        // v1 → v2: add peaks_source column.
        add_column_if_missing(conn, "peaks_source", "TEXT NOT NULL DEFAULT 'none'")?;
    }

    if version < 3 {
        // v2 → v3: add marked + audio info columns.
        add_column_if_missing(conn, "marked", "INTEGER NOT NULL DEFAULT 0")?;
        add_column_if_missing(conn, "duration", "REAL")?;
        add_column_if_missing(conn, "sample_rate", "INTEGER")?;
        add_column_if_missing(conn, "bit_depth", "INTEGER")?;
        add_column_if_missing(conn, "channels", "INTEGER")?;
    }

    if version < 4 {
        // v3 → v4: add date, take, track, item columns.
        add_column_if_missing(conn, "date", "TEXT NOT NULL DEFAULT ''")?;
        add_column_if_missing(conn, "take", "TEXT NOT NULL DEFAULT ''")?;
        add_column_if_missing(conn, "track", "TEXT NOT NULL DEFAULT ''")?;
        add_column_if_missing(conn, "item", "TEXT NOT NULL DEFAULT ''")?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Register a `regexp(pattern, text)` scalar function backed by the Rust `regex` crate.
fn register_regexp_udf(conn: &Connection) -> anyhow::Result<()> {
    conn.create_scalar_function("regexp", 2, rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let pattern: String = ctx.get(0)?;
        let text: String = ctx.get(1)?;
        let re = regex::Regex::new(&pattern)
            .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
        Ok(re.is_match(&text))
    })?;
    Ok(())
}

// --- SQL Query Builder ---

/// SQL parameter value.
#[derive(Debug, Clone)]
pub enum SqlValue {
    /// Text value.
    Text(String),
    /// Integer value.
    Int(i64),
}

impl rusqlite::types::ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            SqlValue::Text(s) => s.to_sql(),
            SqlValue::Int(i) => i.to_sql(),
        }
    }
}

/// Build a parameterized SQL query from a `SearchQuery`.
///
/// Returns `(sql_string, parameter_values)`. Uses FTS5 MATCH for single
/// substring queries and LIKE/REGEXP for more complex cases.
pub fn build_sql(query: &SearchQuery) -> (String, Vec<SqlValue>) {
    if query.is_empty() {
        return (
            "SELECT * FROM samples ORDER BY path".to_string(),
            vec![],
        );
    }

    // Free-text: FTS5 MATCH across all indexed columns.
    if let Some(text) = &query.freetext {
        if !text.is_empty() {
            let fts_escaped = format!("\"{}\"", text.replace('"', "\"\""));
            let values = vec![SqlValue::Text(fts_escaped)];
            let sql = "SELECT * FROM samples WHERE samples.id IN \
                        (SELECT rowid FROM samples_fts WHERE samples_fts MATCH ?1) \
                        ORDER BY (SELECT rank FROM samples_fts WHERE samples_fts.rowid = samples.id), path"
                .to_string();
            return (sql, values);
        }
    }

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();
    let mut uses_fts = false;

    // Collect field conditions.
    let fields: &[(&str, &Option<Pattern>)] = &[
        ("vendor", &query.vendor),
        ("library", &query.library),
        ("category", &query.category),
        ("sound_id", &query.sound_id),
        ("key", &query.key),
    ];

    for &(col, pat_opt) in fields {
        if let Some(pat) = pat_opt {
            add_pattern_condition(col, pat, &mut conditions, &mut values);
        }
    }

    // Description searches both description and comment columns.
    if let Some(pat) = &query.description {
        match pat {
            Pattern::Substring(s) => {
                let escaped = escape_like(s);
                let idx = values.len();
                values.push(SqlValue::Text(format!("%{escaped}%")));
                values.push(SqlValue::Text(format!("%{escaped}%")));
                conditions.push(format!(
                    "(description LIKE ?{} ESCAPE '\\' OR comment LIKE ?{} ESCAPE '\\')",
                    idx + 1,
                    idx + 2,
                ));
            }
            Pattern::Regex(r) => {
                let idx = values.len();
                values.push(SqlValue::Text(r.as_str().to_string()));
                values.push(SqlValue::Text(r.as_str().to_string()));
                conditions.push(format!(
                    "(regexp(?{}, description) OR regexp(?{}, comment))",
                    idx + 1,
                    idx + 2,
                ));
            }
        }
    }

    // BPM range.
    if let Some(bpm) = &query.bpm {
        let idx = values.len();
        values.push(SqlValue::Int(bpm.min as i64));
        values.push(SqlValue::Int(bpm.max as i64));
        conditions.push(format!(
            "bpm IS NOT NULL AND bpm BETWEEN ?{} AND ?{}",
            idx + 1,
            idx + 2,
        ));
    }

    // Check if we can use FTS5 for acceleration.
    // Use FTS5 when there's exactly one substring condition on an FTS-indexed column
    // and AND mode. For simplicity, we use LIKE for multi-field queries since FTS5
    // trigram MATCH doesn't do per-column filtering directly.
    if conditions.len() == 1 && matches!(query.match_mode, MatchMode::And) && query.bpm.is_none() {
        // Check if this is a single-field substring query on an FTS column
        if let Some(single_field) = get_single_fts_field(query) {
            if let Some(Pattern::Substring(s)) = single_field.1 {
                // Rewrite to use FTS5 MATCH + field filter.
                // Wrap in double quotes for FTS5 to treat as literal phrase
                // (prevents operators like - being interpreted as NOT).
                let fts_escaped = format!("\"{}\"", s.replace('"', "\"\""));
                conditions.clear();
                values.clear();
                values.push(SqlValue::Text(fts_escaped));
                values.push(SqlValue::Text(format!("%{}%", escape_like(s))));
                uses_fts = true;
                conditions.push(format!(
                    "samples.id IN (SELECT rowid FROM samples_fts WHERE samples_fts MATCH ?1) AND {} LIKE ?2 ESCAPE '\\'",
                    single_field.0,
                ));
            }
        }
    }

    let joiner = match query.match_mode {
        MatchMode::And => " AND ",
        MatchMode::Or => " OR ",
    };
    let where_clause = conditions.join(joiner);

    let order = if uses_fts {
        // Use FTS5 rank (BM25) for FTS queries, with path as tiebreaker.
        "ORDER BY (SELECT rank FROM samples_fts WHERE samples_fts.rowid = samples.id), path"
    } else {
        "ORDER BY path"
    };

    let sql = format!("SELECT * FROM samples WHERE {where_clause} {order}");
    (sql, values)
}

/// Get the single FTS-indexed field if query has exactly one text filter set.
fn get_single_fts_field(query: &SearchQuery) -> Option<(&str, &Option<Pattern>)> {
    let fields: Vec<(&str, &Option<Pattern>)> = vec![
        ("vendor", &query.vendor),
        ("library", &query.library),
        ("category", &query.category),
        ("sound_id", &query.sound_id),
        ("key", &query.key),
    ];

    let mut active = fields.into_iter().filter(|(_, p)| p.is_some());
    let first = active.next()?;
    if active.next().is_some() {
        return None; // more than one
    }
    // Also check description separately — it maps to two columns
    if query.description.is_some() {
        return None;
    }
    Some(first)
}

fn add_pattern_condition(
    col: &str,
    pat: &Pattern,
    conditions: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
) {
    match pat {
        Pattern::Substring(s) => {
            let escaped = escape_like(s);
            let idx = values.len();
            values.push(SqlValue::Text(format!("%{escaped}%")));
            conditions.push(format!("{col} LIKE ?{} ESCAPE '\\'", idx + 1));
        }
        Pattern::Regex(r) => {
            let idx = values.len();
            values.push(SqlValue::Text(r.as_str().to_string()));
            conditions.push(format!("regexp(?{}, {col})", idx + 1));
        }
    }
}

/// Escape LIKE special characters.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Convert a rusqlite Row to UnifiedMetadata.
fn row_to_metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<UnifiedMetadata> {
    let path_str: String = row.get("path")?;
    let bpm: Option<i32> = row.get("bpm")?;
    let recid: i64 = row.get("recid")?;

    Ok(UnifiedMetadata {
        path: PathBuf::from(path_str),
        vendor: row.get("vendor")?,
        library: row.get("library")?,
        description: row.get("description")?,
        umid: row.get("umid")?,
        recid: recid as u64,
        comment: row.get("comment")?,
        rating: row.get("rating")?,
        bpm: bpm.map(|v| v as u16),
        subcategory: row.get("subcategory")?,
        category: row.get("category")?,
        genre_id: row.get("genre_id")?,
        sound_id: row.get("sound_id")?,
        usage_id: row.get("usage_id")?,
        key: row.get("key")?,
        date: row.get("date")?,
        take: row.get("take")?,
        track: row.get("track")?,
        item: row.get("item")?,
    })
}

/// Convert a rusqlite Row to TableRow (metadata + audio info + marked).
fn row_to_table_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<super::TableRow> {
    let meta = row_to_metadata(row)?;

    let duration: Option<f64> = row.get("duration")?;
    let sample_rate: Option<i32> = row.get("sample_rate")?;
    let bit_depth: Option<i32> = row.get("bit_depth")?;
    let channels: Option<i32> = row.get("channels")?;
    let marked: i32 = row.get("marked").unwrap_or(0);

    let audio_info = match (duration, sample_rate, bit_depth, channels) {
        (Some(dur), Some(sr), Some(bd), Some(ch)) => Some(super::wav::AudioInfo {
            duration_secs: dur,
            sample_rate: sr as u32,
            bit_depth: bd as u16,
            channels: ch as u16,
        }),
        _ => None,
    };

    Ok(super::TableRow {
        meta,
        audio_info,
        marked: marked != 0,
    })
}

// --- Peak data compression ---

/// Compress raw peak bytes with zstd.
pub fn compress_peaks(raw: &[u8]) -> Vec<u8> {
    zstd::encode_all(raw, 3).unwrap_or_else(|_| raw.to_vec())
}

/// Decompress zstd-compressed peak bytes.
pub fn decompress_peaks(blob: &[u8]) -> Vec<u8> {
    zstd::decode_all(blob).unwrap_or_else(|_| blob.to_vec())
}

// --- DB path resolution ---

/// Resolve the database file path from CLI options and environment.
///
/// Priority: `--db-path` > `RIFFGREP_DB` env var > platform default.
pub fn resolve_db_path(
    explicit_path: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    // 1. Explicit CLI flag.
    if let Some(p) = explicit_path {
        ensure_parent_dirs(p)?;
        return Ok(p.to_path_buf());
    }

    // 2. Environment variable.
    if let Ok(env_path) = std::env::var("RIFFGREP_DB") {
        let p = PathBuf::from(env_path);
        ensure_parent_dirs(&p)?;
        return Ok(p);
    }

    // 3. Platform default.
    let default = default_db_path()?;
    ensure_parent_dirs(&default)?;
    Ok(default)
}

/// Platform-specific default database path.
fn default_db_path() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("HOME not set"))?;
        Ok(PathBuf::from(home)
            .join("Library/Application Support/riffgrep/index.db"))
    }
    #[cfg(target_os = "linux")]
    {
        let data_dir = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.local/share")
        });
        Ok(PathBuf::from(data_dir).join("riffgrep/index.db"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow::anyhow!("HOME not set"))?;
        Ok(PathBuf::from(home).join(".riffgrep/index.db"))
    }
}

/// Create parent directories for a path if they don't exist.
fn ensure_parent_dirs(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// Get the Unix epoch mtime of a file.
pub fn file_mtime(path: &Path) -> anyhow::Result<i64> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(mtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::BpmRange;

    // --- Ticket 1 tests: Schema & pragmas ---

    #[test]
    fn test_create_new_database() {
        let dir = std::env::temp_dir().join("riffgrep_test_create_db");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        let db = Database::open(&db_path).unwrap();

        // Verify schema via table_info.
        let mut stmt = db.conn.prepare("PRAGMA table_info(samples)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(cols.contains(&"path".to_string()));
        assert!(cols.contains(&"vendor".to_string()));
        assert!(cols.contains(&"peaks".to_string()));
        assert!(cols.contains(&"mtime".to_string()));
        assert!(cols.contains(&"bpm".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_open_existing_database() {
        let dir = std::env::temp_dir().join("riffgrep_test_open_existing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        let _db1 = Database::open(&db_path).unwrap();
        drop(_db1);
        let _db2 = Database::open(&db_path).unwrap();
        // No error — idempotent.

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pragmas_applied() {
        let db = Database::open_in_memory().unwrap();
        let journal: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        // In-memory databases use "memory" journal mode, but the pragma was set.
        // For file-based DBs it would be "wal".
        assert!(
            journal == "wal" || journal == "memory",
            "unexpected journal mode: {journal}"
        );
    }

    #[test]
    fn test_fts5_trigram_available() {
        let db = Database::open_in_memory().unwrap();

        // Insert a row.
        db.conn
            .execute(
                "INSERT INTO samples (path, name, parent_folder, mtime)
                 VALUES ('test.wav', 'test', '/tmp', 100)",
                [],
            )
            .unwrap();

        // Update searchable fields.
        db.conn
            .execute(
                "UPDATE samples SET vendor = 'Samples From Mars' WHERE path = 'test.wav'",
                [],
            )
            .unwrap();

        // FTS5 trigram query.
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM samples_fts WHERE samples_fts MATCH 'mars'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_user_version_set() {
        let db = Database::open_in_memory().unwrap();
        let version: u32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
    }

    // --- Ticket 2 tests: DB path resolution ---

    #[test]
    fn test_explicit_path_takes_priority() {
        let dir = std::env::temp_dir().join("riffgrep_test_explicit_path");
        let _ = std::fs::remove_dir_all(&dir);
        let explicit = dir.join("custom.db");

        // Explicit path always wins regardless of env var.
        let result = resolve_db_path(Some(&explicit)).unwrap();
        assert_eq!(result, explicit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_default_path_macos() {
        #[cfg(target_os = "macos")]
        {
            let result = default_db_path().unwrap();
            assert!(
                result
                    .to_string_lossy()
                    .contains("Library/Application Support/riffgrep"),
                "expected macOS default path, got: {}",
                result.display()
            );
        }
    }

    #[test]
    fn test_parent_dirs_created() {
        let dir = std::env::temp_dir().join("riffgrep_test_parent_dirs/deep/nested");
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join("riffgrep_test_parent_dirs"),
        );
        let db_path = dir.join("index.db");

        let result = resolve_db_path(Some(&db_path)).unwrap();
        assert_eq!(result, db_path);
        assert!(dir.exists(), "parent dirs should have been created");

        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join("riffgrep_test_parent_dirs"),
        );
    }

    // --- Ticket 3 tests: Batch insert & index_writer ---

    fn make_test_meta(path: &str, vendor: &str) -> UnifiedMetadata {
        UnifiedMetadata {
            path: PathBuf::from(path),
            vendor: vendor.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_insert_single_record() {
        let db = Database::open_in_memory().unwrap();
        let meta = UnifiedMetadata {
            path: PathBuf::from("/samples/test.wav"),
            vendor: "Samples From Mars".to_string(),
            library: "DX100".to_string(),
            category: "LOOP".to_string(),
            sound_id: "DHC".to_string(),
            description: "A cool synth".to_string(),
            comment: "Prophet-10".to_string(),
            key: "A#m".to_string(),
            bpm: Some(164),
            rating: "****".to_string(),
            subcategory: "DEMO".to_string(),
            genre_id: "ACID".to_string(),
            usage_id: "XPM".to_string(),
            umid: "abc123".to_string(),
            recid: 985188,
            ..Default::default()
        };

        db.insert_batch(&[(meta, 1000, None)]).unwrap();

        let row: (String, String, String, String, String, String, String, Option<i32>, String, String, String, String, String, i64) = db.conn.query_row(
            "SELECT vendor, library, category, sound_id, description, comment, key, bpm, rating, subcategory, genre_id, usage_id, umid, recid FROM samples WHERE path = '/samples/test.wav'",
            [],
            |r| Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?,
                r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?,
                r.get(12)?, r.get(13)?,
            )),
        ).unwrap();

        assert_eq!(row.0, "Samples From Mars");
        assert_eq!(row.1, "DX100");
        assert_eq!(row.2, "LOOP");
        assert_eq!(row.3, "DHC");
        assert_eq!(row.4, "A cool synth");
        assert_eq!(row.5, "Prophet-10");
        assert_eq!(row.6, "A#m");
        assert_eq!(row.7, Some(164));
        assert_eq!(row.8, "****");
        assert_eq!(row.9, "DEMO");
        assert_eq!(row.10, "ACID");
        assert_eq!(row.11, "XPM");
        assert_eq!(row.12, "abc123");
        assert_eq!(row.13, 985188);
    }

    #[test]
    fn test_insert_batch_1000() {
        let db = Database::open_in_memory().unwrap();
        let records: Vec<(UnifiedMetadata, i64, Option<Vec<u8>>)> = (0..1000)
            .map(|i| {
                (
                    make_test_meta(&format!("/samples/test_{i}.wav"), "Vendor"),
                    100,
                    None,
                )
            })
            .collect();
        let count = db.insert_batch(&records).unwrap();
        assert_eq!(count, 1000);

        let actual: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
            .unwrap();
        assert_eq!(actual, 1000);
    }

    #[test]
    fn test_insert_or_replace_dedup() {
        let db = Database::open_in_memory().unwrap();
        let meta1 = UnifiedMetadata {
            path: PathBuf::from("/samples/test.wav"),
            vendor: "Old Vendor".to_string(),
            ..Default::default()
        };
        let meta2 = UnifiedMetadata {
            path: PathBuf::from("/samples/test.wav"),
            vendor: "New Vendor".to_string(),
            ..Default::default()
        };

        db.insert_batch(&[(meta1, 100, None)]).unwrap();
        db.insert_batch(&[(meta2, 200, None)]).unwrap();

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let vendor: String = db
            .conn
            .query_row(
                "SELECT vendor FROM samples WHERE path = '/samples/test.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(vendor, "New Vendor");
    }

    #[test]
    fn test_fts_populated_after_insert() {
        let db = Database::open_in_memory().unwrap();
        let meta = make_test_meta("/samples/test.wav", "Samples From Mars");
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM samples_fts WHERE samples_fts MATCH 'mars'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_index_writer_from_channel() {
        let db = Database::open_in_memory().unwrap();
        let (tx, rx) = crossbeam_channel::bounded(64);

        for i in 0..50 {
            tx.send((
                make_test_meta(&format!("/samples/test_{i}.wav"), "Vendor"),
                100,
                None,
            ))
            .unwrap();
        }
        drop(tx);

        let count = db.index_writer(&rx, 1000).unwrap();
        assert_eq!(count, 50);
    }

    #[test]
    fn test_index_writer_empty_channel() {
        let db = Database::open_in_memory().unwrap();
        let (tx, rx) = crossbeam_channel::bounded::<(UnifiedMetadata, i64, Option<Vec<u8>>)>(64);
        drop(tx);

        let count = db.index_writer(&rx, 1000).unwrap();
        assert_eq!(count, 0);
    }

    // --- Ticket 5 tests: SQL query builder ---

    #[test]
    fn test_substring_generates_like() {
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            ..Default::default()
        };
        let (sql, values) = build_sql(&query);
        assert!(sql.contains("LIKE"), "SQL should contain LIKE: {sql}");
        assert_eq!(values.len(), 2); // FTS5 match + LIKE (single field optimization)
    }

    #[test]
    fn test_regex_generates_regexp() {
        let query = SearchQuery {
            vendor: Some(Pattern::Regex(regex::Regex::new("DX\\d+").unwrap())),
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(
            sql.contains("regexp"),
            "SQL should contain regexp: {sql}"
        );
    }

    #[test]
    fn test_bpm_range_generates_between() {
        let query = SearchQuery {
            bpm: Some(BpmRange { min: 120, max: 128 }),
            ..Default::default()
        };
        let (sql, values) = build_sql(&query);
        assert!(
            sql.contains("BETWEEN"),
            "SQL should contain BETWEEN: {sql}"
        );
        assert!(values.len() >= 2);
    }

    #[test]
    fn test_and_mode_joins_with_and() {
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::And,
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(sql.contains(" AND "), "SQL should use AND: {sql}");
    }

    #[test]
    fn test_or_mode_joins_with_or() {
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::Or,
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(sql.contains(" OR "), "SQL should use OR: {sql}");
    }

    #[test]
    fn test_empty_query_no_where() {
        let query = SearchQuery::default();
        let (sql, values) = build_sql(&query);
        assert!(
            !sql.contains("WHERE"),
            "empty query should not have WHERE: {sql}"
        );
        assert!(values.is_empty());
    }

    #[test]
    fn test_description_searches_comment_too() {
        let query = SearchQuery {
            description: Some(Pattern::Substring("prophet".to_string())),
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(
            sql.contains("description LIKE") && sql.contains("comment LIKE"),
            "description should search both columns: {sql}"
        );
    }

    #[test]
    fn test_special_chars_escaped() {
        let escaped = escape_like("100%_done");
        assert_eq!(escaped, "100\\%\\_done");
    }

    #[test]
    fn test_fts5_used_for_substring() {
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(
            sql.contains("samples_fts MATCH"),
            "single substring should use FTS5: {sql}"
        );
    }

    // --- Ticket 6 tests: SqliteFinder ---

    fn index_test_files(db: &Database) {
        let test_dir = PathBuf::from("test_files");
        if !test_dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(&test_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wav") {
                if let Ok(meta) = super::super::read_metadata(&path) {
                    let mtime = file_mtime(&path).unwrap_or(0);
                    db.insert_batch(&[(meta, mtime, None)]).unwrap();
                }
            }
        }
    }

    #[test]
    fn test_sqlite_finder_returns_all() {
        if !PathBuf::from("test_files").exists() {
            return;
        }
        let db = Database::open_in_memory().unwrap();
        index_test_files(&db);

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery::default();
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 9, "expected 9 results, got {}", results.len());
    }

    #[test]
    fn test_sqlite_finder_vendor_filter() {
        if !PathBuf::from("test_files").exists() {
            return;
        }
        let db = Database::open_in_memory().unwrap();
        index_test_files(&db);

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("IART-Artist".to_string())),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert!(!results.is_empty(), "should find IART-Artist vendor");
        for r in &results {
            assert!(
                r.vendor.to_ascii_lowercase().contains("iart-artist"),
                "vendor mismatch: {}",
                r.vendor
            );
        }
    }

    #[test]
    fn test_sqlite_finder_regex() {
        if !PathBuf::from("test_files").exists() {
            return;
        }
        let db = Database::open_in_memory().unwrap();
        index_test_files(&db);

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            description: Some(Pattern::Regex(regex::Regex::new("DX-?1[0-9]{2}").unwrap())),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert!(!results.is_empty(), "should match DX-100 pattern");
    }

    #[test]
    fn test_sqlite_finder_no_matches() {
        if !PathBuf::from("test_files").exists() {
            return;
        }
        let db = Database::open_in_memory().unwrap();
        index_test_files(&db);

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("nonexistent_vendor_xyz".to_string())),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_sqlite_finder_description_searches_comment() {
        let db = Database::open_in_memory().unwrap();
        let meta = UnifiedMetadata {
            path: PathBuf::from("/test/comment_match.wav"),
            comment: "Sequential Circuits Prophet-10".to_string(),
            description: "plain text".to_string(),
            ..Default::default()
        };
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            description: Some(Pattern::Substring("prophet".to_string())),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 1, "should match via comment column");
    }

    #[test]
    fn test_sqlite_finder_and_mode() {
        let db = Database::open_in_memory().unwrap();
        let meta = UnifiedMetadata {
            path: PathBuf::from("/test/both.wav"),
            vendor: "Samples From Mars".to_string(),
            category: "LOOP".to_string(),
            ..Default::default()
        };
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let meta2 = UnifiedMetadata {
            path: PathBuf::from("/test/vendor_only.wav"),
            vendor: "Samples From Mars".to_string(),
            category: "ONESHOT".to_string(),
            ..Default::default()
        };
        db.insert_batch(&[(meta2, 100, None)]).unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::And,
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 1, "AND mode should return only the matching row");
        assert!(results[0].path.to_string_lossy().contains("both.wav"));
    }

    #[test]
    fn test_sqlite_finder_or_mode() {
        let db = Database::open_in_memory().unwrap();
        let meta = UnifiedMetadata {
            path: PathBuf::from("/test/mars.wav"),
            vendor: "Samples From Mars".to_string(),
            ..Default::default()
        };
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let meta2 = UnifiedMetadata {
            path: PathBuf::from("/test/loop.wav"),
            category: "LOOP".to_string(),
            ..Default::default()
        };
        db.insert_batch(&[(meta2, 100, None)]).unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::Or,
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);

        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 2, "OR mode should return both rows");
    }

    // --- Ticket 9 tests: Peak compression ---

    #[test]
    fn test_compress_decompress_roundtrip() {
        let raw: Vec<u8> = (0..180).map(|i| (i * 7 % 256) as u8).collect();
        let compressed = compress_peaks(&raw);
        let decompressed = decompress_peaks(&compressed);
        assert_eq!(decompressed, raw);
    }

    #[test]
    fn test_compress_all_zeros() {
        let raw = vec![0u8; 180];
        let compressed = compress_peaks(&raw);
        let decompressed = decompress_peaks(&compressed);
        assert_eq!(decompressed, raw);
    }

    #[test]
    fn test_peaks_stored_in_db() {
        let db = Database::open_in_memory().unwrap();
        let raw: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let compressed = compress_peaks(&raw);

        let meta = make_test_meta("/test/peaks.wav", "Vendor");
        db.insert_batch(&[(meta, 100, Some(compressed.clone()))])
            .unwrap();

        let blob: Vec<u8> = db
            .conn
            .query_row(
                "SELECT peaks FROM samples WHERE path = '/test/peaks.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        let decompressed = decompress_peaks(&blob);
        assert_eq!(decompressed, raw);
    }

    #[test]
    fn test_peaks_empty_for_no_peaks() {
        let db = Database::open_in_memory().unwrap();
        let meta = make_test_meta("/test/no_peaks.wav", "Vendor");
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let blob: Option<Vec<u8>> = db
            .conn
            .query_row(
                "SELECT peaks FROM samples WHERE path = '/test/no_peaks.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(blob.is_none());
    }

    // --- Ticket 8 tests: Incremental indexing ---

    #[test]
    fn test_get_path_mtimes() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/a.wav", "V"), 100, None),
            (make_test_meta("/b.wav", "V"), 200, None),
        ])
        .unwrap();

        let mtimes = db.get_path_mtimes().unwrap();
        assert_eq!(mtimes.len(), 2);
        assert_eq!(mtimes[&PathBuf::from("/a.wav")], 100);
        assert_eq!(mtimes[&PathBuf::from("/b.wav")], 200);
    }

    #[test]
    fn test_delete_paths() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/a.wav", "V"), 100, None),
            (make_test_meta("/b.wav", "V"), 200, None),
            (make_test_meta("/c.wav", "V"), 300, None),
        ])
        .unwrap();

        let deleted = db
            .delete_paths(&[Path::new("/a.wav"), Path::new("/c.wav")])
            .unwrap();
        assert_eq!(deleted, 2);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_fts_consistent_after_delete() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(make_test_meta("/a.wav", "Samples From Mars"), 100, None)])
            .unwrap();

        // Verify FTS has the entry.
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM samples_fts WHERE samples_fts MATCH 'mars'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete.
        db.delete_paths(&[Path::new("/a.wav")]).unwrap();

        // FTS should be updated.
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM samples_fts WHERE samples_fts MATCH 'mars'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    // --- Ticket 10 tests: DB stats ---

    #[test]
    fn test_db_stats_populated() {
        if !PathBuf::from("test_files").exists() {
            return;
        }
        let db = Database::open_in_memory().unwrap();
        index_test_files(&db);

        let stats = db.stats().unwrap();
        assert_eq!(stats.file_count, 9);
    }

    #[test]
    fn test_db_stats_empty_db() {
        let db = Database::open_in_memory().unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.file_count, 0);
    }

    #[test]
    fn test_db_stats_vendor_counts() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..5 {
            db.insert_batch(&[(
                make_test_meta(&format!("/test/mars_{i}.wav"), "Samples From Mars"),
                100,
                None,
            )])
            .unwrap();
        }
        for i in 0..3 {
            db.insert_batch(&[(
                make_test_meta(&format!("/test/splice_{i}.wav"), "Splice"),
                100,
                None,
            )])
            .unwrap();
        }

        let stats = db.stats().unwrap();
        assert!(!stats.top_vendors.is_empty());
        assert_eq!(stats.top_vendors[0].0, "Samples From Mars");
        assert_eq!(stats.top_vendors[0].1, 5);
    }

    // --- Ticket 11 tests: BM25 ranking ---

    #[test]
    fn test_non_fts_defaults_to_path_order() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/z/file.wav", "V"), 100, None),
            (make_test_meta("/a/file.wav", "V"), 100, None),
        ])
        .unwrap();

        let query = SearchQuery {
            bpm: Some(BpmRange { min: 0, max: 999 }),
            ..Default::default()
        };
        let (sql, _) = build_sql(&query);
        assert!(
            sql.contains("ORDER BY path"),
            "non-FTS should order by path: {sql}"
        );
    }

    #[test]
    fn test_ranking_deterministic() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..10 {
            db.insert_batch(&[(
                make_test_meta(&format!("/test/{i}.wav"), "Vendor"),
                100,
                None,
            )])
            .unwrap();
        }

        let query = SearchQuery {
            vendor: Some(Pattern::Substring("vendor".to_string())),
            ..Default::default()
        };

        let (tx1, rx1) = crossbeam_channel::bounded(64);
        db.search(&query, &tx1);
        drop(tx1);
        let results1: Vec<_> = rx1.iter().map(|m| m.path.clone()).collect();

        let (tx2, rx2) = crossbeam_channel::bounded(64);
        db.search(&query, &tx2);
        drop(tx2);
        let results2: Vec<_> = rx2.iter().map(|m| m.path.clone()).collect();

        assert_eq!(results1, results2);
    }

    // --- Ticket T2 tests: Free-text query + get_peaks ---

    #[test]
    fn test_freetext_sqlite_matches_vendor() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(
            make_test_meta("/test/mars.wav", "Samples From Mars"),
            100,
            None,
        )])
        .unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            freetext: Some("mars".to_string()),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_freetext_sqlite_matches_description() {
        let db = Database::open_in_memory().unwrap();
        let mut meta = make_test_meta("/test/kick.wav", "Vendor");
        meta.description = "punchy kick drum".to_string();
        db.insert_batch(&[(meta, 100, None)]).unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            freetext: Some("kick".to_string()),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_freetext_sqlite_no_match() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(make_test_meta("/test/a.wav", "Vendor"), 100, None)])
            .unwrap();

        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            freetext: Some("zzzznonexistent".to_string()),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_freetext_empty_returns_all() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/test/a.wav", "V"), 100, None),
            (make_test_meta("/test/b.wav", "V"), 100, None),
        ])
        .unwrap();

        // Empty string freetext → treated as empty query → all results.
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            freetext: Some(String::new()),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_freetext_special_chars_escaped() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(make_test_meta("/test/a.wav", "Vendor"), 100, None)])
            .unwrap();

        // These should not cause SQL errors.
        for input in &["-", "\"", "%", "test-value", "foo\"bar"] {
            let query = SearchQuery {
                freetext: Some(input.to_string()),
                ..Default::default()
            };
            let (sql, _) = build_sql(&query);
            // Just verify it doesn't panic.
            let (tx, rx) = crossbeam_channel::bounded(64);
            db.search(&query, &tx);
            drop(tx);
            let _: Vec<_> = rx.iter().collect();
        }
    }

    #[test]
    fn test_get_peaks_returns_decompressed() {
        let db = Database::open_in_memory().unwrap();
        let raw: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let compressed = compress_peaks(&raw);
        let meta = make_test_meta("/test/peaks.wav", "V");
        db.insert_batch(&[(meta, 100, Some(compressed))]).unwrap();

        let peaks = db.get_peaks("/test/peaks.wav").unwrap();
        assert!(peaks.is_some());
        assert_eq!(peaks.unwrap().len(), 180);
    }

    #[test]
    fn test_get_peaks_missing_path_returns_none() {
        let db = Database::open_in_memory().unwrap();
        let peaks = db.get_peaks("/nonexistent/path.wav").unwrap();
        assert!(peaks.is_none());
    }

    #[test]
    fn test_existing_field_search_unchanged() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(
            make_test_meta("/test/mars.wav", "Samples From Mars"),
            100,
            None,
        )])
        .unwrap();

        // Per-field --vendor search still works.
        let (tx, rx) = crossbeam_channel::bounded(64);
        let query = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            ..Default::default()
        };
        db.search(&query, &tx);
        drop(tx);
        let results: Vec<_> = rx.iter().collect();
        assert_eq!(results.len(), 1);
    }

    // --- Schema migration tests ---

    #[test]
    fn test_fresh_db_is_v4() {
        let db = Database::open_in_memory().unwrap();
        let version: u32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        let mut stmt = db.conn.prepare("PRAGMA table_info(samples)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(cols.contains(&"peaks_source".to_string()));
        assert!(cols.contains(&"marked".to_string()));
        assert!(cols.contains(&"duration".to_string()));
        assert!(cols.contains(&"sample_rate".to_string()));
        assert!(cols.contains(&"bit_depth".to_string()));
        assert!(cols.contains(&"channels".to_string()));
        assert!(cols.contains(&"date".to_string()));
        assert!(cols.contains(&"take".to_string()));
        assert!(cols.contains(&"track".to_string()));
        assert!(cols.contains(&"item".to_string()));
    }

    #[test]
    fn test_migrate_v1_to_v4() {
        let dir = std::env::temp_dir().join("riffgrep_test_migrate_v1v4");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        // Create a v1 database manually (without peaks_source or v3/v4 columns).
        {
            let conn = Connection::open(&db_path).unwrap();
            apply_pragmas(&conn).unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS samples (
                    id INTEGER PRIMARY KEY,
                    path TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    parent_folder TEXT NOT NULL,
                    vendor TEXT NOT NULL DEFAULT '',
                    library TEXT NOT NULL DEFAULT '',
                    category TEXT NOT NULL DEFAULT '',
                    sound_id TEXT NOT NULL DEFAULT '',
                    description TEXT NOT NULL DEFAULT '',
                    comment TEXT NOT NULL DEFAULT '',
                    key TEXT NOT NULL DEFAULT '',
                    bpm INTEGER,
                    rating TEXT NOT NULL DEFAULT '',
                    subcategory TEXT NOT NULL DEFAULT '',
                    genre_id TEXT NOT NULL DEFAULT '',
                    usage_id TEXT NOT NULL DEFAULT '',
                    umid TEXT NOT NULL DEFAULT '',
                    recid INTEGER NOT NULL DEFAULT 0,
                    mtime INTEGER NOT NULL,
                    peaks BLOB
                );",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1u32).unwrap();
            conn.execute(
                "INSERT INTO samples (path, name, parent_folder, mtime) VALUES ('a.wav', 'a', '/test', 100)",
                [],
            ).unwrap();
        }

        let db = Database::open(&db_path).unwrap();
        let version: u32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        // All new columns should exist.
        let source: String = db
            .conn
            .query_row(
                "SELECT peaks_source FROM samples WHERE path = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "none");

        let marked: i32 = db
            .conn
            .query_row(
                "SELECT marked FROM samples WHERE path = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marked, 0);

        // v4 columns should exist.
        assert!(has_column(&db.conn, "date"));
        assert!(has_column(&db.conn, "take"));
        assert!(has_column(&db.conn, "track"));
        assert!(has_column(&db.conn, "item"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_migrate_v2_to_v4() {
        let dir = std::env::temp_dir().join("riffgrep_test_migrate_v2v4");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        // Create a v2 database (has peaks_source, no v3/v4 columns).
        {
            let conn = Connection::open(&db_path).unwrap();
            apply_pragmas(&conn).unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS samples (
                    id INTEGER PRIMARY KEY,
                    path TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    parent_folder TEXT NOT NULL,
                    vendor TEXT NOT NULL DEFAULT '',
                    library TEXT NOT NULL DEFAULT '',
                    category TEXT NOT NULL DEFAULT '',
                    sound_id TEXT NOT NULL DEFAULT '',
                    description TEXT NOT NULL DEFAULT '',
                    comment TEXT NOT NULL DEFAULT '',
                    key TEXT NOT NULL DEFAULT '',
                    bpm INTEGER,
                    rating TEXT NOT NULL DEFAULT '',
                    subcategory TEXT NOT NULL DEFAULT '',
                    genre_id TEXT NOT NULL DEFAULT '',
                    usage_id TEXT NOT NULL DEFAULT '',
                    umid TEXT NOT NULL DEFAULT '',
                    recid INTEGER NOT NULL DEFAULT 0,
                    mtime INTEGER NOT NULL,
                    peaks BLOB,
                    peaks_source TEXT NOT NULL DEFAULT 'none'
                );",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 2u32).unwrap();
            conn.execute(
                "INSERT INTO samples (path, name, parent_folder, mtime) VALUES ('b.wav', 'b', '/test', 200)",
                [],
            ).unwrap();
        }

        let db = Database::open(&db_path).unwrap();
        let version: u32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        // v3 columns should exist.
        assert!(has_column(&db.conn, "marked"));
        assert!(has_column(&db.conn, "duration"));
        // v4 columns should exist.
        assert!(has_column(&db.conn, "date"));
        assert!(has_column(&db.conn, "take"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Mark/unmark tests ---

    #[test]
    fn test_mark_unmark_roundtrip() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[(make_test_meta("/test/m.wav", "V"), 100, None)])
            .unwrap();

        assert!(!db.is_marked("/test/m.wav").unwrap());
        db.mark_path("/test/m.wav").unwrap();
        assert!(db.is_marked("/test/m.wav").unwrap());
        db.unmark_path("/test/m.wav").unwrap();
        assert!(!db.is_marked("/test/m.wav").unwrap());
    }

    #[test]
    fn test_marked_paths_returns_only_marked() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/a.wav", "V"), 100, None),
            (make_test_meta("/b.wav", "V"), 100, None),
            (make_test_meta("/c.wav", "V"), 100, None),
        ])
        .unwrap();

        db.mark_path("/a.wav").unwrap();
        db.mark_path("/c.wav").unwrap();

        let paths = db.marked_paths().unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("/a.wav")));
        assert!(paths.contains(&PathBuf::from("/c.wav")));
    }

    #[test]
    fn test_clear_all_marks() {
        let db = Database::open_in_memory().unwrap();
        db.insert_batch(&[
            (make_test_meta("/a.wav", "V"), 100, None),
            (make_test_meta("/b.wav", "V"), 100, None),
        ])
        .unwrap();

        db.mark_path("/a.wav").unwrap();
        db.mark_path("/b.wav").unwrap();
        let cleared = db.clear_all_marks().unwrap();
        assert_eq!(cleared, 2);
        assert!(db.marked_paths().unwrap().is_empty());
    }

    // --- Audio info in DB tests ---

    #[test]
    fn test_audio_info_stored_during_index() {
        let db = Database::open_in_memory().unwrap();
        let meta = make_test_meta("/test/audio.wav", "V");
        let audio_info = Some(super::super::wav::AudioInfo {
            duration_secs: 3.5,
            sample_rate: 48000,
            bit_depth: 24,
            channels: 2,
        });

        db.insert_batch_with_audio(&[(meta, 100, None, "none".to_string(), audio_info)])
            .unwrap();

        let (dur, sr, bd, ch): (Option<f64>, Option<i32>, Option<i32>, Option<i32>) = db
            .conn
            .query_row(
                "SELECT duration, sample_rate, bit_depth, channels FROM samples WHERE path = '/test/audio.wav'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();

        assert!((dur.unwrap() - 3.5).abs() < 0.001);
        assert_eq!(sr, Some(48000));
        assert_eq!(bd, Some(24));
        assert_eq!(ch, Some(2));
    }

    #[test]
    fn test_insert_with_peaks_source() {
        let db = Database::open_in_memory().unwrap();
        let raw: Vec<u8> = (0..180).map(|i| (i * 3 % 256) as u8).collect();
        let compressed = compress_peaks(&raw);
        let meta = make_test_meta("/test/gen.wav", "Vendor");
        db.insert_batch_with_source(&[(meta, 100, Some(compressed))], "generated")
            .unwrap();

        let source: String = db
            .conn
            .query_row(
                "SELECT peaks_source FROM samples WHERE path = '/test/gen.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "generated");
    }

    #[test]
    fn test_stats_peaks_breakdown() {
        let db = Database::open_in_memory().unwrap();
        let raw = compress_peaks(&vec![128u8; 180]);
        db.insert_batch_with_source(
            &[(make_test_meta("/a.wav", "V"), 100, Some(raw.clone()))],
            "generated",
        )
        .unwrap();
        db.insert_batch_with_source(
            &[(make_test_meta("/b.wav", "V"), 100, Some(raw))],
            "generated",
        )
        .unwrap();
        db.insert_batch(&[(make_test_meta("/c.wav", "V"), 100, None)])
            .unwrap();

        let stats = db.stats().unwrap();
        assert_eq!(stats.file_count, 3);

        // Check peaks breakdown.
        let gen_count = stats
            .peaks_breakdown
            .iter()
            .find(|(s, _)| s == "generated")
            .map(|(_, c)| *c)
            .unwrap_or(0);
        assert_eq!(gen_count, 2);

        let none_count = stats
            .peaks_breakdown
            .iter()
            .find(|(s, _)| s == "none")
            .map(|(_, c)| *c)
            .unwrap_or(0);
        assert_eq!(none_count, 1);
    }

    #[test]
    fn test_migration_idempotent() {
        let dir = std::env::temp_dir().join("riffgrep_test_migrate_idempotent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        // Create v4 DB.
        let _db1 = Database::open(&db_path).unwrap();
        drop(_db1);

        // Open again — should not error.
        let db2 = Database::open(&db_path).unwrap();
        let version: u32 = db2
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
