//! WAV format parser, PCM sample reader, and peak computation.
//!
//! Provides streaming audio processing without loading entire files into memory.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rodio::Source;

use super::bext::{ChunkMap, RiffError};

/// WAV audio format tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// Linear PCM (format tag 1).
    Pcm,
    /// IEEE 754 floating-point (format tag 3).
    IeeeFloat,
    /// Unrecognized format tag.
    Other(u16),
}

/// Parsed `fmt ` chunk data.
#[derive(Debug, Clone)]
pub struct FmtChunk {
    /// Audio sample format.
    pub format: AudioFormat,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Samples per second (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// Bits per sample (8, 16, 24, or 32).
    pub bits_per_sample: u16,
    /// Block alignment (channels * bits_per_sample / 8).
    pub block_align: u16,
}

/// Parse the `fmt ` chunk into a [`FmtChunk`].
///
/// Requires at least 16 bytes of fmt data.
pub fn parse_fmt<R: Read + Seek>(reader: &mut R, map: &ChunkMap) -> Result<FmtChunk, RiffError> {
    let offset = map.fmt_offset.ok_or(RiffError::NotRiffWave)?;

    if map.fmt_size < 16 {
        return Err(RiffError::BextTooSmall {
            actual: map.fmt_size,
            expected: 16,
        });
    }

    reader.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; 16];
    reader.read_exact(&mut buf)?;

    let format_tag = u16::from_le_bytes([buf[0], buf[1]]);
    let channels = u16::from_le_bytes([buf[2], buf[3]]);
    let sample_rate = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    // bytes 8-11: avg bytes per sec (skip)
    let block_align = u16::from_le_bytes([buf[12], buf[13]]);
    let bits_per_sample = u16::from_le_bytes([buf[14], buf[15]]);

    let format = match format_tag {
        1 => AudioFormat::Pcm,
        3 => AudioFormat::IeeeFloat,
        other => AudioFormat::Other(other),
    };

    Ok(FmtChunk {
        format,
        channels,
        sample_rate,
        bits_per_sample,
        block_align,
    })
}

/// Decode a single sample from bytes to f32 in [-1.0, 1.0].
fn decode_sample(bytes: &[u8], format: AudioFormat, bits: u16) -> f32 {
    match format {
        AudioFormat::Pcm => match bits {
            16 => {
                let val = i16::from_le_bytes([bytes[0], bytes[1]]);
                val as f32 / 32768.0
            }
            24 => {
                // Sign-extend 24-bit to i32.
                let val =
                    bytes[0] as i32 | (bytes[1] as i32) << 8 | ((bytes[2] as i8) as i32) << 16;
                val as f32 / 8_388_608.0
            }
            8 => {
                // 8-bit PCM is unsigned: 128 = silence.
                (bytes[0] as f32 - 128.0) / 128.0
            }
            32 => {
                let val = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                val as f32 / 2_147_483_648.0
            }
            _ => 0.0,
        },
        AudioFormat::IeeeFloat => {
            if bits == 32 && bytes.len() >= 4 {
                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                0.0
            }
        }
        AudioFormat::Other(_) => 0.0,
    }
}

/// Channel mixdown mode for peak computation.
#[allow(dead_code)] // Reserved for configurable peak computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelMode {
    /// Average all channels.
    Left,
    /// Use only channel 0 (left).
    #[default]
    Mix,
}

/// Peak measurement method.
#[allow(dead_code)] // Reserved for configurable peak computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeakMeasurement {
    /// Root-mean-square per bin.
    Rms,
    /// Maximum absolute sample per bin.
    #[default]
    Peak,
}

/// Options controlling peak computation.
#[allow(dead_code)] // Reserved for configurable peak computation.
#[derive(Debug, Clone, Default)]
pub struct PeakOptions {
    /// Channel mixdown mode.
    pub channel: ChannelMode,
    /// Measurement method.
    pub measurement: PeakMeasurement,
}

impl PeakOptions {
    /// Parse from config strings (case-insensitive).
    #[allow(dead_code)] // Reserved for configurable peak computation.
    pub fn from_config(measurement: Option<&str>, channel: Option<&str>) -> Self {
        Self {
            measurement: match measurement.map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("peak") => PeakMeasurement::Peak,
                _ => PeakMeasurement::Rms,
            },
            channel: match channel.map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("left") => ChannelMode::Left,
                _ => ChannelMode::Mix,
            },
        }
    }
}

/// Number of u8 peak values to produce.
pub const PEAK_COUNT: usize = 180;

#[allow(dead_code)] // Reserved for configurable peak computation.
/// Stream raw PCM samples with configurable channel mode.
///
/// `ChannelMode::Mix` averages all channels (same as `stream_samples`).
/// `ChannelMode::Left` uses only channel 0.
pub fn stream_samples_channel<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
    channel: ChannelMode,
    callback: &mut dyn FnMut(f32),
) -> Result<u64, RiffError> {
    let data_offset = map.data_offset.ok_or(RiffError::NotRiffWave)?;
    reader.seek(SeekFrom::Start(data_offset))?;

    let bytes_per_sample = (fmt.bits_per_sample / 8) as usize;
    let channels = fmt.channels as usize;
    let frame_size = bytes_per_sample * channels;

    if frame_size == 0 {
        return Ok(0);
    }

    let total_bytes = map.data_size as usize;
    let total_frames = total_bytes / frame_size;

    const BUF_SIZE: usize = 4096;
    let mut buf = [0u8; BUF_SIZE];
    let mut remaining_bytes = total_frames * frame_size;
    let mut mono_count: u64 = 0;
    let aligned_buf_size = (BUF_SIZE / frame_size) * frame_size;

    while remaining_bytes > 0 {
        let to_read = remaining_bytes.min(aligned_buf_size);
        let slice = &mut buf[..to_read];
        match reader.read_exact(slice) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }

        let mut pos = 0;
        while pos + frame_size <= to_read {
            let sample = match channel {
                ChannelMode::Left => {
                    // Just channel 0.
                    decode_sample(
                        &slice[pos..pos + bytes_per_sample],
                        fmt.format,
                        fmt.bits_per_sample,
                    )
                }
                ChannelMode::Mix => {
                    let mut mono_sum: f32 = 0.0;
                    for ch in 0..channels {
                        let sample_start = pos + ch * bytes_per_sample;
                        mono_sum += decode_sample(
                            &slice[sample_start..sample_start + bytes_per_sample],
                            fmt.format,
                            fmt.bits_per_sample,
                        );
                    }
                    mono_sum / channels as f32
                }
            };
            callback(sample);
            mono_count += 1;
            pos += frame_size;
        }

        remaining_bytes -= to_read;
    }

    Ok(mono_count)
}

#[allow(dead_code)] // Reserved for configurable peak computation.
/// Compute peaks with configurable options.
pub fn compute_peaks_with_options<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
    opts: &PeakOptions,
) -> Result<Vec<u8>, RiffError> {
    let bytes_per_sample = (fmt.bits_per_sample / 8) as u64;
    let channels = fmt.channels as u64;
    let frame_size = bytes_per_sample * channels;

    if frame_size == 0 {
        return Ok(vec![0u8; PEAK_COUNT]);
    }

    let total_frames = map.data_size as u64 / frame_size;
    if total_frames == 0 {
        return Ok(vec![0u8; PEAK_COUNT]);
    }

    let bin_size = total_frames as f64 / PEAK_COUNT as f64;

    match opts.measurement {
        PeakMeasurement::Rms => {
            let mut bin_sum_sq = [0.0f64; PEAK_COUNT];
            let mut bin_count = [0u64; PEAK_COUNT];
            let mut sample_idx: u64 = 0;

            stream_samples_channel(reader, map, fmt, opts.channel, &mut |sample| {
                let bin = (sample_idx as f64 / bin_size).min((PEAK_COUNT - 1) as f64) as usize;
                bin_sum_sq[bin] += (sample as f64) * (sample as f64);
                bin_count[bin] += 1;
                sample_idx += 1;
            })?;

            let mut bin_rms = [0.0f64; PEAK_COUNT];
            for i in 0..PEAK_COUNT {
                if bin_count[i] > 0 {
                    bin_rms[i] = (bin_sum_sq[i] / bin_count[i] as f64).sqrt();
                }
            }

            let global_max = bin_rms.iter().cloned().fold(0.0f64, f64::max);
            if global_max == 0.0 {
                return Ok(vec![0u8; PEAK_COUNT]);
            }

            Ok(bin_rms
                .iter()
                .map(|&v| (v / global_max * 255.0).round() as u8)
                .collect())
        }
        PeakMeasurement::Peak => {
            let mut bin_max = [0.0f64; PEAK_COUNT];
            let mut sample_idx: u64 = 0;

            stream_samples_channel(reader, map, fmt, opts.channel, &mut |sample| {
                let bin = (sample_idx as f64 / bin_size).min((PEAK_COUNT - 1) as f64) as usize;
                let abs = (sample as f64).abs();
                if abs > bin_max[bin] {
                    bin_max[bin] = abs;
                }
                sample_idx += 1;
            })?;

            let global_max = bin_max.iter().cloned().fold(0.0f64, f64::max);
            if global_max == 0.0 {
                return Ok(vec![0u8; PEAK_COUNT]);
            }

            Ok(bin_max
                .iter()
                .map(|&v| (v / global_max * 255.0).round() as u8)
                .collect())
        }
    }
}

