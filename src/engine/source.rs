//! Format-agnostic audio source abstraction.
//!
//! [`AudioSource`] defines the operations riffgrep needs from any audio format.
//! [`AudioRegistry`] maps file extensions to the appropriate source implementation.
//!
//! Two implementations:
//! - [`RiffSource`]: fast BEXT/chunk path for WAV files, supports metadata writes.
//! - [`DecoderSource`]: rodio/symphonia fallback for AIFF, MP3, FLAC, etc. Read-only.

use std::collections::HashMap;
use std::path::Path;

use super::bext::RiffError;
use super::wav::{AudioInfo, PcmData};
use super::UnifiedMetadata;

/// Operations riffgrep needs from any audio format.
///
/// Implementations must be stateless and thread-safe. All methods take
/// a file path and return results — no open file handles are held.
pub trait AudioSource: Send + Sync {
    /// File extensions this source handles (lowercase, no dot).
    fn extensions(&self) -> &[&str];

    /// Read metadata from file headers.
    /// Fast path (<4KB read) where possible; full decode as fallback.
    fn read_metadata(&self, path: &Path) -> Result<UnifiedMetadata, RiffError>;

    /// Compute stereo RMS peaks: 180 left + 180 right = 360 u8 values.
    fn compute_peaks_stereo(&self, path: &Path) -> Result<Vec<u8>, RiffError>;

    /// Load full mono PCM (left channel, i16) for zoom rendering.
    fn load_pcm(&self, path: &Path) -> Result<PcmData, RiffError>;

    /// Audio format info (duration, sample rate, bit depth, channels).
    fn audio_info(&self, path: &Path) -> Result<AudioInfo, RiffError>;

    /// Write metadata changes back to the file.
    /// Returns `None` if the format is read-only.
    fn write_metadata(
        &self,
        _path: &Path,
        _before: &UnifiedMetadata,
        _after: &UnifiedMetadata,
        _force: bool,
    ) -> Option<anyhow::Result<()>> {
        None
    }
}

// ---------------------------------------------------------------------------
// RiffSource — fast BEXT/chunk path for WAV
// ---------------------------------------------------------------------------

/// WAV/RIFF source with surgical BEXT reads and writes.
///
/// Uses `scan_chunks` to locate BEXT, fmt, data chunks in the first 4KB.
/// Metadata reads touch only ~1KB per file. Writes are fixed-offset
/// overwrites — no re-encoding of audio data.
pub struct RiffSource;

impl AudioSource for RiffSource {
    fn extensions(&self) -> &[&str] {
        &["wav"]
    }

    fn read_metadata(&self, path: &Path) -> Result<UnifiedMetadata, RiffError> {
        // Delegate to the RIFF-specific BEXT+INFO reader.
        super::read_metadata_riff(path)
    }

    fn compute_peaks_stereo(&self, path: &Path) -> Result<Vec<u8>, RiffError> {
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::with_capacity(8192, file);
        let map = super::bext::scan_chunks(&mut reader)?;
        let fmt = super::wav::parse_fmt(&mut reader, &map)?;
        super::wav::compute_peaks_stereo(&mut reader, &map, &fmt)
    }

    fn load_pcm(&self, path: &Path) -> Result<PcmData, RiffError> {
        // Call the RIFF-specific loader (reads data chunk directly).
        super::wav::load_pcm_data_riff(path)
    }

    fn audio_info(&self, path: &Path) -> Result<AudioInfo, RiffError> {
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::with_capacity(8192, file);
        let map = super::bext::scan_chunks(&mut reader)?;
        let fmt = super::wav::parse_fmt(&mut reader, &map)?;
        Ok(AudioInfo::from_fmt(&fmt, map.data_size))
    }

    fn write_metadata(
        &self,
        path: &Path,
        before: &UnifiedMetadata,
        after: &UnifiedMetadata,
        force: bool,
    ) -> Option<anyhow::Result<()>> {
        Some(super::workflow::write_metadata_changes(path, before, after, force))
    }
}

// ---------------------------------------------------------------------------
// DecoderSource — rodio/symphonia fallback for non-RIFF formats
// ---------------------------------------------------------------------------

/// Format-agnostic source using rodio's decoder (symphonia backend).
///
/// Works for any format symphonia supports. Slower than RiffSource because
/// it decodes the entire file for peaks/zoom/info. Read-only — no metadata
/// writes.
pub struct DecoderSource;

impl AudioSource for DecoderSource {
    fn extensions(&self) -> &[&str] {
        &["aif", "aiff", "mp3", "flac", "ogg"]
    }

    fn read_metadata(&self, path: &Path) -> Result<UnifiedMetadata, RiffError> {
        // Non-RIFF files have no BEXT/INFO metadata in the header.
        // Return minimal metadata with path only; enrichment comes from DB.
        Ok(UnifiedMetadata {
            path: path.to_path_buf(),
            ..Default::default()
        })
    }

    fn compute_peaks_stereo(&self, path: &Path) -> Result<Vec<u8>, RiffError> {
        super::wav::compute_peaks_stereo_via_decoder(path)
    }

    fn load_pcm(&self, path: &Path) -> Result<PcmData, RiffError> {
        super::wav::load_pcm_data_via_decoder(path)
    }

    fn audio_info(&self, path: &Path) -> Result<AudioInfo, RiffError> {
        use rodio::Source;
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let decoder = rodio::Decoder::new(reader)
            .map_err(|_| RiffError::NotRiffWave)?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let total_i16: usize = decoder.count();
        let total_frames = total_i16 / channels as usize;
        let duration_secs = total_frames as f64 / sample_rate as f64;
        Ok(AudioInfo {
            total_samples: total_frames as u32,
            duration_secs,
            sample_rate,
            bit_depth: 16,
            channels,
        })
    }
}

