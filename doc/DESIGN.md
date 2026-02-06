# riffgrep - Fast Metadata Search for WAV Sample Libraries

**Status:** Design phase  
**Target:** 1.2M WAV files in `~/Music/Samples`  
**Goal:** Ripgrep-speed metadata search (subsecond to ~5 seconds max)  
**Date:** January 22, 2026

---

## Overview

A high-performance CLI search tool optimized for the tiered RIFF INFO schema. Designed to search 1.2M WAV files with ripgrep-level latencies by minimizing disk I/O and maximizing multi-core utilization.

**Performance targets:**
- Tier 1 (RIFF INFO only): 1-3 seconds for 1.2M files
- Tier 1 + Tier 2 (INFO + ID3v2): 3-5 seconds for 1.2M files
- Streaming results (show as found, not batch at end)

---

## Schema-Optimized Search Strategy

### Tiered Reading Approach

**Hot path (Tier 1 - RIFF INFO):**
```
Read RIFF header (12 bytes)
Scan chunks until LIST/INFO found
Parse INFO subchunks:
  - IART (Vendor)
  - INAM (Library)
  - IGNR (Category)
  - IKEY (ShortID)
  - ICMT (Description)
Stop reading if all search criteria matched
```

**Cost:** 10-50 µs per file (most files ~40-80 KB header)

**Cold path (Tier 2 - ID3v2):**
```
If BPM or Key filter specified:
  Continue reading to ID3v2 tag
  Parse TBPM, TKEY
  Match numeric ranges
```

**Cost:** Additional 50-100 µs per file

**Never read iXML** unless explicitly requested (rarely needed)

### Search Examples

```bash
# Fast (Tier 1 only - ~2 seconds for 1.2M files)
riffgrep --library "DX100" --category "SHOT" --shortid "F"

# Medium (Tier 1 + Tier 2 - ~4 seconds)
riffgrep --vendor "Splice" --bpm "120-128" --key "C min"

# Regex support
riffgrep --library "DX\d+" --category "SHOT.*"

# Combine with other tools
riffgrep --category "MULT" --usage "GRP" | fzf
riffgrep --vendor "Mars" | xargs -I {} cp {} /tmp/export/
```

---

## Architecture

### High-Level Design

The basic design is a nvim-telescope style 'Picker' plugin architecture with a ripgrep-like backend. Riffgrep supports both search (read) and surgical metadata editing (write) via fixed-offset BEXT overwrites. Metadata writes do not re-encode audio data.

#### Core Architectural Components

Telescope's basic design feature is its Picker abstraction. Pickers feature a modular, four-part architecture that separates data retrieval, filtering, and user interaction. A Picker is composed of these four distinct modules working together:

- Finder: Supplies the list of items. For us this could be a static list of files, a database (as with Soundminer), or a nested directory traversal. The item itself is a JSON object including the requested file metadata along with a path to the audio file itself and optionally a 'sidecar' file for visual representation. See below for a more in-depth analysis. 
- Sorter: Handles the filtering and ranking of items based on our input. Telescope uses a modified fzf that assigns a score to each entry, but in our case the query will include a sort-order so this component may be minimal or non-existent. Fuzzyfind is not a feature requirement.
- Previewer: Dynamically renders the contents of the currently selected item in a separate window pane. For Telescope this means showing file contents or git commit diffs. Use cases for riffgrep are: full color terminal-based rendering of file metadata in a tabular format, basic waveform rendering using Reapeaks files. The Previewer would include a static view of the waveform, as well as a tabular view of file metadata. Depending on the number of search results this tabular view will need to scroll.
- Actions: Defines what happens when you press a key (like <CR>). For example, the default action for a Telescope file picker is to open that file in the current buffer. For a riffgrep the default is playback, so a file Picker should include basic support for varispeed file playback and scrubbing.

#### User Interface Design

Telescope's visual interface is built using Neovim's floating windows and is typically divided into three areas: 

- Prompt Buffer: The top (or bottom) line where you type your search query.
- Results Buffer: The main list of matches that update in real-time.
- Preview Window: An optional side or top pane that shows context for the highlighted item. 

#### Layout & Themes

The interface appearance is governed by two main settings:

