//! Reference state machine + property-test harness for the playback FSM.
//!
//! `PlaybackFsmModel` drives [`PlaybackMachine::transition`] directly —
//! the SUT (`PlaybackFsm` wrapper) runs the exact same function, so the
//! two stay in lockstep by construction.
//!
//! Property layout:
//! - Q1, Q4, Q8 are `proptest!` tests that inject specific transitions
//!   into an arbitrary prefix.
//! - Q2, Q6, Q7 are full `StateMachineTest` harnesses with generator
//!   restrictions that make the invariant hold for the whole stream.
//! - Q3, Q5 are mid-stream post-condition properties expressed as
//!   `proptest!` tests.

use proptest::prelude::*;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
use rust_fsm::StateMachineImpl;

use riffgrep::engine::playback_fsm::{
    Input, PlaybackFsm, PlaybackFsmState, PlaybackMachine, Transport,
};

use crate::generators;

// =============================================================================
// Reference model (shared baseline for every property test)
// =============================================================================

/// Reference model: pure state + the FSM's `transition` function.
pub struct PlaybackFsmModel;

impl ReferenceStateMachine for PlaybackFsmModel {
    type State = PlaybackFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(PlaybackFsmState::default()).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::any_input(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        PlaybackMachine::transition(&state, transition).unwrap_or(state)
    }
}

/// Invariants that must hold after *every* transition, regardless of the
/// property under test.
pub struct SharedInvariants;

impl SharedInvariants {
    /// Assert every shared invariant. Call from each
    /// `StateMachineTest::check_invariants` impl so adding a contract
    /// here covers every harness automatically.
    pub fn assert_all(sut: &PlaybackFsm, reference: &PlaybackFsmState) {
        // Reference and SUT never drift. Structural — both drive the
        // same `PlaybackMachine::transition`.
        assert_eq!(
            sut.state(),
            reference,
            "SUT state diverged from reference model",
        );
    }
}

/// Baseline harness: drive arbitrary streams and verify invariants.
pub struct SyncedSutTest;

impl StateMachineTest for SyncedSutTest {
    type SystemUnderTest = PlaybackFsm;
    type Reference = PlaybackFsmModel;

    fn init_test(_ref_state: &PlaybackFsmState) -> Self::SystemUnderTest {
        PlaybackFsm::new()
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &PlaybackFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &PlaybackFsmState) {
        SharedInvariants::assert_all(state, ref_state);
    }
}

// =============================================================================
// Q-series helpers
// =============================================================================

fn run_prefix(fsm: &mut PlaybackFsm, prefix: &[Input]) {
    for input in prefix {
        let _ = fsm.consume(*input);
    }
}

fn fsm_with_prefix(prefix: &[Input]) -> PlaybackFsm {
    let mut fsm = PlaybackFsm::new();
    run_prefix(&mut fsm, prefix);
    fsm
}

fn input_seq_strategy(
    len: impl Into<proptest::collection::SizeRange>,
) -> BoxedStrategy<Vec<Input>> {
    let dummy = PlaybackFsmState::default();
    proptest::collection::vec(generators::any_input(&dummy), len).boxed()
}

// =============================================================================
// Q1: Play ∘ Stop ∘ Play ≡ Play (always lands on Playing)
// =============================================================================

proptest! {
    #[test]
    fn q1_play_stop_play_lands_on_playing(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Stop);
        let _ = fsm.consume(Input::Play);
        prop_assert_eq!(fsm.transport(), Transport::Playing);
    }
}

// =============================================================================
// Q3: Stop clears pending_seek and pending_restart
// =============================================================================

proptest! {
    #[test]
    fn q3_stop_clears_pending(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::Stop);
        prop_assert_eq!(fsm.pending_seek(), None);
        prop_assert!(!fsm.pending_restart());
    }
}

// =============================================================================
// Q4: ToggleReverse ∘ ToggleReverse ≡ id
// =============================================================================

proptest! {
    #[test]
    fn q4_toggle_reverse_self_inverse(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let before = *fsm.state();
        let _ = fsm.consume(Input::ToggleReverse);
        let _ = fsm.consume(Input::ToggleReverse);
        prop_assert_eq!(*fsm.state(), before);
    }
}

// =============================================================================
// Q5: Seek(p) followed by ConsumeSeek drains pending_seek
// =============================================================================
// Runs an arbitrary prefix that excludes `Stop` and `ProgramEnded` (both
// clear / can clear `pending_seek` as a side channel), then injects
// `Seek(pos)` and `ConsumeSeek`. Post-condition: `pending_seek` is `None`
// regardless of what the prefix touched. Without the prefix this would
// only test the trivial two-step sequence — the restricted generator is
// the teeth of the property.

fn restricted_input_seq_strategy(
    len: impl Into<proptest::collection::SizeRange>,
) -> BoxedStrategy<Vec<Input>> {
    let dummy = PlaybackFsmState::default();
    proptest::collection::vec(
        generators::transitions_no_stop_or_program_end(&dummy),
        len,
    )
    .boxed()
}

