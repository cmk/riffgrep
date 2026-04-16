# Agentic Integration Design

## Context

riffgrep exposes sample search, similarity, and metadata as a Rust
lib+bin. To make it usable by an LLM agent (Claude via MCP, or any
tool-calling model), we need a programmatic interface and a hosting
strategy.

This doc distills the actionable ideas from a Gemini brainstorm into
concrete plans aligned with the stdio MCP server architecture.

---

## Integration Architecture

### stdio as host

riffgrep becomes a git submodule in stdio (`ext/riffgrep`), with a thin
`crates/driver-rfg` wrapper that exposes MCP tools via rmcp's
`#[tool_router]` / `#[tool]` macros. The server composes it alongside
driver-mpc, driver-qu, etc. via `ToolRouter::merge()`.

```
stdio/
├── ext/riffgrep/          ← git submodule (pinned commit)
├── crates/driver-rfg/     ← MCP tool wrapper
│   └── src/lib.rs         ← calls riffgrep::engine::api::*
└── crates/server/
    └── src/main.rs        ← merges all drivers
```

### MCP Tool Surface

```
rfg_search(query, filters, limit)
  → Search by metadata (vendor, category, bpm, key, etc.)
  → Returns JSON array of UnifiedMetadata

rfg_similar(path, limit)
  → Find perceptually similar sounds via CLAP embeddings
  → Returns ranked results with distance + sim score

rfg_metadata(path)
  → Read full metadata for a single audio file
  → Returns UnifiedMetadata as JSON

rfg_index(roots, db_path)
  → Build or update the search index
  → Returns IndexStats (files indexed, duration, errors)
```

The agent never sees raw embeddings. It works with paths, metadata
fields, and similarity scores.

### Future tools (post-PQ)

```
rfg_sounds_like(text, limit)
  → Text-to-audio search via CLAP's shared latent space
  → "bright metallic percussion" → ranked audio results

rfg_cluster_outliers(limit)
  → Find sounds that don't fit any cluster
  → Agent helps triage: trash vs. unique textures

rfg_timbral_stats(folder)
  → Aggregate embedding statistics for a folder
  → "80% percussive, mean spectral centroid 4.2kHz"
```

---

## Search Performance Tiers

### Phase 1 (current): Brute-force L2

- Loads all embeddings into RAM, computes distances
- Acceptable up to ~50K files (~100MB RAM)
- Latency: ~200ms at 50K, linear scaling

### Phase 2: Product Quantization (ADC)

- 128 sub-spaces × 256 centroids, trained via k-means
- Each 512-dim f32 vector compressed to 128-byte PQ code
- Search via Asymmetric Distance Computation (lookup table)
- Memory: ~160MB for 1.2M codes (fits in L3 cache)
- Latency: ~15-30ms at 1.2M (rayon parallel scan)

```rust
// ADC inner loop — 128 table lookups per sample
fn compute_adc(code: &[u8; 128], lut: &[f32; 128 * 256]) -> f32 {
    code.iter()
        .enumerate()
        .map(|(sub, &idx)| lut[(sub << 8) | idx as usize])
        .sum()
}
```

No SIMD intrinsics in v1 — profile first. The compiler auto-vectorizes
the fixed-size loop in most cases. SIMD (NEON on Apple Silicon, AVX2
on Intel) is a Phase 2+ optimization if profiling shows the scan is
the bottleneck rather than I/O.

### Phase 3: Text-query via CLAP

- CLAP's text encoder produces vectors in the same latent space
- `--sounds-like "warm analog pad"` → encode text → ADC scan
- Same PQ infrastructure, different query source

---

## Agent Interaction Patterns

### Reactive search (low latency required)

With PQ search under 30ms, the agent can run multiple queries
speculatively before responding:

```
User: "I need a more aggressive snare for this breakbeat"
Agent: runs rfg_sounds_like("aggressive industrial snare")
       filters results by category=ONESHOT
       runs rfg_similar(best_match) for alternatives
       presents top 3 with metadata context
```

### Metadata cleanup (batch, latency-tolerant)

```
Agent: runs rfg_cluster_outliers(100)
       for each outlier, reads metadata + nearest neighbors
       suggests: reclassify, retag, or flag for review
```

### Session context

The agent can maintain a "palette" of sounds it has recommended in
the current session. When asked for variations, it searches near the
palette centroid rather than starting fresh.

---

## Implementation Sequence

```
Plan 1: Harden prototype (fix review findings)
Plan 2: Extract headless API (engine::api module)
Plan 3: PQ acceleration (sub-30ms at 1.2M)
Plan 4: stdio integration (driver-rfg crate)

Plan 1 ──→ Plan 3 (PQ needs schema migration from Plan 1)
Plan 2 ──→ Plan 4 (stdio needs headless API from Plan 2)
```

---

## What was cut from the Gemini brainstorm

- **SIMD intrinsics** (AVX-512, NEON `tbl`): premature. Profile first.
- **BLAKE3 content-addressing**: UUID v7 anchors in BEXT already solve
  file identity. Content hashing adds complexity without clear benefit.
- **Named pipe / daemon mode**: stdio already provides the persistent
  server. No need for a separate daemon.
- **Real-time transient remixing**: vaporware.
- **Hardware-specific tuning** (384GB RAM, 6-channel DDR4): profile on
  real data before optimizing for a specific machine.
