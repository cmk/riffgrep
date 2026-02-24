# riffgrep

A high-performance Rust CLI tool for searching WAV sample library metadata.

- **Ripgrep speed:** 1-3 seconds (RIFF INFO only) to 3-5 seconds (INFO + ID3v2) across 1.2M ~ 4TB files
- **Dual-mode search:** SQLite-indexed mode for instant results, databaseless filesystem mode for zero-setup use
- **Surgical I/O:** Read only the first ~1KB of each file; write metadata via fixed-offset overwrites without re-encoding audio
- **Modular architecture:** Telescope-inspired Picker pattern with swappable data sources

It's aspiring to be the BurntSushi version of a sample manager.

## Architecture

The architecture follows a Telescope-style Picker abstraction with four modular components connected by Rust traits:

```
┌───────────────────────────────────────────────────────┐
│  Finder       Async stream of search results          │
│               SQLite FTS5 Trigram  OR  ignore walker  │
├───────────────────────────────────────────────────────┤
│  Sorter       BM25 ranking (DB) or Nucleo (fuzzy)     │
├───────────────────────────────────────────────────────┤
│  Previewer    JIT BEXT header read                    │
├───────────────────────────────────────────────────────┤
│  Actions      Play, edit metadata, run Lua workflows  │
└───────────────────────────────────────────────────────┘
```

### Dual-Mode Data Sources

| Feature              | SQLite Source (`--db`)        | Filesystem Source (`--no-db`) |
|----------------------|------------------------------|-------------------------------|
| Search speed         | Instant (<10ms via FTS5)     | O(n) filesystem walk          |
| Setup                | Requires initial index scan  | Zero setup                    |
| Metadata             | Cached in DB                 | JIT from BEXT headers         |
| Fuzzy matching       | FTS5 Trigram (substring)     | Regex / filename match        |
| Vendor/UMID lookup   | Supported                    | Not supported                 |

### Unified Metadata IR

All metadata schemas (BEXT, ID3v2, iXML) parse into a common internal representation:

```rust
pub struct UnifiedMetadata<'a> {
    pub file_id: u64,               // High 64 bits of UUID v7 (0 = unpacked)
    pub umid: Cow<'a, str>,
    pub vendor: Cow<'a, str>,       // BEXT Originator
    pub library: Cow<'a, str>,      // BEXT OriginatorReference
    pub description: Cow<'a, str>,  // BEXT Description / TIT3
    pub category: Cow<'a, str>,     // TCON
    pub grouping: Cow<'a, str>,     // TIT1
    pub bpm: Option<u16>,           // TBPM
    pub key: Cow<'a, str>,          // TKEY
    pub rating: u8,                 // POPM (0-255)
    pub duration_ms: u32,
    pub sample_rate: u32,
}
```

## CLI Usage

```bash
# Fast search (Tier 1 - RIFF INFO only, ~2 seconds)
riffgrep --library "DX100" --category "SHOT" --shortid "F"

# Medium search (Tier 1 + Tier 2 - includes ID3v2, ~4 seconds)
riffgrep --vendor "Splice" --bpm "120-128" --key "C min"

# Regex support
riffgrep --library "DX\d+" --category "SHOT.*"

# Output formats
riffgrep --verbose --library "DX100"    # Metadata table
riffgrep --json --category "SHOT"       # JSON output
riffgrep --count --vendor "Mars"        # Count only

# Filesystem only mode (no index required)
riffgrep --no-db --category "DRUMS" ./Samples

# Lua workflows
riffgrep --eval 'if sample:bpm() > 140 then sample:set_category("Hardcore") end' ./Incoming
```

### Field-to-Tag Mapping

| CLI Flag        | RIFF INFO | ID3v2 | Tier |
|-----------------|-----------|-------|------|
| `--vendor`      | IART      | TPE1  | 1    |
| `--library`     | INAM      | TPE2  | 1    |
| `--category`    | IGNR      | TCON  | 1    |
| `--shortid`     | IKEY      | TIT2  | 1    |
| `--description` | ICMT      | COMM  | 1    |
| `--bpm`         | -         | TBPM  | 2    |
| `--key`         | -         | TKEY  | 2    |
| `--usage`       | -         | TXXX  | 2    |

## Build

### Release Profile

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```

Build with `RUSTFLAGS="-C target-cpu=native"` for SIMD acceleration.

## SQLite Schema

The indexed mode uses SQLite with FTS5 Trigram tokenization for substring matching across 1.2M rows:

```sql
CREATE TABLE samples (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    parent_folder TEXT NOT NULL,
    vendor TEXT,
    library TEXT,
    category TEXT,
    bpm INTEGER,
    rating INTEGER,
    peaks BLOB,            -- 180-byte u8 peak data (Zstd compressed)
    bext_description TEXT
);

