# Testing and verification

How Kobo is checked: how to run the tests and what each tier needs, what a run is
checked against, and then each check, its data, and how that data is made. Results of
past runs, with their dates, are in [testing-log.md](testing-log.md); what a rendered
level does not reproduce is in [known-gaps.md](known-gaps.md).

## Running the tests

- `cargo test --workspace` runs every test. Unit tests and the synthetic ones (no ROM)
  always run; every other test needs data that is never committed and skips without it
  (the tiers, below). Kobo-core is optimised in a dev build (`Cargo.toml`), keeping debug
  assertions and overflow checks, so the tests that run the ROM's own code are about as
  fast as in release; `CARGO_PROFILE_DEV_PACKAGE_KOBO_CORE_OPT_LEVEL=0` gives a debugger
  an unoptimised build.
- `cargo xtask verify` (crates/xtask) runs what CI runs and more: `cargo fmt --check`,
  clippy with warnings denied, `cargo xtask lint-tools` (the scripts in `tools/`), a table
  of what this machine has for each tier, the tests, and at the end every test that
  skipped, by tier. libtest hides a passing test's output, so a skip is invisible
  otherwise: each skip is appended to the file `KOBO_SKIP_LOG` names, which verify sets
  and `cargo xtask skips FILE` reports. `--strict` requires every tier this machine has
  set, so a test that still skips fails; `--require-all` requires every tier; `--full`
  turns on the checks of every level. Arguments after `--` go to the test binaries.
- `cargo xtask tiers` prints each tier, where it is set, and whether its paths exist.
- CI (`.github/workflows/ci.yml`) runs fmt, clippy, `lint-tools --strict`, the licence
  check, and the tests on Linux, Windows, and macOS, with the pinned Asar, PIXI, UberASM
  Tool, and SA-1 Pack required and no ROM, so it shows the ROM-backed tests skipping.
- A test that fails in a corpus hack names the hack, the level, and what differs; a
  seeded test names its seed (`KOBO_LOADER_SEED`, `KOBO_EXANIM_SEED`). `KOBO_KEEP_TEMP=1`
  keeps a failed test's scratch folders.
- To trace a vanilla test (`KOBO_CPU_TRACE`, below), run it alone: `cargo test --test
  <binary> <name>`. Loading a ROM Lunar Magic saved turns instruction-level output off
  for the rest of the process (`kobo_core::clean_room`), and any test in a binary may load
  one; `tests/clean_room_process.rs` checks the switch, and the tests written around one
  corpus hack are a binary of their own (`tests/corpus_hacks.rs`).

## Tiers

Each tier is set by its environment variable, then by the config file
(`~/.config/kobo/config.toml` on Linux): `roms.smw`, `tools.<key>`, or the `[tests]`
table, whose paths may start with `~/` (`kobo_core::tiers`, and `tools/kobo_config.py`
for the scripts). `KOBO_REQUIRE_<TIER>` (the name below in capitals) makes a missing tier
a failure instead of a skip, and `KOBO_REQUIRE_ALL` every tier; a tier that is set but
names nothing, or a malformed configuration, is an error either way.

| Tier | Variable | Config key | What it enables |
|---|---|---|---|
| `rom` | `KOBO_SMW_ROM` | `roms.smw` | every ROM-backed test (the vanilla ROM, checked by its SHA-1) |
| `asar` | `KOBO_ASAR_LIB` | `tools.asar` | Kobo's patches and builds; else the pinned build, once `kobo tools fetch` cached it |
| `pixi`, `uberasm` | `KOBO_PIXI`, `KOBO_UBERASM` | `tools.pixi`, `tools.uberasm` | those tool stages; else the pinned builds |
| `sa1pack` | `KOBO_SA1PACK` | `tools.sa1pack` | SA-1 builds, and every build test's SA-1 variant; else the pinned release |
| `gps`, `addmusick` | `KOBO_GPS`, `KOBO_ADDMUSICK` | `tools.gps`, `tools.addmusick` | those tool stages (never bundled) |
| `lunar_magic` | `KOBO_LUNAR_MAGIC` | `tests.lunar_magic` | save checks, exports, Kobo's code against Lunar Magic's (Linux, Wine) |
| `lm_roms` | `KOBO_LM_ROMS` | `tests.lm_roms` | every corpus test: ROMs, `.bps` patches, or folders of them |
| `mwl` | `KOBO_MWL_DIR` | `tests.mwl_dir` | MWL round trips, size tables against the exports |
| `oracle` | `KOBO_ORACLE_DIR` | `tests.oracle_dir` | emulator dumps (and `KOBO_ORACLE_ROM`, `tests.oracle_rom`, their ROM) |
| `boss_oracle` | `KOBO_BOSS_ORACLE_DIR` | `tests.boss_oracle_dir` | the boss arenas' dumps |
| `video_oracle` | `KOBO_VIDEO_ORACLE_DIRS` | `tests.video_oracle_dirs` | whole pictures against emulator frames |
| `65816_tests` | `KOBO_65816_TESTS` | `tests.cpu_tests` | SingleStepTests, every 65816 opcode |
| `sa1_reference` | `KOBO_SA1_REFERENCE` | `tests.sa1_reference` | the SA-1 reference ROM's picture baseline, and its bytes for `examples/swap.rs` |

The checks of every level are switched on by `KOBO_FULL_RENDER=1` or `tests.full_render
= true`. This machine's configuration, as an example (the corpus folders are listed as
they are laid out; a folder gives its ROMs and the patches not applied beside them):

```toml
[roms]
smw = "/path/to/Super Mario World.smc"     # `~/` is for the [tests] paths only

[tests]
lm_roms = [
    "~/.local/share/kobo/roms",
    "~/.local/share/kobo/roms/corpus_more",
    "~/.local/share/kobo/roms/QLDC 2021",
    "~/.local/share/kobo/roms/QLDC 2022",
]
lunar_magic = "~/.local/share/kobo/tools/lunar-magic-3.70"
mwl_dir = "~/.local/share/kobo/mwl"
sa1_reference = "~/.local/share/kobo/roms/sa1/smw-sa1.sfc"
```

The corpus folder holds the vanilla ROM too, which the corpus tests leave out. The QLDC
folders make a full run about an hour; leave them out for a quicker one.

## What a run is checked against

- **Known failures**: `tests/fixtures/known_failures.toml` lists what each corpus hack is
  known to fail, by headerless SHA-1, test, and level, with why. A corpus test collects
  every failure over every hack (`tests/common/failures.rs`) and fails on one the list
  does not hold, and on a listed one that now passes on a hack it ran on, so the list
  shrinks as gaps are fixed; known failures that still fail are printed, a line a hack.
  `common::failures::catch` turns a check written as assertions into one failure per
  level.
- **Picture baselines**: `tests/fixtures/render_hashes/` holds every level's picture
  hashes (`examples/render_hashes.rs`) of the vanilla ROM and of the SA-1 reference ROM,
  and `tests/render_baselines.rs` requires them. A change meant to change pictures writes
  them again with `cargo xtask baseline`, which says which levels changed; the diff shows
  them in review.
- **The checks of every level** (`KOBO_FULL_RENDER`, `verify --full`): the tests that
  compare a few levels' pictures of a build with its base compare all 512, drawn and as
  markers (`common::every_level_draws_the_same`): the vanilla import built back, Kobo's
  patches on the vanilla ROM and the SA-1 base, the SA-1 base's import built back, and an
  SA-1 build's GFX as LC_LZ3. A listed exception must still differ. Each takes a minute
  or two.
- **The corpus sweep's baseline**: `tools/corpus-sweep --baseline old/results.json`
  (below) lists what got worse since an earlier sweep.

## Checks

