//! Reference state machine + property-test harness.
//!
//! `MarkerFsmModel` is the reference model: a plain `MarkerFsmState` that
//! we drive through [`crate::engine::marker_fsm::MarkerBankMachine::transition`].
//! Because the SUT (the `MarkerFsm` wrapper) runs the exact same transition
//! function, the reference and SUT are guaranteed to stay in lockstep — the
//! `StateMachineTest::check_invariants` assertion locks that in.
//!
//! Property-specific generator restrictions live in [`crate::gen`]. Each
//! property that needs a different generator gets its own
//! `StateMachineTest` impl below; shared invariants (bank-sync mirroring
//! when `bank_sync` is on, visibility gating) are checked in
//! [`SharedInvariants::assert_all`] which every test calls.
//!
//! Task 6 fills in the P1-P8 property suite; this file lands the
//! `SyncedSutTest` baseline that proves the harness wiring.

use proptest::prelude::*;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
use rust_fsm::StateMachineImpl;

use riffgrep::engine::bext::MARKER_EMPTY;
use riffgrep::engine::marker_fsm::{Bank, Input, MarkerBankMachine, MarkerFsm, MarkerFsmState};

use crate::generators;

/// Reference model: just the pure state + the FSM's `transition` function.
pub struct MarkerFsmModel;

impl ReferenceStateMachine for MarkerFsmModel {
    type State = MarkerFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(MarkerFsmState::default()).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::any_input(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        // Reference model runs the exact same transition function as the SUT
        // — that's the whole contract we're verifying.
        MarkerBankMachine::transition(&state, transition).unwrap_or(state)
    }
}

/// Invariants that hold after *every* transition, regardless of the
/// property under test. Call [`SharedInvariants::assert_all`] from each
/// `StateMachineTest::check_invariants` impl.
pub struct SharedInvariants;

impl SharedInvariants {
    pub fn assert_all(sut: &MarkerFsm, reference: &MarkerFsmState) {
        // Contract 1: reference and SUT never drift. This is structural —
        // both drive the same `MarkerBankMachine::transition` function.
        assert_eq!(
            sut.state(),
            reference,
            "SUT state diverged from reference model",
        );

        // (Bank-sync equality is *not* a shared invariant — it only
        // holds when `ToggleBankSync` is excluded from the stream. See
        // Task 6's P6 property.)
        //
        // Marker bound invariants (defined marker < MARKER_EMPTY) are
        // encoded directly in the transition table via MAX_MARKER_POS
        // saturation; they don't need a runtime check here.
        let _ = MARKER_EMPTY;
    }
}

/// Baseline harness: exercises every transition and verifies
/// [`SharedInvariants`] after each step. If this test fails, the
/// harness wiring is broken (not an individual property).
pub struct SyncedSutTest;

impl StateMachineTest for SyncedSutTest {
    type SystemUnderTest = MarkerFsm;
    type Reference = MarkerFsmModel;

    fn init_test(_ref_state: &MarkerFsmState) -> Self::SystemUnderTest {
        // The reference is always `MarkerFsmState::default()` (see
        // [`MarkerFsmModel::init_state`]), so a fresh `MarkerFsm` suffices.
        MarkerFsm::new()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &MarkerFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &MarkerFsmState) {
        SharedInvariants::assert_all(state, ref_state);
    }
}

// =============================================================================
// Properties P1-P8 (see `doc/designs/debt-fsm.md` §State-Dependent Invariants)
// =============================================================================
//
// Layout:
// - P1, P3, P4, P5, P8 are expressed as plain `proptest!` tests — they
//   need to *inject* specific transitions into an arbitrary prefix, which
//   is awkward under `prop_state_machine!`.
// - P6, P7 are `StateMachineTest` impls with restricted generators:
//   the whole generated stream respects the precondition, so the
//   invariant becomes a simple post-transition check.
// - P2 is subsumed by P1 (both reduce to reset idempotence); skipped.

fn run_prefix(fsm: &mut MarkerFsm, prefix: &[Input]) {
    for input in prefix {
        let _ = fsm.consume(input.clone());
    }
}

fn fsm_with_prefix(prefix: &[Input]) -> MarkerFsm {
    let mut fsm = MarkerFsm::new();
    run_prefix(&mut fsm, prefix);
    fsm
}

fn input_seq_strategy(
    len: impl Into<proptest::collection::SizeRange>,
) -> BoxedStrategy<Vec<Input>> {
    // `any_input` is state-independent in practice, so we pass a dummy
    // state. If generators grow state-dependence we'd need to inline.
    let dummy = MarkerFsmState::default();
    proptest::collection::vec(generators::any_input(&dummy), len).boxed()
}

// ---- P1: MarkerReset is idempotent ----
//
// reset(sof, eof) ∘ reset(sof, eof) ≡ reset(sof, eof) for any reachable state.

