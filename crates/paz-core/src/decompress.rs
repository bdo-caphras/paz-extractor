//! Black Desert custom LZ77-style decompressor.
//!
//! Pearl Abyss does not use zlib for in-archive payloads; they use a bespoke
//! LZ77 variant. The control-bit / match-encoding scheme below was derived from
//! the public quickbms `blackdesert` script and cross-checked against the
//! observed archive bytes. This is an independent Rust reimplementation.
//!
//! ## Payload header
//! Byte 0 is a flag:
//! * bit 0 (`0x01`): payload is compressed (otherwise it is stored raw).
//! * bit 1 (`0x02`): "long" header — sizes are 32-bit instead of 8-bit.
//!
//! Short header (3 bytes): `[flags][compressed_len: u8][decompressed_len: u8]`.
//! Long  header (9 bytes): `[flags][compressed_len: u32][decompressed_len: u32]`.

use crate::error::PazError;

/// Number of literal bytes consumed per control nibble value.
const DW_TABLE: [usize; 16] = [4, 0, 1, 0, 2, 0, 1, 0, 3, 0, 1, 0, 2, 0, 1, 0];

#[inline]
fn rd_u32(buf: &[u8], pos: usize) -> Result<u32, PazError> {
    buf.get(pos..pos + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(PazError::Decompress("read past end of compressed input"))
}

/// Decompress one payload. Returns the decompressed bytes (length is taken from
/// the header and validated against `expected_original` when non-zero).
pub fn decompress(input: &[u8], expected_original: u32) -> Result<Vec<u8>, PazError> {
    if input.is_empty() {
        return Err(PazError::Decompress("empty payload"));
    }
    let flags = input[0];
    let long = flags & 0x02 != 0;

    // Parse declared decompressed length.
    let declared_len = if long {
        rd_u32(input, 5)? as usize
    } else {
        *input
            .get(2)
            .ok_or(PazError::Decompress("short header truncated"))? as usize
    };

    let out_len = if expected_original != 0 {
        expected_original as usize
    } else {
        declared_len
    };

    // Stored (uncompressed) payload.
    if flags & 0x01 == 0 {
        let data_off = if long { 9 } else { 3 };
        let end = data_off + out_len;
        return input
            .get(data_off..end)
            .map(|s| s.to_vec())
            .ok_or(PazError::Decompress(
                "stored payload shorter than declared length",
            ));
    }

    unpack_core(input, out_len)
}

/// Core LZ77 expander. `out_len` is the target decompressed size.
fn unpack_core(input: &[u8], out_len: usize) -> Result<Vec<u8>, PazError> {
    let mut out = vec![0u8; out_len];

    // Compressed-stream length + start of the data section.
    let (compressed_len, mut in_pos) = if input[0] & 0x02 != 0 {
        (rd_u32(input, 1)? as usize, 9usize)
    } else {
        (
            *input
                .get(1)
                .ok_or(PazError::Decompress("short header truncated"))? as usize,
            3usize,
        )
    };

    // `last_in` is the index of the last valid input byte (inclusive).
    let last_in = compressed_len
        .checked_sub(1)
        .ok_or(PazError::Decompress("zero compressed length"))?;
    if out_len == 0 {
        return Ok(out);
    }
    let last_out = out_len - 1; // index of last writable output byte

    let mut out_pos: usize = 0;
    let mut group_header: u32 = 1;

    loop {
        // Refill the 32-bit control group when exhausted (sentinel == 1).
        if group_header == 1 {
            if in_pos + 3 > last_in {
                return Err(PazError::Decompress("control group past end"));
            }
            group_header = rd_u32(input, in_pos)?;
            in_pos += 4;
        }

        if in_pos + 3 > last_in {
            return Err(PazError::Decompress("block header past end"));
        }
        let block_header = rd_u32(input, in_pos)?;

        // A 0 control bit means "emit literals".
        if group_header & 1 == 0 {
            let valid = DW_TABLE[(group_header & 0xF) as usize];
            if out_pos + 4 > out.len() {
                // Near the tail: fall through to the byte-wise tail copy.
                break;
            }
            out[out_pos..out_pos + 4].copy_from_slice(&block_header.to_le_bytes());
            group_header >>= valid;
            out_pos += valid;
            in_pos += valid;

            if out_pos >= last_out.saturating_sub(10) {
                break;
            }
            continue;
        }

        // Otherwise decode a back-reference (match).
        let (repeat, length, advance) = match block_header & 0x03 {
            0x03 => {
                if block_header & 0x7F == 3 {
                    (
                        (block_header >> 15) as usize,
                        (((block_header >> 7) & 0xFF) as usize) + 3,
                        4usize,
                    )
                } else {
                    (
                        ((block_header >> 7) & 0x1FFFF) as usize,
                        (((block_header >> 2) & 0x1F) as usize) + 2,
                        3usize,
                    )
                }
            }
            0x02 => (
                ((block_header as u16) >> 6) as usize,
                (((block_header >> 2) & 0xF) as usize) + 3,
                2usize,
            ),
            0x01 => (((block_header as u16) >> 2) as usize, 3usize, 2usize),
            _ => (((block_header as u8) >> 2) as usize, 3usize, 1usize),
        };
        in_pos += advance;

        // Validate the back-reference against output bounds.
        if repeat < 3 || repeat > out_pos || out_pos + length + 3 > out.len() {
            return Err(PazError::Decompress("invalid back-reference"));
        }

        // Copy `length` bytes from `out_pos - repeat`. The original copies in
        // 3-byte (u32) chunks; do an overlap-safe byte copy here.
        let src_start = out_pos - repeat;
        let mut copied = 0usize;
        while copied < length {
            out[out_pos + copied] = out[src_start + copied];
            copied += 1;
        }

        group_header >>= 1;
        out_pos += length;

        if out_pos >= last_out.saturating_sub(10) {
            break;
        }
    }

    // Tail: copy the remaining literal bytes one at a time.
    if out_pos <= last_out {
        let end_in = last_in + 1;
        loop {
            if group_header == 1 {
                in_pos += 4;
                group_header = 0x8000_0000;
            }
            if in_pos >= end_in {
                break;
            }
            if out_pos >= out.len() {
                break;
            }
            out[out_pos] = input[in_pos];
            out_pos += 1;
            in_pos += 1;
            group_header >>= 1;
            if out_pos > last_out {
                break;
            }
        }
    }

    Ok(out)
}
