//! Search / Results / Similarity finite state machine.
//!
//! Formalizes the search-transport and similarity-mode state that today
//! lives as ad-hoc flags on `App` (`search_in_progress`,
//! `search_pending`, `query_changed`, `in_similarity_mode`).
//!
//! First sub-region of the TUI refactor. Designed against the six
//! considerations in `stdio/doc/designs/tui-fsm.md` so the eventual
//! stdio embedding is mechanical:
//!
//! - State is [`Serialize`] + [`Deserialize`] (consideration #1).
//! - [`Input`] is the external event API (#2).
//! - [`Output`] is a pure effect descriptor; a runner consumes it and
//!   performs I/O (#3).
//! - The FSM receives already-translated events — keybindings live
//!   above (#4).
//! - [`Output::FireSelection`] is a placeholder; the runner reads
//!   wrapper state and synthesizes a typed
//!   [`TypedAction::LoadSample(PathBuf)`](crate::engine::search_runner::TypedAction)
//!   (#5).
//! - Cleanup (#6) is split across the runner/FSM boundary: the
//!   App-side integration receives [`Output::CancelSearch`] and
//!   performs the abort; [`Input::SearchCancelled`] is the reverse
//!   signal from the runner back to the FSM after cancellation has
//!   occurred (collapses transport to `Settled`; no output).
//!
//! See `doc/plans/plan-2026-04-19-01.md` for the full sprint scope.

use rust_fsm::{StateMachine, StateMachineImpl};
use serde::{Deserialize, Serialize};

/// Search transport state. Mirrors the combination of today's
/// `search_in_progress` / `search_pending` flags into an explicit tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Transport {
    /// No search in flight; results list (if any) is the last settled
    /// result set.
    #[default]
    Idle,
    /// A new search has been dispatched, first batch hasn't arrived.
    /// Matches today's `search_pending` while old results stay visible
    /// to prevent flicker.
    Pending,
    /// Search is streaming results; matches `search_in_progress` once
    /// the first batch has landed.
    Running,
    /// Search completed (or was cancelled); results list is stable.
    Settled,
}

/// Search mode. `Remote` dispatches queries to the DB / filesystem
/// walker; `Similarity` filters the runner-held snapshot locally (the
/// ranking from the preceding `rfg --similar PATH` call).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Mode {
    /// Queries spawn DB/walker tasks.
    #[default]
    Remote,
    /// Queries run a local substring filter over the runner's snapshot.
    /// Matches today's `App::in_similarity_mode == true`.
    Similarity,
}

/// Full search-FSM state. Query text lives here (it's user-observable
/// and drives transitions); the `results` list, scroll offset, sort
/// state, and the similarity snapshot all live on the runner (they're
/// data mutated by wrapper methods, not modes governed by the FSM).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFsmState {
    /// Current search-transport state.
    pub transport: Transport,
    /// Current search mode.
    pub mode: Mode,
    /// Current query string. Mutated by [`Input::QueryChanged`] and
    /// [`Input::QueryCleared`].
    pub query: String,
    /// Debounce flag: set by `QueryChanged`, cleared by `DebounceTick`.
    /// Matches today's `App::query_changed`.
    pub debounce_dirty: bool,
}

impl Default for SearchFsmState {
    fn default() -> Self {
        SearchFsmState {
            transport: Transport::Idle,
            mode: Mode::Remote,
            query: String::new(),
            debounce_dirty: false,
        }
    }
}

