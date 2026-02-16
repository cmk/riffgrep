//! Audio playback engine using symphonia for decoding and rodio for output.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{OutputStream, OutputStreamHandle, Sink};

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

/// Thread-safe audio playback engine.
///
/// Holds the audio output stream and provides play/pause/stop controls.
/// All methods are safe to call from any thread.
pub struct PlaybackEngine {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Arc<Mutex<Option<Sink>>>,
    state: Arc<AtomicU8>,
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
}

impl PlaybackEngine {
    /// Try to create a new playback engine. Returns Err if no audio device.
    pub fn try_new() -> Result<Self, anyhow::Error> {
        let (stream, stream_handle) = OutputStream::try_default()
            .map_err(|e| anyhow::anyhow!("no audio device: {e}"))?;

        Ok(Self {
            _stream: stream,
            stream_handle,
            sink: Arc::new(Mutex::new(None)),
            state: Arc::new(AtomicU8::new(PlaybackState::Stopped.to_u8())),
            play_start: Arc::new(Mutex::new(None)),
            paused_elapsed: Arc::new(Mutex::new(Duration::ZERO)),
            current_path: Arc::new(Mutex::new(None)),
            duration: Arc::new(Mutex::new(None)),
            drain_start: Arc::new(Mutex::new(None)),
            sample_offset: Arc::new(Mutex::new(0)),
            total_samples: Arc::new(Mutex::new(0)),
            sample_rate_hz: Arc::new(Mutex::new(0)),
        })
    }

    /// Start playing a WAV file. Stops any current playback first.
    pub fn play(&self, path: &Path) -> Result<(), anyhow::Error> {
        self.stop();

        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        let sink = Sink::try_new(&self.stream_handle)
            .map_err(|e| anyhow::anyhow!("audio output error: {e}"))?;

        let source = rodio::Decoder::new(reader)
            .map_err(|e| anyhow::anyhow!("decode error: {e}"))?;

        sink.append(source);

        // Compute duration and sample info from WAV headers.
        if let Ok((dur, total, rate)) = compute_playback_info(path) {
            *self.duration.lock().unwrap() = Some(dur);
            *self.total_samples.lock().unwrap() = total;
            *self.sample_rate_hz.lock().unwrap() = rate;
        }

        *self.sink.lock().unwrap() = Some(sink);
        *self.play_start.lock().unwrap() = Some(Instant::now());
        *self.paused_elapsed.lock().unwrap() = Duration::ZERO;
        *self.current_path.lock().unwrap() = Some(path.to_path_buf());
        *self.drain_start.lock().unwrap() = None;
        *self.sample_offset.lock().unwrap() = 0;
        self.state.store(PlaybackState::Playing.to_u8(), Ordering::Relaxed);

        Ok(())
    }

    /// Toggle pause/resume. If stopped, this is a no-op.
    pub fn toggle_pause(&self) {
        let current = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        match current {
            PlaybackState::Playing => {
                if let Some(ref sink) = *self.sink.lock().unwrap() {
                    sink.pause();
                }
                // Save elapsed time before pausing.
                let elapsed = self.compute_elapsed();
                *self.paused_elapsed.lock().unwrap() = elapsed;
                *self.play_start.lock().unwrap() = None;
                self.state.store(PlaybackState::Paused.to_u8(), Ordering::Relaxed);
            }
            PlaybackState::Paused => {
                if let Some(ref sink) = *self.sink.lock().unwrap() {
                    sink.play();
                }
                *self.play_start.lock().unwrap() = Some(Instant::now());
                self.state.store(PlaybackState::Playing.to_u8(), Ordering::Relaxed);
            }
            PlaybackState::Stopped => {}
        }
    }

    /// Stop playback and reset state.
    pub fn stop(&self) {
        if let Some(sink) = self.sink.lock().unwrap().take() {
            sink.stop();
        }
        *self.play_start.lock().unwrap() = None;
        *self.paused_elapsed.lock().unwrap() = Duration::ZERO;
        *self.current_path.lock().unwrap() = None;
        *self.duration.lock().unwrap() = None;
        *self.drain_start.lock().unwrap() = None;
        *self.sample_offset.lock().unwrap() = 0;
        self.state.store(PlaybackState::Stopped.to_u8(), Ordering::Relaxed);
    }

    /// Get the current playback state.
    ///
    /// When the rodio sink reports empty, a 150ms grace period allows the
    /// audio output buffer to fully drain before transitioning to Stopped.
    pub fn state(&self) -> PlaybackState {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state == PlaybackState::Playing {
            if let Some(ref sink) = *self.sink.lock().unwrap() {
                if sink.empty() {
                    let mut drain = self.drain_start.lock().unwrap();
                    if drain.is_none() {
                        *drain = Some(Instant::now());
                        return PlaybackState::Playing;
                    }
                    if drain.unwrap().elapsed() < Duration::from_millis(DRAIN_GRACE_MS) {
                        return PlaybackState::Playing;
                    }
                    // Grace period elapsed — transition to Stopped.
                    *drain = None;
                    self.state
                        .store(PlaybackState::Stopped.to_u8(), Ordering::Relaxed);
                    return PlaybackState::Stopped;
                }
            }
        }
        state
    }

    /// Elapsed playback time since play started.
    pub fn elapsed(&self) -> Duration {
        self.compute_elapsed()
    }

    /// Total duration of the current track (if known).
    pub fn duration(&self) -> Option<Duration> {
        *self.duration.lock().unwrap()
    }

