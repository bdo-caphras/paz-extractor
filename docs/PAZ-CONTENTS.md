# What the `.PAZ` archives contain — full taxonomy & extraction verdict

This is the deep reference for the contents of the Black Desert Online `.PAZ`
game archives, as inventoried by `paz-extractor`. It answers two questions:

1. **Did the prior extraction cover everything, or just a sample?**
2. **What is every other (non-extracted) bucket for, and is it useful for
   Caphras?**

Companion docs:

* `../README.md` — the tool, the PAZ container format, build/run.
* `/mnt/d/paz-extraction/README.md` — the curated 9-category output tree
  (path patterns, per-category usage, "wire it into Caphras" plan).

All figures below come from the tool's own inventory of the **live set on
`/mnt/d/caphras-paz`** (`paz stats` / `paz categories`, plus the cached
`paz-index.txt` of all 834,727 virtual paths). Numbers are exact, not sampled.

---

## 0. TL;DR — sample vs. full

**The inventory is the FULL set; the *body extraction* was a deliberate, curated
slice (~8% of files).**

* `paz stats` reports `paz files = 10832`, **`indexed paz = 10832`** — every
  archive's index was parsed. `total files = 834727`, `122.48 GiB` decompressed.
* The cached `paz-index.txt` contains **834,727 lines** — one per virtual path
  across all 10,832 archives. Nothing was sampled at the *index* level.
* The 9 curated categories in `/mnt/d/paz-extraction` total **68,850 files
  (8.25% of files)** — these are the file *bodies* actually decoded/written.
  The remaining **765,877 files (91.75%)** were intentionally NOT extracted as
  bodies: they are overwhelmingly 3D models, animation, audio, video, effects
  and engine binaries (see §3), which are not exploitable for a web tools site.

So: we went through the **entirety** of the archive *catalogue* and understood
what each bucket is; we then extracted only the buckets that are useful for
Caphras. The "other data" is accounted for below — almost all of it is
understood-and-skipped, with a small genuinely-unknown residue called out
honestly in §4.

---

## 1. Container structure (recap)

* `pad00000.meta` (plaintext): `[u32 version=0x0d0e][u32 paz_count]` then
  per archive `{u32 paz_id, u32 crc, u32 size}`.
* `PAD#####.PAZ`: `[u32 crc][u32 file_count][u32 path_block_len]`, then
  `file_count` × 24-byte records `{crc, folder_id, file_id, offset, comp_size,
  orig_size}`, then an **ICE-encrypted**, NUL-separated string table. Full path
  = `table[folder_id] + table[file_id]`.
* **Encryption:** ICE (Matthew Kwan), level 1, 8 rounds, 8-byte ECB blocks.
  Static key `51 F3 0F 11 04 24 6A 00` decrypts the entire current global live
  set — filename table and every payload **except `.dbss`** (stored plaintext).
  No per-build / patch-specific key blocks any of this.
* **Compression:** custom BDO LZ77 (payload byte 0 = flag; bit0 compressed,
  bit1 long 32-bit header).

Full format details and the clean-room derivation are in `../README.md`.

---

## 2. Top-level layout (where things live)

By top-level directory (file counts from `paz-index.txt`):

| top dir | files | what it is |
|---------|------:|------------|
| `character/` | 242,647 | character/monster/NPC models, motion, textures, AI scripts, action charts, cutscenes |
| `mapdata_real/` | 184,330 | world map: sector/terrain data, terrain-color & thumbnail tiles, probes, HLOD, **spawn placement**, occluders, navigation |
| `ui_texture/` | 150,569 | all UI textures: **item/skill/quest icons**, packed atlases (`combine`), artwork, **world-map / region icons & crests** |
| `sound2022/` | 90,572 | audio banks (`bnk`) + streamed waves (`wem`) |
| `object/` | 79,876 | placed world objects: meshes (`pam`), textures (`dds`), positional audio (`pcm`) |
| `ui_data/` | 29,415 | **`ui_html/xml` item/codex data + locale strings**, plus UI window/widget layouts, fonts |
| `effectbin/` | 24,610 | compiled visual-effect blobs (`pae`/`paem`) |
| `effect/` | 9,744 | effect source/aux assets |
| `speedtreedata/` | 6,202 | SpeedTree foliage/tree data |
| `gamecommondata/` | 6,176 | **region client data**, customization presets, waypoints, dialog/trigger/dungeon-event config, `dbss` binary tables |
| `luacscript/` | 3,242 | compiled UI Lua (`luac`) |
| `mapdata_instancedungeon/` | 2,406 | instanced-dungeon map data |
| `sequence/` | 1,889 | cutscene/cinematic sequence data |
| `texture/` | 1,571 | misc shared textures |
| `ui_movie/` | 923 | UI videos (`bk2`/`webm`) + subtitles (`srt`) |
| `fxo*` / `ui_customize/` / `mapdata_common/` / `effect/` … | <300 each | shaders, customization, shared/common map data |

