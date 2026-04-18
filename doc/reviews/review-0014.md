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

<!-- gh-id: 3104722575 -->
### Copilot on [`src/main.rs:62`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722575) (2026-04-18 06:31 UTC)

The docs describe `TuiSimilar(PATH)` and imply rule 3 applies to all filters, but `Dispatch::TuiSimilar` is a unit variant and `--similar` is evaluated before `has_search_filters()`. Please update the precedence documentation to reflect the actual behavior (e.g., `--similar PATH` → `TuiSimilar`, and filters only route to `Headless` when `--similar` is not set).
```suggestion
/// 2. `--similar PATH` → `TuiSimilar`.
/// 3. Any non-empty search filter, when `--similar` is not set
///    → `Headless`.
/// 4. Otherwise → `TuiBrowse`.
///
/// Rule 1 subsumes `--no-tui`, `--verbose`, `--json`, `--count`,
/// `--index`, `--db-stats`, any workflow flag (`--eval`/`--workflow`),
/// and stdout-not-a-TTY. Rule 3 preserves the historical behavior
/// where filter flags at a terminal go headless (e.g. `rfg --vendor
/// Mars` prints paths instead of launching the TUI), but only if
/// rule 2 did not already match because `dispatch()` checks
/// `opts.similar.is_some()` before `opts.has_search_filters()`.
```

<!-- gh-id: 3104722596 -->
### Copilot on [`src/main.rs:185`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722596) (2026-04-18 06:31 UTC)

This test comment claims `--vendor` can be applied via the TUI search bar, but in similarity mode the search bar only substring-filters paths and explicitly does not support `@field=value` column filters. Either (a) treat `--similar` + search-filter flags as `Headless`/error to avoid silently ignoring flags, or (b) explicitly document that non-path filters are ignored in similarity TUI mode and adjust the comment accordingly.
```suggestion
    fn similar_with_filter_at_tty_still_routes_to_similar_but_filter_is_ignored() {
        // Precedence: --similar currently wins over --vendor. If both
        // are set we still launch the TUI in similarity mode rather
        // than falling through to the filter-triggered headless path.
        //
        // Note: in similarity mode the TUI search bar only performs a
        // substring filter on paths; it does not support column-style
        // filters such as `--vendor`. So this test documents dispatch
        // precedence only, not interactive application of `--vendor`.
```

<!-- gh-id: 3104722603 -->
### Copilot on [`src/ui/mod.rs:2881`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722603) (2026-04-18 06:31 UTC)

The comment says 'trim sims to match' but the code bails out instead. Please update the comment to match the behavior (or implement trimming if that’s the intended recovery).

<!-- gh-id: 3104722608 -->
### Copilot on [`src/ui/mod.rs:438`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722608) (2026-04-18 06:31 UTC)

The warning triggers on any `@` anywhere in the query, which can produce false positives for legitimate path substrings (e.g., searching for `@` in filenames). A more precise check would be to reuse the existing `parse_column_filters` splitter and warn only if it detects column filters (or at least gate on `trim_start().starts_with('@')`).

<!-- gh-id: 3104722614 -->
### Copilot on [`src/engine/sqlite.rs:757`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722614) (2026-04-18 06:31 UTC)

If `paths` contains duplicates, the `HashMap<&str, usize>` overwrites earlier indices and `WHERE path IN (...)` will only return one row per unique path, so output ordering/length won’t correspond to input. Consider either (a) explicitly enforcing uniqueness with an `ensure!` + clear error message, or (b) supporting duplicates by mapping `path -> Vec<usize>` and reconstructing the output (potentially by querying unique paths and then expanding back to the original list).

<!-- gh-id: 3104722622 -->
### Copilot on [`doc/designs/debt-fsm.md:17`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722622) (2026-04-18 06:31 UTC)

Correct the typo 'contraint' to 'constraint'.
```suggestion
  - Make size, complexity, & constraint specifics configurable whereever possible
```

<!-- gh-id: 3104722630 -->
### Copilot on [`doc/designs/debt-fsm.md:17`](https://github.com/cmk/riffgrep/pull/14#discussion_r3104722630) (2026-04-18 06:31 UTC)

Correct the typo 'whereever' to 'wherever'.
```suggestion
  - Make size, complexity, & contraint specifics configurable wherever possible
```

<!-- gh-id: 4133839554 -->
### copilot-pull-request-reviewer[bot] — COMMENTED ([2026-04-18 06:31 UTC](https://github.com/cmk/riffgrep/pull/14#pullrequestreview-4133839554))

## Pull request overview

> [!NOTE]
> Copilot was unable to run its full agentic suite in this review.

Fixes `rfg --similar PATH` incorrectly launching the browse TUI at a TTY by introducing explicit dispatch logic and ensuring the TUI preloads similarity-ranked results on startup.

**Changes:**
- Replace `should_launch_tui` with a `dispatch(opts, is_tty) -> Dispatch` enum and add table-driven unit tests for dispatch precedence.
- Add similarity-mode state + local filtering behavior to the TUI, including synchronous initial similarity load and status-bar warning for unsupported `@field=value` filters.
- Add a SQLite batch loader to fetch `TableRow`s for a list of paths while preserving input order.

### Reviewed changes

Copilot reviewed 5 out of 7 changed files in this pull request and generated 7 comments.

<details>
<summary>Show a summary per file</summary>

| File | Description |
| ---- | ----------- |
| src/main.rs | Introduces a dispatch enum to route between headless, browse TUI, and similarity TUI; adds unit tests for dispatch precedence. |
| src/ui/mod.rs | Adds similarity-mode state, startup preload of similarity results, and local filtering behavior in the TUI. |
| src/engine/sqlite.rs | Adds `load_table_rows_for_paths` to batch-load rows by path in input order + tests. |
| doc/reviews/review-0014.md | Adds an internal review log for this PR’s changes and decisions. |
| doc/designs/debt-fsm.md | Adds a forward-looking design note for FSM + property-testing refactors. |
| .gitignore | Ignores `ext/` directory intended for external clones/models. |
</details>





