//! TOML configuration file loading and defaults.
//!
//! Config is loaded from the platform config directory. CLI flags override
//! config values. Missing or malformed file returns sensible defaults.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Scrubbing / seek configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScrubConfig {
    /// Small seek increment in seconds (default 0.1).
    pub small_increment: Option<f64>,
    /// Large seek increment in seconds (default 1.0).
    pub large_increment: Option<f64>,
    /// Auto-advance to next sample on playback completion (default false).
    pub auto_advance: Option<bool>,
}

/// Application configuration loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Columns to display in the table view.
    pub columns: Option<Vec<String>>,
    /// Peak measurement method: "rms" or "peak".
    pub peak_measurement: Option<String>,
    /// Peak channel mode: "mix" or "left".
    pub peak_channel: Option<String>,
    /// TUI color theme name.
    pub theme: Option<String>,
    /// Path to the marks CSV file (filesystem mode).
    pub marks_file: Option<String>,
    /// Default sort column name.
    pub default_sort: Option<String>,
    /// Default sort order: "asc" or "desc".
    pub default_sort_order: Option<String>,
    /// Normal mode key-to-action overrides (key name → action name).
    pub keymap: Option<HashMap<String, String>>,
    /// Scrubbing / seek settings.
    pub scrub: Option<ScrubConfig>,
}

/// Resolve scrub increments from config, with clamping and defaults.
///
/// Returns `(small_increment, large_increment)` in seconds.
pub fn resolve_scrub_increments(scrub: Option<&ScrubConfig>) -> (f64, f64) {
    let small = scrub
        .and_then(|s| s.small_increment)
        .unwrap_or(0.1)
        .clamp(0.01, 100.0);
    let large = scrub
        .and_then(|s| s.large_increment)
        .unwrap_or(1.0)
        .clamp(0.01, 100.0);
    (small, large)
}