CREATE VIRTUAL TABLE samples_fts USING fts5(
    name,
    parent_folder,
    bext_description,
    content='samples',
    content_rowid='id',
    tokenize='trigram'     -- Enables substring search: "808" finds "TR808"
);
```

Performance pragmas:
```sql
PRAGMA journal_mode = WAL;          -- Concurrent reads during indexing
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 2147483648;      -- 2GB memory-map
PRAGMA cache_size = -100000;        -- 100MB page cache
```

The indexing pipeline uses a producer-consumer pattern: `ignore::WalkParallel` workers parse BEXT headers in parallel and send results through a bounded `crossbeam::channel` to a single SQLite writer thread that commits in batches of 1,000.

## Waveform Rendering

Peak data is stored as 180 u8 values (0-255) in the BEXT reserved field at offset 422-602. This provides 4x the horizontal resolution of 45 f32 values in the same 180 bytes.

The TUI renders peaks using a 4-row bipolar Braille waveform (Unicode U+2800-U+28FF). Each Braille character is a 2x4 dot grid, giving 16 dots of vertical resolution across 4 terminal rows. The top 2 rows show positive amplitude; bottom 2 rows mirror as negative.

Themes control waveform colors: `--theme ableton` (orange), `--theme soundminer` (green), `--theme telescope` (cyan/blue).

## Tech Stack (key crates)

| Crate | Purpose |
|-------|---------|
| `bpaf` | CLI parsing (lightweight vs clap) |
| `ignore` | Parallel directory walking (from ripgrep) |
| `rusqlite` | SQLite with FTS5 Trigram indexing |
| `mlua` | Embedded Lua 5.4 for workflow scripting |
| `uuid` | UUID v7 file identity (timestamp-monotonic, 64-bit) |
| `ratatui` + `crossterm` | TUI framework |
| `symphonia` + `rodio` | WAV decoding + audio playback |
| `rayon` | Parallel file processing |
| `zstd` | Compression for peak BLOBs in SQLite |
| `mimalloc` | High-performance allocator (macOS) |
| `proptest` | Property-based testing for BEXT parser |
| `criterion` | Benchmarking |

## Workflow DSL

Power users can script metadata operations via embedded Lua (`mlua`):

```lua
-- Batch tagging via CLI
-- riffgrep --eval '<script>' ./path

if sample:description():find("Kick") then
    sample:set_category("DRUMS")
end
```

Workflows support dry-run mode by default, showing a colorized diff before committing surgical BEXT writes. Use `--commit` to apply changes.

## Workflow Engine

The workflow engine runs Lua scripts against each WAV file. Scripts access and mutate metadata via the `sample` object; changes are shown as a diff and only written with `--commit`.

```bash
# One-liner transformation (dry-run)
rfg --eval 'sample:set_category("DRUMS")' --no-db ./Incoming

# Script from disk
rfg --workflow scripts/etl_soundminer.lua --no-db ./Samples

# Apply changes
rfg --workflow scripts/etl_soundminer.lua --no-db --commit ./Samples

# Force re-process already-ported files (bypasses bext_umid guard)
rfg --workflow scripts/etl_soundminer.lua --no-db --force --commit ./Samples

# Cap files processed (safe test run before a large port)
rfg --workflow scripts/etl_soundminer.lua --no-db --limit 10 ./Samples
```

**Flags:**

| Flag          | Description |
|---------------|-------------|
| `--eval CODE` | Run a Lua one-liner |
| `--workflow F` | Run a Lua script from disk |
| `--commit`    | Write changes (default: dry-run, print diff only) |
| `--force`     | Re-process files whose migration receipt (`bext_umid`) is already set |
| `--limit N`   | Cap files processed (for test runs before a large port) |
| `--no-db`     | Bypass SQLite index, walk filesystem directly |

**Lua `sample` API:**

```lua
sample:path()           -- absolute path
sample:basename()       -- filename only (e.g. "kick.wav")
sample:dirname()        -- parent directory
sample:category()       -- getter
sample:set_category(s)  -- setter
sample:bext_umid()      -- 64-byte BEXT UMID field (migration receipt)
sample:file_id()        -- riffgrep UUID v7 file identity (0 = not yet packed)
sample:is_packed()      -- true when file_id != 0

-- riffgrep global
riffgrep.force          -- true when --force passed
riffgrep.commit         -- true when --commit passed

-- SQLite module (for ETL scripts)
local db = sqlite.open("/path/to/db.sqlite", "readonly")
local row = db:query_one("SELECT * FROM t WHERE path = ?", path)
db:close()
```

**Full ETL example (SoundMiner → riffgrep):**

```bash
rfg --workflow scripts/etl_soundminer.lua --no-db --commit ~/Music/Samples
```

**Status: Schema locked — big port of 1.2M WAV files in `~/Music/Samples` imminent.**

## Project Source Layout

```
src/
├── main.rs              # CLI entry point, bpaf parsing, source selection
├── engine/
│   ├── mod.rs           # Finder/Previewer/Action trait definitions
│   ├── bext.rs          # Surgical BEXT parser/writer, UnifiedMetadata
│   ├── sqlite.rs        # FTS5 Trigram search, batch indexing
│   ├── filesystem.rs    # Databaseless ignore walker implementation
│   └── workflow.rs      # Lua interpreter, DSL logic, diff view
├── ui/
│   ├── mod.rs           # tokio::select! event loop, debounced search
│   ├── widgets.rs       # 4-row bipolar Braille waveform
│   └── theme.rs         # Theme definitions (Ableton, SoundMiner, Telescope)
└── util.rs              # Logging, path normalization, hashing
```