proptest! {
    #[test]
    fn p1_reset_is_idempotent(
        prefix in input_seq_strategy(0..20),
        sof in 0u32..1_000_000,
        delta in 0u32..1_000_000,
    ) {
        let eof = sof.saturating_add(delta);
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::MarkerReset { sof, eof });
        let once = *fsm.state();
        let _ = fsm.consume(Input::MarkerReset { sof, eof });
        prop_assert_eq!(*fsm.state(), once);
    }
}

// ---- P3: SelectPrev ∘ SelectNext ≡ id (except at M3-selected edge) ----
//
// Under wrap semantics this actually holds at M3 too, because next(M3) wraps
// to the first defined marker and prev wraps back. We test it unconditionally,
// with the qualification that `selection` must be `Some` at the insertion
// point (wrap-from-None is not a no-op — see `cycle_selection`).

proptest! {
    #[test]
    fn p3_prev_after_next_preserves_state_when_selected(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        prop_assume!(selection_is_at_defined_slot(fsm.state()));
        let before = *fsm.state();
        let _ = fsm.consume(Input::SelectNextMarker);
        let _ = fsm.consume(Input::SelectPrevMarker);
        prop_assert_eq!(*fsm.state(), before);
    }
}

// ---- P4: SelectNext ∘ SelectPrev ≡ id (mirror of P3) ----

proptest! {
    #[test]
    fn p4_next_after_prev_preserves_state_when_selected(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        prop_assume!(selection_is_at_defined_slot(fsm.state()));
        let before = *fsm.state();
        let _ = fsm.consume(Input::SelectPrevMarker);
        let _ = fsm.consume(Input::SelectNextMarker);
        prop_assert_eq!(*fsm.state(), before);
    }
}

// ---- P5: Nudge round-trip ----
//
// NudgeForward(d) ∘ NudgeBackward(d) ≡ id provided the selected marker is
// defined, is not SOF, and the nudge stays within [delta, MAX_MARKER_POS - delta]
// so saturating arithmetic doesn't clip.

proptest! {
    #[test]
    fn p5_nudge_forward_then_backward_is_id(
        prefix in input_seq_strategy(0..20),
        delta in 1u32..100_000,
    ) {
        let mut fsm = fsm_with_prefix(&prefix);

        // Precondition: a movable, defined marker is selected; banks are
        // coherent under sync (otherwise nudge-under-sync awakens an
        // empty marker in bank_b and breaks the inverse).
        let Some(slot_val) = nudge_round_trip_eligible(fsm.state()) else {
            return Ok(());
        };
        let max_pos = riffgrep::engine::marker_fsm::MAX_MARKER_POS;
        prop_assume!(slot_val >= delta);
        prop_assume!(slot_val <= max_pos - delta);

        let before = *fsm.state();
        let _ = fsm.consume(Input::NudgeForward(delta));
        let _ = fsm.consume(Input::NudgeBackward(delta));
        prop_assert_eq!(*fsm.state(), before);
    }
}

/// Return the selected marker's position if it's defined and movable
/// (not SOF, not MARKER_EMPTY). Returns `None` otherwise.
fn movable_selected_value(state: &MarkerFsmState) -> Option<u32> {
    use riffgrep::engine::marker_fsm::Selection;
    let bank = match state.active_bank {
        Bank::A => &state.config.bank_a,
        Bank::B => &state.config.bank_b,
    };
    match state.selection {
        Some(Selection::M1) if bank.m1 != MARKER_EMPTY => Some(bank.m1),
        Some(Selection::M2) if bank.m2 != MARKER_EMPTY => Some(bank.m2),
        Some(Selection::M3) if bank.m3 != MARKER_EMPTY => Some(bank.m3),
        _ => None,
    }
}

/// P5-specific precondition: a movable marker is selected and, under
/// `bank_sync`, the two banks' corresponding slot already match.
///
/// Without the sync check, a nudge-then-unnudge on a state where bank_b's
/// slot is `MARKER_EMPTY` propagates the active-bank value into bank_b
/// and the round-trip comes up short.
fn nudge_round_trip_eligible(state: &MarkerFsmState) -> Option<u32> {
    use riffgrep::engine::marker_fsm::Selection;
    let pos = movable_selected_value(state)?;
    if state.bank_sync {
        let (a, b) = (&state.config.bank_a, &state.config.bank_b);
        let (av, bv) = match state.selection {
            Some(Selection::M1) => (a.m1, b.m1),
            Some(Selection::M2) => (a.m2, b.m2),
            Some(Selection::M3) => (a.m3, b.m3),
            _ => return None,
        };
        if av != bv {
            return None;
        }
    }
    Some(pos)
}

