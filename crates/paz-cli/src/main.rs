//! `paz` — command-line tool to inspect and extract Black Desert Online
//! `.PAZ` archives.

mod convert;
mod index;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, Subcommand};
use paz_core::{IceKey, PazArchive, PAZ_ICE_KEY};
use rayon::prelude::*;

use index::ArchiveIndex;

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
        Command::Extract {
            output,
            filter,
            ext,
            convert,
            limit,
        } => {
            extract(&cli.input, &ice, output, filter, ext, convert, limit)?;
        }
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
) -> Result<(), Box<dyn std::error::Error>> {
    let idx = ArchiveIndex::build(input, ice)?;
    let needle = filter.map(|s| s.to_lowercase());
    let exts: Vec<String> = exts.into_iter().map(|e| e.to_lowercase()).collect();

    // Select matching files.
    let selected: Vec<&index::FileEntry> = idx
        .files
        .iter()
        .filter(|fe| {
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
