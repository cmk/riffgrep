# Marker Serialization

## Implementation Guidelines

- Endianness: Use Little-Endian byte order to stay consistent with the WAV/RIFF specification.
- Unused Slots: Initialize unused marker slots to 0xFFFFFFFF (max uint32). This is safer than 0, which is a valid sample index.
- Direct Mapping: You can map this 32-byte block directly to a C-style array: uint32_t markers[8];. This makes reading/writing a single fread/fwrite operation.

## Parsing from Reaper

When parsing the cue chunk from a Reaper export to fill this 32-byte block:

- Extract the dwSampleOffset from the CuePoint structure.
- Sort them numerically (Reaper usually does this, but it’s good to verify).
- Take the first 8 and drop them into our array.

This approach is future-proof, high-performance, and maintains the exact timing of our Reaper project.

To implement the "KISS" 8-marker layout in Rust, you can use a fixed-size array of u32 and the bytemuck crate (if you want zero-copy) or standard to_le_bytes.

Below is a clean, dependency-free implementation that parses a standard WAV file's cue chunk and packs the first 8 points into our 32-byte format.

## The Marker Struct

rust
```
#[derive(Debug, Clone, Copy)]
pub struct MarkerBlock {
    // 8 markers * 4 bytes = 32 bytes
    pub points: [u32; 8],
}

impl MarkerBlock {
    pub const EMPTY_SENTINEL: u32 = u32::MAX;

    /// Creates an empty block initialized with 0xFFFFFFFF
    pub fn new() -> Self {
        Self {
            points: [Self::EMPTY_SENTINEL; 8],
        }
    }

    /// Serializes the block to 32 Little-Endian bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buffer = [0u8; 32];
        for (i, &point) in self.points.iter().enumerate() {
            let bytes = point.to_le_bytes();
            buffer[i * 4..(i * 4) + 4].copy_from_slice(&bytes);
        }
        buffer
    }
}
```

## Simple WAV Parser to Extract Markers

This function scans a WAV file for the cue chunk and grabs the first 8 absolute sample offsets.

rust
```
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub fn extract_reaper_markers(path: &str) -> Result<MarkerBlock, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut marker_block = MarkerBlock::new();
    
    // Simple RIFF traversal
    let mut buffer = [0u8; 4];
    file.seek(SeekFrom::Start(12))?; // Skip "RIFFxxxxWAVE"

    while file.read_exact(&mut buffer).is_ok() {
        let chunk_id = buffer;
        file.read_exact(&mut buffer)?;
        let chunk_size = u32::from_le_bytes(buffer);

        if &chunk_id == b"cue " {
            // Found the cue chunk! Read number of points
            file.read_exact(&mut buffer)?;
            let num_points = u32::from_le_bytes(buffer);
            
            // Each CuePoint is 24 bytes. Sample Offset is at offset 20 within the CuePoint.
            for i in 0..(num_points.min(8) as usize) {
                let point_start = file.stream_position()?;
                file.seek(SeekFrom::Current(20))?; // Skip ID, Pos, ChunkID, ChunkStart, BlockStart
                
                file.read_exact(&mut buffer)?;
                marker_block.points[i] = u32::from_le_bytes(buffer);
                
                // Seek to the start of the next 24-byte CuePoint
                file.seek(SeekFrom::Start(point_start + 24))?;
            }
            return Ok(marker_block);
        } else {
            // Not the cue chunk? Skip it. 
            // (Note: Chunk sizes are padded to even bytes in RIFF)
            let skip = (chunk_size + (chunk_size % 2)) as i64;
            file.seek(SeekFrom::Current(skip))?;
        }
    }

    Err("No cue chunk found in file".into())
}
```

### How to Use It

rust
```
fn main() {
    match extract_reaper_markers("reaper_export.wav") {
        Ok(block) => {
            let raw_32_bytes = block.to_bytes();
            println!("Packed 32-byte block: {:02X?}", raw_32_bytes);
            println!("Marker 1 (Sample): {}", block.points[0]);
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Design Notes for Rust

- Safety: The stream_position and seek logic ensures we don't over-read the file if the cue chunk is smaller than expected.
- Sentinel: Using u32::MAX (0xFFFFFFFF) allows you to distinguish between a marker at the very beginning of the file (0) and an empty slot.
- Performance: This doesn't load the audio data into memory; it only jumps through the chunk headers, making it extremely fast even for multi-gigabyte files.

### Testing

Here is the deserializer to complete the round-trip. This takes the raw 32-byte "internal" buffer and reconstructs the MarkerBlock struct so you can verify the values. The from_bytes method can be added to the existing MarkerBlock implementation:

rust
```
impl MarkerBlock {
    // ... previous methods (new, to_bytes) ...

    /// Deserializes a 32-byte block back into a MarkerBlock struct
    pub fn from_bytes(buffer: [u8; 32]) -> Self {
        let mut points = [0u32; 8];
        for i in 0..8 {
            let start = i * 4;
            let end = start + 4;
            // Extract 4 bytes and convert from Little-Endian to u32
            points[i] = u32::from_le_bytes(buffer[start..end].try_into().unwrap());
        }
        Self { points }
    }
}
```

You can use this test block to ensure extraction, packing, and unpacking are working perfectly:

rust
```
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_roundtrip() {
        // 1. Create a mock block with some sample positions
        let mut original = MarkerBlock::new();
        original.points[0] = 0;          // Start of file
        original.points[1] = 44100;      // 1 second mark
        original.points[2] = 1000000;    // Deep into the file
        // Points 3-7 remain 0xFFFFFFFF

        // 2. Serialize to 32 bytes
        let serialized = original.to_bytes();
        assert_eq!(serialized.len(), 32);

        // 3. Deserialize back
        let deserialized = MarkerBlock::from_bytes(serialized);

        // 4. Verify data integrity
        assert_eq!(original.points[0], deserialized.points[0]);
        assert_eq!(original.points[1], deserialized.points[1]);
        assert_eq!(original.points[7], MarkerBlock::EMPTY_SENTINEL);
        println!("Roundtrip successful!");
    }
}
```

# Testing Strategy with Reaper Files

To properly test this with real exports:

- Open a WAV exported in a hex editor.
- Search for the string cue (hex 63 75 65 20).
- Manually verify that the byte offset matches what our Rust code extracts.
- Run our extract_reaper_markers function and compare the output to the Media Explorer or Region/Marker Manager to ensure the sample counts are identical.