    /// Path of the currently loaded file.
    pub fn current_path(&self) -> Option<PathBuf> {
        self.current_path.lock().unwrap().clone()
    }

    /// Current playback position in frames.
    pub fn sample_offset(&self) -> u32 {
        *self.sample_offset.lock().unwrap()
    }

    /// Total number of frames in the current file.
    pub fn total_samples(&self) -> u32 {
        *self.total_samples.lock().unwrap()
    }

    /// Sample rate of the current file in Hz.
    pub fn sample_rate(&self) -> u32 {
        *self.sample_rate_hz.lock().unwrap()
    }

    /// Playback position as a fraction 0.0–1.0 (derived from sample_offset / total_samples).
    pub fn position_fraction(&self) -> f32 {
        let offset = *self.sample_offset.lock().unwrap();
        let total = *self.total_samples.lock().unwrap();
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

        let secs = clamped as f64 / rate as f64;
        let duration = Duration::from_secs_f64(secs);

        // Seek the rodio sink.
        if let Some(ref sink) = *self.sink.lock().unwrap() {
            sink.try_seek(duration)
                .map_err(|e| anyhow::anyhow!("seek error: {e}"))?;
        }

        // Update sample offset.
        *self.sample_offset.lock().unwrap() = clamped;

        // Reset elapsed tracking to match the new position.
        *self.paused_elapsed.lock().unwrap() = duration;
        if state == PlaybackState::Playing {
            *self.play_start.lock().unwrap() = Some(Instant::now());
        }
        // If paused, play_start stays None — position updates but no resume.

        // Clear drain_start (seeking restarts drain detection).
        *self.drain_start.lock().unwrap() = None;

        Ok(())
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

    /// Recompute sample_offset from elapsed playback time.
    ///
    /// Called from the TUI tick loop to keep the sample offset in sync with
    /// wall-clock elapsed time. Clamped to total_samples.
    pub fn update_sample_offset(&self) {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        if state != PlaybackState::Playing {
            return;
        }
        let rate = self.sample_rate();
        if rate == 0 {
            return;
        }
        let elapsed_secs = self.compute_elapsed().as_secs_f64();
        let total = self.total_samples();
        let offset = ((elapsed_secs * rate as f64) as u32).min(total);
        *self.sample_offset.lock().unwrap() = offset;
    }

    fn compute_elapsed(&self) -> Duration {
        let paused = *self.paused_elapsed.lock().unwrap();
        match *self.play_start.lock().unwrap() {
            Some(start) => paused + start.elapsed(),
            None => paused,
        }
    }
}

/// Compute playback info from a WAV file's fmt + data chunks.
///
/// Returns `(duration, total_samples, sample_rate)`.
fn compute_playback_info(path: &Path) -> Result<(Duration, u32, u32), anyhow::Error> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = crate::engine::bext::scan_chunks(&mut reader)?;
    let fmt = crate::engine::wav::parse_fmt(&mut reader, &map)?;
    let info = crate::engine::wav::AudioInfo::from_fmt(&fmt, map.data_size);
    let duration = Duration::from_secs_f64(info.duration_secs);
    let sample_rate = info.sample_rate;
    let total_samples = (info.duration_secs * sample_rate as f64).round() as u32;
    Ok((duration, total_samples, sample_rate))
}

#[cfg(test)]
impl PlaybackEngine {
    /// Test helper: read drain_start value.
    pub fn test_drain_start(&self) -> Option<Instant> {
        *self.drain_start.lock().unwrap()
    }

    /// Test helper: set drain_start directly.
    pub fn test_set_drain_start(&self, v: Option<Instant>) {
        *self.drain_start.lock().unwrap() = v;
    }

    /// Test helper: create an empty sink (no audio appended = immediately empty).
    pub fn test_create_empty_sink(&self) {
        if let Ok(sink) = Sink::try_new(&self.stream_handle) {
            *self.sink.lock().unwrap() = Some(sink);
        }
    }

    /// Test helper: set sample position fields directly.
    pub fn test_set_sample_position(&self, offset: u32, total: u32) {
        *self.sample_offset.lock().unwrap() = offset;
        *self.total_samples.lock().unwrap() = total;
    }

    /// Test helper: set sample rate directly.
    pub fn test_set_sample_rate(&self, rate: u32) {
        *self.sample_rate_hz.lock().unwrap() = rate;
    }

    /// Test helper: set internal state directly (Playing/Paused/Stopped).
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
        assert!(engine.total_samples() > 0, "total_samples should be set after play");
        assert!(engine.sample_rate() > 0, "sample_rate should be set after play");
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
        *engine.sample_offset.lock().unwrap() = 44100;
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

    // --- S8-T3 tests: Seek API ---

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
        // Advance offset artificially.
        *engine.sample_offset.lock().unwrap() = 22050;
        engine.seek_to_sample(0).unwrap();
        assert_eq!(engine.sample_offset(), 0, "seek_to_sample(0) should rewind to start");
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
        *engine.sample_offset.lock().unwrap() = 0;
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
        *engine.sample_offset.lock().unwrap() = 1000;
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
        // Engine is Stopped by default. Set up sample info to verify no change.
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
        // Simulate drain_start being set.
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
        // Seek to 2s mark.
        engine.seek_to_sample(rate * 2).unwrap();
        // Seek back 1s.
        engine.seek_relative(-1.0).unwrap();
        let expected = rate; // Should be at ~1s mark.
        let actual = engine.sample_offset();
        assert!(
            actual.abs_diff(expected) <= 1,
            "after seek to 2s then -1s, should be at ~1s ({expected}), got {actual}"
        );
    }
}
