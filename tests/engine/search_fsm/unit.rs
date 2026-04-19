//! Unit-level regressions for the search FSM.
//!
//! Today this file is a placeholder. The inline `#[cfg(test)]` block
//! in `src/engine/search_fsm.rs` covers every transition path + the
//! PR #14 `EnterSimilarityMode → CancelSearch` regression, and
//! `src/engine/search_runner.rs` covers the runner-side data layer
//! (set_results / filter / TypedAction synthesis). Plan 08 Task 4
//! (App integration) will populate this file with App-level
//! regressions for the migrated call-sites.
