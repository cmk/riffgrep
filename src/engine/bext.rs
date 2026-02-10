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
/// For metadata chunks (bext, LIST-INFO), scanning stops at [`SCAN_LIMIT`] (4KB).
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
    /// Our packed schema (Description v0.1): Reserved holds 180 u8 amplitude peaks.
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
    pub bext_version: u16,
    /// Detected format of the Reserved field.
    pub peaks_format: PeaksFormat,

    // --- Packed Description fields (when schema detected) ---
    /// Whether the packed schema was detected in the Description field.
    pub packed: bool,
    /// `[000:008]` SM recid (u64 LE).
    pub recid: u64,
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

    // Detect packed schema: requires BOTH the schema version marker at bytes
    // [008:012] (major=0, minor=1) AND bext_version >= 1 per PICKER_SCHEMA.md.
    // The schema marker alone ([0x00, 0x00, 0x01, 0x00]) is too weak — bytes
    // 8-11 of the Description could match by coincidence in third-party files
    // (the adjacent marker block at [012:044] is sample offsets, not a hash).
    let version_major = u16::from_le_bytes([buf[8], buf[9]]);
    let version_minor = u16::from_le_bytes([buf[10], buf[11]]);
    let is_packed = version_major == 0 && version_minor == 1 && bext_version >= 1;

    // Determine peaks format.
    // RiffgrepU8 requires BOTH the packed schema marker AND bext_version >= 1.
    // This prevents misinterpreting arbitrary BEXT Reserved bytes from third-party
    // DAWs (Reaper, SoundMiner, etc.) as waveform peak data.
    let peaks_format = if all_zeros {
        PeaksFormat::Empty
    } else if is_packed && bext_version >= 1 {
        PeaksFormat::RiffgrepU8
    } else {
        // Non-zero data but not our packed schema, or packed without the required
        // BWF version — treat as opaque BWF Reserved data (not valid peaks).
        PeaksFormat::BwfReserved
    };

    if is_packed {
        // Packed Description format per PICKER_SCHEMA.md.
        let comment = decode_fixed_ascii(&buf[44..76]);
        let description = comment.clone();
        BextFields {
            vendor,
            library,
            date,
            umid,
            peaks,
            bext_version,
            peaks_format,
            packed: true,
            recid: u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Build a minimal RIFF/WAVE header + chunks for testing.
    fn make_riff(chunks: &[(& [u8; 4], &[u8])]) -> Vec<u8> {
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
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        // fmt is at offset 12, size 16+8=24 -> bext starts at 12+24=36, data at 36+8=44
        assert_eq!(map.bext_offset, Some(44));
        assert_eq!(map.bext_size, 604);
    }

    #[test]
    fn scan_no_bext() {
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"data", &[0u8; 100]),
        ]);
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
        // recid [000:008] = 985188
        buf[0..8].copy_from_slice(&985188u64.to_le_bytes());
        // version [008:012] = 0.1 (packed schema marker)
        buf[8..10].copy_from_slice(&0u16.to_le_bytes());
        buf[10..12].copy_from_slice(&1u16.to_le_bytes());
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
        // BWF Version [346:348] = 1 (required for RiffgrepU8 peak detection).
        buf[346..348].copy_from_slice(&1u16.to_le_bytes());
        // UMID [348:412] = "976132720e774b668c36826386ae6505" as ASCII
        buf[348..380].copy_from_slice(b"976132720e774b668c36826386ae6505");
        buf
    }

    #[test]
    fn parse_packed_bext_all_fields() {
        let buf = make_bext_buffer_packed();
        let fields = parse_bext_buffer(&buf);
        assert!(fields.packed);
        assert_eq!(fields.recid, 985188);
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
        assert_eq!(fields.recid, 0);
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
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &buf),
        ]);
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
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        assert_eq!(map.bext_size, 100);
        let result = parse_bext_data(&mut cursor, &map);
        assert!(result.is_err());
    }

    #[test]
    fn bext_chunk_larger_than_602() {
        let bext_data = vec![0u8; 800]; // CodingHistory appended
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"bext", &bext_data),
        ]);
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
        assert_eq!(fields.bext_version, 1, "packed helper should set bext_version=1");
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
        assert!(!fields.packed, "bext_version=0 should not activate packed schema");
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
        // Simulate a riffgrep-written file: packed schema + bext_version=1.
        // All version-dependent fields should be present.
        let mut buf = make_bext_buffer_packed();
        // Set non-zero peaks.
        for i in 422..602 {
            buf[i] = ((i * 7) % 256) as u8;
        }
        let fields = parse_bext_buffer(&buf);
        // Version-dependent features all active:
        assert!(fields.packed, "packed schema should be detected");
        assert_eq!(fields.bext_version, 1);
        assert_eq!(fields.peaks_format, PeaksFormat::RiffgrepU8);
        assert!(!fields.umid.is_empty(), "UMID should be read for v1");
        assert_eq!(fields.recid, 985188);
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
                prop_assert_eq!(a.recid, b.recid);
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

            /// bext_version=0 always produces empty UMID, regardless of bytes at [348:412].
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
    }
}
