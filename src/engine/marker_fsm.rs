//! Markers finite state machine.
//!
//! Formalizes the marker selection, bank, sync, and visibility state
//! that previously lived as ad-hoc fields on `App`. Implements
//! [`rust_fsm::StateMachineImpl`] so every transition is explicit and
//! the machine is amenable to property-based testing via
//! `proptest-state-machine`.
//!
//! See `doc/designs/debt-fsm.md` for the invariant roadmap and
//! `doc/plans/plan-2026-04-18-02.md` for this sprint's scope.
//!
//! **Status:** scaffold. All transitions are identity (`Some(*state)`)
//! until Task 3 fills in the real transition table.

// Removed in Task 4 once `App` consumes these types at every call-site.
#![allow(dead_code)]

use std::path::PathBuf;

use rust_fsm::{StateMachine, StateMachineImpl};

use crate::engine::bext::{MARKER_EMPTY, MarkerBank, MarkerConfig};

/// Which marker slot is currently selected for nudge/snap/rep operations.
///
/// Maps onto the legacy `App.selected_marker: Option<usize>`:
/// - `None`        ↔ `None`
/// - `Some(0)`     ↔ `Some(Selection::Sof)`
/// - `Some(1..=3)` ↔ `Some(Selection::M1..=M3)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Selection {
    /// Start-of-file pseudo-marker (sample 0). Always defined.
    Sof,
    /// Marker 1 (`m1`).
    M1,
    /// Marker 2 (`m2`).
    M2,
    /// Marker 3 (`m3`).
    M3,
}

/// Which marker bank is the active edit target.
///
/// Mirrors [`crate::ui::Bank`]. The two definitions will be unified
/// when the App-integration pass lands (Task 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Bank {
    /// Bank A (top half of the waveform).
    #[default]
    A,
    /// Bank B (bottom half of the waveform).
    B,
}

/// Full marker-bank FSM state.
///
/// Bundles every coordinate that used to be a bare field on `App`
/// (`selected_marker`, `active_bank`, `bank_sync`, `markers_visible`) with
/// the underlying [`MarkerConfig`] data. All fields are `Copy`, so
/// identity transitions are a cheap `*state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerFsmState {
    /// Currently selected marker (`None` when no marker is selected).
    pub selection: Option<Selection>,
    /// Active bank: edits target this bank (both banks if `bank_sync` is on).
    pub active_bank: Bank,
    /// When true, edits to one bank mirror to the other.
    pub bank_sync: bool,
    /// When false, all edit inputs become no-ops.
    pub visible: bool,
    /// Marker values for both banks.
    pub config: MarkerConfig,
}

impl Default for MarkerFsmState {
    fn default() -> Self {
        INITIAL_STATE
    }
}

/// Const-constructible initial state matching the historical App defaults
/// (`selected_marker = None`, `active_bank = A`, `bank_sync = true`,
/// `markers_visible = true`, both banks empty).
const INITIAL_STATE: MarkerFsmState = MarkerFsmState {
    selection: None,
    active_bank: Bank::A,
    bank_sync: true,
    visible: true,
    config: MarkerConfig {
        bank_a: EMPTY_BANK,
        bank_b: EMPTY_BANK,
    },
};

const EMPTY_BANK: MarkerBank = MarkerBank {
    m1: MARKER_EMPTY,
    m2: MARKER_EMPTY,
    m3: MARKER_EMPTY,
    reps: [0; 4],
};

/// All inputs (user actions) that drive marker-FSM transitions.
///
/// Variant set matches the 23-action inventory from `debt-fsm.md`. Data
/// payloads carry the context that transitions need (cursor position,
/// SOF/EOF bounds, CSV paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Set marker 1 to the given absolute sample position.
    SetMarker1(u32),
    /// Set marker 2 to the given absolute sample position.
    SetMarker2(u32),
    /// Set marker 3 to the given absolute sample position.
    SetMarker3(u32),
    /// Flip `active_bank` (A ↔ B).
    ToggleBank,
    /// Flip `bank_sync`.
    ToggleBankSync,
    /// Advance selection forward (SOF → M1 → M2 → M3; stops at M3).
    SelectNextMarker,
    /// Advance selection backward (M3 → M2 → M1 → SOF; stops at SOF).
    SelectPrevMarker,
    /// Clear the marker nearest to the given cursor position.
    ClearNearestMarker(u32),
    /// Clear all markers in the active bank (both banks when `bank_sync`).
    ClearBankMarkers,
    /// Nudge selected marker forward by `marker_nudge_small` zero-crossings.
    NudgeForwardSmall,
    /// Nudge selected marker forward by `marker_nudge_large` zero-crossings.
    NudgeForwardLarge,
    /// Nudge selected marker backward by `marker_nudge_small` zero-crossings.
    NudgeBackwardSmall,
    /// Nudge selected marker backward by `marker_nudge_large` zero-crossings.
    NudgeBackwardLarge,
    /// Snap selected marker to the next zero-crossing forward.
    SnapZeroCrossingForward,
    /// Snap selected marker to the next zero-crossing backward.
    SnapZeroCrossingBackward,
    /// Reset markers to the 25/50/75 % layout between `sof` and `eof`.
    MarkerReset {
        /// Start-of-file sample boundary (almost always 0).
        sof: u32,
        /// End-of-file sample boundary (total sample count).
        eof: u32,
    },
    /// Increment the selected segment's repetition nibble (clamped at 15).
    IncrementRep,
    /// Decrement the selected segment's repetition nibble (clamped at 0).
    DecrementRep,
    /// Flip `markers_visible`.
    ToggleMarkerDisplay,
    /// Export the current `MarkerConfig` to a CSV file.
    ExportMarkersCsv(PathBuf),
    /// Import a `MarkerConfig` from a CSV file.
    ImportMarkersCsv(PathBuf),
}

