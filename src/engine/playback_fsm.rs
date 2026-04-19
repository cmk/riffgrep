//! Playback finite state machine.
//!
//! Formalizes the tri-state transport (`Stopped | Playing | Paused`) plus
//! the runtime reverse / loop toggles and the pending seek / restart
//! intents that today live as ad-hoc atomics on
//! [`crate::engine::playback::SourceControl`].
//!
//! This FSM is intended to become the single source of truth for
//! UI-observable transitions once Plan 07 wires it into
//! [`PlaybackEngine`](crate::engine::playback::PlaybackEngine) and the
//! TUI action handlers. In the current state it models those
//! transitions while the existing atomics remain the active
//! mixer-thread interface; the `#[allow(dead_code)]` attributes below
//! on `Input`, `MixerCommand`, and `PlaybackFsm` come off when that
//! wiring lands.
//!
//! See `doc/designs/debt-playback.md` for the reverse-path context and
//! `doc/plans/plan-2026-04-18-04.md` for this sprint's scope.

use rust_fsm::{StateMachine, StateMachineImpl};

/// Transport state. Mirrors today's
/// [`crate::engine::playback::PlaybackState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Transport {
    /// No audio playing; mixer idle.
    #[default]
    Stopped,
    /// Audio is playing.
    Playing,
    /// Audio is paused (mixer holds state but emits silence).
    Paused,
}

/// Full playback-FSM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackFsmState {
    /// Current transport state.
    pub transport: Transport,
    /// Runtime reverse toggle. When `true`, each segment is traversed
    /// end→start; composition with marker-ordered reversed segments is
    /// handled by the mixer (see `debt-playback.md`).
    pub reversed: bool,
    /// Whether the program auto-restarts when the final segment ends.
    pub loop_enabled: bool,
    /// Pending seek target in file-space frames, consumed by the mixer
    /// on the next frame boundary.
    pub pending_seek: Option<u32>,
    /// Pending restart-from-segment-0 intent, consumed by the mixer.
    pub pending_restart: bool,
}

impl Default for PlaybackFsmState {
    fn default() -> Self {
        INITIAL_STATE
    }
}

const INITIAL_STATE: PlaybackFsmState = PlaybackFsmState {
    transport: Transport::Stopped,
    reversed: false,
    loop_enabled: false,
    pending_seek: None,
    pending_restart: false,
};

/// All inputs that drive playback-FSM transitions.
///
/// The Consume* variants are dispatched by the mixer thread when it has
/// actually applied the pending intent, closing the feedback loop so the
/// FSM knows the atomic is now clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Wiring through PlaybackEngine lands in Task 3.
pub enum Input {
    /// Transition `Stopped`/`Paused` → `Playing`. A fresh start (new
    /// program) is `Stop` followed by `Play`; when already `Paused`,
    /// `Play` also resumes playback and emits `MixerCommand::Resume`.
    /// [`Input::Resume`] is the explicit Paused→Playing alias.
    Play,
    /// Transition `Playing` → `Paused`.
    Pause,
    /// Transition `Paused` → `Playing`. No-op in other transports.
    Resume,
    /// Transition any transport → `Stopped`; clears `pending_*`.
    Stop,
    /// Record a pending seek target. Valid in any transport; the mixer
    /// consumes it on the next frame boundary via [`Input::ConsumeSeek`].
    Seek(u32),
    /// Record a pending restart-from-segment-0 intent. No-op when
    /// transport is `Stopped` (matches `PlaybackEngine::restart_program`);
    /// otherwise queues the restart without changing transport. The
    /// mixer applies it on the next frame boundary via
    /// [`Input::ConsumeRestart`].
    Restart,
    /// Flip the runtime reverse toggle.
    ToggleReverse,
    /// Set the runtime reverse toggle to a specific value.
    SetReverse(bool),
    /// Flip the loop-enabled toggle.
    ToggleLoop,
    /// Set the loop-enabled toggle to a specific value.
    SetLoop(bool),
    /// Mixer signal: current segment's repetition count exhausted.
    /// State-preserving; exists so the reference model sees the same
    /// transition the SUT does.
    SegmentEnded,
    /// Mixer signal: all segments done. If `loop_enabled`, triggers
    /// `pending_restart = true`; else transitions to `Stopped`.
    ProgramEnded,
    /// Mixer signal: consumed the pending seek. Clears `pending_seek`.
    ConsumeSeek,
    /// Mixer signal: consumed the pending restart. Clears the flag
    /// when the precondition holds (`pending_restart == true` AND
    /// transport is `Playing` — the mixer is actively producing
    /// frames). Silently no-ops otherwise so property tests can
    /// dispatch freely. Transport is not changed; a real mixer-side
    /// restart happens inside the mixer thread, which only runs when
    /// transport is already `Playing`. Not for user dispatch — the
    /// mixer is the only caller.
    ConsumeRestart,
}

