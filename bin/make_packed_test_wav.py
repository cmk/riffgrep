#!/usr/bin/env python3
"""Generate a WAV file with riffgrep packed BEXT schema for testing markers.

Creates a 16-bit 48kHz mono 1-second sine wave with:
- Packed Description header (version_major=1, version_minor=2)
- file_id = hardcoded non-zero UUID-v7-like value (big-endian at [0:8])
- BEXT version=2 (EBU Tech 3285)
- preset_loop markers at quarter points
- Comment "test marker file"

Usage: python3 bin/make_packed_test_wav.py [output_path]
"""

import struct
import math
import sys

SAMPLE_RATE = 48000
DURATION_SECS = 1.0
CHANNELS = 1
BIT_DEPTH = 16
BEXT_STANDARD_SIZE = 602
MARKER_BLOCK_SIZE = 32
MARKER_EMPTY = 0xFFFFFFFF


def make_sine_wave():
    """Generate 1 second of 440Hz sine wave as 16-bit PCM."""
    n_samples = int(SAMPLE_RATE * DURATION_SECS)
    samples = bytearray()
    for i in range(n_samples):
        t = i / SAMPLE_RATE
        val = int(32767 * 0.5 * math.sin(2 * math.pi * 440 * t))
        samples.extend(struct.pack("<h", val))
    return bytes(samples)


def make_bext_data():
    """Build a 602-byte BEXT data block with packed schema."""
    buf = bytearray(BEXT_STANDARD_SIZE)

    # Description[0:8] - file_id (high 64 bits of UUID v7, big-endian).
    # Using a hardcoded non-zero fixture value; real files get a UUID v7 from
    # init_packed_and_write_markers() at first pack time.
    # Value: 0x01937BEEFCAFE701 — plausible early-2026 ms timestamp + v7 nibble.
    struct.pack_into(">Q", buf, 0, 0x01937BEEFCAFE701)

    # Description[8:10] - version_major = 1 (signals binary UUID content)
    struct.pack_into("<H", buf, 8, 1)

    # Description[10:12] - version_minor = 2
    struct.pack_into("<H", buf, 10, 2)

    # Description[12:44] - MARKERSv2 block: preset_loop(48000)
    total = int(SAMPLE_RATE * DURATION_SECS)
    q1 = total // 4      # 12000
    q2 = total // 2      # 24000
    q3 = (3 * total) // 4  # 36000

    # Bank A: [m1:4][m2:4][m3:4][nibbles:2][pad:2]
    bank_a = struct.pack("<III", q1, q2, q3)
    # reps = [1, 1, 1, 1] -> nibbles: byte0 = (1<<4)|1 = 0x11, byte1 = (1<<4)|1 = 0x11
    bank_a += bytes([0x11, 0x11, 0x00, 0x00])

    # Bank B: same as Bank A
    bank_b = bank_a

    buf[12:44] = bank_a + bank_b

    # Description[44:76] - Comment (32 ASCII, null-padded)
    comment = b"test marker file"
    buf[44:44 + len(comment)] = comment

    # Description[76:80] - Rating
    buf[76:80] = b"****"

    # Description[80:84] - BPM = "120 "
    buf[80:84] = b"120\x00"

    # Description[88:92] - Category
    buf[88:92] = b"TEST"

    # Description[96:100] - Sound ID
    buf[96:100] = b"MRK1"

    # Originator (bytes 256-288) - vendor
    vendor = b"TestVendor"
    buf[256:256 + len(vendor)] = vendor

    # OriginatorReference (bytes 288-320) - library
    library = b"TestLibrary"
    buf[288:288 + len(library)] = library

    # OriginationDate (bytes 320-330)
    buf[320:330] = b"2026-02-16"

    # BWF version (bytes 346-347) = 2 (EBU Tech 3285; required for packed detection)
    struct.pack_into("<H", buf, 346, 2)

    return bytes(buf)


def make_wav(output_path):
    """Build a complete RIFF/WAVE with fmt, bext, and data chunks."""
    # fmt chunk: 16-bit PCM mono 48kHz
    byte_rate = SAMPLE_RATE * CHANNELS * (BIT_DEPTH // 8)
    block_align = CHANNELS * (BIT_DEPTH // 8)
    fmt_data = struct.pack(
        "<HHIIHH",
        1,            # audio_format = PCM
        CHANNELS,
        SAMPLE_RATE,
        byte_rate,
        block_align,
        BIT_DEPTH,
    )

    bext_data = make_bext_data()
    audio_data = make_sine_wave()

    # Build RIFF file
    chunks = bytearray()

    # fmt chunk
    chunks.extend(b"fmt ")
    chunks.extend(struct.pack("<I", len(fmt_data)))
    chunks.extend(fmt_data)

    # bext chunk
    chunks.extend(b"bext")
    chunks.extend(struct.pack("<I", len(bext_data)))
    chunks.extend(bext_data)

    # data chunk
    chunks.extend(b"data")
    chunks.extend(struct.pack("<I", len(audio_data)))
    chunks.extend(audio_data)

    # RIFF header
    riff = bytearray()
    riff.extend(b"RIFF")
    riff.extend(struct.pack("<I", 4 + len(chunks)))
    riff.extend(b"WAVE")
    riff.extend(chunks)

    with open(output_path, "wb") as f:
        f.write(riff)

    print(f"Created {output_path}")
    print(f"  Format: {SAMPLE_RATE}Hz {BIT_DEPTH}-bit mono, {DURATION_SECS}s")
    print(f"  BEXT: packed schema v1.2, BWF version 2, file_id=0x01937BEEFCAFE701")
    print(f"  Markers: preset_loop (quarter points at 12000/24000/36000)")
    print(f"  Comment: 'test marker file', Category: TEST, Sound ID: MRK1")


if __name__ == "__main__":
    output = sys.argv[1] if len(sys.argv) > 1 else "test_files/packed_markers.wav"
    make_wav(output)
