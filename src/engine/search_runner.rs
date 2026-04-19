//! Stateful wrapper around [`SearchFsm`] that holds the data the FSM
//! intentionally doesn't: the results vec, selected row, scroll
//! offset, `total_matches`, the similarity snapshot, and the
//! columns/sort display state.
//!
//! Per `stdio/doc/designs/tui-fsm.md` §3 ("Side effects returned, not
//! performed, by transitions"), this runner does **not** spawn tokio
//! tasks or perform I/O. It consumes [`Input`](crate::engine::search_fsm::Input)
//! events, delegates mode transitions to the FSM, mutates its own
//! data fields, and returns [`DispatchResult`] — which pairs the
//! FSM's [`Output`](crate::engine::search_fsm::Output) effect
//! descriptor (for the App or stdio form-handler to honor) with an
//! optional [`TypedAction`] synthesized from runner state (for the
//! `FireSelection` placeholder).
//!
//! Typed actions replace today's stringified action dispatches at the
//! output edge. Starting narrow — `LoadSample(PathBuf)` — and
//! expanding as real use cases surface. A single-variant enum is
//! still typed; it's strings we're avoiding.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::engine::TableRow;
use crate::engine::search_fsm::{Input, Output, SearchFsm};

/// Actions the runner hands back to the App / stdio form-handler at
/// the output edge. The FSM's [`Output::FireSelection`] is a
/// placeholder; the runner reads its selected-row state and builds
/// a typed action. Per the rust-fsm gotcha in `tui-fsm.md` — the FSM
/// can't see wrapper data, so data-dependent effects synthesize here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedAction {
    /// Open the selected row's sample in the external editor / sampler.
    /// Today the downstream consumer is a keybind that spawns a
    /// shell-open; in stdio this wraps into a tool-call.
    LoadSample(PathBuf),
}

/// Paired (Output, TypedAction) from a [`SearchRunner::dispatch`] call.
/// Either, both, or neither may be `Some`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DispatchResult {
    /// FSM effect descriptor; the App performs the I/O (spawn, abort,
    /// etc.).
    pub output: Option<Output>,
    /// Typed action synthesized from wrapper state. Set only when the
    /// FSM emitted `Output::FireSelection` AND the runner has a valid
    /// selected row.
    pub action: Option<TypedAction>,
}

/// Holds the data layer that [`SearchFsm`] deliberately excludes, plus
/// the FSM itself. `App` wraps this; tests instantiate directly.
#[allow(dead_code)] // App integration lands in Task 4.
pub struct SearchRunner {
    /// The underlying FSM.
    fsm: SearchFsm,
    /// Current result set (streams in from a spawned search task).
    results: Vec<TableRow>,
    /// Selected row index within `results`.
    selected: usize,
    /// Scroll offset for the results viewport.
    scroll_offset: usize,
    /// Total match count from the settled search.
    total_matches: usize,
    /// Immutable snapshot installed by [`SearchRunner::load_similarity_snapshot`].
    /// `filter_similarity` filters this into `results` without
    /// re-querying the DB. `None` outside similarity mode.
    similarity_snapshot: Option<Vec<TableRow>>,
    /// Column display list.
    columns: Vec<String>,
    /// Active sort column name (None = unsorted).
    sort_column: Option<String>,
    /// Sort direction: true = ascending.
    sort_ascending: bool,
}

#[allow(dead_code)] // App integration lands in Task 4.
impl SearchRunner {
    /// Create a runner with the given initial column list. Every
    /// other field starts empty / zero; use the setter methods to
    /// install initial state for tests.
    pub fn new(columns: Vec<String>) -> Self {
        SearchRunner {
            fsm: SearchFsm::new(),
            results: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            total_matches: 0,
            similarity_snapshot: None,
            columns,
            sort_column: None,
            sort_ascending: true,
        }
    }

    // ---------- FSM dispatch ----------