/// Side-effect descriptors the caller must honor (e.g. start/stop the
/// rodio sink). Pure state transitions return `None` from
/// [`StateMachineImpl::output`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Consumed by PlaybackEngine wiring (Task 3).
pub enum MixerCommand {
    /// Start the mixer (sink.play() + SegmentSource).
    Start,
    /// Stop the mixer (sink.stop()).
    Stop,
    /// Pause the sink.
    Pause,
    /// Resume the paused sink.
    Resume,
}

/// Unit marker type implementing [`StateMachineImpl`] for the playback
/// FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackMachine;

impl StateMachineImpl for PlaybackMachine {
    type Input = Input;
    type State = PlaybackFsmState;
    type Output = MixerCommand;
    const INITIAL_STATE: Self::State = INITIAL_STATE;

    fn transition(state: &Self::State, input: &Self::Input) -> Option<Self::State> {
        let mut next = *state;
        match input {
            Input::Play => {
                // Play from Stopped or Paused lands on Playing. From
                // Playing it's a no-op (idempotent).
                next.transport = Transport::Playing;
            }
            Input::Pause => {
                if matches!(state.transport, Transport::Playing) {
                    next.transport = Transport::Paused;
                }
            }
            Input::Resume => {
                if matches!(state.transport, Transport::Paused) {
                    next.transport = Transport::Playing;
                }
            }
            Input::Stop => {
                next.transport = Transport::Stopped;
                next.pending_seek = None;
                next.pending_restart = false;
            }
            Input::Seek(pos) => {
                next.pending_seek = Some(*pos);
            }
            Input::Restart => {
                // Match `PlaybackEngine::restart_program`: no-op when
                // Stopped; otherwise queue the pending_restart intent
                // and leave transport alone. A user who queues a
                // restart then pauses sees the restart fire when they
                // subsequently resume.
                if matches!(state.transport, Transport::Stopped) {
                    // no-op
                } else {
                    next.pending_restart = true;
                }
            }
            Input::ToggleReverse => {
                next.reversed = !next.reversed;
            }
            Input::SetReverse(v) => {
                next.reversed = *v;
            }
            Input::ToggleLoop => {
                next.loop_enabled = !next.loop_enabled;
            }
            Input::SetLoop(v) => {
                next.loop_enabled = *v;
            }
            Input::SegmentEnded => {
                // Pure signal; no state change at the FSM level. The mixer
                // handles segment advance locally.
            }
            Input::ProgramEnded => {
                if next.loop_enabled {
                    next.pending_restart = true;
                    // Stay Playing — the mixer will apply the restart on
                    // the next frame boundary.
                } else {
                    next.transport = Transport::Stopped;
                    // Clear any pending restart — a queued restart that
                    // never fired (e.g. user hit Ctrl-O just before the
                    // final segment ended) must not carry into the
                    // Stopped state and then spuriously fire on the next
                    // Play. Matches the spirit of Q7 (no restart
                    // queued against an idle sink).
                    next.pending_restart = false;
                    // Don't clear pending_seek here: a user-issued seek
                    // could legitimately survive a natural end. Stop
                    // clears it; ProgramEnded does not.
                }
            }
            Input::ConsumeSeek => {
                next.pending_seek = None;
            }
            Input::ConsumeRestart => {
                // Mixer-internal signal: only fires when the mixer has
                // actually started replaying from segment 0. That
                // means `pending_restart` must be set AND transport
                // must already be `Playing` (the mixer is producing
                // frames). `Paused` uses `sink.pause()` so the audio
                // path is silent — if ConsumeRestart fired there,
                // snapping transport to `Playing` would desync the UI
                // from the silent audio. A spurious dispatch is a
                // no-op rather than a panic so property tests can
                // exercise it freely.
                if state.pending_restart && matches!(state.transport, Transport::Playing) {
                    next.pending_restart = false;
                }
            }
        }
        Some(next)
    }

