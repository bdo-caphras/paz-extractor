//! Parser for `pad00000.meta`, the top-level archive manifest.
//!
//! Layout (all little-endian):
//! ```text
//! [u32 version][u32 paz_count]
//! repeated paz_count times:
//!   [u32 paz_id][u32 crc][u32 size]
//! ```
//! `crc` equals the first 4 bytes of `PAD<paz_id>.PAZ`; `size` is its file size.
//! The meta file is *not* encrypted.

use std::path::Path;

use crate::error::{io, PazError};

/// One entry of the manifest, describing a single `.PAZ` file.
#[derive(Debug, Clone, Copy)]
pub struct PazEntry {
    /// Numeric id, i.e. `N` in `PAD0000N.PAZ`.
    pub paz_id: u32,
    /// First 4 bytes / CRC of the PAZ file (used as a sanity check).
    pub crc: u32,
    /// Declared size of the PAZ file in bytes.
    pub size: u32,
}

/// The parsed manifest.
#[derive(Debug, Clone)]
pub struct Meta {
    /// Format version word.
    pub version: u32,
    /// All PAZ files referenced by the manifest.
    pub entries: Vec<PazEntry>,
}

impl Meta {
    /// Parse a `pad00000.meta` file from disk.
    pub fn load(path: &Path) -> Result<Self, PazError> {
        let bytes = std::fs::read(path).map_err(|e| io(path, e))?;
        Self::parse(&bytes)
    }

    /// Parse meta bytes already in memory.
    pub fn parse(bytes: &[u8]) -> Result<Self, PazError> {
        if bytes.len() < 8 {
            return Err(PazError::Malformed("meta shorter than header"));
        }
        let version = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;

        let need = 8 + count * 12;
        if bytes.len() < need {
            return Err(PazError::Malformed(
                "meta shorter than declared record count",
            ));
        }

        let mut entries = Vec::with_capacity(count);
        let mut off = 8;
        for _ in 0..count {
            let paz_id = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
            let crc = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
            let size = u32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap());
            entries.push(PazEntry { paz_id, crc, size });
            off += 12;
        }

        Ok(Meta { version, entries })
    }
}
