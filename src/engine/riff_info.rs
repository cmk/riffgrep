//! RIFF LIST-INFO chunk parser. Extracts IART, INAM, IGNR, IKEY, ICMT.
//!
//! Structure: `LIST` (4) + size (4) + `INFO` (4) + sequence of subchunks.
//! Each subchunk: id (4) + size (4) + null-terminated text + optional WORD padding.

use std::io::{self, Read, Seek, SeekFrom};

use crate::engine::bext::{ChunkMap, RiffError};

/// Fields extracted from RIFF INFO subchunks.
#[derive(Debug, Clone, Default)]
pub struct InfoFields {
    /// IART — Artist/Vendor.
    pub vendor: String,
    /// INAM — Name/Library.
    pub library: String,
    /// IGNR — Genre/Category.
    pub category: String,
    /// IKEY — Keywords/Sound ID.
    pub sound_id: String,
    /// ICMT — Comment/Description.
    pub description: String,
}

/// Parse the LIST-INFO chunk from a reader, given offsets from [`ChunkMap`].
///
/// The `info_offset` in the ChunkMap points to the start of the LIST data
/// (right after "LIST" + size), which begins with "INFO".
pub fn parse_riff_info<R: Read + Seek>(
    reader: &mut R,
    map: &ChunkMap,
) -> Result<InfoFields, RiffError> {
    let offset = match map.info_offset {
        Some(o) => o,
        None => return Ok(InfoFields::default()),
    };

    // Skip the "INFO" fourcc (4 bytes) — the scanner already verified it.
    let subchunk_start = offset + 4;
    // Total data size includes "INFO" (4 bytes), so subchunk data = info_size - 4.
    let subchunk_data_len = map.info_size.saturating_sub(4) as u64;
    let end = subchunk_start + subchunk_data_len;

    reader.seek(SeekFrom::Start(subchunk_start))?;

    let mut fields = InfoFields::default();
    let mut pos = subchunk_start;

    while pos + 8 <= end {
        reader.seek(SeekFrom::Start(pos))?;

        let mut header = [0u8; 8];
        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RiffError::Io(e)),
        }

        let subchunk_id = [header[0], header[1], header[2], header[3]];
        let subchunk_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

        let data_start = pos + 8;

        // Only read subchunks we care about, and only up to a reasonable size.
        let value = if subchunk_size > 0 && subchunk_size <= 4096 {
            let mut buf = vec![0u8; subchunk_size as usize];
            match reader.read_exact(&mut buf) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(RiffError::Io(e)),
            }
            decode_info_text(&buf)
        } else {
            String::new()
        };

        match &subchunk_id {
            b"IART" => fields.vendor = value,
            b"INAM" => fields.library = value,
            b"IGNR" => fields.category = value,
            b"IKEY" => fields.sound_id = value,
            b"ICMT" => fields.description = value,
            _ => {} // Skip unknown subchunks.
        }

        // Advance: WORD-align the subchunk data.
        let padded = (subchunk_size as u64 + 1) & !1;
        pos = data_start + padded;
    }

    Ok(fields)
}

