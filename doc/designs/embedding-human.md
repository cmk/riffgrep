# Timbral Similarity Embeddings

## Overview

Full-precision (512 × f32) CLAP embeddings stored in the SQLite index.
Enables "find similar sounds" queries across the library. Product
quantization is used as a search-time acceleration structure, not a
storage format — embeddings are stored losslessly in the DB, and the
PQ index is built in memory from the stored vectors.

BEXT Description `[128:256]` is left reserved (see Alternatives).

---

## Model

### Selected: LAION-CLAP (`music_audioset_epoch_15_esc_90.14.pt`)

Contrastive Language-Audio Pretraining model trained on music and AudioSet.
Produces 512-dimensional float32 embeddings from variable-length audio.

**Why CLAP over alternatives:**

| Model | Dim | Size | Timbral quality | Notes |
|-------|-----|------|-----------------|-------|
| **LAION-CLAP** | 512 | ~600MB | Excellent | Music-trained, captures attack/decay/spectrum |
| VGGish | 128 | ~300MB | Good | Older, designed for audio events not music timbre |
| OpenL3 | 512 | ~20MB | Good | Lighter, environmental sound focus |
| PANNs (CNN14) | 2048 | ~300MB | Excellent | Overkill dimensionality, harder to quantize to 128B |
| Encodec latents | 128 | ~200MB | Good | Codec-oriented, not designed for similarity |

**Trade-offs:**
- CLAP is contrastively trained: audio and text descriptions share a latent
  space. This means the embedding dimensions are organized by *human
  perceptual categories* — "bright," "punchy," "warm," "metallic" — rather
  than incidental acoustic features (noise floor, loudness, sample rate
  artifacts). This is exactly what timbral similarity needs.
- The text side of CLAP is not used for audio→audio similarity search, but
  it shapes the quality of the audio embeddings. It also enables a future
  feature: text-based sound search (`rfg --sounds-like "bright metallic
  percussion"`) using the same 128-byte PQ codes, since the text query and
  the audio codes share the same latent space.
- 512 dims → 128 bytes requires 4:1 compression, which PQ handles well.
- VGGish's native 128 dims would avoid PQ entirely (just quantize to u8)
  but the timbral quality is noticeably worse for music/sample content,
  and the embedding space lacks the semantic structure that CLAP's text
  alignment provides.

### Alternative: Train a custom encoder

A small convolutional autoencoder trained on the user's own library could
produce a 128-dim latent directly. Advantages: no PQ needed, codebook-free,
embeddings are self-contained in the BEXT bytes. Disadvantage: requires
a training pipeline, less generalizable, and the user's library may not
be large enough for good generalization.

**Recommendation:** Start with LAION-CLAP + PQ. If the PQ codebook
dependency becomes a pain point, revisit with a custom 128-dim encoder.

---

## Storage

### Selected: Full-precision vectors in SQLite

Store 512 × f32 = 2048 bytes per file in the SQLite `samples.embedding`
column as a BLOB. No quantization at storage time.

```
CLAP output: [f32; 512]
    → serialize as 2048-byte little-endian BLOB
    → INSERT INTO samples SET embedding = ? WHERE path = ?
```

**Size:** 2048 bytes × 1.2M files = 2.3 GB. SQLite handles this fine —
it's a static column, read sequentially during search, and benefits from
mmap (`PRAGMA mmap_size`).

**Advantages over in-file storage:**
- **Lossless** — no quantization error, no recall penalty
- **Format-agnostic** — same code path for WAV, AIFF, MP3, FLAC
- **Retrain PQ without re-embedding** — the expensive CLAP inference is
  done once; PQ codebook training is cheap and repeatable
- **No codebook dependency for storage** — the embedding is self-contained
  in the DB row
- **Frees BEXT `[128:256]`** for future use

**Disadvantage:** embeddings don't travel with the file. But they're
meaningless without a distance metric anyway (unlike peaks, which are
directly renderable), so portability isn't a real concern.

## Quantization (search-time only)

### Selected: Product Quantization (PQ) as in-memory index

PQ is used as a **search acceleration structure**, not a storage format.
At startup (or on first `--similar` query), build the PQ index from the
stored full-precision vectors:

1. Load codebook from DB metadata table (512 KB, trained offline)
2. For each row, encode the 512-dim vector → 128-byte PQ code (in memory)
3. Search uses asymmetric distance computation against the in-memory codes

The full-precision vectors remain in the DB. PQ codes are ephemeral.

```
Storage:   samples.embedding  →  [f32; 512]  →  2048 bytes/file (lossless)
Search:    PQ encode on load  →  [u8; 128]   →  128 bytes/file (in memory)
           ADC scan           →  ~20ms for 1.2M files
```

**Codebook:** 128 subquantizers × 256 centroids × 4 floats = 512 KB.
Stored in SQLite `metadata` table. Trained once on a representative
sample of the corpus (~10K files).

