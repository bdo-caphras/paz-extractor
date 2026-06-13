# Change-tracking manifests

Dated baselines produced by `paz manifest -o manifests/<YYYY-MM-DD>.tsv`. Each is
a deterministic, path-sorted TSV fingerprint of the **entire** PAZ set at one
game build:

```
path<TAB>paz_id<TAB>crc<TAB>orig_size<TAB>comp_size
```

The `(crc, orig_size)` pair is the change key — it comes straight from each PAZ
index, so a manifest is generated with **no decompression** (~45 s, same cost as
`paz index`). `paz diff` compares two of these to classify every path as
ADDED / MODIFIED / REMOVED / UNCHANGED, and `paz extract --changed-since` uses
that to re-extract only the delta after a patch. See the repo `README.md`
("Change tracking & incremental extraction") for the full weekly workflow.

## Why the `*.tsv` are gitignored

A manifest is ~68 MB (834,727 lines) of **derived, regenerable** data. Committing
it would bloat the repo and produce unreadable 68 MB diffs on every game build.
This follows the existing `paz-index.txt` precedent (also gitignored). Keep the
dated baselines here on disk for week-to-week comparison; this `README.md` is the
only tracked file in the directory.

Tradeoff: the durable history of baselines is **local-only**. If a shared,
versioned history is later wanted, a manifest compresses extremely well
(sorted text → `zstd`/`gzip` is ~5–8×, i.e. under ~12 MB), so committing
`*.tsv.zst` would be the next step — but that is deferred until there's a need.

## Baselines on disk

| date | lines | size | notes |
|------|------:|-----:|-------|
| 2026-06-14 | 834,727 | 68 MB | first baseline (10,832 archives) |