---

## 3. Full file-type taxonomy + Caphras verdict

Counts and bytes are from `paz categories` (decompressed sizes). "Verdict" is
one of:

* **EXTRACTED** — written into one of the 9 curated categories.
* **SKIPPED (not useful)** — understood; deliberately not extracted because it
  has no value for a BDO web tools site.
* **TODO (potentially useful)** — understood; not yet extracted but could matter.
* **UNKNOWN** — format/purpose not positively identified (honest residue).

### 3.1 Textures & images

| ext | count | bytes | format / purpose | verdict |
|-----|------:|------:|------------------|---------|
| `dds` | 292,719 | 41.9 GB | BC-compressed textures: item/skill/quest/UI icons, map terrain tiles, region crests, model skins | **partly EXTRACTED** (icons, crests, symbolicons, world-map node/monster icons, terrain tiles → PNG). The vast majority are model/world skins → **SKIPPED**. Skill icons (`04_pc_skill`, 4,609) & cash icons (`09_cash`, 24,293) are **TODO** (id-keyed, drop-in like item icons). |
| `combine` | 53,573 | 5.9 GB | packed UI texture atlases (sprite sheets) | **TODO** — could be unpacked for extra UI sprites, but the icons we need already exist un-atlased; low priority. |
| `png` | 16,927 | 0.6 GB | ready-made images, mainly `product_icon_png` item icons | **EXTRACTED** (product PNGs into item-icons). Remainder are scattered UI pngs → SKIPPED. |
| `bmp` `jpg` `tga` `svg` `hdr` | ~70 | small | stray UI/branding images, a couple logos, HDR env maps | SKIPPED (branding/engine). |

### 3.2 Models, meshes, animation (3D engine assets)

| ext | count | bytes | format / purpose | verdict |
|-----|------:|------:|------------------|---------|
| `pac` | 92,280 | 28.4 GB | proprietary compiled model/mesh container | **SKIPPED** (raw blob; 3D, not web-exploitable). |
| `paa` | 69,151 | 9.3 GB | proprietary animation data | SKIPPED. |
| `pam` | 58,890 | 8.2 GB | proprietary mesh/model (objects, terrain meshes) | SKIPPED. |
| `pae` `paem` | 24,611 | 1.18 GB | compiled visual-effect blobs (effectbin) | SKIPPED. |
| `paac` | 7,902 | 1.82 GB | animation-clip container | SKIPPED. |
| `pah` | 4,241 | 1.5 GB | model/physics helper blob | SKIPPED. |
| `pab` `pad` `pat` `pas` `paseqfe` `ipam` `paap` `pami` `pm` `pa` `pc` `ph` `r3m` `lod` `pab`… | ~5k total | a few GB | the rest of the `pa*` / mesh / LOD model family | SKIPPED (3D engine). |
| `speedtree*` (in `speedtreedata/`) | 6,202 files | — | SpeedTree foliage data | SKIPPED (3D foliage). |
| `collisiondata2` `barrier` `occluder` `navigation` `tome` `bss` `bwp` `vnl` `vnm` `probe` `hloddata` | tens of k (mostly under `mapdata_real`) | several GB | server/engine map geometry: collision, barriers, occlusion, navmesh, baked light/probe, terrain HLOD | **SKIPPED** (engine-internal world geometry; the one *gameplay* slice we need — **spawn placement** — is extracted, see §3.5). |

### 3.3 Audio & video

