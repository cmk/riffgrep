//! Marker-FSM property suite entrypoint.
//!
//! See `doc/designs/debt-fsm.md` and `doc/plans/plan-2026-04-18-02.md`.
//!
//! Structure:
//! - `generators`: action-stream generators (state-filtered as needed
//!   per-property). Named `generators` rather than `gen` because `gen`
//!   is a reserved keyword in Rust 2024.
//! - `prop`: [`proptest_state_machine::ReferenceStateMachine`] +
//!   [`proptest_state_machine::StateMachineTest`] impls
//! - `unit`: migrated unit regressions (populated incrementally)
//!
//! `TestConfig` centralises prop-test parameters; defaults are picked
//! to keep wall-clock < 60 s for the full suite in debug builds.

mod generators;
mod prop;
mod unit;

use proptest::test_runner::Config as ProptestConfig;
use proptest_state_machine::prop_state_machine;

use prop::{BankSyncPreservationTest, DisabledFixedPointTest, SyncedSutTest};

/// Central knob for the property suite.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Number of proptest cases per property.
    pub cases: u32,
    /// Maximum transition sequence length per case.
    pub max_steps: u32,
    /// Verbose proptest logging (prints each transition).
    pub verbose: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        // Read `RIFFGREP_PROPTEST_CASES` env var if set; fall back to a
        // small default so `cargo test` stays snappy.
        let cases = std::env::var("RIFFGREP_PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(64);
        Self {
            cases,
            max_steps: 32,
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

// Materialise the TestConfig once so the proptest_config expression in
// the macro below is a simple `const`-y expression.
fn default_config() -> ProptestConfig {
    TestConfig::default().proptest()
}

// Baseline: drive arbitrary action streams through both reference and
// SUT; [`SyncedSutTest::check_invariants`] catches any drift. If P1-P8
// are failing for harness reasons rather than logic reasons, this test
// will fail first.
prop_state_machine! {
    #![proptest_config(default_config())]
    #[test]
    fn synced_sut_matches_reference(sequential 1..32 => SyncedSutTest);

    #[test]
    fn p6_bank_sync_preservation(sequential 1..32 => BankSyncPreservationTest);

    #[test]
    fn p7_markers_disabled_fixed_point(sequential 1..32 => DisabledFixedPointTest);
}
