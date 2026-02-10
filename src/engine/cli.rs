//! CLI argument parsing with bpaf.

use std::path::PathBuf;

use bpaf::Bpaf;

/// Output format mode.
#[derive(Debug, Clone, Default)]
pub enum OutputMode {
    /// One path per line (default).
    #[default]
    Path,
    /// Path + indented metadata.
    Verbose,
    /// JSON Lines.
    Json,
    /// Total count only.
    Count,
}

/// Parsed CLI options.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(options, version)]
pub struct Opts {
    /// Filter by vendor (BEXT Originator / RIFF IART / TPE1)
    #[bpaf(long, short, argument("PATTERN"))]
    pub vendor: Option<String>,

    /// Filter by library (BEXT OriginatorReference / RIFF INAM / TPE2)
    #[bpaf(long, short('l'), argument("PATTERN"))]
    pub library: Option<String>,

    /// Filter by category (packed TCON / RIFF IGNR)
    #[bpaf(long, short('c'), argument("PATTERN"))]
    pub category: Option<String>,

    /// Filter by Sound ID (packed TIT2 / RIFF IKEY)
    #[bpaf(long("sound-id"), short('s'), argument("PATTERN"))]
    pub sound_id: Option<String>,

    /// Filter by description (BEXT Description / RIFF ICMT / packed COMR)
    #[bpaf(long, short('d'), argument("PATTERN"))]
    pub description: Option<String>,

    /// Filter by BPM (single value or range like "120-128")
    #[bpaf(long, argument("BPM"))]
    pub bpm: Option<String>,

    /// Filter by musical key (packed TKEY)
    #[bpaf(long, short('k'), argument("KEY"))]
    pub key: Option<String>,

    /// Treat filter patterns as regex
    #[bpaf(long)]
    pub regex: bool,

    /// Use OR logic (default is AND)
    #[bpaf(long("or"))]
    pub or_mode: bool,

    /// Verbose output (path + metadata)
    #[bpaf(long("verbose"))]
    pub verbose: bool,

    /// JSON Lines output
    #[bpaf(long)]
    pub json: bool,

    /// Count matches only
    #[bpaf(long)]
    pub count: bool,

    /// Number of search threads
    #[bpaf(long, argument("N"), fallback(0usize))]
    pub threads: usize,

    /// Databaseless mode — force filesystem walk even when DB exists
    #[bpaf(long("no-db"))]
    pub no_db: bool,

    /// Build or update the SQLite index
    #[bpaf(long)]
    pub index: bool,

    /// Explicit path to the SQLite database file
    #[bpaf(long("db-path"), argument("PATH"))]
    pub db_path: Option<PathBuf>,

    /// Display database index statistics
    #[bpaf(long("db-stats"))]
    pub db_stats: bool,

    /// Force full re-index (ignore mtime, reparse all files)
    #[bpaf(long("force-reindex"))]
    pub force_reindex: bool,

    /// Regenerate peaks from audio data (ignores existing BEXT peaks)
    #[bpaf(long("regenerate-peaks"))]
    pub regenerate_peaks: bool,

    /// Disable interactive TUI (force headless output)
    #[bpaf(long("no-tui"))]
    pub no_tui: bool,

    /// TUI color theme
    #[bpaf(long, argument("THEME"))]
    pub theme: Option<String>,

    /// Search paths (default: current directory)
    #[bpaf(positional("PATH"))]
    pub paths: Vec<PathBuf>,
}

const HELP_FOOTER: &str = "\
EXAMPLES:
  riffgrep                             Launch TUI browser
  riffgrep --vendor \"Mars\"             Search by vendor (headless)
  riffgrep --index ~/Samples           Build/update search index
  riffgrep --no-db ~/Samples           Search without database
  riffgrep --db-stats                  Show index health
  riffgrep --theme ableton             Launch TUI with theme
 .
TUI KEYS (Normal mode):
  i, /    Enter search mode       Esc, Ctrl-C  Exit search mode
  j/k     Navigate rows           h/l          Navigate columns
  o/O     Sort ascending/desc     Space        Play/pause
  s       Stop playback           m            Toggle mark
  M       Clear all marks         f            Filter to marked
  g/G     Jump to top/bottom      q            Quit
  ?       Show keybinding help
 .
CONFIG:
  ~/Library/Application Support/riffgrep/config.toml";

/// Build the CLI parser with rich help output.
pub fn opts_with_help() -> bpaf::OptionParser<Opts> {
    opts().header(
        "riffgrep — high-performance WAV sample library search\n\
         Search, browse, and play WAV files with BEXT/RIFF/ID3 metadata.",
    )
    .footer(HELP_FOOTER)
}

impl Opts {
    /// Determine the output mode from flags.
    pub fn output_mode(&self) -> OutputMode {
        if self.json {
            OutputMode::Json
        } else if self.verbose {
            OutputMode::Verbose
        } else if self.count {
            OutputMode::Count
        } else {
            OutputMode::Path
        }
    }

    /// Returns true if any search filter flags are set.
    pub fn has_search_filters(&self) -> bool {
        self.vendor.is_some()
            || self.library.is_some()
            || self.category.is_some()
            || self.sound_id.is_some()
            || self.description.is_some()
            || self.bpm.is_some()
            || self.key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help_text() -> String {
        opts_with_help()
            .run_inner(&["--help"])
            .unwrap_err()
            .unwrap_stdout()
    }

    #[test]
    fn test_help_contains_examples_section() {
        let help = help_text();
        assert!(
            help.contains("EXAMPLES:"),
            "help should contain EXAMPLES section:\n{help}"
        );
        assert!(
            help.contains("Launch TUI browser"),
            "help should show TUI launch example:\n{help}"
        );
    }

    #[test]
    fn test_help_contains_tui_keys_section() {
        let help = help_text();
        assert!(
            help.contains("TUI KEYS"),
            "help should contain TUI KEYS section:\n{help}"
        );
        assert!(
            help.contains("Navigate rows"),
            "help should show key descriptions:\n{help}"
        );
    }

    #[test]
    fn test_help_contains_config_path() {
        let help = help_text();
        assert!(
            help.contains("CONFIG:"),
            "help should contain CONFIG section:\n{help}"
        );
    }
}