| ext | count | bytes | format / purpose | verdict |
|-----|------:|------:|------------------|---------|
| `bnk` | 86,283 | 9.2 GB | Wwise sound banks | **SKIPPED** (audio, proprietary). |
| `wem` | 4,283 | 3.9 GB | Wwise streamed audio | SKIPPED. |
| `pcm` | 14,156 | 89 MB | positional/object audio (under `object/`) | SKIPPED. |
| `bk2` | 210 | 8.99 GB | Bink2 video (cutscenes/intros) | SKIPPED (video). |
| `webm` | 207 | 2.07 GB | WebM video | SKIPPED. |
| `srt` | 2,774 | 375 MB | subtitle tracks for the videos | SKIPPED (tied to video). |

### 3.4 Game data — XML (the high-value tabular data)

37,758 XML files, 1.53 GB. By location:

| location | count | content | verdict |
|----------|------:|---------|---------|
| `ui_data/ui_html/xml/en/*` | 24,060 | per-item codex data: 6,015 EN (plain) + 6,015 each `_fr_`/`_de_`/`_sp_` prefixed. Schema: `<itemInfo>` itemKey/itemName/itemIcon/itemDesc, `<shop>` vendors, `<house>` recipes | **EXTRACTED** → `item-data/{en,fr,de,sp}` |
| `ui_data/ui_html/xml/[0-9]+.xml` | 3,424 | KR-default item codex (numeric ids) | **EXTRACTED** → `item-data/kr` |
| `ui_data/ui_html/xml/{itemmaking,string}.xml` | 2 | master recipe table + string table | **EXTRACTED** → `item-data/` |
| `gamecommondata/regionclientdata*.xml` | 19 | region key↔name (all locales) + region attributes (`_en_` ≈ 22 MB) | **EXTRACTED** → `region-data/` (305 MB) |
| `mapdata_real/spawnplacement/**/*_monster.xml` | 3,638 | monster spawn geometry: `<Position X Y Z>`, `SpawnCharacterKey`, quantity, respawn | **EXTRACTED** → `spawn-geometry/` |
| `gamecommondata/customization/*.xml` | 1,091 | character-creator presets (face/body/hair) | **TODO** — could feed a character-creator reference; low priority. |
| `ui_data/window/*.xml` | 1,154 | UI **window layouts** (not content) | SKIPPED (UI layout, not data). |
| `ui_data/widget/*.xml` | 535 | UI **widget layouts** — incl. the only "quest"/"knowledge" XML, which are *layouts not content* | SKIPPED (UI layout). See §5 limitation. |
| `gamecommondata/waypoint/*.xml` | 456 | NPC/transport waypoint paths | **TODO** — could support a node/carriage route map; medium interest. |
| `gamecommondata/dialogscene/*.xml` | 238 | dialog scene scripting | SKIPPED (cinematic scripting). |
| `gamecommondata/trigger/*.xml` | 128 | world trigger volumes | SKIPPED (engine). |
| `gamecommondata/clientdungeonevent/*.xml` | 11 | dungeon event config | SKIPPED. |
| `character/**/*.xml` | 1,652 | per-character config (uvanimation, cloth, dynamics) | SKIPPED (3D rig config). |
| `sequence/*.xml` | 937 | cutscene sequence definitions | SKIPPED (cinematics). |
| `mapdata_real/*.xml` (non-spawn) | ~121 | weather color tables, nav-water regions, occluders | SKIPPED (engine/world). |
| other scattered xml | ~hundreds | font/actor/lobby/loading UI config | SKIPPED. |

### 3.5 Map / spawn data (non-XML)

| ext | count | bytes | format / purpose | verdict |
|-----|------:|------:|------------------|---------|
| `mapdata` | 31,453 | 0.95 GB | binary sector/terrain map data | **SKIPPED** (binary world data; the gameplay slice we need is the spawn XML, already extracted). |
| terrain-color `dds` (under `mapdata_real/terraincolortexture` + `worldmap_terrain_color_*`) | 50,973 + tiles | — | per-sector terrain color textures + the **world-map terrain tile pyramid** | **map-terrain-tiles EXTRACTED** (the 96-tile world-map pyramid → PNG). The 50k per-sector terrain textures are in-engine ground textures → SKIPPED. |
| `spawnplacementfieldgrid` | 1,373 | — | spawn field grid metadata (pairs with spawnplacement) | **TODO** — complements spawn-geometry for density gridding. |

### 3.6 Scripts, shaders, fonts, misc

