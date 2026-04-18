//! Action generators for the marker-FSM property suite.
//!
//! Each generator is parameterised so individual properties can exclude
//! actions that would invalidate the invariant under test. For example,
//! the bank-sync-preservation property (P6) calls [`transitions_no_sync_toggle`]
//! so the generated stream never contains `ToggleBankSync`.
//!
//! Generators take the current reference state so future properties can
//! make transition choice state-dependent (e.g. "only generate nudge
//! inputs when a marker is selected"). For the base generator we keep
//! transitions state-independent so shrinking isn't complicated.

use std::path::PathBuf;

use proptest::prelude::*;

use riffgrep::engine::marker_fsm::{Input, MarkerFsmState};

/// Upper bound for generated marker positions. Below `MAX_MARKER_POS`
/// so proptest can exercise the saturation path without hitting the
/// `MARKER_EMPTY` sentinel.
const MAX_GEN_POS: u32 = 1_000_000;

/// Upper bound for nudge deltas. Stays small enough that proptest
/// generates enough cases in the "non-saturating" regime.
const MAX_GEN_DELTA: u32 = 10_000;

/// Base generator: any input, no state-dependent filtering.
pub fn any_input(_state: &MarkerFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker1),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker2),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker3),
        (0u32..MAX_GEN_POS).prop_map(Input::SetSelectedMarker),
        Just(Input::SelectNextMarker),
        Just(Input::SelectPrevMarker),
        Just(Input::ToggleBank),
        Just(Input::ToggleBankSync),
        (0u32..MAX_GEN_POS).prop_map(Input::ClearNearestMarker),
        Just(Input::ClearBankMarkers),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeForward),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeBackward),
        marker_reset_strategy(),
        Just(Input::IncrementRep),
        Just(Input::DecrementRep),
        Just(Input::ToggleMarkerDisplay),
        csv_export_strategy(),
        csv_import_strategy(),
    ]
    .boxed()
}

/// Like [`any_input`] but never generates [`Input::ToggleBankSync`]. Used
/// by property P6 (bank-sync preservation).
#[allow(dead_code)] // Wired up in Task 6 (P6).
pub fn transitions_no_sync_toggle(_state: &MarkerFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker1),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker2),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker3),
        (0u32..MAX_GEN_POS).prop_map(Input::SetSelectedMarker),
        Just(Input::SelectNextMarker),
        Just(Input::SelectPrevMarker),
        Just(Input::ToggleBank),
        (0u32..MAX_GEN_POS).prop_map(Input::ClearNearestMarker),
        Just(Input::ClearBankMarkers),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeForward),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeBackward),
        marker_reset_strategy(),
        Just(Input::IncrementRep),
        Just(Input::DecrementRep),
        Just(Input::ToggleMarkerDisplay),
    ]
    .boxed()
}

/// Like [`any_input`] but never generates [`Input::ToggleMarkerDisplay`].
/// Used by property P7 (markers-disabled fixed point) once the display
/// is flipped off — the generator runs until the property loop re-enables
/// display.
#[allow(dead_code)] // Wired up in Task 6 (P7).
pub fn transitions_no_display_toggle(_state: &MarkerFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker1),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker2),
        (0u32..MAX_GEN_POS).prop_map(Input::SetMarker3),
        (0u32..MAX_GEN_POS).prop_map(Input::SetSelectedMarker),
        Just(Input::SelectNextMarker),
        Just(Input::SelectPrevMarker),
        Just(Input::ToggleBank),
        Just(Input::ToggleBankSync),
        (0u32..MAX_GEN_POS).prop_map(Input::ClearNearestMarker),
        Just(Input::ClearBankMarkers),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeForward),
        (1u32..MAX_GEN_DELTA).prop_map(Input::NudgeBackward),
        marker_reset_strategy(),
        Just(Input::IncrementRep),
        Just(Input::DecrementRep),
    ]
    .boxed()
}

fn marker_reset_strategy() -> BoxedStrategy<Input> {
    (0u32..MAX_GEN_POS, 0u32..MAX_GEN_POS)
        .prop_map(|(a, b)| {
            let (sof, eof) = if a <= b { (a, b) } else { (b, a) };
            Input::MarkerReset { sof, eof }
        })
        .boxed()
}

fn csv_export_strategy() -> BoxedStrategy<Input> {
    "[a-z]{1,8}"
        .prop_map(|name| Input::ExportMarkersCsv(PathBuf::from(format!("/tmp/{name}.csv"))))
        .boxed()
}

fn csv_import_strategy() -> BoxedStrategy<Input> {
    "[a-z]{1,8}"
        .prop_map(|name| Input::ImportMarkersCsv(PathBuf::from(format!("/tmp/{name}.csv"))))
        .boxed()
}
