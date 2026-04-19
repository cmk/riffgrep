//! Reference model + property tests for the search FSM.
//!
//! Layout mirrors markers/playback: baseline `SyncedSutTest` + the
//! R-series properties from Plan 08 §6.

use proptest::prelude::*;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
use rust_fsm::StateMachineImpl;

use riffgrep::engine::search_fsm::{
    Input, Mode, Output, SearchFsm, SearchFsmState, SearchMachine, Transport,
};

use crate::generators;

// =============================================================================
// Reference model
// =============================================================================

/// Reference model: drives the same `SearchMachine::transition` as the SUT.
pub struct SearchFsmModel;

impl ReferenceStateMachine for SearchFsmModel {
    type State = SearchFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(SearchFsmState::default()).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::any_input(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        SearchMachine::transition(&state, transition).unwrap_or(state)
    }
}

/// Invariants that hold after every transition.
pub struct SharedInvariants;

impl SharedInvariants {
    /// Assert every shared invariant. Call from each harness's
    /// `check_invariants`.
    pub fn assert_all(sut: &SearchFsm, reference: &SearchFsmState) {
        assert_eq!(
            sut.state(),
            reference,
            "SUT state diverged from reference model",
        );
    }
}

/// Baseline harness.
pub struct SyncedSutTest;

impl StateMachineTest for SyncedSutTest {
    type SystemUnderTest = SearchFsm;
    type Reference = SearchFsmModel;

    fn init_test(_ref_state: &SearchFsmState) -> Self::SystemUnderTest {
        SearchFsm::new()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &SearchFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &SearchFsmState) {
        SharedInvariants::assert_all(state, ref_state);
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn run_prefix(fsm: &mut SearchFsm, prefix: &[Input]) {
    for input in prefix {
        let _ = fsm.consume(input.clone());
    }
}

fn fsm_with_prefix(prefix: &[Input]) -> SearchFsm {
    let mut fsm = SearchFsm::new();
    run_prefix(&mut fsm, prefix);
    fsm
}

fn input_seq_strategy(
    len: impl Into<proptest::collection::SizeRange>,
) -> BoxedStrategy<Vec<Input>> {
    let dummy = SearchFsmState::default();
    proptest::collection::vec(generators::any_input(&dummy), len).boxed()
}

// =============================================================================
// R1: DebounceTick without debounce_dirty is a no-op
// =============================================================================

proptest! {
    #[test]
    fn r1_debounce_tick_without_dirty_is_noop(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        // Force dirty=false (two ticks in a row; second is guaranteed
        // to be no-op regardless of prefix).
        let _ = fsm.consume(Input::DebounceTick);
        let before = fsm.state().clone();
        prop_assume!(!before.debounce_dirty);
        let out = fsm.consume(Input::DebounceTick);
        prop_assert_eq!(fsm.state(), &before);
        prop_assert!(out.is_none());
    }
}

// =============================================================================
// R2: EnterSimilarityMode ∘ ExitSimilarityMode returns to Remote + dirty
// =============================================================================

proptest! {
    #[test]
    fn r2_enter_exit_similarity_returns_to_remote(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::EnterSimilarityMode);
        let _ = fsm.consume(Input::ExitSimilarityMode);
        prop_assert_eq!(fsm.state().mode, Mode::Remote);
        prop_assert!(
            fsm.state().debounce_dirty,
            "exit must queue a fresh remote search",
        );
    }
}

// =============================================================================
// R3: In Similarity mode, SpawnSearch is NEVER emitted
// =============================================================================
// Install Similarity mode in the initial state, then run a stream
// that can't toggle out. Any SpawnSearch output is a failure.

/// Reference model for the R3 (no `SpawnSearch` in Similarity) harness
/// — installs Similarity mode at init and uses a stream that can't
/// toggle out.
pub struct NoSpawnInSimilarityModel;

impl ReferenceStateMachine for NoSpawnInSimilarityModel {
    type State = SearchFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        let state = SearchFsmState {
            mode: Mode::Similarity,
            ..SearchFsmState::default()
        };
        Just(state).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::transitions_no_mode_toggle(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        SearchMachine::transition(&state, transition).unwrap_or(state)
    }
}

/// Harness for R3 — asserts no `SpawnSearch` on any transition.
pub struct NoSpawnInSimilarityTest;

impl StateMachineTest for NoSpawnInSimilarityTest {
    type SystemUnderTest = SearchFsm;
    type Reference = NoSpawnInSimilarityModel;