For libraries under ~100K files, brute-force L2 on the full vectors is
fast enough (~50ms) and needs no PQ at all. PQ is an optimization for
the 1.2M SUB-scale library.

### Quantization alternatives considered

| Method | Bytes/file | Recall@10 | Codebook | Notes |
|--------|-----------|-----------|----------|-------|
| **Full f32 (DB) + PQ search** | 2048 (disk) / 128 (RAM) | ~95% search, 100% stored | 512KB | **Selected** |
| PQ stored in BEXT `[128:256]` | 128 (disk+RAM) | ~90-95% | 512KB | Lossy, format-dependent (see Alternatives) |
| Scalar u8 (128-dim PCA) | 128 | ~70-80% | No | Simpler but worse recall |
| Binary (sign bits) | 64 | ~60-70% | No | Hamming distance, very fast but low recall |

---

## Pre-processing

### Input: one-shot audio

Files with `category` matching `LOOP/*` are either:
1. **Skipped** (no embedding computed, `[128:256]` stays zeroed), or
2. **Split at the first onset**, embedding computed from the first
   transient through the first decay. This captures the attack/decay
   character without looping content polluting the embedding.

Onset detection: simple energy threshold on the waveform envelope.
No need for a neural onset detector — these are clean sample library files.

### Audio preparation

1. **Resample to model input rate** (CLAP expects 48kHz)
2. **Mono mixdown** (average channels)
3. **Trim silence** (leading/trailing samples below -60dBFS)
4. **Pad or truncate to model window** (CLAP: variable length, but
   inference is faster on fixed-length inputs — use 1-3 seconds)
5. **Normalize peak to -1.0 dBFS** (removes volume as a similarity axis)

All pre-processing happens in the Python training/embedding script.

---

## Post-processing

### Codebook training

One-time step. Run on a representative sample of the library:

1. Compute CLAP embeddings for ~10K one-shot files
2. Train PQ codebook: `faiss.ProductQuantizer(512, 128, 8)`
3. Serialize codebook to binary (512 KB)
4. Store in SQLite `metadata` table as `(key='pq_codebook', value=BLOB)`

Retraining is needed only if the library changes dramatically in character
(e.g., adding a large collection of a totally new instrument family).
Since full-precision vectors are stored in the DB, retraining the codebook
does not require re-running CLAP inference.

### Encoding

For each file:

1. Compute CLAP embedding (512 × f32)
2. Serialize to 2048-byte little-endian BLOB
3. Write to SQLite `samples.embedding` column

PQ encoding happens at search time (in memory), not at storage time.

### Distance computation

Two modes depending on library size:

**Small libraries (<100K files): brute-force L2**

Load all 512-dim vectors into a contiguous `[f32; 512 * N]` buffer.
Compute L2 distance from query to every vector. With SIMD this is ~50ms
for 100K files — fast enough without PQ.

**Large libraries (>100K files): Asymmetric Distance Computation (ADC)**

At query time, the query vector is *not* quantized. Instead, precompute a
distance table: for each of the 128 subquantizers, compute the distance
from the query sub-vector to all 256 centroids. This gives a
128 × 256 lookup table (128 KB).

At startup, PQ-encode all DB vectors into an in-memory `[u8; 128 * N]`
buffer (one-time cost: ~2s for 1.2M files). Then for each code:

```
dist(query, code) = sum over m in 0..128:
    distance_table[m][code[m]]
```

This is a single pass over 128 bytes per candidate — scanning 1.2M codes
takes ~150ms with scalar code, <20ms with SIMD.

---

## Training Pipeline (Python)

```
scripts/embed_train.py
    --sample-dir ~/Samples          # directory to sample from
    --db-path index.db              # riffgrep SQLite DB
    --codebook-out codebook.bin     # PQ codebook output
    --n-train 10000                 # number of files to train on
    --exclude-loops                 # skip LOOP/* categories

Steps:
1. Sample N one-shot files from the DB (by category != LOOP/*)
2. Load audio, preprocess (resample, mono, trim, normalize)
3. Run CLAP inference → [N, 512] float32 matrix
4. Train FAISS ProductQuantizer on the matrix
5. Serialize codebook to file and insert into DB metadata table
```

```
scripts/embed_encode.py
    --db-path index.db              # riffgrep SQLite DB
    --batch-size 256                # files per inference batch

Steps:
1. Walk all files in DB where embedding IS NULL
2. Batch load + preprocess audio
3. Batch CLAP inference → [batch, 512]
4. Serialize each 512-dim vector to 2048-byte LE BLOB
5. UPDATE samples SET embedding = ? WHERE path = ?
```

**Dependencies (Python, training only):**
- `laion_clap` or `transformers` (CLAP inference)
- `faiss-cpu` (PQ training + encoding)
- `librosa` or `soundfile` (audio loading)
- `numpy`