    /// Dispatch an [`Input`] through the FSM and, when the emitted
    /// [`Output`] is data-dependent (today: `FireSelection`),
    /// synthesize the corresponding [`TypedAction`] from wrapper state.
    ///
    /// This is the single entry point callers should use. It does
    /// **not** perform any I/O — the returned `output` is the caller's
    /// to honor (spawn a tokio task, abort a handle, etc.).
    pub fn dispatch(&mut self, input: Input) -> DispatchResult {
        let output = self.fsm.consume(input);
        let action = match &output {
            Some(Output::FireSelection) => self.selected_path().map(TypedAction::LoadSample),
            _ => None,
        };
        DispatchResult { output, action }
    }

    // ---------- wrapper-method data mutations ----------

    /// Replace the result set wholesale. Called by the App when a
    /// search's first batch arrives (after firing
    /// [`Input::SearchStarted`]) or when similarity filtering rebuilds
    /// the list. Resets `selected` and `scroll_offset` to 0.
    pub fn set_results(&mut self, rows: Vec<TableRow>) {
        self.results = rows;
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Append to the current result set (streaming search batches).
    pub fn append_results(&mut self, rows: Vec<TableRow>) {
        self.results.extend(rows);
    }

    /// Set total match count reported by the settled search.
    pub fn set_total_matches(&mut self, total: usize) {
        self.total_matches = total;
    }

    /// Install a similarity-ranked snapshot. Caller should immediately
    /// follow with `dispatch(Input::EnterSimilarityMode)` so the FSM
    /// mode flips.
    pub fn load_similarity_snapshot(&mut self, mut rows: Vec<TableRow>, sims: Vec<f32>) {
        assert_eq!(
            rows.len(),
            sims.len(),
            "load_similarity_snapshot: rows.len()={} sims.len()={}",
            rows.len(),
            sims.len(),
        );
        for (row, sim) in rows.iter_mut().zip(sims.iter()) {
            row.sim = Some(*sim);
        }
        self.similarity_snapshot = Some(rows.clone());
        self.results = rows;
        self.selected = 0;
        self.scroll_offset = 0;
        self.total_matches = self.results.len();
        self.sort_column = Some("sim".to_string());
        self.sort_ascending = false;
    }

    /// Drop the similarity snapshot. Called by the App when exiting
    /// similarity mode so the next remote search's results don't
    /// collide with stale cached data.
    pub fn clear_similarity_snapshot(&mut self) {
        self.similarity_snapshot = None;
    }

    /// Re-apply the local substring filter against the similarity
    /// snapshot. No-op when the snapshot is absent. Case-insensitive
    /// substring match on each row's path. Called by the App in
    /// response to [`Output::FilterSimilarity`].
    pub fn apply_similarity_filter(&mut self, query: &str) {
        let Some(ref snapshot) = self.similarity_snapshot else {
            return;
        };
        let q = query.trim().to_lowercase();
        self.results = if q.is_empty() {
            snapshot.clone()
        } else {
            snapshot
                .iter()
                .filter(|row| row.meta.path.to_string_lossy().to_lowercase().contains(&q))
                .cloned()
                .collect()
        };
        // Preserve selection when still in range; clamp when the
        // filter shrank past it. Matches App::filter_similarity_results
        // — resetting to 0 would be a user-observable regression
        // (filter changes feel like "you lost my place").
        self.selected = if self.results.is_empty() {
            0
        } else {
            self.selected.min(self.results.len() - 1)
        };
        self.scroll_offset = 0;
        self.total_matches = self.results.len();
    }

    // ---------- selection + navigation ----------

    /// Move the selected row by `delta` (positive = down). Clamped to
    /// `[0, results.len() - 1]`.
    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let max = self.results.len() - 1;
        let new_sel = if delta >= 0 {
            self.selected.saturating_add(delta as usize).min(max)
        } else {
            self.selected.saturating_sub((-delta) as usize)
        };
        self.selected = new_sel;
    }

    /// Set scroll offset directly (called from the render pass).
    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    // ---------- accessors ----------

