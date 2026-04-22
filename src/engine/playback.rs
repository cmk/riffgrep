//! Audio playback engine using symphonia for decoding and rodio for output.
//!
//! Segment boundaries, looping, and crossfades are enforced at the sample level
//! by `SegmentSource`, eliminating pops from rodio's mixer buffering.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{OutputStream, OutputStreamHandle, Sink, Source};

use crate::engine::playback_fsm::{Input as FsmInput, MixerCommand, PlaybackFsm, Transport};

/// Playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// No audio playing.
    Stopped,
    /// Audio is playing.
    Playing,
    /// Audio is paused.
    Paused,
}

impl PlaybackState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => PlaybackState::Playing,
            2 => PlaybackState::Paused,
            _ => PlaybackState::Stopped,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            PlaybackState::Stopped => 0,
            PlaybackState::Playing => 1,
            PlaybackState::Paused => 2,
        }
    }
}

/// Grace period (ms) after rodio reports sink empty before transitioning to
/// Stopped. Prevents audible click from premature buffer teardown.
const DRAIN_GRACE_MS: u64 = 150;

// ---------------------------------------------------------------------------
// SegmentSource — sample-level boundary enforcement
// ---------------------------------------------------------------------------

/// Sentinel for "no pending seek".
const NO_SEEK: u32 = u32::MAX;

/// Crossfade duration in frames (~1.3ms at 48kHz).
const CROSSFADE_FRAMES: u32 = 64;

/// Shared state between the `SegmentSource` (mixer thread) and the UI.
pub struct SourceControl {
    /// Current frame position in buffer space (written by source, read by UI).
    pub frame: AtomicU32,
    /// Pending seek target frame (written by UI, consumed by source).
    pub pending_seek: AtomicU32,
    /// Whether looping is enabled: gates both per-segment infinite reps and
    /// global playlist restart. Written by UI toggle, read by source per frame.
    pub loop_enabled: AtomicBool,
    /// Restart the program from segment 0. Written by UI, consumed by source.
    pub pending_restart: AtomicBool,
    /// Global reverse flag. Toggled by the ReversePlayback TUI action.
    ///
    /// Effective per-segment direction is `segment.reversed ^ global_reversed`,
    /// computed by the source on every frame against the current segment.
    /// Reversed segments (i.e. marker-ordered "reverse" entries in
    /// `play_with_segments`) carry their own `reversed: true` on the
    /// `PlaySegment`; XORing with this atomic lets the two compose
    /// (reversed segment + global reverse = forward playback).
    pub reversed: AtomicBool,
}

impl SourceControl {
    fn new() -> Self {
        Self {
            frame: AtomicU32::new(0),
            pending_seek: AtomicU32::new(NO_SEEK),
            // Default false: callers (play / play_with_segments) set this explicitly.
            // A true default causes reps=1 segments to restart the program forever.
            loop_enabled: AtomicBool::new(false),
            pending_restart: AtomicBool::new(false),
            reversed: AtomicBool::new(false),
        }
    }
}

/// A segment in the playback program.
#[derive(Clone)]
struct PlaySegment {
    /// Start frame (inclusive), in buffer space.
    start: u32,
    /// End frame (exclusive), in buffer space.
    end: u32,
    /// Repetitions: 1 = play once, 2+ = repeat, 255 = infinite loop.
    reps: u8,
    /// When true, the segment is traversed end→start rather than start→end.
    /// The effective direction at runtime is
    /// `reversed ^ SourceControl::reversed` so the global-reverse toggle can
    /// flip a reversed segment back to forward playback (XOR identity).
    reversed: bool,
}

/// Pre-decoded audio buffer with segment-aware, pop-free playback.
///
/// Handles segment boundaries, looping, and program advance entirely at the
/// sample level on the mixer thread. Discontinuities (loop-back, seek, segment
/// advance) are smoothed with matched fade-out / fade-in ramps.
struct SegmentSource {
    /// Pre-decoded interleaved f32 samples (normalized to -1.0..1.0).
    buffer: Vec<f32>,
    channels: u16,
    rate: u32,
    total_frames: u32,

    /// Current interleaved sample index.
    pos: usize,
    /// Which channel within the current frame (0..channels).
    channel: u16,

    /// Program playlist (immutable once playback starts).
    playlist: Vec<PlaySegment>,
    /// Current segment index in playlist.
    seg_idx: usize,
    /// Remaining reps for current segment (255 = infinite).
    reps_left: u8,

    /// Fade-out frames remaining before a boundary (counts down to 0).
    fade_out: u32,
    /// Fade-in frames remaining after a jump (counts down to 0).
    fade_in: u32,

    /// Reverse-traversal sentinel: set by the iterator when stepping back
    /// would underflow past `seg.start * channels`. `on_frame_boundary`
    /// treats this as equivalent to `past_boundary`, which lets us emit
    /// `frame == seg.start` inclusively without losing it to the boundary
    /// check. Cleared the moment past-boundary logic runs.
    past_reverse_start: bool,

    /// Shared control for UI communication.
    control: Arc<SourceControl>,
}

impl SegmentSource {
    /// Current frame index.
    fn frame(&self) -> u32 {
        (self.pos / self.channels as usize) as u32
    }

    /// Jump to a frame, applying a short fade-in.
    fn jump_to(&mut self, frame: u32) {
        self.pos = frame as usize * self.channels as usize;
        self.channel = 0;
        self.fade_in = CROSSFADE_FRAMES;
        self.fade_out = 0;
        self.past_reverse_start = false;
        self.control.frame.store(frame, Ordering::Relaxed);
    }

    /// Whether the current segment will loop (infinite or finite reps > 1).
    /// Infinite reps (255) are additionally gated on `control.loop_enabled`.
    fn will_loop(&self) -> bool {
        if self.reps_left == 255 {
            self.control.loop_enabled.load(Ordering::Relaxed)
        } else {
            self.reps_left > 1
        }
    }

    /// Effective traversal direction for a given segment index.
    ///
    /// `segment.reversed ^ global_reversed`. Called from the boundary
    /// logic and the iterator step-back path; keeping it in one helper
    /// keeps the XOR identity in one place.
    fn effective_reversed(&self, seg: &PlaySegment) -> bool {
        seg.reversed ^ self.control.reversed.load(Ordering::Relaxed)
    }

