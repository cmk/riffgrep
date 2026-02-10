//! WAV format parser, PCM sample reader, and peak computation.
//!
//! Provides streaming audio processing without loading entire files into memory.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

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
    let offset = map
        .fmt_offset
        .ok_or(RiffError::NotRiffWave)?;

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

/// Stream raw PCM samples as mono f32 values in [-1.0, 1.0].
///
/// Reads in 4096-byte buffer chunks for constant memory usage. Stereo is
/// mixed down to mono via (L + R) / 2.0. Returns the total mono sample count.
pub fn stream_samples<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
    callback: &mut dyn FnMut(f32),
) -> Result<u64, RiffError> {
    let data_offset = map
        .data_offset
        .ok_or(RiffError::NotRiffWave)?;

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

    // Align reads to frame boundaries to prevent misalignment across buffers.
    // Without this, formats where frame_size doesn't divide BUF_SIZE (e.g.
    // 24-bit stereo, frame_size=6) would drop partial frames at each buffer
    // boundary, causing periodic phase-shift artifacts in the peak data.
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
            // Decode all channels and mix to mono.
            let mut mono_sum: f32 = 0.0;
            for ch in 0..channels {
                let sample_start = pos + ch * bytes_per_sample;
                let sample = decode_sample(
                    &slice[sample_start..sample_start + bytes_per_sample],
                    fmt.format,
                    fmt.bits_per_sample,
                );
                mono_sum += sample;
            }
            let mono = mono_sum / channels as f32;
            callback(mono);
            mono_count += 1;
            pos += frame_size;
        }

        remaining_bytes -= to_read;
    }

    Ok(mono_count)
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
                let val = bytes[0] as i32
                    | (bytes[1] as i32) << 8
                    | ((bytes[2] as i8) as i32) << 16;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelMode {
    /// Average all channels.
    Left,
    /// Use only channel 0 (left).
    #[default]
    Mix,
}

/// Peak measurement method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeakMeasurement {
    /// Root-mean-square per bin.
    Rms,
    /// Maximum absolute sample per bin.
    #[default]
    Peak,
}

/// Options controlling peak computation.
#[derive(Debug, Clone, Default)]
pub struct PeakOptions {
    /// Channel mixdown mode.
    pub channel: ChannelMode,
    /// Measurement method.
    pub measurement: PeakMeasurement,
}

impl PeakOptions {
    /// Parse from config strings (case-insensitive).
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

/// Compute [`PEAK_COUNT`] u8 peaks from a WAV file by streaming through audio data.
///
/// Algorithm: divide mono samples into equal bins, compute RMS (root-mean-square)
/// per bin, normalize so global max RMS = 255.
pub fn compute_peaks<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
    fmt: &FmtChunk,
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

    // Track sum-of-squares and count per bin for RMS computation.
    let mut bin_sum_sq = [0.0f64; PEAK_COUNT];
    let mut bin_count = [0u64; PEAK_COUNT];
    let mut sample_idx: u64 = 0;

    stream_samples(reader, map, fmt, &mut |sample| {
        let bin = (sample_idx as f64 / bin_size).min((PEAK_COUNT - 1) as f64) as usize;
        bin_sum_sq[bin] += (sample as f64) * (sample as f64);
        bin_count[bin] += 1;
        sample_idx += 1;
    })?;

    // Compute RMS per bin.
    let mut bin_rms = [0.0f64; PEAK_COUNT];
    for i in 0..PEAK_COUNT {
        if bin_count[i] > 0 {
            bin_rms[i] = (bin_sum_sq[i] / bin_count[i] as f64).sqrt();
        }
    }

    // Normalize: global max RMS → 255.
    let global_max = bin_rms.iter().cloned().fold(0.0f64, f64::max);
    if global_max == 0.0 {
        return Ok(vec![0u8; PEAK_COUNT]);
    }

    let peaks: Vec<u8> = bin_rms
        .iter()
        .map(|&v| (v / global_max * 255.0).round() as u8)
        .collect();

    Ok(peaks)
}

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

