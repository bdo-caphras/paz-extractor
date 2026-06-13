//! `paz` — command-line tool to inspect and extract Black Desert Online
//! `.PAZ` archives.

mod convert;
mod index;
mod manifest;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, Subcommand};
use paz_core::{IceKey, PazArchive, PAZ_ICE_KEY};
use rayon::prelude::*;

use index::ArchiveIndex;
use manifest::{Manifest, ManifestEntry};

#[derive(Parser)]
#[command(
    name = "paz",
    version,
    about = "Clean-room extractor for Black Desert Online .PAZ archives"
)]
struct Cli {
    /// Directory containing PAD#####.PAZ and pad00000.meta.
    #[arg(short = 'i', long, global = true, default_value = "/mnt/d/caphras-paz")]
    input: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse the manifest + every PAZ index and print a summary.
    Stats,
    /// List virtual file paths (optionally filtered by a substring).
    List {
        /// Only show paths containing this substring (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
        /// Maximum number of paths to print.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Report the distribution of file extensions across all archives.
    Categories,
    /// Build a cached, newline-separated list of every virtual path
    /// (default: `paz-index.txt`). Lets `search` run instantly instead of
    /// re-scanning ~10k archive indices (~45 s) on every query.
    Index {
        /// Where to write the path list.
        #[arg(short = 'o', long, default_value = "paz-index.txt")]
        output: PathBuf,
    },
    /// Fast search over a cached index file (see `index`). Prints paths that
    /// contain ALL of the given terms (case-insensitive AND). Optionally
    /// restrict by extension. Falls back to a live scan if the cache is absent.
    Search {
        /// Substrings that must all appear in the path (case-insensitive).
        #[arg(required = true)]
        terms: Vec<String>,
        /// Path to the cached index produced by `index`.
        #[arg(long, default_value = "paz-index.txt")]
        cache: PathBuf,
        /// Only show paths with one of these extensions (comma-separated).
        #[arg(long, value_delimiter = ',')]
        ext: Vec<String>,
        /// Maximum number of paths to print (0 = no limit).
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// Print only the number of matches, not the paths.
        #[arg(long)]
        count: bool,
    },
    /// Build a deterministic, path-sorted change-tracking manifest of every
    /// virtual path: `path<TAB>paz_id<TAB>crc<TAB>orig_size<TAB>comp_size`.
    /// Uses only the per-file `crc`/`orig_size` already in each PAZ index — no
    /// payload is decompressed, so this is as fast as `index`. Two manifests of
    /// different game builds `diff` cleanly to drive incremental extraction.
    Manifest {
        /// Where to write the manifest TSV.
        #[arg(short = 'o', long, default_value = "paz-manifest.tsv")]
        output: PathBuf,
    },
    /// Compare an OLD manifest against a NEW one (or a live scan of `-i`) and
    /// classify every path as ADDED (`+`), MODIFIED (`~`), REMOVED (`-`) or
    /// UNCHANGED. The `(crc, orig_size)` change key drives MODIFIED detection.
    Diff {
        /// Baseline manifest to compare against (produced by `manifest`).
        #[arg(long)]
        old: PathBuf,
        /// Newer manifest. If omitted, a current manifest is built in-memory
        /// from `-i` (a live scan of the archive set).
        #[arg(long)]
        new: Option<PathBuf>,
        /// Restrict the comparison to these extensions (comma-separated).
        #[arg(long, value_delimiter = ',')]
        ext: Vec<String>,
        /// Restrict the comparison to paths containing this substring.
        #[arg(long)]
        filter: Option<String>,
        /// Print only the changed paths (added + modified), one per line, for
        /// piping. Suppresses the summary and status prefixes.
        #[arg(long)]
        names_only: bool,
        /// Print only the per-class tallies, not the path lists.
        #[arg(long)]
        count: bool,
    },
    /// Extract files to an output directory.
    Extract {
        /// Output root directory.
        #[arg(short = 'o', long, default_value = "./out")]
        output: PathBuf,
        /// Only extract paths containing this substring (case-insensitive).
        #[arg(long)]
        filter: Option<String>,
        /// Only extract files with one of these extensions (comma-separated).
        #[arg(long, value_delimiter = ',')]
        ext: Vec<String>,
        /// Convert images (DDS) to PNG when possible.
        #[arg(long)]
        convert: bool,
        /// Stop after this many files (0 = no limit). Useful for sampling.
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// Incremental mode: extract ONLY paths that are ADDED or MODIFIED
        /// versus this baseline manifest (still honouring `--filter`/`--ext`/
        /// `--convert`). This is the weekly delta path.
        #[arg(long)]
        changed_since: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let ice = IceKey::new(&PAZ_ICE_KEY);

    match cli.command {
        Command::Stats => {
            let idx = ArchiveIndex::build(&cli.input, &ice)?;
            println!("meta version : 0x{:08x}", idx.meta_version);
            println!("paz files    : {}", idx.archive_count);
            println!("indexed paz  : {}", idx.parsed_archives);
            println!("total files  : {}", idx.files.len());
            let total_orig: u64 = idx.files.iter().map(|f| f.rec.orig_size as u64).sum();
            println!(
                "total bytes  : {} ({:.2} GiB decompressed)",
                total_orig,
                total_orig as f64 / (1u64 << 30) as f64
            );
        }
        Command::List { filter, limit } => {
            let idx = ArchiveIndex::build(&cli.input, &ice)?;
            let f = filter.map(|s| s.to_lowercase());
            let mut shown = 0;
            for fe in &idx.files {
                if let Some(ref needle) = f {
                    if !fe.rec.path.to_lowercase().contains(needle) {
                        continue;
                    }
                }
                println!("{}", fe.rec.path);
                shown += 1;
                if shown >= limit {
                    break;
                }
            }
            eprintln!("({shown} shown)");
        }
        Command::Categories => {
            let idx = ArchiveIndex::build(&cli.input, &ice)?;
            let mut map: std::collections::BTreeMap<String, (u64, u64)> = Default::default();
            for fe in &idx.files {
                let ext = fe.rec.path.rsplit('.').next().unwrap_or("").to_lowercase();
                let e = map.entry(ext).or_default();
                e.0 += 1;
                e.1 += fe.rec.orig_size as u64;
            }
            let mut rows: Vec<_> = map.into_iter().collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r.1 .0));
            println!("{:<14} {:>10} {:>14}", "ext", "count", "bytes");
            for (ext, (count, bytes)) in rows {
                println!("{:<14} {:>10} {:>14}", ext, count, bytes);
            }
        }
        Command::Index { output } => {
            let idx = ArchiveIndex::build(&cli.input, &ice)?;
            let mut buf = String::with_capacity(idx.files.len() * 48);
            for fe in &idx.files {
                buf.push_str(&fe.rec.path);
                buf.push('\n');
            }
            std::fs::write(&output, buf)?;
            eprintln!("wrote {} paths to {}", idx.files.len(), output.display());
        }
        Command::Search {
            terms,
            cache,
            ext,
            limit,
            count,
        } => {
            search(&cli.input, &ice, terms, cache, ext, limit, count)?;
        }
        Command::Manifest { output } => {
            let m = build_manifest(&cli.input, &ice)?;
            m.write_tsv(&output)?;
            eprintln!("wrote {} paths to {}", m.len(), output.display());
        }
        Command::Diff {
            old,
            new,
            ext,
            filter,
            names_only,
            count,
        } => {
            diff_cmd(&cli.input, &ice, old, new, ext, filter, names_only, count)?;
        }
        Command::Extract {
            output,
            filter,
            ext,
            convert,
            limit,
            changed_since,
        } => {
            extract(
                &cli.input,
                &ice,
                output,
                filter,
                ext,
                convert,
                limit,
                changed_since,
            )?;
        }
    }
    Ok(())
}

