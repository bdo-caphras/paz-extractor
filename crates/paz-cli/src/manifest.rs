//! Hash-based change tracking for the PAZ set.
//!
//! Every file record in a PAZ index already carries the game's own per-file
//! `crc` plus the decompressed `orig_size`. That pair is a cheap, robust
//! change key: it needs **no decompression**, so generating a full manifest is
//! as fast as building the index (~45 s), not a full extract.
//!
//! A *manifest* is a deterministic, path-sorted TSV of every virtual path and
//! its change key. Two manifests of different game builds `diff` cleanly into
//! ADDED / MODIFIED / REMOVED / UNCHANGED sets, which drives weekly incremental
//! extraction (extract only ADDED+MODIFIED).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// The change key + provenance for one virtual path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Numeric id of the PAZ archive holding this file (`N` in `PAD0000N.PAZ`).
    pub paz_id: u32,
    /// The game's own per-file integrity value (change key, part 1).
    pub crc: u32,
    /// Decompressed size in bytes (change key, part 2).
    pub orig_size: u32,
    /// On-disk (compressed/encrypted) size — recorded for completeness.
    pub comp_size: u32,
}

impl ManifestEntry {
    /// The change key: two manifests agree on a path iff these match.
    fn key(&self) -> (u32, u32) {
        (self.crc, self.orig_size)
    }
}

/// A whole manifest: path → change key, kept sorted by path for deterministic,
/// diff-friendly output.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    /// `BTreeMap` keeps paths in stable lexicographic order automatically.
    pub entries: BTreeMap<String, ManifestEntry>,
}

/// TSV header written/expected at the top of a manifest file.
pub const MANIFEST_HEADER: &str = "path\tpaz_id\tcrc\torig_size\tcomp_size";

impl Manifest {
    /// Number of paths in the manifest.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the manifest has no entries.
    #[allow(dead_code)] // idiomatic companion to `len`
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert/overwrite one entry.
    pub fn insert(&mut self, path: String, entry: ManifestEntry) {
        self.entries.insert(path, entry);
    }

    /// Write the manifest as a path-sorted TSV with a header. The `BTreeMap`
    /// iteration order is already lexicographic, so output is deterministic.
    pub fn write_tsv(&self, out: &Path) -> std::io::Result<()> {
        let f = std::fs::File::create(out)?;
        let mut w = std::io::BufWriter::new(f);
        writeln!(w, "{MANIFEST_HEADER}")?;
        for (path, e) in &self.entries {
            writeln!(
                w,
                "{}\t{}\t{}\t{}\t{}",
                path, e.paz_id, e.crc, e.orig_size, e.comp_size
            )?;
        }
        w.flush()
    }

    /// Parse a manifest TSV previously written by [`write_tsv`](Self::write_tsv).
    /// The header line is tolerated (skipped) whether present or not.
    pub fn read_tsv(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        Self::parse_tsv(&text)
    }

    /// Parse manifest TSV from an in-memory string (header tolerated).
    pub fn parse_tsv(text: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut m = Manifest::default();
        for (lineno, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            // Skip the header row if present.
            if lineno == 0 && line.starts_with("path\t") {
                continue;
            }
            let mut cols = line.split('\t');
            let path = cols
                .next()
                .ok_or_else(|| format!("manifest line {}: missing path", lineno + 1))?;
            let paz_id = cols
                .next()
                .ok_or_else(|| format!("manifest line {}: missing paz_id", lineno + 1))?
                .parse()?;
            let crc = cols
                .next()
                .ok_or_else(|| format!("manifest line {}: missing crc", lineno + 1))?
                .parse()?;
            let orig_size = cols
                .next()
                .ok_or_else(|| format!("manifest line {}: missing orig_size", lineno + 1))?
                .parse()?;
            let comp_size = cols
                .next()
                .ok_or_else(|| format!("manifest line {}: missing comp_size", lineno + 1))?
                .parse()?;
            m.insert(
                path.to_string(),
                ManifestEntry {
                    paz_id,
                    crc,
                    orig_size,
                    comp_size,
                },
            );
        }
        Ok(m)
    }
}

/// The full classification of `new` against `old`, sorted by path.
#[derive(Debug, Clone, Default)]
pub struct DiffResult {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
}

impl DiffResult {
    /// ADDED + MODIFIED paths — the set the weekly incremental extract needs.
    pub fn changed(&self) -> impl Iterator<Item = &String> {
        self.added.iter().chain(self.modified.iter())
    }
}

