//! Unit-level regressions for the playback FSM.
//!
//! Today this module is a placeholder: the FSM's own inline `#[cfg(test)]`
//! block in `src/engine/playback_fsm.rs` already covers the transport
//! transitions, Stop's pending-clear, the Q7 Paused∧pending_restart
//! invariant, and the ProgramEnded-with-loop / without-loop branches.
//!
//! Plan 07 (the reverse-path unification sprint) will populate this
//! file with `SegmentSource` regressions for reverse-mode seek,
//! restart, and crossfade-boundary cases.
