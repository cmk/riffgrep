//! riffgrep — high-performance WAV sample library metadata search.

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod engine;
mod ui;
mod util;

fn main() {
    let opts = engine::cli::opts_with_help().run();

    if should_launch_tui(&opts) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        if let Err(e) = rt.block_on(ui::run_tui(opts)) {
            eprintln!("riffgrep: {e}");
            std::process::exit(2);
        }
    } else {
        if let Err(e) = engine::run(opts) {
            eprintln!("riffgrep: {e}");
            std::process::exit(2);
        }
    }
}

/// Determine whether to launch the interactive TUI.
///
/// TUI launches when:
/// - stdout is a TTY (not piped)
/// - `--no-tui` is not set
/// - No search filters are set (interactive browsing mode)
/// - Not running a subcommand (--index, --db-stats)
fn should_launch_tui(opts: &engine::cli::Opts) -> bool {
    use std::io::IsTerminal;

    !opts.no_tui
        && std::io::stdout().is_terminal()
        && !opts.has_search_filters()
        && !opts.index
        && !opts.db_stats
        && !opts.verbose
        && !opts.json
        && !opts.count
        && !opts.is_workflow_mode()
}
