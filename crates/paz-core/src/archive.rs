//! Parser and extractor for individual `.PAZ` archive files.
//!
//! Layout (all little-endian):
//! ```text
//! [u32 crc][u32 file_count][u32 path_block_len]
//! file_count * 24 bytes of file records:
//!   [u32 crc][u32 folder_id][u32 file_id][u32 offset][u32 comp_size][u32 orig_size]
//! path_block_len bytes: ICE-encrypted, NUL-separated string table.
//!   folder_id / file_id index into this table; full path = table[folder] + table[file].
//! ```
//! Each file's bytes live at `offset` (absolute, from start of the PAZ) for
//! `comp_size` bytes. They are ICE-encrypted unless the path ends in `.dbss`,
//! and compressed iff `orig_size > comp_size` or the first byte is `0x6E`.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::decompress::decompress;
use crate::error::{io, PazError};
use crate::ice::IceKey;

/// The ICE key Pearl Abyss uses for the global (NA/EU/KR) live client.
/// Bytes: `51 F3 0F 11 04 24 6A 00`.
pub const PAZ_ICE_KEY: [u8; 8] = [0x51, 0xF3, 0x0F, 0x11, 0x04, 0x24, 0x6A, 0x00];

/// One logical file inside a PAZ archive.
#[derive(Debug, Clone)]
pub struct FileRecord {
    /// Per-file checksum (unused for extraction, kept for diagnostics).
    pub crc: u32,
    /// Absolute byte offset of the payload within the PAZ file.
    pub offset: u32,
    /// On-disk (possibly compressed/encrypted) size.
    pub comp_size: u32,
    /// Decompressed size.
    pub orig_size: u32,
    /// Virtual path, e.g. `binary/language/languagedata_en.loc`.
    pub path: String,
}

impl FileRecord {
    /// True if the payload is stored compressed.
    pub fn is_compressed(&self) -> bool {
        self.orig_size > self.comp_size
    }

    /// True if the payload is ICE-encrypted (everything except `.dbss`).
    pub fn is_encrypted(&self) -> bool {
        !self.path.to_ascii_lowercase().ends_with(".dbss")
    }
}

/// A parsed PAZ archive: its path plus the file index.
#[derive(Debug, Clone)]
pub struct PazArchive {
    /// Path to the `.PAZ` file on disk.
    pub path: PathBuf,
    /// Header CRC (first 4 bytes).
    pub crc: u32,
    /// All files indexed by this archive.
    pub files: Vec<FileRecord>,
}

impl PazArchive {
    /// Open a `.PAZ` file and parse its index (header + records + filename table).
    ///
    /// This reads **only** the header, file index, and filename table from the
    /// front of the file — not the (potentially multi-megabyte) payload region —
    /// so building a global index over tens of GiB of archives stays fast.
    pub fn open(path: &Path, ice: &IceKey) -> Result<Self, PazError> {
        let mut f = std::fs::File::open(path).map_err(|e| io(path, e))?;

        // Read the 12-byte header to learn the index + path-block sizes.
        let mut header = [0u8; 12];
        f.read_exact(&mut header).map_err(|e| io(path, e))?;
        let crc = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let file_count = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let path_block_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;

        // Read exactly the index + filename table that follow the header.
        let front_len = file_count * 24 + path_block_len;
        let mut front = vec![0u8; front_len];
        f.read_exact(&mut front).map_err(|e| io(path, e))?;

        Self::parse_index(
            path.to_path_buf(),
            crc,
            file_count,
            path_block_len,
            &front,
            ice,
        )
    }

    /// Parse an archive whose bytes are already fully in memory (header included).
    pub fn parse(path: PathBuf, bytes: Vec<u8>, ice: &IceKey) -> Result<Self, PazError> {
        if bytes.len() < 12 {
            return Err(PazError::Malformed("paz shorter than header"));
        }
        let crc = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let file_count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let path_block_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let front_end = 12 + file_count * 24 + path_block_len;
        if bytes.len() < front_end {
            return Err(PazError::Malformed("paz shorter than index + path block"));
        }
        Self::parse_index(
            path,
            crc,
            file_count,
            path_block_len,
            &bytes[12..front_end],
            ice,
        )
    }