/// Default column list for the metadata table.
pub fn default_columns() -> Vec<String> {
    [
        "vendor",
        "library",
        "date",
        "duration",
        "sample_rate",
        "bit_depth",
        "channels",
        "rating",
        "bpm",
        "subcategory",
        "category",
        "genre_id",
        "sound_id",
        "usage_id",
        "key",
        "take",
        "track",
        "item",
        "comment",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// All available column keys.
pub const AVAILABLE_COLUMNS: &[&str] = &[
    "name",
    "vendor",
    "library",
    "category",
    "sound_id",
    "description",
    "comment",
    "key",
    "bpm",
    "rating",
    "subcategory",
    "genre_id",
    "usage_id",
    "duration",
    "sample_rate",
    "bit_depth",
    "channels",
    "date",
    "take",
    "track",
    "item",
    "format",
    "parent_folder",
];

/// Column display definition.
pub struct ColumnDef {
    /// Config key name.
    pub key: &'static str,
    /// Header label text.
    pub label: &'static str,
    /// Minimum width in characters.
    pub min_width: u16,
    /// Relative width weight for proportional sizing.
    pub weight: u16,
}

/// Get the column definition for a key.
pub fn column_def(key: &str) -> Option<ColumnDef> {
    match key {
        "name" => Some(ColumnDef { key: "name", label: "Name", min_width: 10, weight: 4 }),
        "vendor" => Some(ColumnDef { key: "vendor", label: "Vendor", min_width: 12, weight: 3 }),
        "library" => Some(ColumnDef { key: "library", label: "Library", min_width: 16, weight: 3 }),
        "date" => Some(ColumnDef { key: "date", label: "Date", min_width: 8, weight: 2 }),
        "category" => Some(ColumnDef { key: "category", label: "CAT", min_width: 4, weight: 2 }),
        "sound_id" => Some(ColumnDef { key: "sound_id", label: "Snd ID", min_width: 6, weight: 2 }),
        "description" => Some(ColumnDef { key: "description", label: "Description", min_width: 10, weight: 4 }),
        "comment" => Some(ColumnDef { key: "comment", label: "Comment", min_width: 8, weight: 3 }),
        "key" => Some(ColumnDef { key: "key", label: "Key", min_width: 8, weight: 1 }),
        "bpm" => Some(ColumnDef { key: "bpm", label: "BPM", min_width: 4, weight: 1 }),
        "rating" => Some(ColumnDef { key: "rating", label: "Rating", min_width: 4, weight: 1 }),
        "subcategory" => Some(ColumnDef { key: "subcategory", label: "SUB", min_width: 4, weight: 1 }),
        "genre_id" => Some(ColumnDef { key: "genre_id", label: "Gen ID", min_width: 6, weight: 2 }),
        "usage_id" => Some(ColumnDef { key: "usage_id", label: "Use ID", min_width: 6, weight: 2 }),
        "duration" => Some(ColumnDef { key: "duration", label: "Length", min_width: 6, weight: 1 }),
        "sample_rate" => Some(ColumnDef { key: "sample_rate", label: "Rate", min_width: 4, weight: 1 }),
        "bit_depth" => Some(ColumnDef { key: "bit_depth", label: "Bits", min_width: 4, weight: 1 }),
        "channels" => Some(ColumnDef { key: "channels", label: "Ch", min_width: 3, weight: 1 }),
        "take" => Some(ColumnDef { key: "take", label: "Take", min_width: 4, weight: 1 }),
        "track" => Some(ColumnDef { key: "track", label: "Trck", min_width: 4, weight: 1 }),
        "item" => Some(ColumnDef { key: "item", label: "Item", min_width: 4, weight: 1 }),
        "format" => Some(ColumnDef { key: "format", label: "Format", min_width: 8, weight: 2 }),
        "parent_folder" => Some(ColumnDef { key: "parent_folder", label: "Folder", min_width: 8, weight: 3 }),
        _ => None,
    }
}

/// Resolve the config file path from environment or platform default.
///
/// Priority: `RIFFGREP_CONFIG` env var > platform default.
pub fn resolve_config_path() -> anyhow::Result<PathBuf> {
    if let Ok(env_path) = std::env::var("RIFFGREP_CONFIG") {
        return Ok(PathBuf::from(env_path));
    }
    default_config_path()
}

/// Platform-specific default config path.
fn default_config_path() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home =
            std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
        Ok(PathBuf::from(home).join("Library/Application Support/riffgrep/config.toml"))
    }
    #[cfg(target_os = "linux")]
    {
        let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/.config")
        });
        Ok(PathBuf::from(config_dir).join("riffgrep/config.toml"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow::anyhow!("HOME not set"))?;
        Ok(PathBuf::from(home).join(".riffgrep/config.toml"))
    }
}

/// Load configuration from the platform config path.
///
/// Never errors: returns `Config::default()` if file is missing or invalid.
/// Malformed TOML logs a warning to stderr.
pub fn load_config() -> Config {
    let path = match resolve_config_path() {
        Ok(p) => p,
        Err(_) => return Config::default(),
    };

    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Config::default(),
    };

    match toml::from_str::<Config>(&contents) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("riffgrep: warning: malformed config at {}: {e}", path.display());
            Config::default()
        }
    }
}

/// Resolve the marks CSV file path.
///
/// Priority: config `marks_file` > platform default alongside config.
pub fn resolve_marks_path(config: &Config) -> PathBuf {
    if let Some(ref p) = config.marks_file {
        return PathBuf::from(p);
    }
    match resolve_config_path() {
        Ok(p) => p.with_file_name("marks.csv"),
        Err(_) => PathBuf::from("riffgrep_marks.csv"),
    }
}

