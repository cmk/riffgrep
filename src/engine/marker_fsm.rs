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

// The non-`Bank` items (Selection, MarkerFsm, Input, Output,
// MarkerFsmState, MarkerBankMachine) are exercised by the property
// suite but not yet consumed from `App`. The module-level allow comes
// off when App is carved out to dispatch through MarkerFsm. See
// doc/designs/debt-fsm.md and plan-2026-04-18-02.md.
#![allow(dead_code)]

use std::path::PathBuf;

use rust_fsm::{StateMachine, StateMachineImpl};

use crate::engine::bext::{MARKER_EMPTY, MarkerBank, MarkerConfig};

/// Maximum legal marker sample value. One below [`MARKER_EMPTY`] so
/// saturating arithmetic can't accidentally collide with the sentinel.
pub const MAX_MARKER_POS: u32 = MARKER_EMPTY - 1;

/// Maximum repetition nibble value (15 = infinite).
pub const REP_MAX: u8 = 15;

/// Which marker slot is currently selected for nudge/snap/rep operations.
///
/// Maps onto the legacy `App.selected_marker: Option<usize>`:
/// `None ↔ None`, `Some(0) ↔ Some(Sof)`, `Some(1..=3) ↔ Some(M1..=M3)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Selection {
    /// Start-of-file pseudo-marker (sample 0). Always defined, never mutable.
    Sof,
    /// Marker 1 (`m1`).
    M1,
    /// Marker 2 (`m2`).
    M2,
    /// Marker 3 (`m3`).
    M3,
}

impl Selection {
    /// Map `Selection` to the legacy 0..=3 usize representation used by `App`.
    pub fn as_index(self) -> usize {
        match self {
            Selection::Sof => 0,
            Selection::M1 => 1,
            Selection::M2 => 2,
            Selection::M3 => 3,
        }
    }

    /// Inverse of [`Self::as_index`]. Returns `None` for out-of-range indices.
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Selection::Sof),
            1 => Some(Selection::M1),
            2 => Some(Selection::M2),
            3 => Some(Selection::M3),
            _ => None,
        }
    }
}

/// Which marker bank is the active edit target.
///
/// Re-exported by `crate::ui::Bank`; this is the canonical
/// definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Bank {
    /// Bank A (top half of the waveform).
    #[default]
    A,
    /// Bank B (bottom half of the waveform).
    B,
}

impl Bank {
    /// Return the opposite bank.
    pub fn flip(self) -> Self {
        match self {
            Bank::A => Bank::B,
            Bank::B => Bank::A,
        }
    }
}

/// Full marker-bank FSM state.
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

const EMPTY_BANK: MarkerBank = MarkerBank {
    m1: MARKER_EMPTY,
    m2: MARKER_EMPTY,
    m3: MARKER_EMPTY,
    reps: [0; 4],
};

/// Const-constructible initial state matching the historical App defaults.
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

/// All inputs that drive marker-FSM transitions.
///
/// Transitions that need audio-domain data (zero-crossing search for nudge,
/// file total for reset bounds) take that data as input payload rather
/// than holding it in state — keeps the FSM pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Set marker 1 to the given absolute sample position.
    SetMarker1(u32),
    /// Set marker 2 to the given absolute sample position.
    SetMarker2(u32),
    /// Set marker 3 to the given absolute sample position.
    SetMarker3(u32),
    /// Set the currently-selected marker to the given position. No-op when
    /// no marker is selected or when selection is [`Selection::Sof`] (SOF
    /// is a pseudo-marker at sample 0 and cannot be moved).
    SetSelectedMarker(u32),
    /// Advance selection forward through the defined markers (wraps).
    SelectNextMarker,
    /// Advance selection backward through the defined markers (wraps).
    SelectPrevMarker,
    /// Flip `active_bank` (A ↔ B).
    ToggleBank,
    /// Flip `bank_sync`.
    ToggleBankSync,
    /// Clear the marker whose value is closest to the given cursor position.
    ClearNearestMarker(u32),
    /// Clear all markers in the active bank (both banks when `bank_sync`).
    ClearBankMarkers,
    /// Nudge the selected marker forward by `delta` samples
    /// ([`u32::saturating_add`], capped at [`MAX_MARKER_POS`]).
    NudgeForward(u32),
    /// Nudge the selected marker backward by `delta` samples
    /// ([`u32::saturating_sub`], floored at 0).
    NudgeBackward(u32),
    /// Reset bank markers to the 25/50/75 % layout between `sof` and `eof`.
    MarkerReset {
        /// Start-of-file boundary (usually 0).
        sof: u32,
        /// End-of-file boundary (total sample count).
        eof: u32,
    },
    /// Increment the rep nibble for the segment indexed by the current
    /// selection (clamped at [`REP_MAX`]).
    IncrementRep,
    /// Decrement the rep nibble for the segment indexed by the current
    /// selection (clamped at 0).
    DecrementRep,
    /// Flip `markers_visible`.
    ToggleMarkerDisplay,
    /// Emit a [`Output::WriteCsv`] descriptor; state unchanged.
    ExportMarkersCsv(PathBuf),
    /// Emit a [`Output::ReadCsv`] descriptor; the caller performs the
    /// read and re-injects data via `SetMarkerN` inputs.
    ImportMarkersCsv(PathBuf),
}