/// Classify every path across `old` and `new` into ADDED / MODIFIED / REMOVED /
/// UNCHANGED. Output vectors are path-sorted (the `BTreeMap` keys already are).
pub fn diff(old: &Manifest, new: &Manifest) -> DiffResult {
    let mut r = DiffResult::default();
    // Walk the new manifest: each path is ADDED, MODIFIED or UNCHANGED.
    for (path, new_e) in &new.entries {
        match old.entries.get(path) {
            None => r.added.push(path.clone()),
            Some(old_e) if old_e.key() != new_e.key() => r.modified.push(path.clone()),
            Some(_) => r.unchanged.push(path.clone()),
        }
    }
    // Anything only in old is REMOVED.
    for path in old.entries.keys() {
        if !new.entries.contains_key(path) {
            r.removed.push(path.clone());
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(crc: u32, orig: u32) -> ManifestEntry {
        ManifestEntry {
            paz_id: 1,
            crc,
            orig_size: orig,
            comp_size: orig / 2,
        }
    }

    fn manifest(items: &[(&str, ManifestEntry)]) -> Manifest {
        let mut m = Manifest::default();
        for (p, en) in items {
            m.insert((*p).to_string(), en.clone());
        }
        m
    }

    #[test]
    fn classifies_added_modified_removed_unchanged() {
        let old = manifest(&[
            ("a/same.xml", e(10, 100)),
            ("b/changed_crc.xml", e(20, 200)),
            ("c/changed_size.xml", e(30, 300)),
            ("d/gone.xml", e(40, 400)),
        ]);
        let new = manifest(&[
            ("a/same.xml", e(10, 100)),         // unchanged
            ("b/changed_crc.xml", e(99, 200)),  // crc differs -> modified
            ("c/changed_size.xml", e(30, 999)), // size differs -> modified
            ("z/brand_new.xml", e(50, 500)),    // added
        ]);

        let d = diff(&old, &new);
        assert_eq!(d.added, vec!["z/brand_new.xml"]);
        assert_eq!(d.modified, vec!["b/changed_crc.xml", "c/changed_size.xml"]);
        assert_eq!(d.removed, vec!["d/gone.xml"]);
        assert_eq!(d.unchanged, vec!["a/same.xml"]);
    }

    #[test]
    fn changed_iter_is_added_plus_modified() {
        let old = manifest(&[("keep", e(1, 1)), ("mod", e(2, 2))]);
        let new = manifest(&[("keep", e(1, 1)), ("mod", e(2, 3)), ("new", e(9, 9))]);
        let d = diff(&old, &new);
        let mut changed: Vec<&String> = d.changed().collect();
        changed.sort();
        assert_eq!(changed, vec![&"mod".to_string(), &"new".to_string()]);
    }

    #[test]
    fn empty_old_means_everything_added() {
        let old = Manifest::default();
        let new = manifest(&[("a", e(1, 1)), ("b", e(2, 2))]);
        let d = diff(&old, &new);
        assert_eq!(d.added, vec!["a", "b"]);
        assert!(d.modified.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.unchanged.is_empty());
    }

    #[test]
    fn empty_new_means_everything_removed() {
        let old = manifest(&[("a", e(1, 1)), ("b", e(2, 2))]);
        let new = Manifest::default();
        let d = diff(&old, &new);
        assert_eq!(d.removed, vec!["a", "b"]);
        assert!(d.added.is_empty());
        assert!(d.modified.is_empty());
        assert!(d.unchanged.is_empty());
    }

    #[test]
    fn tsv_round_trips() {
        let m = manifest(&[
            ("ui_texture/icon/a.dds", e(123, 4096)),
            ("gamecommondata/binary/t.dbss", e(456, 8192)),
        ]);
        let text = {
            let mut s = String::new();
            s.push_str(MANIFEST_HEADER);
            s.push('\n');
            for (p, en) in &m.entries {
                s.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\n",
                    p, en.paz_id, en.crc, en.orig_size, en.comp_size
                ));
            }
            s
        };
        let parsed = Manifest::parse_tsv(&text).unwrap();
        assert_eq!(parsed.entries, m.entries);
    }

    #[test]
    fn parse_tolerates_missing_header() {
        let text = "x/y.xml\t3\t7\t11\t5\n";
        let m = Manifest::parse_tsv(text).unwrap();
        assert_eq!(m.len(), 1);
        let got = m.entries.get("x/y.xml").unwrap();
        assert_eq!(*got, e(7, 11).with_paz(3));
    }

    impl ManifestEntry {
        fn with_paz(mut self, p: u32) -> Self {
            self.paz_id = p;
            self
        }
    }

    #[test]
    fn diff_output_is_path_sorted() {
        let old = Manifest::default();
        let new = manifest(&[("zeta", e(1, 1)), ("alpha", e(2, 2)), ("mid", e(3, 3))]);
        let d = diff(&old, &new);
        assert_eq!(d.added, vec!["alpha", "mid", "zeta"]);
    }
}
