//! Integration test entrypoint for the playback-FSM property suite.
//!
//! Declares submodules and wires [`prop_state_machine!`] harness
//! invocations using a per-suite [`TestConfig`]. Case count is
//! overridable via the `RIFFGREP_PROPTEST_CASES` env var so CI can
//! crank it up without editing source.

pub mod generators;
pub mod prop;
pub mod unit;

use proptest::test_runner::Config as ProptestConfig;
use proptest_state_machine::prop_state_machine;

use prop::{PauseResumeTest, SyncedSutTest};

/// Harness configuration. Defaults picked so the full suite runs in
/// well under the 60 s budget at `cargo test --release`.
///
/// Per-case step range is controlled by the `sequential 1..{n}` arg
/// to `prop_state_machine!` below, not by this struct — the macro
/// needs a literal range expression, so the single source of truth
/// for step count is [`MAX_STEPS`].
#[derive(Debug, Clone, Copy)]
pub struct TestConfig {
    /// Number of proptest cases per property.
    pub cases: u32,
    /// Enable proptest's per-case trace output.
    pub verbose: bool,
}

/// Maximum transitions per `prop_state_machine!` case. Passed through
/// to `sequential 1..MAX_STEPS` on each harness invocation. A single
/// constant so raising it raises both the marker_fsm-style baseline
/// and every Q-property harness in lockstep.
pub const MAX_STEPS: usize = 32;

impl Default for TestConfig {
    fn default() -> Self {
        let cases = std::env::var("RIFFGREP_PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(64);
        TestConfig {
            cases,
            verbose: false,
        }
    }
}

impl TestConfig {
    /// Translate into a [`ProptestConfig`] suitable for
    /// `prop_state_machine!`.
    pub fn proptest(&self) -> ProptestConfig {
        ProptestConfig {
            cases: self.cases,
            max_shrink_iters: 4096,
            verbose: if self.verbose { 1 } else { 0 },
            ..ProptestConfig::default()
        }
    }
}

fn default_config() -> ProptestConfig {
    TestConfig::default().proptest()
}

prop_state_machine! {
    #![proptest_config(default_config())]
    #[test]
    fn synced_sut_matches_reference(sequential 1..MAX_STEPS => SyncedSutTest);

    #[test]
    fn q2_pause_resume_inverse_from_playing(sequential 1..MAX_STEPS => PauseResumeTest);
}