    /// Process frame-boundary logic. Returns `false` if the source should end.
    fn on_frame_boundary(&mut self) -> bool {
        // 0. Pending program restart: reset seg_idx and jump to playlist[0].
        if self.control.pending_restart.swap(false, Ordering::Relaxed) {
            self.seg_idx = 0;
            if let Some(first) = self.playlist.first().cloned() {
                self.reps_left = first.reps;
                let origin = if self.effective_reversed(&first) {
                    first.end.saturating_sub(1)
                } else {
                    first.start
                };
                self.jump_to(origin);
            }
            return true;
        }

        // 1. Pending seek from UI (user scrub / marker jump).
        let seek = self.control.pending_seek.swap(NO_SEEK, Ordering::Relaxed);
        if seek != NO_SEEK {
            let target = seek.min(self.total_frames);
            self.jump_to(target);
            return true;
        }

        // 2. Segment boundary logic.
        if let Some(seg) = self.playlist.get(self.seg_idx).cloned() {
            let frame = self.frame();
            let rev = self.effective_reversed(&seg);

            // In reverse mode, traversal runs seg.end-1 → seg.start, so the
            // "playback origin" is seg.end-1 and the "boundary" is seg.start.
            let (origin, boundary) = if rev {
                (seg.end.saturating_sub(1), seg.start)
            } else {
                (seg.start, seg.end)
            };

            // 2a. Start fade-out before segment boundary for loops.
            let seg_len = seg.end.saturating_sub(seg.start);
            let fade_len = CROSSFADE_FRAMES.min(seg_len);
            let at_boundary = if rev {
                frame <= boundary.saturating_add(fade_len) && frame > boundary
            } else {
                frame >= boundary.saturating_sub(fade_len) && frame < boundary
            };
            let past_boundary = if rev {
                // Exclusive below-start mirrors forward's exclusive-end:
                // frame==start is still inside the segment and gets emitted.
                // The iterator's step-back sets `past_reverse_start` when it
                // would underflow past seg.start (the only way past-boundary
                // can be reached at seg.start == 0), so we treat that as a
                // one-shot equivalent to past_boundary.
                frame < boundary || self.past_reverse_start
            } else {
                frame >= boundary
            };

            if self.fade_out == 0 && self.will_loop() && at_boundary {
                self.fade_out = if rev {
                    frame.saturating_sub(boundary)
                } else {
                    boundary.saturating_sub(frame)
                };
            }

            // 2b. At segment boundary: handle loop or advance.
            if past_boundary {
                self.past_reverse_start = false;
                self.fade_out = 0;
                let loop_en = self.control.loop_enabled.load(Ordering::Relaxed);

                if self.reps_left == 255 && loop_en {
                    // Infinite loop: jump back to origin with fade-in.
                    self.pos = origin as usize * self.channels as usize;
                    self.channel = 0;
                    self.fade_in = fade_len;
                } else if self.reps_left > 1 && self.reps_left != 255 {
                    // Finite reps: always honored regardless of loop_enabled.
                    self.reps_left -= 1;
                    self.pos = origin as usize * self.channels as usize;
                    self.channel = 0;
                    self.fade_in = fade_len;
                } else {
                    // Reps exhausted (or infinite with loop disabled): advance.
                    self.seg_idx += 1;
                    if self.seg_idx >= self.playlist.len() {
                        if loop_en {
                            self.seg_idx = 0;
                        } else {
                            self.control.frame.store(self.frame(), Ordering::Relaxed);
                            return false; // Program complete.
                        }
                    }
                    let next = self.playlist[self.seg_idx].clone();
                    self.reps_left = next.reps;
                    let next_origin = if self.effective_reversed(&next) {
                        next.end.saturating_sub(1)
                    } else {
                        next.start
                    };
                    if self.frame() == next_origin {
                        // Continuous: no seek.
                    } else {
                        self.jump_to(next_origin);
                    }
                }
            }
        }

        // 3. Update UI position — buffer frame is the logical frame (Plan 07
        // unified the two; reversed segments no longer live in an appended
        // scratch region).
        self.control.frame.store(self.frame(), Ordering::Relaxed);
        true
    }
}

impl Iterator for SegmentSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // Frame boundary logic runs once per frame (on channel 0).
        if self.channel == 0 && !self.on_frame_boundary() {
            return None;
        }

        if self.pos >= self.buffer.len() {
            return None;
        }

        let mut sample = self.buffer[self.pos];

        // Fade-out before a loop boundary (decreasing gain → 0).
        if self.fade_out > 0 {
            let t = self.fade_out as f32 / CROSSFADE_FRAMES as f32;
            sample *= t;
            if self.channel == self.channels - 1 {
                self.fade_out -= 1;
            }
        }

        // Fade-in after a jump (increasing gain from 0 → 1).
        if self.fade_in > 0 {
            let t = 1.0 - (self.fade_in as f32 / CROSSFADE_FRAMES as f32);
            sample *= t;
            if self.channel == self.channels - 1 {
                self.fade_in -= 1;
            }
        }

        self.pos += 1;
        self.channel += 1;
        if self.channel >= self.channels {
            self.channel = 0;

            // Reverse traversal: we just finished emitting frame N (pos now
            // points to frame N+1). Step back 2 frames to land on frame N-1.
            // Channels within a frame are still emitted L→R. The effective
            // direction is `segment.reversed ^ global_reversed`.
            let eff_rev = self
                .playlist
                .get(self.seg_idx)
                .map(|s| s.reversed)
                .unwrap_or(false)
                ^ self.control.reversed.load(Ordering::Relaxed);
            if eff_rev {
                let stride = 2 * self.channels as usize;
                if self.pos >= stride {
                    self.pos -= stride;
                } else {
                    // Reached the start of the segment — the sample we just
                    // emitted was frame == seg.start. Park pos at seg.start
                    // (so buffer[pos] stays in-range if next() is polled
                    // before on_frame_boundary runs) and flag the boundary
                    // for on_frame_boundary to pick up.
                    if let Some(seg) = self.playlist.get(self.seg_idx) {
                        self.pos = seg.start as usize * self.channels as usize;
                    }
                    self.past_reverse_start = true;
                }
            }
        }

        Some(sample)
    }
}

impl rodio::Source for SegmentSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let frame = (pos.as_secs_f64() * self.rate as f64) as u32;
        self.jump_to(frame.min(self.total_frames));
        Ok(())
    }
}

/// Pre-decode a WAV file to interleaved f32 samples.
///
/// Returns `(samples, channels, sample_rate)`.
fn pre_decode(path: &Path) -> Result<(Vec<f32>, u16, u32), anyhow::Error> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let decoder = rodio::Decoder::new(reader).map_err(|e| anyhow::anyhow!("decode error: {e}"))?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.map(|s| s as f32 / 32768.0).collect();
    Ok((samples, channels, sample_rate))
}

// ---------------------------------------------------------------------------
// PlaybackEngine
// ---------------------------------------------------------------------------

/// Thread-safe audio playback engine.
///
/// Holds the audio output stream and provides play/pause/stop controls.
/// All methods are safe to call from any thread.
///
/// UI-observable state is owned by `fsm` (a [`PlaybackFsm`]). Each mutator
/// dispatches an [`FsmInput`], mirrors the resulting FSM state to the
/// mixer-thread atomics on [`SourceControl`], then applies any
/// [`MixerCommand`] side effects returned by the FSM. The `state` atomic
/// is a lock-free mirror of `FsmState::transport` for fast reads by
/// [`PlaybackEngine::state`].
pub struct PlaybackEngine {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Arc<Mutex<Option<Sink>>>,
    state: Arc<AtomicU8>,
    /// UI-side source of truth for transport and pending intents.
    fsm: Arc<Mutex<PlaybackFsm>>,
    play_start: Arc<Mutex<Option<Instant>>>,
    paused_elapsed: Arc<Mutex<Duration>>,
    current_path: Arc<Mutex<Option<PathBuf>>>,
    duration: Arc<Mutex<Option<Duration>>>,
    /// Instant when sink.empty() first returned true. Used for drain grace period.
    drain_start: Arc<Mutex<Option<Instant>>>,
    /// Current playback position in frames (integer-precise).
    sample_offset: Arc<Mutex<u32>>,
    /// Total number of frames in the current file.
    total_samples: Arc<Mutex<u32>>,
    /// Sample rate of the current file in Hz.
    sample_rate_hz: Arc<Mutex<u32>>,
    /// Shared control for the active SegmentSource (None when stopped).
    source_control: Arc<Mutex<Option<Arc<SourceControl>>>>,
}

impl PlaybackEngine {
    /// Try to create a new playback engine. Returns Err if no audio device.
    pub fn try_new() -> Result<Self, anyhow::Error> {
        let (stream, stream_handle) =
            OutputStream::try_default().map_err(|e| anyhow::anyhow!("no audio device: {e}"))?;

        Ok(Self {
            _stream: stream,
            stream_handle,
            sink: Arc::new(Mutex::new(None)),
            state: Arc::new(AtomicU8::new(PlaybackState::Stopped.to_u8())),
            fsm: Arc::new(Mutex::new(PlaybackFsm::new())),
            play_start: Arc::new(Mutex::new(None)),
            paused_elapsed: Arc::new(Mutex::new(Duration::ZERO)),
            current_path: Arc::new(Mutex::new(None)),
            duration: Arc::new(Mutex::new(None)),
            drain_start: Arc::new(Mutex::new(None)),
            sample_offset: Arc::new(Mutex::new(0)),
            total_samples: Arc::new(Mutex::new(0)),
            sample_rate_hz: Arc::new(Mutex::new(0)),
            source_control: Arc::new(Mutex::new(None)),
        })
    }

