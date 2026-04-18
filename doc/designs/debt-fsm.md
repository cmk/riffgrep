# FSM / Testing Refactor

* Refactor TUI and Markers code to use the rust-fsm library
  - The TUI and markers feature have both generated a large number of bugs, so I would like to formalize both as finite state machines with defined DSLs
  - Port both to https://github.com/eugene-babichenko/rust-fsm
* Set up proptest and implement a collection of action-stream generators:
  - Import proptest and implement proptest-state-machine's StateMachineTest trait for both FSMs (see below)
  - Notice the transitions function: we are generating random subclasses of actions that
      - obey constraints necessary to test certain properties (e.g. doesn't include the action ctl-b / 'toggle bank sync')
      - represent a range of valid user stories
  - For the TUI FSM 
      - we may want to store additional data in TuiModel (like the length of a string buffer), in which case
      - upgrade the State type to a struct like `struct ModelData { state: SearchBarState, buffer_len: usize }` to track and assert against more complex invariants.
* Use proptest combinators (e.g. prop_oneof) to build a library of action stream generators
  - Gens should obey the necessary constraints for testing the properties below 
  - Consider the desired shrinking behavior (proptest-state-machine may handle this well enough to skip it)
  - Make size, complexity, & constraint specifics configurable wherever possible
  - Place these gens in gen.rs files for each FSM (e.g. riffgrep/test/engine/gen.rs)
* Implement a large and varied collection of properties:
  - Unreachable States: certain 'impossible' states are never reached regardless of input
  - Qualified Unreachable States: as above but for a smaller subset of inputs (e.g. Bank A can never differ from Bank B if the initial state is 'synced' and the action stream doesn't include ctl-b / 'toggle bank sync')
  - Invariant Verification: add qualified 'no-op sub-sequences' (e.g. [opt-h, opt-l], [ctl-left, ctl-right]) to a random action stream and confirm that the original stream and the altered stream have the same end state
  - Fixed Point States: certain states are impossible to leave for certain action streams (e.g. ctl-alt-d / ' toggle markers disabled')
  - Unit Test Generalization: Look at all of the marker unit tests to see where previous bugs have arisen, and try to generalize as many as possible to prop tests
  - State-Dependent Invariants: identify FSM states where certain operational invariants apply, for example with Markers
      - in the initial state (e.g. a freshly selected sample), ctl-r/marker reset is a no-op
      - in all states ctl-r/marker reset is idempotent
      - in all states other than SOF-selected, opt-l is a right-inverse to opt-h (i.e. opt-h * opt-l = 1)
      - in all states other than Marker-3-selected, opt-h is a left-inverse to opt-l (i.e. opt-l * opt-h = 1)
      - for all markers not located at SOF, ctl-right is a right-inverse to ctl-left, and shift-ctl-right is a right-inverse to shift-ctl-left
      - for all markers not located at EOF, ctl-left is a left-inverse to ctl-right, and shift-ctl-left is a left-inverse to shift-ctl-right
  - Place these props in prop.rs files for each FSM (e.g. riffgrep/test/engine/prop.rs)
* Create a consolidated & feature-rich test suite
  - Collect all of the TUI & Marker tests into consolidated test files
      - riffgrep/test/ui/test.rs & riffgrep/test/engine/test.rs for the prop tests
      - riffgrep/test/ui/unit.rs & riffgrep/test/engine/unit.rs for the unit tests
  - Collect prop test config variables into TestConfig structs for easy use
      - on / off toggle
      - number and size of tests
      - generator config (see above) 
      - shrink config (see above)
      - reporting verbosity
  - Turn off failing properties if the bug isn't trivial/obvious, we will fix these in a separate sprint
* Port the existing prop tests from quickcheck to proptest


```
use proptest_state_machine::ReferenceStateMachine;
use proptest::prelude::*;
use rust_fsm::*;

// We wrap our FSM to use as the "Model" for the test
struct MarkerBankModel {
    machine: StateMachine<MarkerBank>,
}

impl ReferenceStateMachine for MarkerBankModel {
    type State = MarkerBankState;
    type Transition = MarkerBankInput;

    // 1. Start state
    fn init_state() -> Self::State {
        MarkerBankState::Default
    }

    // 2. Define valid transitions for the current state (The "Strategy")
    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        match state {
            MarkerBankState::Default => Just(MarkerBankInput::BankSync).boxed(),
            MarkerBankState::BankSync => prop_oneof![
                Just(MarkerBankInput::Reset),
                Just(MarkerBankInput::Export),
            ].boxed(),
        }
    }

    // 3. Apply the transition to the model
    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        let mut m = StateMachine::<MarkerBank>::from_state(state);
        let _ = m.consume(transition);
        *m.state()
    }

    // 4. Invariants: Assertions that must ALWAYS hold true
    fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool {
        // e.g., Reset returns the Marker Bank to its default state for the given category
        if matches!(transition, MarkerBankInput::Reset) {
            return matches!(state, MarkerBankState::Default);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest_state_machine::prop_state_machine;

    prop_state_machine! {
        #[test]
        fn run_marker_test(sequential 1..20 => MarkerBankModel);
    }
}
```