/// All inputs that drive search-FSM transitions.
///
/// Split into three groups by origin (see `tui-fsm.md` §2):
/// - **User events** (translated from keybindings at a higher layer):
///   `QueryChanged`, `QueryCleared`, `SubmitQuery`, `FireSelection`,
///   `EnterSimilarityMode`, `ExitSimilarityMode`.
/// - **Timing events**: `DebounceTick`.
/// - **Runner / external events**: `SearchStarted`, `SearchSettled`,
///   `SearchCancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)] // Wiring to the runner lands in Task 2-4.
pub enum Input {
    /// User typed into the query bar — updates `query` and sets
    /// `debounce_dirty`. The follow-up action is debounced in both
    /// modes: a subsequent `DebounceTick` emits either
    /// `Output::SpawnSearch` (Remote) or `Output::FilterSimilarity`
    /// (Similarity). Keeping Similarity on the same debounce path
    /// matches today's TUI behavior and avoids re-filtering on every
    /// keystroke.
    QueryChanged(String),
    /// User cleared the query (Ctrl-U or equivalent). Equivalent to
    /// `QueryChanged(String::new())` in effect; kept as a separate
    /// variant so the runner can distinguish "explicit clear" from
    /// "typed over to empty" if needed for logging.
    QueryCleared,
    /// User pressed Enter on the query bar (no-op today outside
    /// dispatch — reserved for future "commit filter" semantics).
    SubmitQuery,
    /// Debounce timer fired. Consumes `debounce_dirty`; emits either
    /// `Output::SpawnSearch` (Remote) or `Output::FilterSimilarity`
    /// (Similarity).
    DebounceTick,
    /// Runner signal: the spawned search task reported its first
    /// batch. Pending → Running.
    SearchStarted,
    /// Runner signal: search completed. Running → Settled. `total` is
    /// the total match count to update `App::total_matches`.
    SearchSettled {
        /// Total match count reported by the search task.
        total: usize,
    },
    /// Runner signal: search was cancelled (either by a new query
    /// arriving or by an external cancel). Any → Settled.
    SearchCancelled,
    /// Runner signal: the search task errored before emitting any
    /// results (e.g. query parse failed, DB unavailable). Any →
    /// Settled with `total_matches = 0`. Separate from
    /// `SearchCancelled` so the runner can show a different status
    /// message (error vs. "cancelled"), and so stdio can distinguish
    /// a clean cancel from a failure in form-patch telemetry.
    SearchFailed,
    /// User pressed Enter on a result row. Emits `Output::FireSelection`
    /// (a placeholder; runner synthesizes `TypedAction::LoadSample`
    /// from its own selected-row state).
    FireSelection,
    /// Install a similarity-ranked snapshot and switch to
    /// `Mode::Similarity`. The snapshot itself lives on the runner;
    /// this input just flips the mode.
    EnterSimilarityMode,
    /// Exit `Mode::Similarity` back to `Mode::Remote`. Sets
    /// `debounce_dirty` so the next `DebounceTick` fires a fresh
    /// remote search with the current query. **No-op when already
    /// in `Mode::Remote`** — otherwise a defensive call from the
    /// runner would reset a live transport to Idle.
    ExitSimilarityMode,
}

/// Effect descriptors the caller (runner) must honor. Pure state
/// transitions return `None` from [`StateMachineImpl::output`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)] // Wiring to the runner lands in Task 2-4.
pub enum Output {
    /// Runner should spawn a new search task with this query.
    /// Replaces any in-flight task (runner aborts the old
    /// `JoinHandle` before starting the new one).
    SpawnSearch {
        /// Query to dispatch.
        query: String,
    },
    /// Runner should abort any in-flight search task without
    /// starting a new one.
    CancelSearch,
    /// Runner should re-apply its similarity filter using this query.
    /// Synchronous — no task spawn.
    FilterSimilarity {
        /// Query to filter against.
        query: String,
    },
    /// Runner should fire a typed action built from wrapper state
    /// (e.g. `TypedAction::LoadSample(runner.selected_path())`).
    /// Placeholder because the selected path lives outside the FSM
    /// per the rust-fsm gotcha in `tui-fsm.md` §rust-fsm.
    FireSelection,
}

/// Unit marker type implementing [`StateMachineImpl`] for the search
/// FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMachine;

