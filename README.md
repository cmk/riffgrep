# riffgrep

A high-performance Rust CLI tool for searching WAV sample library metadata. Designed to search 1.2M WAV files (~4TB) at ripgrep-level speed by reading only file headers and maximizing multi-core utilization.

**Status:** Design complete, ready for implementation

## Design Goals

- **Ripgrep speed:** 1-3 seconds (RIFF INFO only) to 3-5 seconds (INFO + ID3v2) across 1.2M files
- **Dual-mode search:** SQLite-indexed mode for instant results, databaseless filesystem mode for zero-setup use
- **SoundMiner migration:** Maintain compatibility with SoundMiner's database and metadata during transition
- **Surgical I/O:** Read only the first ~1KB of each file; write metadata via fixed-offset overwrites without re-encoding audio
- **Modular architecture:** Telescope-inspired Picker pattern with swappable data sources

## Architecture

The architecture follows a Telescope-style Picker abstraction with four modular components connected by Rust traits:

```
┌─────────────────────────────────────────────────────┐
│  Finder       Async stream of search results        │
│               SQLite FTS5 Trigram  OR  ignore walker │
├─────────────────────────────────────────────────────┤
│  Sorter       BM25 ranking (DB) or Nucleo (fuzzy)   │
├─────────────────────────────────────────────────────┤
│  Previewer    JIT BEXT header read → Braille waveform│
├─────────────────────────────────────────────────────┤
│  Actions      Play, edit metadata, run Lua workflows │
└─────────────────────────────────────────────────────┘
```

### Dual-Mode Data Sources

| Feature              | SQLite Source (`--db`)        | Filesystem Source (`--no-db`) |
|----------------------|------------------------------|-------------------------------|
| Search speed         | Instant (<10ms via FTS5)     | O(n) filesystem walk          |
| Setup                | Requires initial index scan  | Zero setup                    |
| Metadata             | Cached in DB                 | JIT from BEXT headers         |
| Fuzzy matching       | FTS5 Trigram (substring)     | Regex / filename match        |
| Vendor/UMID lookup   | Supported                    | Not supported                 |

### Core Traits

```rust
#[async_trait]
pub trait Finder: Send + Sync {
    async fn find(&self, query: &str) -> Pin<Box<dyn Stream<Item = Vec<SearchResult>> + Send>>;
    fn metadata_capabilities(&self) -> MetadataLevel;
}

#[async_trait]
pub trait Previewer: Send + Sync {
    async fn get_peaks(&self, path: &Path) -> Result<Vec<u8>, AudioError>;
    async fn get_technical_info(&self, path: &Path) -> Result<TechnicalMetadata, AudioError>;
}

pub trait MetadataSchema {
    fn name(&self) -> &'static str;
    fn parse(&self, data: &[u8]) -> Result<UnifiedMetadata, ParseError>;
    fn write_field(&self, path: &Path, field: FieldKey, value: &str) -> Result<(), WriteError>;
}

pub trait Sorter: Send + Sync {
    fn sort(&self, query: &str, results: &mut Vec<SearchResult>);
}

#[async_trait]
pub trait ActionHandler: Send + Sync {
    fn available_actions(&self, result: &SearchResult) -> Vec<ActionType>;
    async fn dispatch(&self, action: ActionType, result: &SearchResult) -> Result<(), ActionError>;
}
```

### Unified Metadata IR

All metadata schemas (BEXT, ID3v2, iXML) parse into a common internal representation:

```rust
pub struct UnifiedMetadata<'a> {
    pub recid: u64,
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

# Databaseless mode (no index required)
riffgrep --no-db --category "DRUMS" ./Samples

# Lua workflow one-liner
riffgrep --eval 'if sample:bpm() > 140 then sample:set_category("Hardcore") end' ./Incoming

# Pipe to fzf for interactive selection
riffgrep --category "MULT" --usage "GRP" | fzf
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

## Tech Stack

### Core Dependencies

```toml
[dependencies]
# CLI parsing
bpaf = { version = "0.9", features = ["derive"] }

# Directory traversal
ignore = "0.4"                    # Ripgrep's parallel walker

# Metadata reading
lofty = "0.21"                    # RIFF/ID3v2 parser (fallback)

# Parallelism
rayon = "1.10"
crossbeam = "0.8"                 # Bounded channels for producer-consumer

# Search
regex = "1.10"

