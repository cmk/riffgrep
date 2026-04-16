//! Surgical BEXT chunk parser. Scans the first 4KB to locate chunks,
//! then parses the 602-byte BEXT data block into [`super::UnifiedMetadata`].

use std::io::{self, Read, Seek, SeekFrom};

use thiserror::Error;

/// Maximum bytes to scan for chunk headers (covers typical header region).
const SCAN_LIMIT: u64 = 4096;

/// Standard BEXT data size through Reserved field (Description + standard + reserved).
/// 256 + 32 + 32 + 10 + 8 + 8 + 2 + 64 + 10 + 180 = 602
const BEXT_STANDARD_SIZE: usize = 602;

/// Errors from RIFF chunk scanning and BEXT parsing.
#[derive(Debug, Error)]
pub enum RiffError {
    /// Not a valid RIFF/WAVE file.
    #[error("not a RIFF/WAVE file")]
    NotRiffWave,
    /// File is too short to contain a valid RIFF header.
    #[error("file too short ({0} bytes)")]
    TooShort(u64),
    /// I/O error during reading.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// BEXT chunk is smaller than expected.
    #[error("BEXT chunk too small ({actual} bytes, need {expected})")]
    BextTooSmall {
        /// Actual chunk size.
        actual: u32,
        /// Minimum expected size.
        expected: usize,
    },
}

/// Chunk offsets discovered by scanning the RIFF header region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMap {
    /// File offset of the BEXT chunk data (after the 8-byte chunk header).
    pub bext_offset: Option<u64>,
    /// BEXT chunk data size.
    pub bext_size: u32,
    /// File offset of the LIST-INFO chunk data (after "LIST" + size, pointing at "INFO").
    pub info_offset: Option<u64>,
    /// LIST-INFO chunk data size (includes the "INFO" fourcc).
    pub info_size: u32,
    /// File offset of the `fmt ` chunk data (after the 8-byte chunk header).
    pub fmt_offset: Option<u64>,
    /// `fmt ` chunk data size.
    pub fmt_size: u32,
    /// File offset of the `data` chunk data (after the 8-byte chunk header).
    pub data_offset: Option<u64>,
    /// `data` chunk data size.
    pub data_size: u32,
}

/// Scan a RIFF/WAVE file to locate `bext`, `LIST`-`INFO`, `fmt `, and `data` chunks.
///
/// Returns a [`ChunkMap`] with the file offsets and sizes of discovered chunks.
/// For metadata chunks (bext, LIST-INFO), scanning stops at `SCAN_LIMIT` (4KB).
/// For audio chunks (fmt, data), scanning continues until all chunks are found
/// or the file ends, since these are needed for audio reading.
pub fn scan_chunks<R: Read + Seek>(reader: &mut R) -> Result<ChunkMap, RiffError> {
    // Read and validate the 12-byte RIFF header.
    let mut header = [0u8; 12];
    let n = reader.read(&mut header)?;
    if n < 12 {
        return Err(RiffError::TooShort(n as u64));
    }
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(RiffError::NotRiffWave);
    }

    let riff_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let file_end = 8 + riff_size as u64;

    let mut map = ChunkMap::default();
    let mut pos: u64 = 12; // Current position after RIFF header.
    let mut metadata_done = false;

    loop {
        // Past SCAN_LIMIT: only keep scanning if we still need fmt or data.
        if pos >= SCAN_LIMIT && !metadata_done {
            metadata_done = true;
        }
        if metadata_done && map.fmt_offset.is_some() && map.data_offset.is_some() {
            break;
        }
        if pos >= file_end {
            break;
        }

        // Ensure we're at the right position (handles cases where seeks may drift).
        reader.seek(SeekFrom::Start(pos))?;

        // Read chunk header: 4-byte ID + 4-byte little-endian size.
        let mut chunk_header = [0u8; 8];
        match reader.read_exact(&mut chunk_header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }

        let chunk_id = &chunk_header[0..4];
        let chunk_size = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]);

        let data_offset = pos + 8;

        if chunk_id == b"bext" && !metadata_done {
            map.bext_offset = Some(data_offset);
            map.bext_size = chunk_size;
        } else if chunk_id == b"LIST" && !metadata_done {
            // Check if this LIST is INFO type by reading the 4-byte form type.
            let mut form_type = [0u8; 4];
            match reader.read_exact(&mut form_type) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(RiffError::Io(e)),
            }
            if &form_type == b"INFO" {
                map.info_offset = Some(data_offset);
                map.info_size = chunk_size;
            }
        } else if chunk_id == b"fmt " && map.fmt_offset.is_none() {
            map.fmt_offset = Some(data_offset);
            map.fmt_size = chunk_size;
        } else if chunk_id == b"data" && map.data_offset.is_none() {
            map.data_offset = Some(data_offset);
            map.data_size = chunk_size;
        }

        // Advance to next chunk. Chunk data is WORD-aligned (padded to even boundary).
        let padded_size = (chunk_size as u64 + 1) & !1;
        pos = data_offset + padded_size;
    }

    Ok(map)
}

/// Parse BEXT data into [`super::UnifiedMetadata`] fields.
///
/// Reads `bext_size` bytes from the given offset and extracts standard BEXT fields
/// plus packed Description fields when the schema version is detected.
pub fn parse_bext_data<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
) -> Result<BextFields, RiffError> {
    let offset = match map.bext_offset {
        Some(o) => o,
        None => return Ok(BextFields::default()),
    };

    let read_size = (map.bext_size as usize).min(BEXT_STANDARD_SIZE);
    if read_size < BEXT_STANDARD_SIZE {
        return Err(RiffError::BextTooSmall {
            actual: map.bext_size,
            expected: BEXT_STANDARD_SIZE,
        });
    }

    reader.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; BEXT_STANDARD_SIZE];
    reader.read_exact(&mut buf)?;

    Ok(parse_bext_buffer(&buf))
}

/// Detected format of the BEXT Reserved field (peak data source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PeaksFormat {
    /// Our packed schema (Description v1.1): Reserved holds 180 u8 amplitude peaks.
    RiffgrepU8,
    /// Standard BWF (BEXT Version >= 1): Reserved holds R7 spectral data.
    BwfReserved,
    /// Reserved field is all zeros — no peaks present.
    #[default]
    Empty,
}

impl PeaksFormat {
    /// Return the `peaks_source` string for database storage.
    pub fn source_str(&self) -> &'static str {
        match self {
            PeaksFormat::RiffgrepU8 => "riffgrep_u8",
            PeaksFormat::BwfReserved => "bwf_reserved",
            PeaksFormat::Empty => "none",
        }
    }
}

/// Raw fields extracted from a BEXT chunk, before merging with RIFF INFO.
#[derive(Debug, Clone, Default)]
pub struct BextFields {
    // --- Standard BEXT fields ---
    /// BEXT Description (bytes 0-255), plain text.
    pub description: String,
    /// BEXT Originator (bytes 256-287). Maps to vendor/TPE1.
    pub vendor: String,
    /// BEXT OriginatorReference (bytes 288-319). Maps to library/TPE2.
    pub library: String,
    /// BEXT UMID (bytes 348-411) as hex string.
    pub umid: String,
    /// BEXT Reserved (bytes 422-601) as peak data (180 u8 values).
    pub peaks: Vec<u8>,
    /// BWF Version field at offset 346-347.
    #[allow(dead_code)] // Verified in tests; not read in production paths.
    pub bext_version: u16,
    /// Detected format of the Reserved field.
    pub peaks_format: PeaksFormat,

    // --- Packed Description fields (when schema detected) ---
    /// Whether the packed schema was detected in the Description field.
    #[allow(dead_code)] // Verified in tests; not read in production paths.
    pub packed: bool,
    /// `[000:008]` riffgrep file identity — high 64 bits of UUID v7 generated at first pack (BE).
    /// Zero means not yet packed.
    pub file_id: u64,
    /// `[044:076]` COMR/Comment (32 ASCII).
    pub comment: String,
    /// `[076:080]` POPM/Rating (4 ASCII).
    pub rating: String,
    /// `[080:084]` TBPM/BPM (4 ASCII), parsed to integer.
    pub bpm: Option<u16>,
    /// `[084:088]` TCOM/Subcategory (4 ASCII).
    pub subcategory: String,
    /// `[088:092]` TCON/Category (4 ASCII).
    pub category: String,
    /// `[092:096]` TIT1/Genre ID (4 ASCII).
    pub genre_id: String,
    /// `[096:100]` TIT2/Sound ID (4 ASCII).
    pub sound_id: String,
    /// `[100:104]` TIT3/Usage ID (4 ASCII).
    pub usage_id: String,
    /// `[104:112]` TKEY/Key (8 ASCII).
    pub key: String,
    /// `[112:116]` Take number (4 ASCII).
    pub take: String,
    /// `[116:120]` Track number (4 ASCII).
    pub track: String,
    /// `[120:128]` Item number (8 ASCII).
    pub item: String,

    // --- Standard BEXT date field ---
    /// BEXT OriginationDate (bytes 320-329), 10-char ASCII e.g. "2024-01-15".
    pub date: String,

    // --- MARKERSv2 (when packed schema detected) ---
    /// Marker configuration from packed Description`[12:44]`. None for unpacked files.
    pub markers: Option<MarkerConfig>,
}