impl Input {
    /// Whether this input *could* mutate marker/rep data. Used to gate
    /// transitions under `visible = false`.
    fn is_edit(&self) -> bool {
        matches!(
            self,
            Input::SetMarker1(_)
                | Input::SetMarker2(_)
                | Input::SetMarker3(_)
                | Input::SetSelectedMarker(_)
                | Input::ClearNearestMarker(_)
                | Input::ClearBankMarkers
                | Input::NudgeForward(_)
                | Input::NudgeBackward(_)
                | Input::MarkerReset { .. }
                | Input::IncrementRep
                | Input::DecrementRep
        )
    }
}

/// Observable outputs: I/O descriptors that the caller must act on.
///
/// Pure state transitions (selection, bank flips, edits) return `None`
/// from [`StateMachineImpl::output`]; the caller inspects `state()`
/// after `consume()` to see what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Caller should write the current config to this path.
    WriteCsv(PathBuf),
    /// Caller should read a config from this path; subsequent
    /// `SetMarkerN` inputs merge data back in.
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
        // Visibility gate: editing inputs are no-ops when markers aren't shown.
        if !state.visible && input.is_edit() {
            return Some(*state);
        }

        let mut next = *state;
        match input {
            Input::SetMarker1(pos) => write_slot(&mut next, 1, *pos),
            Input::SetMarker2(pos) => write_slot(&mut next, 2, *pos),
            Input::SetMarker3(pos) => write_slot(&mut next, 3, *pos),
            Input::SetSelectedMarker(pos) => match next.selection {
                Some(Selection::M1) => write_slot(&mut next, 1, *pos),
                Some(Selection::M2) => write_slot(&mut next, 2, *pos),
                Some(Selection::M3) => write_slot(&mut next, 3, *pos),
                Some(Selection::Sof) | None => {
                    // SOF is immovable; empty selection is a no-op.
                }
            },
            Input::SelectNextMarker => {
                next.selection = cycle_selection(&next, CycleDirection::Next);
            }
            Input::SelectPrevMarker => {
                next.selection = cycle_selection(&next, CycleDirection::Prev);
            }
            Input::ToggleBank => {
                next.active_bank = next.active_bank.flip();
            }
            Input::ToggleBankSync => {
                next.bank_sync = !next.bank_sync;
            }
            Input::ClearNearestMarker(cursor) => {
                if let Some(slot) = nearest_defined_slot(active_ref(&next), *cursor) {
                    write_slot(&mut next, slot, MARKER_EMPTY);
                }
            }
            Input::ClearBankMarkers => {
                clear_active(&mut next);
            }
            Input::NudgeForward(delta) => {
                nudge_selected(&mut next, *delta, true);
            }
            Input::NudgeBackward(delta) => {
                nudge_selected(&mut next, *delta, false);
            }
            Input::MarkerReset { sof, eof } => {
                reset_to_quartiles(&mut next, *sof, *eof);
            }
            Input::IncrementRep => adjust_rep(&mut next, 1),
            Input::DecrementRep => adjust_rep(&mut next, -1),
            Input::ToggleMarkerDisplay => {
                next.visible = !next.visible;
            }
            Input::ExportMarkersCsv(_) | Input::ImportMarkersCsv(_) => {
                // State-preserving; output() emits the I/O descriptor.
            }
        }
        Some(next)
    }

    fn output(_state: &Self::State, input: &Self::Input) -> Option<Self::Output> {
        match input {
            Input::ExportMarkersCsv(p) => Some(Output::WriteCsv(p.clone())),
            Input::ImportMarkersCsv(p) => Some(Output::ReadCsv(p.clone())),
            _ => None,
        }
    }
}