/// Observable side-effect descriptors produced by transitions.
///
/// Outputs describe what happened without running I/O, so tests can
/// assert on them. Real I/O (CSV read/write, audio seeks) is performed
/// by the caller after inspecting the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// No state change occurred (input was a no-op in the current state).
    Noop,
    /// A marker or rep value was edited.
    DidEdit,
    /// The selection changed to the given target.
    DidSelect(Option<Selection>),
    /// The active bank was toggled.
    DidToggleBank,
    /// `bank_sync` was flipped.
    DidToggleBankSync,
    /// `markers_visible` was flipped.
    DidToggleDisplay,
    /// Caller should write the current config to the given path.
    WriteCsv(PathBuf),
    /// Caller should read a config from the given path.
    ReadCsv(PathBuf),
}

/// Unit marker type implementing [`StateMachineImpl`] for the marker FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerBankMachine;

impl StateMachineImpl for MarkerBankMachine {
    type Input = Input;
    type State = MarkerFsmState;
    type Output = Output;
    const INITIAL_STATE: Self::State = INITIAL_STATE;

    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        // Scaffold: every transition is identity. Task 3 replaces this
        // with the real transition table.
        let _ = input;
        Some(*state)
    }

    fn output(_state: &Self::State, _input: &Self::Input) -> Option<Self::Output> {
        // Scaffold: every transition emits `Noop`. Task 3 refines outputs
        // per transition.
        Some(Output::Noop)
    }
}

/// App-facing wrapper around [`StateMachine<MarkerBankMachine>`].
///
/// Exposes legacy-shaped accessors (`selected_marker() -> Option<usize>`,
/// `active_bank() -> Bank`, …) so the App-integration pass (Task 4) can
/// swap out the four bare fields in-place without churning every read
/// site.
pub struct MarkerFsm {
    machine: StateMachine<MarkerBankMachine>,
}

impl MarkerFsm {
    /// Create a fresh machine at `INITIAL_STATE`.
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
        }
    }

    /// Apply an input; returns the emitted output. `None` only if the
    /// scaffold transition fails (never expected once Task 3 lands).
    pub fn consume(&mut self, input: Input) -> Option<Output> {
        self.machine.consume(&input).ok().flatten()
    }

    /// Current FSM state.
    pub fn state(&self) -> &MarkerFsmState {
        self.machine.state()
    }

    /// Legacy view matching `App.selected_marker: Option<usize>`.
    pub fn selected_marker(&self) -> Option<usize> {
        self.state().selection.map(|s| match s {
            Selection::Sof => 0,
            Selection::M1 => 1,
            Selection::M2 => 2,
            Selection::M3 => 3,
        })
    }

    /// Legacy view matching `App.active_bank: Bank`.
    pub fn active_bank(&self) -> Bank {
        self.state().active_bank
    }

    /// Legacy view matching `App.bank_sync: bool`.
    pub fn bank_sync(&self) -> bool {
        self.state().bank_sync
    }

    /// Legacy view matching `App.markers_visible: bool`.
    pub fn markers_visible(&self) -> bool {
        self.state().visible
    }

    /// Read-only access to both banks' marker data.
    pub fn config(&self) -> &MarkerConfig {
        &self.state().config
    }
}

impl Default for MarkerFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_matches_legacy_app_defaults() {
        let fsm = MarkerFsm::new();
        assert_eq!(fsm.selected_marker(), None);
        assert_eq!(fsm.active_bank(), Bank::A);
        assert!(fsm.bank_sync());
        assert!(fsm.markers_visible());
        assert!(fsm.config().bank_a.is_empty());
        assert!(fsm.config().bank_b.is_empty());
    }

    #[test]
    fn scaffold_transitions_are_identity() {
        // Until Task 3 fills in real transitions, every consume() leaves
        // state unchanged. This test exists so that Task 3 *must* update
        // it — a red test will flag that transition logic is now real.
        let mut fsm = MarkerFsm::new();
        let before = *fsm.state();

        for input in [
            Input::SetMarker1(100),
            Input::ToggleBank,
            Input::ToggleBankSync,
            Input::SelectNextMarker,
            Input::MarkerReset {
                sof: 0,
                eof: 44100,
            },
            Input::ToggleMarkerDisplay,
        ] {
            let _ = fsm.consume(input);
            assert_eq!(
                *fsm.state(),
                before,
                "scaffold transition should be identity",
            );
        }
    }

    #[test]
    fn selection_roundtrip_to_legacy_usize() {
        // The legacy `App.selected_marker: Option<usize>` view must stay
        // byte-compatible once we flip the state; encode that here so we
        // don't silently reindex in Task 3.
        let cases = [
            (None, None),
            (Some(Selection::Sof), Some(0usize)),
            (Some(Selection::M1), Some(1)),
            (Some(Selection::M2), Some(2)),
            (Some(Selection::M3), Some(3)),
        ];
        for (selection, expected) in cases {
            let state = MarkerFsmState {
                selection,
                ..MarkerFsmState::default()
            };
            let fsm = MarkerFsm {
                machine: StateMachine::from_state(state),
            };
            assert_eq!(fsm.selected_marker(), expected);
        }
    }
}