### Emulator oracle

- **Emulator oracle** (`tools/oracle/`): `dump.sh <rom> <outdir> 105,106,...` runs Mesen 2
  headlessly, navigates to each level through the file select, and dumps the tile grid,
  CGRAM, VRAM, header RAM, and the sprite slot tables on the first level frame. The script
  zeroes work RAM, save RAM, video memory, and an SA-1's I-RAM before the game starts, as
  `expand`'s machine starts, whatever power-on state Mesen is set to (random by default):
  dumps repeat byte for byte, VRAM included, and hacks that read memory they never wrote
  behave the same each run.
  `tests/oracle_levels.rs` compares `expand::expand_level` against a dump directory (the
  `oracle` tier); the dumps are of the vanilla ROM unless `KOBO_ORACLE_ROM`
  (`tests.oracle_rom`) names another. All 512 vanilla levels match byte for byte in the tile grid, and in which sprite
  is in which slot (the sprite and its place, not whether it is alive: the dump comes a
  frame or so into the level, and a sprite spawned beyond the despawn range is erased and
  spawned again as the loader comes round). Dumps live in `~/.local/share/kobo/oracle/`
  and are never committed. `trace_writes.lua` logs who writes a RAM address, for debugging,
  and refuses a ROM Lunar Magic saved (clean room): run it as `dump.sh` runs its script,
  `Mesen --testRunner tools/oracle/trace_writes.lua rom.sfc` under `xvfb-run -a`, with
  `KOBO_ORACLE_LEVELS=102 KOBO_TRACE_ADDR=7EE400 KOBO_ORACLE_OUT=dir`. `dump.sh` gives
  Mesen `KOBO_ORACLE_TIMEOUT` seconds (900). `dump.sh` first checks, with
  `tools/clean_room.py sites`, that the three addresses the script stops at are the game's
  instructions or where a hook starts, and the WRAM a video dump writes has the stack page
  zeroed.
  A level the game itself cannot load ends the script with `stuck in stage level (game
  mode $xx)` and a garbage mode: that, with the same routine failing in `expand`, is how a
  hack's own defect is told from a bug here (QLDC 2021 `34_idol`, nine slots). On the
  `expand` side, `KOBO_CPU_TRACE=<n>` prints the last `n` instructions of the CPU (either
  one, on an SA-1 ROM), with registers, before any routine fails, fatal or not.
  `KOBO_RAM_WATCH=$40D5D5` reports every write to a bus address with the instruction that
  made it, and the `n` before it when the trace is on. Once a ROM Lunar Magic saved is
  loaded (`kobo_core::clean_room`) the trace is off for the process and a watch reports
  the write alone, or nothing of a write to the stack. `KOBO_VRAM_WATCH=$3A1E` does the same for a
  VRAM word, naming the DMA source.
  A full run of the vanilla ROM takes 20 minutes and an SA-1 ROM emulates slower; four
  runs of 128 levels each into separate directories, moved together afterwards, take a
  quarter of that.
  `KOBO_ORACLE_VIDEO=1 dump.sh ...` instead waits for visible video and also writes PPM,
  full WRAM, and PPU state. Keep these later-frame captures in a separate directory. The
  screen buffer the script reads is a frame or two behind the PPU state, which of the two
  varying from run to run, so it waits for four frames at full brightness
  (`KOBO_ORACLE_VISIBLE_FRAMES=<n>` waits for another count, to see a level's first frames
  one by one).
  The `boss_oracle` tier (`KOBO_BOSS_ORACLE_DIR`) enables stable boss graphics comparisons for levels 096, 0CC,
  0D9, and 1C7 (Mode 7 characters, layer 3 GFX, arena tilemap, SP3); either capture mode
  works. The loader override must only run in game mode `$11` (overriding the
  title-screen load in mode `$03` contaminates the graphics cache).
  `tests/video_oracle.rs` compares whole rendered pictures (with sprites) against the
  PPM frames of `KOBO_ORACLE_VIDEO=1` dumps listed in the `video_oracle` tier
  (`KOBO_VIDEO_ORACLE_DIRS`, `:`-separated), cropped at the camera from the dumped WRAM (`$1A`/`$1C`; the PPU
  scroll registers keep ten bits, too few for vertical levels) and below the status bar.
  Mesen's frame is 239 lines with the picture 6 rows down; the test tries paddings 4-9.
  Agreement is 95-99.9% on most levels; tides that have moved and sprites that have
  animated account for the rest, and the threshold is 85%. Captures live in
  `~/.local/share/kobo/oracle/layer3-video/`, `colormath-video/`, and `entry-video/` (the
  levels SA-1 Pack changes, the boss arenas, and vertical levels `0DB`, `12A`, and `1ED`).
  In the `1D4` and `1D9` captures the picture shows no OAM object at all (no Mario, no
  candle flames) although the dumped OAM holds them where the player pass puts them; treat
  those two frames' object layer as unreliable. A vanilla boss arena's frame sometimes comes
  without its background or objects (`098` or `0D9`, one or the other from run to run) and
  then falls just under the threshold; the SA-1 ROM's do not.
- **Emulator oracle on hacks**: `dump_hack.sh <vanilla> <hack> <outdir> [count]` picks the
  levels whose layer 1 pointer is not vanilla's and dumps `count` (12) of them, spread over
  the hack; `KOBO_ORACLE_ROM=<hack> KOBO_ORACLE_DIR=<outdir>` then runs
  `tests/oracle_levels.rs` on it. The script gets into a level in every hack of the corpus:
  besides the vanilla title screen and file select it handles a hack that boots straight
  into a level (`RHRS1C`), one that skips the intro level and starts on the overworld (it
  presses A on the level the player stands on: Super Diagonal Mario 2, Super Sheffy World
  2), and a "No Yoshi" intro that the ROM's tables would not predict (Grand Poo World 2
  plays it before levels of any tileset), which is why it watches the game choose the intro
  instead of predicting it. When it gives up it writes `stuck.ppm`, the screen it was on.
  Dumps live in `~/.local/share/kobo/oracle/hacks/`. With `KOBO_ORACLE_VIDEO=1` the same
  dumps give whole frames.
- **Per-sprite comparison on hacks**: `examples/sprite_oracle.rs` takes a ROM and
  directories of `KOBO_ORACLE_VIDEO=1` dumps of it, renders each level with sprites, and
  scores the pixels each captured sprite entry's objects cover against the frame, where
  the entry is on the emulator's screen, along with the whole visible picture; the final
  table is per sprite number and extra bits, worst first. It exists to find a custom sprite
  drawn with the wrong graphics, colours, or not at all, which scores far below one that has
  merely animated or moved.
- **SA-1**: the oracle script reads SA-1 Pack's RAM map (`ram()` in `dump_levels.lua` is
  `RamMap::Sa1Pack` for what it touches, and the full-WRAM dump is laid out as vanilla's)
  and hooks the pointer lookup on the SA-1 too, where the level loader runs. With
  `KOBO_ORACLE_ROM` on the reference ROM from [sa1.md](sa1.md), the tile grids of all 512
  levels, the layer 3 tilemaps, and the sprite slots match `sa1-all/`, and the 13 frames in
  `sa1-video/` match at 94.9-99.7%. `render_hashes` against the vanilla ROM is the other
  check: the marker column must match on every level but the three boss arenas, and the
  differences in the drawn column are SA-1 Pack's own ([sa1.md](sa1.md)). The corpus has
  40 SA-1 hacks: `Super Diagonal Mario 2`, `corpus_more`'s `Extended Interactions`, and 38
  QLDC 2021 and 2022 entries, which are BPS patches: the corpus tier takes them as they are,
  while `render_hashes` and the CLI need `kobo bps apply` first. `render_hashes` on each says whether the code ran,
  not whether the pictures are right; what fails is in [known-gaps.md](known-gaps.md).

