//! Integration test entrypoint for the search-FSM property suite.
//!
//! Declares submodules and wires [`prop_state_machine!`] harness
//! invocations using a per-suite [`TestConfig`]. Same `MAX_STEPS`
//! const pattern as playback_fsm — the macro needs a literal range.

pub mod generators;
pub mod prop;
pub mod unit;

use proptest::test_runner::Config as ProptestConfig;
use proptest_state_machine::prop_state_machine;

use prop::{NoSpawnInSimilarityTest, SyncedSutTest};

/// Harness configuration.
#[derive(Debug, Clone, Copy)]
pub struct TestConfig {
    /// Number of proptest cases per property.
    pub cases: u32,
    /// Enable proptest's per-case trace output.
    pub verbose: bool,
}

/// Maximum transitions per `prop_state_machine!` case. Passed via
/// `sequential 1..MAX_STEPS` to each harness invocation — same
/// single-const pattern as playback_fsm so bumping this widens all
/// suites in lockstep.
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
    /// Translate into a [`ProptestConfig`] suitable for `prop_state_machine!`.
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
    fn r3_no_spawn_search_in_similarity_mode(sequential 1..MAX_STEPS => NoSpawnInSimilarityTest);
}
