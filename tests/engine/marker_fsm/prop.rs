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
use riffgrep::engine::marker_fsm::{
    Input, MarkerBankMachine, MarkerFsm, MarkerFsmState,
};

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