proptest! {
    #[test]
    fn q5_seek_then_consume_seek_drains(
        prefix in restricted_input_seq_strategy(0..20),
        pos in 0u32..1_000_000,
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        let _ = fsm.consume(Input::Seek(pos));
        let _ = fsm.consume(Input::ConsumeSeek);
        prop_assert_eq!(fsm.pending_seek(), None);
    }
}

// =============================================================================
// Q8: [Pause, Resume] insertion preserves end state (from Playing)
// =============================================================================

proptest! {
    #[test]
    fn q8_pause_resume_insertion_preserves_state(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm_a = fsm_with_prefix(&prefix);
        // Force Playing so Pause/Resume are both live.
        let _ = fsm_a.consume(Input::Play);
        let baseline = *fsm_a.state();

        let mut fsm_b = fsm_with_prefix(&prefix);
        let _ = fsm_b.consume(Input::Play);
        let _ = fsm_b.consume(Input::Pause);
        let _ = fsm_b.consume(Input::Resume);
        prop_assert_eq!(*fsm_b.state(), baseline);
    }
}

// =============================================================================
// Q2: Resume ∘ Pause ≡ id when transport = Playing
// =============================================================================
// Expressed as a full state-machine test: every transition in the stream
// ends on an invariant check that Pause-then-Resume from a Playing state
// is a no-op. Setup constrains the starting state to Playing by
// injecting Play before the harness runs.

/// Reference model for the Q2 (Pause/Resume inverse) harness —
/// forces the initial transport to [`Transport::Playing`] so the
/// Pause/Resume injection is meaningful.
pub struct PauseResumeModel;

impl ReferenceStateMachine for PauseResumeModel {
    type State = PlaybackFsmState;
    type Transition = Input;

    fn init_state() -> BoxedStrategy<Self::State> {
        // Start at Playing so Pause/Resume are both meaningful.
        let playing = PlaybackFsmState {
            transport: Transport::Playing,
            ..PlaybackFsmState::default()
        };
        Just(playing).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        generators::any_input(state)
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        PlaybackMachine::transition(&state, transition).unwrap_or(state)
    }
}

/// SUT harness for Q2 (Pause/Resume inverse).
pub struct PauseResumeTest;

impl StateMachineTest for PauseResumeTest {
    type SystemUnderTest = PlaybackFsm;
    type Reference = PauseResumeModel;

    fn init_test(ref_state: &PlaybackFsmState) -> Self::SystemUnderTest {
        PlaybackFsm::from_state(*ref_state)
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        _ref_state_after: &PlaybackFsmState,
        transition: Input,
    ) -> Self::SystemUnderTest {
        let _ = state.consume(transition);
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &PlaybackFsmState) {
        SharedInvariants::assert_all(state, ref_state);
        // Q2: specifically when current transport is Playing, injecting
        // a Pause-then-Resume must be a no-op.
        if ref_state.transport == Transport::Playing {
            let mut probe = PlaybackFsm::from_state(*ref_state);
            let _ = probe.consume(Input::Pause);
            let _ = probe.consume(Input::Resume);
            assert_eq!(
                probe.state(),
                ref_state,
                "Q2 violated: Pause∘Resume should be id when Playing",
            );
        }
    }
}

// =============================================================================
// Q7: Restart from Stopped is a no-op
// =============================================================================
// Mirrors the `PlaybackEngine::restart_program` early-return. Without
// this the FSM would queue a pending_restart against a sink that isn't
// playing, which the mixer thread would then see and act on the next
// time playback starts — a latent foot-gun if the user presses Ctrl-O
// while stopped then starts a new program.

proptest! {
    #[test]
    fn q7_restart_from_stopped_is_noop(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        // Force Stopped state regardless of prefix.
        let _ = fsm.consume(Input::Stop);
        prop_assert_eq!(fsm.transport(), Transport::Stopped);
        prop_assert!(!fsm.pending_restart());

        let _ = fsm.consume(Input::Restart);
        prop_assert_eq!(fsm.transport(), Transport::Stopped);
        prop_assert!(!fsm.pending_restart());
    }
}

// =============================================================================
// Q6: loop_enabled=true ⇒ ProgramEnded keeps Playing + sets pending_restart
// =============================================================================
// This is a local transition property: at any reachable state where
// `loop_enabled` is true and `transport` is Playing, dispatching
// `ProgramEnded` must leave `transport == Playing` and
// `pending_restart == true`. Tested by forcing those preconditions
// after an arbitrary prefix, not by restricting the generator stream.

proptest! {
    #[test]
    fn q6_loop_plus_program_end_queues_restart(
        prefix in input_seq_strategy(0..20),
    ) {
        let mut fsm = fsm_with_prefix(&prefix);
        // Force the precondition regardless of prefix.
        let _ = fsm.consume(Input::SetLoop(true));
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::ProgramEnded);
        prop_assert_eq!(fsm.transport(), Transport::Playing);
        prop_assert!(fsm.pending_restart());
    }
}