impl StateMachineImpl for SearchMachine {
    type Input = Input;
    type State = SearchFsmState;
    type Output = Output;
    const INITIAL_STATE: Self::State = SearchFsmState {
        transport: Transport::Idle,
        mode: Mode::Remote,
        query: String::new(),
        debounce_dirty: false,
    };

    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        let mut next = state.clone();
        match input {
            Input::QueryChanged(q) => {
                next.query = q.clone();
                next.debounce_dirty = true;
                // In Similarity mode the filter is synchronous; we
                // still set debounce_dirty so a DebounceTick fires
                // the filter. (Could also emit FilterSimilarity here
                // for immediate response — kept debounced to match
                // today's behavior and avoid flooding on fast typing.)
            }
            Input::QueryCleared => {
                next.query.clear();
                next.debounce_dirty = true;
            }
            Input::SubmitQuery => {
                // Reserved for future semantics; currently a no-op at
                // the FSM level. The runner may interpret a Submit as
                // "bypass debounce and dispatch immediately" later.
            }
            Input::DebounceTick => {
                if state.debounce_dirty {
                    next.debounce_dirty = false;
                    match state.mode {
                        Mode::Remote => {
                            // New query supersedes any in-flight
                            // search. Transport collapses to Pending.
                            // The Output arm emits SpawnSearch only —
                            // rust-fsm's output() returns a single
                            // Option<Output>, so the "cancel + spawn"
                            // pair is encoded in SpawnSearch's runner
                            // contract (see Output::SpawnSearch doc:
                            // the runner aborts any in-flight handle
                            // before starting the new one). No
                            // separate CancelSearch fires on tick.
                            next.transport = Transport::Pending;
                        }
                        Mode::Similarity => {
                            // Local filter — no transport change.
                        }
                    }
                }
            }
            Input::SearchStarted => {
                if matches!(state.transport, Transport::Pending) {
                    next.transport = Transport::Running;
                }
            }
            Input::SearchSettled { .. } => {
                if matches!(state.transport, Transport::Pending | Transport::Running) {
                    next.transport = Transport::Settled;
                }
            }
            Input::SearchCancelled => {
                // Any transport collapses to Settled on cancel. The
                // runner has already aborted the task; this just
                // marks the FSM as not waiting on anything.
                next.transport = Transport::Settled;
            }
            Input::SearchFailed => {
                // Same transport transition as SearchCancelled — the
                // task is no longer running. Kept as a separate
                // variant so logs / UI status distinguish "failed" from
                // "cancelled"; the runner that fires this also sets
                // total_matches = 0 on its own state.
                next.transport = Transport::Settled;
            }
            Input::FireSelection => {
                // Pure signal; output arm emits the effect.
            }
            Input::EnterSimilarityMode => {
                next.mode = Mode::Similarity;
                next.transport = Transport::Settled;
                next.debounce_dirty = false;
                // Output arm emits CancelSearch — any in-flight remote
                // search is moot now that we're filtering locally.
            }
            Input::ExitSimilarityMode => {
                // Guarded: only flip mode when we're actually in
                // Similarity. Without this, a defensive Task 4 call
                // from Remote mode would reset a live Settled/Running
                // transport to Idle and discard the result state.
                if matches!(state.mode, Mode::Similarity) {
                    next.mode = Mode::Remote;
                    next.transport = Transport::Idle;
                    // Next DebounceTick should fire a fresh remote
                    // search for the current query.
                    next.debounce_dirty = true;
                }
            }
        }
        Some(next)
    }

    fn output(state: &Self::State, input: &Self::Input) -> Option<Self::Output> {
        match input {
            Input::DebounceTick if state.debounce_dirty => match state.mode {
                Mode::Remote => Some(Output::SpawnSearch {
                    query: state.query.clone(),
                }),
                Mode::Similarity => Some(Output::FilterSimilarity {
                    query: state.query.clone(),
                }),
            },
            Input::EnterSimilarityMode => Some(Output::CancelSearch),
            Input::FireSelection => Some(Output::FireSelection),
            _ => None,
        }
    }
}

// ---------- App-facing wrapper ----------

/// App-facing wrapper around [`StateMachine<SearchMachine>`].
///
/// Holds the pure state; the runner (future Task 2) wraps this and
/// owns the tokio task handles, results list, similarity snapshot,
/// and columns/sort state.
#[allow(dead_code)] // Wiring lands in Task 2-4.
pub struct SearchFsm {
    machine: StateMachine<SearchMachine>,
}