**No Python at runtime.** The trained codebook and PQ codes are consumed
by riffgrep's Rust search engine.

---

## Runtime (Rust)

### Schema

```sql
-- Add to existing samples table (2048-byte f32 BLOB, or NULL if not yet embedded):
ALTER TABLE samples ADD COLUMN embedding BLOB;

-- Codebook and metadata:
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value BLOB
);
-- INSERT INTO metadata VALUES ('pq_codebook', ?);    -- 512 KB, trained offline
-- INSERT INTO metadata VALUES ('embedding_model', 'laion-clap-music-audioset');
```

### Codebook loading

At startup (or on first similarity query), load the codebook from DB:
128 subquantizers × 256 centroids × 4 dims × f32 = 512 KB. Parse into
`[[f32; 4]; 256]; 128]` — a 2D array of centroid vectors.

### Query: `rfg --similar <path>`

1. **Look up query embedding:** load 512-dim vector from DB by path.
   If not found, error: "file not embedded — run embed_encode.py first."
   (v2 could add on-the-fly CLAP inference via ONNX runtime.)
2. **Small library path (<100K):** brute-force L2 against all stored
   vectors. Load into contiguous buffer, SIMD-accelerated scan.
3. **Large library path (>100K):** build PQ codes in memory (once per
   session), then ADC scan:
   a. Build ADC distance table: for each subquantizer m, compute
      `dist_table[m][j] = ||query_sub[m] - centroid[m][j]||²`.
   b. Scan all codes: `dist = sum(dist_table[m][code[m]] for m in 0..128)`.
4. **Return top-N** by ascending distance.

### Performance estimate

**Small library (100K files, brute-force L2):**
- Load: 100K × 2048 bytes = 195 MB (mmap or sequential read)
- Scan: 100K × 512 float multiplies = ~50ms with SIMD
- Total: <100ms

**Large library (1.2M files, PQ ADC):**
- PQ encode on first query: 1.2M × 128 centroid lookups = ~2s (cached)
- ADC table construction: ~0.1ms
- Scan: 1.2M × 128 additions = ~150ms scalar, ~20ms SIMD
- Total: ~2s first query, <200ms subsequent

### Pure Rust PQ implementation (search-time only)

PQ is simple enough to implement without FAISS. Only needed for
libraries >100K files; smaller libraries use brute-force L2:

```rust
struct ProductQuantizer {
    /// m subquantizers, each with 256 centroids of dim dsub.
    /// Shape: [m][256][dsub] where m=128, dsub=4.
    centroids: Vec<Vec<[f32; 4]>>,
}

impl ProductQuantizer {
    /// Load codebook from serialized blob (512 KB).
    fn from_bytes(data: &[u8]) -> Self { ... }

    /// Encode a 512-dim vector to 128 bytes.
    fn encode(&self, vector: &[f32; 512]) -> [u8; 128] {
        let mut code = [0u8; 128];
        for m in 0..128 {
            let sub = &vector[m*4..(m+1)*4];
            code[m] = nearest_centroid(&self.centroids[m], sub);
        }
        code
    }

    /// Build ADC distance table for a query vector.
    fn adc_table(&self, query: &[f32; 512]) -> [[f32; 256]; 128] {
        let mut table = [[0.0f32; 256]; 128];
        for m in 0..128 {
            let sub = &query[m*4..(m+1)*4];
            for j in 0..256 {
                table[m][j] = l2_dist_4d(sub, &self.centroids[m][j]);
            }
        }
        table
    }

    /// Compute approximate distance from query (via ADC table) to a code.
    fn distance(&self, table: &[[f32; 256]; 128], code: &[u8; 128]) -> f32 {
        let mut dist = 0.0f32;
        for m in 0..128 {
            dist += table[m][code[m] as usize];
        }
        dist
    }
}
```

This is ~100 lines of Rust. No FAISS dependency at runtime.

---

## BEXT Schema

BEXT Description `[128:256]` remains **reserved**. Embeddings are DB-only.

This is intentional — unlike peaks (which are read on every file scan and
are self-contained), embeddings are only used at query time, require a
distance metric to interpret, and would be format-dependent (AIFF/MP3/FLAC
have no BEXT chunk). DB-only storage is format-agnostic, lossless, and
allows PQ retraining without re-running CLAP inference.

---

## TUI Integration

### Sort by similarity

The user selects a file in the TUI and triggers "sort by similarity"
(a new `Action`). The currently selected file becomes the **subject**.

1. Load the subject's 512-dim embedding from DB
2. Compute distances to all other files (brute-force or PQ ADC)
3. Re-sort results by ascending distance
4. The subject is pinned to position 0 (similarity = 100) regardless
   of what the embedding distance says — identity is always max similarity