/// Parse a 602-byte BEXT buffer into [`BextFields`].
///
/// This is the core parsing function, separated from I/O for testability.
pub fn parse_bext_buffer(buf: &[u8; BEXT_STANDARD_SIZE]) -> BextFields {
    let vendor = decode_fixed_ascii(&buf[256..288]);
    let library = decode_fixed_ascii(&buf[288..320]);
    let date = decode_fixed_ascii(&buf[320..330]);

    // BEXT Version at offset 346-347 (u16 LE). Must be read before fields
    // that depend on it (UMID exists only in BWF version >= 1).
    let bext_version = u16::from_le_bytes([buf[346], buf[347]]);

    // UMID (bytes 348-411) only exists in BWF version >= 1. In version 0
    // files, these bytes were part of the original 190-byte Reserved block
    // and may contain arbitrary data from non-compliant tools.
    let umid = if bext_version >= 1 {
        decode_umid(&buf[348..412])
    } else {
        String::new()
    };

    // Extract 180-byte Reserved field (bytes 422-601) as peak data.
    let peaks_raw = &buf[422..602];
    let all_zeros = peaks_raw.iter().all(|&b| b == 0);
    let peaks = if all_zeros {
        Vec::new()
    } else {
        peaks_raw.to_vec()
    };

    // Detect packed schema: requires version_major == 1, version_minor >= 1,
    // bext_version >= 2, AND a non-zero file_id (high 64 bits of UUID v7).
    // All four conditions must hold — version_major=1 signals binary UUID content
    // in a traditionally ASCII field; bext_version=2 signals EBU Tech 3285
    // conformance; the non-zero file_id makes coincidental matches essentially
    // impossible (collision probability ~1/370K for 10M files).
    // Old files with version_major=0 or bext_version<2 are treated as unpacked —
    // they predate the UUID schema and will be re-initialized on next write.
    let version_major = u16::from_le_bytes([buf[8], buf[9]]);
    let version_minor = u16::from_le_bytes([buf[10], buf[11]]);
    let file_id = u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]);
    let is_packed = version_major == 1 && version_minor >= 1 && bext_version >= 2 && file_id != 0;

    // Determine peaks format.
    // RiffgrepU8 requires the full packed schema (version_major=1, bext_version>=2,
    // non-zero file_id). This prevents misinterpreting arbitrary BEXT Reserved bytes
    // from third-party DAWs (Reaper, SoundMiner, etc.) as waveform peak data.
    let peaks_format = if all_zeros {
        PeaksFormat::Empty
    } else if is_packed {
        PeaksFormat::RiffgrepU8
    } else {
        // Non-zero data but not our packed schema — treat as opaque BWF Reserved data.
        PeaksFormat::BwfReserved
    };

    if is_packed {
        // Packed Description format per PICKER_SCHEMA.md.
        let comment = decode_fixed_ascii(&buf[44..76]);
        let description = comment.clone();

        // Extract MARKERSv2 from Description[12:44].
        let marker_bytes: [u8; MARKER_BLOCK_SIZE] = buf[12..44].try_into().unwrap();
        let markers = MarkerConfig::from_bytes(&marker_bytes);

        BextFields {
            vendor,
            library,
            date,
            umid,
            peaks,
            bext_version,
            peaks_format,
            packed: true,
            file_id,
            comment,
            description,
            rating: decode_fixed_ascii(&buf[76..80]),
            bpm: parse_bpm_ascii(&buf[80..84]),
            subcategory: decode_fixed_ascii(&buf[84..88]),
            category: decode_fixed_ascii(&buf[88..92]),
            genre_id: decode_fixed_ascii(&buf[92..96]),
            sound_id: decode_fixed_ascii(&buf[96..100]),
            usage_id: decode_fixed_ascii(&buf[100..104]),
            key: decode_fixed_ascii(&buf[104..112]),
            take: decode_fixed_ascii(&buf[112..116]),
            track: decode_fixed_ascii(&buf[116..120]),
            item: decode_fixed_ascii(&buf[120..128]),
            markers: Some(markers),
        }
    } else {
        // Plain-text Description (entire 256 bytes).
        BextFields {
            vendor,
            library,
            date,
            umid,
            peaks,
            bext_version,
            peaks_format,
            description: decode_fixed_ascii(&buf[0..256]),
            ..Default::default()
        }
    }
}

/// Decode a fixed-width ASCII/UTF-8 byte slice, trimming null bytes and trailing whitespace.
fn decode_fixed_ascii(bytes: &[u8]) -> String {
    // Try UTF-8 first (superset of ASCII), lossy fallback.
    let s = String::from_utf8_lossy(bytes);
    s.trim_end_matches('\0').trim_end().to_string()
}

/// Decode the 64-byte UMID field as hex, returning empty string if all zeros.
fn decode_umid(bytes: &[u8]) -> String {
    if bytes.iter().all(|&b| b == 0) {
        return String::new();
    }
    // UMID is stored as ASCII hex in our schema (32 hex chars in 64 bytes).
    // First try reading it as ASCII text.
    let text = decode_fixed_ascii(bytes);
    if !text.is_empty() {
        return text;
    }
    // Fallback: encode raw bytes as hex.
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .trim_end_matches('0')
        .to_string()
}

/// Parse BPM from a fixed ASCII field (e.g., "164 " -> Some(164)).
fn parse_bpm_ascii(bytes: &[u8]) -> Option<u16> {
    let s = decode_fixed_ascii(bytes);
    s.parse::<u16>().ok()
}

// --- Surgical BEXT writer ---

/// Errors from BEXT write operations.
#[derive(Debug, Error)]
pub enum BextWriteError {
    /// File has no BEXT chunk.
    #[error("no BEXT chunk in file")]
    NoBextChunk,
    /// BEXT chunk is too small for the requested write.
    #[error("BEXT chunk too small ({actual} bytes, need offset {offset} + {len})")]
    BextTooSmall {
        /// Actual BEXT chunk size.
        actual: u32,
        /// Requested write offset within BEXT data.
        offset: usize,
        /// Requested write length.
        len: usize,
    },
    /// Not a packed-schema BEXT (write_markers requires packed format).
    #[error("BEXT Description is not packed schema (required for marker writes)")]
    NotPacked,
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// RIFF parsing error (e.g., not a valid RIFF/WAVE file).
    #[error("RIFF error: {0}")]
    Riff(#[from] RiffError),
}

/// Write raw bytes at `field_offset` within the BEXT data block.
///
/// Opens the file for write-only, seeks to `bext_data_offset + field_offset`,
/// and writes `data`. File size is unchanged. Validates that the BEXT chunk
/// exists and is large enough.
pub fn write_bext_field(
    path: &std::path::Path,
    map: &ChunkMap,
    field_offset: usize,
    data: &[u8],
) -> Result<(), BextWriteError> {
    let bext_offset = map.bext_offset.ok_or(BextWriteError::NoBextChunk)?;

    if field_offset + data.len() > map.bext_size as usize {
        return Err(BextWriteError::BextTooSmall {
            actual: map.bext_size,
            offset: field_offset,
            len: data.len(),
        });
    }

    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(bext_offset + field_offset as u64))?;
    io::Write::write_all(&mut file, data)?;

    Ok(())
}

/// Packed schema version minor for MARKERSv2.
pub const PACKED_VERSION_MINOR_V2: u16 = 2;

/// Write a `MarkerConfig` to packed Description`[12:44]` and bump version minor to 2.
///
/// Requires an existing BEXT chunk with the packed schema detected. Reads the
/// file to verify the packed schema marker, then writes 32 bytes of marker data
/// and updates the schema version minor.
pub fn write_markers(path: &std::path::Path, markers: &MarkerConfig) -> Result<(), BextWriteError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(4096, file);
    let map = scan_chunks(&mut reader)?;

    let bext_offset = map.bext_offset.ok_or(BextWriteError::NoBextChunk)?;

    // Verify packed schema: read bytes [8:12] from the BEXT Description.
    if map.bext_size < BEXT_STANDARD_SIZE as u32 {
        return Err(BextWriteError::BextTooSmall {
            actual: map.bext_size,
            offset: 12,
            len: MARKER_BLOCK_SIZE,
        });
    }
    reader.seek(SeekFrom::Start(bext_offset + 8))?;
    let mut version_buf = [0u8; 4];
    reader.read_exact(&mut version_buf)?;
    let version_major = u16::from_le_bytes([version_buf[0], version_buf[1]]);
    let version_minor = u16::from_le_bytes([version_buf[2], version_buf[3]]);

    // Also check BWF version at offset 346.
    reader.seek(SeekFrom::Start(bext_offset + 346))?;
    let mut bwf_buf = [0u8; 2];
    reader.read_exact(&mut bwf_buf)?;
    let bext_version = u16::from_le_bytes(bwf_buf);

    // Also read file_id (8 bytes BE) at Description[0:8] to confirm non-zero.
    reader.seek(SeekFrom::Start(bext_offset))?;
    let mut id_buf = [0u8; 8];
    reader.read_exact(&mut id_buf)?;
    let file_id = u64::from_be_bytes(id_buf);

    let is_packed = version_major == 1 && version_minor >= 1 && bext_version >= 2 && file_id != 0;
    if !is_packed {
        return Err(BextWriteError::NotPacked);
    }

    drop(reader);

    // Write marker bytes at Description[12:44] (32 bytes).
    write_bext_field(path, &map, 12, &markers.to_bytes())?;

    // Bump version minor to 2.
    write_bext_field(path, &map, 10, &PACKED_VERSION_MINOR_V2.to_le_bytes())?;

    Ok(())
}