#[allow(dead_code)] // Wiring lands in Task 2-4.
impl SearchFsm {
    /// Create a fresh machine at the initial state.
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
        }
    }

    /// Create a machine pre-seeded with the given state. Useful for
    /// tests and for stdio form-patch application (deserialize →
    /// from_state).
    pub fn from_state(state: SearchFsmState) -> Self {
        Self {
            machine: StateMachine::from_state(state),
        }
    }

    /// Apply an input. Returns the side-effect output, if any.
    pub fn consume(&mut self, input: Input) -> Option<Output> {
        self.machine.consume(&input).ok().flatten()
    }

    /// Current state (for persistence, test assertions, UI reads).
    pub fn state(&self) -> &SearchFsmState {
        self.machine.state()
    }

    // ---- Legacy accessors matching today's App field reads ----

    /// Matches `App::search_in_progress`.
    pub fn search_in_progress(&self) -> bool {
        matches!(
            self.state().transport,
            Transport::Pending | Transport::Running,
        )
    }

    /// Matches `App::search_pending` (new query dispatched, first
    /// batch not yet arrived).
    pub fn search_pending(&self) -> bool {
        matches!(self.state().transport, Transport::Pending)
    }

    /// Matches `App::in_similarity_mode`.
    pub fn in_similarity_mode(&self) -> bool {
        matches!(self.state().mode, Mode::Similarity)
    }

    /// Current query text.
    pub fn query(&self) -> &str {
        &self.state().query
    }
}

