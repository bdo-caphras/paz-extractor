# paz-extractor

A standalone, first-party **Rust** CLI that extracts Black Desert Online
`.PAZ` (Pearl Abyss) game archives into clean, organised, lighter derived files.

It is a clean-room reimplementation: the PAZ container layout, the ICE cipher,
and the BDO LZ77 decompressor were re-derived from public sources (see
**Credits**) and written fresh in Rust. No third-party code is vendored.

This tool is **independent of the Caphras web app**. It runs locally against the
full `.PAZ` set on disk and produces a much smaller tree of decoded assets/data
that flows into Caphras.

---

## What it does

* Parses `pad00000.meta` (the archive manifest) and every `PAD#####.PAZ` index.
* Decrypts the per-archive filename table (ICE, level 1) and the file payloads.
* Decompresses payloads (custom BDO LZ77).
* Optionally converts DDS textures to PNG.
* Lists, filters, categorises, and extracts files in parallel (rayon).

Verified against a real live-client data set: **10,832 archives,
834,727 files, ~122 GiB decompressed** — all indices parsed, sample text/XML
and DDS→PNG outputs confirmed valid.

---

## The PAZ format (as implemented)

All integers are little-endian.

### `pad00000.meta` — manifest (not encrypted)
```
[u32 version]          // observed 0x0d0e
[u32 paz_count]
paz_count × {
  [u32 paz_id]         // N in PAD0000N.PAZ
  [u32 crc]            // == first 4 bytes of the PAZ file
  [u32 size]           // PAZ file size in bytes
}
```

### `PAD#####.PAZ` — one archive
```
[u32 crc]              // first 4 bytes (matches manifest)
[u32 file_count]
[u32 path_block_len]
file_count × {         // 24 bytes each
  [u32 crc]
  [u32 folder_id]      // index into the filename string table
  [u32 file_id]        // index into the filename string table
  [u32 offset]         // absolute offset of payload within this PAZ
  [u32 comp_size]      // on-disk (encrypted/compressed) size
  [u32 orig_size]      // decompressed size
}
[path_block_len bytes] // ICE-encrypted, NUL-separated string table.
                       // full path = table[folder_id] + table[file_id]
```

### Encryption — ICE (level 1)
* Algorithm: ICE block cipher (Matthew Kwan), 8-byte blocks, ECB.
* Key (8 bytes): `51 F3 0F 11 04 24 6A 00`  → level 1, 8 rounds.
* Applies to: the filename table, and every file payload **except** `.dbss`
  (those are stored plaintext). ICE works on 8-byte blocks; payloads are padded
  to a multiple of 8, and only the whole-block prefix is decrypted.

### Compression — custom BDO LZ77
Payload byte 0 is a flag: bit 0 = compressed, bit 1 = "long" 32-bit header.
* Short header (3 bytes): `[flags][comp_len:u8][orig_len:u8]`.
* Long header (9 bytes): `[flags][comp_len:u32][orig_len:u32]`.
A file is treated as compressed when `orig_size > comp_size` or the first
decrypted byte is `0x6E`. The match/literal control-bit scheme is implemented in
`crates/paz-core/src/decompress.rs`.

> Note: there is **no patch-specific / per-build decryption key** blocking this
> data — the single static ICE key above decrypts the entire live set. (Pearl
> Abyss changed the KR meta key once back in 2016; the current global key is the
> one shipped here.)

---

## Data categories found

Top extensions by file count in the test set:

| ext | meaning | count |
|-----|---------|------:|
| dds | textures, item icons, map tiles | 292,719 |
| pac/paa/pam/pae… | proprietary model/animation/mesh blobs | ~250k |
| bnk/wem/pcm/bk2/webm | audio / video | ~90k |
| xml | game data, config, per-locale tables | 37,758 |
| mapdata | map/terrain data | 31,453 |
| png | images | 16,927 |
| txt | text / lists / AI scripts | 344 |
| dbss | (plaintext) string/binary data | 375 |

**Directly exploitable for Caphras today:** `dds`/`png` (icons, map tiles → PNG),
`xml`/`txt`/`dbss` (tabular game data, per-locale strings). The `pa*` model and
`bnk/wem` audio formats are extracted as raw blobs (their inner formats are
proprietary and out of scope here).

---

## Build & run

### Native (WSL/Linux, for development & testing)
```bash
cargo build --release
./target/release/paz stats                # summary of the whole set
./target/release/paz categories           # extension distribution
./target/release/paz list --filter icon --limit 50

# Build a one-off cached path index (~45 s), then search it instantly (~0.2 s):
./target/release/paz index -o paz-index.txt
./target/release/paz search territorymark --ext dds            # AND-search by terms
./target/release/paz search symbolicon ulukita                 # all terms must match
./target/release/paz search worldmapmonster --count            # just the count

./target/release/paz extract -o ./out --ext dds,png --convert
./target/release/paz extract -o ./out --filter languagedata --ext xml,txt
```
Default input dir is `/mnt/d/caphras-paz`; override with `-i <dir>`.