/// Initialize packed schema on an existing unpacked BEXT chunk and write markers.
///
/// For files with BEXT that are NOT yet packed:
/// 1. Generates a UUID v7 and writes the high 8 bytes (BE) at Description`[0:8]`
/// 2. Writes version_major=1, version_minor=2 at Description`[8:12]`
/// 3. Writes marker data at Description`[12:44]`
/// 4. Sets bext_version to 2 at offset 346 (signals EBU Tech 3285 compliance)
///
/// Preserves all other BEXT fields (Originator, OriginatorReference, Date, UMID, etc.)
/// since those live at fixed offsets outside `[0:44]`.
///
/// The UUID v7 high 64 bits encode: 48-bit ms timestamp | 0x7 version nibble | 12-bit random.
/// A non-zero file_id combined with version_major=1 and bext_version=2 is definitively
/// riffgrep-packed — collision probability ~1/370K for a 10M file collection.
pub fn init_packed_and_write_markers(
    path: &std::path::Path,
    markers: &MarkerConfig,
) -> Result<(), BextWriteError> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(4096, file);
    let map = scan_chunks(&mut reader)?;

    let _bext_offset = map.bext_offset.ok_or(BextWriteError::NoBextChunk)?;

    if map.bext_size < BEXT_STANDARD_SIZE as u32 {
        return Err(BextWriteError::BextTooSmall {
            actual: map.bext_size,
            offset: 0,
            len: BEXT_STANDARD_SIZE,
        });
    }

    drop(reader);

    // 1. Generate UUID v7 and write the high 8 bytes (big-endian) at Description[0:8].
    //    High 64 bits = 48-bit ms timestamp | 4-bit version nibble (0x7) | 12-bit random.
    let uuid = uuid::Uuid::now_v7();
    let uuid_bytes = uuid.as_bytes();
    write_bext_field(path, &map, 0, &uuid_bytes[..8])?;

    // 2. Write version_major=1, version_minor=2 at Description[8:12].
    write_bext_field(path, &map, 8, &1u16.to_le_bytes())?;
    write_bext_field(path, &map, 10, &PACKED_VERSION_MINOR_V2.to_le_bytes())?;

    // 3. Write marker data at Description[12:44].
    write_bext_field(path, &map, 12, &markers.to_bytes())?;

    // 4. Set bext_version=2 at offset 346 (signals EBU Tech 3285 compliance and
    //    honest acknowledgment of non-standard use of loudness fields for u8 peaks).
    write_bext_field(path, &map, 346, &2u16.to_le_bytes())?;

    Ok(())
}

// --- MARKERSv2 binary format ---

/// Sentinel value indicating an unused marker slot (all-ones u32).
pub const MARKER_EMPTY: u32 = u32::MAX;

/// Total size of the marker block in the packed Description field (bytes 12-43).
pub const MARKER_BLOCK_SIZE: usize = 32;

/// A bank of 3 markers with 4 repetition nibbles.
///
/// Binary layout (16 bytes):
/// - `[0:4]`   m1 (u32 LE) — absolute sample offset
/// - `[4:8]`   m2 (u32 LE) — absolute sample offset
/// - `[8:12]`  m3 (u32 LE) — absolute sample offset
/// - `[12:14]` nibbles — packed repetitions: byte0 = `(S1<<4)|S2`, byte1 = `(S3<<4)|S4`
/// - `[14:16]` padding (zero)
///
/// Repetition nibble values: 0=skip, 1-14=repeat count, 15=infinite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerBank {
    /// Marker 1: absolute sample offset (MARKER_EMPTY if unused).
    pub m1: u32,
    /// Marker 2: absolute sample offset (MARKER_EMPTY if unused).
    pub m2: u32,
    /// Marker 3: absolute sample offset (MARKER_EMPTY if unused).
    pub m3: u32,
    /// Repetition nibbles for 4 segments: 0=skip, 1-14=repeat, 15=infinite.
    pub reps: [u8; 4],
}

impl MarkerBank {
    /// Create an empty marker bank (all slots unused, no repetitions).
    pub fn empty() -> Self {
        Self {
            m1: MARKER_EMPTY,
            m2: MARKER_EMPTY,
            m3: MARKER_EMPTY,
            reps: [0; 4],
        }
    }

    /// Serialize to 16 bytes.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&self.m1.to_le_bytes());
        buf[4..8].copy_from_slice(&self.m2.to_le_bytes());
        buf[8..12].copy_from_slice(&self.m3.to_le_bytes());
        let nibbles = Self::pack_nibbles(&self.reps);
        buf[12..14].copy_from_slice(&nibbles);
        // buf[14..16] stays zero (padding)
        buf
    }

    /// Deserialize from 16 bytes.
    pub fn from_bytes(buf: &[u8; 16]) -> Self {
        let m1 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let m2 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let m3 = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let reps = Self::unpack_nibbles([buf[12], buf[13]]);
        Self { m1, m2, m3, reps }
    }

    /// Whether all marker slots are empty.
    pub fn is_empty(&self) -> bool {
        self.m1 == MARKER_EMPTY && self.m2 == MARKER_EMPTY && self.m3 == MARKER_EMPTY
    }

    /// Return sample offsets for defined (non-empty) markers.
    pub fn defined_markers(&self) -> Vec<u32> {
        let mut v = Vec::new();
        if self.m1 != MARKER_EMPTY {
            v.push(self.m1);
        }
        if self.m2 != MARKER_EMPTY {
            v.push(self.m2);
        }
        if self.m3 != MARKER_EMPTY {
            v.push(self.m3);
        }
        v
    }

    /// Pack 4 nibble values into 2 bytes: byte0 = `(S1<<4)|S2`, byte1 = `(S3<<4)|S4`.
    fn pack_nibbles(reps: &[u8; 4]) -> [u8; 2] {
        [
            (reps[0] & 0x0F) << 4 | (reps[1] & 0x0F),
            (reps[2] & 0x0F) << 4 | (reps[3] & 0x0F),
        ]
    }

    /// Unpack 2 bytes into 4 nibble values.
    fn unpack_nibbles(bytes: [u8; 2]) -> [u8; 4] {
        [
            (bytes[0] >> 4) & 0x0F,
            bytes[0] & 0x0F,
            (bytes[1] >> 4) & 0x0F,
            bytes[1] & 0x0F,
        ]
    }
}

impl Default for MarkerBank {
    fn default() -> Self {
        Self::empty()
    }
}

/// Two-bank marker configuration (32 bytes total).
///
/// Banks A and B each hold 3 markers + repetition nibbles. Both banks are
/// interpreted sequentially with a single playhead (not dual-playhead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkerConfig {
    /// Bank A markers and repetitions.
    pub bank_a: MarkerBank,
    /// Bank B markers and repetitions.
    pub bank_b: MarkerBank,
}

#[allow(dead_code)] // Convenience constructors used by tests and TUI marker system.
impl MarkerConfig {
    /// Create an empty marker config (both banks empty).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Serialize to 32 bytes (bank_a ++ bank_b).
    pub fn to_bytes(self) -> [u8; MARKER_BLOCK_SIZE] {
        let mut buf = [0u8; MARKER_BLOCK_SIZE];
        buf[0..16].copy_from_slice(&self.bank_a.to_bytes());
        buf[16..32].copy_from_slice(&self.bank_b.to_bytes());
        buf
    }

    /// Deserialize from 32 bytes.
    pub fn from_bytes(buf: &[u8; MARKER_BLOCK_SIZE]) -> Self {
        let a: [u8; 16] = buf[0..16].try_into().unwrap();
        let b: [u8; 16] = buf[16..32].try_into().unwrap();
        Self {
            bank_a: MarkerBank::from_bytes(&a),
            bank_b: MarkerBank::from_bytes(&b),
        }
    }

    /// Preset: "shot" — all markers at sample 0, reps=`[0,0,0,1]`, both banks identical.
    pub fn preset_shot() -> Self {
        let bank = MarkerBank {
            m1: 0,
            m2: 0,
            m3: 0,
            reps: [0, 0, 0, 1],
        };
        Self {
            bank_a: bank,
            bank_b: bank,
        }
    }

    /// Preset: "loop" — markers at quarter points, reps=`[1,1,1,1]`.
    pub fn preset_loop(total_samples: u32) -> Self {
        let q1 = total_samples / 4;
        let q2 = total_samples / 2;
        let q3 = total_samples * 3 / 4;
        let bank = MarkerBank {
            m1: q1,
            m2: q2,
            m3: q3,
            reps: [1, 1, 1, 1],
        };
        Self {
            bank_a: bank,
            bank_b: bank,
        }
    }

    /// Whether both banks are empty.
    pub fn is_empty(&self) -> bool {
        self.bank_a.is_empty() && self.bank_b.is_empty()
    }