// ---------- transition helpers ----------

fn active_ref(state: &MarkerFsmState) -> &MarkerBank {
    match state.active_bank {
        Bank::A => &state.config.bank_a,
        Bank::B => &state.config.bank_b,
    }
}

fn write_slot(state: &mut MarkerFsmState, slot: u8, pos: u32) {
    let apply = |bank: &mut MarkerBank| match slot {
        1 => bank.m1 = pos,
        2 => bank.m2 = pos,
        3 => bank.m3 = pos,
        _ => unreachable!("slot must be 1..=3, got {slot}"),
    };
    if state.bank_sync {
        apply(&mut state.config.bank_a);
        apply(&mut state.config.bank_b);
    } else {
        match state.active_bank {
            Bank::A => apply(&mut state.config.bank_a),
            Bank::B => apply(&mut state.config.bank_b),
        }
    }
}

fn clear_active(state: &mut MarkerFsmState) {
    let clear = |bank: &mut MarkerBank| {
        bank.m1 = MARKER_EMPTY;
        bank.m2 = MARKER_EMPTY;
        bank.m3 = MARKER_EMPTY;
        // Reps are intentionally preserved: they're segment-level metadata,
        // not marker-level, and clearing markers alone shouldn't nuke them.
    };
    if state.bank_sync {
        clear(&mut state.config.bank_a);
        clear(&mut state.config.bank_b);
    } else {
        match state.active_bank {
            Bank::A => clear(&mut state.config.bank_a),
            Bank::B => clear(&mut state.config.bank_b),
        }
    }
}

fn nearest_defined_slot(bank: &MarkerBank, cursor: u32) -> Option<u8> {
    [(1u8, bank.m1), (2, bank.m2), (3, bank.m3)]
        .into_iter()
        .filter(|(_, v)| *v != MARKER_EMPTY)
        .min_by_key(|(_, v)| v.abs_diff(cursor))
        .map(|(s, _)| s)
}

fn selected_slot(state: &MarkerFsmState) -> Option<u8> {
    match state.selection {
        Some(Selection::M1) => Some(1),
        Some(Selection::M2) => Some(2),
        Some(Selection::M3) => Some(3),
        Some(Selection::Sof) | None => None,
    }
}

fn read_slot(bank: &MarkerBank, slot: u8) -> u32 {
    match slot {
        1 => bank.m1,
        2 => bank.m2,
        3 => bank.m3,
        _ => unreachable!(),
    }
}

fn nudge_selected(state: &mut MarkerFsmState, delta: u32, forward: bool) {
    let slot = match selected_slot(state) {
        Some(s) => s,
        None => return,
    };
    let current = read_slot(active_ref(state), slot);
    if current == MARKER_EMPTY {
        return;
    }
    let new_pos = if forward {
        current.saturating_add(delta).min(MAX_MARKER_POS)
    } else {
        current.saturating_sub(delta)
    };
    write_slot(state, slot, new_pos);
}

fn reset_to_quartiles(state: &mut MarkerFsmState, sof: u32, eof: u32) {
    let clamped_eof = eof.min(MAX_MARKER_POS);
    let len = clamped_eof.saturating_sub(sof);
    let m1 = sof.saturating_add(len / 4);
    let m2 = sof.saturating_add(len / 2);
    let m3 = sof.saturating_add((len / 4).saturating_mul(3));
    write_slot(state, 1, m1);
    write_slot(state, 2, m2);
    write_slot(state, 3, m3);
}