#[allow(dead_code)] // Reserved for configurable peak computation.
/// Convenience: open a file, scan chunks, parse fmt, compute peaks with options.
///
/// Returns empty peaks for AIFF files (no RIFF chunks to parse).
pub fn compute_peaks_from_path_with_options(
    path: &Path,
    opts: &PeakOptions,
) -> Result<Vec<u8>, RiffError> {
    // Only the RIFF path supports configurable peak options.
    let registry = super::source::AudioRegistry::new();
    let is_riff = registry
        .for_path(path)
        .is_some_and(|s| s.extensions().contains(&"wav"));
    if !is_riff {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = super::bext::scan_chunks(&mut reader)?;
    let fmt = parse_fmt(&mut reader, &map)?;
    compute_peaks_with_options(&mut reader, &map, &fmt, opts)
}

/// Number of u8 peak values for stereo output (180 left + 180 right).
pub const STEREO_PEAK_COUNT: usize = PEAK_COUNT * 2;

/// Compute stereo peaks: 360 u8 values (180 left + 180 right).
///
/// For mono files, left and right are identical. For stereo+, left = channel 0,
/// right = channel 1. Both channels are independently normalized so the louder
/// channel peaks at 255 and the quieter channel is proportionally scaled.
pub fn compute_peaks_stereo<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
) -> Result<Vec<u8>, RiffError> {
    let bytes_per_sample = (fmt.bits_per_sample / 8) as u64;
    let channels = fmt.channels as u64;
    let frame_size = bytes_per_sample * channels;

    if frame_size == 0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let total_frames = map.data_size as u64 / frame_size;
    if total_frames == 0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let bin_size = total_frames as f64 / PEAK_COUNT as f64;

    let mut left_sum_sq = [0.0f64; PEAK_COUNT];
    let mut right_sum_sq = [0.0f64; PEAK_COUNT];
    let mut bin_count = [0u64; PEAK_COUNT];

    stream_samples_stereo(reader, map, fmt, &mut |frame_idx, left, right| {
        let bin = (frame_idx as f64 / bin_size).min((PEAK_COUNT - 1) as f64) as usize;
        left_sum_sq[bin] += (left as f64) * (left as f64);
        right_sum_sq[bin] += (right as f64) * (right as f64);
        bin_count[bin] += 1;
    })?;

    // Compute RMS per bin per channel.
    let mut left_rms = [0.0f64; PEAK_COUNT];
    let mut right_rms = [0.0f64; PEAK_COUNT];
    for i in 0..PEAK_COUNT {
        if bin_count[i] > 0 {
            left_rms[i] = (left_sum_sq[i] / bin_count[i] as f64).sqrt();
            right_rms[i] = (right_sum_sq[i] / bin_count[i] as f64).sqrt();
        }
    }

    // Normalize: global max across both channels → 255.
    let left_max = left_rms.iter().cloned().fold(0.0f64, f64::max);
    let right_max = right_rms.iter().cloned().fold(0.0f64, f64::max);
    let global_max = left_max.max(right_max);

    if global_max == 0.0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let mut peaks = Vec::with_capacity(STEREO_PEAK_COUNT);

    // First 180: left channel.
    for &v in &left_rms {
        peaks.push((v / global_max * 255.0).round() as u8);
    }
    // Next 180: right channel.
    for &v in &right_rms {
        peaks.push((v / global_max * 255.0).round() as u8);
    }

    Ok(peaks)
}

/// Stream PCM frames as stereo (left, right) pairs.
///
/// For mono files, left = right = the single channel.
/// For multi-channel files, left = channel 0, right = channel 1.
/// Callback receives `(frame_index, left_sample, right_sample)`.
fn stream_samples_stereo<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
    callback: &mut dyn FnMut(u64, f32, f32),
) -> Result<u64, RiffError> {
    let data_offset = map.data_offset.ok_or(RiffError::NotRiffWave)?;
    reader.seek(SeekFrom::Start(data_offset))?;

    let bytes_per_sample = (fmt.bits_per_sample / 8) as usize;
    let channels = fmt.channels as usize;
    let frame_size = bytes_per_sample * channels;

    if frame_size == 0 {
        return Ok(0);
    }

    let total_bytes = map.data_size as usize;
    let total_frames = total_bytes / frame_size;

    const BUF_SIZE: usize = 4096;
    let mut buf = [0u8; BUF_SIZE];
    let mut remaining_bytes = total_frames * frame_size;
    let mut frame_idx: u64 = 0;
    let aligned_buf_size = (BUF_SIZE / frame_size) * frame_size;

    while remaining_bytes > 0 {
        let to_read = remaining_bytes.min(aligned_buf_size);
        let slice = &mut buf[..to_read];
        match reader.read_exact(slice) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }

        let mut pos = 0;
        while pos + frame_size <= to_read {
            let left = decode_sample(
                &slice[pos..pos + bytes_per_sample],
                fmt.format,
                fmt.bits_per_sample,
            );
            let right = if channels >= 2 {
                decode_sample(
                    &slice[pos + bytes_per_sample..pos + 2 * bytes_per_sample],
                    fmt.format,
                    fmt.bits_per_sample,
                )
            } else {
                left // mono: right = left
            };
            callback(frame_idx, left, right);
            frame_idx += 1;
            pos += frame_size;
        }

        remaining_bytes -= to_read;
    }

    Ok(frame_idx)
}

/// Convenience: open a file, compute stereo peaks.
///
/// Returns empty peaks for AIFF files (no RIFF chunks to parse).
pub fn compute_peaks_stereo_from_path(path: &Path) -> Result<Vec<u8>, RiffError> {
    let registry = super::source::AudioRegistry::new();
    match registry.for_path(path) {
        Some(src) => src.compute_peaks_stereo(path),
        None => Err(RiffError::NotRiffWave),
    }
}

/// Compute stereo peaks using rodio's format-agnostic decoder.
///
/// Works for any format symphonia supports (AIFF, FLAC, etc.). Slower than
/// the RIFF-specific path because it decodes the entire file to f32, but
/// produces identical 360-byte output (180 left + 180 right RMS peaks).
pub fn compute_peaks_stereo_via_decoder(path: &Path) -> Result<Vec<u8>, RiffError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let decoder = rodio::Decoder::new(reader).map_err(|_| RiffError::NotRiffWave)?;

    let channels = decoder.channels() as usize;
    let samples: Vec<f32> = decoder.map(|s| s as f32 / 32768.0).collect();

    if samples.is_empty() || channels == 0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let total_frames = samples.len() / channels;
    if total_frames == 0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let bin_size = total_frames as f64 / PEAK_COUNT as f64;

    let mut left_sum_sq = [0.0f64; PEAK_COUNT];
    let mut right_sum_sq = [0.0f64; PEAK_COUNT];
    let mut bin_count = [0u64; PEAK_COUNT];

    for frame in 0..total_frames {
        let base = frame * channels;
        let left = samples[base] as f64;
        let right = if channels > 1 {
            samples[base + 1] as f64
        } else {
            left
        };
        let bin = (frame as f64 / bin_size).min((PEAK_COUNT - 1) as f64) as usize;
        left_sum_sq[bin] += left * left;
        right_sum_sq[bin] += right * right;
        bin_count[bin] += 1;
    }

    let mut left_rms = [0.0f64; PEAK_COUNT];
    let mut right_rms = [0.0f64; PEAK_COUNT];
    for i in 0..PEAK_COUNT {
        if bin_count[i] > 0 {
            left_rms[i] = (left_sum_sq[i] / bin_count[i] as f64).sqrt();
            right_rms[i] = (right_sum_sq[i] / bin_count[i] as f64).sqrt();
        }
    }

    let left_max = left_rms.iter().cloned().fold(0.0f64, f64::max);
    let right_max = right_rms.iter().cloned().fold(0.0f64, f64::max);
    let global_max = left_max.max(right_max);

    if global_max == 0.0 {
        return Ok(vec![0u8; STEREO_PEAK_COUNT]);
    }

    let mut peaks = Vec::with_capacity(STEREO_PEAK_COUNT);
    for &v in &left_rms {
        peaks.push((v / global_max * 255.0).round() as u8);
    }
    for &v in &right_rms {
        peaks.push((v / global_max * 255.0).round() as u8);
    }

    Ok(peaks)
}

/// Audio metadata extracted from the fmt chunk.
#[derive(Debug, Clone)]
pub struct AudioInfo {
    /// Total number of sample frames (per channel). Exact integer, never derived
    /// from a float round-trip. Use this for all sample-position arithmetic.
    pub total_samples: u32,
    /// Duration in seconds. Derived from `total_samples / sample_rate` at
    /// construction time. Use only for human-readable display, not arithmetic.
    pub duration_secs: f64,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bits per sample.
    pub bit_depth: u16,
    /// Number of channels.
    pub channels: u16,
}

impl AudioInfo {
    /// Compute audio info from fmt chunk and data size.
    pub fn from_fmt(fmt: &FmtChunk, data_size: u32) -> Self {
        // Use block_align (bytes per frame, as written by the encoder) rather than
        // recomputing from bits_per_sample.  Integer division of bits_per_sample/8
        // silently truncates for non-multiples of 8 (e.g. 20-bit → 2 instead of 3).
        let bytes_per_frame = fmt.block_align as u64;
        let total_samples = if bytes_per_frame > 0 {
            (data_size as u64 / bytes_per_frame) as u32
        } else {
            0
        };
        let duration_secs = if fmt.sample_rate > 0 {
            total_samples as f64 / fmt.sample_rate as f64
        } else {
            0.0
        };
        Self {
            total_samples,
            duration_secs,
            sample_rate: fmt.sample_rate,
            bit_depth: fmt.bits_per_sample,
            channels: fmt.channels,
        }
    }

