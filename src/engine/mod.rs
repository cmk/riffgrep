//! Core search engine: metadata reading, matching, and output.

pub mod bext;
pub mod cli;
pub mod filesystem;
pub mod riff_info;

use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Serialize;

use bext::{BextFields, RiffError};
use riff_info::InfoFields;

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
    /// `[000:008]` SM recid (u64 LE).
    pub recid: u64,
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

/// Merge BEXT and INFO fields. BEXT takes priority; INFO fills empty fields.
fn merge_metadata(path: &Path, bext: BextFields, info: InfoFields) -> UnifiedMetadata {
    let mut meta = UnifiedMetadata {
        path: path.to_path_buf(),
        vendor: bext.vendor,
        library: bext.library,
        description: bext.description,
        umid: bext.umid,
        recid: bext.recid,
        comment: bext.comment,
        rating: bext.rating,
        bpm: bext.bpm,
        subcategory: bext.subcategory,
        category: bext.category,
        genre_id: bext.genre_id,
        sound_id: bext.sound_id,
        usage_id: bext.usage_id,
        key: bext.key,
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
    }

    /// Test whether metadata matches this query.
    pub fn matches(&self, meta: &UnifiedMetadata) -> bool {
        if self.is_empty() {
            return true;
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
        if active.is_empty() {
            return true;
        }

        match self.match_mode {
            MatchMode::And => active.iter().all(|&b| b),
            MatchMode::Or => active.iter().any(|&b| b),
        }
    }
}

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
    })
}

/// Run the search pipeline with the given CLI options.
pub fn run(opts: cli::Opts) -> anyhow::Result<()> {
    use std::io::{Write, BufWriter};
    use std::thread;

    let query = build_query(&opts)?;
    let mode = opts.output_mode();

    let roots = if opts.paths.is_empty() {
        vec![std::env::current_dir()?]
    } else {
        opts.paths.clone()
    };

    let finder = filesystem::FilesystemFinder::new(roots, opts.threads);
    let (tx, rx) = crossbeam_channel::bounded::<UnifiedMetadata>(2048);

    // Spawn walker on background thread.
    let walk_query = query.clone();
    let walker = thread::spawn(move || {
        finder.walk(&walk_query, tx);
    });

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
    if meta.recid != 0 {
        writeln!(out, "  recid: {}", meta.recid)?;
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
}