    /// Whether both banks are identical.
    pub fn is_synced(&self) -> bool {
        self.bank_a == self.bank_b
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Build a minimal RIFF/WAVE header + chunks for testing.
    fn make_riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        // Accumulate chunk data to compute RIFF size.
        let mut chunk_bytes = Vec::new();
        for (id, payload) in chunks {
            chunk_bytes.extend_from_slice(*id);
            chunk_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            chunk_bytes.extend_from_slice(payload);
            // WORD-align.
            if payload.len() % 2 != 0 {
                chunk_bytes.push(0);
            }
        }
        // RIFF header.
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&(4 + chunk_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(&chunk_bytes);
        data
    }

    fn make_list_info(subchunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut info_data = Vec::new();
        info_data.extend_from_slice(b"INFO");
        for (id, payload) in subchunks {
            info_data.extend_from_slice(*id);
            info_data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            info_data.extend_from_slice(payload);
            if payload.len() % 2 != 0 {
                info_data.push(0);
            }
        }
        info_data
    }

    #[test]
    fn scan_bext_at_offset_36() {
        let bext_data = vec![0u8; 604];
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"bext", &bext_data)]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        // fmt is at offset 12, size 16+8=24 -> bext starts at 12+24=36, data at 36+8=44
        assert_eq!(map.bext_offset, Some(44));
        assert_eq!(map.bext_size, 604);
    }

    #[test]
    fn scan_no_bext() {
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"data", &[0u8; 100])]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert_eq!(map.bext_offset, None);
        assert_eq!(map.bext_size, 0);
    }

    #[test]
    fn scan_word_alignment() {
        // Odd-sized fmt chunk (17 bytes) should be padded to 18 before bext.
        let bext_data = vec![0u8; 604];
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 17]), // odd size
            (b"bext", &bext_data),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        // fmt: offset 12, header 8, data 17, pad 1 = 26 -> bext at 12+26=38, data at 38+8=46
        assert_eq!(map.bext_offset, Some(46));
        assert_eq!(map.bext_size, 604);
    }

    #[test]
    fn scan_bext_and_list_info() {
        let bext_data = vec![0u8; 604];
        let info = make_list_info(&[(b"IART", b"TestArtist\0")]);
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert!(map.bext_offset.is_some());
        assert!(map.info_offset.is_some());
        assert!(map.info_size > 0);
    }

    #[test]
    fn scan_list_info_beyond_4kb_not_found() {
        // Put bext at the start, then a large data chunk, then LIST-INFO > 4KB.
        let bext_data = vec![0u8; 604];
        let big_data = vec![0u8; 4096]; // pushes LIST beyond scan limit
        let info = make_list_info(&[(b"IART", b"TestArtist\0")]);
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"data", &big_data),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert!(map.bext_offset.is_some());
        // LIST-INFO is past 4KB so should not be found.
        assert_eq!(map.info_offset, None);
    }

    #[test]
    fn scan_empty_file() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result = scan_chunks(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn scan_text_file() {
        let mut cursor = Cursor::new(b"Hello, this is not a WAV file at all!");
        let result = scan_chunks(&mut cursor);
        assert!(matches!(result, Err(RiffError::NotRiffWave)));
    }

    #[test]
    fn scan_truncated_riff_header() {
        let mut cursor = Cursor::new(b"RIFF\x00");
        let result = scan_chunks(&mut cursor);
        assert!(result.is_err());
    }

    // --- BEXT parser tests (Ticket 3) ---

    /// Build a 602-byte BEXT buffer with known values at every field.
    fn make_bext_buffer_packed() -> [u8; BEXT_STANDARD_SIZE] {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        // file_id [000:008] = 985188 (stored big-endian, as UUID v7 high bytes are)
        buf[0..8].copy_from_slice(&985188u64.to_be_bytes());
        // version [008:012] = 1.2 (version_major=1, version_minor=2)
        buf[8..10].copy_from_slice(&1u16.to_le_bytes());
        buf[10..12].copy_from_slice(&2u16.to_le_bytes());
        // markers [012:044] = zeros (unused)
        // COMR/Comment [044:076] = "Sequential Circuits Prophet-10"
        buf[44..74].copy_from_slice(b"Sequential Circuits Prophet-10");
        // POPM/Rating [076:080] = "****"
        buf[76..80].copy_from_slice(b"****");
        // TBPM/BPM [080:084] = "164 "
        buf[80..84].copy_from_slice(b"164 ");
        // TCOM/Subcategory [084:088] = "DEMO"
        buf[84..88].copy_from_slice(b"DEMO");
        // TCON/Category [088:092] = "LOOP"
        buf[88..92].copy_from_slice(b"LOOP");
        // TIT1/Genre ID [092:096] = "ACID"
        buf[92..96].copy_from_slice(b"ACID");
        // TIT2/Sound ID [096:100] = "DHC "
        buf[96..100].copy_from_slice(b"DHC ");
        // TIT3/Usage ID [100:104] = "XPM "
        buf[100..104].copy_from_slice(b"XPM ");
        // TKEY/Key [104:112] = "A#m\0\0\0\0\0"
        buf[104..107].copy_from_slice(b"A#m");
        // Take [112:116] = "67  "
        buf[112..114].copy_from_slice(b"67");
        // Track [116:120] = "1   "
        buf[116..117].copy_from_slice(b"1");
        // Item [120:128] = "12345678"
        buf[120..128].copy_from_slice(b"12345678");
        // Originator [256:288] = "Samples From Mars"
        buf[256..273].copy_from_slice(b"Samples From Mars");
        // OriginatorReference [288:320] = "DX100 From Mars"
        buf[288..303].copy_from_slice(b"DX100 From Mars");
        // OriginationDate [320:330] = "2024-01-15"
        buf[320..330].copy_from_slice(b"2024-01-15");
        // BWF Version [346:348] = 2 (signals EBU Tech 3285; required for RiffgrepU8 detection).
        buf[346..348].copy_from_slice(&2u16.to_le_bytes());
        // UMID [348:412] = "976132720e774b668c36826386ae6505" as ASCII
        buf[348..380].copy_from_slice(b"976132720e774b668c36826386ae6505");
        buf
    }

    #[test]
    fn parse_packed_bext_all_fields() {
        let buf = make_bext_buffer_packed();
        let fields = parse_bext_buffer(&buf);
        assert!(fields.packed);
        assert_eq!(fields.file_id, 985188);
        assert_eq!(fields.comment, "Sequential Circuits Prophet-10");
        assert_eq!(fields.description, "Sequential Circuits Prophet-10");
        assert_eq!(fields.rating, "****");
        assert_eq!(fields.bpm, Some(164));
        assert_eq!(fields.subcategory, "DEMO");
        assert_eq!(fields.category, "LOOP");
        assert_eq!(fields.genre_id, "ACID");
        assert_eq!(fields.sound_id, "DHC");
        assert_eq!(fields.usage_id, "XPM");
        assert_eq!(fields.key, "A#m");
        assert_eq!(fields.vendor, "Samples From Mars");
        assert_eq!(fields.library, "DX100 From Mars");
        assert_eq!(fields.date, "2024-01-15");
        assert_eq!(fields.umid, "976132720e774b668c36826386ae6505");
        assert_eq!(fields.take, "67");
        assert_eq!(fields.track, "1");
        assert_eq!(fields.item, "12345678");
    }

    #[test]
    fn parse_all_zeros() {
        let buf = [0u8; BEXT_STANDARD_SIZE];
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed);
        assert_eq!(fields.file_id, 0);
        assert_eq!(fields.description, "");
        assert_eq!(fields.vendor, "");
        assert_eq!(fields.library, "");
        assert_eq!(fields.umid, "");
        assert_eq!(fields.bpm, None);
    }

    #[test]
    fn parse_plain_text_description() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..14].copy_from_slice(b"Yamaha DX-100\0");
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed);
        assert_eq!(fields.description, "Yamaha DX-100");
        // Packed fields should be defaults.
        assert_eq!(fields.category, "");
        assert_eq!(fields.sound_id, "");
        assert_eq!(fields.bpm, None);
    }

    #[test]
    fn parse_bext_data_from_reader() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..14].copy_from_slice(b"Yamaha DX-100\0");
        buf[256..273].copy_from_slice(b"Samples From Mars");
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"bext", &buf)]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        let fields = parse_bext_data(&mut cursor, &map).unwrap();
        assert_eq!(fields.description, "Yamaha DX-100");
        assert_eq!(fields.vendor, "Samples From Mars");
    }

    #[test]
    fn parse_bext_data_no_bext() {
        let map = ChunkMap::default();
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let fields = parse_bext_data(&mut cursor, &map).unwrap();
        assert_eq!(fields.description, "");
        assert_eq!(fields.vendor, "");
    }

    #[test]
    fn integration_parse_clean_base_bext() {
        let path = "test_files/clean_base.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed);
        assert_eq!(fields.description, "Yamaha DX-100");
    }

    #[test]
    fn integration_parse_reaper_sm_bext() {
        let path = "test_files/riff+defaults-info_reaper-sm.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert_eq!(fields.description, "project note");
    }

    #[test]
    fn integration_clean_base() {
        let path = "test_files/clean_base.wav";
        if !std::path::Path::new(path).exists() {
            return; // Skip if test files not available.
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        // clean_base.wav has bext at offset 36, data at 44.
        assert_eq!(map.bext_offset, Some(44));
        assert_eq!(map.bext_size, 604);
        // No LIST-INFO within first 4KB (data chunk follows bext).
        assert_eq!(map.info_offset, None);
    }

    #[test]
    fn integration_all_riff_info() {
        let path = "test_files/all_riff_info_tags_with_numbers.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        // bext at offset 36, data at 44.
        assert_eq!(map.bext_offset, Some(44));
        assert_eq!(map.bext_size, 604);
        // LIST-INFO at offset 648, data at 656.
        assert!(map.info_offset.is_some());
        let info_off = map.info_offset.unwrap();
        assert_eq!(info_off, 656);
        assert_eq!(map.info_size, 1740);
    }

    #[test]
    fn integration_reaper_sm() {
        let path = "test_files/riff+defaults-info_reaper-sm.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        // bext at offset 36.
        assert_eq!(map.bext_offset, Some(44));
        // LIST-INFO is after audio data (>4KB), so not found in fast scan.
        assert_eq!(map.info_offset, None);
    }

    // --- Edge case tests ---

    #[test]
    fn bext_chunk_smaller_than_602() {
        let bext_data = vec![0u8; 100];
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"bext", &bext_data)]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert_eq!(map.bext_size, 100);
        let result = parse_bext_data(&mut cursor, &map);
        assert!(result.is_err());
    }

    #[test]
    fn bext_chunk_larger_than_602() {
        let bext_data = vec![0u8; 800]; // CodingHistory appended
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"bext", &bext_data)]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert_eq!(map.bext_size, 800);
        let fields = parse_bext_data(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "");
    }

    #[test]
    fn wav_no_chunks_after_header() {
        let riff = make_riff(&[]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert!(map.bext_offset.is_none());
        assert!(map.info_offset.is_none());
    }

    #[test]
    fn riff_non_wave_form_type() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(b"AVI ");
        buf.extend_from_slice(&[0u8; 96]);
        let mut cursor = Cursor::new(buf);
        let result = scan_chunks(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn file_shorter_than_12_bytes() {
        let mut cursor = Cursor::new(b"RIFF".to_vec());
        let result = scan_chunks(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn utf8_in_bext_description() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        // UTF-8: "Über" = C3 9C 62 65 72
        buf[0] = 0xC3;
        buf[1] = 0x9C;
        buf[2] = b'b';
        buf[3] = b'e';
        buf[4] = b'r';
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.description, "\u{00DC}ber");
    }

    // --- T4 tests: PeaksFormat detection ---

    #[test]
    fn test_peaks_format_riffgrep_u8() {
        let mut buf = make_bext_buffer_packed();
        // Set some non-zero peaks in Reserved[422:602].
        for i in 422..602 {
            buf[i] = (i % 256) as u8;
        }
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        assert_eq!(
            fields.bext_version, 2,
            "packed helper should set bext_version=2"
        );
        assert!(!fields.peaks.is_empty());
    }

    #[test]
    fn test_peaks_format_packed_without_bwf_version_is_bwf_reserved() {
        // Schema marker at bytes 8-11 present but bext_version = 0 → NOT recognized
        // as packed schema at all. Peaks are BwfReserved, metadata parsed as plain text.
        let mut buf = make_bext_buffer_packed();
        // Override BWF version to 0.
        buf[346] = 0;
        buf[347] = 0;
        // Set non-zero Reserved data.
        for i in 422..602 {
            buf[i] = (i % 256) as u8;
        }
        let fields = parse_bext_buffer(&buf);
        assert_eq!(
            fields.peaks_format,
            PeaksFormat::BwfReserved,
            "bext_version=0 should not produce RiffgrepU8"
        );
        assert!(
            !fields.packed,
            "bext_version=0 should not activate packed schema"
        );
    }

    #[test]
    fn test_peaks_format_packed_v0_empty_reserved() {
        // Schema marker present with bext_version=0 and all-zero Reserved.
        // Neither packed schema nor peaks should be detected.
        let mut buf = make_bext_buffer_packed();
        buf[346] = 0;
        buf[347] = 0;
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.peaks_format, PeaksFormat::Empty);
        assert!(
            !fields.packed,
            "bext_version=0 should not activate packed schema"
        );
    }

    #[test]
    fn test_peaks_format_bwf_reserved() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        // BEXT Version >= 1 at offset 346.
        buf[346] = 1; // version = 1
        // Non-zero Reserved.
        buf[422] = 0xFF;
        buf[500] = 0x42;
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.peaks_format, PeaksFormat::BwfReserved);
        assert_eq!(fields.bext_version, 1);
    }

    #[test]
    fn test_peaks_format_empty() {
        let buf = [0u8; BEXT_STANDARD_SIZE];
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.peaks_format, PeaksFormat::Empty);
        assert!(fields.peaks.is_empty());
    }

    #[test]
    fn test_peaks_format_unknown_nonzero_is_bwf() {
        // BEXT version=0, no packed schema, but non-zero Reserved data.
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[422] = 0xAB;
        buf[430] = 0xCD;
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.peaks_format, PeaksFormat::BwfReserved);
        assert_eq!(fields.bext_version, 0);
    }

    #[test]
    fn test_bext_version_field_read() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[346] = 2; // version 2 (LE)
        buf[347] = 0;
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.bext_version, 2);
    }

    #[test]
    fn integration_id3_all_r7_is_bwf_reserved() {
        let path = "test_files/id3-all_r7.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        // Reaper file with non-zero Reserved → BwfReserved.
        // (Clean files with no bext peaks will be Empty)
        // Check based on actual file content:
        assert!(
            fields.peaks_format == PeaksFormat::BwfReserved
                || fields.peaks_format == PeaksFormat::Empty,
            "expected BwfReserved or Empty for Reaper file, got {:?}",
            fields.peaks_format,
        );
    }

    // --- Version guard boundary tests ---

    #[test]
    fn bext_v1_without_packed_schema_reads_umid() {
        // BWF v1 file from a compliant tool (no packed schema marker).
        // UMID should be read, but packed should be false.
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..11].copy_from_slice(b"hello world"); // plain text description
        buf[346..348].copy_from_slice(&1u16.to_le_bytes()); // bext_version = 1
        buf[348..380].copy_from_slice(b"abc123def456abc123def456abc123de"); // UMID ASCII
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed, "no schema marker → not packed");
        assert_eq!(fields.bext_version, 1);
        assert_eq!(fields.umid, "abc123def456abc123def456abc123de");
        assert_eq!(fields.description, "hello world");
    }

    #[test]
    fn bext_v1_without_packed_schema_nonzero_reserved_is_bwf() {
        // BWF v1 file with non-zero Reserved but no packed schema.
        // Peaks should be BwfReserved (not RiffgrepU8).
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[346..348].copy_from_slice(&1u16.to_le_bytes()); // bext_version = 1
        // No schema marker at [8:12].
        buf[422] = 0xFF;
        buf[500] = 0x42;
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed);
        assert_eq!(fields.peaks_format, PeaksFormat::BwfReserved);
    }

    #[test]
    fn bext_v2_without_packed_schema() {
        // BWF v2 (loudness extension). Same rules apply — no packed schema
        // means plain Description, UMID readable, Reserved is BwfReserved.
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..14].copy_from_slice(b"loudness test\0");
        buf[346..348].copy_from_slice(&2u16.to_le_bytes()); // bext_version = 2
        buf[348..380].copy_from_slice(b"00112233445566778899aabbccddeeff");
        buf[422] = 0x01; // non-zero Reserved
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.bext_version, 2);
        assert!(!fields.packed, "no schema marker → not packed");
        assert_eq!(fields.umid, "00112233445566778899aabbccddeeff");
        assert_eq!(fields.peaks_format, PeaksFormat::BwfReserved);
        assert_eq!(fields.description, "loudness test");
    }

    #[test]
    fn bext_v0_plain_description_no_umid() {
        // Typical third-party v0 file: Description is plain text, UMID area
        // may have garbage. Verify UMID is NOT read.
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..18].copy_from_slice(b"SoundMiner export\0");
        // bext_version = 0 (default zeros).
        // Put garbage in the UMID area.
        for i in 348..412 {
            buf[i] = 0xAB;
        }
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.bext_version, 0);
        assert!(!fields.packed);
        assert!(
            fields.umid.is_empty(),
            "v0 files must not have UMID read (got {:?})",
            fields.umid
        );
        assert_eq!(fields.description, "SoundMiner export");
    }

    #[test]
    fn version_round_trip_packed_schema() {
        // Simulate a riffgrep-written file: packed schema v1.2 + bext_version=2.
        // All version-dependent fields should be present.
        let mut buf = make_bext_buffer_packed();
        // Set non-zero peaks.
        for i in 422..602 {
            buf[i] = ((i * 7) % 256) as u8;
        }
        let fields = parse_bext_buffer(&buf);
        // Version-dependent features all active:
        assert!(fields.packed, "packed schema should be detected");
        assert_eq!(fields.bext_version, 2);
        assert_eq!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        assert!(!fields.umid.is_empty(), "UMID should be read for v>=1");
        assert_eq!(fields.file_id, 985188);
        assert_eq!(fields.category, "LOOP");
        assert_eq!(fields.sound_id, "DHC");
        assert!(!fields.peaks.is_empty());
        assert_eq!(fields.peaks.len(), 180);
    }

    // --- Real-world file regression tests ---
    // These test that third-party files are NEVER misidentified as having
    // riffgrep-encoded data. Each test verifies:
    //   - packed == false
    //   - peaks_format != RiffgrepU8
    //   - UMID is empty (since test files are bext_version=0)

    #[test]
    fn regression_clean_base_not_riffgrep() {
        let path = "test_files/clean_base.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed, "clean_base.wav should not be packed");
        assert_ne!(
            fields.peaks_format,
            PeaksFormat::RiffgrepU8,
            "clean_base.wav should not have RiffgrepU8 peaks"
        );
        if fields.bext_version == 0 {
            assert!(
                fields.umid.is_empty(),
                "v0 file should not have UMID (got {:?})",
                fields.umid
            );
        }
    }

    #[test]
    fn regression_all_riff_info_not_riffgrep() {
        let path = "test_files/all_riff_info_tags_with_numbers.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed);
        assert_ne!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        if fields.bext_version == 0 {
            assert!(fields.umid.is_empty());
        }
    }

    #[test]
    fn regression_all_riff_info_sm_not_riffgrep() {
        let path = "test_files/all_riff_info_tags_with_numbers-sm.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed);
        assert_ne!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        if fields.bext_version == 0 {
            assert!(fields.umid.is_empty());
        }
    }

    #[test]
    fn regression_id3_all_r7_not_riffgrep() {
        let path = "test_files/id3-all_r7.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed);
        assert_ne!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        if fields.bext_version == 0 {
            assert!(fields.umid.is_empty());
        }
    }

    #[test]
    fn regression_id3_all_sm_not_riffgrep() {
        let path = "test_files/id3-all_sm.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map).unwrap();
        assert!(!fields.packed);
        assert_ne!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        if fields.bext_version == 0 {
            assert!(fields.umid.is_empty());
        }
    }

    #[test]
    fn regression_id3_only_not_riffgrep() {
        let path = "test_files/id3-only.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        // id3-only may not have a BEXT chunk at all.
        if map.bext_offset.is_some() {
            let fields = parse_bext_data(&mut file, &map).unwrap();
            assert!(!fields.packed);
            assert_ne!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        }
    }

    // --- S9-T1 tests: MarkerBank & MarkerConfig ---

    #[test]
    fn test_marker_bank_roundtrip() {
        let bank = MarkerBank {
            m1: 1000,
            m2: 2000,
            m3: 3000,
            reps: [1, 2, 3, 4],
        };
        let bytes = bank.to_bytes();
        let roundtripped = MarkerBank::from_bytes(&bytes);
        assert_eq!(roundtripped, bank);
    }

    #[test]
    fn test_marker_bank_empty() {
        let bank = MarkerBank::empty();
        assert!(bank.is_empty());
        assert!(bank.defined_markers().is_empty());
        assert_eq!(bank.m1, MARKER_EMPTY);
        assert_eq!(bank.m2, MARKER_EMPTY);
        assert_eq!(bank.m3, MARKER_EMPTY);
        assert_eq!(bank.reps, [0; 4]);
    }

    #[test]
    fn test_marker_config_roundtrip() {
        let config = MarkerConfig {
            bank_a: MarkerBank {
                m1: 100,
                m2: 200,
                m3: 300,
                reps: [1, 14, 0, 15],
            },
            bank_b: MarkerBank {
                m1: 400,
                m2: 500,
                m3: 600,
                reps: [5, 6, 7, 8],
            },
        };
        let bytes = config.to_bytes();
        let roundtripped = MarkerConfig::from_bytes(&bytes);
        assert_eq!(roundtripped, config);
    }

    #[test]
    fn test_nibble_packing() {
        let reps = [1u8, 14, 0, 15];
        let packed = MarkerBank::pack_nibbles(&reps);
        // byte0 = (1<<4)|14 = 0x1E, byte1 = (0<<4)|15 = 0x0F
        assert_eq!(packed, [0x1E, 0x0F]);
        let unpacked = MarkerBank::unpack_nibbles(packed);
        assert_eq!(unpacked, reps);
    }

    #[test]
    fn test_preset_shot() {
        let config = MarkerConfig::preset_shot();
        assert_eq!(config.bank_a.m1, 0);
        assert_eq!(config.bank_a.m2, 0);
        assert_eq!(config.bank_a.m3, 0);
        assert_eq!(config.bank_a.reps, [0, 0, 0, 1]);
        assert!(config.is_synced());
        assert!(!config.is_empty());
    }

    #[test]
    fn test_preset_loop_48000() {
        let config = MarkerConfig::preset_loop(48000);
        assert_eq!(config.bank_a.m1, 12000);
        assert_eq!(config.bank_a.m2, 24000);
        assert_eq!(config.bank_a.m3, 36000);
        assert_eq!(config.bank_a.reps, [1, 1, 1, 1]);
        assert!(config.is_synced());
        assert!(!config.is_empty());
    }

    #[test]
    fn test_marker_bank_defined_markers() {
        let bank = MarkerBank {
            m1: 100,
            m2: MARKER_EMPTY,
            m3: 300,
            reps: [0; 4],
        };
        assert!(!bank.is_empty());
        let defined = bank.defined_markers();
        assert_eq!(defined, vec![100, 300]);
    }

    #[test]
    fn test_marker_config_empty() {
        let config = MarkerConfig::empty();
        assert!(config.is_empty());
        assert!(config.is_synced());
    }

    #[test]
    fn test_marker_config_not_synced() {
        let config = MarkerConfig {
            bank_a: MarkerBank {
                m1: 100,
                m2: MARKER_EMPTY,
                m3: MARKER_EMPTY,
                reps: [0; 4],
            },
            bank_b: MarkerBank::empty(),
        };
        assert!(!config.is_synced());
    }

    #[test]
    fn test_marker_bank_max_values() {
        let bank = MarkerBank {
            m1: u32::MAX - 1,
            m2: 0,
            m3: u32::MAX,
            reps: [15, 15, 15, 15],
        };
        let bytes = bank.to_bytes();
        let rt = MarkerBank::from_bytes(&bytes);
        assert_eq!(rt, bank);
    }

    #[test]
    fn test_nibble_all_zeros() {
        let reps = [0u8; 4];
        let packed = MarkerBank::pack_nibbles(&reps);
        assert_eq!(packed, [0, 0]);
        let unpacked = MarkerBank::unpack_nibbles(packed);
        assert_eq!(unpacked, reps);
    }

    #[test]
    fn test_nibble_all_fifteens() {
        let reps = [15u8; 4];
        let packed = MarkerBank::pack_nibbles(&reps);
        assert_eq!(packed, [0xFF, 0xFF]);
        let unpacked = MarkerBank::unpack_nibbles(packed);
        assert_eq!(unpacked, reps);
    }

    // --- S9-T5 tests: Write markers to BEXT ---

    /// Create a temp WAV file with a packed BEXT chunk (version 1.2, bext_version=2).
    fn make_temp_wav_packed(suffix: &str) -> (std::path::PathBuf, ChunkMap) {
        let mut bext_data = [0u8; BEXT_STANDARD_SIZE];
        // file_id [0:8] — non-zero fixture value, big-endian.
        bext_data[0..8].copy_from_slice(&0x0000_0001_0000_0001u64.to_be_bytes());
        // Packed schema: version_major=1, version_minor=2.
        bext_data[8..10].copy_from_slice(&1u16.to_le_bytes());
        bext_data[10..12].copy_from_slice(&2u16.to_le_bytes());
        // BWF version = 2 at offset 346.
        bext_data[346..348].copy_from_slice(&2u16.to_le_bytes());
        // Some comment text.
        bext_data[44..55].copy_from_slice(b"TestComment");
        bext_data[88..92].copy_from_slice(b"LOOP");

        let audio_data = vec![0u8; 1000];
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"data", &audio_data),
        ]);
        let path = std::env::temp_dir().join(format!(
            "riffgrep_test_packed_{}_{}.wav",
            suffix,
            std::process::id()
        ));
        std::fs::write(&path, &riff).unwrap();

        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        (path, map)
    }

    #[test]
    fn test_write_markers_roundtrip() {
        let (path, _map) = make_temp_wav_packed("wm_rt");
        let markers = MarkerConfig::preset_loop(48000);
        write_markers(&path, &markers).unwrap();

        // Re-read and verify.
        let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let map2 = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map2).unwrap();
        assert_eq!(fields.markers, Some(markers));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_markers_bumps_version() {
        let (path, _map) = make_temp_wav_packed("wm_ver");
        let markers = MarkerConfig::preset_shot();
        write_markers(&path, &markers).unwrap();

        // Re-read and check version minor.
        let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let map2 = scan_chunks(&mut file).unwrap();
        let bext_off = map2.bext_offset.unwrap();
        file.seek(SeekFrom::Start(bext_off + 10)).unwrap();
        let mut ver_buf = [0u8; 2];
        file.read_exact(&mut ver_buf).unwrap();
        let version_minor = u16::from_le_bytes(ver_buf);
        assert_eq!(version_minor, PACKED_VERSION_MINOR_V2);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_markers_no_bext_errors() {
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"data", &[0u8; 100])]);
        let path = std::env::temp_dir().join(format!(
            "riffgrep_test_wm_no_bext_{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, &riff).unwrap();

        let result = write_markers(&path, &MarkerConfig::empty());
        assert!(matches!(result, Err(BextWriteError::NoBextChunk)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_markers_not_packed_errors() {
        // File with plain-text BEXT (no packed schema marker).
        let mut bext_data = [0u8; BEXT_STANDARD_SIZE];
        bext_data[0..14].copy_from_slice(b"Plain text bxt");
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"data", &[0u8; 100]),
        ]);
        let path = std::env::temp_dir().join(format!(
            "riffgrep_test_wm_not_packed_{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, &riff).unwrap();

        let result = write_markers(&path, &MarkerConfig::empty());
        assert!(matches!(result, Err(BextWriteError::NotPacked)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_markers_preserves_other_fields() {
        let (path, _map) = make_temp_wav_packed("wm_preserve");
        let markers = MarkerConfig::preset_loop(48000);
        write_markers(&path, &markers).unwrap();

        // Re-read and verify other packed fields are unchanged.
        let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let map2 = scan_chunks(&mut file).unwrap();
        let fields = parse_bext_data(&mut file, &map2).unwrap();
        assert!(fields.packed);
        assert_eq!(fields.comment, "TestComment");
        assert_eq!(fields.category, "LOOP");

        std::fs::remove_file(&path).unwrap();
    }

    // --- S9-T4 tests: Surgical BEXT byte writer ---

    /// Create a temp WAV file with a BEXT chunk for write testing.
    fn make_temp_wav_with_bext(suffix: &str) -> (std::path::PathBuf, ChunkMap) {
        let bext_data = vec![0u8; BEXT_STANDARD_SIZE];
        let audio_data = vec![0u8; 1000];
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"data", &audio_data),
        ]);
        let path = std::env::temp_dir().join(format!(
            "riffgrep_test_write_{}_{}.wav",
            suffix,
            std::process::id()
        ));
        std::fs::write(&path, &riff).unwrap();

        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        (path, map)
    }

    #[test]
    fn test_write_bext_field_roundtrip() {
        let (path, map) = make_temp_wav_with_bext("rt");
        let original_size = std::fs::metadata(&path).unwrap().len();

        // Write 4 bytes at offset 44 (within BEXT).
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        write_bext_field(&path, &map, 44, &data).unwrap();

        // Verify file size unchanged.
        assert_eq!(std::fs::metadata(&path).unwrap().len(), original_size);

        // Re-read and verify.
        let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let bext_off = map.bext_offset.unwrap();
        file.seek(SeekFrom::Start(bext_off + 44)).unwrap();
        let mut readback = [0u8; 4];
        file.read_exact(&mut readback).unwrap();
        assert_eq!(readback, data);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_bext_field_no_bext_errors() {
        let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"data", &[0u8; 100])]);
        let path =
            std::env::temp_dir().join(format!("riffgrep_test_no_bext_{}.wav", std::process::id()));
        std::fs::write(&path, &riff).unwrap();

        let map = ChunkMap::default(); // No BEXT offset
        let result = write_bext_field(&path, &map, 0, &[0]);
        assert!(matches!(result, Err(BextWriteError::NoBextChunk)));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_bext_field_too_small() {
        let (path, mut map) = make_temp_wav_with_bext("small");
        // Pretend BEXT is only 100 bytes.
        map.bext_size = 100;

        let result = write_bext_field(&path, &map, 500, &[0; 10]);
        assert!(matches!(result, Err(BextWriteError::BextTooSmall { .. })));

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_write_bext_preserves_audio() {
        let bext_data = vec![0u8; BEXT_STANDARD_SIZE];
        let audio_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
            (b"data", &audio_data),
        ]);
        let path = std::env::temp_dir().join(format!(
            "riffgrep_test_audio_preserve_{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, &riff).unwrap();

        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();

        // Write to BEXT.
        write_bext_field(&path, &map, 0, &[0xFF; 32]).unwrap();

        // Re-read audio data and verify unchanged.
        let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
        let data_off = map.data_offset.unwrap();
        file.seek(SeekFrom::Start(data_off)).unwrap();
        let mut readback = vec![0u8; 1000];
        file.read_exact(&mut readback).unwrap();
        assert_eq!(readback, audio_data);

        std::fs::remove_file(&path).unwrap();
    }

    // --- S9-T2 tests: BEXT parser marker extraction ---

    #[test]
    fn test_packed_bext_has_markers() {
        let mut buf = make_bext_buffer_packed();
        // Write known marker bytes at Description[12:44].
        let config = MarkerConfig {
            bank_a: MarkerBank {
                m1: 1000,
                m2: 2000,
                m3: 3000,
                reps: [1, 2, 3, 4],
            },
            bank_b: MarkerBank {
                m1: 4000,
                m2: 5000,
                m3: 6000,
                reps: [5, 6, 7, 8],
            },
        };
        buf[12..44].copy_from_slice(&config.to_bytes());
        let fields = parse_bext_buffer(&buf);
        assert!(fields.packed);
        assert_eq!(fields.markers, Some(config));
    }

    #[test]
    fn test_unpacked_bext_no_markers() {
        let mut buf = [0u8; BEXT_STANDARD_SIZE];
        buf[0..14].copy_from_slice(b"Yamaha DX-100\0");
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed);
        assert_eq!(fields.markers, None);
    }

    #[test]
    fn test_markers_preset_shot_roundtrip_in_bext() {
        let mut buf = make_bext_buffer_packed();
        let shot = MarkerConfig::preset_shot();
        buf[12..44].copy_from_slice(&shot.to_bytes());
        let fields = parse_bext_buffer(&buf);
        assert_eq!(fields.markers, Some(shot));
    }

    #[test]
    fn test_markers_none_when_bext_version_zero() {
        let mut buf = make_bext_buffer_packed();
        // Override BWF version to 0 → packed detection fails → markers=None.
        buf[346] = 0;
        buf[347] = 0;
        let fields = parse_bext_buffer(&buf);
        assert!(!fields.packed);
        assert_eq!(fields.markers, None);
    }

    // --- Proptest ---

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_602_bytes() -> impl Strategy<Value = [u8; BEXT_STANDARD_SIZE]> {
            proptest::collection::vec(any::<u8>(), BEXT_STANDARD_SIZE).prop_map(|v| {
                let mut arr = [0u8; BEXT_STANDARD_SIZE];
                arr.copy_from_slice(&v);
                arr
            })
        }

        proptest! {
            /// MarkerBank round-trips through arbitrary values.
            #[test]
            fn proptest_marker_bank_roundtrip(
                m1 in any::<u32>(), m2 in any::<u32>(), m3 in any::<u32>(),
                r0 in 0u8..16, r1 in 0u8..16, r2 in 0u8..16, r3 in 0u8..16,
            ) {
                let bank = MarkerBank { m1, m2, m3, reps: [r0, r1, r2, r3] };
                let bytes = bank.to_bytes();
                let rt = MarkerBank::from_bytes(&bytes);
                prop_assert_eq!(rt, bank);
            }

            /// MarkerConfig round-trips through arbitrary 32-byte inputs.
            #[test]
            fn proptest_marker_config_roundtrip(
                data in proptest::collection::vec(any::<u8>(), MARKER_BLOCK_SIZE)
            ) {
                let mut arr = [0u8; MARKER_BLOCK_SIZE];
                arr.copy_from_slice(&data);
                let config = MarkerConfig::from_bytes(&arr);
                let bytes = config.to_bytes();
                let rt = MarkerConfig::from_bytes(&bytes);
                prop_assert_eq!(rt, config);
            }

            // --- Group 2: Nibble packing ---

            /// P1: Nibble round-trip via bank serialization.
            #[test]
            fn proptest_nibble_roundtrip(
                r0 in 0u8..16, r1 in 0u8..16, r2 in 0u8..16, r3 in 0u8..16,
            ) {
                let bank = MarkerBank { m1: 0, m2: 0, m3: 0, reps: [r0, r1, r2, r3] };
                let bytes = bank.to_bytes();
                let rt = MarkerBank::from_bytes(&bytes);
                prop_assert_eq!(rt.reps, [r0, r1, r2, r3]);
            }

            /// P2: High nibble byte independence — byte[12] depends only on r0,r1;
            /// byte[13] depends only on r2,r3.
            #[test]
            fn proptest_nibble_byte_independence(
                a0 in 0u8..16, a1 in 0u8..16, a2 in 0u8..16, a3 in 0u8..16,
                b2 in 0u8..16, b3 in 0u8..16,
            ) {
                let bank_a = MarkerBank { m1: 0, m2: 0, m3: 0, reps: [a0, a1, a2, a3] };
                let bank_b = MarkerBank { m1: 0, m2: 0, m3: 0, reps: [a0, a1, b2, b3] };
                let bytes_a = bank_a.to_bytes();
                let bytes_b = bank_b.to_bytes();
                // Byte 12 (first nibble byte) should be identical since r0,r1 are same.
                prop_assert_eq!(bytes_a[12], bytes_b[12]);
            }

            // --- Group 6: Preset properties ---

            /// P12: Preset shot encodes single full-file play (segments 0-2 zero-length).
            #[test]
            fn proptest_preset_shot_segments(total in 1u32..1_000_000) {
                let config = MarkerConfig::preset_shot();
                let bank = &config.bank_a;
                let segs = crate::ui::segment_bounds(bank, total);
                prop_assert_eq!(segs.len(), 4);
                // First 3 segments: zero-length (all markers at 0).
                for i in 0..3 {
                    prop_assert_eq!(segs[i].start, segs[i].end);
                }
                // Segment 4 spans full file.
                prop_assert_eq!(segs[3].start, 0);
                prop_assert_eq!(segs[3].end, total);
                prop_assert_eq!(segs[3].rep, 1);
                prop_assert!(!segs[3].reverse);
            }

            /// P13: Preset loop has 4 non-empty forward segments (total >= 4).
            #[test]
            fn proptest_preset_loop_forward(total in 4u32..1_000_000) {
                let config = MarkerConfig::preset_loop(total);
                let bank = &config.bank_a;
                let segs = crate::ui::segment_bounds(bank, total);
                prop_assert_eq!(segs.len(), 4);
                for seg in segs.iter() {
                    prop_assert!(seg.start < seg.end);
                    prop_assert!(!seg.reverse);
                    prop_assert_eq!(seg.rep, 1);
                }
            }

            /// P14: Preset loop markers are strictly increasing.
            #[test]
            fn proptest_preset_loop_increasing(total in 4u32..1_000_000) {
                let config = MarkerConfig::preset_loop(total);
                let bank = &config.bank_a;
                prop_assert!(bank.m1 > 0);
                prop_assert!(bank.m1 < bank.m2);
                prop_assert!(bank.m2 < bank.m3);
                prop_assert!(bank.m3 < total);
            }

            /// P15: Presets are synced (bank_a == bank_b).
            #[test]
            fn proptest_presets_synced(total in 1u32..1_000_000) {
                let shot = MarkerConfig::preset_shot();
                prop_assert!(shot.is_synced());
                let looper = MarkerConfig::preset_loop(total);
                prop_assert!(looper.is_synced());
            }

            /// scan_chunks never panics on arbitrary bytes after valid RIFF/WAVE.
            #[test]
            fn scan_chunks_panic_freedom(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
                let riff = make_riff_from_raw(&data);
                let mut cursor = Cursor::new(riff);
                let _ = scan_chunks(&mut cursor);
            }

            /// scan_chunks never panics on completely arbitrary bytes.
            #[test]
            fn scan_chunks_arbitrary(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
                let mut cursor = Cursor::new(data);
                let _ = scan_chunks(&mut cursor);
            }

            /// parse_bext_buffer never panics on arbitrary 602-byte inputs.
            #[test]
            fn parse_bext_panic_freedom(buf in arb_602_bytes()) {
                let _ = parse_bext_buffer(&buf);
            }

            /// Parsing same buffer twice gives identical results.
            #[test]
            fn parse_bext_idempotent(buf in arb_602_bytes()) {
                let a = parse_bext_buffer(&buf);
                let b = parse_bext_buffer(&buf);
                prop_assert_eq!(a.vendor, b.vendor);
                prop_assert_eq!(a.library, b.library);
                prop_assert_eq!(a.description, b.description);
                prop_assert_eq!(a.category, b.category);
                prop_assert_eq!(a.sound_id, b.sound_id);
                prop_assert_eq!(a.key, b.key);
                prop_assert_eq!(a.umid, b.umid);
                prop_assert_eq!(a.file_id, b.file_id);
                prop_assert_eq!(a.bext_version, b.bext_version);
                prop_assert_eq!(a.peaks_format, b.peaks_format);
            }

            /// PeaksFormat is always one of the three variants.
            #[test]
            fn peaks_format_no_panic(buf in arb_602_bytes()) {
                let fields = parse_bext_buffer(&buf);
                match fields.peaks_format {
                    PeaksFormat::RiffgrepU8 | PeaksFormat::BwfReserved | PeaksFormat::Empty => {}
                }
            }

            /// bext_version=0 with arbitrary Reserved bytes NEVER produces RiffgrepU8.
            /// This is the core safety property: third-party files (which are always
            /// v0 unless they specifically set a BWF version) cannot be misinterpreted
            /// as having riffgrep peak data.
            #[test]
            fn v0_arbitrary_reserved_never_riffgrep_u8(
                reserved in proptest::collection::vec(any::<u8>(), 180)
            ) {
                let mut buf = [0u8; BEXT_STANDARD_SIZE];
                // bext_version = 0 (default zeros at [346:348]).
                // Write arbitrary data into Reserved [422:602].
                buf[422..602].copy_from_slice(&reserved);
                // Even if bytes [8:12] happen to match the schema marker:
                buf[8..10].copy_from_slice(&0u16.to_le_bytes());
                buf[10..12].copy_from_slice(&1u16.to_le_bytes());
                let fields = parse_bext_buffer(&buf);
                prop_assert_ne!(
                    fields.peaks_format,
                    PeaksFormat::RiffgrepU8,
                    "bext_version=0 must never produce RiffgrepU8"
                );
            }

            /// bext_version=0 with arbitrary Description bytes NEVER sets packed=true.
            /// The 4-byte schema marker alone is too weak without the version guard.
            #[test]
            fn v0_arbitrary_description_never_packed(
                desc in proptest::collection::vec(any::<u8>(), 256)
            ) {
                let mut buf = [0u8; BEXT_STANDARD_SIZE];
                // bext_version = 0 (default zeros at [346:348]).
                buf[0..256].copy_from_slice(&desc);
                let fields = parse_bext_buffer(&buf);
                prop_assert!(
                    !fields.packed,
                    "bext_version=0 must never activate packed schema, \
                     even if Description bytes coincidentally match the marker"
                );
            }

            /// bext_version=0 always produces empty UMID, regardless of bytes at `[348:412]`.
            #[test]
            fn v0_arbitrary_umid_bytes_always_empty(
                umid_bytes in proptest::collection::vec(any::<u8>(), 64)
            ) {
                let mut buf = [0u8; BEXT_STANDARD_SIZE];
                // bext_version = 0.
                buf[348..412].copy_from_slice(&umid_bytes);
                let fields = parse_bext_buffer(&buf);
                prop_assert!(
                    fields.umid.is_empty(),
                    "bext_version=0 must never read UMID (got {:?})",
                    fields.umid
                );
            }
        }

        /// Build a RIFF/WAVE wrapper around raw bytes (not structured chunks).
        fn make_riff_from_raw(inner: &[u8]) -> Vec<u8> {
            let mut buf = Vec::with_capacity(12 + inner.len());
            buf.extend_from_slice(b"RIFF");
            buf.extend_from_slice(&((4 + inner.len()) as u32).to_le_bytes());
            buf.extend_from_slice(b"WAVE");
            buf.extend_from_slice(inner);
            buf
        }

        // --- S10-T1 tests: Init packed BEXT on save ---

        #[test]
        fn test_init_packed_roundtrip() {
            // Create file with unpacked BEXT (plain text Description).
            let mut bext_data = [0u8; BEXT_STANDARD_SIZE];
            bext_data[0..11].copy_from_slice(b"Hello World");
            let riff = make_riff(&[
                (b"fmt ", &[0u8; 16]),
                (b"bext", &bext_data),
                (b"data", &[0u8; 100]),
            ]);
            let path = std::env::temp_dir().join(format!(
                "riffgrep_test_init_packed_{}.wav",
                std::process::id()
            ));
            std::fs::write(&path, &riff).unwrap();

            // write_markers should fail (not packed).
            let markers = MarkerConfig::preset_loop(48000);
            assert!(matches!(
                write_markers(&path, &markers),
                Err(BextWriteError::NotPacked)
            ));

            // init_packed_and_write_markers should succeed.
            init_packed_and_write_markers(&path, &markers).unwrap();

            // Re-read: should now be packed with our markers.
            let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
            let map = scan_chunks(&mut file).unwrap();
            let fields = parse_bext_data(&mut file, &map).unwrap();
            assert!(fields.packed, "should be packed after init");
            assert_eq!(
                fields.markers.unwrap(),
                markers,
                "markers should round-trip"
            );

            // write_markers should now succeed (already packed).
            let markers2 = MarkerConfig::preset_shot();
            write_markers(&path, &markers2).unwrap();
            let mut file2 = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
            let map2 = scan_chunks(&mut file2).unwrap();
            let fields2 = parse_bext_data(&mut file2, &map2).unwrap();
            assert_eq!(fields2.markers.unwrap(), markers2);

            std::fs::remove_file(&path).unwrap();
        }

        #[test]
        fn test_init_packed_no_bext_errors() {
            let riff = make_riff(&[(b"fmt ", &[0u8; 16]), (b"data", &[0u8; 100])]);
            let path = std::env::temp_dir().join(format!(
                "riffgrep_test_init_no_bext_{}.wav",
                std::process::id()
            ));
            std::fs::write(&path, &riff).unwrap();

            let result = init_packed_and_write_markers(&path, &MarkerConfig::preset_shot());
            assert!(matches!(result, Err(BextWriteError::NoBextChunk)));

            std::fs::remove_file(&path).unwrap();
        }

        #[test]
        fn test_init_packed_preserves_originator() {
            let mut bext_data = [0u8; BEXT_STANDARD_SIZE];
            // Write vendor at Originator (bytes 256-288).
            bext_data[256..266].copy_from_slice(b"MyVendorXX");
            // Write library at OriginatorReference (bytes 288-320).
            bext_data[288..298].copy_from_slice(b"MyLibraryY");
            let riff = make_riff(&[
                (b"fmt ", &[0u8; 16]),
                (b"bext", &bext_data),
                (b"data", &[0u8; 100]),
            ]);
            let path = std::env::temp_dir().join(format!(
                "riffgrep_test_init_preserve_{}.wav",
                std::process::id()
            ));
            std::fs::write(&path, &riff).unwrap();

            init_packed_and_write_markers(&path, &MarkerConfig::preset_shot()).unwrap();

            // Originator and OriginatorReference should be preserved.
            let mut file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
            let map = scan_chunks(&mut file).unwrap();
            let fields = parse_bext_data(&mut file, &map).unwrap();
            assert_eq!(fields.vendor, "MyVendorXX");
            assert_eq!(fields.library, "MyLibraryY");

            std::fs::remove_file(&path).unwrap();
        }

        #[test]
        fn test_init_packed_preserves_file_size() {
            let bext_data = [0u8; BEXT_STANDARD_SIZE];
            let riff = make_riff(&[
                (b"fmt ", &[0u8; 16]),
                (b"bext", &bext_data),
                (b"data", &[0u8; 1000]),
            ]);
            let path = std::env::temp_dir().join(format!(
                "riffgrep_test_init_size_{}.wav",
                std::process::id()
            ));
            std::fs::write(&path, &riff).unwrap();
            let original_size = std::fs::metadata(&path).unwrap().len();

            init_packed_and_write_markers(&path, &MarkerConfig::preset_shot()).unwrap();

            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                original_size,
                "file size should not change"
            );

            std::fs::remove_file(&path).unwrap();
        }
    }
}
