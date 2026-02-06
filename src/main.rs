//! riffgrep — high-performance WAV sample library metadata search.

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod engine;
mod util;

fn main() {
    let opts = engine::cli::opts().run();
    if let Err(e) = engine::run(opts) {
        eprintln!("riffgrep: {e}");
        std::process::exit(2);
    }
}
