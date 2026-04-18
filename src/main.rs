//! riffgrep — high-performance WAV sample library metadata search.

use std::path::PathBuf;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod engine;
mod ui;
mod util;

fn main() {
    use std::io::IsTerminal;

    let opts = engine::cli::opts_with_help().run();
    let is_tty = std::io::stdout().is_terminal();

    let result = match dispatch(&opts, is_tty) {
        Dispatch::Headless => engine::run(opts),
        Dispatch::TuiBrowse | Dispatch::TuiSimilar(_) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(ui::run_tui(opts))
        }
    };
    if let Err(e) = result {
        eprintln!("riffgrep: {e}");
        std::process::exit(2);
    }
}

/// Where the CLI invocation should go.
///
/// Separated from `main()` so the predicate is pure and table-driven
/// unit-testable without coupling to `stdout().is_terminal()` or the
/// tokio runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Dispatch {
    /// `engine::run` — headless search, subcommand, or piped output.
    Headless,
    /// `ui::run_tui` — interactive browse TUI, no initial query.
    TuiBrowse,
    /// `ui::run_tui` — interactive TUI pre-loaded with similarity
    /// results for the given query path. The inner `PathBuf` is
    /// redundant with `opts.similar` but included in the variant so
    /// the dispatch decision is self-describing without reaching back
    /// into opts.
    TuiSimilar(PathBuf),
}

/// Classify a CLI invocation into one of the three dispatch paths.
///
/// Precedence rules (first match wins):
/// 1. Any output-forcing flag or non-TTY stdout → `Headless`.
/// 2. `--similar PATH` → `TuiSimilar(PATH)`.
/// 3. Any non-empty search filter → `Headless`.
/// 4. Otherwise → `TuiBrowse`.
///
/// Rule 1 subsumes `--no-tui`, `--verbose`, `--json`, `--count`,
/// `--index`, `--db-stats`, any workflow flag (`--eval`/`--workflow`),
/// and stdout-not-a-TTY. Rule 3 preserves the historical behavior
/// where filter flags at a terminal go headless (e.g. `rfg --vendor
/// Mars` prints paths instead of launching the TUI).
fn dispatch(opts: &engine::cli::Opts, is_tty: bool) -> Dispatch {
    if opts.no_tui
        || opts.verbose
        || opts.json
        || opts.count
        || opts.index
        || opts.db_stats
        || opts.is_workflow_mode()
        || !is_tty
    {
        return Dispatch::Headless;
    }

    if let Some(path) = &opts.similar {
        return Dispatch::TuiSimilar(path.clone());
    }

    if opts.has_search_filters() {
        return Dispatch::Headless;
    }

    Dispatch::TuiBrowse
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> engine::cli::Opts {
        engine::cli::opts_with_help().run_inner(args).unwrap()
    }

    #[test]
    fn similar_at_tty_routes_to_tui_similar() {
        let opts = parse(&["--similar", "/path/foo.wav"]);
        assert_eq!(
            dispatch(&opts, true),
            Dispatch::TuiSimilar(PathBuf::from("/path/foo.wav"))
        );
    }

    #[test]
    fn similar_piped_routes_to_headless() {
        let opts = parse(&["--similar", "/path/foo.wav"]);
        assert_eq!(dispatch(&opts, false), Dispatch::Headless);
    }

    #[test]
    fn similar_with_no_tui_routes_to_headless() {
        let opts = parse(&["--similar", "/path/foo.wav", "--no-tui"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn similar_with_json_routes_to_headless() {
        let opts = parse(&["--similar", "/path/foo.wav", "--json"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn similar_with_verbose_routes_to_headless() {
        let opts = parse(&["--similar", "/path/foo.wav", "--verbose"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn no_args_at_tty_routes_to_tui_browse() {
        let opts = parse(&[]);
        assert_eq!(dispatch(&opts, true), Dispatch::TuiBrowse);
    }

    #[test]
    fn no_args_piped_routes_to_headless() {
        let opts = parse(&[]);
        assert_eq!(dispatch(&opts, false), Dispatch::Headless);
    }

    #[test]
    fn vendor_filter_at_tty_routes_to_headless() {
        let opts = parse(&["--vendor", "Mars"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn bpm_filter_at_tty_routes_to_headless() {
        let opts = parse(&["--bpm", "120-128"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn index_at_tty_routes_to_headless() {
        let opts = parse(&["--index"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn db_stats_at_tty_routes_to_headless() {
        let opts = parse(&["--db-stats"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn workflow_at_tty_routes_to_headless() {
        let opts = parse(&["--workflow", "foo.lua"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn eval_at_tty_routes_to_headless() {
        let opts = parse(&["--eval", "print(1)"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn count_at_tty_routes_to_headless() {
        let opts = parse(&["--count"]);
        assert_eq!(dispatch(&opts, true), Dispatch::Headless);
    }

    #[test]
    fn similar_with_filter_at_tty_still_routes_to_similar() {
        // Precedence: --similar wins over --vendor. If both are set we
        // still launch the TUI in similarity mode rather than falling
        // through to the filter-triggered headless path. This matches
        // user intent — the filter can be applied interactively via
        // the search bar after the similarity list is loaded.
        let opts = parse(&["--similar", "/path/foo.wav", "--vendor", "Mars"]);
        assert_eq!(
            dispatch(&opts, true),
            Dispatch::TuiSimilar(PathBuf::from("/path/foo.wav"))
        );
    }
}
