//! Error types for the PAZ core library.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur while reading or extracting PAZ archives.
#[derive(Debug, Error)]
pub enum PazError {
    /// An underlying filesystem error.
    #[error("io error on {path}: {source}")]
    Io {
        /// The path that triggered the error.
        path: PathBuf,
        /// The underlying io error.
        #[source]
        source: std::io::Error,
    },

    /// The archive header or index was malformed.
    #[error("malformed archive: {0}")]
    Malformed(&'static str),

    /// The custom decompressor failed.
    #[error("decompression failed: {0}")]
    Decompress(&'static str),

    /// A filename-table index was out of range.
    #[error("path index {0} out of range")]
    BadPathIndex(u32),

    /// The decrypted filename table was not valid UTF-8 / contained no strings.
    #[error("filename table parse error")]
    BadPathTable,
}

/// Convenience for attaching a path to an [`std::io::Error`].
pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> PazError {
    PazError::Io {
        path: path.into(),
        source,
    }
}