| ext | count | bytes | format / purpose | verdict |
|-----|------:|------:|------------------|---------|
| `luac` | 3,242 | 79 MB | compiled Lua 5.1 (UI logic) | **SKIPPED** (needs a Lua decompiler; logic, not data). |
| `ai` | 7,017 | 124 MB | monster/pet AI behaviour scripts (text) | **TODO/low** — readable AI scripts; niche (could mine aggro/patrol behaviour), not core. |
| `binaryactionchart` (`bss`/`bin` under `character`) | thousands | — | compiled skill/action charts (frame data) | **TODO/hard** — skill frame data is interesting but binary/undocumented → effectively UNKNOWN schema. |
| `dbss` | 375 | 622 MB | **plaintext** string/binary tables (`gamecommondata/binary/`) | **TODO** — raw-extractable, plaintext, but per-table schema is unknown; could hold useful master tables. Worth a probing pass. |
| `txt` | 344 | 767 MB | text lists, AI scripts, tattoo/cutscene lists, sector strings | mostly SKIPPED; a few (tattoo lists, sector strings) low value. |
| `fxo` `fxo10` `fxo11` `cl` `chroma` | ~400 | — | compiled shaders / color-grading | SKIPPED (renderer). |
| `ttf` `otf` | 35 | 164 MB | UI fonts | SKIPPED (licensed fonts; do not redistribute). |
| `html` `js` `css` | 79 | small | the legacy in-client HTML UI shell for the codex | SKIPPED (the data we want is the sibling XML, already extracted). |
| `weathercolortablexml` `light` `volumefog` `procedural` `temp` `object` `data` `tree` `rid` `db` `exe` `pc` `fcb` `col` | ~700 | varies | engine/world tuning, two stray `exe`, db caches | SKIPPED. The handful of odd one-off extensions (`fcb`, `rid`, `procedural`, `volumefog`, `temp`, `light`, `col`, `tree`) are **UNKNOWN** in exact format but clearly engine-internal. |

---

## 4. Accounting — is everything covered?

* **Files:** 834,727 total. Extracted as bodies: 68,850 (8.25%). The other
  91.75% is classified above; the overwhelming majority is positively
  identified as 3D model / animation / audio / video / effect / engine-geometry
  (SKIPPED) or UI-layout / shader / font (SKIPPED).
* **Genuinely UNKNOWN residue:** a *small* set of one-off / rare extensions
  whose exact binary schema we have not reverse-engineered — `binaryactionchart`
  (skill frame data), the various `pa*` sub-variants beyond the obvious
  model/anim ones, and the long tail of single-digit-count extensions
  (`fcb`, `rid`, `procedural`, `volumefog`, `temp`, `light`, `col`, `tree`,
  `db`). Together these are **well under 1% of files**, and all are clearly
  engine-internal (none look like web-exploitable tabular data). The `dbss`
  (375, plaintext) is the one "unknown-but-maybe-useful" data bucket worth a
  future probe (see §5).
* **Conclusion:** there is no large unexplained bucket. The "other data" is the
  game engine's 3D/audio/effect payload plus UI plumbing — understood, and
  correctly judged not useful for a BDO web tools site.

---

## 5. Useful-but-not-yet-extracted (consolidated TODO)

Ordered by likely value to Caphras. **Items 1–6 were extracted in the
2026-06-14 baseline delta run** (see counts/sizes below); only the UI atlases
remain a TODO.

1. **Skill icons** — `ui_texture/icon/new_icon/04_pc_skill` (4,609; 4,608 DDS +
   1 PNG), id-keyed exactly like item icons. Drop-in for a skill/build tool.
   `paz extract -o … --convert --filter icon/new_icon/04_pc_skill`
   → **EXTRACTED** 4,608 PNG, 28 MB.
2. **Cash-shop icons** — `09_cash` (24,293 DDS), same keying; large but trivial.
   → **EXTRACTED** 24,293 PNG, 404 MB.
3. **`dbss` plaintext tables** — `gamecommondata/binary/*.dbss` (375, ~622 MB,
   not encrypted). Schema unknown but plaintext → cheap to probe; may hold
   master gameplay tables. → **EXTRACTED** 373 (2 hit a stored-payload
   decompression edge case), 594 MB.
4. **Spawn field grid** — `spawnplacementfieldgrid/*` complements the extracted
   spawn geometry for density heatmaps. The bare `--filter spawnplacementfieldgrid`
   matches **1,505** files: 1,373 under `mapdata_real/` (as documented) **plus**
   132 instance-dungeon grids under `mapdata_instancedungeon/`. → **EXTRACTED**
   1,505, 18 MB.
