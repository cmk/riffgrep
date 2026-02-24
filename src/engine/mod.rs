//! Core search engine: metadata reading, matching, and output.

pub mod bext;
pub mod cli;
pub mod config;
pub mod filesystem;
pub mod id3;
pub mod marks;
pub mod playback;
pub mod riff_info;
pub mod sqlite;
pub mod wav;
pub mod workflow;

use std::collections::HashSet;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use bext::{BextFields, RiffError};
use riff_info::InfoFields;

/// A TUI-specific wrapper around `UnifiedMetadata` with display fields.
///
/// Keeps TUI-specific state (marked, audio_info) separate from the engine
/// data model so that headless/JSON output paths are unaffected.
#[derive(Debug, Clone)]
pub struct TableRow {
    /// Core metadata from BEXT/RIFF INFO.
    pub meta: UnifiedMetadata,
    /// Audio format info (populated from DB in SQLite mode, None in filesystem mode).
    pub audio_info: Option<wav::AudioInfo>,
    /// Whether this file is marked/selected.
    pub marked: bool,
    /// Marker configuration from BEXT (if packed schema present).
    pub markers: Option<bext::MarkerConfig>,
}

/// Merged metadata from BEXT and RIFF INFO chunks.
///
/// Field names follow PICKER_SCHEMA.md. Packed Description fields use the
/// ID3v2.4 frame names from the schema (TCON, TIT2, TIT3, etc.).
#[derive(Debug, Clone, Default, Serialize)]
pub struct UnifiedMetadata {
    /// File path.
    pub path: PathBuf,

    // --- Standard BEXT fields ---
    /// BEXT Originator (bytes 256-288) or RIFF IART. Maps to TPE1/Vendor.
    pub vendor: String,
    /// BEXT OriginatorReference (bytes 288-320) or RIFF INAM. Maps to TPE2/Library.
    pub library: String,
    /// BEXT Description (bytes 0-255) as plain text, or RIFF ICMT.
    /// When the packed schema is detected, this holds the COMR/Comment.
    pub description: String,
    /// BEXT UMID (bytes 348-412) as hex string.
    pub umid: String,

    // --- Packed Description fields (per PICKER_SCHEMA.md) ---
    /// `[000:008]` riffgrep file identity — high 64 bits of UUID v7 generated at first pack (BE).
    /// Zero means not yet packed.
    pub file_id: u64,
    /// `[044:076]` COMR/Comment (32 ASCII).
    pub comment: String,
    /// `[076:080]` POPM/Rating (4 ASCII).
    pub rating: String,
    /// `[080:084]` TBPM/BPM (4 ASCII), parsed to integer.
    pub bpm: Option<u16>,
    /// `[084:088]` TCOM/Subcategory (4 ASCII).
    pub subcategory: String,
    /// `[088:092]` TCON/Category (4 ASCII) or RIFF IGNR.
    pub category: String,
    /// `[092:096]` TIT1/Genre ID (4 ASCII).
    pub genre_id: String,
    /// `[096:100]` TIT2/Sound ID (4 ASCII) or RIFF IKEY.
    pub sound_id: String,
    /// `[100:104]` TIT3/Usage ID (4 ASCII).
    pub usage_id: String,
    /// `[104:112]` TKEY/Key (8 ASCII).
    pub key: String,
    /// `[112:116]` Take number (4 ASCII, packed only).
    pub take: String,
    /// `[116:120]` Track number (4 ASCII, packed only).
    pub track: String,
    /// `[120:128]` Item number (8 ASCII, packed only).
    pub item: String,
    /// BEXT OriginationDate (bytes 320-329), e.g. "2024-01-15".
    pub date: String,
}

/// Read and merge metadata from a single WAV file.
///
/// 1. Scan first 4KB for chunk offsets
/// 2. Parse BEXT if found
/// 3. Parse LIST-INFO if found within 4KB
/// 4. Merge: BEXT fields take priority, INFO fills empty fields
pub fn read_metadata(path: &Path) -> Result<UnifiedMetadata, RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(8192, file);

    let map = bext::scan_chunks(&mut reader)?;
    let bext = bext::parse_bext_data(&mut reader, &map)?;
    let info = riff_info::parse_riff_info(&mut reader, &map)?;

    Ok(merge_metadata(path, bext, info))
}

/// Read metadata, extract BEXT peaks, markers, and return the detected peaks format.
/// Returns `(metadata, peaks_bytes, peaks_format, markers)`.
pub fn read_metadata_with_peaks_format(
    path: &Path,
) -> Result<(UnifiedMetadata, Vec<u8>, bext::PeaksFormat, Option<bext::MarkerConfig>), RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(8192, file);

    let map = bext::scan_chunks(&mut reader)?;
    let bext = bext::parse_bext_data(&mut reader, &map)?;
    let peaks = bext.peaks.clone();
    let peaks_format = bext.peaks_format;
    let markers = bext.markers;
    let info = riff_info::parse_riff_info(&mut reader, &map)?;

    Ok((merge_metadata(path, bext, info), peaks, peaks_format, markers))
}