    /// Underlying FSM, for transport/mode reads.
    pub fn fsm(&self) -> &SearchFsm {
        &self.fsm
    }

    /// Current result list.
    pub fn results(&self) -> &[TableRow] {
        &self.results
    }

    /// Selected row index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Total match count from the last settled search.
    pub fn total_matches(&self) -> usize {
        self.total_matches
    }

    /// Path of the currently-selected row, if any.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.results
            .get(self.selected)
            .map(|row| row.meta.path.clone())
    }

    /// Whether a similarity snapshot is currently loaded.
    pub fn has_similarity_snapshot(&self) -> bool {
        self.similarity_snapshot.is_some()
    }

    /// Column display list.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Active sort column name.
    pub fn sort_column(&self) -> Option<&str> {
        self.sort_column.as_deref()
    }

    /// Sort direction (true = ascending).
    pub fn sort_ascending(&self) -> bool {
        self.sort_ascending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::UnifiedMetadata;
    use crate::engine::search_fsm::{Mode, Transport};

    fn row(path: &str) -> TableRow {
        TableRow {
            meta: UnifiedMetadata {
                path: PathBuf::from(path),
                ..UnifiedMetadata::default()
            },
            audio_info: None,
            marked: false,
            markers: None,
            sim: None,
        }
    }

    fn default_runner() -> SearchRunner {
        SearchRunner::new(vec!["path".to_string()])
    }

    // ---------- dispatch + Output routing ----------

    #[test]
    fn dispatch_query_changed_sets_fsm_dirty_no_output() {
        let mut r = default_runner();
        let res = r.dispatch(Input::QueryChanged("foo".to_string()));
        assert_eq!(res.output, None);
        assert_eq!(res.action, None);
        assert_eq!(r.fsm().query(), "foo");
        assert!(r.fsm().state().debounce_dirty);
    }

    #[test]
    fn dispatch_debounce_tick_remote_emits_spawn_search() {
        let mut r = default_runner();
        let _ = r.dispatch(Input::QueryChanged("hi".to_string()));
        let res = r.dispatch(Input::DebounceTick);
        assert_eq!(
            res.output,
            Some(Output::SpawnSearch {
                query: "hi".to_string()
            }),
        );
        assert_eq!(res.action, None);
        assert_eq!(r.fsm().state().transport, Transport::Pending);
    }

    #[test]
    fn dispatch_enter_similarity_emits_cancel() {
        let mut r = default_runner();
        let _ = r.dispatch(Input::QueryChanged("q".to_string()));
        let _ = r.dispatch(Input::DebounceTick); // Pending
        let res = r.dispatch(Input::EnterSimilarityMode);
        assert_eq!(res.output, Some(Output::CancelSearch));
        assert_eq!(r.fsm().state().mode, Mode::Similarity);
    }

    // ---------- FireSelection synthesis ----------

    #[test]
    fn fire_selection_synthesizes_typed_action_when_row_selected() {
        let mut r = default_runner();
        r.set_results(vec![row("/a.wav"), row("/b.wav")]);
        r.move_selection(1); // selected = 1 → /b.wav
        let res = r.dispatch(Input::FireSelection);
        assert_eq!(res.output, Some(Output::FireSelection));
        assert_eq!(
            res.action,
            Some(TypedAction::LoadSample(PathBuf::from("/b.wav"))),
        );
    }

    #[test]
    fn fire_selection_action_none_when_results_empty() {
        let mut r = default_runner();
        let res = r.dispatch(Input::FireSelection);
        assert_eq!(res.output, Some(Output::FireSelection));
        assert_eq!(
            res.action, None,
            "no selected path → runner must not synthesize LoadSample",
        );
    }

    // ---------- similarity snapshot + filter ----------

    #[test]
    fn similarity_snapshot_roundtrip_with_filter() {
        let mut r = default_runner();
        let rows = vec![row("/kick.wav"), row("/snare.wav"), row("/kick_808.wav")];
        let sims = vec![1.0, 0.5, 0.9];
        r.load_similarity_snapshot(rows, sims);
        assert!(r.has_similarity_snapshot());
        assert_eq!(r.results().len(), 3);
        assert_eq!(r.total_matches(), 3);
        assert_eq!(r.sort_column(), Some("sim"));
        assert!(!r.sort_ascending());

        // Simulate the FSM emitting FilterSimilarity on tick.
        r.apply_similarity_filter("kick");
        assert_eq!(r.results().len(), 2);
        assert_eq!(r.total_matches(), 2);

        // Clearing the query restores the full snapshot.
        r.apply_similarity_filter("");
        assert_eq!(r.results().len(), 3);
    }

    #[test]
    fn apply_similarity_filter_without_snapshot_is_noop() {
        let mut r = default_runner();
        r.set_results(vec![row("/a.wav"), row("/b.wav")]);
        let before_paths: Vec<PathBuf> = r.results().iter().map(|r| r.meta.path.clone()).collect();
        r.apply_similarity_filter("anything");
        let after_paths: Vec<PathBuf> = r.results().iter().map(|r| r.meta.path.clone()).collect();
        assert_eq!(after_paths, before_paths);
    }

    // ---------- full lifecycle spot check ----------

    #[test]
    fn end_to_end_remote_search_lifecycle() {
        let mut r = default_runner();

        // 1. User types — dirty set, no output yet.
        let res = r.dispatch(Input::QueryChanged("foo".to_string()));
        assert_eq!(res.output, None);

        // 2. Debounce fires — FSM → Pending, caller spawns task.
        let res = r.dispatch(Input::DebounceTick);
        assert!(matches!(res.output, Some(Output::SpawnSearch { .. })));
        assert_eq!(r.fsm().state().transport, Transport::Pending);

        // 3. First batch arrives — caller calls set_results, fires
        //    SearchStarted.
        r.set_results(vec![row("/hit.wav")]);
        let _ = r.dispatch(Input::SearchStarted);
        assert_eq!(r.fsm().state().transport, Transport::Running);

        // 4. Search completes — SearchSettled with total.
        r.set_total_matches(42);
        let _ = r.dispatch(Input::SearchSettled { total: 42 });
        assert_eq!(r.fsm().state().transport, Transport::Settled);
        assert_eq!(r.total_matches(), 42);

        // 5. User hits Enter — FireSelection synthesizes LoadSample.
        let res = r.dispatch(Input::FireSelection);
        assert_eq!(
            res.action,
            Some(TypedAction::LoadSample(PathBuf::from("/hit.wav"))),
        );
    }

    // ---------- search-failed path (the gotcha fix) ----------

    #[test]
    fn search_failed_lands_settled_without_orphaning_pending() {
        let mut r = default_runner();
        let _ = r.dispatch(Input::QueryChanged("bad".to_string()));
        let _ = r.dispatch(Input::DebounceTick); // Pending + SpawnSearch

        // Task errors before any batch or settle arrives. Runner
        // fires SearchFailed.
        r.set_total_matches(0);
        let res = r.dispatch(Input::SearchFailed);
        assert_eq!(res.output, None, "SearchFailed is a pure state signal");
        assert_eq!(
            r.fsm().state().transport,
            Transport::Settled,
            "FSM must not stay orphaned in Pending after task error",
        );
    }

    // ---------- selection bounds ----------

    #[test]
    fn move_selection_clamps_to_bounds() {
        let mut r = default_runner();
        r.set_results(vec![row("/a"), row("/b"), row("/c")]);
        r.move_selection(10);
        assert_eq!(r.selected(), 2, "clamp to last row");
        r.move_selection(-10);
        assert_eq!(r.selected(), 0, "clamp to first row");
    }

    #[test]
    fn move_selection_on_empty_is_noop() {
        let mut r = default_runner();
        r.move_selection(1);
        assert_eq!(r.selected(), 0);
    }
}