/// Load configuration from a specific path (for testing).
pub fn load_config_from(path: &std::path::Path) -> Config {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Config::default(),
    };

    match toml::from_str::<Config>(&contents) {
        Ok(config) => config,
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = Config::default();
        assert!(config.columns.is_none());
        assert!(config.peak_measurement.is_none());
        assert!(config.peak_channel.is_none());
        assert!(config.theme.is_none());
        assert!(config.marks_file.is_none());
    }

    #[test]
    fn test_config_roundtrip_toml() {
        let config = Config {
            columns: Some(vec!["name".to_string(), "vendor".to_string()]),
            peak_measurement: Some("rms".to_string()),
            peak_channel: Some("mix".to_string()),
            theme: Some("ableton".to_string()),
            marks_file: Some("/tmp/marks.csv".to_string()),
            default_sort: None,
            default_sort_order: None,
            keymap: None,
            scrub: None,
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.columns.unwrap(), vec!["name", "vendor"]);
        assert_eq!(parsed.peak_measurement.unwrap(), "rms");
        assert_eq!(parsed.peak_channel.unwrap(), "mix");
        assert_eq!(parsed.theme.unwrap(), "ableton");
        assert_eq!(parsed.marks_file.unwrap(), "/tmp/marks.csv");
    }

    #[test]
    fn test_config_missing_file_returns_default() {
        let path = std::path::Path::new("/tmp/riffgrep_nonexistent_config_test.toml");
        let _ = std::fs::remove_file(path);
        let config = load_config_from(path);
        assert!(config.columns.is_none());
    }

    #[test]
    fn test_config_partial_toml() {
        let dir = std::env::temp_dir().join("riffgrep_test_config_partial");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        std::fs::write(&path, "theme = \"soundminer\"\n").unwrap();
        let config = load_config_from(&path);
        assert_eq!(config.theme.as_deref(), Some("soundminer"));
        assert!(config.columns.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_keymap_toml() {
        let dir = std::env::temp_dir().join("riffgrep_test_config_keymap");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let toml_str = r#"
[keymap]
"j" = "move_up"
"k" = "move_down"
"Space" = "toggle_mark"
"#;
        std::fs::write(&path, toml_str).unwrap();
        let config = load_config_from(&path);
        let km = config.keymap.unwrap();
        assert_eq!(km.get("j").unwrap(), "move_up");
        assert_eq!(km.get("k").unwrap(), "move_down");
        assert_eq!(km.get("Space").unwrap(), "toggle_mark");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_default_columns() {
        let cols = default_columns();
        assert_eq!(cols, vec![
            "vendor", "library", "date", "duration",
            "sample_rate", "bit_depth", "channels",
            "rating", "bpm", "subcategory", "category",
            "genre_id", "sound_id", "usage_id",
            "key", "take", "track", "item", "comment",
        ]);
    }

    #[test]
    fn test_column_def_known() {
        let def = column_def("name").unwrap();
        assert_eq!(def.label, "Name");
        assert!(def.min_width > 0);
    }

    #[test]
    fn test_column_def_unknown() {
        assert!(column_def("nonexistent").is_none());
    }

    #[test]
    fn test_resolve_config_path_macos() {
        #[cfg(target_os = "macos")]
        {
            let path = default_config_path().unwrap();
            assert!(
                path.to_string_lossy().contains("Library/Application Support/riffgrep"),
                "expected macOS config path, got: {}",
                path.display()
            );
        }
    }

    // --- S8-T4 tests: Scrub config ---

    #[test]
    fn test_scrub_config_defaults() {
        let (small, large) = resolve_scrub_increments(None);
        assert!((small - 0.1).abs() < f64::EPSILON);
        assert!((large - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scrub_config_override() {
        let scrub = ScrubConfig {
            small_increment: Some(0.5),
            large_increment: Some(2.0),
            auto_advance: None,
        };
        let (small, large) = resolve_scrub_increments(Some(&scrub));
        assert!((small - 0.5).abs() < f64::EPSILON);
        assert!((large - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scrub_config_clamps_range() {
        let scrub = ScrubConfig {
            small_increment: Some(0.001),
            large_increment: Some(500.0),
            auto_advance: None,
        };
        let (small, large) = resolve_scrub_increments(Some(&scrub));
        assert!((small - 0.01).abs() < f64::EPSILON, "should clamp low to 0.01, got {small}");
        assert!((large - 100.0).abs() < f64::EPSILON, "should clamp high to 100.0, got {large}");
    }
}