    /// Mirror the FSM state into the mixer-thread atomics on
    /// `SourceControl`, and into the lock-free `state` atomic.
    ///
    /// Every UI-side FSM dispatch ends with a mirror call so the mixer
    /// thread's view of pending intents and flags is kept in sync.
    fn mirror_fsm_to_atomics(&self) {
        let state = *self.fsm.lock().expect("playback lock poisoned").state();
        let transport = match state.transport {
            Transport::Stopped => PlaybackState::Stopped,
            Transport::Playing => PlaybackState::Playing,
            Transport::Paused => PlaybackState::Paused,
        };
        self.state.store(transport.to_u8(), Ordering::Relaxed);
        if let Some(ref ctl) = *self.source_control.lock().expect("playback lock poisoned") {
            ctl.reversed.store(state.reversed, Ordering::Relaxed);
            ctl.loop_enabled
                .store(state.loop_enabled, Ordering::Relaxed);
            ctl.pending_seek
                .store(state.pending_seek.unwrap_or(NO_SEEK), Ordering::Relaxed);
            ctl.pending_restart
                .store(state.pending_restart, Ordering::Relaxed);
        }
    }

    /// Observe the mixer-thread atomics and close the loop by dispatching
    /// [`FsmInput::ConsumeSeek`] / [`FsmInput::ConsumeRestart`] back into
    /// the FSM when the mixer has drained an intent. Called from the TUI
    /// tick via [`PlaybackEngine::update_sample_offset`].
    fn sync_consumed_intents(&self) {
        let Some(ctl) = self
            .source_control
            .lock()
            .expect("playback lock poisoned")
            .clone()
        else {
            return;
        };
        let mut fsm = self.fsm.lock().expect("playback lock poisoned");
        if fsm.pending_seek().is_some() && ctl.pending_seek.load(Ordering::Relaxed) == NO_SEEK {
            let _ = fsm.consume(FsmInput::ConsumeSeek);
        }
        if fsm.pending_restart() && !ctl.pending_restart.load(Ordering::Relaxed) {
            let _ = fsm.consume(FsmInput::ConsumeRestart);
        }
    }

    /// Start playing a WAV file from the beginning. Stops any current playback.
    ///
    /// Uses a `SegmentSource` with a single segment spanning the entire file
    /// (no boundaries, no looping).
    pub fn play(&self, path: &Path) -> Result<(), anyhow::Error> {
        self.stop();

        let (samples, channels, sample_rate) = pre_decode(path)?;
        let total_frames = samples.len() as u32 / channels as u32;

        let control = Arc::new(SourceControl::new());
        let source = SegmentSource {
            buffer: samples,
            channels,
            rate: sample_rate,
            total_frames,
            pos: 0,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 0,
                end: total_frames,
                reps: 1,
                reversed: false,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| anyhow::anyhow!("audio output error: {e}"))?;
        sink.append(source);

        // Compute duration from pre-decoded data.
        let dur = Duration::from_secs_f64(total_frames as f64 / sample_rate as f64);

        *self.sink.lock().expect("playback lock poisoned") = Some(sink);
        *self.source_control.lock().expect("playback lock poisoned") = Some(control);
        *self.duration.lock().expect("playback lock poisoned") = Some(dur);
        *self.total_samples.lock().expect("playback lock poisoned") = total_frames;
        *self.sample_rate_hz.lock().expect("playback lock poisoned") = sample_rate;
        *self.play_start.lock().expect("playback lock poisoned") = Some(Instant::now());
        *self.paused_elapsed.lock().expect("playback lock poisoned") = Duration::ZERO;
        *self.current_path.lock().expect("playback lock poisoned") = Some(path.to_path_buf());
        *self.drain_start.lock().expect("playback lock poisoned") = None;
        *self.sample_offset.lock().expect("playback lock poisoned") = 0;

        // Dispatch through the FSM: Stop clears transport & pending, Play
        // transitions Stopped → Playing. `play()` is always single-shot so
        // SetLoop(false) is included explicitly for symmetry with
        // `play_with_segments`, which sets it per `global_loop`.
        {
            let mut fsm = self.fsm.lock().expect("playback lock poisoned");
            let _ = fsm.consume(FsmInput::SetLoop(false));
            let _ = fsm.consume(FsmInput::Play);
        }
        self.mirror_fsm_to_atomics();

        Ok(())
    }

    /// Start segment-based playback with a program playlist.
    ///
    /// Each entry is `(start_frame, end_frame, reps, reverse)` where:
    /// - `reps`: 1 = play once, 2+ = repeat, 15 = infinite loop
    /// - `reverse`: when true, the segment is traversed end→start at runtime.
    ///   The caller may pass either `start > end` or `start < end`; the
    ///   frame range is normalized and stored forward-oriented with
    ///   `reversed = true` on the [`PlaySegment`].
    ///
    /// The effective per-segment direction is `segment.reversed ^
    /// SourceControl::reversed` (the global reverse toggle), so a reversed
    /// segment flips back to forward playback when the user toggles global
    /// reverse. `global_loop` restarts the entire playlist when all segments
    /// complete. Boundaries, looping, and crossfades are handled at the
    /// sample level — no pops.
    pub fn play_with_segments(
        &self,
        path: &Path,
        playlist: &[(u32, u32, u8, bool)],
        global_loop: bool,
    ) -> Result<(), anyhow::Error> {
        self.stop();

        let (samples, channels, sample_rate) = pre_decode(path)?;
        let total_frames = samples.len() as u32 / channels as u32;

        let mut segments: Vec<PlaySegment> = Vec::with_capacity(playlist.len());
        for &(start, end, reps, reverse) in playlist {
            let reps = if reps == 15 { 255 } else { reps };
            // Normalize the frame range to forward-oriented [lo, hi) — the
            // caller may pass start > end for reversed entries. The traversal
            // direction is stored on PlaySegment.reversed.
            let (lo, hi) = if start <= end {
                (start, end.min(total_frames))
            } else {
                (end, start.min(total_frames))
            };
            if lo >= hi {
                // Degenerate segment (empty range) — skip.
                continue;
            }
            segments.push(PlaySegment {
                start: lo,
                end: hi,
                reps,
                reversed: reverse,
            });
        }

        let first = segments.first().cloned();
        let first_reps = first.as_ref().map(|s| s.reps).unwrap_or(1);
        // Entry position depends on the first segment's *effective* direction:
        // `seg.reversed ^ fsm.reversed`. `Input::Stop` (fired by the
        // `self.stop()` call above) does not clear the FSM's reverse flag,
        // so a user who toggled global-reverse and then called us would
        // otherwise enter at `seg.start` while the mixer's effective
        // direction is reverse — the first frame boundary would immediately
        // fire `past_reverse_start` and skip the whole segment.
        let fsm_reversed = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .state()
            .reversed;
        let first_pos_frame = match &first {
            Some(seg) if seg.reversed ^ fsm_reversed => seg.end.saturating_sub(1),
            Some(seg) => seg.start,
            None => 0,
        };
        // UI's sample_offset tracks the mixer's actual entry frame so the
        // TUI cursor lines up with the first emitted sample.
        let first_start_for_offset = first_pos_frame;

        let control = Arc::new(SourceControl::new());
        control.frame.store(first_pos_frame, Ordering::Relaxed);

        let source = SegmentSource {
            buffer: samples,
            channels,
            rate: sample_rate,
            total_frames,
            pos: first_pos_frame as usize * channels as usize,
            channel: 0,
            playlist: segments,
            seg_idx: 0,
            reps_left: first_reps,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| anyhow::anyhow!("audio output error: {e}"))?;
        sink.append(source);

        let dur = Duration::from_secs_f64(total_frames as f64 / sample_rate as f64);

        *self.sink.lock().expect("playback lock poisoned") = Some(sink);
        *self.source_control.lock().expect("playback lock poisoned") = Some(control);
        *self.duration.lock().expect("playback lock poisoned") = Some(dur);
        *self.total_samples.lock().expect("playback lock poisoned") = total_frames;
        *self.sample_rate_hz.lock().expect("playback lock poisoned") = sample_rate;
        *self.play_start.lock().expect("playback lock poisoned") = Some(Instant::now());
        *self.paused_elapsed.lock().expect("playback lock poisoned") = Duration::ZERO;
        *self.current_path.lock().expect("playback lock poisoned") = Some(path.to_path_buf());
        *self.drain_start.lock().expect("playback lock poisoned") = None;
        *self.sample_offset.lock().expect("playback lock poisoned") = first_start_for_offset;

        // Dispatch through the FSM: Stop has already cleared state via the
        // stop() call above. Set loop to the requested value, then Play.
        {
            let mut fsm = self.fsm.lock().expect("playback lock poisoned");
            let _ = fsm.consume(FsmInput::SetLoop(global_loop));
            let _ = fsm.consume(FsmInput::Play);
        }
        self.mirror_fsm_to_atomics();

        Ok(())
    }