### The 65816 core

- **CPU suite**: `tests/cpu_single_step.rs` runs the 65816 core against SingleStepTests
  (10,000 native-mode tests per opcode, about a second in release) with the `65816_tests` tier
  (`KOBO_65816_TESTS`, the suite's `v1` directory). The native files are in
  `~/.local/share/kobo/cpu-tests/65816/v1` (sparse clone of `SingleStepTests/65816`, 1.7 GiB,
  no licence, never committed). `BRK`, `COP`, `WAI`, and `STP` are left out (the core stops
  on them by design), and the suite's block moves are cut off after 100 cycles, so those
  are compared over the bytes moved. All other opcodes pass in full. SMW itself never sets
  decimal mode; custom code may.

### The vanilla ROM

- `tests/map16_sheet.rs` checks that the foreground Map16 a loaded vanilla level resolves
  (`LevelTiles::foreground_map16`, what `map16 png --level` draws) is the vanilla table of
  its tileset, and its background definitions the BG table. The sheet a level shows differs
  from the tileset's only where a tile references the animated area of VRAM (`$040`-`$07F`
  and the coin, water, and scenery frames), which the loaded level has as the first frame
  uploaded it, and in the palette entries the vanilla assembly leaves black (Mario's row).
- `tests/render_levels.rs` checks that vanilla level `105`'s dragon coins use the
  ROM's flashing yellow palette after the NMI, even with sprites and Mario hidden.
  It also changes the animation colours in an in-memory ROM copy to check that the
  capture follows those colours instead of substituting yellow in the renderer.
- **Decompression**: `tests/gfx_decompression.rs` decodes every GFX file of the pointer
  tables natively and has the ROM decompress the same file on the headless CPU
  (`expand::decompress_gfx_file`, the game's `PrepareGraphicsFile` with whatever routine a
  hack put behind it); the two must agree. It runs on the vanilla ROM and on the corpus,
  skipping locked ROMs. QLDC 2021 `34_idol` (a BPS patch) is the one LC_LZ3 hack in the
  corpus, so list its `.bps` to exercise that decoder; Lunar Magic's `-ExportGFX` of it is
  `fixtures/lunar_magic_gfx_export.txt`.
  Lunar Magic asks before it touches a headerless ROM, which a headless run never gets
  past: a patched BPS comes out headerless, so export from a copy with 512 zero bytes in
  front.
- **Compression**: `tests/lz2_compression.rs` recompresses the 52 vanilla GFX files with
  `compress::lz2::compress`, writes them over the originals in a copy of the ROM, and
  checks that the native decoder and the game's routine (the 50 table files through
  `decompress_gfx_file`, `GFX32` and `GFX33` through a load of level `105`) read them back,
  and that none is larger than Nintendo's (121,663 bytes against 130,317 in all). The unit
  tests check the parse against a brute-force search of every command, length, and source
  on small inputs.
- **Level data**: `tests/level_data.rs` decodes and encodes every level's object data,
  sprite list, and distinct background of the vanilla ROM and of every corpus hack
  but the locked ones, and requires the same objects, sprites, and tiles back, an encoding
  no longer than the stored one, and a stored length within the RATS block holding it. On
  vanilla all 538 object lists and 512 sprite lists but 18 object lists come out byte for
  byte ([smw.md](smw.md)). In the corpus, 70% to 100% of each ROM's lists do; the rest are
  Lunar Magic's encoding choices.

### Lunar Magic's exports and MWL files

- Lunar Magic exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
  The fixtures were made with 3.21; `tools/lunar-magic/fixture-lines` prints their lines
  for a ROM from Lunar Magic's own exports (hashes only), to check or extend them. Lunar
  Magic 3.70's `-ExportGFX` of the vanilla ROM differs in one file: `GFX17` has `$FF` where
  3.21 and `gfx export` have `$00`, in 32 bytes from offset `$11` (fixture-lines reproduces
  the Map16 fixture exactly and the GFX fixture but for that file). Regenerating the
  fixtures with 3.70 needs that explained first.
  3.70 ships a 64-bit build (`x64/Lunar Magic.exe`) that runs under 64-bit Wine alone.