    /// Parse the index + filename table from `front` (the bytes *after* the
    /// 12-byte header): `file_count * 24` record bytes followed by the
    /// ICE-encrypted, `path_block_len`-byte filename table.
    fn parse_index(
        path: PathBuf,
        crc: u32,
        file_count: usize,
        path_block_len: usize,
        front: &[u8],
        ice: &IceKey,
    ) -> Result<Self, PazError> {
        let index_end = file_count * 24;
        let path_end = index_end + path_block_len;
        if front.len() < path_end {
            return Err(PazError::Malformed("paz shorter than index + path block"));
        }

        // Read raw file records (paths filled in below).
        struct Raw {
            crc: u32,
            folder_id: u32,
            file_id: u32,
            offset: u32,
            comp_size: u32,
            orig_size: u32,
        }
        let mut raws = Vec::with_capacity(file_count);
        let mut off = 0usize;
        for _ in 0..file_count {
            let rd = |o: usize| u32::from_le_bytes(front[off + o..off + o + 4].try_into().unwrap());
            raws.push(Raw {
                crc: rd(0),
                folder_id: rd(4),
                file_id: rd(8),
                offset: rd(12),
                comp_size: rd(16),
                orig_size: rd(20),
            });
            off += 24;
        }

        // Decrypt the filename table. The block length is always a multiple of 8.
        let mut path_block = front[index_end..path_end].to_vec();
        if !path_block.len().is_multiple_of(8) {
            return Err(PazError::Malformed("path block not a multiple of 8"));
        }
        ice.decrypt(&mut path_block);

        // Split into NUL-terminated strings.
        let strings = split_nul_strings(&path_block)?;

        let mut files = Vec::with_capacity(file_count);
        for r in raws {
            let folder = strings
                .get(r.folder_id as usize)
                .ok_or(PazError::BadPathIndex(r.folder_id))?;
            let name = strings
                .get(r.file_id as usize)
                .ok_or(PazError::BadPathIndex(r.file_id))?;
            let mut full = String::with_capacity(folder.len() + name.len());
            full.push_str(folder);
            full.push_str(name);
            files.push(FileRecord {
                crc: r.crc,
                offset: r.offset,
                comp_size: r.comp_size,
                orig_size: r.orig_size,
                path: normalize_path(&full),
            });
        }

        Ok(PazArchive { path, crc, files })
    }

    /// Extract a single file's decoded bytes, reading from the open archive.
    ///
    /// `archive_bytes` is the full PAZ file content (callers can mmap/read once
    /// and extract many files cheaply).
    pub fn extract(
        archive_bytes: &[u8],
        rec: &FileRecord,
        ice: &IceKey,
    ) -> Result<Vec<u8>, PazError> {
        let start = rec.offset as usize;
        let end = start + rec.comp_size as usize;
        let raw = archive_bytes
            .get(start..end)
            .ok_or(PazError::Malformed("file payload past end of archive"))?;

        let mut buf = raw.to_vec();

        if rec.is_encrypted() {
            // ICE works on 8-byte blocks; payloads are padded to a multiple of 8.
            let usable = buf.len() - (buf.len() % 8);
            ice.decrypt(&mut buf[..usable]);
        }

        let compressed = rec.is_compressed() || buf.first() == Some(&0x6E);
        if compressed {
            decompress(&buf, rec.orig_size)
        } else {
            buf.truncate(rec.orig_size as usize);
            Ok(buf)
        }
    }
}

/// Split a NUL-separated byte buffer into owned `String`s (lossy UTF-8).
fn split_nul_strings(buf: &[u8]) -> Result<Vec<String>, PazError> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, &b) in buf.iter().enumerate() {
        if b == 0 {
            out.push(String::from_utf8_lossy(&buf[start..i]).into_owned());
            start = i + 1;
        }
    }
    if out.is_empty() {
        return Err(PazError::BadPathTable);
    }
    Ok(out)
}

/// Normalise a virtual path to forward slashes and strip leading separators.
fn normalize_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.trim_start_matches('/').to_string()
}