/// Merge BEXT and INFO fields. BEXT takes priority; INFO fills empty fields.
fn merge_metadata(path: &Path, bext: BextFields, info: InfoFields) -> UnifiedMetadata {
    let mut meta = UnifiedMetadata {
        path: path.to_path_buf(),
        vendor: bext.vendor,
        library: bext.library,
        description: bext.description,
        umid: bext.umid,
        file_id: bext.file_id,
        comment: bext.comment,
        rating: bext.rating,
        bpm: bext.bpm,
        subcategory: bext.subcategory,
        category: bext.category,
        genre_id: bext.genre_id,
        sound_id: bext.sound_id,
        usage_id: bext.usage_id,
        key: bext.key,
        take: bext.take,
        track: bext.track,
        item: bext.item,
        date: bext.date,
    };

    // Fill empty fields from RIFF INFO.
    if meta.vendor.is_empty() {
        meta.vendor = info.vendor;
    }
    if meta.library.is_empty() {
        meta.library = info.library;
    }
    if meta.category.is_empty() {
        meta.category = info.category;
    }
    if meta.sound_id.is_empty() {
        meta.sound_id = info.sound_id;
    }
    if meta.description.is_empty() {
        meta.description = info.description;
    }

    meta
}

// --- Search Query Matching ---

/// A columnar filter parsed from `@field=value` syntax.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnFilter {
    /// The field name (e.g. "vendor", "bpm").
    pub field: String,
    /// One or more values to match (OR within a single filter).
    pub values: Vec<String>,
}

/// Parse `@field=value`, `@field="quoted value"`, and `@field=[v1,v2]` tokens
/// from search input.
///
/// Field names are case-insensitive (`@Vendor` matches `vendor`).
/// Values containing spaces must be double-quoted: `@vendor="Beat MPC"`.
///
/// Returns `(remaining_freetext, filters)`. Invalid field names are left in freetext.
/// Empty values are ignored.
pub fn parse_column_filters(input: &str) -> (String, Vec<ColumnFilter>) {
    let mut filters = Vec::new();
    let mut freetext_parts = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace.
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        if chars[i] == '@' {
            let start = i;
            i += 1; // skip '@'

            // Read field name until '=' or whitespace.
            let field_start = i;
            while i < len && chars[i] != '=' && !chars[i].is_whitespace() {
                i += 1;
            }

            if i < len && chars[i] == '=' {
                let field: String = chars[field_start..i].iter().collect();
                let field_lower = field.to_ascii_lowercase();
                i += 1; // skip '='

                // Read value: quoted, bracketed, or bare word.
                let value_str: String = if i < len && chars[i] == '"' {
                    i += 1; // skip opening quote
                    let val_start = i;
                    while i < len && chars[i] != '"' {
                        i += 1;
                    }
                    let val: String = chars[val_start..i].iter().collect();
                    if i < len {
                        i += 1; // skip closing quote
                    }
                    val
                } else if i < len && chars[i] == '[' {
                    let val_start = i;
                    while i < len && chars[i] != ']' {
                        i += 1;
                    }
                    if i < len {
                        i += 1; // include closing ']'
                    }
                    chars[val_start..i].iter().collect()
                } else {
                    let val_start = i;
                    while i < len && !chars[i].is_whitespace() {
                        i += 1;
                    }
                    chars[val_start..i].iter().collect()
                };

                // Validate field name and value.
                if !field_lower.is_empty()
                    && config::AVAILABLE_COLUMNS.contains(&field_lower.as_str())
                    && !value_str.is_empty()
                {
                    let values = if value_str.starts_with('[') && value_str.ends_with(']') {
                        // Multi-value: @field=[v1,v2]
                        let inner = &value_str[1..value_str.len() - 1];
                        inner
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    } else {
                        vec![value_str]
                    };

                    if !values.is_empty() {
                        filters.push(ColumnFilter {
                            field: field_lower,
                            values,
                        });
                        continue;
                    }
                }

                // Invalid filter — put the whole token back as freetext.
                let token: String = chars[start..i].iter().collect();
                freetext_parts.push(token);
            } else {
                // No '=' found — put back as freetext.
                let token: String = chars[start..i].iter().collect();
                freetext_parts.push(token);
            }
        } else {
            // Regular word.
            let word_start = i;
            while i < len && !chars[i].is_whitespace() {
                i += 1;
            }
            let word: String = chars[word_start..i].iter().collect();
            freetext_parts.push(word);
        }
    }

    (freetext_parts.join(" "), filters)
}

/// A text pattern for field matching.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Case-insensitive substring match.
    Substring(String),
    /// Regular expression match.
    Regex(regex::Regex),
}

impl Pattern {
    /// Test whether a field value matches this pattern.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Pattern::Substring(s) => value.to_ascii_lowercase().contains(&s.to_ascii_lowercase()),
            Pattern::Regex(r) => r.is_match(value),
        }
    }
}

/// BPM range filter.
#[derive(Debug, Clone)]
pub struct BpmRange {
    /// Minimum BPM (inclusive).
    pub min: u16,
    /// Maximum BPM (inclusive).
    pub max: u16,
}

impl BpmRange {
    /// Parse a BPM string: "120" -> 120-120, "120-128" -> 120-128.
    pub fn parse(s: &str) -> Option<BpmRange> {
        if let Some((a, b)) = s.split_once('-') {
            let min = a.trim().parse().ok()?;
            let max = b.trim().parse().ok()?;
            Some(BpmRange { min, max })
        } else {
            let v = s.trim().parse().ok()?;
            Some(BpmRange { min: v, max: v })
        }
    }

    /// Test whether a BPM value falls within the range.
    pub fn matches(&self, bpm: Option<u16>) -> bool {
        match bpm {
            Some(v) => v >= self.min && v <= self.max,
            None => false,
        }
    }
}

/// Whether field filters are combined with AND or OR.
#[derive(Debug, Clone, Default)]
pub enum MatchMode {
    /// All specified filters must match (default).
    #[default]
    And,
    /// Any specified filter can match.
    Or,
}

