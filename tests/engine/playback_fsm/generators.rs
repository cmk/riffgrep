//! Action generators for the playback-FSM property suite.
//!
//! Each generator is parameterised so individual properties can exclude
//! actions that would invalidate the invariant under test. For example,
//! Q6 (loop forces restart on ProgramEnded) holds only when
//! `loop_enabled` stays true throughout the stream, so the generator
//! drops `ToggleLoop` / `SetLoop` while that property is under test.

use proptest::prelude::*;

use riffgrep::engine::playback_fsm::{Input, PlaybackFsmState};

/// Upper bound for generated seek frame targets. Below u32::MAX so
/// proptest spends its cases in realistic ranges, not at the sentinel.
const MAX_GEN_FRAME: u32 = 10_000_000;

/// Base generator: any input, no state-dependent filtering.
pub fn any_input(_state: &PlaybackFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        Just(Input::Play),
        Just(Input::Pause),
        Just(Input::Resume),
        Just(Input::Stop),
        (0u32..MAX_GEN_FRAME).prop_map(Input::Seek),
        Just(Input::Restart),
        Just(Input::ToggleReverse),
        any::<bool>().prop_map(Input::SetReverse),
        Just(Input::ToggleLoop),
        any::<bool>().prop_map(Input::SetLoop),
        Just(Input::SegmentEnded),
        Just(Input::ProgramEnded),
        Just(Input::ConsumeSeek),
        Just(Input::ConsumeRestart),
    ]
    .boxed()
}

/// Like [`any_input`] but never emits `Stop` or `ProgramEnded`. Used
/// by Q5 (`Seek` → `ConsumeSeek` drains) so the stream can't clear
/// `pending_seek` through a side channel.
#[allow(dead_code)] // Referenced by prop.rs; false-positive until that lands.
pub fn transitions_no_stop_or_program_end(_state: &PlaybackFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        Just(Input::Play),
        Just(Input::Pause),
        Just(Input::Resume),
        (0u32..MAX_GEN_FRAME).prop_map(Input::Seek),
        Just(Input::Restart),
        Just(Input::ToggleReverse),
        any::<bool>().prop_map(Input::SetReverse),
        Just(Input::ToggleLoop),
        any::<bool>().prop_map(Input::SetLoop),
        Just(Input::SegmentEnded),
    ]
    .boxed()
}

/// Like [`any_input`] but never flips the loop toggle. Used by Q6
/// so the initial `loop_enabled=true` persists through the stream.
#[allow(dead_code)] // Referenced by prop.rs; false-positive until that lands.
pub fn transitions_no_loop_toggle(_state: &PlaybackFsmState) -> BoxedStrategy<Input> {
    prop_oneof![
        Just(Input::Play),
        Just(Input::Pause),
        Just(Input::Resume),
        Just(Input::Stop),
        (0u32..MAX_GEN_FRAME).prop_map(Input::Seek),
        Just(Input::Restart),
        Just(Input::ToggleReverse),
        any::<bool>().prop_map(Input::SetReverse),
        Just(Input::SegmentEnded),
        Just(Input::ProgramEnded),
        Just(Input::ConsumeSeek),
        Just(Input::ConsumeRestart),
    ]
    .boxed()
}
