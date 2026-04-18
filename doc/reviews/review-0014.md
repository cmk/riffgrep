# PR #14 — fix/similar-tui-dispatch (--similar TUI dispatch fix)

## Local review (2026-04-18)

**Branch:** fix/similar-tui-dispatch
**Commits (pre-fix):** 5 (origin/main..fix/similar-tui-dispatch, excluding the operator's `doc:` commit that added FSM.md)
**Reviewer:** Claude (sonnet, independent)

---

### Dispatch precedence (main.rs)

Rules correct and ordered defensibly. 15 table-driven tests cover the cross product of `{--similar on/off} × {is_tty true/false} × {output-forcing flags, filters, subcommands}`. One matrix gap — `--similar` + `--count` — closed in the fix commit.

### TUI startup error recovery (run_tui)

Falling back to browse mode + status-bar error is correct UX. One pre-existing edge case noted (no-DB spawns a SearchHandleTable that will itself error) — not introduced by this branch, not blocking.

### load_table_rows_for_paths

500-cap and placeholder-only interpolation are correct. `HashMap<&str, usize>` lifetimes are sound — keys borrow from `paths: &[&str]` which outlives the function. Test shuffles query order but SQLite happens to return in insertion order; comment notes the gap.

### filter_similarity_results — path-only filter

Path-only limitation was undocumented for end users. `@field=value` column filters (supported in normal-mode search) silently return zero in similarity mode. Fixed: now surfaces a one-shot status message when the query contains `@`, warning that `@field=value` filters are not supported.

### Panic on length mismatch

`load_similarity_results` asserts `rows.len() == sims.len()` and panics on mismatch. Consistent with codebase style for programming-error assertions. Caller (`run_tui`) enforces the invariant via a prior `bail!`, so the panic path is unreachable in normal operation. The `#[should_panic]` test documents the contract.

### scroll_offset reset

Original "only reset if out of range" was inconsistent with normal-mode search behavior. Fixed: always reset to 0 on filter change.

---

### Resolution (pre-push)

| # | Item | Fix commit |
|---|------|------------|
| 1 | `Dispatch::TuiSimilar(PathBuf)` dead inner value | `599f422` — unit variant, opts.similar stays single source of truth |
| 2 | `unwrap_or("")` on non-UTF-8 paths silently broke order invariant | `599f422` — bail with diagnostic on non-UTF-8 |
| 3 | `--similar` + `--count` test gap | `599f422` — added `similar_with_count_routes_to_headless` |
| 4 | `@field=value` silently returns zero in similarity mode | `599f422` — one-shot status message when query contains `@` |
| 5 | `scroll_offset` preserved across filter shrink is disorienting | `599f422` — unconditional reset to 0 |

Cleared for push. 763 tests pass, clippy clean, fmt clean.