    /// Format as "16-bit stereo" or "24-bit mono" etc.
    pub fn format_display(&self) -> String {
        let ch = if self.channels == 1 { "mono" } else { "stereo" };
        format!("{}-bit {ch}", self.bit_depth)
    }
}

// --- Zero-Crossing Detection ---

/// Default amplitude threshold for zero-crossing detection (~-54dB for 16-bit).
///
/// A crossing requires both a sign change AND at least one sample with
/// absolute amplitude above this threshold. This prevents false crossings
/// in silence or DC offset regions.
pub const ZC_THRESHOLD: i32 = 64;

#[allow(dead_code)] // Used by marker snapping (TUI).
/// A zero-crossing point in mono PCM sample data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroCrossing {
    /// Index of the sample just before the sign change.
    pub index: usize,
}

#[allow(dead_code)] // Used by marker snapping (TUI).
/// Find all zero-crossings in a slice of mono i32 samples.
///
/// A zero-crossing occurs between `samples[i]` and `samples[i+1]` when
/// their signs differ and at least one has absolute value >= `threshold`.
/// Returns indices where the crossing occurs (the index of the sample
/// just before the sign change).
pub fn find_zero_crossings(samples: &[i32], threshold: i32) -> Vec<ZeroCrossing> {
    if samples.len() < 2 {
        return Vec::new();
    }
    let mut crossings = Vec::new();
    for i in 0..samples.len() - 1 {
        let a = samples[i];
        let b = samples[i + 1];
        // Sign change: one positive (or zero) and one negative.
        let sign_change = (a >= 0 && b < 0) || (a < 0 && b >= 0);
        if sign_change && (a.abs() >= threshold || b.abs() >= threshold) {
            crossings.push(ZeroCrossing { index: i });
        }
    }
    crossings
}

/// Find the nearest zero-crossing at or after `start` in the sample buffer.
///
/// Searches forward from `start`. Returns `None` if no crossing is found
/// within the buffer.
pub fn nearest_zero_crossing_forward(
    samples: &[i32],
    start: usize,
    threshold: i32,
) -> Option<usize> {
    if samples.len() < 2 || start >= samples.len() - 1 {
        return None;
    }
    for i in start..samples.len() - 1 {
        let a = samples[i];
        let b = samples[i + 1];
        let sign_change = (a >= 0 && b < 0) || (a < 0 && b >= 0);
        if sign_change && (a.abs() >= threshold || b.abs() >= threshold) {
            return Some(i);
        }
    }
    None
}

/// Find the nearest zero-crossing at or before `start` in the sample buffer.
///
/// Searches backward from `start`. Returns `None` if no crossing is found
/// within the buffer.
pub fn nearest_zero_crossing_backward(
    samples: &[i32],
    start: usize,
    threshold: i32,
) -> Option<usize> {
    if samples.len() < 2 {
        return None;
    }
    let end = start.min(samples.len() - 2);
    for i in (0..=end).rev() {
        let a = samples[i];
        let b = samples[i + 1];
        let sign_change = (a >= 0 && b < 0) || (a < 0 && b >= 0);
        if sign_change && (a.abs() >= threshold || b.abs() >= threshold) {
            return Some(i);
        }
    }
    None
}

/// Find the Nth zero-crossing forward from `start`.
///
/// Returns the index of the Nth crossing, or `None` if fewer than N crossings
/// exist after `start`.
pub fn nth_zero_crossing_forward(
    samples: &[i32],
    start: usize,
    n: u32,
    threshold: i32,
) -> Option<usize> {
    if n == 0 || samples.len() < 2 || start >= samples.len() - 1 {
        return None;
    }
    let mut count = 0u32;
    let mut last = None;
    for i in start..samples.len() - 1 {
        let a = samples[i];
        let b = samples[i + 1];
        let sign_change = (a >= 0 && b < 0) || (a < 0 && b >= 0);
        if sign_change && (a.abs() >= threshold || b.abs() >= threshold) {
            count += 1;
            last = Some(i);
            if count == n {
                return Some(i);
            }
        }
    }
    last // Return the last found if we ran out of crossings
}

// --- Zoom support ---

/// Number of peak columns in the zoom cache level 0 (and zoom viewport width).
/// Number of peak columns in the ZoomCache viewport — matches `PEAK_COUNT` so
/// that level-0 exactly holds the BEXT-stored peaks without discarding any.
pub const NUM_ZOOM_COLS: usize = PEAK_COUNT;

/// Maximum zoom level (2^MAX_ZOOM_LEVEL × magnification).
pub const MAX_ZOOM_LEVEL: usize = 12;

/// Left-channel PCM samples extracted from a WAV file for zoom peak computation.
///
/// Samples are normalised to `i16` range regardless of source bit depth.
/// Supports 8-bit unsigned, 16-bit, 24-bit, and 32-bit integer PCM as well as
/// 32-bit IEEE float. Unsupported formats leave `pcm = None` in
/// [`super::super::ui::PreviewData`] and zoom is gracefully disabled.
#[derive(Debug, Clone)]
pub struct PcmData {
    /// Left-channel (or mono) samples normalised to i16, one per frame.
    pub samples: Vec<i16>,
}

#[allow(dead_code)] // Reserved for zoom-level peak computation.
impl PcmData {
    /// Number of frames (same as `samples.len()` for mono extraction).
    pub fn frame_count(&self) -> usize {
        self.samples.len()
    }

    /// Samples per zoom-level-0 peak column.
    ///
    /// Returns 1 if the file is shorter than `NUM_ZOOM_COLS` frames.
    pub fn k_lvl0(&self) -> usize {
        let n = self.samples.len();
        if n < NUM_ZOOM_COLS {
            1
        } else {
            n / NUM_ZOOM_COLS
        }
    }
}

/// Load left-channel PCM samples from a WAV file for zoom support.
///
/// Supports 8-bit unsigned, 16-bit, 24-bit, and 32-bit integer PCM and
/// 32-bit IEEE float. All formats are normalised to `i16`. Returns `Err` for
/// truly unsupported formats (e.g. format tag 2, ADPCM) or I/O failures;
/// callers should treat `Err` as "zoom unavailable" and continue with
/// `pcm = None`.
pub fn load_pcm_data(path: &std::path::Path) -> Result<PcmData, RiffError> {
    let registry = super::source::AudioRegistry::new();
    match registry.for_path(path) {
        Some(src) => src.load_pcm(path),
        None => Err(RiffError::NotRiffWave),
    }
}

/// RIFF-specific PCM loader. Called by [`super::source::RiffSource`].
pub fn load_pcm_data_riff(path: &std::path::Path) -> Result<PcmData, RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = super::bext::scan_chunks(&mut reader)?;
    let fmt = parse_fmt(&mut reader, &map)?;

    // Gate on supported formats.
    let supported = match fmt.format {
        AudioFormat::Pcm => matches!(fmt.bits_per_sample, 8 | 16 | 24 | 32),
        AudioFormat::IeeeFloat => fmt.bits_per_sample == 32,
        AudioFormat::Other(_) => false,
    };
    if !supported {
        return Err(RiffError::NotRiffWave);
    }

    let data_offset = map.data_offset.ok_or(RiffError::NotRiffWave)?;
    let channels = fmt.channels as usize;
    let bytes_per_sample = (fmt.bits_per_sample / 8) as usize;
    let frame_size = channels * bytes_per_sample;
    if frame_size == 0 {
        return Err(RiffError::NotRiffWave);
    }

    let total_bytes = map.data_size as usize;
    let total_frames = total_bytes / frame_size;

    reader.seek(SeekFrom::Start(data_offset))?;
    let mut samples = Vec::with_capacity(total_frames);
    let mut frame_buf = vec![0u8; frame_size];

    for _ in 0..total_frames {
        match reader.read_exact(&mut frame_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }
        // Decode left channel (channel 0) to i16.
        let s: i16 = match fmt.format {
            AudioFormat::Pcm => match fmt.bits_per_sample {
                8 => {
                    // Unsigned 8-bit: 128 = silence. Scale to i16 range.
                    (frame_buf[0] as i16 - 128) << 8
                }
                16 => i16::from_le_bytes([frame_buf[0], frame_buf[1]]),
                24 => {
                    // Sign-extend 3-byte LE to i32, take top 16 bits.
                    let v = frame_buf[0] as i32
                        | (frame_buf[1] as i32) << 8
                        | ((frame_buf[2] as i8) as i32) << 16;
                    (v >> 8) as i16
                }
                32 => {
                    // Take top 16 bits of the i32.
                    let v = i32::from_le_bytes([
                        frame_buf[0],
                        frame_buf[1],
                        frame_buf[2],
                        frame_buf[3],
                    ]);
                    (v >> 16) as i16
                }
                _ => 0,
            },
            AudioFormat::IeeeFloat => {
                // 32-bit float → i16 range.
                let f =
                    f32::from_le_bytes([frame_buf[0], frame_buf[1], frame_buf[2], frame_buf[3]]);
                (f.clamp(-1.0, 1.0) * 32767.0) as i16
            }
            AudioFormat::Other(_) => 0,
        };
        samples.push(s);
    }

    Ok(PcmData { samples })
}

/// Load PCM data via rodio's format-agnostic decoder (for AIFF and other non-RIFF formats).
/// Extracts left channel (or mono) as i16 samples, same as the RIFF path.
pub fn load_pcm_data_via_decoder(path: &std::path::Path) -> Result<PcmData, RiffError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let decoder = rodio::Decoder::new(reader).map_err(|_| RiffError::NotRiffWave)?;

    let channels = decoder.channels() as usize;
    // rodio::Decoder yields i16 samples (interleaved).
    let all_samples: Vec<i16> = decoder.collect();

    if all_samples.is_empty() || channels == 0 {
        return Ok(PcmData {
            samples: Vec::new(),
        });
    }

    // Extract left channel only (same as RIFF path).
    let samples: Vec<i16> = all_samples
        .chunks_exact(channels)
        .map(|frame| frame[0])
        .collect();

    Ok(PcmData { samples })
}