# Database
rusqlite = { version = "0.31", features = ["bundled", "modern_sqlite"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# TUI
ratatui = { version = "0.26", features = ["crossterm"] }
crossterm = { version = "0.27", features = ["event-stream"] }

# Async
tokio = { version = "1.36", features = ["macros", "rt-multi-thread", "time", "sync", "fs"] }
tokio-stream = "0.1"
async-trait = "0.1"

# Audio playback
symphonia = { version = "0.5", default-features = false, features = ["wav", "pcm"] }
rodio = { version = "0.17", default-features = false, features = ["wav"] }

# Scripting (workflows)
mlua = { version = "0.9", features = ["luajit", "vendored", "async"] }

# Performance
mimalloc = { version = "0.1", default-features = false }
bytemuck = { version = "1.16", features = ["derive"] }
memmap2 = "0.9"
zstd = "0.13"

# Hashing
blake3 = "1.5"

# CLI diff output
similar = { version = "2.4", features = ["inline"] }
console = "0.15"
```

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

## SQLite Architecture

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

## Workflow DSL

Power users can script metadata operations via embedded Lua (`mlua`):

```lua
-- Batch tagging via CLI
-- riffgrep --eval '<script>' ./path

if sample:description():find("Kick") then
    sample:set_category("DRUMS")
end
```

Workflows support dry-run mode by default, showing a colorized diff (via `similar` crate) before committing surgical BEXT writes. Use `--commit` to apply changes.

## POPM Rating Conversion

SoundMiner and Kid3 use the ID3v2 POPM standard (0-255 byte → star rating):

| Rating Byte | Stars |
|-------------|-------|
| 0           | Unrated |
| 1-49        | 1 star |
| 50-113      | 2 stars |
| 114-185     | 3 stars |
| 186-241     | 4 stars |
| 242-255     | 5 stars |

## Path Resolution (Migration)

For SoundMiner migration where stored paths may not match local mounts, use component-based resolution: reconstruct paths from BEXT Originator (Vendor) + OriginatorReference (Library) + filename, then verify against the SQLite index.

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

## Implementation Roadmap

### Phase 1: Surgical Core (Data Layer)
- Implement `parse_bext_fast` using `std::fs::File` + `Seek`
- Implement null-padded BEXT surgical writer
- Write proptest suites for round-trip, locality, idempotency, panic freedom
- Basic CLI with `bpaf`: `riffgrep dump <file>` to dump metadata

### Phase 2: Ripgrep Engine (Parallelism)
- `ignore::WalkParallel` directory traversal
- `crossbeam::channel` producer-consumer pipeline with `rayon` workers
- Benchmark files/second throughput (target: 10,000+ on NVMe)
- Implement databaseless `SampleSource` trait

### Phase 3: Telescope UI (Ratatui + Async)
- 4-row bipolar Braille waveform widget with hardcoded test data
- `tokio::select!` event loop skeleton with search debouncing
- Connect `FilesystemSource` to TUI for functional databaseless browser
- JIT preview triggering with 150ms debounce

### Phase 4: SQLite Power-Up (Persistence + Scripting)
- Connect producer-consumer pipeline to `rusqlite` with batch transactions
- FTS5 Trigram index with sync triggers
- Implement SQLite `SampleSource` (instant search)
- Embed `mlua`, expose `UnifiedMetadata`, implement diff view
- Theme system, `--eval` flag, error handling with `anyhow`

## Testing Strategy

- **Property-based testing:** `proptest` for BEXT parser fuzzing (round-trip, locality, panic freedom)
- **Snapshot testing:** `insta` for workflow DSL output verification
- **CLI integration:** `assert_cmd` + `assert_fs` with mock directory structures
- **Benchmarking:** `criterion` for BEXT parser and waveform widget performance
- **Profiling:** `cargo-flamegraph` with `RUSTFLAGS="-C force-frame-pointers=yes"`

## Documentation

- [`doc/DESIGN.md`](doc/DESIGN.md) - Complete architectural specification
- [`doc/PICKER_SCHEMA.md`](doc/PICKER_SCHEMA.md) - BEXT metadata schema for file headers
- [`doc/MARKER_SERIALIZATION.md`](doc/MARKER_SERIALIZATION.md) - Marker/cue point binary format
- [`doc/SOUNDMINER_SCHEMA_ANALYSIS.md`](doc/SOUNDMINER_SCHEMA_ANALYSIS.md) - SoundMiner SQLite database reverse engineering

## Reference Implementations

- `reference_libraries/ripgrep/` - ripgrep v15.1.0 source (parallel traversal patterns, `ignore` crate usage)
- `reference_libraries/wavdeets/` - WAV metadata parser (Rust workspace, edition 2024)
- `bin/wavdeets` - Pre-compiled metadata extraction binary

## SoundMiner Database

Location: `/Users/cmk/Library/SMDataBeta/Databases/SUB.sqlite`

Key fields: `recid` (integer PK), `_UMID` (32-char hex), `FilePath` + `_FilePathHash` (SHA1). The BEXT UMID field embeds SoundMiner's `_UMID` for cross-referencing during migration. Open in read-only mode with `PRAGMA query_only = ON` to avoid locking SoundMiner.