    /// Toggle pause/resume. If stopped, this is a no-op.
    pub fn toggle_pause(&self) {
        let cmd = {
            let mut fsm = self.fsm.lock().expect("playback lock poisoned");
            match fsm.transport() {
                Transport::Playing => fsm.consume(FsmInput::Pause),
                Transport::Paused => fsm.consume(FsmInput::Resume),
                Transport::Stopped => return,
            }
        };
        match cmd {
            Some(MixerCommand::Pause) => {
                if let Some(ref sink) = *self.sink.lock().expect("playback lock poisoned") {
                    sink.pause();
                }
                let elapsed = self.compute_elapsed();
                *self.paused_elapsed.lock().expect("playback lock poisoned") = elapsed;
                *self.play_start.lock().expect("playback lock poisoned") = None;
            }
            Some(MixerCommand::Resume) => {
                if let Some(ref sink) = *self.sink.lock().expect("playback lock poisoned") {
                    sink.play();
                }
                *self.play_start.lock().expect("playback lock poisoned") = Some(Instant::now());
            }
            _ => {}
        }
        self.mirror_fsm_to_atomics();
    }

    /// Stop playback and reset state.
    pub fn stop(&self) {
        let cmd = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .consume(FsmInput::Stop);
        if matches!(cmd, Some(MixerCommand::Stop))
            && let Some(sink) = self.sink.lock().expect("playback lock poisoned").take()
        {
            sink.stop();
        }
        *self.source_control.lock().expect("playback lock poisoned") = None;
        *self.play_start.lock().expect("playback lock poisoned") = None;
        *self.paused_elapsed.lock().expect("playback lock poisoned") = Duration::ZERO;
        *self.current_path.lock().expect("playback lock poisoned") = None;
        *self.duration.lock().expect("playback lock poisoned") = None;
        *self.drain_start.lock().expect("playback lock poisoned") = None;
        *self.sample_offset.lock().expect("playback lock poisoned") = 0;
        // Mirror runs after source_control is None — it updates the state
        // atomic but the control-mirror short-circuits cleanly.
        self.mirror_fsm_to_atomics();
    }

    /// Get the current playback state.
    ///
    /// When the rodio sink reports empty, a 150ms grace period allows the
    /// audio output buffer to fully drain before transitioning to Stopped.
    pub fn state(&self) -> PlaybackState {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state == PlaybackState::Playing
            && let Some(ref sink) = *self.sink.lock().expect("playback lock poisoned")
            && sink.empty()
        {
            let mut drain = self.drain_start.lock().expect("playback lock poisoned");
            if drain.is_none() {
                *drain = Some(Instant::now());
                return PlaybackState::Playing;
            }
            if drain.unwrap().elapsed() < Duration::from_millis(DRAIN_GRACE_MS) {
                return PlaybackState::Playing;
            }
            // Grace period elapsed — transition to Stopped. Dispatch
            // ProgramEnded to the FSM so it sees the natural end (which
            // clears any queued pending_restart and lands transport =
            // Stopped when loop_enabled is false). Drop `drain` first so
            // the FSM lock ordering stays sink → drain → fsm.
            *drain = None;
            drop(drain);
            let _ = self
                .fsm
                .lock()
                .expect("playback lock poisoned")
                .consume(FsmInput::ProgramEnded);
            self.mirror_fsm_to_atomics();
            return PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        }
        state
    }

    /// Elapsed playback time since play started.
    pub fn elapsed(&self) -> Duration {
        self.compute_elapsed()
    }

    /// Total duration of the current track (if known).
    pub fn duration(&self) -> Option<Duration> {
        *self.duration.lock().expect("playback lock poisoned")
    }