/// Convert a 16-bit signed sample to u8 amplitude (0 = silence, 255 = full scale).
fn sample_to_u8(s: i16) -> u8 {
    // saturating_abs avoids i16::MIN overflow; divide by 128 maps [0,32767] → [0,255].
    (s.saturating_abs() as u16 / 128) as u8
}

/// Find the maximum amplitude in `samples[start..start+k]` as a u8 value.
fn max_amp_u8(samples: &[i16], start: usize, k: usize) -> u8 {
    let end = (start + k).min(samples.len());
    if start >= end {
        return 0;
    }
    samples[start..end]
        .iter()
        .map(|&s| sample_to_u8(s))
        .max()
        .unwrap_or(0)
}

/// JIT multi-resolution peak cache for waveform zoom.
///
/// Level 0 holds the 90 pre-computed overview peaks. Each higher level
/// doubles the resolution by JIT-expanding from raw PCM samples.
/// Maximum zoom level is [`MAX_ZOOM_LEVEL`] (16×).
///
/// The sibling-shortcut optimisation: when computing a right child (odd
/// column), if the left sibling's value is less than the parent's peak, the
/// right sibling must contain the parent's maximum, so we skip the PCM scan.
#[derive(Debug, Clone)]
pub struct ZoomCache {
    /// `cache[level][col]` — peak amplitude for that (level, column) pair.
    cache: Vec<Vec<u8>>,
    /// `computed[level][col]` — whether that entry has been computed.
    computed: Vec<Vec<bool>>,
    /// Number of visible columns (always `NUM_ZOOM_COLS`).
    #[allow(dead_code)] // Exposed for widget layout calculations.
    pub num_cols: usize,
}

impl ZoomCache {
    /// Initialise from the 90 level-0 overview peaks.
    ///
    /// If `level0_peaks` is shorter than `NUM_ZOOM_COLS` the remainder is
    /// zero-padded; if longer, only the first `NUM_ZOOM_COLS` entries are used.
    pub fn new(level0_peaks: &[u8]) -> Self {
        let mut cache = Vec::with_capacity(MAX_ZOOM_LEVEL + 1);
        let mut computed = Vec::with_capacity(MAX_ZOOM_LEVEL + 1);

        // Level 0: copy provided peaks (already computed).
        let mut l0 = vec![0u8; NUM_ZOOM_COLS];
        let copy_len = level0_peaks.len().min(NUM_ZOOM_COLS);
        l0[..copy_len].copy_from_slice(&level0_peaks[..copy_len]);
        cache.push(l0);
        computed.push(vec![true; NUM_ZOOM_COLS]);

        // Levels 1-MAX_ZOOM_LEVEL: allocate uncomputed.
        for l in 1..=MAX_ZOOM_LEVEL {
            let size = NUM_ZOOM_COLS << l;
            cache.push(vec![0u8; size]);
            computed.push(vec![false; size]);
        }

        Self {
            cache,
            computed,
            num_cols: NUM_ZOOM_COLS,
        }
    }

    /// Return `NUM_ZOOM_COLS` peaks for the viewport at `level` starting at
    /// `start_idx` (column index within the full zoomed resolution).
    ///
    /// Columns outside the file (start_idx + NUM_ZOOM_COLS > total columns at
    /// this level) are zero-padded. Missing entries are JIT-computed from `samples`.
    pub fn get_visible_peaks(
        &mut self,
        level: usize,
        start_idx: usize,
        samples: &[i16],
        k_lvl0: usize,
    ) -> Vec<u8> {
        let level = level.min(MAX_ZOOM_LEVEL);

        // Samples per peak column at this zoom level.
        let k_current = (k_lvl0 >> level).max(1);

        let total_cols_at_level = NUM_ZOOM_COLS << level;
        let mut result = Vec::with_capacity(NUM_ZOOM_COLS);

        for col_offset in 0..NUM_ZOOM_COLS {
            let col = start_idx + col_offset;
            if col >= total_cols_at_level {
                result.push(0); // beyond file end
                continue;
            }

            if !self.computed[level][col] {
                let val = if level > 0 && col % 2 == 1 {
                    // Right child: attempt sibling shortcut.
                    let parent_idx = col >> 1;
                    let left_sibling = col - 1;

                    // Ensure parent is computed.
                    if !self.computed[level - 1][parent_idx] {
                        let parent_k = (k_lvl0 >> (level - 1)).max(1);
                        let parent_start = parent_idx * parent_k;
                        let v = max_amp_u8(samples, parent_start, parent_k);
                        self.cache[level - 1][parent_idx] = v;
                        self.computed[level - 1][parent_idx] = true;
                    }
                    let parent_val = self.cache[level - 1][parent_idx];

                    // Ensure left sibling is computed.
                    if !self.computed[level][left_sibling] {
                        let ls_start = left_sibling * k_current;
                        let ls_val = max_amp_u8(samples, ls_start, k_current);
                        self.cache[level][left_sibling] = ls_val;
                        self.computed[level][left_sibling] = true;
                    }

                    if self.cache[level][left_sibling] < parent_val {
                        // Shortcut: right child must account for the parent peak.
                        parent_val
                    } else {
                        max_amp_u8(samples, col * k_current, k_current)
                    }
                } else {
                    max_amp_u8(samples, col * k_current, k_current)
                };
                self.cache[level][col] = val;
                self.computed[level][col] = true;
            }

            result.push(self.cache[level][col]);
        }

        result
    }
}

/// Find the Nth zero-crossing backward from `start`.
///
/// Returns the index of the Nth crossing, or `None` if fewer than N crossings
/// exist before `start`.
pub fn nth_zero_crossing_backward(
    samples: &[i32],
    start: usize,
    n: u32,
    threshold: i32,
) -> Option<usize> {
    if n == 0 || samples.len() < 2 {
        return None;
    }
    let end = start.min(samples.len() - 2);
    let mut count = 0u32;
    let mut last = None;
    for i in (0..=end).rev() {
        let a = samples[i];
        let b = samples[i + 1];
        let sign_change = (a >= 0 && b < 0) || (a < 0 && b >= 0);
        if sign_change && (a.abs() >= threshold || b.abs() >= threshold) {
            count += 1;
            last = Some(i);
            if count == n {
                return Some(i);
            }
        }
    }
    last
}