    fn output(state: &Self::State, input: &Self::Input) -> Option<Self::Output> {
        match input {
            Input::Play => match state.transport {
                Transport::Stopped => Some(MixerCommand::Start),
                Transport::Paused => Some(MixerCommand::Resume),
                Transport::Playing => None,
            },
            Input::Pause => match state.transport {
                Transport::Playing => Some(MixerCommand::Pause),
                _ => None,
            },
            Input::Resume => match state.transport {
                Transport::Paused => Some(MixerCommand::Resume),
                _ => None,
            },
            Input::Stop => match state.transport {
                Transport::Stopped => None,
                _ => Some(MixerCommand::Stop),
            },
            _ => None,
        }
    }
}

// ---------- App-facing wrapper ----------

/// App-facing wrapper around [`StateMachine<PlaybackMachine>`].
#[allow(dead_code)] // Consumed by PlaybackEngine wiring (Task 3).
pub struct PlaybackFsm {
    machine: StateMachine<PlaybackMachine>,
}

#[allow(dead_code)] // Consumed by PlaybackEngine wiring (Task 3).
impl PlaybackFsm {
    /// Create a fresh machine at the initial state.
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
        }
    }

    /// Create a machine pre-seeded with the given state. Useful for
    /// tests that need to start from a non-default configuration.
    pub fn from_state(state: PlaybackFsmState) -> Self {
        Self {
            machine: StateMachine::from_state(state),
        }
    }

    /// Apply an input. Returns the side-effect output, if any.
    pub fn consume(&mut self, input: Input) -> Option<MixerCommand> {
        self.machine.consume(&input).ok().flatten()
    }

    /// Current state.
    pub fn state(&self) -> &PlaybackFsmState {
        self.machine.state()
    }

    /// Legacy view of the tri-state transport.
    pub fn transport(&self) -> Transport {
        self.state().transport
    }

    /// Whether the runtime reverse toggle is on.
    pub fn reversed(&self) -> bool {
        self.state().reversed
    }

    /// Whether the program auto-restarts when the final segment ends.
    pub fn loop_enabled(&self) -> bool {
        self.state().loop_enabled
    }

    /// Pending seek target, if any.
    pub fn pending_seek(&self) -> Option<u32> {
        self.state().pending_seek
    }

    /// Whether a restart is pending.
    pub fn pending_restart(&self) -> bool {
        self.state().pending_restart
    }
}