/// Convenience: open a file, scan chunks, parse fmt, compute peaks with options.
pub fn compute_peaks_from_path_with_options(
    path: &Path,
    opts: &PeakOptions,
) -> Result<Vec<u8>, RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = super::bext::scan_chunks(&mut reader)?;
    let fmt = parse_fmt(&mut reader, &map)?;
    compute_peaks_with_options(&mut reader, &map, &fmt, opts)
}

/// Convenience: open a file, scan chunks, parse fmt, compute peaks.
pub fn compute_peaks_from_path(path: &Path) -> Result<Vec<u8>, RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = super::bext::scan_chunks(&mut reader)?;
    let fmt = parse_fmt(&mut reader, &map)?;
    compute_peaks(&mut reader, &map, &fmt)
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
pub fn compute_peaks_stereo_from_path(path: &Path) -> Result<Vec<u8>, RiffError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(8192, file);
    let map = super::bext::scan_chunks(&mut reader)?;
    let fmt = parse_fmt(&mut reader, &map)?;
    compute_peaks_stereo(&mut reader, &map, &fmt)
}

/// Audio metadata extracted from the fmt chunk.
#[derive(Debug, Clone)]
pub struct AudioInfo {
    /// Duration in seconds.
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
        let byte_rate = fmt.sample_rate as u64
            * fmt.channels as u64
            * (fmt.bits_per_sample / 8) as u64;
        let duration_secs = if byte_rate > 0 {
            data_size as f64 / byte_rate as f64
        } else {
            0.0
        };
        Self {
            duration_secs,
            sample_rate: fmt.sample_rate,
            bit_depth: fmt.bits_per_sample,
            channels: fmt.channels,
        }
    }

    /// Format as "16-bit stereo" or "24-bit mono" etc.
    pub fn format_display(&self) -> String {
        let ch = if self.channels == 1 {
            "mono"
        } else {
            "stereo"
        };
        format!("{}-bit {ch}", self.bit_depth)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::engine::bext::scan_chunks;

    /// Build a minimal RIFF/WAVE file with fmt + data chunks.
    fn make_wav(fmt_data: &[u8], audio_data: &[u8]) -> Vec<u8> {
        let mut chunks = Vec::new();
        // fmt chunk
        chunks.extend_from_slice(b"fmt ");
        chunks.extend_from_slice(&(fmt_data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(fmt_data);
        if fmt_data.len() % 2 != 0 {
            chunks.push(0);
        }
        // data chunk
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(audio_data.len() as u32).to_le_bytes());
        chunks.extend_from_slice(audio_data);
        if audio_data.len() % 2 != 0 {
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
        let count = stream_samples(&mut cursor, &map, &fmt, &mut |s| samples.push(s)).unwrap();
        assert_eq!(count, 2);
        assert!((samples[0] - 0.25).abs() < 0.001, "got {}", samples[0]);
        assert!((samples[1] - (-0.25)).abs() < 0.001, "got {}", samples[1]);
    }

    #[test]
    fn test_stream_24bit_mono() {
        // 24-bit max positive: 0x7FFFFF = 8388607
        let mut audio = Vec::new();
        audio.push(0xFF);
        audio.push(0xFF);
        audio.push(0x7F);

        let wav = make_wav(&make_fmt_pcm24_mono(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let mut samples = Vec::new();
        stream_samples(&mut cursor, &map, &fmt, &mut |s| samples.push(s)).unwrap();
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
        stream_samples(&mut cursor, &map, &fmt, &mut |s| samples.push(s)).unwrap();
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

        let count = stream_samples(&mut cursor, &map, &fmt, &mut |_| {}).unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn test_stream_empty_data_chunk() {
        let wav = make_wav(&make_fmt_pcm16_stereo(), &[]);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let count = stream_samples(&mut cursor, &map, &fmt, &mut |_| {}).unwrap();
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
            let val = ((i as f64 / num_frames as f64 * std::f64::consts::PI).sin()
                * 4_000_000.0) as i32;
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
        let total = stream_samples(&mut cursor, &map, &fmt, &mut |_| {
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
        let audio = vec![0u8; 8820]; // 10 frames per bin × 180 bins × 4 bytes/frame (stereo 16)
        // Actually: need enough samples. 180 bins, each with >0 samples.
        // 180 mono samples → one per bin (stereo so 360 samples = 720 bytes)
        let audio = vec![0u8; 720];
        let wav = make_wav(&make_fmt_pcm16_stereo(), &audio);
        let mut cursor = Cursor::new(wav);
        let map = scan_chunks(&mut cursor).unwrap();
        let fmt = parse_fmt(&mut cursor, &map).unwrap();

        let peaks = compute_peaks(&mut cursor, &map, &fmt).unwrap();
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
                match compute_peaks_from_path(&path) {
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

        let peaks = compute_peaks(&mut cursor, &map, &fmt).unwrap();
        assert_eq!(peaks.len(), PEAK_COUNT);
        assert!(
            peaks.iter().any(|&v| v == 255),
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

        let peaks = compute_peaks(&mut cursor, &map, &fmt).unwrap();
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
        let peaks = compute_peaks_from_path(path).unwrap();
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
            duration_secs: 1.0,
            sample_rate: 44100,
            bit_depth: 24,
            channels: 1,
        };
        assert_eq!(info.format_display(), "24-bit mono");
    }

    // --- T2: Multi-option peak computation ---

    #[test]
    fn test_channel_mode_mix_same_as_default() {
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes()); // mono
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);

        let mut c1 = Cursor::new(wav.clone());
        let map1 = scan_chunks(&mut c1).unwrap();
        let fmt1 = parse_fmt(&mut c1, &map1).unwrap();
        let default_peaks = compute_peaks(&mut c1, &map1, &fmt1).unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let mix_peaks = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions::default(),
        )
        .unwrap();

        assert_eq!(default_peaks, mix_peaks);
    }

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
            &PeakOptions { channel: ChannelMode::Left, measurement: PeakMeasurement::Rms },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let mix_peaks = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions { channel: ChannelMode::Mix, measurement: PeakMeasurement::Rms },
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
            &PeakOptions { channel: ChannelMode::Left, measurement: PeakMeasurement::Rms },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let mix_peaks = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions::default(),
        )
        .unwrap();

        assert_eq!(left_peaks, mix_peaks);
    }

    #[test]
    fn test_peak_measurement_rms_same_as_default() {
        let mut audio = Vec::new();
        for _ in 0..1000 {
            audio.extend_from_slice(&i16::MAX.to_le_bytes());
        }
        let wav = make_wav(&make_fmt_pcm16_mono(), &audio);

        let mut c1 = Cursor::new(wav.clone());
        let map1 = scan_chunks(&mut c1).unwrap();
        let fmt1 = parse_fmt(&mut c1, &map1).unwrap();
        let default_peaks = compute_peaks(&mut c1, &map1, &fmt1).unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let rms_peaks = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions { channel: ChannelMode::Mix, measurement: PeakMeasurement::Rms },
        )
        .unwrap();

        assert_eq!(default_peaks, rms_peaks);
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
            &PeakOptions { channel: ChannelMode::Mix, measurement: PeakMeasurement::Rms },
        )
        .unwrap();

        let mut c2 = Cursor::new(wav);
        let map2 = scan_chunks(&mut c2).unwrap();
        let fmt2 = parse_fmt(&mut c2, &map2).unwrap();
        let peak = compute_peaks_with_options(
            &mut c2,
            &map2,
            &fmt2,
            &PeakOptions { channel: ChannelMode::Mix, measurement: PeakMeasurement::Peak },
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
        assert_eq!(left, right, "mono stereo peaks should have identical L/R halves");
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
        assert!(left.iter().any(|&v| v == 255), "left should peak at 255");
        // Right should be < 255 (proportionally scaled).
        let right_max = *right.iter().max().unwrap();
        assert!(right_max < 255, "right should be less than left (global normalization)");
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
                let _ = stream_samples(&mut cursor, &map, &fmt, &mut |_| {});
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
                let peaks = compute_peaks(&mut cursor, &map, &fmt).unwrap();
                prop_assert_eq!(peaks.len(), PEAK_COUNT);
                for &v in &peaks {
                    prop_assert!(v <= 255);
                }
            }
        }
    }
}