/// A compiled search query built from CLI arguments.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Filter by vendor.
    pub vendor: Option<Pattern>,
    /// Filter by library.
    pub library: Option<Pattern>,
    /// Filter by category.
    pub category: Option<Pattern>,
    /// Filter by Sound ID.
    pub sound_id: Option<Pattern>,
    /// Filter by description (searches description and comment).
    pub description: Option<Pattern>,
    /// Filter by BPM range.
    pub bpm: Option<BpmRange>,
    /// Filter by musical key.
    pub key: Option<Pattern>,
    /// AND or OR logic.
    pub match_mode: MatchMode,
    /// Free-text search across all text fields (for TUI search box).
    pub freetext: Option<String>,
    /// Columnar filters from `@field=value` syntax.
    pub column_filters: Vec<ColumnFilter>,
}

impl SearchQuery {
    /// Returns true if no filters are set (matches everything).
    pub fn is_empty(&self) -> bool {
        self.vendor.is_none()
            && self.library.is_none()
            && self.category.is_none()
            && self.sound_id.is_none()
            && self.description.is_none()
            && self.bpm.is_none()
            && self.key.is_none()
            && self.freetext_is_empty()
            && self.column_filters.is_empty()
    }

    /// Returns true if freetext is None or empty string.
    fn freetext_is_empty(&self) -> bool {
        match &self.freetext {
            None => true,
            Some(s) => s.is_empty(),
        }
    }

    /// Test whether metadata matches this query.
    pub fn matches(&self, meta: &UnifiedMetadata) -> bool {
        if self.is_empty() {
            return true;
        }

        // Free-text: OR across all text fields (case-insensitive substring).
        if let Some(text) = &self.freetext {
            if !text.is_empty() {
                let lower = text.to_ascii_lowercase();
                let any_match = meta.vendor.to_ascii_lowercase().contains(&lower)
                    || meta.library.to_ascii_lowercase().contains(&lower)
                    || meta.category.to_ascii_lowercase().contains(&lower)
                    || meta.sound_id.to_ascii_lowercase().contains(&lower)
                    || meta.description.to_ascii_lowercase().contains(&lower)
                    || meta.comment.to_ascii_lowercase().contains(&lower)
                    || meta.key.to_ascii_lowercase().contains(&lower)
                    || meta.path.to_string_lossy().to_ascii_lowercase().contains(&lower);
                if !any_match {
                    return false;
                }
            }
        }

        let checks: Vec<Option<bool>> = vec![
            self.vendor.as_ref().map(|p| p.matches(&meta.vendor)),
            self.library.as_ref().map(|p| p.matches(&meta.library)),
            self.category.as_ref().map(|p| p.matches(&meta.category)),
            self.sound_id.as_ref().map(|p| p.matches(&meta.sound_id)),
            self.description.as_ref().map(|p| {
                p.matches(&meta.description) || p.matches(&meta.comment)
            }),
            self.bpm.as_ref().map(|r| r.matches(meta.bpm)),
            self.key.as_ref().map(|p| p.matches(&meta.key)),
        ];

        let active: Vec<bool> = checks.into_iter().flatten().collect();
        if !active.is_empty() {
            let field_match = match self.match_mode {
                MatchMode::And => active.iter().all(|&b| b),
                MatchMode::Or => active.iter().any(|&b| b),
            };
            if !field_match {
                return false;
            }
        }

        // Column filters: each filter must match (AND semantics).
        for filter in &self.column_filters {
            let value = meta_field_value(meta, &filter.field);
            let is_numeric = NUMERIC_COLUMNS.contains(&filter.field.as_str());

            let filter_match = filter.values.iter().any(|fv| {
                if is_numeric {
                    // Exact numeric comparison.
                    value == *fv
                } else {
                    // Case-insensitive substring.
                    value.to_ascii_lowercase().contains(&fv.to_ascii_lowercase())
                }
            });
            if !filter_match {
                return false;
            }
        }

        true
    }
}