impl Default for SearchFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- baseline ----------

    #[test]
    fn initial_state_is_idle_remote_empty() {
        let fsm = SearchFsm::new();
        assert_eq!(fsm.state().transport, Transport::Idle);
        assert_eq!(fsm.state().mode, Mode::Remote);
        assert!(fsm.state().query.is_empty());
        assert!(!fsm.state().debounce_dirty);
        assert!(!fsm.search_in_progress());
        assert!(!fsm.search_pending());
        assert!(!fsm.in_similarity_mode());
    }

    // ---------- query + debounce ----------

    #[test]
    fn query_changed_sets_dirty_but_not_transport() {
        let mut fsm = SearchFsm::new();
        let out = fsm.consume(Input::QueryChanged("foo".to_string()));
        assert_eq!(fsm.query(), "foo");
        assert!(fsm.state().debounce_dirty);
        assert_eq!(
            fsm.state().transport,
            Transport::Idle,
            "no transport change until DebounceTick"
        );
        assert_eq!(out, None);
    }

    #[test]
    fn debounce_tick_remote_fires_spawn_and_enters_pending() {
        let mut fsm = SearchFsm::new();
        let _ = fsm.consume(Input::QueryChanged("bar".to_string()));
        let out = fsm.consume(Input::DebounceTick);
        assert_eq!(fsm.state().transport, Transport::Pending);
        assert!(!fsm.state().debounce_dirty, "dirty cleared on tick");
        assert_eq!(
            out,
            Some(Output::SpawnSearch {
                query: "bar".to_string()
            }),
        );
    }

    #[test]
    fn debounce_tick_similarity_fires_filter_no_transport_change() {
        let state = SearchFsmState {
            mode: Mode::Similarity,
            query: "baz".to_string(),
            debounce_dirty: true,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let out = fsm.consume(Input::DebounceTick);
        assert_eq!(
            fsm.state().transport,
            Transport::Idle,
            "similarity tick doesn't touch transport"
        );
        assert_eq!(
            out,
            Some(Output::FilterSimilarity {
                query: "baz".to_string()
            }),
        );
        assert!(!fsm.state().debounce_dirty);
    }

    #[test]
    fn debounce_tick_without_dirty_is_noop() {
        let mut fsm = SearchFsm::new();
        let before = fsm.state().clone();
        let out = fsm.consume(Input::DebounceTick);
        assert_eq!(fsm.state(), &before);
        assert_eq!(out, None);
    }

    #[test]
    fn query_cleared_sets_empty_and_dirty() {
        let mut fsm = SearchFsm::new();
        let _ = fsm.consume(Input::QueryChanged("abc".to_string()));
        let _ = fsm.consume(Input::QueryCleared);
        assert!(fsm.query().is_empty());
        assert!(fsm.state().debounce_dirty);
    }

    // ---------- transport lifecycle ----------

    #[test]
    fn search_started_pending_to_running() {
        let state = SearchFsmState {
            transport: Transport::Pending,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let _ = fsm.consume(Input::SearchStarted);
        assert_eq!(fsm.state().transport, Transport::Running);
    }

    #[test]
    fn search_started_from_idle_is_noop() {
        // Only Pending → Running; other transports unchanged.
        let mut fsm = SearchFsm::new();
        let _ = fsm.consume(Input::SearchStarted);
        assert_eq!(fsm.state().transport, Transport::Idle);
    }

    #[test]
    fn search_settled_from_running() {
        let state = SearchFsmState {
            transport: Transport::Running,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let _ = fsm.consume(Input::SearchSettled { total: 42 });
        assert_eq!(fsm.state().transport, Transport::Settled);
    }

    #[test]
    fn search_settled_from_pending_shortcircuits() {
        // If a search cancelled before any batch arrived, Pending
        // can settle directly.
        let state = SearchFsmState {
            transport: Transport::Pending,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let _ = fsm.consume(Input::SearchSettled { total: 0 });
        assert_eq!(fsm.state().transport, Transport::Settled);
    }

    #[test]
    fn search_failed_lands_settled_like_cancel() {
        let state = SearchFsmState {
            transport: Transport::Pending,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let _ = fsm.consume(Input::SearchFailed);
        assert_eq!(fsm.state().transport, Transport::Settled);
    }

    #[test]
    fn search_cancelled_always_lands_settled() {
        for initial in [
            Transport::Idle,
            Transport::Pending,
            Transport::Running,
            Transport::Settled,
        ] {
            let state = SearchFsmState {
                transport: initial,
                ..SearchFsmState::default()
            };
            let mut fsm = SearchFsm::from_state(state);
            let _ = fsm.consume(Input::SearchCancelled);
            assert_eq!(
                fsm.state().transport,
                Transport::Settled,
                "from {:?}",
                initial
            );
        }
    }

    #[test]
    fn search_failed_always_lands_settled() {
        // Symmetry check with search_cancelled_always_lands_settled —
        // SearchFailed must collapse to Settled from every transport
        // so error paths can never orphan Pending / Running. Fills
        // the Tier 1 F3 coverage gap (the inline spot check only
        // exercised from Pending).
        for initial in [
            Transport::Idle,
            Transport::Pending,
            Transport::Running,
            Transport::Settled,
        ] {
            let state = SearchFsmState {
                transport: initial,
                ..SearchFsmState::default()
            };
            let mut fsm = SearchFsm::from_state(state);
            let _ = fsm.consume(Input::SearchFailed);
            assert_eq!(
                fsm.state().transport,
                Transport::Settled,
                "from {:?}",
                initial
            );
        }
    }

    // ---------- similarity mode ----------

    #[test]
    fn enter_similarity_mode_cancels_in_flight() {
        // PR #14 regression: entering similarity mode while a remote
        // search is running must cancel that search.
        let state = SearchFsmState {
            transport: Transport::Running,
            mode: Mode::Remote,
            query: "old".to_string(),
            debounce_dirty: false,
        };
        let mut fsm = SearchFsm::from_state(state);
        let out = fsm.consume(Input::EnterSimilarityMode);
        assert_eq!(fsm.state().mode, Mode::Similarity);
        assert_eq!(fsm.state().transport, Transport::Settled);
        assert_eq!(out, Some(Output::CancelSearch));
    }

    #[test]
    fn exit_similarity_mode_queues_fresh_search() {
        let state = SearchFsmState {
            mode: Mode::Similarity,
            query: "foo".to_string(),
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let _ = fsm.consume(Input::ExitSimilarityMode);
        assert_eq!(fsm.state().mode, Mode::Remote);
        assert_eq!(fsm.state().transport, Transport::Idle);
        assert!(
            fsm.state().debounce_dirty,
            "exit similarity must queue a fresh debounce",
        );

        // The queued debounce now fires a Remote SpawnSearch.
        let out = fsm.consume(Input::DebounceTick);
        assert_eq!(fsm.state().transport, Transport::Pending);
        assert_eq!(
            out,
            Some(Output::SpawnSearch {
                query: "foo".to_string()
            })
        );
    }

    #[test]
    fn exit_similarity_from_remote_is_noop() {
        // Regression for Tier 1 review M1: ExitSimilarityMode must not
        // reset transport when we're already in Remote mode. A defensive
        // runner call from Remote must not clobber a live Settled or
        // Running transport.
        let state = SearchFsmState {
            transport: Transport::Settled,
            mode: Mode::Remote,
            query: "q".to_string(),
            debounce_dirty: false,
        };
        let before = state.clone();
        let mut fsm = SearchFsm::from_state(state);
        let out = fsm.consume(Input::ExitSimilarityMode);
        assert_eq!(
            fsm.state(),
            &before,
            "ExitSimilarityMode from Remote must be a no-op",
        );
        assert_eq!(out, None);
    }

    #[test]
    fn similarity_mode_never_emits_spawn_search() {
        // Property R3 as an inline spot check: while in Similarity,
        // any DebounceTick that does emit must be FilterSimilarity,
        // never SpawnSearch.
        let state = SearchFsmState {
            mode: Mode::Similarity,
            query: "q".to_string(),
            debounce_dirty: true,
            ..SearchFsmState::default()
        };
        let mut fsm = SearchFsm::from_state(state);
        let out = fsm.consume(Input::DebounceTick);
        assert!(
            matches!(out, Some(Output::FilterSimilarity { .. })),
            "expected FilterSimilarity, got {:?}",
            out,
        );
    }

    // ---------- FireSelection ----------

    #[test]
    fn fire_selection_emits_effect_no_state_change() {
        let state = SearchFsmState {
            transport: Transport::Settled,
            query: "q".to_string(),
            ..SearchFsmState::default()
        };
        let before = state.clone();
        let mut fsm = SearchFsm::from_state(state);
        let out = fsm.consume(Input::FireSelection);
        assert_eq!(fsm.state(), &before, "FireSelection must not change state");
        assert_eq!(out, Some(Output::FireSelection));
    }

    // ---------- Serde round-trip (R6) ----------

    #[test]
    fn state_round_trips_through_json() {
        // stdio's form-patch protocol is serde-based; the state must
        // survive a round-trip without loss.
        let state = SearchFsmState {
            transport: Transport::Running,
            mode: Mode::Similarity,
            query: "hello world".to_string(),
            debounce_dirty: true,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let decoded: SearchFsmState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, state);
    }

    #[test]
    fn input_and_output_serde_round_trip() {
        let inputs = vec![
            Input::QueryChanged("foo".to_string()),
            Input::QueryCleared,
            Input::SubmitQuery,
            Input::DebounceTick,
            Input::SearchStarted,
            Input::SearchSettled { total: 100 },
            Input::SearchCancelled,
            Input::SearchFailed,
            Input::FireSelection,
            Input::EnterSimilarityMode,
            Input::ExitSimilarityMode,
        ];
        for input in inputs {
            let j = serde_json::to_string(&input).expect("ser input");
            let back: Input = serde_json::from_str(&j).expect("de input");
            assert_eq!(back, input);
        }

        let outputs = vec![
            Output::SpawnSearch {
                query: "q".to_string(),
            },
            Output::CancelSearch,
            Output::FilterSimilarity {
                query: "q".to_string(),
            },
            Output::FireSelection,
        ];
        for output in outputs {
            let j = serde_json::to_string(&output).expect("ser output");
            let back: Output = serde_json::from_str(&j).expect("de output");
            assert_eq!(back, output);
        }
    }
}