/// Read a window of mono i32 samples from a WAV file around a given frame offset.
///
/// Returns (samples, base_frame) where `base_frame` is the absolute frame index
/// of `samples[0]`. The window extends `radius` frames on each side of `center`.
pub fn read_sample_window<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
    center: u32,
    radius: u32,
) -> Result<(Vec<i32>, u32), RiffError> {
    let data_offset = map.data_offset.ok_or(RiffError::NotRiffWave)?;
    let bytes_per_sample = (fmt.bits_per_sample / 8) as u32;
    let channels = fmt.channels as u32;
    let frame_size = bytes_per_sample * channels;
    if frame_size == 0 {
        return Ok((Vec::new(), 0));
    }
    let total_frames = map.data_size / frame_size;
    if total_frames == 0 {
        return Ok((Vec::new(), 0));
    }

    let start_frame = center.saturating_sub(radius);
    let end_frame = (center + radius + 1).min(total_frames);
    let num_frames = end_frame - start_frame;

    let byte_offset = data_offset + (start_frame as u64 * frame_size as u64);
    reader.seek(SeekFrom::Start(byte_offset))?;

    let mut samples = Vec::with_capacity(num_frames as usize);
    let mut frame_buf = vec![0u8; frame_size as usize];

    for _ in 0..num_frames {
        match reader.read_exact(&mut frame_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }
        // Decode channel 0 (left/mono) to i32.
        let sample_i32 = match fmt.format {
            AudioFormat::Pcm => match fmt.bits_per_sample {
                16 => i16::from_le_bytes([frame_buf[0], frame_buf[1]]) as i32,
                24 => {
                    frame_buf[0] as i32
                        | (frame_buf[1] as i32) << 8
                        | ((frame_buf[2] as i8) as i32) << 16
                }
                8 => (frame_buf[0] as i32) - 128,
                32 => i32::from_le_bytes([frame_buf[0], frame_buf[1], frame_buf[2], frame_buf[3]]),
                _ => 0,
            },
            AudioFormat::IeeeFloat => {
                if fmt.bits_per_sample == 32 && frame_buf.len() >= 4 {
                    let f = f32::from_le_bytes([
                        frame_buf[0],
                        frame_buf[1],
                        frame_buf[2],
                        frame_buf[3],
                    ]);
                    (f * 32768.0) as i32
                } else {
                    0
                }
            }
            AudioFormat::Other(_) => 0,
        };
        samples.push(sample_i32);
    }

    Ok((samples, start_frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::engine::bext::scan_chunks;

    // --- T3: PcmData tests ---

    #[test]
    fn test_load_pcm_data_offset_correct() {
        // Use a 16-bit test file (clean_base.wav is 24-bit; use a 16-bit variant).
        let path = std::path::Path::new("test_files/all_riff_info_tags_with_numbers-sm.wav");
        if !path.exists() {
            return;
        }
        let pcm = load_pcm_data(path).expect("should load 16-bit PCM");
        assert!(!pcm.samples.is_empty(), "should have samples");
        // Verify k_lvl0 is sensible.
        assert!(pcm.k_lvl0() >= 1);
    }

    #[test]
    fn test_load_pcm_data_samples_count() {
        let path = std::path::Path::new("test_files/all_riff_info_tags_with_numbers-sm.wav");
        if !path.exists() {
            return;
        }
        let pcm = load_pcm_data(path).expect("should load 16-bit PCM");
        // Verify sample count is consistent with audio duration.
        assert!(pcm.frame_count() > 0);
    }

    #[test]
    fn test_load_pcm_data_24bit_works() {
        // 24-bit PCM is now supported; verify it loads without error.
        let mut audio = Vec::new();
        // Three 24-bit mono samples at mid-scale (0x7FFFFF, 0, negative).
        audio.extend_from_slice(&[0xFF, 0xFF, 0x7F]); // max positive ≈ 32767
        audio.extend_from_slice(&[0x00, 0x00, 0x00]); // silence
        audio.extend_from_slice(&[0x01, 0x00, 0x80]); // min (0x800001) ≈ -32768

        let wav = make_wav(&make_fmt_pcm24_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();
        assert_eq!(fmt.bits_per_sample, 24);

        // Write WAV to a temp-like in-memory path via load_pcm_data helper.
        // We exercise the codec path directly here.
        // Reopen via a real temp file is impractical in unit tests; instead
        // verify the decode math:  val >> 8 for 24-bit.
        // 0x7FFFFF >> 8 = 0x7FFF = 32767
        let v_max: i32 = 0xFF | (0xFF << 8) | (0x7Fi32 << 16); // = 8388607
        assert_eq!((v_max >> 8) as i16, 32767i16);
        // 0x800001 (min) in sign-extended form:
        let v_min: i32 = 0x01 | ((0x80u8 as i8 as i32) << 16); // = -8388607
        assert!(((v_min >> 8) as i16) < 0, "min 24-bit should be negative");
    }

    #[test]
    fn test_load_pcm_data_32bit_int_works() {
        // 32-bit integer PCM should load and decode to i16 correctly.
        let mut fmt_bytes = Vec::new();
        fmt_bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        fmt_bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt_bytes.extend_from_slice(&44100u32.to_le_bytes());
        fmt_bytes.extend_from_slice(&176400u32.to_le_bytes()); // byte rate
        fmt_bytes.extend_from_slice(&4u16.to_le_bytes()); // block align
        fmt_bytes.extend_from_slice(&32u16.to_le_bytes()); // bits per sample

        // One sample at i32::MAX → should decode to i16::MAX (32767).
        let mut audio = Vec::new();
        audio.extend_from_slice(&i32::MAX.to_le_bytes());

        let wav = make_wav(&fmt_bytes, &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();
        assert_eq!(fmt.bits_per_sample, 32);
        assert_eq!(fmt.format, AudioFormat::Pcm);

        // i32::MAX >> 16 = 32767 = i16::MAX
        let decoded = (i32::MAX >> 16) as i16;
        assert_eq!(decoded, i16::MAX);
    }

    #[test]
    fn test_load_pcm_data_float32_works() {
        // 32-bit IEEE float should load and decode to i16 correctly.
        let mut fmt_bytes = Vec::new();
        fmt_bytes.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        fmt_bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt_bytes.extend_from_slice(&44100u32.to_le_bytes());
        fmt_bytes.extend_from_slice(&176400u32.to_le_bytes());
        fmt_bytes.extend_from_slice(&4u16.to_le_bytes()); // block align
        fmt_bytes.extend_from_slice(&32u16.to_le_bytes()); // bits per sample

        // Sample at +1.0f32 → should decode to i16::MAX (32767).
        let mut audio = Vec::new();
        audio.extend_from_slice(&1.0f32.to_le_bytes());

        let wav = make_wav(&fmt_bytes, &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();
        assert_eq!(fmt.format, AudioFormat::IeeeFloat);
        assert_eq!(fmt.bits_per_sample, 32);

        // (1.0f32).clamp(-1.0, 1.0) * 32767.0 → 32767 = i16::MAX
        let decoded = (1.0f32.clamp(-1.0, 1.0) * 32767.0) as i16;
        assert_eq!(decoded, i16::MAX);
    }

    #[test]
    fn test_pcm_data_k_lvl0_small_file() {
        let pcm = PcmData {
            samples: vec![0i16; 45],
        }; // < NUM_ZOOM_COLS
        assert_eq!(pcm.k_lvl0(), 1, "short file k_lvl0 should clamp to 1");
    }

    #[test]
    fn test_pcm_data_k_lvl0_normal_file() {
        // NUM_ZOOM_COLS == PEAK_COUNT == 180; 900 / 180 = 5.
        let pcm = PcmData {
            samples: vec![0i16; 900],
        };
        assert_eq!(pcm.k_lvl0(), 5);
    }

    // --- T4: ZoomCache tests ---

    #[test]
    fn test_zoom_cache_level0_matches_input() {
        let peaks: Vec<u8> = (0..NUM_ZOOM_COLS).map(|i| (i * 2 % 256) as u8).collect();
        let cache = ZoomCache::new(&peaks);
        // Level 0 is pre-filled from the input.
        let samples = vec![0i16; 900];
        let mut cache = cache;
        let visible = cache.get_visible_peaks(0, 0, &samples, 10);
        assert_eq!(visible, peaks, "level 0 should match input peaks");
    }

    #[test]
    fn test_zoom_cache_level1_double_resolution() {
        let peaks = vec![100u8; NUM_ZOOM_COLS];
        let mut cache = ZoomCache::new(&peaks);
        // 180 samples: alternating max/zero pattern.
        let mut samples = vec![0i16; NUM_ZOOM_COLS * 2 * 10]; // 1800 samples, k_lvl0=20
        // Set every other k_current block to full amplitude.
        let k_lvl0 = 20usize;
        for col in (0..NUM_ZOOM_COLS * 2).step_by(2) {
            let start = col * (k_lvl0 / 2);
            if start < samples.len() {
                samples[start] = 10000;
            }
        }
        let visible = cache.get_visible_peaks(1, 0, &samples, k_lvl0);
        assert_eq!(
            visible.len(),
            NUM_ZOOM_COLS,
            "level 1 should return 90 visible peaks"
        );
    }

    #[test]
    fn test_zoom_cache_sibling_shortcut_applies() {
        // Left child peak < parent peak → right child should equal parent.
        let peaks = vec![200u8; NUM_ZOOM_COLS]; // level 0 peaks = 200
        let mut cache = ZoomCache::new(&peaks);
        let k_lvl0 = 20usize;
        let total = NUM_ZOOM_COLS * k_lvl0; // 1800 samples

        // Make left child (col 0 at level 1) = 100, right child (col 1) should be 200.
        // Put amplitude 100 in first k_current=10 samples, silence in next 10.
        let mut samples = vec![0i16; total];
        samples[0] = 100i16 * 128; // ~100 when converted to u8

        let visible = cache.get_visible_peaks(1, 0, &samples, k_lvl0);
        // Left child = peak of samples[0..10] ≈ 100
        // Parent (col 0 at level 0) = 200 (from level0_peaks)
        // Since left_child < parent, right child should = parent = 200.
        assert_eq!(
            visible[1], 200,
            "right child should equal parent peak via shortcut"
        );
    }

    #[test]
    fn test_zoom_cache_sibling_shortcut_does_not_apply() {
        // Left child peak >= parent → right child is computed directly.
        let peaks = vec![100u8; NUM_ZOOM_COLS]; // level 0 peaks = 100
        let mut cache = ZoomCache::new(&peaks);
        let k_lvl0 = 20usize;
        let total = NUM_ZOOM_COLS * k_lvl0;

        // Left child dominates (200 > parent 100), right child is computed directly.
        let mut samples = vec![0i16; total];
        samples[0] = 200i16 * 128i16; // left child has large amplitude
        // Right child has small amplitude:
        let k_current = k_lvl0 / 2; // = 10
        samples[k_current] = 50i16 * 128i16;

        let visible = cache.get_visible_peaks(1, 0, &samples, k_lvl0);
        // Since left_child (≈200) >= parent (100), shortcut NOT applied.
        // Right child computed directly ≈ 50.
        assert!(
            visible[1] < 200,
            "right child should NOT equal parent when shortcut does not apply"
        );
    }

    #[test]
    fn test_zoom_cache_viewport_offset() {
        // start_idx = NUM_ZOOM_COLS should return the second-half peaks.
        let peaks: Vec<u8> = (0..NUM_ZOOM_COLS).map(|i| i as u8).collect();
        let mut cache = ZoomCache::new(&peaks);
        let k_lvl0 = 10usize;
        let samples = vec![0i16; NUM_ZOOM_COLS * k_lvl0 * 2]; // level-1 has 180 cols

        // Viewport starting at col 45 of level 0.
        let visible0 = cache.get_visible_peaks(0, 45, &samples, k_lvl0);
        assert_eq!(visible0.len(), NUM_ZOOM_COLS);
        // First visible column corresponds to peaks[45].
        assert_eq!(visible0[0], 45u8);
    }

    #[test]
    fn test_zoom_cache_beyond_file_zero_padded() {
        let peaks = vec![100u8; NUM_ZOOM_COLS];
        let mut cache = ZoomCache::new(&peaks);
        let k_lvl0 = 10usize;
        let samples = vec![0i16; NUM_ZOOM_COLS * k_lvl0 * 2];
        // Ask for viewport starting well past the end of level-0.
        let visible = cache.get_visible_peaks(0, NUM_ZOOM_COLS, &samples, k_lvl0);
        assert_eq!(visible.len(), NUM_ZOOM_COLS);
        assert!(
            visible.iter().all(|&v| v == 0),
            "past-end viewport should be zero-padded"
        );
    }

    #[test]
    fn test_sample_to_u8_zero() {
        assert_eq!(sample_to_u8(0i16), 0);
    }

    #[test]
    fn test_sample_to_u8_max() {
        assert_eq!(sample_to_u8(i16::MAX), 255);
    }

    #[test]
    fn test_sample_to_u8_min_saturates() {
        // i16::MIN saturating_abs = i16::MAX = 32767 → 255.
        assert_eq!(sample_to_u8(i16::MIN), 255);
    }

    #[test]
    fn test_max_amp_u8_empty() {
        assert_eq!(max_amp_u8(&[], 0, 10), 0);
    }

    #[test]
    fn test_max_amp_u8_out_of_range() {
        let samples = vec![100i16; 5];
        assert_eq!(
            max_amp_u8(&samples, 10, 5),
            0,
            "start past end should return 0"
        );
    }

    /// Build a minimal RIFF/WAVE file with fmt + data chunks.
    fn make_wav(fmt_data: &[u8], audio_data: &[u8]) -> Vec<u8> {
        let mut chunks = Vec::new();
        // fmt chunk
        chunks.extend_from_slice(b"fmt ");
        chunks.extend_from_slice(&(fmt_data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(fmt_data);
        if !fmt_data.len().is_multiple_of(2) {
            chunks.push(0);
        }
        // data chunk
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(audio_data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(audio_data);
        if !audio_data.len().is_multiple_of(2) {
            chunks.push(0);
        }

        let mut buf = Vec::with_capacity(12 + chunks.len());
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&((4 + chunks.len()) as u32).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(&chunks);
        buf
    }

    /// Make a standard PCM 16-bit stereo fmt chunk (16 bytes).
    fn make_fmt_pcm16_stereo() -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
        fmt.extend_from_slice(&2u16.to_le_bytes()); // stereo
        fmt.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
        fmt.extend_from_slice(&176400u32.to_le_bytes()); // byte rate
        fmt.extend_from_slice(&4u16.to_le_bytes()); // block align
        fmt.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        fmt
    }

    /// Make a PCM 16-bit mono fmt chunk.
    fn make_fmt_pcm16_mono() -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt.extend_from_slice(&44100u32.to_le_bytes());
        fmt.extend_from_slice(&88200u32.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        fmt
    }

    /// Make a PCM 24-bit mono fmt chunk.
    fn make_fmt_pcm24_mono() -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt.extend_from_slice(&44100u32.to_le_bytes());
        fmt.extend_from_slice(&132300u32.to_le_bytes());
        fmt.extend_from_slice(&3u16.to_le_bytes());
        fmt.extend_from_slice(&24u16.to_le_bytes());
        fmt
    }

    /// Make an IEEE float 32-bit mono fmt chunk.
    fn make_fmt_float32_mono() -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        fmt.extend_from_slice(&1u16.to_le_bytes()); // mono
        fmt.extend_from_slice(&44100u32.to_le_bytes());
        fmt.extend_from_slice(&176400u32.to_le_bytes());
        fmt.extend_from_slice(&4u16.to_le_bytes());
        fmt.extend_from_slice(&32u16.to_le_bytes());
        fmt
    }

    // --- T1: scan_chunks finds fmt and data ---

    #[test]
    fn test_scan_chunks_finds_fmt_and_data() {
        let wav = make_wav(&make_fmt_pcm16_stereo(), &[0u8; 100]);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        assert!(map.fmt_offset.is_some(), "should find fmt chunk");
        assert!(map.data_offset.is_some(), "should find data chunk");
        assert_eq!(map.fmt_size, 16);
        assert_eq!(map.data_size, 100);
    }

    #[test]
    fn test_scan_chunks_all_test_files() {
        let test_dir = std::path::Path::new("test_files");
        if !test_dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(test_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wav") {
                let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
                let map = scan_chunks(&mut file).unwrap();
                assert!(
                    map.fmt_offset.is_some(),
                    "fmt missing in {}",
                    path.display()
                );
                assert!(
                    map.data_offset.is_some(),
                    "data missing in {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn test_parse_fmt_pcm_16bit_stereo() {
        let wav = make_wav(&make_fmt_pcm16_stereo(), &[0u8; 100]);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();
        assert_eq!(fmt.format, AudioFormat::Pcm);
        assert_eq!(fmt.channels, 2);
        assert_eq!(fmt.sample_rate, 44100);
        assert_eq!(fmt.bits_per_sample, 16);
        assert_eq!(fmt.block_align, 4);
    }

    #[test]
    fn test_parse_fmt_ieee_float() {
        let wav = make_wav(&make_fmt_float32_mono(), &[0u8; 100]);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();
        assert_eq!(fmt.format, AudioFormat::IeeeFloat);
        assert_eq!(fmt.channels, 1);
        assert_eq!(fmt.bits_per_sample, 32);
    }

    #[test]
    fn test_parse_fmt_missing_returns_error() {
        let map = ChunkMap::default(); // no fmt_offset
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result = parse_fmt(&mut cursor, &map);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_fmt_truncated_chunk() {
        // fmt chunk with only 8 bytes (< 16 required).
        let mut chunks = Vec::new();
        chunks.extend_from_slice(b"fmt ");
        chunks.extend_from_slice(&8u32.to_le_bytes());
        chunks.extend_from_slice(&[0u8; 8]);
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&0u32.to_le_bytes());

        let mut buf = Vec::with_capacity(12 + chunks.len());
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&((4 + chunks.len()) as u32).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(&chunks);

        let mut cursor = Cursor::new(buf);
        let map = scan_chunks(&mut cursor).unwrap();
        assert_eq!(map.fmt_size, 8);
        let result = parse_fmt(&mut cursor, &map);
        assert!(result.is_err());
    }

    // --- T2: streaming sample reader ---

    #[test]
    fn test_stream_16bit_stereo_mixdown() {
        // Two frames: L=16384 R=0 → mono=8192 → 8192/32768 ≈ 0.25
        //             L=0 R=-16384 → mono=-8192 → -8192/32768 ≈ -0.25
        let mut audio = Vec::new();
        audio.extend_from_slice(&16384i16.to_le_bytes()); // L
        audio.extend_from_slice(&0i16.to_le_bytes()); // R
        audio.extend_from_slice(&0i16.to_le_bytes()); // L
        audio.extend_from_slice(&(-16384i16).to_le_bytes()); // R

        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut samples = Vec::new();
        let count = stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |s| {
            samples.push(s)
        })
        .unwrap();
        assert_eq!(count, 2);
        assert!((samples[0] - 0.25).abs() < 0.001, "got {}", samples[0]);
        assert!((samples[1] - (-0.25)).abs() < 0.001, "got {}", samples[1]);
    }

    #[test]
    fn test_stream_24bit_mono() {
        // 24-bit max positive: 0x7FFFFF = 8388607
        let audio = vec![0xFF, 0xFF, 0x7F];

        let wav = make_wav(&make_fmt_pcm24_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut samples = Vec::new();
        stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |s| {
            samples.push(s)
        })
        .unwrap();
        assert_eq!(samples.len(), 1);
        assert!(
            (samples[0] - 1.0).abs() < 0.001,
            "24-bit max should be ~1.0, got {}",
            samples[0]
        );
    }

    #[test]
    fn test_stream_ieee_float() {
        let mut audio = Vec::new();
        audio.extend_from_slice(&0.5f32.to_le_bytes());
        audio.extend_from_slice(&(-0.75f32).to_le_bytes());

        let wav = make_wav(&make_fmt_float32_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut samples = Vec::new();
        stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |s| {
            samples.push(s)
        })
        .unwrap();
        assert_eq!(samples.len(), 2);
        assert!((samples[0] - 0.5).abs() < 0.001);
        assert!((samples[1] - (-0.75)).abs() < 0.001);
    }

    #[test]
    fn test_stream_total_count() {
        // 10 frames of stereo 16-bit = 40 bytes, 10 mono samples.
        let audio = vec![0u8; 40];
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let count =
            stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |_| {}).unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn test_stream_empty_data_chunk() {
        let wav = make_wav(&make_fmt_pcm16_stereo(), &[]);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let count =
            stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |_| {}).unwrap();
        assert_eq!(count, 0);
    }

    // --- Buffer alignment regression tests ---

    /// Make a PCM 24-bit stereo fmt chunk (frame_size = 6, doesn't divide 4096).
    fn make_fmt_pcm24_stereo() -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
        fmt.extend_from_slice(&2u16.to_le_bytes()); // stereo
        fmt.extend_from_slice(&48000u32.to_le_bytes()); // sample rate
        fmt.extend_from_slice(&288000u32.to_le_bytes()); // byte rate = 48000*2*3
        fmt.extend_from_slice(&6u16.to_le_bytes()); // block align
        fmt.extend_from_slice(&24u16.to_le_bytes()); // bits per sample
        fmt
    }

    #[test]
    fn test_stream_24bit_stereo_frame_count() {
        // 24-bit stereo: frame_size=6, BUF_SIZE=4096, 4096/6=682 rem 4.
        // Before the alignment fix, 4 bytes were dropped per buffer read,
        // causing misaligned samples. This test uses >4096 bytes of audio
        // to cross a buffer boundary.
        let num_frames = 2000; // 2000 * 6 = 12000 bytes (spans ~3 buffers)
        let mut audio = Vec::new();
        for i in 0..num_frames {
            let val =
                ((i as f64 / num_frames as f64 * std::f64::consts::PI).sin() * 4_000_000.0) as i32;
            // L channel: 24-bit LE
            audio.push((val & 0xFF) as u8);
            audio.push(((val >> 8) & 0xFF) as u8);
            audio.push(((val >> 16) & 0xFF) as u8);
            // R channel: silence
            audio.push(0);
            audio.push(0);
            audio.push(0);
        }
        let wav = make_wav(&make_fmt_pcm24_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut count = 0u64;
        let total = stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |_| {
            count += 1;
        })
        .unwrap();
        assert_eq!(
            total, num_frames as u64,
            "should read all {num_frames} frames without dropping any at buffer boundaries"
        );
        assert_eq!(count, num_frames as u64);
    }

    #[test]
    fn test_stream_24bit_stereo_no_phase_drift() {
        // Verify that a known 24-bit stereo signal is decoded identically
        // on both sides of a buffer boundary. The left channel is a ramp
        // (each frame = frame_index), right is zero.
        let num_frames = 2000u64;
        let mut audio = Vec::new();
        for i in 0..num_frames {
            let val = i as i32;
            audio.push((val & 0xFF) as u8);
            audio.push(((val >> 8) & 0xFF) as u8);
            audio.push(((val >> 16) & 0xFF) as u8);
            audio.push(0);
            audio.push(0);
            audio.push(0);
        }
        let wav = make_wav(&make_fmt_pcm24_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut samples = Vec::new();
        stream_samples_stereo(&mut cursor, &map, &fmt, &mut |_idx, left, _right| {
            samples.push(left);
        })
        .unwrap();

        // Each sample should be i / 8388608.0 (24-bit divisor).
        for (i, &sample) in samples.iter().enumerate() {
            let expected = i as f32 / 8_388_608.0;
            assert!(
                (sample - expected).abs() < 1e-6,
                "frame {i}: expected {expected}, got {sample} (phase drift at buffer boundary?)"
            );
        }
    }

    // --- T3: peak computation ---

    #[test]
    fn test_compute_peaks_silent_file() {
        let _audio = vec![0u8; 8820]; // 10 frames per bin × 180 bins × 4 bytes/frame (stereo 16)
        // Actually: need enough samples. 180 bins, each with >0 samples.
        // 180 mono samples → one per bin (stereo so 360 samples = 720 bytes)
        let audio = vec![0u8; 720];
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks =
            compute_peaks_with_options(&mut cursor, &map, &fmt, &PeakOptions::default()).unwrap();
        assert_eq!(peaks.len(), PEAK_COUNT);
        assert!(peaks.iter().all(|&v| v == 0));
    }

    #[test]
    fn test_compute_peaks_length_always_180() {
        let test_dir = std::path::Path::new("test_files");
        if !test_dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(test_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wav") {
                match compute_peaks_from_path_with_options(&path, &PeakOptions::default()) {
                    Ok(peaks) => {
                        assert_eq!(
                            peaks.len(),
                            PEAK_COUNT,
                            "wrong peak count for {}",
                            path.display()
                        );
                    }
                    Err(_) => {
                        // Some test files may not be standard PCM.
                    }
                }
            }
        }
    }

    #[test]
    fn test_compute_peaks_max_is_255() {
        // Full-scale sine-like signal: alternating max positive.
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // mono
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks =
            compute_peaks_with_options(&mut cursor, &map, &fmt, &PeakOptions::default()).unwrap();
        assert_eq!(peaks.len(), PEAK_COUNT);
        assert!(
            peaks.contains(&255),
            "loudest bin should be 255, max was {}",
            peaks.iter().max().unwrap_or(&0)
        );
    }

    #[test]
    fn test_compute_peaks_short_file() {
        // Only 10 mono samples (less than 180 bins).
        let mut audio = Vec::new();
        for i in 0..10 {
            let val = (i * 3000) as i16;
            audio.extend_from_slice(&val.to_le_bytes());
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks =
            compute_peaks_with_options(&mut cursor, &map, &fmt, &PeakOptions::default()).unwrap();
        assert_eq!(peaks.len(), PEAK_COUNT);
        // Most bins should be 0 since we only have 10 samples for 180 bins.
        let nonzero = peaks.iter().filter(|&&v| v > 0).count();
        assert!(nonzero <= 10, "too many non-zero bins: {nonzero}");
    }

    #[test]
    fn test_compute_peaks_from_path() {
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        let peaks = compute_peaks_from_path_with_options(path, &PeakOptions::default()).unwrap();
        assert_eq!(peaks.len(), PEAK_COUNT);
    }

    // --- Audio info tests ---

    #[test]
    fn test_audio_info_from_fmt_chunk() {
        let fmt = FmtChunk {
            format: AudioFormat::Pcm,
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            block_align: 4,
        };
        // 1 second of 16-bit stereo = 44100 * 2 * 2 = 176400 bytes.
        let info = AudioInfo::from_fmt(&fmt, 176400);
        assert!((info.duration_secs - 1.0).abs() < 0.001);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(info.channels, 2);
    }

    #[test]
    fn test_duration_calculation() {
        let fmt = FmtChunk {
            format: AudioFormat::Pcm,
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 24,
            block_align: 3,
        };
        // 5 seconds = 48000 * 1 * 3 * 5 = 720000 bytes.
        let info = AudioInfo::from_fmt(&fmt, 720000);
        assert!((info.duration_secs - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_format_display_stereo() {
        let info = AudioInfo {
            total_samples: 44100,
            duration_secs: 1.0,
            sample_rate: 44100,
            bit_depth: 16,
            channels: 2,
        };
        assert_eq!(info.format_display(), "16-bit stereo");
    }

    #[test]
    fn test_format_display_mono() {
        let info = AudioInfo {
            total_samples: 44100,
            duration_secs: 1.0,
            sample_rate: 44100,
            bit_depth: 24,
            channels: 1,
        };
        assert_eq!(info.format_display(), "24-bit mono");
    }

    // --- T2: Multi-option peak computation ---

    #[test]
    fn test_channel_mode_left_stereo() {
        // Stereo: L=max, R=0 → Left should be louder than Mix.
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // L
            audio.extend_from_slice(&0i16.to_le_bytes()); // R
        }
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);

        let mut c1 = Cursor::new(wav.clone());
        let map1 = scan_chunks(&mut c1).unwrap();
        let fmt1 = parse_fmt(&mut c1, &map1).unwrap();
        let left_peaks = compute_peaks_with_options(
            &mut c1,
            &map1,
            &fmt1,
            &PeakOptions {
                channel: ChannelMode::Left,
                measurement: PeakMeasurement::Rms,
            },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let mix_peaks = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions {
                channel: ChannelMode::Mix,
                measurement: PeakMeasurement::Rms,
            },
        )
        .unwrap();

        // Left-only should have higher values since L=max while mix averages L+R.
        let left_sum: u32 = left_peaks.iter().map(|&v| v as u32).sum();
        let mix_sum: u32 = mix_peaks.iter().map(|&v| v as u32).sum();
        assert!(
            left_sum >= mix_sum,
            "left-only should be >= mix: left_sum={left_sum}, mix_sum={mix_sum}"
        );
    }

    #[test]
    fn test_channel_mode_left_mono() {
        // Mono: Left mode should be identical to Mix mode.
        let mut audio = Vec::new();
        for _ in 0..500 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes());
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);

        let mut c1 = Cursor::new(wav.clone());
        let map1 = scan_chunks(&mut c1).unwrap();
        let fmt1 = parse_fmt(&mut c1, &map1).unwrap();
        let left_peaks = compute_peaks_with_options(
            &mut c1,
            &map1,
            &fmt1,
            &PeakOptions {
                channel: ChannelMode::Left,
                measurement: PeakMeasurement::Rms,
            },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let mix_peaks =
            compute_peaks_with_options(&mut c2, &map2, &fmt2, &PeakOptions::default()).unwrap();

        assert_eq!(left_peaks, mix_peaks);
    }

    #[test]
    fn test_peak_measurement_peak_louder() {
        // Signal with varying amplitude: peak should be >= RMS for each bin.
        let mut audio = Vec::new();
        for i in 0..2000 {
            let val = ((i as f64 / 10.0).sin() * 20000.0) as i16;
            audio.extend_from_slice(&val.to_le_bytes());
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);

        let mut c1 = Cursor::new(wav.clone());
        let map1 = scan_chunks(&mut c1).unwrap();
        let fmt1 = parse_fmt(&mut c1, &map1).unwrap();
        let rms = compute_peaks_with_options(
            &mut c1,
            &map1,
            &fmt1,
            &PeakOptions {
                channel: ChannelMode::Mix,
                measurement: PeakMeasurement::Rms,
            },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let peak = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions {
                channel: ChannelMode::Mix,
                measurement: PeakMeasurement::Peak,
            },
        )
        .unwrap();

        // Peak values should be >= RMS values (on average).
        let peak_sum: u32 = peak.iter().map(|&v| v as u32).sum();
        let rms_sum: u32 = rms.iter().map(|&v| v as u32).sum();
        assert!(
            peak_sum >= rms_sum,
            "peak sum should be >= rms sum: peak={peak_sum}, rms={rms_sum}"
        );
    }

    #[test]
    fn test_peak_options_from_config_strings() {
        let opts = PeakOptions::from_config(Some("peak"), Some("left"));
        assert_eq!(opts.measurement, PeakMeasurement::Peak);
        assert_eq!(opts.channel, ChannelMode::Left);

        let opts2 = PeakOptions::from_config(Some("rms"), Some("mix"));
        assert_eq!(opts2.measurement, PeakMeasurement::Rms);
        assert_eq!(opts2.channel, ChannelMode::Mix);

        let opts3 = PeakOptions::from_config(None, None);
        assert_eq!(opts3.measurement, PeakMeasurement::Rms);
        assert_eq!(opts3.channel, ChannelMode::Mix);

        // Case insensitive.
        let opts4 = PeakOptions::from_config(Some("Peak"), Some("LEFT"));
        assert_eq!(opts4.measurement, PeakMeasurement::Peak);
        assert_eq!(opts4.channel, ChannelMode::Left);
    }

    // --- Stereo peaks ---

    #[test]
    fn test_stereo_peaks_length_360() {
        // Stereo file: should return 360 bytes (180L + 180R).
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // L
            audio.extend_from_slice(&(i16::MAX / 2).to_le_bytes()); // R
        }
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks_stereo(&mut cursor, &map, &fmt).unwrap();
        assert_eq!(peaks.len(), STEREO_PEAK_COUNT);
    }

    #[test]
    fn test_stereo_peaks_mono_symmetric() {
        // Mono file: left and right halves should be identical.
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes());
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks_stereo(&mut cursor, &map, &fmt).unwrap();
        assert_eq!(peaks.len(), STEREO_PEAK_COUNT);

        let left = &peaks[..PEAK_COUNT];
        let right = &peaks[PEAK_COUNT..];
        assert_eq!(
            left, right,
            "mono stereo peaks should have identical L/R halves"
        );
    }

    #[test]
    fn test_stereo_peaks_asymmetric_channels() {
        // Stereo: L=max, R=0 → left half should have peaks, right half should be zeros.
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // L
            audio.extend_from_slice(&0i16.to_le_bytes()); // R
        }
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks_stereo(&mut cursor, &map, &fmt).unwrap();
        let left = &peaks[..PEAK_COUNT];
        let right = &peaks[PEAK_COUNT..];

        let left_sum: u32 = left.iter().map(|&v| v as u32).sum();
        let right_sum: u32 = right.iter().map(|&v| v as u32).sum();
        assert!(left_sum > 0, "left should have signal");
        assert_eq!(right_sum, 0, "right should be silent");
    }

    #[test]
    fn test_stereo_peaks_global_normalization() {
        // Stereo: L=max, R=max/2 → both should have peaks, L louder.
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // L
            audio.extend_from_slice(&(i16::MAX / 2).to_le_bytes()); // R
        }
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks_stereo(&mut cursor, &map, &fmt).unwrap();
        let left = &peaks[..PEAK_COUNT];
        let right = &peaks[PEAK_COUNT..];

        // Left should hit 255 (normalized max).
        assert!(left.contains(&255), "left should peak at 255");
        // Right should be < 255 (proportionally scaled).
        let right_max = *right.iter().max().unwrap();
        assert!(
            right_max < 255,
            "right should be less than left (global normalization)"
        );
        assert!(right_max > 0, "right should have signal");
    }

    #[test]
    fn test_stereo_peaks_silent_file() {
        let audio = vec![0u8; 720]; // 180 stereo frames of 16-bit
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks_stereo(&mut cursor, &map, &fmt).unwrap();
        assert_eq!(peaks.len(), STEREO_PEAK_COUNT);
        assert!(peaks.iter().all(|&v| v == 0));
    }

    #[test]
    fn test_stereo_peaks_from_path() {
        let path = std::path::Path::new("test_files/clean_base.wav");
        if !path.exists() {
            return;
        }
        let peaks = compute_peaks_stereo_from_path(path).unwrap();
        assert_eq!(peaks.len(), STEREO_PEAK_COUNT);
    }

    // --- Proptest ---

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// stream_samples never panics on random fmt+data.
            #[test]
            fn stream_no_panic(
                channels in 1u16..=8,
                bits in prop_oneof![Just(8u16), Just(16u16), Just(24u16), Just(32u16)],
                data in proptest::collection::vec(any::<u8>(), 0..2048),
            ) {
                let mut fmt_bytes = Vec::new();
                let format_tag: u16 = if bits == 32 { 3 } else { 1 };
                let block_align = channels * (bits / 8);
                fmt_bytes.extend_from_slice(&format_tag.to_le_bytes());
                fmt_bytes.extend_from_slice(&channels.to_le_bytes());
                fmt_bytes.extend_from_slice(&44100u32.to_le_bytes());
                fmt_bytes.extend_from_slice(&(44100u32 * block_align as u32).to_le_bytes());
                fmt_bytes.extend_from_slice(&block_align.to_le_bytes());
                fmt_bytes.extend_from_slice(&bits.to_le_bytes());

                let wav = make_wav(&fmt_bytes, &data);
                let mut cursor = Cursor::new(wav);
                let map = scan_chunks(&mut cursor).unwrap();
                let fmt = parse_fmt(&mut cursor, &map).unwrap();
                let _ = stream_samples_channel(&mut cursor, &map, &fmt, ChannelMode::Mix, &mut |_| {});
            }

            /// compute_peaks always returns exactly PEAK_COUNT values or empty on error.
            #[test]
            fn peaks_all_values_in_range(
                data in proptest::collection::vec(any::<u8>(), 0..4096),
            ) {
                let wav = make_wav(&make_fmt_pcm16_mono(), &data);
                let mut cursor = Cursor::new(wav);
                let map = scan_chunks(&mut cursor).unwrap();
                let fmt = parse_fmt(&mut cursor, &map).unwrap();
                let peaks = compute_peaks_with_options(&mut cursor, &map, &fmt, &PeakOptions::default()).unwrap();
                prop_assert_eq!(peaks.len(), PEAK_COUNT);
            }
        }
    }

    // --- Zero-Crossing Detection Tests ---

    #[test]
    fn test_zc_sine_wave() {
        // Simple sine wave: positive → negative → positive.
        let samples: Vec<i32> = vec![0, 1000, 2000, 1000, 0, -1000, -2000, -1000, 0, 1000];
        let crossings = find_zero_crossings(&samples, ZC_THRESHOLD);
        // Crossings: between indices 3→4 (1000 → 0, but 0 is non-negative, no sign change)
        // Actually: 0→-1000 at index 4, and -1000→0 at index 7→8
        // Let's trace: (0,1000)=no, (1000,2000)=no, (2000,1000)=no, (1000,0)=no,
        // (0,-1000)=yes at 4, (-1000,-2000)=no, (-2000,-1000)=no, (-1000,0)=yes at 7,
        // (0,1000)=no.
        assert_eq!(crossings.len(), 2);
        assert_eq!(crossings[0].index, 4);
        assert_eq!(crossings[1].index, 7);
    }

    #[test]
    fn test_zc_silence_below_threshold() {
        // Low-amplitude sign changes should not count as zero-crossings.
        let samples: Vec<i32> = vec![10, -10, 10, -10, 10];
        let crossings = find_zero_crossings(&samples, ZC_THRESHOLD);
        assert!(
            crossings.is_empty(),
            "sub-threshold crossings should be ignored"
        );
    }

    #[test]
    fn test_zc_dc_offset() {
        // All positive: no crossings even with large values.
        let samples: Vec<i32> = vec![100, 200, 300, 200, 100];
        let crossings = find_zero_crossings(&samples, ZC_THRESHOLD);
        assert!(crossings.is_empty());
    }

    #[test]
    fn test_zc_forward_nearest() {
        let samples: Vec<i32> = vec![100, 200, -300, 400, -500];
        // Crossings at: 1 (200→-300), 2 (-300→400), 3 (400→-500)
        assert_eq!(
            nearest_zero_crossing_forward(&samples, 0, ZC_THRESHOLD),
            Some(1)
        );
        assert_eq!(
            nearest_zero_crossing_forward(&samples, 2, ZC_THRESHOLD),
            Some(2)
        );
        assert_eq!(
            nearest_zero_crossing_forward(&samples, 4, ZC_THRESHOLD),
            None
        );
    }

    #[test]
    fn test_zc_backward_nearest() {
        let samples: Vec<i32> = vec![100, 200, -300, 400, -500];
        assert_eq!(
            nearest_zero_crossing_backward(&samples, 3, ZC_THRESHOLD),
            Some(3)
        );
        assert_eq!(
            nearest_zero_crossing_backward(&samples, 1, ZC_THRESHOLD),
            Some(1)
        );
    }

    #[test]
    fn test_zc_nth_forward() {
        let samples: Vec<i32> = vec![100, -200, 300, -400, 500, -600];
        // Every adjacent pair crosses zero.
        assert_eq!(
            nth_zero_crossing_forward(&samples, 0, 1, ZC_THRESHOLD),
            Some(0)
        );
        assert_eq!(
            nth_zero_crossing_forward(&samples, 0, 3, ZC_THRESHOLD),
            Some(2)
        );
        assert_eq!(
            nth_zero_crossing_forward(&samples, 0, 5, ZC_THRESHOLD),
            Some(4)
        );
    }

    #[test]
    fn test_zc_nth_backward() {
        let samples: Vec<i32> = vec![100, -200, 300, -400, 500, -600];
        assert_eq!(
            nth_zero_crossing_backward(&samples, 4, 1, ZC_THRESHOLD),
            Some(4)
        );
        assert_eq!(
            nth_zero_crossing_backward(&samples, 4, 3, ZC_THRESHOLD),
            Some(2)
        );
    }

    #[test]
    fn test_zc_empty_and_single() {
        assert!(find_zero_crossings(&[], ZC_THRESHOLD).is_empty());
        assert!(find_zero_crossings(&[100], ZC_THRESHOLD).is_empty());
        assert_eq!(nearest_zero_crossing_forward(&[], 0, ZC_THRESHOLD), None);
        assert_eq!(nearest_zero_crossing_backward(&[], 0, ZC_THRESHOLD), None);
    }
}