### Change tracking & incremental extraction

Each PAZ index already stores, per file, the game's own `crc` and the
decompressed `orig_size`. The pair `(crc, orig_size)` is a robust change key
that needs **no decompression**, so we can fingerprint the entire set as fast as
`index` (~45 s) and only re-extract what actually changed between game builds.

```bash
# 1. Fingerprint the whole set into a deterministic, path-sorted TSV manifest.
#    Columns: path<TAB>paz_id<TAB>crc<TAB>orig_size<TAB>comp_size
./target/release/paz manifest -o manifests/2026-06-14.tsv      # 834,727 lines, ~68 MB, ~45 s

# 2. After a patch, diff a fresh set against the baseline. Either pass --new
#    <manifest> or omit it to live-scan -i. Classes: + added, ~ modified,
#    - removed, unchanged.
./target/release/paz diff --old manifests/2026-06-14.tsv --count          # just the tallies
./target/release/paz diff --old manifests/2026-06-14.tsv --ext dds        # tagged path lists
./target/release/paz diff --old manifests/2026-06-14.tsv --names-only \
    --filter icon | head                                                  # pipe-friendly

# 3. Incremental extract: only ADDED/MODIFIED paths vs the baseline, still
#    honouring --filter/--ext/--convert.
./target/release/paz extract -o /mnt/d/paz-extraction \
    --changed-since manifests/2026-06-14.tsv --filter icon --convert
```

**Weekly workflow** (fresh PAZ set dropped at `-i`):

```bash
NEW=manifests/$(date +%F).tsv
paz manifest -o "$NEW"                                     # fingerprint the new set
paz diff --old manifests/<last-baseline>.tsv --new "$NEW" --count   # what moved?
# re-extract only the delta for each relevant bucket, e.g. skill + cash icons:
paz extract -o /mnt/d/paz-extraction \
    --changed-since manifests/<last-baseline>.tsv \
    --filter icon/new_icon --convert
# "$NEW" becomes next week's baseline.
```

Dated baselines live in `manifests/` (gitignored — see that note below).

### Windows 11 single `.exe`

**Option A — MinGW cross-compile from WSL (no Visual Studio):**
```bash
sudo apt-get install -y gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/paz.exe   (self-contained, no runtime)
```

**Option B — native MSVC (build on Windows):**
```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```
Either produces a single static `paz.exe`; no runtime to install. The release
profile is tuned for size/speed (LTO, `panic=abort`, stripped).

---

## Output → Caphras data flow

The extractor writes a virtual-path-preserving tree under `-o <out>`, e.g.
`out/character/texture/<name>.png`, `out/<...>/<table>.xml`.

Proposed convention (gitignored — never commit game binaries):

* **Images** (DDS/PNG, e.g. item icons, map tiles): convert to PNG and stage
  under `scraper-assets/` (or push to our CDN). Path mirrors the in-archive
  path, so de-duplication and incremental sync are trivial.
* **Data/text** (XML/TXT/DBSS tables, per-locale strings): stage under
  `scraper-data/`. A follow-up transform step (separate, app-side) parses the
  relevant tables into the JSON/CSV shapes Caphras consumes.

Sibling stores referenced for staging:
`../scraper-data` and `../scraper-assets`.

---

## Limitations & next steps

* **Proprietary inner formats** (`pac/paa/pam/pae`, `bnk/wem`) are extracted as
  raw blobs only — decoding meshes/animation/audio is out of scope.
* **DDS conversion** currently relies on the `dds-rs` decoder (works for the
  common BC formats seen); exotic DXGI formats may fall back to raw `.dds`.
* **Non-ASCII filenames**: a few paths contain EUC-KR Korean bytes; they are
  written using the raw bytes (lossy UTF-8), which is harmless on most FS but
  could be normalised in a future pass.
* **Per-file index granularity**: `stats`/`list` re-scan all archive indices
  (~40 s on a 9p `/mnt/d` mount, ~2.5 s CPU). A cached on-disk index would make
  repeated queries instant; not yet implemented.
* **No `.dbss` payload decode** beyond raw extraction (they are plaintext blobs).

---

## Credits / sources

Format understanding was reconstructed from public, open-source BDO unpackers and
the quickbms community script. None of their code is copied here; this is an
independent Rust implementation.

* kukdh1 / sibercat — PAZ-Unpacker (format + ICE key + decompress logic, studied)
* AMGarkin — UnPAZ (format cross-reference)
* Matthew Kwan — ICE cipher (public-domain algorithm)
* quickbms `blackdesert` script — LZ77 control scheme reference

## License

MIT (this implementation). The ICE algorithm is public domain.
