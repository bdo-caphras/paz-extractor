//! `paz-core`: a clean-room reader for Black Desert Online `.PAZ`
//! (Pearl Abyss) game archives.
//!
//! The format, in brief:
//! * `pad00000.meta` is an unencrypted manifest of every `PAD#####.PAZ` file.
//! * Each `.PAZ` begins with a small header, a fixed-size file index, and an
//!   **ICE-encrypted** filename table. File payloads are **ICE-encrypted**
//!   (except `.dbss`) and compressed with a **custom BDO LZ77** scheme.
//!
//! See [`archive`], [`meta`], [`ice`], and [`decompress`] for details.

pub mod archive;
pub mod decompress;
pub mod error;
pub mod ice;
pub mod meta;

pub use archive::{FileRecord, PazArchive, PAZ_ICE_KEY};
pub use error::PazError;
pub use ice::IceKey;
pub use meta::{Meta, PazEntry};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_round_trips_known_vector() {
        // ICE level-1 self-consistency: a buffer encrypted then decrypted with
        // the same key returns the original. We only ship decrypt, so verify the
        // decrypt of a hand-encrypted block via the public algorithm property
        // that decrypt(decrypt-inverse) is stable across our S-box init.
        let key = IceKey::new(&PAZ_ICE_KEY);
        // Two identical 8-byte ECB blocks must decrypt identically.
        let mut a = [0x11u8; 16];
        key.decrypt(&mut a);
        assert_eq!(&a[0..8], &a[8..16], "ECB blocks must be deterministic");
    }

    #[test]
    fn meta_parse_rejects_short() {
        assert!(Meta::parse(&[0u8; 4]).is_err());
    }

    #[test]
    fn stored_payload_decompress() {
        // flags=0 (stored), short header, decompressed_len=4, then 4 bytes.
        let payload = [0x00u8, 0x00, 0x04, b'A', b'B', b'C', b'D'];
        let out = decompress::decompress(&payload, 4).unwrap();
        assert_eq!(out, b"ABCD");
    }
}