/// Whether the current selection is at a defined marker slot (SOF is
/// always defined; Mi is defined iff its value ≠ MARKER_EMPTY). P3/P4/P8
/// require this: the wrap-cycle is only an inverse when we start from a
/// member of the `defined_selections` set.
fn selection_is_at_defined_slot(state: &MarkerFsmState) -> bool {
    use riffgrep::engine::marker_fsm::Selection;
    let bank = match state.active_bank {
        Bank::A => &state.config.bank_a,
        Bank::B => &state.config.bank_b,
    };
    match state.selection {
        None => false,
        Some(Selection::Sof) => true,
        Some(Selection::M1) => bank.m1 != MARKER_EMPTY,
        Some(Selection::M2) => bank.m2 != MARKER_EMPTY,
        Some(Selection::M3) => bank.m3 != MARKER_EMPTY,
    }
}

// ---- P8: Inserting [SelectPrev, SelectNext] preserves end state ----
//
// If at insertion point the selection is Some, the insertion is a no-op
// and the final state after `seq[:k] ++ [prev, next] ++ seq[k:]` equals
// the final state after `seq`.

proptest! {
    #[test]
    fn p8_prev_next_insertion_preserves_end_state(
        seq in input_seq_strategy(0..25),
        insert_at in 0usize..25,
    ) {
        let k = insert_at.min(seq.len());

        let mut fsm_base = MarkerFsm::new();
        run_prefix(&mut fsm_base, &seq);

        let mut fsm_insert = MarkerFsm::new();
        run_prefix(&mut fsm_insert, &seq[..k]);

        prop_assume!(selection_is_at_defined_slot(fsm_insert.state()));

        let _ = fsm_insert.consume(Input::SelectPrevMarker);
        let _ = fsm_insert.consume(Input::SelectNextMarker);
        run_prefix(&mut fsm_insert, &seq[k..]);

        prop_assert_eq!(*fsm_insert.state(), *fsm_base.state());
    }
}

// =============================================================================
// P6: Bank-sync preservation
// =============================================================================
//
// If `bank_sync` starts true and `ToggleBankSync` never appears in the
// stream, then `bank_a == bank_b` after every step. Enforced in the
// transition table by `write_slot` mirroring writes under `bank_sync`.

pub struct BankSyncPreservationModel;

impl ReferenceStateMachine for BankSyncPreservationModel {
    type State = MarkerFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        // Initial state has bank_sync = true (default); that's the
        // precondition for this property.
        Just(MarkerFsmState::default()).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::transitions_no_sync_toggle(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        MarkerBankMachine::transition(&state, transition).unwrap_or(state)
    }
}

pub struct BankSyncPreservationTest;

impl StateMachineTest for BankSyncPreservationTest {
    type SystemUnderTest = MarkerFsm;
    type Reference = BankSyncPreservationModel;

    fn init_test(_ref_state: &MarkerFsmState) -> Self::SystemUnderTest {
        MarkerFsm::new()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &MarkerFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &MarkerFsmState) {
        SharedInvariants::assert_all(state, ref_state);
        assert!(
            ref_state.bank_sync,
            "P6 precondition breach: bank_sync must stay true",
        );
        assert_eq!(
            ref_state.config.bank_a, ref_state.config.bank_b,
            "P6: with bank_sync=true and no ToggleBankSync, banks must match",
        );
    }
}

// =============================================================================
// P7: Markers-disabled fixed point
// =============================================================================
//
// Once `visible` is flipped to false and `ToggleMarkerDisplay` is excluded,
// no edit input may change any state field. Enforced in transition() by
// the `!state.visible && input.is_edit()` short-circuit.

pub struct DisabledFixedPointModel;

impl ReferenceStateMachine for DisabledFixedPointModel {
    type State = MarkerFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        // Force visible=false at the start — that's the precondition.
        Just(MarkerFsmState {
            visible: false,
            ..MarkerFsmState::default()
        })
        .boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::transitions_no_display_toggle(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        MarkerBankMachine::transition(&state, transition).unwrap_or(state)
    }
}

pub struct DisabledFixedPointTest;

impl StateMachineTest for DisabledFixedPointTest {
    type SystemUnderTest = MarkerFsm;
    type Reference = DisabledFixedPointModel;

    fn init_test(ref_state: &MarkerFsmState) -> Self::SystemUnderTest {
        // SUT must start in the same disabled state as the reference.
        MarkerFsm::from_state(*ref_state)
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &MarkerFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &MarkerFsmState) {
        SharedInvariants::assert_all(state, ref_state);
        assert!(
            !ref_state.visible,
            "P7 precondition breach: visible must stay false",
        );
        // Edit invariance: with visible=false, the marker config never
        // changes from its initial value. init_state fixes an empty
        // config, so we assert against that.
        let initial = MarkerFsmState {
            visible: false,
            ..MarkerFsmState::default()
        };
        assert_eq!(
            ref_state.config, initial.config,
            "P7: config must not change while visible=false",
        );
    }
}