fn adjust_rep(state: &mut MarkerFsmState, delta: i8) {
    // Segment index is the same as selected marker index (0..=3); adjust
    // the rep nibble for that segment. SPRINT12 F1 regression: ensure we
    // target the selected segment, not always reps[3].
    let seg = match state.selection {
        Some(s) => s.as_index(),
        None => return,
    };
    if seg > 3 {
        return;
    }
    let apply = |bank: &mut MarkerBank| {
        let cur = bank.reps[seg] as i16;
        let new = (cur + delta as i16).clamp(0, REP_MAX as i16);
        bank.reps[seg] = new as u8;
    };
    if state.bank_sync {
        apply(&mut state.config.bank_a);
        apply(&mut state.config.bank_b);
    } else {
        match state.active_bank {
            Bank::A => apply(&mut state.config.bank_a),
            Bank::B => apply(&mut state.config.bank_b),
        }
    }
}

/// Direction for [`cycle_selection`].
#[derive(Debug, Clone, Copy)]
enum CycleDirection {
    Next,
    Prev,
}

/// Advance or retreat selection through the defined markers in the active
/// bank (wrapping). SOF is always defined. Matches legacy `App.select_next_marker`
/// / `App.select_prev_marker` semantics.
fn cycle_selection(state: &MarkerFsmState, dir: CycleDirection) -> Option<Selection> {
    let defined = defined_selections(state);
    if defined.is_empty() {
        return state.selection;
    }
    let current = state.selection;

    match dir {
        CycleDirection::Next => {
            // Legacy: if current is None, use usize::MAX so wrap-to-first fires.
            let cur_idx = current.map(|s| s.as_index()).unwrap_or(usize::MAX);
            defined
                .iter()
                .find(|s| s.as_index() > cur_idx)
                .copied()
                .or_else(|| defined.first().copied())
        }
        CycleDirection::Prev => {
            // Legacy: if current is None, use 0 so wrap-to-last fires.
            let cur_idx = current.map(|s| s.as_index()).unwrap_or(0);
            defined
                .iter()
                .rev()
                .find(|s| s.as_index() < cur_idx)
                .copied()
                .or_else(|| defined.last().copied())
        }
    }
}

/// Indices of defined markers in the active bank, sorted ascending.
/// SOF is always included.
fn defined_selections(state: &MarkerFsmState) -> Vec<Selection> {
    let bank = active_ref(state);
    let mut out = vec![Selection::Sof];
    if bank.m1 != MARKER_EMPTY {
        out.push(Selection::M1);
    }
    if bank.m2 != MARKER_EMPTY {
        out.push(Selection::M2);
    }
    if bank.m3 != MARKER_EMPTY {
        out.push(Selection::M3);
    }
    out
}

// ---------- App-facing wrapper ----------

/// App-facing wrapper around [`StateMachine<MarkerBankMachine>`].
pub struct MarkerFsm {
    machine: StateMachine<MarkerBankMachine>,
}

