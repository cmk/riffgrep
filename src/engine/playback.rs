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

        // Try to compute duration from file metadata.
        if let Ok(dur) = compute_duration(path) {
            *self.duration.lock().unwrap() = Some(dur);
        }

        *self.sink.lock().unwrap() = Some(sink);
        *self.play_start.lock().unwrap() = Some(Instant::now());
        *self.paused_elapsed.lock().unwrap() = Duration::ZERO;
        *self.current_path.lock().unwrap() = Some(path.to_path_buf());
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
        self.state.store(PlaybackState::Stopped.to_u8(), Ordering::Relaxed);
    }

    /// Get the current playback state.
    pub fn state(&self) -> PlaybackState {
        let state = PlaybackState::from_u8(self.state.load(Ordering::Relaxed));
        // Auto-detect when playback finishes.
        if state == PlaybackState::Playing {
            if let Some(ref sink) = *self.sink.lock().unwrap() {
                if sink.empty() {
                    self.state.store(PlaybackState::Stopped.to_u8(), Ordering::Relaxed);
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

    fn compute_elapsed(&self) -> Duration {
        let paused = *self.paused_elapsed.lock().unwrap();
        match *self.play_start.lock().unwrap() {
            Some(start) => paused + start.elapsed(),
            None => paused,
        }
    }
}

/// Compute duration from a WAV file's fmt + data chunks.
fn compute_duration(path: &Path) -> Result<Duration, anyhow::Error> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = crate::engine::bext::scan_chunks(&mut reader)?;
    let fmt = crate::engine::wav::parse_fmt(&mut reader, &map)?;
    let info = crate::engine::wav::AudioInfo::from_fmt(&fmt, map.data_size);
    Ok(Duration::from_secs_f64(info.duration_secs))
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
}