- Layout Strategy: Determines the positioning and size of the windows (e.g., horizontal, vertical, center, or cursor).
- Themes: Pre-packaged visual configurations that change the look instantly. Popular ones include get_dropdown, get_ivy (a bottom-docked panel), and get_cursor. 

We want riffgrep to remain a CLI application, so these last two components will be constrained by terminal support.

### Frontend / Search Query DSL

**Field-based filters:**
```bash
--vendor <pattern>      # IART (Artist)
--library <pattern>     # INAM (Title)
--category <pattern>    # IGNR (Genre)
--shortid <pattern>     # IKEY (Keywords)
--description <pattern> # ICMT (Comment)
--bpm <range>          # TBPM (e.g., "120-128" or "120")
--key <pattern>        # TKEY (e.g., "C min")
--usage <pattern>      # TXXX:USAGE
```

**Logical operators:**
```bash
# AND (default)
riffgrep --vendor "Mars" --category "SHOT"

# OR (explicit flag)
riffgrep --or --vendor "Mars" --vendor "Splice"

# NOT (negate filter)
riffgrep --vendor "Mars" --not-category "LOOP"
```

**Regex support:**
```bash
# Enable regex for all patterns
riffgrep --regex --library "DX\d+"

# Or per-field
riffgrep --library-regex "DX\d+" --category "SHOT"
```

### Backend / Finder Design

See this blog post by the author on how he optimized ripgrep: https://blog.burntsushi.net/ripgrep/ 

#### 1. Parallel Directory Traversal

**Use `ignore` or `jwalk` for parallel file discovery:**