impl MarkerFsm {
    /// Create a fresh machine at the initial state.
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
        }
    }

    /// Create a machine pre-seeded with the given state. Useful for
    /// tests that need to start from a non-default configuration.
    pub fn from_state(state: MarkerFsmState) -> Self {
        Self {
            machine: StateMachine::from_state(state),
        }
    }

    /// Apply an input. Returns the output, if any.
    pub fn consume(&mut self, input: Input) -> Option<Output> {
        self.machine.consume(&input).ok().flatten()
    }

    /// Current FSM state.
    pub fn state(&self) -> &MarkerFsmState {
        self.machine.state()
    }

    /// Legacy view matching `App.selected_marker: Option<usize>`.
    pub fn selected_marker(&self) -> Option<usize> {
        self.state().selection.map(Selection::as_index)
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

    fn with_bank_a(bank_a: MarkerBank) -> MarkerFsm {
        let state = MarkerFsmState {
            config: MarkerConfig {
                bank_a,
                bank_b: EMPTY_BANK,
            },
            bank_sync: false,
            ..MarkerFsmState::default()
        };
        MarkerFsm {
            machine: StateMachine::from_state(state),
        }
    }

    // ---------- baseline ----------

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
    fn selection_roundtrip_to_legacy_usize() {
        for (selection, expected) in [
            (None, None),
            (Some(Selection::Sof), Some(0usize)),
            (Some(Selection::M1), Some(1)),
            (Some(Selection::M2), Some(2)),
            (Some(Selection::M3), Some(3)),
        ] {
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

    // ---------- bank/sync/visibility ----------

    #[test]
    fn toggle_bank_flips_active_bank() {
        let mut fsm = MarkerFsm::new();
        assert_eq!(fsm.active_bank(), Bank::A);
        let _ = fsm.consume(Input::ToggleBank);
        assert_eq!(fsm.active_bank(), Bank::B);
        let _ = fsm.consume(Input::ToggleBank);
        assert_eq!(fsm.active_bank(), Bank::A);
    }

    #[test]
    fn toggle_bank_sync_flips_and_is_idempotent_in_pairs() {
        let mut fsm = MarkerFsm::new();
        assert!(fsm.bank_sync());
        let _ = fsm.consume(Input::ToggleBankSync);
        assert!(!fsm.bank_sync());
        let _ = fsm.consume(Input::ToggleBankSync);
        assert!(fsm.bank_sync());
    }

    #[test]
    fn toggle_markers_disabled_blocks_edits() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::ToggleMarkerDisplay);
        assert!(!fsm.markers_visible());
        let before = *fsm.state();
        let _ = fsm.consume(Input::SetMarker1(42));
        let _ = fsm.consume(Input::ClearBankMarkers);
        let _ = fsm.consume(Input::MarkerReset {
            sof: 0,
            eof: 44_100,
        });
        let _ = fsm.consume(Input::IncrementRep);
        assert_eq!(
            *fsm.state(),
            before,
            "edit inputs must be no-ops while markers_visible=false",
        );
    }

    // ---------- set + sync ----------

    #[test]
    fn set_marker_mirrors_to_both_banks_under_sync() {
        let mut fsm = MarkerFsm::new();
        assert!(fsm.bank_sync());
        let _ = fsm.consume(Input::SetMarker1(1000));
        let _ = fsm.consume(Input::SetMarker2(2000));
        let _ = fsm.consume(Input::SetMarker3(3000));
        assert_eq!(fsm.config().bank_a.m1, 1000);
        assert_eq!(fsm.config().bank_a.m2, 2000);
        assert_eq!(fsm.config().bank_a.m3, 3000);
        assert_eq!(fsm.config().bank_b.m1, 1000);
        assert_eq!(fsm.config().bank_b.m2, 2000);
        assert_eq!(fsm.config().bank_b.m3, 3000);
    }

    #[test]
    fn set_marker_targets_only_active_bank_without_sync() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::ToggleBankSync);
        assert!(!fsm.bank_sync());
        let _ = fsm.consume(Input::SetMarker1(100));
        assert_eq!(fsm.config().bank_a.m1, 100);
        assert_eq!(fsm.config().bank_b.m1, MARKER_EMPTY);
    }

    // ---------- clear ----------

    #[test]
    fn clear_nearest_picks_closest_defined_marker() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 100,
            m2: 500,
            m3: 900,
            reps: [0; 4],
        });
        let _ = fsm.consume(Input::ClearNearestMarker(510));
        assert_eq!(fsm.config().bank_a.m1, 100);
        assert_eq!(fsm.config().bank_a.m2, MARKER_EMPTY); // cleared
        assert_eq!(fsm.config().bank_a.m3, 900);
    }

    #[test]
    fn clear_nearest_is_noop_on_empty_bank() {
        let mut fsm = with_bank_a(EMPTY_BANK);
        let before = *fsm.state();
        let _ = fsm.consume(Input::ClearNearestMarker(500));
        assert_eq!(*fsm.state(), before);
    }

    #[test]
    fn clear_bank_markers_preserves_reps() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 100,
            m2: 200,
            m3: 300,
            reps: [1, 2, 3, 4],
        });
        let _ = fsm.consume(Input::ClearBankMarkers);
        assert_eq!(fsm.config().bank_a.m1, MARKER_EMPTY);
        assert_eq!(fsm.config().bank_a.m2, MARKER_EMPTY);
        assert_eq!(fsm.config().bank_a.m3, MARKER_EMPTY);
        assert_eq!(
            fsm.config().bank_a.reps,
            [1, 2, 3, 4],
            "rep values are segment-level metadata, not marker-level",
        );
    }

    // ---------- selection cycling ----------

    #[test]
    fn select_next_from_none_with_empty_bank_lands_on_sof() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::Sof));
    }

    #[test]
    fn select_next_cycles_through_defined_markers_and_wraps() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 100,
            m2: 200,
            m3: 300,
            reps: [0; 4],
        });
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::Sof));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::M1));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::M2));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::M3));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::Sof), "wrap");
    }

    #[test]
    fn select_next_skips_undefined_markers() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: MARKER_EMPTY,
            m2: 500,
            m3: MARKER_EMPTY,
            reps: [0; 4],
        });
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::Sof));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(fsm.state().selection, Some(Selection::M2));
        let _ = fsm.consume(Input::SelectNextMarker);
        assert_eq!(
            fsm.state().selection,
            Some(Selection::Sof),
            "M1 and M3 are undefined; wrap past M2 to SOF",
        );
    }

    #[test]
    fn select_prev_with_none_wraps_to_last_defined() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 100,
            m2: 200,
            m3: 300,
            reps: [0; 4],
        });
        let _ = fsm.consume(Input::SelectPrevMarker);
        assert_eq!(fsm.state().selection, Some(Selection::M3));
    }

    // ---------- nudge ----------

    #[test]
    fn nudge_forward_moves_selected_marker() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 1_000,
            ..EMPTY_BANK
        });
        let _ = fsm.consume(Input::SelectNextMarker); // SOF
        let _ = fsm.consume(Input::SelectNextMarker); // M1
        let _ = fsm.consume(Input::NudgeForward(50));
        assert_eq!(fsm.config().bank_a.m1, 1_050);
    }

    #[test]
    fn nudge_is_noop_with_sof_selected() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 1_000,
            ..EMPTY_BANK
        });
        let _ = fsm.consume(Input::SelectNextMarker); // SOF
        let before = fsm.config().bank_a;
        let _ = fsm.consume(Input::NudgeForward(50));
        let _ = fsm.consume(Input::NudgeBackward(50));
        assert_eq!(
            fsm.config().bank_a,
            before,
            "SOF is immovable; nudge must be a no-op",
        );
    }

    #[test]
    fn nudge_is_noop_on_undefined_marker() {
        let mut fsm = with_bank_a(EMPTY_BANK);
        let state_with_selection = MarkerFsmState {
            selection: Some(Selection::M1),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state_with_selection),
        };
        let before = fsm.config().bank_a;
        let _ = fsm.consume(Input::NudgeForward(100));
        assert_eq!(
            fsm.config().bank_a,
            before,
            "undefined markers (MARKER_EMPTY) must not be nudged",
        );
    }

    #[test]
    fn nudge_round_trip_holds_away_from_boundaries() {
        // Inverse property P5 under saturating arithmetic: holds when
        // marker stays in [delta, MAX_MARKER_POS - delta].
        let mut fsm = with_bank_a(MarkerBank {
            m1: 10_000,
            ..EMPTY_BANK
        });
        let state = MarkerFsmState {
            selection: Some(Selection::M1),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let before = fsm.config().bank_a.m1;
        let _ = fsm.consume(Input::NudgeForward(250));
        let _ = fsm.consume(Input::NudgeBackward(250));
        assert_eq!(fsm.config().bank_a.m1, before);
    }

    #[test]
    fn nudge_saturates_at_max_marker_pos() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: MAX_MARKER_POS - 5,
            ..EMPTY_BANK
        });
        let state = MarkerFsmState {
            selection: Some(Selection::M1),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let _ = fsm.consume(Input::NudgeForward(10_000));
        assert_eq!(
            fsm.config().bank_a.m1,
            MAX_MARKER_POS,
            "must cap below MARKER_EMPTY sentinel",
        );
    }

    // ---------- reset / rep ----------

    #[test]
    fn marker_reset_writes_quartile_layout() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::MarkerReset {
            sof: 0,
            eof: 40_000,
        });
        assert_eq!(fsm.config().bank_a.m1, 10_000);
        assert_eq!(fsm.config().bank_a.m2, 20_000);
        assert_eq!(fsm.config().bank_a.m3, 30_000);
        // Mirrored to bank_b under default sync=on.
        assert_eq!(fsm.config().bank_b.m1, 10_000);
    }

    #[test]
    fn marker_reset_is_idempotent() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::MarkerReset {
            sof: 0,
            eof: 40_000,
        });
        let once = *fsm.state();
        let _ = fsm.consume(Input::MarkerReset {
            sof: 0,
            eof: 40_000,
        });
        assert_eq!(*fsm.state(), once);
    }

    #[test]
    fn rep_increment_targets_selected_segment_regression_sprint12_f1() {
        // SPRINT12 F1: Rep Increment Always Affects Segment 4 regardless of
        // which marker is selected. Verify the FSM targets the indexed segment.
        let mut fsm = with_bank_a(MarkerBank {
            m1: 1_000,
            m2: 2_000,
            m3: 3_000,
            reps: [5, 5, 5, 5],
        });
        let state = MarkerFsmState {
            selection: Some(Selection::M1),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let _ = fsm.consume(Input::IncrementRep);
        assert_eq!(
            fsm.config().bank_a.reps,
            [5, 6, 5, 5],
            "M1 selection → reps[1] increments, others untouched",
        );
    }

    #[test]
    fn rep_clamps_at_bounds() {
        let mut fsm = with_bank_a(MarkerBank {
            reps: [0, 15, 0, 0],
            ..EMPTY_BANK
        });
        // DecrementRep when already at 0 stays at 0.
        let state = MarkerFsmState {
            selection: Some(Selection::Sof),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let _ = fsm.consume(Input::DecrementRep);
        assert_eq!(fsm.config().bank_a.reps[0], 0);

        // IncrementRep when at REP_MAX stays at REP_MAX.
        let state = MarkerFsmState {
            selection: Some(Selection::M1),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let _ = fsm.consume(Input::IncrementRep);
        assert_eq!(fsm.config().bank_a.reps[1], REP_MAX);
    }

    // ---------- I/O outputs ----------

    #[test]
    fn export_emits_write_csv_descriptor_without_state_change() {
        let mut fsm = MarkerFsm::new();
        let before = *fsm.state();
        let path = PathBuf::from("/tmp/riffgrep/markers.csv");
        let out = fsm.consume(Input::ExportMarkersCsv(path.clone()));
        assert_eq!(out, Some(Output::WriteCsv(path)));
        assert_eq!(*fsm.state(), before);
    }

    #[test]
    fn import_emits_read_csv_descriptor_without_state_change() {
        let mut fsm = MarkerFsm::new();
        let before = *fsm.state();
        let path = PathBuf::from("/tmp/riffgrep/markers.csv");
        let out = fsm.consume(Input::ImportMarkersCsv(path.clone()));
        assert_eq!(out, Some(Output::ReadCsv(path)));
        assert_eq!(*fsm.state(), before);
    }

    // ---------- regression: MARKER_EMPTY sentinel (46168e6) ----------

    #[test]
    fn marker_empty_sentinel_round_trips_across_clear_and_set() {
        let mut fsm = MarkerFsm::new();
        let _ = fsm.consume(Input::SetMarker1(0)); // 0 ≠ MARKER_EMPTY — this is SOF sample, a valid marker
        assert_eq!(fsm.config().bank_a.m1, 0);
        let _ = fsm.consume(Input::ClearBankMarkers);
        assert_eq!(
            fsm.config().bank_a.m1,
            MARKER_EMPTY,
            "cleared marker must be MARKER_EMPTY sentinel, not 0",
        );
    }

    // ---------- regression: nudge targets selected (6d23741) ----------

    #[test]
    fn nudge_targets_selected_marker_not_a_fixed_slot_regression_6d23741() {
        let mut fsm = with_bank_a(MarkerBank {
            m1: 1_000,
            m2: 2_000,
            m3: 3_000,
            reps: [0; 4],
        });
        let state = MarkerFsmState {
            selection: Some(Selection::M2),
            ..*fsm.state()
        };
        fsm = MarkerFsm {
            machine: StateMachine::from_state(state),
        };
        let _ = fsm.consume(Input::NudgeForward(10));
        assert_eq!(fsm.config().bank_a.m1, 1_000, "m1 untouched");
        assert_eq!(fsm.config().bank_a.m2, 2_010, "m2 nudged");
        assert_eq!(fsm.config().bank_a.m3, 3_000, "m3 untouched");
    }
}