/// Build an in-memory [`Manifest`] from the live archive set. Reuses the same
/// index-parsing path as `stats`/`index` — only the per-file `crc`/sizes are
/// read, never the payloads.
fn build_manifest(
    input: &std::path::Path,
    ice: &IceKey,
) -> Result<Manifest, Box<dyn std::error::Error>> {
    let idx = ArchiveIndex::build(input, ice)?;
    let mut m = Manifest::default();
    for fe in &idx.files {
        let paz_id = paz_id_of(&idx, fe.archive_idx);
        m.insert(
            fe.rec.path.clone(),
            ManifestEntry {
                paz_id,
                crc: fe.rec.crc,
                orig_size: fe.rec.orig_size,
                comp_size: fe.rec.comp_size,
            },
        );
    }
    Ok(m)
}

/// Recover the numeric PAZ id (`N` in `PAD0000N.PAZ`) for an archive index by
/// parsing it back out of the archive's filename.
fn paz_id_of(idx: &ArchiveIndex, archive_idx: usize) -> u32 {
    idx.archive_paths
        .get(archive_idx)
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .and_then(|s| {
            s.trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

/// Apply `--ext` / `--filter` restrictions to a path (used by `diff`).
fn path_in_scope(path: &str, exts: &[String], filter: &Option<String>) -> bool {
    if let Some(f) = filter {
        if !path.to_lowercase().contains(f.as_str()) {
            return false;
        }
    }
    if !exts.is_empty() {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        if !exts.iter().any(|e| e == &ext) {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn diff_cmd(
    input: &std::path::Path,
    ice: &IceKey,
    old: PathBuf,
    new: Option<PathBuf>,
    exts: Vec<String>,
    filter: Option<String>,
    names_only: bool,
    count_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let exts: Vec<String> = exts.into_iter().map(|e| e.to_lowercase()).collect();
    let filter = filter.map(|s| s.to_lowercase());

    let old_m = Manifest::read_tsv(&old)?;
    let new_m = match &new {
        Some(p) => Manifest::read_tsv(p)?,
        None => {
            eprintln!(
                "no --new manifest: building current manifest from {} …",
                input.display()
            );
            build_manifest(input, ice)?
        }
    };

    let d = manifest::diff(&old_m, &new_m);

    // Apply scope filters to each class.
    let scope = |v: &[String]| -> Vec<String> {
        v.iter()
            .filter(|p| path_in_scope(p, &exts, &filter))
            .cloned()
            .collect()
    };
    let added = scope(&d.added);
    let modified = scope(&d.modified);
    let removed = scope(&d.removed);
    let unchanged_n = d
        .unchanged
        .iter()
        .filter(|p| path_in_scope(p, &exts, &filter))
        .count();

    if names_only {
        // Added + modified, sorted, for piping.
        let mut changed: Vec<&String> = added.iter().chain(modified.iter()).collect();
        changed.sort();
        for p in changed {
            println!("{p}");
        }
        return Ok(());
    }

    if count_only {
        println!("added     {}", added.len());
        println!("modified  {}", modified.len());
        println!("removed   {}", removed.len());
        println!("unchanged {unchanged_n}");
        return Ok(());
    }

    // Full summary + tagged path lists.
    for p in &added {
        println!("+ {p}");
    }
    for p in &modified {
        println!("~ {p}");
    }
    for p in &removed {
        println!("- {p}");
    }
    eprintln!(
        "summary: {} added, {} modified, {} removed, {} unchanged",
        added.len(),
        modified.len(),
        removed.len(),
        unchanged_n
    );
    Ok(())
}

/// Whether `path` (lowercased into `lc`) matches all `terms` and, if any
/// `exts` are given, ends with one of them.
fn path_matches(lc: &str, terms: &[String], exts: &[String]) -> bool {
    if !terms.iter().all(|t| lc.contains(t.as_str())) {
        return false;
    }
    if !exts.is_empty() {
        let ext = lc.rsplit('.').next().unwrap_or("");
        if !exts.iter().any(|e| e == ext) {
            return false;
        }
    }
    true
}

/// Fast AND-search. Prefers a cached index file (instant); if it is missing,
/// falls back to a one-off live scan of every archive.
fn search(
    input: &std::path::Path,
    ice: &IceKey,
    terms: Vec<String>,
    cache: PathBuf,
    exts: Vec<String>,
    limit: usize,
    count_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let terms: Vec<String> = terms.into_iter().map(|t| t.to_lowercase()).collect();
    let exts: Vec<String> = exts.into_iter().map(|e| e.to_lowercase()).collect();

    // Source of paths: cached file if present, else a live scan.
    let paths: Vec<String> = if cache.exists() {
        std::fs::read_to_string(&cache)?
            .lines()
            .map(|s| s.to_string())
            .collect()
    } else {
        eprintln!(
            "note: cache {} not found — doing a live scan (run `paz index` to cache).",
            cache.display()
        );
        ArchiveIndex::build(input, ice)?
            .files
            .iter()
            .map(|fe| fe.rec.path.clone())
            .collect()
    };

    let mut matched = 0usize;
    let mut shown = 0usize;
    for p in &paths {
        let lc = p.to_lowercase();
        if !path_matches(&lc, &terms, &exts) {
            continue;
        }
        matched += 1;
        if !count_only && (limit == 0 || shown < limit) {
            println!("{p}");
            shown += 1;
        }
    }

    if count_only {
        println!("{matched}");
    } else {
        eprintln!("({matched} matched, {shown} shown)");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn extract(
    input: &std::path::Path,
    ice: &IceKey,
    output: PathBuf,
    filter: Option<String>,
    exts: Vec<String>,
    convert: bool,
    limit: usize,
    changed_since: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let idx = ArchiveIndex::build(input, ice)?;
    let needle = filter.map(|s| s.to_lowercase());
    let exts: Vec<String> = exts.into_iter().map(|e| e.to_lowercase()).collect();

    // Incremental mode: build the set of ADDED/MODIFIED paths vs the baseline
    // manifest. Only those (intersected with --filter/--ext) get extracted.
    let changed_paths: Option<std::collections::HashSet<String>> = match changed_since {
        Some(ref old_path) => {
            let old_m = Manifest::read_tsv(old_path)?;
            let mut new_m = Manifest::default();
            for fe in &idx.files {
                new_m.insert(
                    fe.rec.path.clone(),
                    ManifestEntry {
                        paz_id: 0,
                        crc: fe.rec.crc,
                        orig_size: fe.rec.orig_size,
                        comp_size: fe.rec.comp_size,
                    },
                );
            }
            let d = manifest::diff(&old_m, &new_m);
            let set: std::collections::HashSet<String> = d.changed().cloned().collect();
            eprintln!(
                "changed-since {}: {} added/modified paths in scope before --filter/--ext",
                old_path.display(),
                set.len()
            );
            Some(set)
        }
        None => None,
    };

    // Select matching files.
    let selected: Vec<&index::FileEntry> = idx
        .files
        .iter()
        .filter(|fe| {
            if let Some(ref changed) = changed_paths {
                if !changed.contains(&fe.rec.path) {
                    return false;
                }
            }
            if let Some(ref n) = needle {
                if !fe.rec.path.to_lowercase().contains(n) {
                    return false;
                }
            }
            if !exts.is_empty() {
                let ext = fe.rec.path.rsplit('.').next().unwrap_or("").to_lowercase();
                if !exts.contains(&ext) {
                    return false;
                }
            }
            true
        })
        .take(if limit == 0 { usize::MAX } else { limit })
        .collect();

    println!(
        "extracting {} files to {}",
        selected.len(),
        output.display()
    );

    // Group selections by archive so we read each PAZ once.
    use std::collections::HashMap;
    let mut by_archive: HashMap<usize, Vec<&index::FileEntry>> = HashMap::new();
    for fe in &selected {
        by_archive.entry(fe.archive_idx).or_default().push(fe);
    }

    let written = AtomicU64::new(0);
    let failed = AtomicU64::new(0);
    let groups: Vec<_> = by_archive.into_iter().collect();

    groups.par_iter().for_each(|(archive_idx, entries)| {
        let archive_path = &idx.archive_paths[*archive_idx];
        let bytes = match std::fs::read(archive_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {}: {e}", archive_path.display());
                failed.fetch_add(entries.len() as u64, Ordering::Relaxed);
                return;
            }
        };
        for fe in entries {
            match PazArchive::extract(&bytes, &fe.rec, ice) {
                Ok(data) => {
                    if let Err(e) = write_one(&output, &fe.rec.path, &data, convert) {
                        eprintln!("write {}: {e}", fe.rec.path);
                        failed.fetch_add(1, Ordering::Relaxed);
                    } else {
                        written.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    eprintln!("extract {}: {e}", fe.rec.path);
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    println!(
        "done: {} written, {} failed",
        written.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed)
    );
    Ok(())
}

/// Write one extracted file, optionally converting DDS -> PNG.
fn write_one(
    root: &std::path::Path,
    vpath: &str,
    data: &[u8],
    convert: bool,
) -> std::io::Result<()> {
    let dest = root.join(vpath);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if convert {
        if let Some(png) = convert::try_convert(vpath, data) {
            let png_dest = dest.with_extension("png");
            std::fs::write(&png_dest, png)?;
            return Ok(());
        }
    }

    std::fs::write(&dest, data)
}