**ignore** (ripgrep's library):
- Built-in parallel recursion
- Respects `.gitignore` if needed
- Battle-tested by ripgrep itself

**jwalk** (alternative):
- 4x faster than single-threaded walkers
- Simpler API than `ignore`
- Excellent for pure speed

**Recommendation:** Start with `ignore` (proven), switch to `jwalk` if benchmarks show benefit

#### 2. High-Performance Metadata Reading

**Use `lofty` for RIFF chunk parsing:**

**Why lofty:**
- Industry standard in 2026 Rust ecosystem
- Reads only metadata chunks (no PCM data)
- Optimized for exactly this use case
- Supports RIFF INFO, ID3v2, and iXML

**Alternative:** Custom RIFF parser if lofty proves slow
- Read raw chunks with `std::io::BufReader`
- Simple parsing (RIFF structure is straightforward)
- Full control over what's read

#### 3. Execution Strategy (The "Ripgrep Formula")

**Rayon for Data Parallelism:**
```rust
use rayon::prelude::*;

files_discovered
    .par_iter()
    .filter_map(|path| {
        let metadata = read_riff_info(path)?;
        if matches_criteria(&metadata, &query) {
            Some((path, metadata))
        } else {
            None
        }
    })
    .collect()
```

**Mimalloc allocator:**
```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```
- Outperforms macOS default allocator
- Critical for high-concurrency metadata parsing

**Buffer pooling:**
```rust
use crossbeam::queue::ArrayQueue;

static BUFFER_POOL: ArrayQueue<Vec<u8>> = ArrayQueue::new(1024);

fn get_buffer() -> Vec<u8> {
    BUFFER_POOL.pop().unwrap_or_else(|| Vec::with_capacity(8192))
}

fn return_buffer(mut buf: Vec<u8>) {
    buf.clear();
    let _ = BUFFER_POOL.push(buf);
}
```

### 4. Output Formats

**Default (paths only):**
```
/Users/cmk/Music/Samples/Samples From Mars/DX100 From Mars/WAV/25_AlarmCall_DX100_C#0.wav
/Users/cmk/Music/Samples/Samples From Mars/DX100 From Mars/WAV/26_AlarmCall_DX100_D0.wav
```

**Verbose (with metadata):**
```bash
riffgrep --verbose --library "DX100"
```
```
/Users/cmk/Music/Samples/.../25_AlarmCall_DX100_C#0.wav
  Vendor: Samples From Mars
  Library: DX100 From Mars
  Category: SHOT
  ShortID: F
  BPM: 100
  Key: C#0
```

**JSON (for processing):**
```bash
riffgrep --json --library "DX100"
```
```json
{
  "path": "/Users/cmk/Music/Samples/.../25_AlarmCall_DX100_C#0.wav",
  "vendor": "Samples From Mars",
  "library": "DX100 From Mars",
  "category": "SHOT",
  "shortid": "F",
  "bpm": 100,
  "key": "C#0"
}
```

**Count only:**
```bash
riffgrep --count --category "SHOT"
# 234,567 matches
```

### Backend / Finder Implementation

Handling large numbers of variable-size samples requires a "lazy loading" architecture.

#### 1. Sidecar Files

Many professional audio and video editing applications use deletable "sidecar" files to cache waveform peak data for faster UI rendering. These files prevent the software from having to re-scan massive audio files every time you zoom or scroll.

- Purpose: These files store pre-computed "peaks" (minimum and maximum amplitude values) for specific time intervals of an audio file.
- Performance: They are designed to prevent the software from having to process raw PCM data (like in a WAV file) every time the user zooms or scrolls the UI.
- Disposable nature: Most of these files are non-essential "cache" files; if deleted, the parent application will simply regenerate them the next time the audio file is loaded.
- Closed Formats: File structures are typically proprietary and not officially documented for third-party use. Examples include .asd (Ableton), .reapeak (REAPER), .peak (WavePlayer), .pk/.pkf/.pek (Adobe), .gpk (Steinberg WaveLab), .sfk (SoundForge), .ovm (Logic Pro / Platinum), and .wov (Cakewalk SONAR). Relying on these proprietary formats is a bad idea because they must be reverse-engineered and frequently change between software versions. 

By using the Picker abstraction we can keep support for Ableton or Reaper sidecars outside of the riffgrep core. However for our initial/reference Picker implementation we are going to store peaks information directly in the bext chunk of each file. This minimalist approach allows for a filesystem-only Picker.

Later on we may add support for sidecar files, either using our own open file format or use an open standard of some kind (e.g. BBC's audiowaveform). In either case the format should be binary to keep file sizes manageable.

**File Format**

Here is a streamlined way to design our own .peak format.

- Downsample: Don't store every single sample. Instead, divide the audio into windows (e.g., 256 or 512 samples each). For each window, store only two values: highest positive peak and lowest negative peak.
- Headerless binaries: For maximum performance and simplicity we can write a raw stream of 16-bit integers (Int16) or 32-bit floats:

| Byte Offset | Data Type | Description         |
|-------------|-----------|---------------------|
| 0           | UInt8     | Max Peak (Window 1) |
| 1           | UInt8     | Min Peak (Window 1) |
| 2           | UInt8     | Max Peak (Window 2) |
| 3           | UInt8     | Min Peak (Window 2) |

The Previewer component can map this file directly to memory (Memory Mapping) in a UI thread for instantaneous rendering without "parsing."

#### 2. The "Sidecar Store" Strategy

We don't want large numbers of .peak files cluttering sample folders, so we will need a centralized, keyed cache of some sort.

- Centralized Cache: Store all binary peak data in a single XDG-compliant folder (e.g., ~/.cache/riffgrep/peaks/).
- Keying: Use a BLAKE3 hash of the file path or the first few KB of the audio file as the filename (e.g., ~/.cache/.../a1b2c3d4.peak). This prevents re-generating data if you move files.

Initially we can use SoundMiner's database for this. However eventually we should use our own cache.

Since we are showing a table of results, we should use a Database for the metadata (technical specs, tags, length) while keeping the Waveform in our binary sidecar.

Category
	Storage Method	Why?
Searchable Metadata	SQLite or DuckDB	Fast full-text search (FTS5) for tags, names, and lengths.
Waveform Rendering	Custom Binary .peak	mmap is faster than reading BLOBs from a database when the user scrolls the picker.

#### Integrating with the Picker (UI Threading)

In a Telescope-style picker, the user might scroll through 50 results per second. To keep the UI fluid:

    Async Load: When a result is highlighted, spawn a thread to mmap the peak file and calculate the rendering slice.
    Debouncing: Only trigger the waveform render if the cursor stays on a result for >20ms.
    The "Missing" State: If a peak file doesn't exist yet, show a dim "Analyzing..." placeholder while our Symphonia-based worker generates it in the background.

4. Efficient CLI Drawing (The "Sparkline")

In a TUI, vertical space is precious. Since our samples are short, we can use the Braille approach mentioned earlier to get high-density waveforms even in a single row.

```
// A tiny snippet of what a Picker 'render' might look like
fn draw_waveform_row(peaks: &[f32], width: usize) -> String {
    // Each character is 2 columns wide in Braille
    // Logic: Map the peak slice to braille dots
    // Output looks something like: ⣠⣾⣿⣿⣷⣄
}
```

3. Rendering in the CLI

For a CLI, we can render these peaks using Braille characters (Unicode U+2800 block) or simple blocks to represent the vertical range.

- Logic: Read 2 u8 values from the bext peaks chuck for every column of the terminal.
- Performance: Use the bytemuck crate to instantly cast the binary file's &[u8] buffer into a &[f32] slice without copying data.

4. Why Rust is perfect for this:

- SIMD Acceleration: When calculating min/max over thousands of windows, the Rust compiler (with target-cpu=native) will often use SIMD instructions to check multiple samples at once.
- Zero-Copy: We can use Zero-copy parsing to treat our .peak file as a struct array, making it as fast as native memory access.


#### Implementation Example

1. File Generation

Here is an example implementation using Symphonia (the standard for Rust audio decoding) to generate our own .peak files. This script reads any audio file and writes interleaved f32 min/max pairs to a binary file.

```
use std::fs::File;
use std::io::{BufWriter, Write};
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn generate_peak_file(input_path: &str, output_path: &str, window_size: usize) {
    let src = File::open(input_path).expect("failed to open media");
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    
    let mut probed = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default())
        .expect("unsupported format");

    let track = probed.format.tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .expect("no audio track found");

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .expect("unsupported codec");

    let mut out_file = BufWriter::new(File::create(output_path).unwrap());
    let mut sample_buf = Vec::new();

    while let Ok(packet) = probed.format.next_packet() {
        let decoded = decoder.decode(&packet).unwrap();
        
        // Convert any audio format to f32 samples
        if let AudioBufferRef::F32(buf) = decoded {
            sample_buf.extend_from_slice(buf.chan(0)); // Just use first channel for peaks
        }

        // Process windows once buffer is full enough
        while sample_buf.len() >= window_size {
            let window = &sample_buf[0..window_size];
            let max = window.iter().fold(f32::MIN, |a, &b| a.max(b));
            let min = window.iter().fold(f32::MAX, |a, &b| a.min(b));
            
            out_file.write_all(&max.to_le_bytes()).unwrap();
            out_file.write_all(&min.to_le_bytes()).unwrap();
            
            sample_buf.drain(0..window_size);
        }
    }
}
```

2. The Real-Time Reader 

To achieve instantaneous loading, we should memory-map the file. This tells the OS to treat the file on disk as if it were a slice in RAM, avoiding expensive read calls. Using the memmap2 and bytemuck crates, we can "cast" the raw bytes directly into a slice of f32 without any copying or manual parsing.

Cargo.toml:

```
[dependencies]
memmap2 = "0.9"
bytemuck = { version = "1.14", features = ["derive"] }
```

Since terminal characters are taller than they are wide, we can represent the amplitude using a vertical bar. A simple approach is to map the min and max values to a fixed height:


```
use memmap2::Mmap;
use std::fs::File;

fn draw_column(min: f32, max: f32) {
    let height = 10; // Number of terminal rows for the waveform
    let mid = height / 2;
    
    // Convert -1.0..1.0 to 0..height
    let top = ((max + 1.0) * 0.5 * height as f32) as usize;
    let bottom = ((min + 1.0) * 0.5 * height as f32) as usize;

    // Use characters like '┃' or Braille based on the range
    // For a simple CLI, we might just print a colored '█' at the peak
}

fn render_waveform_from_map(peak_file_path: &str, terminal_width: usize) {
    let file = File::open(peak_file_path).expect("Failed to open peak file");
    
    // Memory map the file (Zero-copy)
    let mmap = unsafe { Mmap::map(&file).expect("Failed to map file") };
    
    // Cast bytes to a slice of f32
    // Since each window is 2 floats (max/min), we divide by 8 bytes
    let peaks: &[f32] = bytemuck::cast_slice(&mmap);
    let total_windows = peaks.len() / 2;

    // Map terminal columns to peak file offsets
    for col in 0..terminal_width {
        let index = (col * total_windows / terminal_width) * 2;
        let max = peaks[index];
        let min = peaks[index + 1];
        
        // Example: Convert float range (-1.0 to 1.0) to terminal blocks
        draw_column(min, max);
    }
}
```

3. Advanced: Use Braille for 2x Higher Resolution

If we want a detailed waveform in the terminal, we could use Braille Patterns (U+2800). Each character is a 2x4 grid of dots, allowing us to render two horizontal peaks per character column.

---

## Recommended Tech Stack

### Core Libraries

```toml
[dependencies]
# CLI parsing
bpaf = { version = "0.9", features = ["derive"] }

# Directory traversal
ignore = "0.4"                    # Ripgrep's parallel walker

# Metadata reading
lofty = "0.21"                    # RIFF/ID3v2 parser (fallback/general tags)

# Parallelism
rayon = "1.10"
crossbeam = "0.8"                 # Bounded channels for producer-consumer

# Regex
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

**bpaf** (not clap):
- Compile times: Much lighter than clap, avoids the massive dependency tree
- Validation: Encourages `Parser::guard` / `Parser::parse` for type-safe parsing (Haskell-inspired)
- Combinator & derivation-based: Mix and match keeps CLI code small while supporting extensions for other metadata schemas (ASWG, Vorbis, etc)

---

## Performance Optimization Checklist

### Tier 1: Basic Fast Search (Target: 2-3 seconds)

- [ ] Use `ignore` or `jwalk` for parallel discovery
- [ ] Use `lofty` for RIFF INFO reading
- [ ] Use `rayon` for parallel metadata parsing
- [ ] Read only RIFF INFO (skip ID3v2 unless needed)
- [ ] Early exit when match found (don't read entire file)

### Tier 2: Extreme Performance (Target: <2 seconds)

- [ ] Switch to `mimalloc` allocator
- [ ] Implement buffer pooling
- [ ] Custom RIFF parser (if lofty too slow)
- [ ] Memory-mapped file reading for hot paths
- [ ] SIMD for string matching (if applicable)

### Tier 3: Advanced Features

- [ ] Incremental search (show results as found)
- [ ] Cache directory tree (avoid re-scanning)
- [ ] Smart prefetch (predict next files to read)
- [ ] Parallel regex compilation

---

## Implementation Phases

### Phase 1: MVP (1-2 days)

**Features:**
- Basic CLI with `--vendor`, `--library`, `--category` filters
- Parallel directory traversal
- RIFF INFO reading only
- Path output

**Goal:** Validate performance (should hit 2-3 second target)

### Phase 2: Core Features (2-3 days)

**Add:**
- All Tier 1 fields (ShortID, Description)
- Tier 2 support (BPM, Key from ID3v2)
- Regex support
- JSON output
- Verbose mode

**Goal:** Feature parity with common search needs

### Phase 3: Polish (1-2 days)

**Add:**
- Count mode
- Boolean operators (AND/OR/NOT)
- Error handling and progress indicators
- Man page and help docs

### Phase 4: Advanced (Future)

**Add:**
- Audio preview (`--play` flag)
- Interactive mode with `fzf` integration
- Tag writing (complement to search)
- Config file support

---

## Benchmarking Strategy

### Test Sets

1. **Small** (1K files): Validate correctness
2. **Medium** (100K files): Profile and optimize
3. **Full** (1.2M files): Real-world performance

### Metrics to Track

- **Latency**: Time to first result
- **Throughput**: Total files/sec
- **CPU utilization**: % of cores active
- **Memory**: Peak RSS
- **Disk I/O**: Bytes read per file

### Target Comparison

| Tool | 1.2M Files | Notes |
|------|-----------|-------|
| **SoundMiner** | 30-60s | Database query + UI |
| **Finder/Spotlight** | 10-20s | Partial metadata indexing |
| **riffgrep (Tier 1)** | 1-3s | RIFF INFO only |
| **riffgrep (Tier 1+2)** | 3-5s | INFO + ID3v2 |
| **ripgrep (text search)** | 1-2s | Text only baseline |

---

## Example Implementation Sketch

```rust
use bpaf::*;
use ignore::WalkBuilder;
use lofty::{AudioFile, Probe};
use rayon::prelude::*;

#[derive(Clone, Debug, Bpaf)]
struct Args {
    #[bpaf(long)]
    vendor: Option<String>,

    #[bpaf(long)]
    library: Option<String>,

    #[bpaf(long)]
    category: Option<String>,

    #[bpaf(long)]
    shortid: Option<String>,
}

fn main() {
    let args = args().run();

    // Parallel directory walk
    let files: Vec<_> = WalkBuilder::new("~/Music/Samples")
        .threads(num_cpus::get())
        .build_parallel()
        .filter(|e| e.as_ref().map(|e| e.path().extension() == Some("wav")).unwrap_or(false))
        .collect();

    // Parallel metadata read + filter
    files.par_iter()
        .filter_map(|path| {
            let tagged = Probe::open(path).ok()?.read().ok()?;

            // Extract RIFF INFO tags
            let vendor = get_info_tag(&tagged, "IART")?;
            let library = get_info_tag(&tagged, "INAM")?;
            let category = get_info_tag(&tagged, "IGNR")?;
            let shortid = get_info_tag(&tagged, "IKEY")?;

            // Filter
            if let Some(v) = &args.vendor {
                if !vendor.contains(v) { return None; }
            }
            if let Some(l) = &args.library {
                if !library.contains(l) { return None; }
            }
            if let Some(c) = &args.category {
                if !category.contains(c) { return None; }
            }
            if let Some(s) = &args.shortid {
                if !shortid.contains(s) { return None; }
            }

            Some(path)
        })
        .for_each(|path| println!("{}", path.display()));
}

fn get_info_tag(tagged: &lofty::Tag, tag: &str) -> Option<String> {
    tagged.get_string(tag).map(|s| s.to_string())
}
```

---

## Future Enhancements

### Tag Writing
```bash
# Update metadata in place
riffgrep --library "DX100" --set-category "SYNTH" --write
```

### Interactive Mode
```bash
# Pipe to fzf for interactive selection
riffgrep --category "SHOT" | fzf --preview 'ffplay -nodisp -autoexit {}'
```

### Daemon Mode
```bash
# Watch directory and maintain index
riffgrep --daemon --index ~/Music/Samples
# Then instant searches using cached metadata
```

---

## Success Criteria

**MVP (Phase 1):**
- ✅ Search 1.2M files in under 5 seconds
- ✅ Correct filtering by vendor/library/category
- ✅ Utilizes all CPU cores

**Production (Phase 3):**
- ✅ Subsecond search for common queries
- ✅ Feature-complete CLI (all tiers supported)
- ✅ Robust error handling
- ✅ User documentation

**Long-term (Phase 4):**
- ✅ Complete SoundMiner replacement for search
- ✅ Integration with REAPER workflow
- ✅ Community adoption (if open-sourced)

---

**Status:** Design complete, ready for implementation after migration
**Dependencies:** RIFF INFO tags must be written to all files first
**Timeline:** Implement after Phase 3 of migration plan

---

## Dual-Mode Architecture

Riffgrep supports two data source modes, selectable via CLI flags:

**SQLite Indexed Mode** (default):
- Uses SQLite with FTS5 Trigram tokenization for instant substring search
- Requires an initial index scan (`--index` to rebuild)
- Stores peaks BLOB, metadata, and path in a local database
- Search returns in <10ms via `ORDER BY rank` with BM25

**Databaseless Filesystem Mode** (`--no-db`):
- Uses `ignore::WalkParallel` to walk the directory tree
- Reads BEXT headers JIT (Just-In-Time) as files are discovered
- Zero setup, no index file required
- Results stream to the UI as they are found

Both modes implement the same `SampleSource` trait, so the UI code is identical.

---

## SQLite Search Architecture

### Schema

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
    peaks BLOB,
    bext_description TEXT
);

CREATE VIRTUAL TABLE samples_fts USING fts5(
    name,
    parent_folder,
    bext_description,
    content='samples',
    content_rowid='id',
    tokenize='trigram'
);
```

### Performance Pragmas

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA mmap_size = 2147483648;
PRAGMA cache_size = -100000;
```

### Indexing Pipeline

Producer-consumer pattern:
1. `ignore::WalkParallel` discovers `.wav` files across all CPU cores
2. Workers parse the 602-byte BEXT block from each file's first 1KB
3. Results are sent through a bounded `crossbeam::channel(2048)` (backpressure)
4. A single SQLite writer thread commits in batches of 1,000

### FTS5 Sync Triggers

```sql
CREATE TRIGGER samples_ai AFTER INSERT ON samples BEGIN
  INSERT INTO samples_fts(rowid, name, parent_folder, bext_description)
  VALUES (new.id, new.name, new.parent_folder, new.bext_description);
END;

CREATE TRIGGER samples_ad AFTER DELETE ON samples BEGIN
  INSERT INTO samples_fts(samples_fts, rowid, name, parent_folder, bext_description)
  VALUES('delete', old.id, old.name, old.parent_folder, old.bext_description);
END;

CREATE TRIGGER samples_au AFTER UPDATE ON samples BEGIN
  INSERT INTO samples_fts(samples_fts, rowid, name, parent_folder, bext_description)
  VALUES('delete', old.id, old.name, old.parent_folder, old.bext_description);
  INSERT INTO samples_fts(rowid, name, parent_folder, bext_description)
  VALUES (new.id, new.name, new.parent_folder, new.bext_description);
END;
```

---

## Trait Interfaces

### Finder

```rust
#[async_trait]
pub trait Finder: Send + Sync {
    async fn find(&self, query: &str) -> Pin<Box<dyn Stream<Item = Vec<SearchResult>> + Send>>;
    fn metadata_capabilities(&self) -> MetadataLevel;
}
```

### Previewer

```rust
#[async_trait]
pub trait Previewer: Send + Sync {
    async fn get_peaks(&self, path: &Path) -> Result<Vec<u8>, AudioError>;
    async fn get_technical_info(&self, path: &Path) -> Result<TechnicalMetadata, AudioError>;
}
```

### MetadataSchema

```rust
pub trait MetadataSchema {
    fn name(&self) -> &'static str;
    fn parse(&self, data: &[u8]) -> Result<UnifiedMetadata, ParseError>;
    fn write_field(&self, path: &Path, field: FieldKey, value: &str) -> Result<(), WriteError>;
}
```

### Sorter

```rust
pub trait Sorter: Send + Sync {
    fn sort(&self, query: &str, results: &mut Vec<SearchResult>);
}
```

SQLite mode uses BM25 ranking from FTS5. Databaseless mode can optionally use the `nucleo` crate (FZF algorithm) for fuzzy relevance ranking.

### ActionHandler

```rust
#[async_trait]
pub trait ActionHandler: Send + Sync {
    fn available_actions(&self, result: &SearchResult) -> Vec<ActionType>;
    async fn dispatch(&self, action: ActionType, result: &SearchResult) -> Result<(), ActionError>;
}
```

Actions include: Play, Stop, CopyAbsolutePath, RevealInFinder, EditDescription, SetRating, AutoTagFromPath, FindSimilarVendor, ApplyLuaScript, RunRenameTemplate. The databaseless source omits actions that require global knowledge (e.g., FindSimilarVendor).

### WorkflowInterpreter

```rust
pub trait WorkflowInterpreter {
    fn parse_workflow(&self, script: &str) -> Result<Vec<WorkflowOp>, ParseError>;
    async fn execute_workflow(
        &self,
        ops: &[WorkflowOp],
        targets: Vec<SearchResult>,
    ) -> Result<WorkflowReport, WorkflowError>;
}
```

---

## Waveform Rendering

### Peak Storage Format

180 u8 values (0-255) stored in the BEXT reserved field at offset 422-602. This provides 4x the horizontal resolution compared to 45 f32 values in the same 180 bytes.

### 4-Row Bipolar Braille Widget

Uses Unicode Braille characters (U+2800-U+28FF), where each character is a 2x4 dot grid:
- 4 terminal rows = 16 dots of vertical resolution
- Top 2 rows: positive amplitude (cyan)
- Bottom 2 rows: negative amplitude (blue)
- 180 peaks map to 90 terminal columns (2 peaks per Braille character)

### Theme System

Themes are a struct mapping semantic UI elements to `ratatui` styles:

```rust
pub struct Theme {
    pub search_border: Style,
    pub selection_row: Style,
    pub waveform_upper: Color,
    pub waveform_lower: Color,
    pub rating_star: Color,
    pub metadata_key: Style,
}
```

Built-in themes: Ableton (orange), SoundMiner (green), Telescope (cyan/blue).

---

## Workflow DSL

Riffgrep supports scripted metadata operations via embedded Lua (`mlua`).

### CLI Interface

```bash
# One-liner
riffgrep --eval 'if sample:bpm() > 140 then sample:set_category("Hardcore") end' ./path

# Script file
riffgrep --workflow ./scripts/organize.lua ./Incoming
```

### Operations

```rust
pub enum WorkflowOp {
    SetField { field: FieldKey, value: String },
    RegexReplace { field: FieldKey, pattern: String, replacement: String },
    RenameFile { template: String },
    MoveToFolder { path: String },
    ExternalScript { script_path: PathBuf },
}
```

Workflows run in dry-run mode by default, displaying a colorized diff before committing. Use `--commit` to apply.

---

## Surgical BEXT Read/Write

### Read Strategy

Scan only the first 4KB of each file to locate the `bext` chunk ID. Skip 8 bytes (ID + size) to reach the 602-byte data block. Zero dependencies on audio crates for the fast path.

```rust
pub struct BextReader;

impl BextReader {
    pub fn read_bext(path: &Path) -> std::io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let offset = Self::find_bext_offset(&mut file)?;
        let mut buffer = vec![0u8; 602];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    pub fn write_field(path: &Path, field_offset: u64, data: &[u8]) -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).open(path)?;
        let bext_base = Self::find_bext_offset(&mut file)?;
        file.seek(SeekFrom::Start(bext_base + field_offset))?;
        file.write_all(data)?;
        Ok(())
    }

    fn find_bext_offset(file: &mut File) -> std::io::Result<u64> {
        let mut head = [0u8; 4096];
        file.read_exact(&mut head)?;
        let pos = head.windows(4)
            .position(|w| w == b"bext")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "No bext chunk"))?;
        Ok(pos as u64 + 8)
    }
}
```

### Write Strategy

Use `OpenOptions::new().write(true)` (without `truncate`) to overwrite specific field bytes. Always null-pad to the field's full width. File size never changes.

---

## TUI Architecture

### Event Loop

Uses `tokio::select!` to race between user input, tick events, and the async search stream:

1. Input handler spawned as a tokio task, sends `AppEvent::Input` via channel
2. Search stream produces `Vec<SearchResult>` batches as they are found
3. Preview timer (150ms debounce) triggers JIT BEXT header read when cursor stops

### JIT Preview Triggering

The previewer only reads the disk when the user stops scrolling (150ms debounce). This prevents I/O thrashing on large libraries.

### Source Selection

```rust
let source: Box<dyn SampleSource> = if opts.no_db {
    Box::new(FilesystemSource::new(opts.root))
} else {
    Box::new(SqliteSource::new(db_path).await?)
};
```

---

## Project Source Layout

```
src/
├── main.rs              # CLI entry point, bpaf parsing, source selection
├── engine/
│   ├── mod.rs           # Trait definitions (Finder, Previewer, ActionHandler)
│   ├── bext.rs          # Surgical BEXT parser/writer, UnifiedMetadata
│   ├── sqlite.rs        # FTS5 Trigram search, batch indexing, triggers
│   ├── filesystem.rs    # Databaseless ignore walker implementation
│   └── workflow.rs      # Lua interpreter, WorkflowOp, diff view
├── ui/
│   ├── mod.rs           # tokio::select! event loop, debounced search
│   ├── widgets.rs       # 4-row bipolar Braille waveform widget
│   └── theme.rs         # Theme struct and built-in definitions
└── util.rs              # Logging, path normalization, BLAKE3 hashing
```

---

## Testing Strategy

### Property-Based Tests (proptest)

- **Round-trip:** Write → Read yields identical `UnifiedMetadata`
- **Locality:** Writing one field does not modify adjacent fields
- **Idempotency:** Applying the same write twice produces the same result
- **Panic freedom:** Parser returns `None`/`Err` on any arbitrary 602-byte input

### Integration Tests

- `assert_cmd` + `assert_fs` for CLI behavior with mock directory structures
- `insta` for snapshot testing of workflow DSL output

### Benchmarking

- `criterion` for BEXT parser throughput and waveform widget rendering
- `cargo-flamegraph` for hotpath analysis (compile with `-C force-frame-pointers=yes`)

### Profiling Targets

| Metric | Target |
|--------|--------|
| Sequential read throughput | 2-5 GB/s (NVMe) |
| Random 4KB read throughput | 50-100 MB/s |
| Indexing speed | 10,000+ files/sec |
| FTS5 query latency | <2ms |