5. **Waypoints** — `gamecommondata/waypoint/*.xml` (456) for node/carriage routes.
   → **EXTRACTED** 456, 82 MB.
6. **Customization presets** — `gamecommondata/customization/*.xml` (1,091) for a
   character-creator reference. → **EXTRACTED** 1,091, 25 MB.
7. **UI atlases** — `combine` (53,573) only if a needed sprite is found only
   atlased; the icons we use already exist un-packed. → still **TODO** (skipped:
   low value, the icons we need exist un-atlased).

**Honest data gaps (not in the archives as content):**

* **Quest & knowledge master tables** are absent — the only quest/knowledge XML
  are UI widget *layouts* (`ui_data/widget|window/**`), not the content tables.
* **Fishing-zone polygons** are not in plain XML — fishing assets are AI scripts,
  models and action charts; the zone geometry is proprietary binary.
* **Models / animation / audio / video / effects** decode is out of scope (raw
  blobs only).
* **`luac`** UI logic is compiled Lua (needs a decompiler).

---

## 6. Reproduce the inventory

From `paz-extractor/` (binary at `target/release/paz`, default input
`/mnt/d/caphras-paz`):

```bash
paz stats         # 10832 / indexed 10832 / 834,727 files / 122.48 GiB
paz categories    # exact count + decompressed bytes per extension
paz index -o paz-index.txt           # cache all 834,727 virtual paths
paz search <terms…> [--ext …] [--count]   # instant AND-search over the cache
paz extract -o <out> --filter <substr> [--ext …] [--convert] [--limit N]
```

The curated output tree and per-category integration plan live at
`/mnt/d/paz-extraction/README.md`.

---

## 7. Change tracking, diff & incremental extraction

The set is re-patched roughly weekly. Re-extracting everything each time is
wasteful, so the tool fingerprints the whole set and re-extracts only the delta.

**The change key is free.** Every 24-byte file record in a PAZ index already
carries the game's own per-file `crc` and the decompressed `orig_size`. The pair
`(crc, orig_size)` per virtual path is a robust change key that needs **no
decompression** — so building a full fingerprint costs the same as `paz index`
(~45 s), not a full extract.

### Manifest

`paz manifest -o manifests/<YYYY-MM-DD>.tsv` writes a deterministic,
path-sorted TSV with a header:

```
path<TAB>paz_id<TAB>crc<TAB>orig_size<TAB>comp_size
```

Path order is lexicographic (byte order on the lossy-UTF-8 path), so two
manifests `diff` cleanly and regenerating the same set is byte-identical. The
2026-06-14 baseline is **834,727 lines, ~68 MB**.

### Diff

`paz diff --old <baseline> [--new <manifest> | live scan of -i]` classifies
every path:

* **ADDED** (`+`) — in new, not old.
* **MODIFIED** (`~`) — path in both, but `crc` *or* `orig_size` differ.
* **REMOVED** (`-`) — in old, not new.
* **UNCHANGED** — both, identical key.

`--ext a,b` / `--filter substr` restrict to a category; `--count` prints only
the tallies; `--names-only` prints just ADDED+MODIFIED paths for piping. If
`--new` is omitted, the current manifest is built in-memory from `-i`.

### Incremental extract

`paz extract --changed-since <baseline>` extracts **only** ADDED/MODIFIED paths
vs that baseline, still honouring `--filter` / `--ext` / `--convert`. This is the
weekly delta path:

```bash
NEW=manifests/$(date +%F).tsv
paz manifest -o "$NEW"
paz diff --old manifests/<last>.tsv --new "$NEW" --count
paz extract -o /mnt/d/paz-extraction \
    --changed-since manifests/<last>.tsv --filter icon/new_icon --convert
```

### Where baselines are stored

Dated baselines live in `manifests/` in the repo (with a tracked `README.md`).
The `*.tsv` files themselves are **gitignored** — 68 MB of derived, regenerable
data would bloat the repo and produce unreadable diffs (same precedent as
`paz-index.txt`). They are the local, durable history for week-to-week
comparison; `manifests/README.md` documents the tradeoff and the
`*.tsv.zst`-commit option if a shared history is later needed.
