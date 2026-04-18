//! Migrated unit regressions for the marker FSM.
//!
//! The FSM's inline `#[cfg(test)] mod tests` inside `src/engine/marker_fsm.rs`
//! already covers transition-level regressions (commits `6d23741`,
//! `46168e6`, SPRINT12 F1). This module is reserved for integration-level
//! regressions that cross FSM boundaries — e.g. tests that compare
//! against the legacy `App` behavior. Populated incrementally as
//! Task 4's App-integration pass lands.