/// Extract a metadata field value by column name (for filesystem columnar filtering).
pub fn meta_field_value(meta: &UnifiedMetadata, field: &str) -> String {
    match field {
        "name" => meta
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "vendor" => meta.vendor.clone(),
        "library" => meta.library.clone(),
        "category" => meta.category.clone(),
        "sound_id" => meta.sound_id.clone(),
        "description" => meta.description.clone(),
        "comment" => meta.comment.clone(),
        "key" => meta.key.clone(),
        "bpm" => meta
            .bpm
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "rating" => meta.rating.clone(),
        "subcategory" => meta.subcategory.clone(),
        "genre_id" => meta.genre_id.clone(),
        "usage_id" => meta.usage_id.clone(),
        "date" => meta.date.clone(),
        "take" => meta.take.clone(),
        "track" => meta.track.clone(),
        "item" => meta.item.clone(),
        "parent_folder" => meta
            .path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Numeric columns for exact-match comparison in columnar filters.
const NUMERIC_COLUMNS: &[&str] = &["bpm", "sample_rate", "bit_depth", "channels"];

/// Build a [`Pattern`] from a string and a regex flag.
pub fn make_pattern(s: &str, is_regex: bool) -> Result<Pattern, regex::Error> {
    if is_regex {
        Ok(Pattern::Regex(regex::Regex::new(s)?))
    } else {
        Ok(Pattern::Substring(s.to_string()))
    }
}

/// Build a [`SearchQuery`] from parsed CLI options.
pub fn build_query(opts: &cli::Opts) -> anyhow::Result<SearchQuery> {
    let pat = |s: &Option<String>| -> anyhow::Result<Option<Pattern>> {
        match s {
            Some(s) => Ok(Some(make_pattern(s, opts.regex)?)),
            None => Ok(None),
        }
    };

    Ok(SearchQuery {
        vendor: pat(&opts.vendor)?,
        library: pat(&opts.library)?,
        category: pat(&opts.category)?,
        sound_id: pat(&opts.sound_id)?,
        description: pat(&opts.description)?,
        bpm: opts.bpm.as_ref().and_then(|s| BpmRange::parse(s)),
        key: pat(&opts.key)?,
        match_mode: if opts.or_mode {
            MatchMode::Or
        } else {
            MatchMode::And
        },
        freetext: None,
        column_filters: Vec::new(),
    })
}

// --- Search mode selection ---

/// Which search backend to use.
enum SearchMode {
    /// SQLite index search.
    Sqlite(PathBuf),
    /// Filesystem walk.
    Filesystem(Vec<PathBuf>),
}

/// Determine the search mode based on CLI options and DB existence.
fn determine_mode(opts: &cli::Opts) -> anyhow::Result<SearchMode> {
    // --no-db always forces filesystem.
    if opts.no_db {
        let roots = resolve_roots(&opts.paths)?;
        return Ok(SearchMode::Filesystem(roots));
    }

    // Check if DB exists.
    let db_path = sqlite::resolve_db_path(opts.db_path.as_deref())?;
    if db_path.exists() {
        Ok(SearchMode::Sqlite(db_path))
    } else {
        let roots = resolve_roots(&opts.paths)?;
        Ok(SearchMode::Filesystem(roots))
    }
}

/// Resolve search root paths from CLI args.
fn resolve_roots(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    if paths.is_empty() {
        Ok(vec![std::env::current_dir()?])
    } else {
        Ok(paths.to_vec())
    }
}

/// Run the search pipeline with the given CLI options.
pub fn run(opts: cli::Opts) -> anyhow::Result<()> {
    // Dispatch to subcommands first.
    if opts.index {
        return run_index(&opts);
    }
    if opts.db_stats {
        return run_db_stats(&opts);
    }
    if opts.is_workflow_mode() {
        return run_workflow(&opts);
    }

    use std::io::{BufWriter, Write};
    use std::thread;

    let query = build_query(&opts)?;
    let mode = opts.output_mode();

    let (tx, rx) = crossbeam_channel::bounded::<UnifiedMetadata>(2048);

    let search_mode = determine_mode(&opts)?;
    let walker = match search_mode {
        SearchMode::Sqlite(db_path) => {
            let query = query.clone();
            thread::spawn(move || {
                let db = match sqlite::Database::open(&db_path) {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!("riffgrep: database error: {e}");
                        return;
                    }
                };
                db.search(&query, &tx);
            })
        }
        SearchMode::Filesystem(roots) => {
            let finder = filesystem::FilesystemFinder::new(roots, opts.threads);
            let walk_query = query.clone();
            thread::spawn(move || {
                finder.walk(&walk_query, tx);
            })
        }
    };

    // Consume results on main thread.
    let stdout = std::io::stdout().lock();
    let mut out = BufWriter::new(stdout);
    let mut count: usize = 0;
    let mut had_write_error = false;

    for meta in rx {
        count += 1;
        let result = match mode {
            cli::OutputMode::Path => writeln!(out, "{}", meta.path.display()),
            cli::OutputMode::Verbose => write_verbose(&mut out, &meta),
            cli::OutputMode::Json => {
                let json = serde_json::to_string(&meta)?;
                writeln!(out, "{json}")
            }
            cli::OutputMode::Count => Ok(()), // Count only at the end.
        };

        if let Err(e) = result {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                had_write_error = true;
                break;
            }
            return Err(e.into());
        }
    }

    if !had_write_error {
        if matches!(mode, cli::OutputMode::Count) {
            let _ = writeln!(out, "{count} matches");
        }
        let _ = out.flush();
    }

    // Wait for walker to finish (it may have already quit via Quit state).
    let _ = walker.join();

    if count == 0 {
        std::process::exit(1);
    }

    Ok(())
}

// --- Workflow pipeline ---

/// Run the `--eval` / `--workflow` subcommand.
///
/// Loads the Lua script, walks matching files, runs the script against each
/// file's metadata, prints a diff, and optionally writes changes back.
fn run_workflow(opts: &cli::Opts) -> anyhow::Result<()> {
    use std::io::{BufWriter, Write};
    use std::thread;

    let script = workflow::load_workflow_script(
        opts.eval.as_deref(),
        opts.workflow.as_deref(),
    )?
    .unwrap_or_default();

    let query = build_query(opts)?;
    let (tx, rx) = crossbeam_channel::bounded::<UnifiedMetadata>(2048);

    let search_mode = determine_mode(opts)?;
    let _walker = match search_mode {
        SearchMode::Sqlite(db_path) => {
            let query = query.clone();
            thread::spawn(move || {
                let db = match sqlite::Database::open(&db_path) {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!("riffgrep: database error: {e}");
                        return;
                    }
                };
                db.search(&query, &tx);
            })
        }
        SearchMode::Filesystem(roots) => {
            let finder = filesystem::FilesystemFinder::new(roots, opts.threads);
            let walk_query = query.clone();
            thread::spawn(move || {
                finder.walk(&walk_query, tx);
            })
        }
    };

    let stdout = std::io::stdout().lock();
    let mut out = BufWriter::new(stdout);
    let mut scanned: usize = 0;
    let mut changed: usize = 0;

    for mut meta in rx {
        if let Some(max) = opts.limit {
            if scanned >= max {
                break;
            }
        }

        scanned += 1;

        // Ensure Lua scripts always see absolute paths regardless of whether the
        // search root was given as a relative path (e.g. `.`).
        if let Ok(abs) = meta.path.canonicalize() {
            meta.path = abs;
        }

        let path = meta.path.clone();

        // Augment with ID3v2 data (fills fields not covered by BEXT/RIFF INFO).
        if let Ok(id3) = id3::read_id3_tags(&path) {
            id3::merge_id3_into_unified(&mut meta, &id3);
        }

        let new_meta = match workflow::run_lua_script(&script, meta.clone(), opts.force, opts.commit) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("riffgrep: lua error on {}: {e}", path.display());
                continue;
            }
        };

        let diff = workflow::compute_meta_diff(&meta, &new_meta);
        if diff.is_empty() {
            continue;
        }

        changed += 1;
        let _ = writeln!(out, "{}", path.display());
        let _ = write!(out, "{}", workflow::format_meta_diff(&diff));

        if opts.commit
            && let Err(e) = workflow::write_metadata_changes(&path, &meta, &new_meta, opts.force)
        {
            eprintln!("riffgrep: write error on {}: {e}", path.display());
        }
    }

    let mode = if opts.commit {
        if opts.force {
            "changes applied --force"
        } else {
            "changes applied"
        }
    } else if opts.force {
        "dry run --force — use --commit to apply"
    } else {
        "dry run — use --commit to apply"
    };
    let limit_info = opts
        .limit
        .map(|n| format!(", limit {n}"))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "{changed} files changed, {scanned} files scanned ({mode}{limit_info})"
    );
    let _ = out.flush();

    Ok(())
}

