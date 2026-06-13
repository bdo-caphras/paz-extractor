//! Builds a unified, in-memory index of every file across every PAZ archive,
//! parsing the per-archive indices in parallel.

use std::path::{Path, PathBuf};

use paz_core::{FileRecord, IceKey, Meta, PazArchive};
use rayon::prelude::*;

/// One file plus a back-reference to the archive that holds it.
pub struct FileEntry {
    /// Index into [`ArchiveIndex::archive_paths`].
    pub archive_idx: usize,
    /// The file's index record (path, offset, sizes).
    pub rec: FileRecord,
}

/// The whole virtual filesystem assembled from all archives.
pub struct ArchiveIndex {
    pub meta_version: u32,
    pub archive_count: usize,
    pub parsed_archives: usize,
    pub archive_paths: Vec<PathBuf>,
    pub files: Vec<FileEntry>,
}

impl ArchiveIndex {
    /// Read the manifest and parse every referenced `.PAZ` index.
    pub fn build(input: &Path, ice: &IceKey) -> Result<Self, Box<dyn std::error::Error>> {
        let meta_path = input.join("pad00000.meta");
        let meta = Meta::load(&meta_path)?;

        // Resolve each PAZ id to a path (case-insensitive PAD#####.PAZ).
        let archive_paths: Vec<PathBuf> = meta
            .entries
            .iter()
            .map(|e| input.join(format!("PAD{:05}.PAZ", e.paz_id)))
            .collect();

        // Parse all archive indices in parallel.
        let results: Vec<(usize, Vec<FileRecord>)> = archive_paths
            .par_iter()
            .enumerate()
            .filter_map(|(i, p)| match PazArchive::open(p, ice) {
                Ok(a) => Some((i, a.files)),
                Err(_) => None, // missing/corrupt archive: skip, keep going
            })
            .collect();

        let parsed_archives = results.len();
        let mut files = Vec::new();
        for (archive_idx, recs) in results {
            for rec in recs {
                files.push(FileEntry { archive_idx, rec });
            }
        }
        files.sort_by(|a, b| a.rec.path.cmp(&b.rec.path));

        Ok(ArchiveIndex {
            meta_version: meta.version,
            archive_count: meta.entries.len(),
            parsed_archives,
            archive_paths,
            files,
        })
    }
}