/// Decode null-terminated INFO text, trimming trailing nulls and whitespace.
fn decode_info_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.trim_end_matches('\0').trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::engine::bext::scan_chunks;

    /// Helper: build a LIST-INFO blob from subchunks.
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

    /// Helper: build a full RIFF file.
    fn make_riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut chunk_bytes = Vec::new();
        for (id, payload) in chunks {
            chunk_bytes.extend_from_slice(*id);
            chunk_bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            chunk_bytes.extend_from_slice(payload);
            if payload.len() % 2 != 0 {
                chunk_bytes.push(0);
            }
        }
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&(4 + chunk_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(&chunk_bytes);
        data
    }

    #[test]
    fn parse_known_info_values() {
        let info = make_list_info(&[
            (b"IART", b"TestVendor\0"),
            (b"INAM", b"TestLibrary\0"),
            (b"IGNR", b"TestCategory\0"),
            (b"IKEY", b"TestSoundID\0"),
            (b"ICMT", b"TestDescription\0"),
        ]);
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        let fields = parse_riff_info(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "TestVendor");
        assert_eq!(fields.library, "TestLibrary");
        assert_eq!(fields.category, "TestCategory");
        assert_eq!(fields.sound_id, "TestSoundID");
        assert_eq!(fields.description, "TestDescription");
    }

    #[test]
    fn parse_odd_length_subchunk() {
        // "ABC" is 3 bytes (odd) — should be padded to 4 in the chunk.
        let info = make_list_info(&[
            (b"IART", b"ABC"),    // odd length, no null terminator
            (b"INAM", b"Next\0"), // next subchunk should still parse
        ]);
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        let fields = parse_riff_info(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "ABC");
        assert_eq!(fields.library, "Next");
    }

    #[test]
    fn skip_unknown_subchunks() {
        let info = make_list_info(&[
            (b"ISFT", b"SomeEditor\0"),
            (b"ICRD", b"2024-01-15\0"),
            (b"IART", b"TheVendor\0"),
        ]);
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        let fields = parse_riff_info(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "TheVendor");
        // Unknown subchunks should not affect our fields.
        assert_eq!(fields.library, "");
    }

    #[test]
    fn empty_list_info() {
        // LIST-INFO with just "INFO" and no subchunks (size=4).
        let info = b"INFO".to_vec();
        let riff = make_riff(&[
            (b"fmt ", &[0u8; 16]),
            (b"LIST", &info),
        ]);
        let mut cursor = Cursor::new(riff);
        let map = scan_chunks(&mut cursor).unwrap();
        let fields = parse_riff_info(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "");
        assert_eq!(fields.library, "");
        assert_eq!(fields.category, "");
    }

    #[test]
    fn no_info_chunk() {
        let map = ChunkMap::default();
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let fields = parse_riff_info(&mut cursor, &map).unwrap();
        assert_eq!(fields.vendor, "");
    }

    #[test]
    fn integration_all_riff_info_tags() {
        let path = "test_files/all_riff_info_tags_with_numbers.wav";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let mut file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
        let map = scan_chunks(&mut file).unwrap();
        let fields = parse_riff_info(&mut file, &map).unwrap();
        assert_eq!(fields.vendor, "IART-Artist");
        assert_eq!(fields.library, "INAM-Name/Title");
        assert_eq!(fields.category, "IGNR-Genre");
        assert_eq!(fields.sound_id, "IKEY-Keywords");
        assert_eq!(fields.description, "ICMT-Comment");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// parse_riff_info never panics on arbitrary bytes after valid INFO header.
            #[test]
            fn parse_info_panic_freedom(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
                let mut info = Vec::new();
                info.extend_from_slice(b"INFO");
                info.extend_from_slice(&data);

                let riff = make_riff_with_list(&info);
                let mut cursor = Cursor::new(riff);
                if let Ok(map) = scan_chunks(&mut cursor) {
                    let _ = parse_riff_info(&mut cursor, &map);
                }
            }
        }

        fn make_riff_with_list(list_data: &[u8]) -> Vec<u8> {
            let mut chunks = Vec::new();
            chunks.extend_from_slice(b"fmt ");
            chunks.extend_from_slice(&16u32.to_le_bytes());
            chunks.extend_from_slice(&[0u8; 16]);
            chunks.extend_from_slice(b"LIST");
            chunks.extend_from_slice(&(list_data.len() as u32).to_le_bytes());
            chunks.extend_from_slice(list_data);
            if list_data.len() % 2 != 0 {
                chunks.push(0);
            }
            let mut buf = Vec::with_capacity(12 + chunks.len());
            buf.extend_from_slice(b"RIFF");
            buf.extend_from_slice(&((4 + chunks.len()) as u32).to_le_bytes());
            buf.extend_from_slice(b"WAVE");
            buf.extend_from_slice(&chunks);
            buf
        }
    }
}