5. Display a `sim` column: integer 0-100, where 100 = identical, 0 = maximally dissimilar

### Similarity score

```
dist = ||query - candidate||₂
sim  = 1.0 - (dist / max_dist)
```

Where `max_dist` is the L2 distance of the furthest result in the current
result set. This gives a relative scale where the most similar file is
close to 1.0 and the least similar in the result set is close to 0.0.

The subject itself is always 1.0 (hardcoded, not computed from the
embedding — identity is always max similarity regardless of what the
distance function returns).

Displayed as a floating-point value in the `sim` column (e.g., `0.87`).

### Column

Add `sim` to the available TUI columns. Only populated when a similarity
sort is active; blank otherwise. Float, right-aligned.

```toml
# config.toml
columns = ["vendor", "library", "key", "bpm", "rating", "sim"]
```

### Ordering

Similarity sort is always ascending distance (most similar first).
**No reverse ordering.** This constraint enables a JIT optimization:
the PQ ADC scan can early-terminate once it has N candidates below a
distance threshold, skipping the tail of the corpus. With reverse
ordering this wouldn't be possible.

### Files without embeddings

Files with `embedding IS NULL` sort to the bottom (sim = 0, or excluded
from results entirely with a `--embedded-only` flag).

## CLI Interface

```bash
# Sort by similarity to a file (headless, top 20)
rfg --similar ~/Samples/kick_707.wav --limit 20

# Combine with filters
rfg --similar ~/Samples/kick_707.wav --category KICK

# Text-based similarity (future, uses CLAP's shared text-audio space)
rfg --sounds-like "bright metallic percussion"

# Embedding management (Python scripts)
python scripts/embed_train.py --sample-dir ~/Samples --n-train 10000
python scripts/embed_encode.py --db-path index.db
```

---

## Open Questions

1. **Codebook versioning.** If the codebook is retrained, the in-memory
   PQ codes (built from full-precision vectors) automatically use the new
   codebook — no re-encoding needed. But if a codebook hash is stored for
   cache validation, it needs to be updated. Simplest: store a version
   counter in the metadata table, bump on retrain.

2. **LOOP splitting.** The "split at first onset" strategy needs a
   concrete onset detection implementation. Could be as simple as
   "first sample above -30dBFS" or use a proper energy-based detector.
   This only affects the Python training/encoding pipeline.

3. **Incremental updates.** New files added to the library need embedding.
   The encoding script is already incremental (`WHERE embedding IS NULL`).
   PQ codebook retraining is optional — the existing codebook works for
   new files from similar distributions.

4. **Query without pre-computed embedding.** v1 requires the query file
   to already be embedded. v2 could ship a small ONNX runtime for
   on-the-fly CLAP inference in Rust, removing the Python dependency
   for single-file queries.

5. **DB size.** 2.3 GB for 1.2M files is significant. Could add an
   `rfg --compact-embeddings` mode that replaces full vectors with PQ
   codes in the DB (128 bytes/file = 146 MB) for space-constrained
   systems. This is lossy and one-way.

---

## Alternatives Considered

### A: PQ codes in BEXT `[128:256]`

The original design stored 128-byte PQ codes in the BEXT packed
Description reserved field and mirrored them in the DB.

**Layout:** `[128:256]` → 128 × u8 PQ centroid indices.

**Advantages:**
- Embeddings travel with the file (portable)
- Smaller DB (128 bytes/file vs 2048)
- No need to load full vectors into memory

**Why rejected:**
- **Lossy storage.** PQ compression costs ~5-10% recall, permanently.
  With DB-only full-precision storage, PQ is a search-time optimization
  that can be retrained without re-running CLAP inference.
- **Format-dependent.** AIFF, MP3, FLAC files have no BEXT chunk. DB-only
  storage is format-agnostic — same code path for all file types.
- **Not self-contained.** The 128 bytes are meaningless without the
  matching PQ codebook. Unlike peaks (directly renderable), embeddings
  require external state to interpret. Portability is illusory.
- **Wastes BEXT space.** The reserved bytes could be used for something
  that actually benefits from in-file storage (e.g., a perceptual hash
  for deduplication, which *is* self-contained).

### B: VGGish (native 128-dim, no PQ)

VGGish outputs 128 floats natively. Quantize to u8 → 128 bytes, fits in
BEXT without PQ.

**Why rejected:** Timbral quality is noticeably worse than CLAP for
music/sample content. The embedding space lacks semantic structure from
text-audio alignment. Also still has the format-dependency problem.

### C: Custom autoencoder (native 128-dim)

Train a small convolutional autoencoder on the user's library to produce
a 128-dim latent directly.

**Why rejected:** Requires a training pipeline, less generalizable, and
the user's library may not be large enough. CLAP's pretrained weights
generalize much better. Could revisit if CLAP's 600MB model size becomes
a deployment issue.