// --- Indexing pipeline ---

/// Run the `--index` subcommand: walk filesystem and populate SQLite database.
fn run_index(opts: &cli::Opts) -> anyhow::Result<()> {
    use ignore::types::TypesBuilder;
    use ignore::WalkBuilder;

    let db_path = sqlite::resolve_db_path(opts.db_path.as_deref())?;
    let db = sqlite::Database::open(&db_path)?;

    let roots = resolve_roots(&opts.paths)?;

    // For incremental indexing, get existing mtimes.
    let existing_mtimes = if opts.force_reindex {
        std::collections::HashMap::new()
    } else {
        db.get_path_mtimes()?
    };

    let start = Instant::now();

    // Build walker (reuse same config as FilesystemFinder).
    let mut types = TypesBuilder::new();
    types.add("wav", "*.wav").expect("valid glob");
    types.add("wav", "*.WAV").expect("valid glob");
    types.select("wav");
    let types = types.build().expect("valid types");

    let mut builder = WalkBuilder::new(&roots[0]);
    for root in &roots[1..] {
        builder.add(root);
    }
    builder.types(types);
    builder.hidden(false);
    builder.git_ignore(false);
    if opts.threads > 0 {
        builder.threads(opts.threads);
    }

    // Channel for walker → writer: (metadata, mtime, compressed_peaks, peaks_source, audio_info, markers).
    let (tx, rx) = crossbeam_channel::bounded::<(
        UnifiedMetadata,
        i64,
        Option<Vec<u8>>,
        String,
        Option<wav::AudioInfo>,
        Option<bext::MarkerConfig>,
    )>(2048);

    // Track all discovered paths for deletion detection.
    let (path_tx, path_rx) = crossbeam_channel::unbounded::<PathBuf>();

    // Spawn writer thread.
    let writer = std::thread::spawn(move || -> anyhow::Result<usize> {
        db.index_writer_with_audio(&rx, 1000)
    });

    // Walk in parallel.
    let existing_mtimes_ref = &existing_mtimes;
    let regenerate_peaks = opts.regenerate_peaks;
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        let path_tx = path_tx.clone();
        Box::new(move |entry| {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("riffgrep: {e}");
                    return ignore::WalkState::Continue;
                }
            };

            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                return ignore::WalkState::Continue;
            }

            let path = entry.path().to_path_buf();
            let _ = path_tx.send(path.clone());

            // Check mtime for incremental indexing.
            let mtime = sqlite::file_mtime(&path).unwrap_or(0);
            if let Some(&stored_mtime) = existing_mtimes_ref.get(&path) {
                if stored_mtime == mtime {
                    return ignore::WalkState::Continue; // unchanged
                }
            }

            match read_metadata_with_peaks_format(&path) {
                Ok((meta, bext_peaks, peaks_format, markers)) => {
                    // Determine whether to generate peaks from audio.
                    // Only trust peaks from RiffgrepU8 format (our packed schema
                    // with bext_version >= 1). All other cases (Empty, BwfReserved)
                    // must compute peaks from the actual audio data.
                    let (peaks, source) =
                        if !regenerate_peaks
                            && peaks_format == bext::PeaksFormat::RiffgrepU8
                        {
                            (bext_peaks, peaks_format.source_str().to_string())
                        } else {
                            match wav::compute_peaks_stereo_from_path(&path) {
                                Ok(p) if !p.is_empty() && p.iter().any(|&v| v > 0) => {
                                    (p, "generated".to_string())
                                }
                                _ => (Vec::new(), "none".to_string()),
                            }
                        };

                    let compressed_peaks = if peaks.is_empty() {
                        None
                    } else {
                        Some(sqlite::compress_peaks(&peaks))
                    };

                    // Compute audio info for the DB.
                    let audio_info = (|| {
                        let file = std::fs::File::open(&path).ok()?;
                        let mut rdr = std::io::BufReader::with_capacity(8192, file);
                        let map = bext::scan_chunks(&mut rdr).ok()?;
                        let fmt = wav::parse_fmt(&mut rdr, &map).ok()?;
                        Some(wav::AudioInfo::from_fmt(&fmt, map.data_size))
                    })();

                    if tx.send((meta, mtime, compressed_peaks, source, audio_info, markers)).is_err() {
                        return ignore::WalkState::Quit;
                    }
                }
                Err(_) => {
                    // Skip files we can't parse.
                }
            }

            ignore::WalkState::Continue
        })
    });

    // Drop senders so writer knows we're done.
    drop(tx);
    drop(path_tx);

    let indexed = writer.join().map_err(|_| anyhow::anyhow!("writer thread panicked"))??;

    // Collect discovered paths for deletion detection.
    let discovered: HashSet<PathBuf> = path_rx.iter().collect();

    // Delete rows for files that no longer exist.
    if !opts.force_reindex && !existing_mtimes.is_empty() {
        let to_delete: Vec<&Path> = existing_mtimes
            .keys()
            .filter(|p| !discovered.contains(p.as_path()))
            .map(|p| p.as_path())
            .collect();

        if !to_delete.is_empty() {
            let db = sqlite::Database::open(&db_path)?;
            let deleted = db.delete_paths(&to_delete)?;
            if deleted > 0 {
                eprintln!("Removed {deleted} stale entries");
            }
        }
    }

    let elapsed = start.elapsed();
    eprintln!("Indexed {indexed} files in {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

// --- DB stats ---

/// Run the `--db-stats` subcommand.
fn run_db_stats(opts: &cli::Opts) -> anyhow::Result<()> {
    use std::io::Write;

    let db_path = sqlite::resolve_db_path(opts.db_path.as_deref())?;
    if !db_path.exists() {
        anyhow::bail!("database not found: {}", db_path.display());
    }

    let db = sqlite::Database::open(&db_path)?;
    let stats = db.stats()?;
    let (stale, sampled) = db.check_staleness(100)?;

    let file_size = std::fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let stdout = std::io::stdout().lock();
    let mut out = std::io::BufWriter::new(stdout);

    writeln!(out, "Database: {}", db_path.display())?;
    writeln!(out, "Size:     {}", format_size(file_size))?;
    writeln!(out, "Files:    {}", format_count(stats.file_count))?;

    if let Some(mtime) = stats.last_mtime {
        writeln!(out, "Last indexed: {}", format_timestamp(mtime))?;
    }

    if sampled > 0 {
        writeln!(out, "Stale paths:  {stale} (of {sampled} sampled)")?;
    }

    if !stats.top_vendors.is_empty() {
        writeln!(out)?;
        writeln!(out, "Top vendors:")?;
        for (vendor, count) in &stats.top_vendors {
            writeln!(out, "  {vendor:<24} {}", format_count(*count as u64))?;
        }
    }

    if !stats.peaks_breakdown.is_empty() {
        writeln!(out)?;
        writeln!(out, "Peaks:")?;
        for (source, count) in &stats.peaks_breakdown {
            writeln!(out, "  {source:<24} {}", format_count(*count as u64))?;
        }
    }

    out.flush()?;
    Ok(())
}

/// Format a byte size as human-readable.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Format a count with comma separators.
fn format_count(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format a Unix timestamp as UTC.
fn format_timestamp(epoch: i64) -> String {
    // Simple UTC formatting without pulling in chrono.
    let secs_per_day: i64 = 86400;
    let days = epoch / secs_per_day;
    let time_of_day = epoch % secs_per_day;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since Unix epoch to Y-M-D (simplified).
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02} UTC")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Civil calendar algorithm from Howard Hinnant.
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Write verbose output for a single result.
fn write_verbose<W: std::io::Write>(out: &mut W, meta: &UnifiedMetadata) -> std::io::Result<()> {
    writeln!(out, "{}", meta.path.display())?;
    let fields: &[(&str, &str)] = &[
        ("vendor", &meta.vendor),
        ("library", &meta.library),
        ("category", &meta.category),
        ("sound_id", &meta.sound_id),
        ("description", &meta.description),
        ("comment", &meta.comment),
        ("subcategory", &meta.subcategory),
        ("genre_id", &meta.genre_id),
        ("usage_id", &meta.usage_id),
        ("key", &meta.key),
        ("rating", &meta.rating),
        ("umid", &meta.umid),
    ];
    for (name, value) in fields {
        if !value.is_empty() {
            writeln!(out, "  {name}: {value}")?;
        }
    }
    if let Some(bpm) = meta.bpm {
        writeln!(out, "  bpm: {bpm}")?;
    }
    if meta.file_id != 0 {
        writeln!(out, "  file_id: {:016x}", meta.file_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_metadata_all_riff_info() {
        let path = Path::new("test_files/all_riff_info_tags_with_numbers.wav");
        if !path.exists() {
            return;
        }
        let meta = read_metadata(path).unwrap();
        // BEXT is empty, so all fields come from RIFF INFO.
        assert_eq!(meta.vendor, "IART-Artist");
        assert_eq!(meta.library, "INAM-Name/Title");
        assert_eq!(meta.category, "IGNR-Genre");
        assert_eq!(meta.sound_id, "IKEY-Keywords");
        assert_eq!(meta.description, "ICMT-Comment");
    }

    #[test]
    fn read_metadata_clean_base() {
        let path = Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        let meta = read_metadata(path).unwrap();
        assert_eq!(meta.description, "Yamaha DX-100");
        // No BEXT originator, no RIFF INFO → empty.
        assert_eq!(meta.vendor, "");
        assert_eq!(meta.category, "");
    }

    #[test]
    fn read_metadata_reaper_sm() {
        let path = Path::new("test_files/riff+defaults-info_reaper-sm.wav");
        if !path.exists() {
            return;
        }
        let meta = read_metadata(path).unwrap();
        assert_eq!(meta.description, "project note");
        // LIST-INFO is past 4KB, so no INFO merge.
    }

    #[test]
    fn substring_case_insensitive() {
        let p = Pattern::Substring("mars".to_string());
        assert!(p.matches("Samples From Mars"));
        assert!(p.matches("MARS ATTACKS"));
        assert!(!p.matches("Jupiter"));
    }

    #[test]
    fn regex_pattern() {
        let p = Pattern::Regex(regex::Regex::new("DX\\d+").unwrap());
        assert!(p.matches("DX100 From Mars"));
        assert!(!p.matches("Jupiter"));
    }

    #[test]
    fn and_mode_all_must_match() {
        let q = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::And,
            ..Default::default()
        };
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.category = "LOOP".to_string();
        assert!(q.matches(&meta));

        meta.category = "ONESHOT".to_string();
        assert!(!q.matches(&meta));
    }

    #[test]
    fn or_mode_any_can_match() {
        let q = SearchQuery {
            vendor: Some(Pattern::Substring("mars".to_string())),
            category: Some(Pattern::Substring("loop".to_string())),
            match_mode: MatchMode::Or,
            ..Default::default()
        };
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.category = "ONESHOT".to_string();
        assert!(q.matches(&meta)); // vendor matches, that's enough

        meta.vendor = "Jupiter".to_string();
        assert!(!q.matches(&meta)); // neither matches
    }

    #[test]
    fn bpm_range_single() {
        let r = BpmRange::parse("120").unwrap();
        assert_eq!(r.min, 120);
        assert_eq!(r.max, 120);
        assert!(r.matches(Some(120)));
        assert!(!r.matches(Some(121)));
        assert!(!r.matches(None));
    }

    #[test]
    fn bpm_range_span() {
        let r = BpmRange::parse("120-128").unwrap();
        assert!(r.matches(Some(124)));
        assert!(!r.matches(Some(140)));
        assert!(!r.matches(None));
    }

    #[test]
    fn empty_query_matches_everything() {
        let q = SearchQuery::default();
        let meta = UnifiedMetadata::default();
        assert!(q.matches(&meta));
    }

    #[test]
    fn description_searches_comment_too() {
        let q = SearchQuery {
            description: Some(Pattern::Substring("prophet".to_string())),
            ..Default::default()
        };
        let mut meta = UnifiedMetadata::default();
        meta.description = "plain text".to_string();
        meta.comment = "Sequential Circuits Prophet-10".to_string();
        assert!(q.matches(&meta));
    }

    // --- Ticket 7: Dual-mode dispatch ---

    #[test]
    fn test_determine_mode_no_db_flag() {
        let opts = cli::Opts {
            no_db: true,
            paths: vec![PathBuf::from("test_files")],
            ..default_opts()
        };
        let mode = determine_mode(&opts).unwrap();
        assert!(matches!(mode, SearchMode::Filesystem(_)));
    }

    #[test]
    fn test_determine_mode_db_missing() {
        // Point to a valid dir but a non-existent .db file.
        let dir = std::env::temp_dir().join("riffgrep_test_mode_missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("does_not_exist.db");

        let opts = cli::Opts {
            no_db: false,
            db_path: Some(db_path),
            paths: vec![PathBuf::from("test_files")],
            ..default_opts()
        };
        let mode = determine_mode(&opts).unwrap();
        assert!(matches!(mode, SearchMode::Filesystem(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_determine_mode_db_exists() {
        let dir = std::env::temp_dir().join("riffgrep_test_mode");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        // Create a real DB.
        let _db = sqlite::Database::open(&db_path).unwrap();
        drop(_db);

        let opts = cli::Opts {
            no_db: false,
            db_path: Some(db_path),
            paths: vec![PathBuf::from("test_files")],
            ..default_opts()
        };
        let mode = determine_mode(&opts).unwrap();
        assert!(matches!(mode, SearchMode::Sqlite(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1,000");
        assert_eq!(format_count(1234567), "1,234,567");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 bytes");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
    }

    // --- Ticket T2: Free-text filesystem matching ---

    #[test]
    fn test_freetext_filesystem_matches_any_field() {
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            freetext: Some("mars".to_string()),
            ..Default::default()
        };
        assert!(query.matches(&meta), "freetext should match vendor");

        let mut meta2 = UnifiedMetadata::default();
        meta2.description = "punchy kick drum".to_string();
        meta2.path = PathBuf::from("/test/file.wav");
        let query2 = SearchQuery {
            freetext: Some("kick".to_string()),
            ..Default::default()
        };
        assert!(query2.matches(&meta2), "freetext should match description");
    }

    #[test]
    fn test_freetext_empty_matches_all() {
        let meta = UnifiedMetadata::default();

        let query_none = SearchQuery {
            freetext: None,
            ..Default::default()
        };
        assert!(query_none.matches(&meta));

        let query_empty = SearchQuery {
            freetext: Some(String::new()),
            ..Default::default()
        };
        assert!(query_empty.matches(&meta));
    }

    #[test]
    fn test_freetext_no_match() {
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Splice".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            freetext: Some("zzzznonexistent".to_string()),
            ..Default::default()
        };
        assert!(!query.matches(&meta));
    }

    // --- S6-T4 tests: Columnar filter parser ---

    #[test]
    fn test_parse_single_filter() {
        let (freetext, filters) = parse_column_filters("@vendor=Mars");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[0].values, vec!["Mars"]);
    }

    #[test]
    fn test_parse_multi_value_filter() {
        let (freetext, filters) = parse_column_filters("@bpm=[120,125]");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "bpm");
        assert_eq!(filters[0].values, vec!["120", "125"]);
    }

    #[test]
    fn test_parse_mixed_freetext_and_filter() {
        let (freetext, filters) = parse_column_filters("kick @vendor=Mars");
        assert_eq!(freetext, "kick");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[0].values, vec!["Mars"]);
    }

    #[test]
    fn test_parse_multiple_filters() {
        let (freetext, filters) = parse_column_filters("@vendor=Mars @category=LOOP");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[1].field, "category");
    }

    #[test]
    fn test_parse_invalid_field_stays_freetext() {
        let (freetext, filters) = parse_column_filters("@foo=bar");
        assert_eq!(freetext, "@foo=bar");
        assert!(filters.is_empty());
    }

    #[test]
    fn test_parse_no_filters() {
        let (freetext, filters) = parse_column_filters("just some text");
        assert_eq!(freetext, "just some text");
        assert!(filters.is_empty());
    }

    #[test]
    fn test_parse_empty_value_ignored() {
        let (freetext, filters) = parse_column_filters("@vendor=");
        assert_eq!(freetext, "@vendor=");
        assert!(filters.is_empty());
    }

    #[test]
    fn test_parse_case_insensitive_field() {
        let (freetext, filters) = parse_column_filters("@Vendor=Mars");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[0].values, vec!["Mars"]);
    }

    #[test]
    fn test_parse_quoted_value_with_spaces() {
        let (freetext, filters) = parse_column_filters("@vendor=\"Beat MPC\"");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[0].values, vec!["Beat MPC"]);
    }

    #[test]
    fn test_parse_quoted_value_mixed_with_freetext() {
        let (freetext, filters) = parse_column_filters("kick @vendor=\"Samples From Mars\" drum");
        assert_eq!(freetext, "kick drum");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].field, "vendor");
        assert_eq!(filters[0].values, vec!["Samples From Mars"]);
    }

    #[test]
    fn test_parse_case_insensitive_and_quoted() {
        let (freetext, filters) =
            parse_column_filters("@Library=\"DX100 From Mars\" @Vendor=\"Samples From Mars\"");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 2);
        assert_eq!(filters[0].field, "library");
        assert_eq!(filters[0].values, vec!["DX100 From Mars"]);
        assert_eq!(filters[1].field, "vendor");
        assert_eq!(filters[1].values, vec!["Samples From Mars"]);
    }

    #[test]
    fn test_parse_unclosed_quote_reads_to_end() {
        let (freetext, filters) = parse_column_filters("@vendor=\"unclosed");
        assert_eq!(freetext, "");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].values, vec!["unclosed"]);
    }

    #[test]
    fn test_parse_empty_quoted_value_ignored() {
        let (freetext, filters) = parse_column_filters("@vendor=\"\"");
        assert_eq!(freetext, "@vendor=\"\"");
        assert!(filters.is_empty());
    }

    // --- S6-T6 tests: Filesystem columnar filter ---

    #[test]
    fn test_column_filter_matches_vendor() {
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            column_filters: vec![ColumnFilter {
                field: "vendor".to_string(),
                values: vec!["Mars".to_string()],
            }],
            ..Default::default()
        };
        assert!(query.matches(&meta));
    }

    #[test]
    fn test_column_filter_case_insensitive() {
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            column_filters: vec![ColumnFilter {
                field: "vendor".to_string(),
                values: vec!["mars".to_string()],
            }],
            ..Default::default()
        };
        assert!(query.matches(&meta), "text filter should be case-insensitive");
    }

    #[test]
    fn test_column_filter_bpm_exact() {
        let mut meta = UnifiedMetadata::default();
        meta.bpm = Some(120);
        meta.path = PathBuf::from("/test/file.wav");

        let query_match = SearchQuery {
            column_filters: vec![ColumnFilter {
                field: "bpm".to_string(),
                values: vec!["120".to_string()],
            }],
            ..Default::default()
        };
        assert!(query_match.matches(&meta));

        let query_no = SearchQuery {
            column_filters: vec![ColumnFilter {
                field: "bpm".to_string(),
                values: vec!["121".to_string()],
            }],
            ..Default::default()
        };
        assert!(!query_no.matches(&meta));
    }

    #[test]
    fn test_column_filter_multi_value_or() {
        let mut meta = UnifiedMetadata::default();
        meta.category = "LOOP".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            column_filters: vec![ColumnFilter {
                field: "category".to_string(),
                values: vec!["SFX".to_string(), "LOOP".to_string()],
            }],
            ..Default::default()
        };
        assert!(query.matches(&meta), "multi-value filter should OR");
    }

    #[test]
    fn test_column_filter_multiple_filters_and() {
        let mut meta = UnifiedMetadata::default();
        meta.vendor = "Samples From Mars".to_string();
        meta.category = "LOOP".to_string();
        meta.path = PathBuf::from("/test/file.wav");

        let query = SearchQuery {
            column_filters: vec![
                ColumnFilter {
                    field: "vendor".to_string(),
                    values: vec!["Mars".to_string()],
                },
                ColumnFilter {
                    field: "category".to_string(),
                    values: vec!["LOOP".to_string()],
                },
            ],
            ..Default::default()
        };
        assert!(query.matches(&meta), "multiple filters should AND");

        let mut meta2 = meta.clone();
        meta2.category = "ONESHOT".to_string();
        assert!(!query.matches(&meta2), "second filter should fail");
    }

    /// Helper to construct default opts for testing.
    fn default_opts() -> cli::Opts {
        cli::Opts {
            vendor: None,
            library: None,
            category: None,
            sound_id: None,
            description: None,
            bpm: None,
            key: None,
            regex: false,
            or_mode: false,
            verbose: false,
            json: false,
            count: false,
            threads: 0,
            no_db: false,
            index: false,
            db_path: None,
            db_stats: false,
            force_reindex: false,
            regenerate_peaks: false,
            no_tui: false,
            theme: None,
            session_bpm: None,
            eval: None,
            workflow: None,
            commit: false,
            force: false,
            limit: None,
            paths: vec![],
        }
    }
}