impl Default for PlaybackFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- baseline ----------

    #[test]
    fn initial_state_is_stopped_everything_else_false() {
        let fsm = PlaybackFsm::new();
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert!(!fsm.reversed());
        assert!(!fsm.loop_enabled());
        assert_eq!(fsm.pending_seek(), None);
        assert!(!fsm.pending_restart());
    }

    // ---------- transport transitions ----------

    #[test]
    fn play_from_stopped_starts_mixer() {
        let mut fsm = PlaybackFsm::new();
        let out = fsm.consume(Input::Play);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert_eq!(out, Some(MixerCommand::Start));
    }

    #[test]
    fn play_from_paused_resumes_mixer() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Pause);
        assert_eq!(fsm.transport(), Transport::Paused);
        let out = fsm.consume(Input::Play);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert_eq!(out, Some(MixerCommand::Resume));
    }

    #[test]
    fn play_from_playing_is_idempotent_no_output() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let out = fsm.consume(Input::Play);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert_eq!(out, None);
    }

    #[test]
    fn pause_only_fires_from_playing() {
        let mut fsm = PlaybackFsm::new();
        let out = fsm.consume(Input::Pause);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert_eq!(out, None);

        let _ = fsm.consume(Input::Play);
        let out = fsm.consume(Input::Pause);
        assert_eq!(fsm.transport(), Transport::Paused);
        assert_eq!(out, Some(MixerCommand::Pause));
    }

    #[test]
    fn resume_only_fires_from_paused() {
        let mut fsm = PlaybackFsm::new();
        // From Stopped — no-op.
        let out = fsm.consume(Input::Resume);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert_eq!(out, None);

        // From Paused — resumes.
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Pause);
        let out = fsm.consume(Input::Resume);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert_eq!(out, Some(MixerCommand::Resume));
    }

    #[test]
    fn stop_clears_pending_and_transitions_to_stopped() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Seek(1000));
        let _ = fsm.consume(Input::Restart);
        assert_eq!(fsm.pending_seek(), Some(1000));
        assert!(fsm.pending_restart());

        let out = fsm.consume(Input::Stop);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert_eq!(fsm.pending_seek(), None);
        assert!(!fsm.pending_restart());
        assert_eq!(out, Some(MixerCommand::Stop));
    }

    #[test]
    fn stop_from_stopped_is_idempotent_no_output() {
        let mut fsm = PlaybackFsm::new();
        let out = fsm.consume(Input::Stop);
        assert_eq!(out, None);
    }

    // ---------- seek + restart intents ----------

    #[test]
    fn seek_records_pending_in_any_transport() {
        for initial in [Transport::Stopped, Transport::Playing, Transport::Paused] {
            let state = PlaybackFsmState {
                transport: initial,
                ..PlaybackFsmState::default()
            };
            let mut fsm = PlaybackFsm::from_state(state);
            let _ = fsm.consume(Input::Seek(42));
            assert_eq!(fsm.pending_seek(), Some(42));
            assert_eq!(fsm.transport(), initial, "seek must not change transport");
        }
    }

    #[test]
    fn consume_seek_clears_pending() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Seek(500));
        assert_eq!(fsm.pending_seek(), Some(500));
        let _ = fsm.consume(Input::ConsumeSeek);
        assert_eq!(fsm.pending_seek(), None);
    }

    #[test]
    fn restart_from_stopped_is_a_noop() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Restart);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert!(
            !fsm.pending_restart(),
            "Restart from Stopped must not queue a restart (matches PlaybackEngine::restart_program)",
        );
    }

    #[test]
    fn restart_from_paused_queues_without_changing_transport() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Pause);
        assert_eq!(fsm.transport(), Transport::Paused);
        let _ = fsm.consume(Input::Restart);
        // User's paused; the restart waits for them to resume. Mixer
        // sees pending_restart the moment it advances again.
        assert_eq!(fsm.transport(), Transport::Paused);
        assert!(fsm.pending_restart());
    }

    #[test]
    fn restart_from_playing_queues_without_changing_transport() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Restart);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert!(fsm.pending_restart());
    }

    #[test]
    fn consume_restart_clears_flag_and_ensures_playing() {
        let mut fsm = PlaybackFsm::new();
        // Restart from Stopped is a no-op, so prime Playing first.
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Restart);
        assert!(fsm.pending_restart());
        let _ = fsm.consume(Input::ConsumeRestart);
        assert!(!fsm.pending_restart());
        assert_eq!(fsm.transport(), Transport::Playing);
    }

    // ---------- reverse + loop ----------

    #[test]
    fn toggle_reverse_flips_and_is_idempotent_in_pairs() {
        let mut fsm = PlaybackFsm::new();
        assert!(!fsm.reversed());
        let _ = fsm.consume(Input::ToggleReverse);
        assert!(fsm.reversed());
        let _ = fsm.consume(Input::ToggleReverse);
        assert!(!fsm.reversed());
    }

    #[test]
    fn set_reverse_assigns_absolute_value() {
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::SetReverse(true));
        assert!(fsm.reversed());
        let _ = fsm.consume(Input::SetReverse(true));
        assert!(fsm.reversed(), "idempotent set to true");
        let _ = fsm.consume(Input::SetReverse(false));
        assert!(!fsm.reversed());
    }

    #[test]
    fn toggle_loop_flips_and_is_idempotent_in_pairs() {
        let mut fsm = PlaybackFsm::new();
        assert!(!fsm.loop_enabled());
        let _ = fsm.consume(Input::ToggleLoop);
        assert!(fsm.loop_enabled());
        let _ = fsm.consume(Input::ToggleLoop);
        assert!(!fsm.loop_enabled());
    }

    // ---------- program end semantics ----------

    #[test]
    fn program_ended_with_loop_stays_playing_and_queues_restart() {
        let state = PlaybackFsmState {
            transport: Transport::Playing,
            loop_enabled: true,
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ProgramEnded);
        assert_eq!(fsm.transport(), Transport::Playing);
        assert!(fsm.pending_restart());
    }

    #[test]
    fn program_ended_without_loop_stops() {
        let state = PlaybackFsmState {
            transport: Transport::Playing,
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ProgramEnded);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert!(!fsm.pending_restart());
    }

    #[test]
    fn program_ended_without_loop_clears_pending_restart() {
        // Regression for Copilot round-2 finding: if a Restart landed
        // just before the final segment naturally ended, the old code
        // left pending_restart=true while snapping transport=Stopped.
        // The next Play would then silently fire the stale restart.
        let state = PlaybackFsmState {
            transport: Transport::Playing,
            pending_restart: true,
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ProgramEnded);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert!(
            !fsm.pending_restart(),
            "ProgramEnded (no loop) must clear a queued restart",
        );
    }

    #[test]
    fn consume_restart_is_noop_when_no_pending() {
        // Spurious ConsumeRestart (e.g. double-fired by the mixer)
        // must not snap transport to Playing out of nowhere.
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Pause);
        let before = *fsm.state();
        let _ = fsm.consume(Input::ConsumeRestart);
        assert_eq!(
            *fsm.state(),
            before,
            "ConsumeRestart with no pending_restart is a no-op"
        );
    }

    #[test]
    fn consume_restart_is_noop_when_paused() {
        // Tightened guard (round-3 feedback on PR #20): Paused + sink
        // paused means the audio path is silent. A spurious
        // ConsumeRestart while Paused must not snap UI to Playing.
        let mut fsm = PlaybackFsm::new();
        let _ = fsm.consume(Input::Play);
        let _ = fsm.consume(Input::Pause);
        let _ = fsm.consume(Input::Restart); // queues pending_restart, stays Paused
        assert_eq!(fsm.transport(), Transport::Paused);
        assert!(fsm.pending_restart());
        let before = *fsm.state();
        let _ = fsm.consume(Input::ConsumeRestart);
        assert_eq!(
            *fsm.state(),
            before,
            "ConsumeRestart while Paused is a no-op — pending_restart stays queued \
             for the eventual Resume",
        );
    }

    #[test]
    fn consume_restart_is_noop_when_stopped() {
        // pending_restart can only arrive against a non-Stopped sink
        // (Restart is a no-op from Stopped), but an earlier ProgramEnded
        // path now clears it — make sure ConsumeRestart doesn't resurrect
        // Playing from Stopped even if some path left pending_restart
        // dangling.
        let state = PlaybackFsmState {
            transport: Transport::Stopped,
            pending_restart: true,
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ConsumeRestart);
        assert_eq!(
            fsm.transport(),
            Transport::Stopped,
            "ConsumeRestart must not resurrect Playing from Stopped",
        );
    }

    #[test]
    fn program_ended_from_paused_without_loop_stops() {
        // Natural program end while paused: mixer finishes its last
        // segment and signals ProgramEnded. Without loop, transport
        // collapses to Stopped regardless of prior Paused state.
        let state = PlaybackFsmState {
            transport: Transport::Paused,
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ProgramEnded);
        assert_eq!(fsm.transport(), Transport::Stopped);
        assert!(!fsm.pending_restart());
    }

    #[test]
    fn program_ended_preserves_pending_seek_but_stop_clears() {
        let state = PlaybackFsmState {
            transport: Transport::Playing,
            pending_seek: Some(1000),
            ..PlaybackFsmState::default()
        };
        let mut fsm = PlaybackFsm::from_state(state);
        let _ = fsm.consume(Input::ProgramEnded);
        // Natural end keeps a user-issued seek alive (consumable when
        // the user next plays). Stop is the sledgehammer that clears it.
        assert_eq!(fsm.pending_seek(), Some(1000));

        let _ = fsm.consume(Input::Stop);
        assert_eq!(fsm.pending_seek(), None);
    }
}