- **MWL files**: `tests/mwl_files.rs` reads Lunar Magic's MWL exports (the `mwl` tier,
  `KOBO_MWL_DIR`), a directory of directories each holding one ROM (`.smc` or `.sfc`) and the MWL
  files of its levels, named `level NNN.mwl`. `tools/lunar-magic/export-mwl <outdir>
  rom...` makes them: it copies each ROM to `<outdir>/<name>/<name>.smc` and has Lunar
  Magic 3.70 `-ExportMultLevels` all 512 levels from the copy (flags 0), reporting a ROM
  it refuses. Every file must come back byte for byte from `MwlFile`, decode with the
  ROM's PIXI size table, encode to a file that decodes the same, and agree with the level
  in the ROM section by section, apart from the rewrites Lunar Magic makes on export
  ([lunar-magic.md](lunar-magic.md#mwl-files)), which the test counts per ROM; Lunar
  Magic's entrance settings, read from the ROM by `kobo_core::entrance` in any version,
  must equal the file's. A ROM
  whose headerless SHA-1 is in `fixtures/lunar_magic_mwl_export.txt` must also have
  exactly the files recorded there (a SHA-1 of the 512 concatenated in level order); the
  test prints the line for one that is not. The export in `~/.local/share/kobo/mwl/`
  (190 MiB, never committed) covers the vanilla ROM and 41 loose and `corpus_more` hacks,
  every one Lunar Magic opens (it refuses the seven locked ROMs and Smb2dx)::

  ```sh
  tools/lunar-magic/export-mwl ~/.local/share/kobo/mwl ~/.local/share/kobo/roms/*.smc \
    ~/.local/share/kobo/roms/corpus_more/*.smc
  KOBO_MWL_DIR=~/.local/share/kobo/mwl cargo test --test mwl_files -- --nocapture
  ```
  `vanilla_exports_import_and_build` imports all 512 vanilla exports into one project
  with `import_mwl`, builds it, and allows only those rewrites in `kobo diff` against
  vanilla: the background of the 276 levels on the shared empty level, `0C5`'s header,
  and layer 1 of eleven levels. It also compares the screen exits' bytes, which `kobo diff`
  puts in one format: the import writes them back in the game's format.
- **ExAnimation in builds**: `tests/mwl_files.rs` (with `KOBO_MWL_DIR`) requires every
  level's list read from the ROM (but Lunar Magic 2.41's older lists, which Kobo does not
  read from a ROM) to equal the
  one in its MWL export, byte for byte when written again; `tests/lunar_magic_save.rs`
  builds level 105 with a list and settings, a global list, and file `60`, and requires
  them all after a save, the settings lost without `$03FDFF`, and the lists lost without
  the `JSL` at `$00A390`.

### The hack corpus

- **Lunar Magic hacks**: `tests/corpus_levels.rs` loads every level of every hack of the corpus
  (the `lm_roms` tier), and of the vanilla ROM, once, on every core
  (`common::par_map`), and checks each two ways. It rebuilds the layer 2 tilemap the game
  uploaded to VRAM from the captured background buffer and BG Map16 table, which catches a
  clobbered buffer or a table read from the wrong place without external data; and it
  parses the sprite list against the RATS tag before it. It reports every hack's failures
  together. The corpus
  is `~/.local/share/kobo/roms`: loose `.smc` hacks (Lunar Magic 1.62 to 3.33), QLDC 2021
  and 2022 as BPS patches, and `corpus_more/`, later downloads distributed as BPS (3.21 to
  3.51, among them the corpus's first 3.40 and 3.51 saves). `apply_bps.py` in that
  directory writes each patch's ROM next to it, headered, after checking every CRC the patch
  carries. The tier takes ROMs, `.bps` patches, which the tests apply to the vanilla ROM in
  memory (`bps::apply_to_rom`), so the QLDC patches are listed as they are distributed, and
  folders of either (`tiers::expand_roms`); `tests/bps.rs` also rebuilds every hack from a
  patch `bps::create` made against the vanilla ROM. What each hack is known to fail is
  `tests/fixtures/known_failures.toml` ("Known failures" above).

  `tests/map16_pages.rs` (`older_layouts_import_as_lunar_magic_exports_them`, with
  Lunar Magic) imports the Map16 of every corpus hack in an older Lunar
  Magic layout and requires it to be Lunar Magic 3.70's `-ExportAllMap16` of the hack,
  tile for tile; `examples/map16_export_check.rs` shows the same per page.
  `map16_files_import_as_the_hacks_do` imports that export of every hack in the
  current layout as a Callisto project's `.map16` file is imported, and requires the
  pages, foreground and BG, the hack's own import gives (Grand Poo World 2 is a known
  failure; lunar-magic.md).

  Hacks whose
  headerless SHA-1 is in `fixtures/lunar_magic_map16_bg_export.txt` also have their BG table
  hashed against Lunar Magic's `-ExportAllMap16` output (file tile index `8000`-`81FF`).
  The rows checked are the 16 the game uploads from the layer 2 position, or from a
  position one row either side: the upload comes before the level loop's first camera
  update, which settles layer 2 by a few pixels in some levels.
  The failure message lists the words that differ.
- **Corpus sweep**: `tools/corpus-sweep out/ [hack.smc|patch.bps ...]` (default: the
  corpus, `lm_roms`) builds `kobo`, `render_hashes`, and `tiles_diff` in release
  (`--no-build` leaves them; `KOBO_BIN` names another `kobo`), then for each hack imports its
  changed levels, builds them, leaving out each level a build refuses until the rest builds,
  and runs `kobo diff`, an import of the build (its level and Map16 files must be the
  import's), `tiles_diff` and `render_hashes` over the levels built, and `save-check`. It
  reads what `kobo` reports as JSON (`--json`). It writes each hack's project, build, and
  log to `out/<hack>/`, and `out/results.json` (each hack by headerless SHA-1, with the
  revision swept) and `out/summary.md` (each hack, then refusals and import notes grouped by
  kind). A run stopped part way resumes from `results.json`, for hacks swept at the same
  revision; `--summary` only rewrites the summary, and `--no-render`, `--no-tiles`, and
  `--no-save` skip the slow steps. `--baseline old/results.json` compares each hack with an
  earlier sweep and lists what got worse in `summary.md`, failing the sweep: a stage not
  reached, more levels refused, a diff, an import of the build, or a save that passed and
  fails, levels that resolve or draw differently that did not. Keep a sweep's
  `results.json` (it holds hashes and messages, no ROM data) as the next one's baseline.
  The whole corpus (175 entries: the 47 loose and `corpus_more` hacks and QLDC 2021's and
  2022's 128 `.bps` patches, 135 LoROM and 40 SA-1, Lunar Magic 1.62 to 3.51) takes about
  45 minutes on four cores, mostly in rendering, and about 1 GB: point `out` and `TMPDIR`
  at a disk, not a `/tmp` in memory. `save-check` leaves its work folder (the saved
  copy, Lunar Magic's files) in `SAVE_CHECK_KEEP` when that is set.
- **A hack built by Kobo**: the corpus sweep imports each hack, builds it, compares what
  every level's load resolves (`examples/tiles_diff.rs hack.smc out.sfc levels...`: grid,
  background tilemap, Map16, BG Map16, graphics aside), imports the build again, and saves
  it with Lunar Magic (`tools/lunar-magic/save-check out.sfc 105 project`). A background in
  Lunar Magic's older format (`C` without `F`: Kaizo Mario 1, 2, and World 3) builds as 32
  rows of its own format: Kaizo Mario 2's six such levels load the same BG2 VRAM, but
  `tiles_diff` reports their "background tilemap", whose stride goes from `$1B0` to `$200`.
- **A hack built by Kobo, played**: `examples/built_play.rs hack.smc|patch.bps
  [against=rom] [levels=...] [paths=...]` imports the hack's changed levels, builds them
  (leaving out what a build refuses, as the corpus sweep does), and plays each level of
  interest (by default those with Lunar Magic's added layer 2 scroll settings and the
  tide levels with layer 3's `advanced` or `tides_act_as`) in both ROMs along seven
  paths of 600 frames, the player standing, running and jumping either way, carried
  right at 3 and at 8 pixels a frame, and up or down, comparing the camera, the scroll
  and layer 3 RAM, the player, what he touches, and the tile planes frame by frame,
  until the level is left. Against the hack, what differs is often the hack's: its own
  code, which the import does not carry ([build.md](build.md#no-base)), and an older Lunar Magic's
  code. `against=` takes the hack's levels moved into a 3.70 ROM
  (`tools/lunar-magic/transfer`, with MWL exports from `tools/lunar-magic/export-mwl` for
  hacks without them) instead, which compares Kobo's code with 3.70's on the same levels:
  what differs there is Kobo's, or the transfer's (the screen counts Lunar Magic's MWL
  import re-counts, and the hack's own sprites, which the build carries and the
  transfer does not). It is an example, not a test: a pass over the 24 hacks takes about
  an hour against each, and the comparison against 3.70 needs Lunar Magic and an MWL
  export of each hack. The 2026-10-04 run (the approved review of the layer 2 scroll
  settings and the tides) is in lunar-magic-install.md ("Layer 2 scroll settings",
  "Layer 3 settings"); what it found is checked by `tests/lunar_magic_save.rs`
  (`kobos_layer2_offset_at_the_entrance_is_lunar_magics`,
  `kobos_first_camera_and_scrolling_off_are_lunar_magics`), `tests/install.rs`
  (`a_point_out_of_the_level_touches_air`), and `tests/layer3_settings.rs`
  (`a_tide_with_an_autoscroll_moves_layer_2_by_its_steps`).
- **A hack's content through Kobo's code**: transfer a hack into a Lunar Magic-saved
  vanilla ROM with Lunar Magic's command line (`tools/lunar-magic/transfer hack.smc
  mwl-dir out.smc`: `-ImportAllGraphics` of its `-ExportGFX`/`-ExportExGFX` first, then
  `-ImportAllMap16`, `-ImportSharedPalette`, `-ImportMultLevels` of its MWL exports,
  `-TransferLevelGlobalExAnim`), then `swap bank06` (examples/swap.rs)
  swaps Kobo's bank `$06` code in, keeping the tables, and `render_hashes` and
  `ramdiff.py --summary` compare the two over all 512 levels. Do not import into a Kobo
  install instead: Lunar Magic's save then installs most of its own code over it
  ([lunar-magic-install.md](lunar-magic-install.md)).
  `swap bank06` also works on a hack itself, for its Lunar Magic 2.52-and-later layout.

### Builds and the toolchain

- **Project build**: `tests/project_build.rs` imports every vanilla level into a
  project, requires Kobo's formatting to be a fixed point on every file, builds it (twice,
  for byte-identical output), and requires every level to read back as vanilla's, its
  layer 1 data in the expanded ROM, and seven levels of different kinds to render the
  same picture. A build through the stage cache must equal one without, cold, warm, and
  after an edit. `a_synthetic_build_is_the_same_everywhere` needs no ROM: it builds
  `fixtures/synthetic_level.toml` onto `common::synthetic_base()` and pins the output's
  SHA-1, so CI shows whether all three platforms build the same bytes; a change to what a
  build writes changes the hash on purpose. With the checks of every level on, all 512 pictures of the build must be vanilla's.
- **Callisto import**: `tests/callisto.rs` imports a synthetic Callisto project (a patch
  that includes `callisto.asm` and a file through `%incsrc_file`, a full Map16 export of
  the clean ROM's tables with one tile of page 2 set, a graphics folder with one file
  changed, and a ROM and a program that must not be copied), checks the project, and
  with Asar builds it. The Romhack Races template is downloaded, so no test builds it; check it by hand
  (`kobo new`, a build with every tool configured, `render_hashes` against the baserom's
  own Callisto build, and `save-check`).
- **Tool stages**: `tests/tool_stages.rs` builds two Asar patches, early and late, one
  including a file, onto the synthetic base when Asar's library is configured (no ROM):
  they apply in order, the output repeats, and changing the included file changes the
  build. With AddmusicK (`KOBO_ADDMUSICK` or `tools.addmusick`, a folder; `~/src/addmusick` here) and the
  vanilla ROM, a project with an empty music folder gets AddmusicK's default music
  (`@AMK` at `$0E8000`), the same bytes twice.
- **GPS**: with GPS (`KOBO_GPS` or `tools.gps`, a folder with GPS built for the system and its files;
  `~/.local/share/kobo/tools/gps-1.4.4` here), `tool_stages.rs` inserts a block acting like
  `$025` at tile `$200` into Kobo's acts-like chain and checks the table and GPS's entry.
- **UberASM Tool**: with UberASM Tool found offline (`KOBO_UBERASM`, a folder with the
  program built for the platform and its files, or the pinned build in the cache after
  `kobo tools fetch`), `tool_stages.rs` inserts one level's code, the same bytes twice.
  It also runs the tool on an image with no game data, which needs no ROM; CI does that on
  all three platforms with the pinned build and requires it (`KOBO_REQUIRE_UBERASM=1`).
  A folder that brings its own Asar library, as upstream's 32-bit Windows release does,
  keeps it.
- **PIXI**: `tests/tool_stages.rs` runs PIXI (configured, or the pinned build in the
  cache; `KOBO_REQUIRE_PIXI=1` makes its absence a failure, as CI does) on a synthetic
  image (`pixi_runs_without_a_rom`, every pinned version: 1.43 and 1.42) and as a build's sprites stage with a sprite placed in
  a level (`pixi_inserts_sprites`, with the ROM); `tests/lunar_magic_save.rs`
  (`a_pixi_build_survives_a_lunar_magic_save`) has Lunar Magic save such a build and
  plays level `0EB` across its secret-exit goal tape in the clean ROM, the build, and the
  saved copy. `pixi_insert_is_read_whole` runs PIXI on the synthetic image with a sprite
  of every type and a shared routine and requires `pixi::read`'s insert, written onto the
  image PIXI ran on, to give PIXI's image again; `pixi_sprites_carry_through_an_import`
  (with the ROM) imports a PIXI build both ways, compiled and with the folder, and plays
  the sprite in each rebuild. `pixi`'s unit tests cover the walk on a synthetic insert,
  `PROT` lists, and a jump at a site Lunar Magic shares that is not into PIXI's block.
- **Pinned tools**: `tools::pinned`'s unit tests serve synthetic builds from a local port:
  a build is downloaded, checked, and unpacked once; one that fails its hash or is longer
  than pinned leaves nothing in the cache; an archive entry outside its folder is refused;
  a `.zip` (SA-1 Pack's release) unpacks stored and deflated entries into its folder and
  fails a damaged one's CRC; and `pinned.toml` names Asar, PIXI, and UberASM Tool for
  every platform, `upstream.toml` SA-1 Pack. CI fetches the real builds and SA-1 Pack's
  release before the tests (a cache keyed by both files), and runs the tests with
  `KOBO_OFFLINE=1`, so no test downloads. The published builds are checked against a
  rebuild from their sources by `kobo-smw/kobo-tools`'s `verify` workflow.
- **SA-1 builds**: with SA-1 Pack (`KOBO_SA1PACK`, `~/src/sa1pack` here, or the pinned
  release in the tool cache, which CI fetches), `sa1_pack_applies_without_a_rom` applies
  it to the synthetic image, which must give the same bytes on every platform; with the
  vanilla ROM as well, `tool_stages.rs` imports
  every level of the SA-1 base and builds it back as an SA-1 project, which must read back
  the same and render four levels the same. With the checks of every level on, all 512 must
  draw the same, but `012`, `0F8`, and `101`, which Kobo's patches change as Lunar Magic
  3.70's code does (the SA-1 build installs them; lunar-magic-install.md).
  `sa1_projects_store_gfx_as_lz3` builds an empty SA-1 project with `[rom] lz3`: SA-1
  Pack's routine must read every GFX file back as the clean ROM's, and three levels load
  the same VRAM and draw the same as in the LC_LZ2 build; with the checks of every level on,
  all 512. With Lunar Magic as well, `lunar_magic_save.rs`
  (`lunar_magic_reads_an_lz3_build`) requires its `-ExportGFX` of the LC_LZ3 build to
  equal that of the LC_LZ2 one, and its save to keep `$0FFFEB`, and without `$0FFFFF` set,
  neither.
- **Map16 pages**: `tests/map16_pages.rs` builds pages in three groups of 16, one past
  `$40`, onto the synthetic base (Asar needed) and reads them back as written; with the
  vanilla ROM it also resolves them through the ROM's own tilemap upload and draws two
  vanilla levels as vanilla does. The corpus sweep imports every hack's build again, whose `map16`
  files must be the import's, and saves it with Lunar Magic. Pages 0 and 1 and the tilesets' own tiles, with page 2
  per tileset: `game_pages_build_and_read_back` builds changes to each onto vanilla, reads
  them back, and resolves them through the game's uploads in levels `103`, `105`, `107`,
  `101`, and `1D` (tilesets 0, 7, 5, 1, 3); the refusals have a test of their own; and
  with the corpus, every LoROM hack's pages 0 and 1 imported and built give its
  tiles in all 15 tilesets and its acts-like settings, and import again the same.
- **Lunar Magic's objects**: `examples/lm_objects.rs make` writes MWL files with every form
  of objects `22`, `23`, `27`, and `29` (groups 0 and 1 for level `105`, group 2 for the
  vertical level `1CE`); Lunar Magic imports them into vanilla, and `lm_objects grid`
  prints the tiles each case left. The same ROM with `asm/lunar-magic/objects.asm`
  applied (`kobo rom expand 2M`, then `kobo asm`) must print the same, also with bank
  `$0D`'s steps (`$0DA900`-`$0DAA1F`) put back to vanilla. `lm_objects hashes` prints
  them as SHA-1s, which are `fixtures/lunar_magic_objects.txt`; `project_build.rs`
  (`lunar_magic_object_forms_build`) builds every case with Kobo's code, needing only the
  ROM and Asar, and requires the same grids. It also builds a user object (`2D`), which
  must leave the level's grid as it was. The cases are in `tests/common/object_cases.rs`;
  a new one needs its line from Lunar Magic's import. `lm_objects timer rom level` prints
  the timer a load leaves, entered from the overworld and through an exit; the time limit
  bypass's forms, with the timer each left in a Lunar Magic ROM, are
  `the_time_limit_bypass_sets_the_timer_as_lunar_magic_does` in `project_build.rs`.

### Kobo's code against Lunar Magic's

`cargo run --release --example swap -- PIECE lm.smc out.sfc` puts Kobo's code for one
piece of Lunar Magic's layout into a ROM Lunar Magic saved, in place of Lunar Magic's,
keeping the ROM's tables (`tests/common/swap.rs` holds each piece's sites, which the
tests use too): `bank06`, `vram`, `graphics`, `layer3`, `sprites`, `exanim`, `exlevel`,
or `entrances`, several joined by `+`. The probes below then compare the two ROMs.

- **Kobo's ROM-side code**: `tests/install.rs` applies `kobo_core::install`'s patches to
  vanilla (Asar's library needed) and runs the ROM: Map16 lookups for pages 0 and 1 as the
  game's, pages past 1 from tables written where Lunar Magic's layout points, and a few
  levels drawn as vanilla. With the checks of every level on, all 512 levels must draw as
  without the patches, on the vanilla ROM and the SA-1 base, but `012`, `0F8`, and `101`,
  three pictures the patches change as Lunar Magic 3.70's code does (lunar-magic-install.md).
- **Lunar Magic's layout on SA-1**: with SA-1 Pack configured (`common::sa1_base`;
  `KOBO_REQUIRE_SA1PACK=1` makes its absence a failure), the tests of Kobo's patches run
  on both bases: `tests/install.rs` installs the patches on the 1 MiB vanilla ROM and on
  the SA-1 base (`installs`), and the build tests build each project as it is and as an
  SA-1 project (`common::builds`, `common::sa1_variants`): `project_build.rs`,
  `map16_pages.rs`, `taller_levels.rs`, `graphics_build.rs`, `layer3_settings.rs`, and the
  Lunar Magic save checks in `lunar_magic_save.rs`; `lunar_magic_layout_builds_past_4_mib`
  builds Lunar Magic's objects, a custom palette, and a taller size onto 6 and 8 MiB
  SA-1 images. Against Lunar Magic's own SA-1 code:
  `kobos_sprite_loader_spawns_as_lunar_magics_on_sa1` and `kobos_exanimation_runs_as_lunar_magics`
  (`KOBO_LOADER_CASES` and `KOBO_EXANIM_CASES` run more), and
  `kobos_layer3_code_plays_as_lunar_magics` takes the corpus's SA-1 hacks with SA-1 Pack's
  bytes at the hook sites. The twelve base patches on the SA-1 base are `tests/install.rs`'s SA-1 variant; with
  the checks of every level on, all 512 levels draw as without them but `012`, `0F8`, and
  `101`, as on the LoROM build. `examples/swap.rs` takes an SA-1 hack's sites back to the
  SA-1 reference ROM's bytes (`KOBO_SA1_REFERENCE`).
- **Kobo's code against Lunar Magic's, swapped into its own save**: `lunar_magic_save.rs`
  saves the clean ROM (and the SA-1 base) with Lunar Magic, puts Kobo's patch for a piece
  in its place, and requires the same memory as Lunar Magic's code leaves:
  `kobos_end_fade_is_lunar_magics` (`palette.asm`: the palette at every step of a level's
  end fade, called with a palette of distinct colours), `kobos_layer2_interaction_and_camera_are_lunar_magics`
  (`exits.asm` and `entrance.asm`: the words layer 2's interaction, a vertical level's
  camera, and the entrance's facing touch, on every frame of eight runs through layer 2,
  tide, and vertical levels), `kobos_special_exits_are_lunar_magics` (the level an exit
  in the game's format, Yoshi's wings, or the bonus game loads, from every third
  translevel on three submaps), and `kobos_vram_patch_lags_as_lunar_magics` (`vram.asm`:
  layer 1's and 2's scroll registers and tilemaps at every vertical blank with every
  other frame lagging, through `expand::play_game_loop_lagging`, then with the player put
  at x 0 or far right on the first frame, which makes the camera skip columns). By hand,
  `exlevel_probe compare` does the same over more levels and paths, with `lag=N` for
  lagging frames; how each was found is in lunar-magic-install.md ("The sites a save keeps
  with the marker", "Graphics").
- **Lunar Magic check**: `tools/lunar-magic/save-check built.sfc [level [project]]` has Lunar Magic
  3.70 save a copy of a ROM (exporting a level and importing it back) and runs `kobo diff`
  on the two: every level must read the same. `tests/lunar_magic_save.rs` does the same
  with the `lunar_magic` tier, Lunar Magic 3.70's folder (Linux, with Wine and
  `xvfb-run`; one run at a time on the machine; CI has no Lunar Magic): a build of level `105` with Lunar Magic objects,
  a palette of its own, entrance settings, Map16 pages in three groups, changes to pages
  0 and 1, and page 2 per tileset must read back the same after a save, pages included;
  without the checked bytes below, only the level's `entrances` may differ. Lunar Magic's
  `-ExportAllMap16` of the same build must show every acts-like setting, every tileset's
  pages 0 and 1 and page 2, and every page the build lists as written (the file's layout is
  in [lunar-magic.md](lunar-magic.md)); without the marker at `$06F5FC` it shows the
  acts-like settings, page 2 per tileset, and pages past `$0F` wrong, and nothing else. A
  new piece of Kobo's install adds its part to that build. A build of the whole vanilla
  import passes with levels `105` and `106`. Every Lunar Magic feature builds write is
  checked this way. Once a build writes any of the bytes Kobo writes only because Lunar Magic
  checks them ([lunar-magic-install.md](lunar-magic-install.md#bytes-kobo-writes-because-lunar-magic-or-other-tools-check-them)),
  every feature's check runs twice, with those bytes and with them cleared back to what a
  build without them has, and anything Lunar Magic then does differently (installs, keeps,
  or drops) is recorded in that section.
- **Graphics and the VRAM patch**: `examples/gfx_probe.rs` reports memory effects only.
  `vram rom level` prints the tilemap registers and the VRAM a load wrote; `dump` writes
  the VRAM; `map a b level` finds each run of `b`'s tiles in `a`'s VRAM; `watch rom level
  frames path` plays the level through the game loop and its NMI
  (`expand::play_game_loop`), steering the player along `path` (`right`, `up`, or
  `dx,dy` pixels a frame), and prints which tilemap cells each frame changed; `play level
  frames path rom...` prints, per frame, the VRAM words and RAM bytes that differ between
  the first ROM and each other, for comparing Kobo's VRAM patch with Lunar Magic's;
  `bypass rom...` counts objects `24` and `25`; `loads a b [range]` lists every level
  whose VRAM after the load differs between two ROMs (default: the tilemaps,
  `3000-4000`); `scroll a b frames path [levels]` plays every level along a path in both
  and reports cells in view that differ for two frames running
  (`KOBO_PROBE_DUMP=dir KOBO_PROBE_FRAME=n` writes that frame's VRAM of each, `a.vram`
  and `b.vram`). Paths are legs `dx,dy@frames/...` of pixels a frame for the player
  (`-1000,0@1/3,0` sends it back 1000 pixels on the first frame, a camera that skips
  columns); `KOBO_GFX_POKE_TILE=dx,dy,tile` puts a
  tile (a coin) beside the player first. For where a list's files go, make ExGFX
  files whose every 16-byte unit starts `A5 5A f j` and look for them in the VRAM dump.
- **Kobo's VRAM patch against Lunar Magic's**: `cargo run --release --example swap -- vram
  lm.smc out.sfc` swaps `vram.asm` into a Lunar Magic-saved ROM; then `gfx_probe loads` and
  `scroll` compare the two. `exlevel_probe call
  ... frames=N then=frame vramw=...` shows where a tile changed in play goes.
  `tests/install.rs` (`the_vram_patch_keeps_layer_1_in_view`) checks, with the ROM and
  Asar, that every cell in view on layer 1 holds its tile's definition along paths in a
  horizontal, a vertical, and a layer 2 objects level.
- **Kobo's taller levels against Lunar Magic's**: `cargo run --release --example swap -- exlevel
  lm.smc out.sfc` puts every site of Lunar Magic's taller levels piece back to the game's
  bytes, applies `exlevel.asm`, and moves the size table to where it then is; swap
  Kobo's sprite loader and VRAM patch into both sides first (`swap sprites`,
  `swap vram`), since swapping in one piece alone leaves the other's RAM otherwise.
  `examples/exlevel_probe.rs` then compares them: `compare a b level` plays a level in
  both frame by frame (the player carried, `path=`, or free with buttons held, `pad=`;
  `hold:`/`ram:` for RAM such as `$1887`, the ground shaking, or `$1412`, the vertical
  scroll setting), `sizes rom level` shows what each size byte changes at the entrance,
  `call rom level addr jsl|jsr` runs one routine with chosen RAM and registers
  (`expand::call_in_level`), `ram`, `show`, `entry` print the tables, words per
  frame, and the entrance's RAM, and `cells a b level frame` draws which Map16 cells of
  layer 1's and 2's tilemaps differ on a frame, in view or not. `entry_probe compare` with `KOBO_ENTRY_SIZE` varies the
  size byte along with the entrance bytes, and `KOBO_ENTRY_BASE=table=VV,...` (`SZ` the size
  table) sets bytes in both first, for a corner of several settings. Hacks with taller levels are moved into a
  3.70 ROM with `transfer` first: their own ROMs carry older versions' code. The results
  (2026-09-28) are in docs/lunar-magic-install.md ("Taller levels").
  `tests/taller_levels.rs` builds every size with and without `B` and requires the
  objects where the size puts them and the RAM Lunar Magic's code leaves, layer 2's
  split, and the refusals; with the corpus and its MWL exports, every corpus ROM's
  size table read where the layout has it must equal its MWL exports.
  `lunar_magic_save.rs` (`a_taller_level_build_survives_a_lunar_magic_save`) has Lunar
  Magic save a build with a taller level, which must keep its size and Kobo's code, and
  without the `JSL` at `$05DA8A` lose the size.
- **Kobo's graphics loader against Lunar Magic's**: `cargo run --release --example swap -- graphics
  lm.smc out.sfc` swaps `graphics.asm` into a Lunar Magic-saved ROM with 4bpp files (and
  ExGFX and lists, if it has them), keeping its tables; `vram+graphics` swaps the VRAM patch
  first. `gfx_probe loads a b 0000-8000` then compares all of VRAM after every load, and
  `render_hashes` the pictures (docs/lunar-magic-install.md, "Kobo's graphics code", has
  the results). The player's tile words (`$6040`-`$6187`) and SP1's last tile (`$67F0`)
  come out of whatever direct page the load leaves (docs/lunar-magic-install.md, "Kobo's
  graphics code"): the same once the graphics loader gave its direct page back. A whole
  build leaves them as the vanilla ROM does (`exlevel_probe compare v.smc built.sfc 105
  free vram=6000-6800 ignore=0-1FFFF runs=0` lists the frames that differ), and a Lunar
  Magic-saved ROM not.
  `tests/graphics_build.rs` builds 4bpp files, ExGFX files in lists, layer 3's files,
  the animated tiles' file, and the older lists with objects `24` and `25`, and checks
  VRAM and RAM after the load, what import reads back, and what builds refuse, and, with
  the corpus, imports every unlocked corpus hack's ExGFX files, older lists, and the
  lists a build takes, builds them onto vanilla levels, and reads them back;
  `tests/lunar_magic_save.rs` (`a_graphics_build_survives_a_lunar_magic_save`) has Lunar
  Magic save such a build, with and without each checked byte.
- **Layer 3 settings**: `examples/layer3_probe.rs` plays a level along a path and prints,
  per frame, layer 3's position and RAM, colour math and screens, and the scroll and
  screen registers the frame's NMI and IRQ left (`play`, with `KOBO_LAYER3_WATCH` and `KOBO_LAYER3_POKE`);
  `compare` and `loads` report where two ROMs differ in those, work RAM, and layer 3's
  VRAM; `sweep a b frames` reads cases (`level path SLOT:n ...`) from standard input,
  writes each list into both ROMs, and reports the cases that differ.
  `cargo run --release --example swap -- layer3 lm.smc out.sfc` puts Kobo's layer 3 code
  (`layer3.asm`) in place of Lunar Magic's. `tests/layer3_settings.rs`
  builds level 105 with settings and checks layer 3 frame by frame, and what builds
  refuse; with the corpus, it puts Kobo's code in every corpus hack that has Lunar
  Magic's, saved by 3.10 or later, and plays each level with
  settings under both, comparing layer 3's position,
  colour math, screens, scroll type, registers, and tilemap (not the code's own state,
  which older versions keep otherwise). `tests/lunar_magic_save.rs`
  (`a_layer3_build_survives_a_lunar_magic_save`) has Lunar Magic save such a build, with
  and without the `JSL` at `$00A01F`.
- **Sprite loader**: `examples/sprite_probe.rs` writes a sprite list into a copy of a ROM
  (`list=new,mem=N,id:x:y[:extra]...`, at `$3F8000` through Lunar Magic's bank table), sets
  ROM and RAM bytes, carries the player along a `route=frame:x:y/...` (with `$13`
  counted as the game loop counts it), and prints each frame's sprite slots, load flags,
  and watched RAM (`play`), the frame each entry first loaded with the camera then
  (`spawns`), or where two ROMs differ (`compare`). Compare vanilla saved once by Lunar
  Magic with the same ROM with the loader group put back to the game's bytes and
  `sprites.asm` applied (`examples/swap.rs sprites`); `lunar_magic_save.rs` (`kobos_sprite_loader_spawns_as_lunar_magics`,
  `KOBO_LOADER_SEED` and `KOBO_LOADER_CASES` to vary it) does that for 24 seeded random
  scenarios, and must find no frame that differs. `SPRITE_LOADER_SITES` there is the group,
  which a save must leave as a build wrote it.
- **ExAnimation**: `examples/exanim_probe.rs run rom level frames` plays a level through
  the game loop and its NMI and prints, per frame, the VRAM words and CGRAM colours the
  frame changed and `$7FC000`-`$7FC0FF`; `ab a b level frames` prints the frames whose
  VRAM, CGRAM, or ExAnimation RAM differ between two ROMs. Chosen data goes into a copy in
  memory (`KOBO_EXANIM_ANIM`, `KOBO_EXANIM_GLOBAL`, `KOBO_EXANIM_SETTINGS`, `KOBO_EXANIM_PALETTE`), RAM before the load (`KOBO_EXANIM_ENTRY`) and
  before frames (`KOBO_EXANIM_SET`), and a word-numbered pattern at `$7EAD00` (`KOBO_EXANIM_FILE`), so an upload
  shows where it came from. `cargo run --release --example swap -- exanim lm.smc out.sfc` swaps
  Kobo's code into a ROM with Lunar Magic's, keeping its tables. `tests/lunar_magic_exanimation.rs`
  (with Lunar Magic) has Lunar Magic install its own on the clean ROM, swaps Kobo's
  in, and plays seeded random lists, settings, trigger states, and events for 96 frames
  in both (`KOBO_EXANIM_SEED`, `KOBO_EXANIM_CASES`, 16 by default; a failure names the
  first differing byte, the lists, and the events; `KOBO_EXANIM_KEEP=dir` writes the two
  ROMs there, for `exanim_probe ab` with chosen lists); the player's tile
  words are left out (lunar-magic-install.md). Every trigger on every type is drawn
  but `0F` on frame types.
- **Exits**: `examples/exit_probe.rs rom [low...] [addr=value]` runs a ROM's entrance
  code for every exit flags nibble, secondary flag, and submap, and prints where each
  leads; two ROMs' outputs compare Kobo's exit code with Lunar Magic's.
- **Entrances**: `examples/entry_probe.rs` runs a ROM's entrance code (`effects`: each
  settings bit flipped; `compare a b level [ignore]`: every value of every byte in two
  ROMs; `batch`: chosen settings from stdin; `levels a b`: every level as it stands), by a
  screen exit, from the overworld (`KOBO_ENTRY_OW=translevel[,midway]`), by an exit with
  `w` (`KOBO_ENTRY_FLAGS=8`), or into a secondary entrance (`KOBO_ENTRY_SECONDARY`), and
  after the whole load with `KOBO_ENTRY_FULL`. Leave out of a comparison with a Lunar
  Magic ROM the RAM its taller levels set (`005B:80 0BE7 0BEE-0D75 13D7-13D8 1936-1937`)
  and scratch and pointers (`0065-006A 008A-008F 00CE-00D0 1BB2-1BBA 0BDA-0BDC`); against
  the hack itself, also `00D1-00D4` and `1DEA`. The RAM both probes compare goes through
  `clean_room::bytes`, which withholds the stack but keeps `$0100`-`$010F`, the game's
  variables under it. The overworld can name a level only up to low byte `$DB`
  (`CODE_05D8A2` takes `$24` off a name of `$25` and up).
- **Block contact probe**: `tools/lunar-magic/block-probe/make-rom out/` builds vanilla
  saved once by Lunar Magic with GPS's logging probe block in level `105` (needs Wine,
  Lunar Magic, and GPS 1.4.4 built for the system in `KOBO_GPS`;
  `~/.local/share/kobo/tools/gps-1.4.4` here), and `cargo run --release --example
  contact_probe -- run out/probe.sfc` plays the level with the player placed against the
  block in each scenario and prints the actions that ran. The same run on a Kobo build with
  the same block must print the same. Clean room: the probe ROM holds Lunar Magic's code,
  so never run it with `KOBO_CPU_TRACE`, which would print Lunar Magic's instructions
  (`kobo_core::clean_room` turns the trace off, and `KOBO_RAM_WATCH` reports only the
  writes, for such a ROM); find what is left open from RAM effects,
  with more scenarios or probe blocks. `contact_probe -- stand rom level x y tile...
  [addr=value...]` drops the player onto each tile and prints whether they landed and
  `$1693`, in any ROM. `contact_probe berry rom x y tile...` and `contact_probe tongue rom
  tile...` put Yoshi's berry check on chosen tiles (a stunned baby Yoshi, or Yoshi's tongue
  with the player riding) and print the berry eaten, the sprites made, the tiles left, and
  what `tongue-slot.asm` logged; `level=n` takes another level, and the baby Yoshi's
  `layer=2` and `other=tile` put the tile on layer 2 and another on layer 1 at the same
  point (`KOBO_BERRY_PLACE` the frame they go in, once the layers' offsets are set).
  `tools/oracle/berry_probe.lua` runs the same baby Yoshi in Mesen 2, through
  `dump_levels.lua`'s `KOBO_ORACLE_PROBE`, to tell Kobo's machine from the ROM's code
  (Lunar Magic's berry check hangs the game in both; lunar-magic-install.md).
- **The paths the harness does not reach**: `tools/oracle/tables_probe.lua`, through
  `dump_levels.lua`'s `KOBO_ORACLE_PROBE`, logs the taller levels' tables in Mesen 2 from
  boot, so a castle's "No Yoshi" intro shows; `PROBE_CI2=1` loads the level as Choc
  Island 2's rooms and `PROBE_CREDITS=1` goes to the credits' enemy list. Run it on a
  Lunar Magic-saved ROM and a Kobo build of the same levels and compare the logs.
  `expand::play_level_entered`'s `entry` reaches Choc Island 2's rooms in Kobo's own
  machine (`tests/taller_levels.rs`).
- **A level played in two ROMs**: `tools/oracle/play_probe.lua`, through
  `dump_levels.lua`'s `KOBO_ORACLE_PROBE`, puts the player and the camera where
  `PROBE_PLACE` says as the level is prepared, holds the buttons `PROBE_SCRIPT` gives
  from each frame on, and every `PROBE_EVERY` frames takes a screenshot and logs the
  player, the ON/OFF flag, and the OAM (`PROBE_VRAM=1`: VRAM too). Run it on a Kobo
  build and on the hack's own Lunar Magic build and compare the shots, the logs, and the
  tilemaps' words (`$3000`-`$3FFF`); it reads memory only, so either ROM may be one
  Lunar Magic saved. It found tiles changed in play going to layer 2's tilemap in the
  Romhack Races template's build (testing-log.md).

### The overworld

`tests/overworld.rs` holds the overworld to what ROMs' own code leaves in RAM, the
yardstick being `expand::load_overworld`: a new game Kobo's machine plays from power-on
(Start on the title screen, A on the file and player selects, `$0109` held at 0 so it
goes to the overworld, not the intro level) to the first frame of game mode `$0E`.
`load_overworld_passed` does the same with events held passed (`SmwBus::pinned` keeps
`$1F02` on, since a new game clears it and hacks' file selects copy it from other
places), and `end_event` then runs an event's last step in play (`CODE_04E9EC`).

- The vanilla overworld reads (`overworld::Overworld::read`) as its load leaves it, and
  Kobo's build of it (no changes, in Lunar Magic's layout) loads the same with every
  event passed and ends each event with further tiles the same.
- Every corpus hack's overworld reads as its load leaves it, and as changes against the
  clean ROM's gives itself back; built by Kobo, it reads back the same, loads as read,
  and with every event passed loads as the hack itself does. Locked hacks, older Lunar
  Magic's one-page overworlds, and hacks whose own code changes the overworld are known
  failures.
- `examples/ow_probe.rs` compares loads by hand: two ROMs with every event passed or
  each alone (`pair`, `each`), each event's end (`end`, `ends`), or a hack against
  Kobo's build of its overworld (`build`, `passed`, `write`).

### Picture hashes

- **Picture hashes**: `cargo run --release --example render_hashes -- rom.smc [level...]`
  prints a line for every level (`tests/common/render_hashes.rs`): a SHA-1 of its picture
  with sprites drawn, one as markers without the player, and a short hash of what the
  passes reported, since a pass that stops working can leave the picture as it was.
  `tests/render_baselines.rs` checks the vanilla ROM's and the SA-1 reference ROM's against
  `tests/fixtures/render_hashes/` ("Picture baselines" above); for a hack, diff the output
  before and after a change. `examples/swap.rs` and the probes compare Kobo's code with
  Lunar Magic's in other ways.

### Parser mutation checks

`tests/input_robustness.rs` runs 512 repeatable synthetic mutation cases in CI, covering
header size codes, mapped pointers, overflowing reads, truncated LC_LZ2, LC_LZ3, and
LC_RLE1 streams, sprite lists, object data, which must also encode back to the same
objects, and MWL files, a small valid one with bytes changed or cut short, which must
encode to a file that decodes the same, and round-trips noise and generated runs and
repeats through the LC_LZ2 compressor. The same generator runs for longer as an example:

```sh
cargo run --release --example fuzz_inputs -- 10000
cargo run --release --example fuzz_inputs -- 1 123   # reproduce a failing seed
```

This is deterministic mutation smoke fuzzing, not coverage-guided fuzzing, and it needs
no ROM. Targeted regressions separately cover out-of-file GFX pointers, invalid header
size codes, broken PIXI pointers, and unterminated sprite lists.