// ---------------------------------------------------------------------------
// AudioRegistry
// ---------------------------------------------------------------------------

/// Maps file extensions to audio source implementations.
///
/// Constructed once at startup. Thread-safe (all sources are `Send + Sync`).
pub struct AudioRegistry {
    sources: Vec<Box<dyn AudioSource>>,
    /// Lowercase extension → index into `sources`.
    ext_map: HashMap<String, usize>,
}

impl AudioRegistry {
    /// Create a registry with the default sources (RiffSource + DecoderSource).
    pub fn new() -> Self {
        let sources: Vec<Box<dyn AudioSource>> = vec![
            Box::new(RiffSource),
            Box::new(DecoderSource),
        ];
        let mut ext_map = HashMap::new();
        for (i, source) in sources.iter().enumerate() {
            for ext in source.extensions() {
                ext_map.insert(ext.to_lowercase(), i);
            }
        }
        Self { sources, ext_map }
    }

    /// Look up the source for a file path based on its extension.
    ///
    /// Extension matching is case-insensitive.
    pub fn for_path(&self, path: &Path) -> Option<&dyn AudioSource> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        let idx = self.ext_map.get(&ext)?;
        Some(self.sources[*idx].as_ref())
    }

    /// All registered extensions (lowercase), for building file walker globs.
    pub fn all_extensions(&self) -> Vec<&str> {
        self.sources
            .iter()
            .flat_map(|s| s.extensions().iter().copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_wav() {
        let reg = AudioRegistry::new();
        assert!(reg.for_path(Path::new("test.wav")).is_some());
        assert!(reg.for_path(Path::new("test.WAV")).is_some());
    }

    #[test]
    fn registry_resolves_aiff() {
        let reg = AudioRegistry::new();
        assert!(reg.for_path(Path::new("test.aif")).is_some());
        assert!(reg.for_path(Path::new("test.AIFF")).is_some());
    }

    #[test]
    fn registry_returns_none_for_unknown() {
        let reg = AudioRegistry::new();
        assert!(reg.for_path(Path::new("test.txt")).is_none());
        assert!(reg.for_path(Path::new("test.mid")).is_none());
    }

    #[test]
    fn registry_resolves_mp3_flac_ogg() {
        let reg = AudioRegistry::new();
        assert!(reg.for_path(Path::new("test.mp3")).is_some());
        assert!(reg.for_path(Path::new("test.flac")).is_some());
        assert!(reg.for_path(Path::new("test.ogg")).is_some());
        assert!(reg.for_path(Path::new("test.MP3")).is_some());
    }

    #[test]
    fn all_extensions_contains_expected() {
        let reg = AudioRegistry::new();
        let all = reg.all_extensions();
        assert!(all.contains(&"wav"));
        assert!(all.contains(&"aif"));
        assert!(all.contains(&"aiff"));
    }

    #[test]
    fn decoder_source_is_readonly() {
        let source = DecoderSource;
        let meta = UnifiedMetadata::default();
        assert!(source.write_metadata(Path::new("x.aiff"), &meta, &meta, false).is_none());
    }

    #[test]
    fn riff_source_supports_writes() {
        let source = RiffSource;
        let meta = UnifiedMetadata::default();
        // write_metadata returns Some (even if the write itself fails on a nonexistent file).
        assert!(source.write_metadata(Path::new("/nonexistent.wav"), &meta, &meta, false).is_some());
    }

    // --- Integration tests with real files ---

    fn test_files_exist() -> bool {
        Path::new("test_files").exists()
    }

    #[test]
    fn riff_source_read_metadata() {
        if !test_files_exist() { return; }
        let source = RiffSource;
        let meta = source.read_metadata(Path::new("test_files/clean_base.wav")).unwrap();
        assert_eq!(meta.path, Path::new("test_files/clean_base.wav"));
        assert!(meta.description.contains("Yamaha"));
    }

    #[test]
    fn riff_source_peaks_length() {
        if !test_files_exist() { return; }
        let source = RiffSource;
        let peaks = source.compute_peaks_stereo(Path::new("test_files/clean_base.wav")).unwrap();
        assert_eq!(peaks.len(), 360);
    }

    #[test]
    fn riff_source_audio_info() {
        if !test_files_exist() { return; }
        let source = RiffSource;
        let info = source.audio_info(Path::new("test_files/clean_base.wav")).unwrap();
        assert!(info.sample_rate > 0);
        assert!(info.total_samples > 0);
    }

    #[test]
    fn riff_source_load_pcm() {
        if !test_files_exist() { return; }
        let source = RiffSource;
        let pcm = source.load_pcm(Path::new("test_files/clean_base.wav")).unwrap();
        assert!(!pcm.samples.is_empty());
    }

    #[test]
    fn decoder_source_metadata_has_path() {
        let source = DecoderSource;
        let meta = source.read_metadata(Path::new("test_files/clean_base.wav")).unwrap();
        assert_eq!(meta.path, Path::new("test_files/clean_base.wav"));
        // DecoderSource returns minimal metadata — no description.
        assert!(meta.description.is_empty());
    }

    #[test]
    fn decoder_source_peaks_on_wav() {
        if !test_files_exist() { return; }
        let source = DecoderSource;
        let peaks = source.compute_peaks_stereo(Path::new("test_files/clean_base.wav")).unwrap();
        assert_eq!(peaks.len(), 360);
    }
}