    /// Path of the currently loaded file.
    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_path
            .lock()
            .expect("playback lock poisoned")
            .clone()
    }

    /// Current playback position in frames.
    pub fn sample_offset(&self) -> u32 {
        *self.sample_offset.lock().expect("playback lock poisoned")
    }

    /// Total number of frames in the current file.
    pub fn total_samples(&self) -> u32 {
        *self.total_samples.lock().expect("playback lock poisoned")
    }

    /// Sample rate of the current file in Hz.
    pub fn sample_rate(&self) -> u32 {
        *self.sample_rate_hz.lock().expect("playback lock poisoned")
    }

    /// Playback position as a fraction 0.0–1.0 (derived from sample_offset / total_samples).
    pub fn position_fraction(&self) -> f32 {
        let offset = *self.sample_offset.lock().expect("playback lock poisoned");
        let total = *self.total_samples.lock().expect("playback lock poisoned");
        if total == 0 {
            return 0.0;
        }
        (offset as f32 / total as f32).clamp(0.0, 1.0)
    }

    /// Seek to an absolute sample offset. Clamped to `[0, total_samples]`.
    ///
    /// No-op when stopped. Seeking while paused updates position without resuming.
    pub fn seek_to_sample(&self, target: u32) -> Result<(), anyhow::Error> {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state == PlaybackState::Stopped {
            return Ok(());
        }

        let total = self.total_samples();
        let clamped = target.min(total);
        let rate = self.sample_rate();
        if rate == 0 {
            return Ok(());
        }

        // Update the frame snapshot immediately for UI responsiveness; the
        // pending_seek atomic is mirrored from the FSM below.
        if let Some(ref ctl) = *self.source_control.lock().expect("playback lock poisoned") {
            ctl.frame.store(clamped, Ordering::Relaxed);
        }
        *self.sample_offset.lock().expect("playback lock poisoned") = clamped;

        // Reset elapsed tracking to match the new position.
        let duration = Duration::from_secs_f64(clamped as f64 / rate as f64);
        *self.paused_elapsed.lock().expect("playback lock poisoned") = duration;
        if state == PlaybackState::Playing {
            *self.play_start.lock().expect("playback lock poisoned") = Some(Instant::now());
        }
        // Clear drain_start (seeking restarts drain detection).
        *self.drain_start.lock().expect("playback lock poisoned") = None;

        let _ = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .consume(FsmInput::Seek(clamped));
        self.mirror_fsm_to_atomics();

        Ok(())
    }

    /// Set playback volume (linear scale, 0.0 = silence, 1.0 = unity, >1.0 = amplified).
    ///
    /// Applied to the active sink. No-op when stopped.
    pub fn set_volume(&self, linear: f32) {
        if let Some(ref sink) = *self.sink.lock().expect("playback lock poisoned") {
            sink.set_volume(linear);
        }
    }

    /// Set playback speed multiplier (1.0 = normal, 0.5 = half, 2.0 = double).
    ///
    /// Applied to the active sink via rodio's speed filter. No-op when stopped.
    pub fn set_speed(&self, ratio: f32) {
        if let Some(ref sink) = *self.sink.lock().expect("playback lock poisoned") {
            sink.set_speed(ratio);
        }
    }

    /// Enable or disable looping on the currently active source. Gates
    /// both infinite per-segment reps (`reps == 255`) and global playlist
    /// restart. No-op when stopped (FSM retains the flag for the next play).
    pub fn set_loop_enabled(&self, enabled: bool) {
        let _ = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .consume(FsmInput::SetLoop(enabled));
        self.mirror_fsm_to_atomics();
    }

    /// Set the global reverse flag. The effective per-segment direction is
    /// `segment.reversed ^ global_reversed`; see Plan 07. Safe to call in
    /// any transport — the FSM mirrors the flag to
    /// [`SourceControl::reversed`] so the next frame picks it up.
    pub fn set_reversed(&self, reversed: bool) {
        let _ = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .consume(FsmInput::SetReverse(reversed));
        self.mirror_fsm_to_atomics();
    }

    /// Restart the active program from segment 0.
    ///
    /// Unlike `seek_to_sample(0)`, this resets `seg_idx` inside the source
    /// and, for reversed first segments, restarts from `seg.end - 1` rather
    /// than `seg.start`. No-op when stopped (matches the FSM's `Restart`
    /// transition rule).
    #[allow(dead_code)] // Reserved for program segment restart.
    pub fn restart_program(&self) {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state == PlaybackState::Stopped {
            return;
        }
        *self.sample_offset.lock().expect("playback lock poisoned") = 0;
        *self.paused_elapsed.lock().expect("playback lock poisoned") = Duration::ZERO;
        if state == PlaybackState::Playing {
            *self.play_start.lock().expect("playback lock poisoned") = Some(Instant::now());
        }
        *self.drain_start.lock().expect("playback lock poisoned") = None;

        let _ = self
            .fsm
            .lock()
            .expect("playback lock poisoned")
            .consume(FsmInput::Restart);
        self.mirror_fsm_to_atomics();
    }

    /// Seek relative to current position in seconds.
    ///
    /// Converts delta to samples via sample_rate. Positive = forward, negative =
    /// backward. Clamped to `[0, total_samples]`. No-op when stopped.
    pub fn seek_relative(&self, delta_secs: f64) -> Result<(), anyhow::Error> {
        let rate = self.sample_rate();
        if rate == 0 {
            return Ok(());
        }
        let delta_samples = (delta_secs * rate as f64) as i64;
        let current = self.sample_offset() as i64;
        let total = self.total_samples() as i64;
        let target = (current + delta_samples).clamp(0, total) as u32;
        self.seek_to_sample(target)
    }

    /// Sync sample_offset from the source's authoritative frame position,
    /// and close the loop on pending-intent atomics so the FSM matches the
    /// mixer's view.
    ///
    /// Called from the TUI tick loop. Reads from the [`SourceControl`]
    /// atomic on the mixer thread — no wall-clock drift. After the frame
    /// snapshot, any `pending_seek` or `pending_restart` the mixer has
    /// already drained is reflected back into the FSM via
    /// [`FsmInput::ConsumeSeek`] / [`FsmInput::ConsumeRestart`].
    pub fn update_sample_offset(&self) {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state != PlaybackState::Playing {
            return;
        }
        if let Some(ref ctl) = *self.source_control.lock().expect("playback lock poisoned") {
            let frame = ctl.frame.load(Ordering::Relaxed);
            *self.sample_offset.lock().expect("playback lock poisoned") = frame;
        }
        self.sync_consumed_intents();
    }

    fn compute_elapsed(&self) -> Duration {
        let paused = *self.paused_elapsed.lock().expect("playback lock poisoned");
        match *self.play_start.lock().expect("playback lock poisoned") {
            Some(start) => paused + start.elapsed(),
            None => paused,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
impl PlaybackEngine {
    /// Test helper: read drain_start value.
    pub fn test_drain_start(&self) -> Option<Instant> {
        *self.drain_start.lock().expect("playback lock poisoned")
    }

    /// Test helper: set drain_start directly.
    pub fn test_set_drain_start(&self, v: Option<Instant>) {
        *self.drain_start.lock().expect("playback lock poisoned") = v;
    }

    /// Test helper: create an empty sink (no audio appended = immediately empty).
    pub fn test_create_empty_sink(&self) {
        if let Ok(sink) = Sink::try_new(&self.stream_handle) {
            *self.sink.lock().expect("playback lock poisoned") = Some(sink);
        }
    }

    /// Test helper: set sample position fields directly.
    pub fn test_set_sample_position(&self, offset: u32, total: u32) {
        *self.sample_offset.lock().expect("playback lock poisoned") = offset;
        *self.total_samples.lock().expect("playback lock poisoned") = total;
    }

    /// Test helper: set sample rate directly.
    pub fn test_set_sample_rate(&self, rate: u32) {
        *self.sample_rate_hz.lock().expect("playback lock poisoned") = rate;
    }

    /// Test helper: set internal state directly (Playing/Paused/Stopped).
    #[allow(dead_code)]
    pub fn test_set_state(&self, state: PlaybackState) {
        self.state.store(state.to_u8(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_engine_creation() {
        // May fail in CI without audio device — that's expected.
        let result = PlaybackEngine::try_new();
        if result.is_err() {
            eprintln!("Skipping: no audio device");
            return;
        }
        let engine = result.unwrap();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_playback_state_transitions() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return, // no audio device
        };

        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }

        assert_eq!(engine.state(), PlaybackState::Stopped);

        // Play.
        engine.play(path).unwrap();
        assert_eq!(engine.state(), PlaybackState::Playing);

        // Pause.
        engine.toggle_pause();
        assert_eq!(engine.state(), PlaybackState::Paused);

        // Resume.
        engine.toggle_pause();
        assert_eq!(engine.state(), PlaybackState::Playing);

        // Stop.
        engine.stop();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_play_nonexistent_file_returns_error() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let result = engine.play(Path::new("/nonexistent/file.wav"));
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_when_already_stopped() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        // Should not panic.
        engine.stop();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_toggle_pause_when_stopped() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        // Should be a no-op.
        engine.toggle_pause();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_current_path_tracking() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };

        assert!(engine.current_path().is_none());

        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }

        engine.play(path).unwrap();
        assert_eq!(engine.current_path().unwrap(), path);

        engine.stop();
        assert!(engine.current_path().is_none());
    }

    // --- S8-T2 tests: Playback cutoff fix ---

    #[test]
    fn test_drain_start_initially_none() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        assert!(engine.test_drain_start().is_none());
    }

    #[test]
    fn test_play_clears_drain_start() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        // Set drain_start to something, then play should clear it.
        engine.test_set_drain_start(Some(Instant::now()));
        engine.play(path).unwrap();
        assert!(
            engine.test_drain_start().is_none(),
            "play() should clear drain_start"
        );
    }

    #[test]
    fn test_stop_clears_drain_start() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        engine.test_set_drain_start(Some(Instant::now()));
        engine.stop();
        assert!(
            engine.test_drain_start().is_none(),
            "stop() should clear drain_start"
        );
    }

    #[test]
    fn test_state_returns_playing_during_drain() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        // Create empty sink (immediately empty) and set state to Playing.
        engine.test_create_empty_sink();
        engine
            .state
            .store(PlaybackState::Playing.to_u8(), Ordering::Relaxed);

        // First call: drain_start gets set, returns Playing.
        assert_eq!(engine.state(), PlaybackState::Playing);
        assert!(
            engine.test_drain_start().is_some(),
            "drain_start should be set on first empty detection"
        );
    }

    #[test]
    fn test_state_returns_stopped_after_drain_grace() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        engine.test_create_empty_sink();
        engine
            .state
            .store(PlaybackState::Playing.to_u8(), Ordering::Relaxed);
        // Set drain_start to 200ms ago — past the 150ms grace.
        engine.test_set_drain_start(Some(Instant::now() - Duration::from_millis(200)));

        assert_eq!(
            engine.state(),
            PlaybackState::Stopped,
            "should transition to Stopped after drain grace period"
        );
    }

    #[test]
    fn test_total_samples_computed_on_play() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        assert!(
            engine.total_samples() > 0,
            "total_samples should be set after play"
        );
        assert!(
            engine.sample_rate() > 0,
            "sample_rate should be set after play"
        );
    }

    #[test]
    fn test_sample_offset_resets_on_play() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        // Simulate offset advancing.
        *engine.sample_offset.lock().expect("playback lock poisoned") = 44100;
        // Play again — should reset offset.
        engine.play(path).unwrap();
        assert_eq!(
            engine.sample_offset(),
            0,
            "sample_offset should reset on new play()"
        );
    }

    #[test]
    fn test_position_fraction_derived() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        engine.test_set_sample_position(100, 200);
        let frac = engine.position_fraction();
        assert!(
            (frac - 0.5).abs() < 0.01,
            "position_fraction should be ~0.5, got {frac}"
        );
    }

    // --- Seek API tests ---

    #[test]
    fn test_seek_to_sample_zero_rewinds() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        *engine.sample_offset.lock().expect("playback lock poisoned") = 22050;
        engine.seek_to_sample(0).unwrap();
        assert_eq!(
            engine.sample_offset(),
            0,
            "seek_to_sample(0) should rewind to start"
        );
    }

    #[test]
    fn test_seek_to_sample_end_clamps() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        let total = engine.total_samples();
        engine.seek_to_sample(u32::MAX).unwrap();
        assert_eq!(
            engine.sample_offset(),
            total,
            "seek_to_sample(u32::MAX) should clamp to total_samples"
        );
    }

    #[test]
    fn test_seek_relative_forward() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        *engine.sample_offset.lock().expect("playback lock poisoned") = 0;
        let rate = engine.sample_rate();
        engine.seek_relative(0.5).unwrap();
        let expected = (0.5 * rate as f64) as u32;
        let actual = engine.sample_offset();
        // Allow ±1 sample for rounding.
        assert!(
            actual.abs_diff(expected) <= 1,
            "seek_relative(0.5) should advance by ~sample_rate/2, got {actual} expected {expected}"
        );
    }

    #[test]
    fn test_seek_relative_backward_clamps_to_zero() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        *engine.sample_offset.lock().expect("playback lock poisoned") = 1000;
        engine.seek_relative(-999.0).unwrap();
        assert_eq!(
            engine.sample_offset(),
            0,
            "seek_relative(-999.0) should clamp to 0"
        );
    }

    #[test]
    fn test_seek_while_paused_preserves_state() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        engine.toggle_pause();
        assert_eq!(engine.state(), PlaybackState::Paused);

        let rate = engine.sample_rate();
        engine.seek_relative(0.5).unwrap();
        assert_eq!(
            engine.state(),
            PlaybackState::Paused,
            "seek while paused should keep Paused state"
        );
        let expected = (0.5 * rate as f64) as u32;
        let actual = engine.sample_offset();
        assert!(
            actual.abs_diff(expected) <= 1,
            "sample_offset should update after seek while paused"
        );
    }

    #[test]
    fn test_seek_while_stopped_is_noop() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        engine.test_set_sample_position(0, 48000);
        engine.test_set_sample_rate(48000);
        engine.seek_to_sample(44100).unwrap();
        assert_eq!(
            engine.sample_offset(),
            0,
            "seek_to_sample on Stopped engine should be no-op"
        );
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_seek_clears_drain_start() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        engine.test_set_drain_start(Some(Instant::now()));
        engine.seek_to_sample(0).unwrap();
        assert!(
            engine.test_drain_start().is_none(),
            "seek should clear drain_start"
        );
    }

    #[test]
    fn test_seek_relative_negative_from_middle() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        engine.play(path).unwrap();
        let rate = engine.sample_rate();
        engine.seek_to_sample(rate * 2).unwrap();
        engine.seek_relative(-1.0).unwrap();
        let expected = rate;
        let actual = engine.sample_offset();
        assert!(
            actual.abs_diff(expected) <= 1,
            "after seek to 2s then -1s, should be at ~1s ({expected}), got {actual}"
        );
    }

    // --- SegmentSource unit tests ---

    #[test]
    fn test_segment_source_single_segment() {
        // Build a tiny mono buffer: 100 frames of ascending values.
        let buffer: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 100,
            pos: 0,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 0,
                end: 100,
                reps: 1,
                reversed: false,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        // Should yield all 100 samples then None.
        let mut count = 0;
        while src.next().is_some() {
            count += 1;
        }
        assert_eq!(count, 100);
        assert_eq!(control.frame.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn test_segment_source_loops_with_crossfade() {
        // 200-frame mono buffer, segment 0..100, reps=2 (play twice).
        let buffer: Vec<f32> = (0..200).map(|i| i as f32 / 200.0).collect();
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 200,
            pos: 0,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 0,
                end: 100,
                reps: 2,
                reversed: false,
            }],
            seg_idx: 0,
            reps_left: 2,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let mut count = 0;
        while src.next().is_some() {
            count += 1;
        }
        // Two passes of 100 frames = 200 samples.
        // Second pass starts with crossfade (64 frames overlap with end of first),
        // but total output count should be 200 (100 + 100).
        assert_eq!(count, 200);
    }

    #[test]
    fn test_segment_source_pending_seek() {
        let buffer: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 100,
            pos: 0,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 0,
                end: 100,
                reps: 1,
                reversed: false,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        // Consume 10 samples (frames 0–9).
        for _ in 0..10 {
            src.next();
        }
        // Frame counter reflects the last frame processed (frame 9).
        assert_eq!(control.frame.load(Ordering::Relaxed), 9);

        // Request seek to frame 50.
        control.pending_seek.store(50, Ordering::Relaxed);
        let sample = src.next().unwrap();
        // After seek, jump_to stores the target frame immediately.
        assert_eq!(control.frame.load(Ordering::Relaxed), 50);
        // Sample should be attenuated (fade-in gain near 0).
        assert!(sample.abs() < 0.5, "fade-in should attenuate: {sample}");
    }

    #[test]
    fn test_segment_source_sequential_advance() {
        // Two sequential segments: 0..50, 50..100 — continuous, no crossfade needed.
        let buffer: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer: buffer.clone(),
            channels: 1,
            rate: 48000,
            total_frames: 100,
            pos: 0,
            channel: 0,
            playlist: vec![
                PlaySegment {
                    start: 0,
                    end: 50,
                    reps: 1,
                    reversed: false,
                },
                PlaySegment {
                    start: 50,
                    end: 100,
                    reps: 1,
                    reversed: false,
                },
            ],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        assert_eq!(samples.len(), 100);
        // Sequential segments should produce continuous output (no crossfade gain dip).
        // Check that sample at frame 50 is close to the buffer value (no attenuation).
        assert!(
            (samples[50] - buffer[50]).abs() < 0.01,
            "sequential segment boundary should be continuous"
        );
    }

    #[test]
    fn test_segment_source_crossfade_no_pop() {
        // Segment 0..50 with 2 reps. On loop-back, crossfade should prevent
        // discontinuity. Use a ramp that ends at 1.0 and restarts at 0.0 —
        // without crossfade that would be a hard jump at the loop point.
        let buffer: Vec<f32> = (0..100).map(|i| (i % 50) as f32 / 49.0).collect();
        // buffer[49] = 1.0, buffer[0] = 0.0 — the loop-back discontinuity.
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 100,
            pos: 0,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 0,
                end: 50,
                reps: 2,
                reversed: false,
            }],
            seg_idx: 0,
            reps_left: 2,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        assert_eq!(samples.len(), 100);
        // At the loop point (around sample 50), the crossfade should smooth the
        // transition. Check that the maximum inter-sample jump is well below 1.0
        // (a hard pop would be ~1.0; crossfade should keep it under ~0.1).
        let max_delta: f32 = samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_delta < 0.15,
            "crossfade should smooth loop transition, max delta = {max_delta}"
        );
    }

    #[test]
    fn test_segment_source_reversed_segment_yields_frames_in_reverse() {
        // A reversed segment reads the original buffer end→start (no scratch
        // copy). Setup: 10 mono frames with values 0.0..0.9. Segment covers
        // frames 2..8 (values 0.2..0.7) with `reversed: true`. Starting
        // position is the reverse origin (end - 1 = 7).
        //
        // Expected output: 0.7, 0.6, 0.5, 0.4, 0.3, 0.2 (6 samples, reversed).
        let ch = 1usize;
        let buffer: Vec<f32> = (0..10).map(|i| i as f32 / 10.0).collect();
        let control = Arc::new(SourceControl::new());

        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 10,
            pos: 7 * ch, // reverse origin = end - 1
            channel: 0,
            playlist: vec![PlaySegment {
                start: 2,
                end: 8,
                reps: 1,
                reversed: true,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        assert_eq!(samples.len(), 6, "reversed segment should yield 6 samples");
        for (i, &s) in samples.iter().enumerate() {
            let expected = (7 - i) as f32 / 10.0;
            assert!(
                (s - expected).abs() < 1e-5,
                "sample[{i}] = {s}, expected {expected}"
            );
        }
    }

    #[test]
    fn test_xor_reversed_segment_plus_global_reverse_plays_forward() {
        // Plan 07 XOR identity: a reversed segment with global reverse on
        // plays forward. Setup same as above (segment frames 2..8,
        // `reversed: true`) but start position is `seg.start = 2` because
        // effective direction is false (true ^ true).
        //
        // Expected output: 0.2, 0.3, 0.4, 0.5, 0.6, 0.7 (forward order).
        let ch = 1usize;
        let buffer: Vec<f32> = (0..10).map(|i| i as f32 / 10.0).collect();
        let control = Arc::new(SourceControl::new());
        control.reversed.store(true, Ordering::Relaxed); // global reverse on

        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 10,
            pos: 2 * ch, // effective-forward origin = seg.start
            channel: 0,
            playlist: vec![PlaySegment {
                start: 2,
                end: 8,
                reps: 1,
                reversed: true,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        assert_eq!(samples.len(), 6);
        for (i, &s) in samples.iter().enumerate() {
            let expected = (2 + i) as f32 / 10.0;
            assert!(
                (s - expected).abs() < 1e-5,
                "sample[{i}] = {s}, expected {expected} (forward order via XOR)"
            );
        }
    }

    #[test]
    fn test_reversed_segment_frame_counts_down_in_buffer_space() {
        // After Plan 07, `control.frame` reports buffer-space frames (the
        // scratch region is gone; buffer space == logical space). A reversed
        // segment's frame counter counts down from end-1 to start.
        let ch = 1usize;
        let buffer: Vec<f32> = (0..10).map(|i| i as f32 / 10.0).collect();
        let control = Arc::new(SourceControl::new());

        let mut src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: 10,
            pos: 7 * ch,
            channel: 0,
            playlist: vec![PlaySegment {
                start: 2,
                end: 8,
                reps: 1,
                reversed: true,
            }],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        let mut frames: Vec<u32> = Vec::new();
        while src.next().is_some() {
            frames.push(control.frame.load(Ordering::Relaxed));
        }
        assert_eq!(frames.len(), 6);
        // Counts down: 7, 6, 5, 4, 3, 2.
        for (i, &f) in frames.iter().enumerate() {
            let expected = 7u32.saturating_sub(i as u32);
            assert_eq!(f, expected, "frame[{i}] = {f}, expected {expected}");
        }
    }

    #[test]
    fn test_segment_source_pending_restart() {
        // Verify that pending_restart resets seg_idx and restarts from segment 0.
        //
        // Setup: two-segment playlist [0..50, 50..100].
        // Consume all of segment 0 (50 samples) so the source is in segment 1.
        // Then set pending_restart and verify the source replays from segment 0.
        let buffer: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let control = Arc::new(SourceControl::new());
        let mut src = SegmentSource {
            buffer: buffer.clone(),
            channels: 1,
            rate: 48000,
            total_frames: 100,
            pos: 0,
            channel: 0,
            playlist: vec![
                PlaySegment {
                    start: 0,
                    end: 50,
                    reps: 1,
                    reversed: false,
                },
                PlaySegment {
                    start: 50,
                    end: 100,
                    reps: 1,
                    reversed: false,
                },
            ],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };

        // Consume segment 0 + a few frames of segment 1.
        for _ in 0..55 {
            src.next();
        }
        // Now in segment 1; frame should be around 55.
        assert!(
            control.frame.load(Ordering::Relaxed) >= 50,
            "should be in segment 1 by now"
        );

        // Signal restart and consume one frame to trigger step 0.
        control.pending_restart.store(true, Ordering::Relaxed);
        src.next();

        // After restart: seg_idx=0, frame should be at start of segment 0 (=0).
        assert_eq!(
            control.frame.load(Ordering::Relaxed),
            0,
            "pending_restart should rewind logical frame to segment 0 start"
        );
        // Source should continue yielding samples (not exhausted).
        assert!(
            src.next().is_some(),
            "source should still yield samples after restart"
        );
    }

    #[test]
    fn test_play_with_segments_basic() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        let total = {
            engine.play(path).unwrap();
            let t = engine.total_samples();
            engine.stop();
            t
        };
        let playlist = vec![(0, total / 2, 1u8, false), (total / 2, total, 1u8, false)];
        engine.play_with_segments(path, &playlist, false).unwrap();
        assert_eq!(engine.state(), PlaybackState::Playing);
        assert_eq!(engine.sample_offset(), 0);
        engine.stop();
    }

    // ---------- Plan 07 reverse-path properties (R1–R6) ----------
    //
    // These tests pin down the XOR unification: the effective per-segment
    // direction is `segment.reversed ^ SourceControl::reversed`. R1/R2 are
    // the XOR identity on sample order; R3 is the reverse-loop crossfade;
    // R4 is seek-during-reverse; R5 is restart-from-reversed-first; R6 is
    // frame-position preservation across a ToggleReverse pair.

    /// Build a single-segment source for tests. Buffer values are
    /// `frame_i -> i as f32 / max as f32` so sample emissions are
    /// linear-coded for easy assertion.
    fn build_one_segment_source(
        total: u32,
        start: u32,
        end: u32,
        reversed: bool,
        global_reversed: bool,
    ) -> (SegmentSource, Arc<SourceControl>) {
        assert!(start < end && end <= total, "start < end <= total");
        let buffer: Vec<f32> = (0..total).map(|i| i as f32 / total as f32).collect();
        let control = Arc::new(SourceControl::new());
        control.reversed.store(global_reversed, Ordering::Relaxed);
        let seg = PlaySegment {
            start,
            end,
            reps: 1,
            reversed,
        };
        let effective_rev = seg.reversed ^ global_reversed;
        let pos_frame = if effective_rev {
            seg.end.saturating_sub(1)
        } else {
            seg.start
        };
        let src = SegmentSource {
            buffer,
            channels: 1,
            rate: 48000,
            total_frames: total,
            pos: pos_frame as usize,
            channel: 0,
            playlist: vec![seg],
            seg_idx: 0,
            reps_left: 1,
            fade_out: 0,
            fade_in: 0,
            past_reverse_start: false,
            control: Arc::clone(&control),
        };
        (src, control)
    }

    // R1 — forward_seg ⊕ global_rev produces reverse traversal.
    #[test]
    fn r1_forward_seg_plus_global_reverse_plays_backwards() {
        let total = 16u32;
        let (mut src, _ctl) = build_one_segment_source(total, 4, 12, false, true);
        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        // Effective reverse = false XOR true = true; emit frames 11..=4 (8 samples).
        assert_eq!(samples.len(), 8);
        for (i, &s) in samples.iter().enumerate() {
            let expected = (11 - i) as f32 / total as f32;
            assert!(
                (s - expected).abs() < 1e-5,
                "R1 sample[{i}] = {s}, expected {expected}"
            );
        }
    }

    // R2 — reversed_seg ⊕ global_rev produces forward traversal. Covered
    // by `test_xor_reversed_segment_plus_global_reverse_plays_forward` above;
    // duplicate here under the canonical R-name so grep("R2 ") finds it.
    #[test]
    fn r2_reversed_seg_plus_global_reverse_plays_forwards() {
        let total = 16u32;
        let (mut src, _ctl) = build_one_segment_source(total, 4, 12, true, true);
        let samples: Vec<f32> = std::iter::from_fn(|| src.next()).collect();
        assert_eq!(samples.len(), 8);
        for (i, &s) in samples.iter().enumerate() {
            let expected = (4 + i) as f32 / total as f32;
            assert!(
                (s - expected).abs() < 1e-5,
                "R2 sample[{i}] = {s}, expected {expected}"
            );
        }
    }

    // R3 — reverse-loop crossfade engages fade_out+fade_in ramps at the
    // segment boundary. Uses reps >= 2 so the loop branch fires.
    #[test]
    fn r3_reverse_loop_crossfade_engages_fade_ramps() {
        // Segment big enough that CROSSFADE_FRAMES (64) fits. Use 256 frames
        // with reps=2 to force an intra-segment loop. reversed=true, global
        // off → effective reverse.
        let total = 256u32;
        let (mut src, _ctl) = build_one_segment_source(total, 0, 256, true, false);
        src.playlist[0].reps = 2;
        src.reps_left = 2;

        let mut saw_fade_out_nonzero = false;
        let mut saw_fade_in_nonzero_after_loop = false;
        let mut looped = false;

        // Pull samples until the source ends. Track fade state after each frame.
        for _step in 0..(total as usize * 4) {
            if src.next().is_none() {
                break;
            }
            if src.fade_out > 0 {
                saw_fade_out_nonzero = true;
            }
            if !looped && saw_fade_out_nonzero && src.fade_out == 0 {
                // fade_out hit zero — we've crossed the boundary. A loop
                // jump should have set fade_in > 0.
                looped = true;
            }
            if looped && src.fade_in > 0 {
                saw_fade_in_nonzero_after_loop = true;
            }
        }
        assert!(saw_fade_out_nonzero, "R3: fade_out never engaged");
        assert!(
            saw_fade_in_nonzero_after_loop,
            "R3: fade_in never engaged after reverse loop boundary"
        );
    }

    // R4 — Seek during reverse playback lands the frame counter at `p`.
    #[test]
    fn r4_seek_during_reverse_lands_at_target() {
        let total = 32u32;
        let (mut src, ctl) = build_one_segment_source(total, 0, 32, true, false);
        // Consume a few frames so pos has moved.
        for _ in 0..10 {
            src.next();
        }
        let target = 5u32;
        ctl.pending_seek.store(target, Ordering::Relaxed);
        // Trigger one more frame so on_frame_boundary picks up the seek.
        src.next();
        assert_eq!(
            ctl.frame.load(Ordering::Relaxed),
            target,
            "R4: seek during reverse must land at target frame"
        );
    }

    // R5 — Restart on a reversed first segment starts at `end - 1`.
    #[test]
    fn r5_restart_reversed_first_starts_at_end_minus_1() {
        let total = 32u32;
        let (mut src, ctl) = build_one_segment_source(total, 0, 16, true, false);
        // Move forward (in reverse traversal) several frames.
        for _ in 0..5 {
            src.next();
        }
        // Signal restart.
        ctl.pending_restart.store(true, Ordering::Relaxed);
        // One next() call runs on_frame_boundary's step 0 (pending_restart).
        src.next();
        assert_eq!(
            ctl.frame.load(Ordering::Relaxed),
            15, // first.end - 1 = 16 - 1
            "R5: reverse restart must land at first.end - 1"
        );
    }

    // R6 — ToggleReverse pair at the SourceControl layer preserves the
    // frame counter over one emitted frame (the ToggleReverse dispatch
    // flips control.reversed twice, a no-op at the source level).
    #[test]
    fn r6_toggle_reverse_pair_preserves_frame() {
        let total = 32u32;
        let (mut src, ctl) = build_one_segment_source(total, 0, 32, false, false);
        // Walk a few frames forward.
        for _ in 0..8 {
            src.next();
        }
        let frame_before = ctl.frame.load(Ordering::Relaxed);
        // Toggle reverse twice (net no-op). Between toggles emit one frame
        // to ensure the state round-trips through next().
        ctl.reversed
            .store(!ctl.reversed.load(Ordering::Relaxed), Ordering::Relaxed);
        src.next();
        ctl.reversed
            .store(!ctl.reversed.load(Ordering::Relaxed), Ordering::Relaxed);
        src.next();
        let frame_after = ctl.frame.load(Ordering::Relaxed);
        // After the pair, frame should have advanced by 2 in the forward
        // direction (no direction change net) — not stuck, not reversed.
        assert!(
            frame_after >= frame_before,
            "R6: toggle pair drifted backwards ({} → {})",
            frame_before,
            frame_after,
        );
    }

    // Regression for the `play_with_segments` initial-position bug
    // caught in Plan 07's local review: `Input::Stop` does not clear
    // the FSM's `reversed` flag, so a prior `set_reversed(true)`
    // would otherwise leave pos pointing at `seg.start` while the
    // mixer's effective direction is reverse — the whole first
    // segment would be skipped. This test exercises the code path
    // that computes the initial position given the FSM state, not
    // the SegmentSource behaviour itself.
    #[test]
    fn initial_pos_xors_fsm_reversed_with_segment_reversed() {
        let engine = match PlaybackEngine::try_new() {
            Ok(e) => e,
            Err(_) => return,
        };
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }

        // Toggle global reverse on before the play_with_segments call.
        engine.set_reversed(true);

        let total = {
            engine.play(path).unwrap();
            let t = engine.total_samples();
            engine.stop();
            t
        };

        // Re-enable the flag (stop() doesn't clear FSM.reversed today
        // but `play()` above may have mirrored the default — assert the
        // condition explicitly so the test is honest about its setup).
        engine.set_reversed(true);

        // Forward segment + global reverse → effective reverse → the
        // entry position must be `end - 1`, not `start`.
        let seg = (10u32, total.min(1000), 1u8, false);
        engine.play_with_segments(path, &[seg], false).unwrap();
        let offset = engine.sample_offset();
        assert_eq!(
            offset,
            seg.1.saturating_sub(1),
            "effective-reversed entry must be seg.end - 1, got {offset}"
        );
        engine.stop();
    }

    // R1/R2 proptest: randomized segment + flag combinations, sample-count
    // invariant. Expected sample count is always `end - start`.
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn r1_r2_xor_identity_sample_count(
            start in 0u32..100,
            len in 1u32..100,
            seg_reversed in any::<bool>(),
            global_reversed in any::<bool>(),
        ) {
            let end = start + len;
            let total = end + 50;
            let (mut src, _ctl) = build_one_segment_source(
                total, start, end, seg_reversed, global_reversed,
            );
            let mut count = 0usize;
            while src.next().is_some() {
                count += 1;
                if count > (len as usize) * 2 {
                    panic!("R1/R2 proptest: emitted > 2x segment length");
                }
            }
            prop_assert_eq!(
                count, len as usize,
                "XOR identity: segment of length {} emitted {} samples",
                len, count,
            );
        }
    }
}