    fn init_test(ref_state: &SearchFsmState) -> Self::SystemUnderTest {
        SearchFsm::from_state(ref_state.clone())
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &SearchFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let out = state.consume(transition);
        assert!(
            !matches!(out, Some(Output::SpawnSearch { .. })),
            "R3 violated: SpawnSearch emitted in Similarity mode",
        );
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &SearchFsmState) {
        SharedInvariants::assert_all(state, ref_state);
        assert_eq!(
            ref_state.mode,
            Mode::Similarity,
            "R3 precondition: mode must stay Similarity",
        );
    }
}

// =============================================================================
// R4: Cancel always lands Settled; dirty+Tick re-enters Pending
// =============================================================================

proptest! {
    #[test]
    fn r4_cancel_then_dirty_tick_re_enters_pending(
        prefix in input_seq_strategy(0..20),
        q in "[a-z]{1,8}",
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::SearchCancelled);
        prop_assert_eq!(fsm.state().transport, Transport::Settled);

        // Only Remote mode can re-enter Pending on tick. The
        // ExitSimilarityMode transition is guarded to be a no-op when
        // mode is already Remote (see exit_similarity_from_remote_is_noop
        // inline test), so this is safe to dispatch unconditionally.
        let _ = fsm.consume(Input::ExitSimilarityMode);
        let _ = fsm.consume(Input::QueryChanged(q));

        let _ = fsm.consume(Input::DebounceTick);
        prop_assert_eq!(fsm.state().transport, Transport::Pending);
    }
}

// =============================================================================
// R5: Idempotent QueryChanged — typing the same string twice is a no-op
// =============================================================================

proptest! {
    #[test]
    fn r5_same_query_twice_preserves_end_state(
        prefix in input_seq_strategy(0..20),
        q in "[a-z]{0,8}",
    ) {
        let mut fsm_a = fsm_with_prefix(&prefix);
        let _ = fsm_a.consume(Input::QueryChanged(q.clone()));
        let baseline = fsm_a.state().clone();

        let mut fsm_b = fsm_with_prefix(&prefix);
        let _ = fsm_b.consume(Input::QueryChanged(q.clone()));
        let _ = fsm_b.consume(Input::QueryChanged(q));
        prop_assert_eq!(fsm_b.state(), &baseline);
    }
}

// =============================================================================
// R6: Serialization round-trip
// =============================================================================

proptest! {
    #[test]
    fn r6_state_round_trips_through_json(
        prefix in input_seq_strategy(0..20),
    ) {
        let fsm = fsm_with_prefix(&prefix);
        let state = fsm.state().clone();
        let json = serde_json::to_string(&state).expect("ser");
        let decoded: SearchFsmState = serde_json::from_str(&json).expect("de");
        prop_assert_eq!(decoded, state);
    }
}

// =============================================================================
// R7: PR #14 regression — EnterSimilarityMode always emits CancelSearch
// =============================================================================
// Already has an inline unit test; repeating as a property to cover
// all reachable states before the enter.

proptest! {
    #[test]
    fn r7_enter_similarity_always_emits_cancel(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        // Precondition: if we're already in Similarity mode, the
        // Enter is a re-enter and still emits Cancel (idempotent by
        // design — no orphan task possible).
        let out = fsm.consume(Input::EnterSimilarityMode);
        prop_assert_eq!(out, Some(Output::CancelSearch));
        prop_assert_eq!(fsm.state().mode, Mode::Similarity);
        prop_assert_eq!(fsm.state().transport, Transport::Settled);
    }
}
