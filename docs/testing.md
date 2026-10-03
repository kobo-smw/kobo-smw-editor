# Oracles and corpus checks

The tiers themselves (unit, ROM-backed, opt-in oracles), where the vanilla ROM is looked up, and
its reference hash are in `AGENTS.md` under "Test tiers and ROM configuration". This file is
how each oracle is produced, where its data lives, and what is known not to match.

- **Emulator oracle** (`tools/oracle/`): `dump.sh <rom> <outdir> 105,106,...` runs Mesen 2
  headlessly, navigates to each level through the file select, and dumps the tile grid,
  CGRAM, VRAM, header RAM, and the sprite slot tables on the first level frame. The script
  zeroes work RAM, save RAM, video memory, and an SA-1's I-RAM before the game starts, as
  `expand`'s machine starts, whatever power-on state Mesen is set to (random by default):
  dumps repeat byte for byte, VRAM included, and hacks that read memory they never wrote
  behave the same each run.
  `tests/oracle_levels.rs` compares `expand::expand_level` against a dump directory when
  `KOBO_ORACLE_DIR` is set; the dumps are of the vanilla ROM unless `KOBO_ORACLE_ROM` names
  another. All 512 vanilla levels match byte for byte in the tile grid, and in which sprite
  is in which slot (the sprite and its place, not whether it is alive: the dump comes a
  frame or so into the level, and a sprite spawned beyond the despawn range is erased and
  spawned again as the loader comes round). Test level `132` used to differ: its Lakitu's
  cloud had thrown two Spinies here and none in the emulator. The cloud throws when the
  frame counter's low seven bits are clear (`$01E98D`), and the loader's counter started
  at zero; it now starts at `$40` ([smw.md](smw.md)). Dumps live in `~/.local/share/kobo/oracle/`
  and are never committed. `trace_writes.lua` logs who writes a RAM address, for debugging,
  and refuses a ROM Lunar Magic saved (clean room). `dump.sh` first checks, with
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
  the write alone, or nothing of a write to the stack. The watch found PIXI's offscreen routine
  erasing a sprite in `43_gui` on the SA-1. `KOBO_VRAM_WATCH=$3A1E` does the same for a
  VRAM word, naming the DMA source: it showed the rows Akogare2 `111` uploads as its layer 2
  moves.
  A full run of the vanilla ROM takes 20 minutes and an SA-1 ROM emulates slower; four
  runs of 128 levels each into separate directories, moved together afterwards, take a
  quarter of that.
  `KOBO_ORACLE_VIDEO=1 dump.sh ...` instead waits for visible video and also writes PPM,
  full WRAM, and PPU state. Keep these later-frame captures in a separate directory. The
  screen buffer the script reads is a frame or two behind the PPU state, which of the two
  varying from run to run, so it waits for four frames at full brightness
  (`KOBO_ORACLE_VISIBLE_FRAMES=<n>` waits for another count, to see a level's first frames
  one by one); captures made before that wait was added can be a fade step darker than
  the render and match nothing.
  `KOBO_BOSS_ORACLE_DIR` enables stable boss graphics comparisons for levels 096, 0CC,
  0D9, and 1C7 (Mode 7 characters, layer 3 GFX, arena tilemap, SP3); either capture mode
  works. The loader override must only run in game mode `$11`: overriding the
  title-screen load in mode `$03` contaminates the graphics cache, and dumps made before
  that guard fail the boss comparison.
  `tests/video_oracle.rs` compares whole rendered pictures (with sprites) against the
  PPM frames of `KOBO_ORACLE_VIDEO=1` dumps listed in `KOBO_VIDEO_ORACLE_DIRS`
  (`:`-separated), cropped at the camera from the dumped WRAM (`$1A`/`$1C`; the PPU
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
  On 12 levels of each of the 42 `.smc` hacks the tile grids and layer 3 tilemaps all match,
  and the sprite slots in 38; the rest are sprites already moving at the dump (`apes1.13`
  `02E`, Luminescent `103`, `SMW_2021-4-24` `0C5`/`1C5`: 8 to 10 pixels on, where the test
  allows 4) and one the emulator has not spawned yet (Super Hark Bros 2 `138` slot 5).
  The 38 SA-1 entries of QLDC 2021 and 2022 (BPS patches, applied first) were dumped the
  same way, six levels each: 37 get into a level (`28_Kitikuchan`'s title screen is a room
  to play through), and 35 of those match throughout. `61_Wakana_Sariel` level `13B`
  differs in the layer 3 tilemap, which its per-frame status bar code has drawn into by
  the time of the dump, and `43_gui` level `105` differs in slots 10 to 13 by the parity
  of the frame its sprites first ran on. The hack places sprites beyond the camera's right
  edge, which PIXI's offscreen routine (one side per frame by `$13 & 1`, PIXI's source) erases
  on even frames and the loader brings back; the dump's first sprite frame was odd and
  kept them all, the loader's here is even (`$13 = $40`, [smw.md](smw.md)) and one had gone
  before a spawner's child took its slot. Captures of the level one to twelve frames on
  (`KOBO_ORACLE_VISIBLE_FRAMES`) show the emulator erasing them from its second frame.
  Dumps live in `~/.local/share/kobo/oracle/hacks/`. With `KOBO_ORACLE_VIDEO=1` the same
  dumps give whole frames; of 27 levels of four hacks whose pictures changed when the
  faults below were fixed, the entry screen of 23 went from 10-56% of pixels matching to
  91-99.7% (Luminescent `148` only to 50%: an HDMA sky, see [known-gaps.md](known-gaps.md);
  the other three barely moved).
  What the hack dumps found, all in code no vanilla level runs: Lunar Magic's graphics
  upload reading work RAM back out of VRAM, `TM` written past its mirror, a game loop in a
  FastROM bank ([lunar-magic.md](lunar-magic.md)), and layer 2 left wherever level-init
  code put it ([smw.md](smw.md)).
- **Per-sprite comparison on hacks**: `examples/sprite_oracle.rs` takes a ROM and
  directories of `KOBO_ORACLE_VIDEO=1` dumps of it, renders each level with sprites, and
  scores the pixels each captured sprite entry's objects cover against the frame, where
  the entry is on the emulator's screen, along with the whole visible picture; the final
  table is per sprite number and extra bits, worst first. It exists to find a custom sprite
  drawn with the wrong graphics, colours, or not at all, which scores far below one that has
  merely animated or moved. Run on 2026-09-24 over 12 `dump_hack.sh` levels each of Akogare2,
  Grand Poo World 2, Luminescent, Invictus, Super Hark Bros 2, and QLDC 2021 `70_DPBOX`,
  `77_NerDose` and `44_Daizo Dee Von` (84 levels, 108 sprite scores; captures and
  scores in `~/.local/share/kobo/oracle/sprite-video/`), every custom sprite that scored
  under 85% was looked at side by side and found drawn as the emulator draws it, at its
  first-frame position (Akogare2 `008`'s piranha plant is up its stem, NerDose `003`'s
  mushrooms have fallen). The whole-picture scores under 85% are layer 2 parallax positions
  (Akogare2 `11A`, GPW2 `107`, DPBOX `102`), an HDMA sky (Luminescent `154`), a player still
  in his pipe in the frame (Invictus `152`, Luminescent `142`), and Invictus `030`'s layer 3
  fog. NerDose `104` scores its info box (`B9`) at 0% because the emulator's frame masks the
  main screen with a window where it and Mario stand (the dumped `windowMaskMain` has BG1,
  BG3 and objects on): the object is in the dumped OAM where the capture puts it. Daizo's
  capture got one level: the emulator stays in game mode `$14` on `026`, the cutscene level
  whose sprite waits for a button ([known-gaps.md](known-gaps.md)).
- Lunar Magic exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
  The fixtures were made with 3.21. Lunar Magic 3.70's `-ExportGFX` of the vanilla ROM
  differs in one file: `GFX17` has `$FF` where 3.21 and `gfx export` have `$00`, in 32
  bytes from offset `$11`. Regenerating the fixtures with 3.70 needs that explained first.
  3.70 ships a 64-bit build (`x64/Lunar Magic.exe`) that runs under 64-bit Wine alone.
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
  hack put behind it); the two must agree. It runs on the vanilla ROM and on `KOBO_LM_ROMS`,
  skipping locked ROMs. QLDC 2021 `34_idol` (a BPS patch) is the one LC_LZ3 hack in the
  corpus, so list its `.bps` to exercise that decoder; all 50 of its files agree, as do those of
  LC_LZ2 hacks on LoROM, SA-1, and a 6 MiB SA-1 image. Lunar Magic's `-ExportGFX` of it
  agrees on all 52 files as well (`fixtures/lunar_magic_gfx_export.txt`, by ROM hash).
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
- **CPU suite**: `tests/cpu_single_step.rs` runs the 65816 core against SingleStepTests
  (10,000 native-mode tests per opcode, about a second in release) when `KOBO_65816_TESTS`
  points at the suite's `v1` directory. The native files are in
  `~/.local/share/kobo/cpu-tests/65816/v1` (sparse clone of `SingleStepTests/65816`, 1.7 GiB,
  no licence, never committed). `BRK`, `COP`, `WAI`, and `STP` are left out (the core stops
  on them by design), and the suite's block moves are cut off after 100 cycles, so those
  are compared over the bytes moved. All other opcodes pass in full. SMW itself never sets
  decimal mode; custom code may.
- **Lunar Magic hacks**: `tests/layer2_background.rs` runs on every ROM listed in `KOBO_LM_ROMS`
  (`:`-separated paths) as well as the vanilla ROM. It rebuilds the layer 2 tilemap the game
  uploaded to VRAM from the captured background buffer and BG Map16 table, which catches a
  clobbered buffer or a table read from the wrong place without external data. The corpus
  is `~/.local/share/kobo/roms`: loose `.smc` hacks (Lunar Magic 1.62 to 3.33), QLDC 2021
  and 2022 as BPS patches, and `corpus_more/`, later downloads distributed as BPS (3.21 to
  3.51, among them the corpus's first 3.40 and 3.51 saves). `apply_bps.py` in that
  directory writes each patch's ROM next to it, headered, after checking every CRC the patch
  carries; `~/.config/kobo/env.sh` puts the loose ROMs but Smb2dx (the known exception
  below) and `corpus_more`'s in `KOBO_LM_ROMS`. The variable also takes a `.bps` entry, which the tests apply to the
  vanilla ROM in memory (`bps::apply_to_rom`), so the QLDC patches are listed as they are
  distributed; `tests/bps.rs` also rebuilds every listed hack from a patch `bps::create`
  made against the vanilla ROM. A 2026-09-26 run of every `KOBO_LM_ROMS` test on env.sh's
  list and the 128 QLDC patches (175 entries, each test restarted after a failing hack, so
  a hack's first failure only): every loose and `corpus_more` hack passes; of the QLDC
  entries, all rebuild from a patch, and what fails is
  - `layer2_background`: 2021 `34_idol`'s nine levels and `76_Bench-kun`'s eighteen
    ([known-gaps.md](known-gaps.md)), and `76_Bench-kun` level `114`, whose
    background is missing entirely (2048 of 2048 words; it renders black, with SA-1
    `$002FFF` and `$420B`, `$2130` reads reported unmodelled);
  - `gfx_decompression`: 2021 `70_DPBOX`, whose `GFX03` decompresses to 4095 bytes, so
    `GfxReader` refuses it (the ROM's own routine gives the same 4095);
  - `level_data`: 2021 `34_idol` level `012`, one of its nine, whose sprite list at
    `$E38008` parses past its 206-byte RATS block;
  - `sprite_lists`: 2021 `32_theunkaizoing` and `62_Rykon-V73` level `012`, vanilla's
    empty list at `$07E76D` with a RATS tag in front that claims the rest of bank `$07`;
    2021 `69_bebn legg+E-man38` level `1CB` (13 bytes, block 14) and `77_NerDose` level
    `136` (407, block 408), a block one byte longer than the list; and 2022 `09_idol`
    level `105`, 32 screens of 64 rows, more than the planes hold.

  All 512 levels of each `corpus_more` hack render without a fatal error (2026-09-25);
  none has Lunar Magic export hashes in the fixtures yet. Hacks whose
  headerless SHA-1 is in `fixtures/lunar_magic_map16_bg_export.txt` also have their BG table
  hashed against Lunar Magic's `-ExportAllMap16` output (file tile index `8000`-`81FF`).
  The rows checked are the 16 the game uploads from the layer 2 position, or from a
  position one row either side: the upload comes before the level loop's first camera
  update, which settles layer 2 by a few pixels in some levels.
  A 2026-09-22 rerun also found Akogare2 levels `0F8` (1792/1920 words) and
  `111` (21/2048 words) failing this tilemap check. Both reproduced at `9b69ab8`, before
  the input hardening and operation control. `0F8` passes since the loader runs the ROM's
  NMI at the frame boundaries (its picture was garbage without the upload the third
  blank does). `111` failed because the player pass kept every VRAM upload of its
  entrance frames, among them the rows uploaded as the level's own code pans layer 2
  upward from its second dozen frames, while the
  level's layer 2 position stayed the loader's; the pass now puts the tilemaps back
  ([smw.md](smw.md)), and an emulator frame of the level agreed with the expected words.
  The failure message lists the words that differ.
  Known exception in the corpus: `Smb2dx` (LM 1.63; 173 levels fail, its mode `$00`
  levels carry object layer 2 pointers), failing before the vertical-level checks were
  added. `Super Hark Bros 2` level `00A` used to fail with 896 of 2048 words: its
  level-init code leaves layer 2 at `$5D` and the game had uploaded for `$C0`, which the
  camera update `expand` now runs after preparation restores.
- **Project build**: `tests/project_build.rs` imports every vanilla level into a
  project, requires Kobo's formatting to be a fixed point on every file, builds it (twice,
  for byte-identical output), and requires every level to read back as vanilla's, its
  layer 1 data in the expanded ROM, and seven levels of different kinds to render the
  same picture. A build through the stage cache must equal one without, cold, warm, and
  after an edit. `a_synthetic_build_is_the_same_everywhere` needs no ROM: it builds
  `fixtures/synthetic_level.toml` onto `common::synthetic_base()` and pins the output's
  SHA-1, so CI shows whether all three platforms build the same bytes; a change to what a
  build writes changes the hash on purpose. The check of all 512 pictures is by hand, as for any change that should
  not change a picture:

  ```sh
  kobo import "$KOBO_SMW_ROM" /tmp/p --all && kobo build /tmp/p -o /tmp/built.sfc
  cargo run --release --example render_hashes -- "$KOBO_SMW_ROM" > vanilla.txt
  cargo run --release --example render_hashes -- /tmp/built.sfc > built.txt
  cmp vanilla.txt built.txt
  ```
- **Corpus sweep**: `tools/corpus-sweep out/ [hack.smc|patch.bps ...]` (default: the
  hacks in `KOBO_LM_ROMS`) imports each hack's changed levels, builds them, leaving out
  each level a build refuses until the rest builds, then runs `kobo diff`, `render_hashes`
  over the levels built (it takes a list of levels after the ROM), and `save-check`. It
  writes each hack's project, build, and log to `out/<hack>/`, and `out/results.json` and
  `out/summary.md` (each hack, then refusals and import notes grouped by kind). It uses
  `target/release`'s `kobo` (or `KOBO_BIN`) and `render_hashes`, so build both with
  `--release` first.
  A run stopped part way resumes from `results.json`; `--summary` only rewrites the summary,
  and `--no-render` and `--no-save` skip the slow steps. The whole corpus (175 entries,
  QLDC 2021's and 2022's `.bps` patches included) takes about two hours on four cores,
  mostly in rendering, and about 500 MB: point `out` and `TMPDIR` at a disk, not a `/tmp` in
  memory. The 2026-10-01 run's results are in [step-2.md](step-2.md) (work order, item 2).
  `save-check` leaves its work folder (the saved copy, Lunar Magic's files) in
  `SAVE_CHECK_KEEP` when that is set.
- **Kobo's ROM-side code**: `tests/install.rs` applies `kobo_core::install`'s patches to
  vanilla (Asar's library needed) and runs the ROM: Map16 lookups for pages 0 and 1 as the
  game's, pages past 1 from tables written where Lunar Magic's layout points, and a few
  levels drawn as vanilla. After a change to a patch, compare `render_hashes` of all 512
  levels with vanilla's by hand (`kobo rom expand 1M`, then `kobo asm` each patch).
- **Map16 pages**: `tests/map16_pages.rs` builds pages in three groups of 16, one past
  `$40`, onto the synthetic base (Asar needed) and reads them back as written; with the
  vanilla ROM it also resolves them through the ROM's own tilemap upload and draws two
  vanilla levels as vanilla does. By hand: `kobo import` Kaizo Kindergarten, keep only
  `[map16]` in the manifest, build, import the build, and `diff -r` the `map16` folders
  (identical); `tools/lunar-magic/save-check` the build and import the saved copy: the
  pages must come back the same. Pages 0 and 1 and the tilesets' own tiles, with page 2
  per tileset: `game_pages_build_and_read_back` builds changes to each onto vanilla, reads
  them back, and resolves them through the game's uploads in levels `103`, `105`, `107`,
  `101`, and `1D` (tilesets 0, 7, 5, 1, 3); the refusals have a test of their own; and
  with `KOBO_LM_ROMS`, every LoROM hack's pages 0 and 1 imported and built give its
  tiles in all 15 tilesets and its acts-like settings, and import again the same (all
  173 LoROM ROMs of the corpus and the QLDC entries pass).
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
  columns); `POKE_TILE=dx,dy,tile` puts a
  tile (a coin) beside the player first. For where a list's files go, make ExGFX
  files whose every 16-byte unit starts `A5 5A f j` and look for them in the VRAM dump.
- **Kobo's VRAM patch against Lunar Magic's**: `tools/lunar-magic/with-kobo-vram lm.smc
  out.sfc` swaps `vram.asm` into a Lunar Magic-saved ROM; then `gfx_probe loads` and
  `scroll` compare the two. On vanilla saved once by Lunar Magic (2026-09-28): the same
  tilemaps after every level's load but for the unseen row above a level's top, and
  nothing visible different for longer than a frame on any level along seven paths
  (`3,0`, `0,-2`, `3,-2@80/-2,2`, `3,2`, `-3,-3`, `1,-1@80/-1,1`, `6,-5@60/-7,6`), held
  still (`0,0`), and three that jump (`-1000,0@1/3,0`, `1000,0@1/-3,0`,
  `400,0@1/0,0@30/-400,0@1/0,0`), 240 frames each (2026-10-04). `exlevel_probe call
  ... frames=N then=frame vramw=...` shows where a tile changed in play goes.
  `tests/install.rs` (`the_vram_patch_keeps_layer_1_in_view`) checks, with the ROM and
  Asar, that every cell in view on layer 1 holds its tile's definition along paths in a
  horizontal, a vertical, and a layer 2 objects level.
- **Kobo's taller levels against Lunar Magic's**: `tools/lunar-magic/with-kobo-exlevel
  lm.smc out.sfc` puts every site of Lunar Magic's taller levels piece back to the game's
  bytes, applies `exlevel.asm`, and moves the size table to where it then is; swap
  Kobo's sprite loader and VRAM patch into both sides first (`with-kobo-sprites`,
  `with-kobo-vram`), since swapping in one piece alone leaves the other's RAM otherwise.
  `examples/exlevel_probe.rs` then compares them: `compare a b level` plays a level in
  both frame by frame (the player carried, `path=`, or free with buttons held, `pad=`;
  `hold:`/`ram:` for RAM such as `$1887`, the ground shaking, or `$1412`, the vertical
  scroll setting), `sizes rom level` shows what each size byte changes at the entrance,
  `call rom level addr jsl|jsr` runs one routine with chosen RAM and registers
  (`expand::call_in_level`), `ram`, `show`, `entry` print the tables, words per
  frame, and the entrance's RAM, and `cells a b level frame` draws which Map16 cells of
  layer 1's and 2's tilemaps differ on a frame, in view or not. `entry_probe compare` with `KOBO_ENTRY_SIZE` varies the
  size byte along with the entrance bytes. Hacks with taller levels are moved into a
  3.70 ROM with `transfer` first: their own ROMs carry older versions' code. The results
  (2026-09-28) are in docs/lunar-magic-install.md ("Taller levels").
  `tests/taller_levels.rs` builds every size with and without `B` and requires the
  objects where the size puts them and the RAM Lunar Magic's code leaves, layer 2's
  split, and the refusals; with `KOBO_LM_ROMS` and `KOBO_MWL_DIR`, every corpus ROM's
  size table read where the layout has it must equal its MWL exports.
  `lunar_magic_save.rs` (`a_taller_level_build_survives_a_lunar_magic_save`) has Lunar
  Magic save a build with a taller level, which must keep its size and Kobo's code, and
  without the `JSL` at `$05DA8A` lose the size.
- **Kobo's graphics loader against Lunar Magic's**: `tools/lunar-magic/with-kobo-graphics
  lm.smc out.sfc` swaps `graphics.asm` into a Lunar Magic-saved ROM with 4bpp files (and
  ExGFX and lists, if it has them), keeping its tables; `WITH_VRAM=1` swaps the VRAM patch
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
  `KOBO_LM_ROMS`, imports every unlocked corpus hack's ExGFX files, older lists, and the
  lists a build takes, builds them onto vanilla levels, and reads them back (41 hacks,
  2026-09-28);
  `tests/lunar_magic_save.rs` (`a_graphics_build_survives_a_lunar_magic_save`) has Lunar
  Magic save such a build, with and without each checked byte.
- **Layer 3 settings**: `examples/layer3_probe.rs` plays a level along a path and prints,
  per frame, layer 3's position and RAM, colour math and screens, and the scroll and
  screen registers the frame's NMI and IRQ left (`play`, with `WATCH` and `POKE`);
  `compare` and `loads` report where two ROMs differ in those, work RAM, and layer 3's
  VRAM; `sweep a b frames` reads cases (`level path SLOT:n ...`) from standard input,
  writes each list into both ROMs, and reports the cases that differ.
  `tools/lunar-magic/with-kobo-layer3 lm.smc out.sfc` puts Kobo's layer 3 code
  (`layer3.asm`) in place of Lunar Magic's. Found and checked against a copy of `+ExGFX`
  with a level imported with settings (so it has Lunar Magic's code), with settings then
  written straight into its lists (2026-09-30): 1,500 random cases all the same, and
  Kaizo Kindergarten's content (`transfer`) all the same. `tests/layer3_settings.rs`
  builds level 105 with settings and checks layer 3 frame by frame, and what builds
  refuse; with `KOBO_LM_ROMS`, it puts Kobo's code in every corpus hack that has Lunar
  Magic's, saved by 3.10 or later (13 hacks, 2026-10-01), and plays each level with
  settings under both, comparing layer 3's position,
  colour math, screens, scroll type, registers, and tilemap (not the code's own state,
  which older versions keep otherwise). `tests/lunar_magic_save.rs`
  (`a_layer3_build_survives_a_lunar_magic_save`) has Lunar Magic save such a build, with
  and without the `JSL` at `$00A01F`.
- **PIXI**: `tests/tool_stages.rs` runs PIXI (configured, or the pinned build in the
  cache; `KOBO_REQUIRE_PIXI=1` makes its absence a failure, as CI does) on a synthetic
  image (`pixi_runs_without_a_rom`) and as a build's sprites stage with a sprite placed in
  a level (`pixi_inserts_sprites`, with the ROM); `tests/lunar_magic_save.rs`
  (`a_pixi_build_survives_a_lunar_magic_save`) has Lunar Magic save such a build and
  plays level `0EB` across its secret-exit goal tape in the clean ROM, the build, and the
  saved copy. `pixi_insert_is_read_whole` runs PIXI on the synthetic image with a sprite
  of every type and a shared routine and requires `pixi::read`'s insert, written onto the
  image PIXI ran on, to give PIXI's image again; `pixi_sprites_carry_through_an_import`
  (with the ROM) imports a PIXI build both ways, compiled and with the folder, and plays
  the sprite in each rebuild. `pixi`'s unit tests cover the walk on a synthetic insert,
  `PROT` lists, and a jump at a site Lunar Magic shares that is not into PIXI's block.
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
  the hack itself, also `00D1-00D4` and `1DEA`. Kaizo Kindergarten's `levels` against
  Kobo's build: every level the same by a screen exit, by `w`, and from the overworld with
  and without the midway point, but for the level the build leaves out (and those the
  overworld override maps onto it, and one vertical "No Yoshi" intro). The overworld can
  name a level only up to low byte `$DB` (`CODE_05D8A2` takes `$24` off a name of `$25` and
  up); the runs recorded here named levels `125`-`13B` by their low byte, so entered
  `101`-`117` for them, which the probe no longer does. Both sides of each comparison ran
  the same way.
- **A hack built by Kobo**: `kobo import` Kaizo Kindergarten, leave out the levels the
  build refuses, build, and `examples/tiles_diff.rs hack.smc out.sfc levels...`, which
  compares what every level's load resolves (grid, background tilemap, Map16, BG Map16),
  graphics aside: all 337 the same. `tools/lunar-magic/save-check out.sfc 105 project`
  passes, and `tiles_diff` of the saved copy against the hack gives the same. Import, build,
  and import again gives the same files. A background in Lunar Magic's older format (`C`
  without `F`: Kaizo Mario 1, 2, and World 3) builds as 32 rows of its own format:
  Kaizo Mario 2's six such levels load the same BG2 VRAM, but `tiles_diff` reports their
  "background tilemap", whose stride goes from `$1B0` to `$200`.
- **A hack's content through Kobo's code**: transfer a hack into a Lunar Magic-saved
  vanilla ROM with Lunar Magic's command line (`tools/lunar-magic/transfer hack.smc
  mwl-dir out.smc`: `-ImportAllGraphics` of its `-ExportGFX`/`-ExportExGFX` first, then
  `-ImportAllMap16`, `-ImportSharedPalette`, `-ImportMultLevels` of its MWL exports,
  `-TransferLevelGlobalExAnim`), then `tools/lunar-magic/with-kobo`
  swaps Kobo's bank `$06` code in, keeping the tables, and `render_hashes` and
  `ramdiff.py --summary` compare the two over all 512 levels. Do not import into a Kobo
  install instead: Lunar Magic's save then installs most of its own code over it
  ([lunar-magic-install.md](lunar-magic-install.md)). Kaizo Kindergarten passes.
  `with-kobo` also works on a hack itself, for its Lunar Magic 2.52-and-later layout:
  Grand Poo World 2, Invictus, Luminescent, Baby Kaizo World 3, and SMW_2022-4-9 draw every
  level alike, but for Grand Poo World 2's `09F` (no background table) and 3 to 9 levels
  each whose pictures differ only with sprites, most likely because the swap drops the
  hack's GPS blocks (not yet confirmed; 2026-09-26).
- **Tool stages**: `tests/tool_stages.rs` builds two Asar patches, early and late, one
  including a file, onto the synthetic base when Asar's library is configured (no ROM):
  they apply in order, the output repeats, and changing the included file changes the
  build. With `KOBO_ADDMUSICK` (an AddmusicK folder; `~/src/addmusick` here) and the
  vanilla ROM, a project with an empty music folder gets AddmusicK's default music
  (`@AMK` at `$0E8000`), the same bytes twice.
- **GPS**: with `KOBO_GPS` (a folder with GPS built for the system and its files;
  `~/.local/share/kobo/tools/gps-1.4.4` here), `tool_stages.rs` inserts a block acting like
  `$025` at tile `$200` into Kobo's acts-like chain and checks the table and GPS's entry.
- **UberASM Tool**: with UberASM Tool found offline (`KOBO_UBERASM`, a folder with the
  program built for the platform and its files, or the pinned build in the cache after
  `kobo tools fetch`), `tool_stages.rs` inserts one level's code, the same bytes twice.
  It also runs the tool on an image with no game data, which needs no ROM; CI does that on
  all three platforms with the pinned build and requires it (`KOBO_REQUIRE_UBERASM=1`).
  A folder that brings its own Asar library, as upstream's 32-bit Windows release does,
  keeps it.
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
  the same and render four levels the same. All 512 pictures, by hand: make the reference
  ROM as [sa1.md](sa1.md) says (`~/.local/share/kobo/roms/sa1/smw-sa1.sfc` here), `kobo
  import` it `--all`, `kobo build`, and compare `render_hashes` of the two.
  `sa1_projects_store_gfx_as_lz3` builds an empty SA-1 project with `[rom] lz3`: SA-1
  Pack's routine must read every GFX file back as the clean ROM's, and three levels load
  the same VRAM and draw the same as in the LC_LZ2 build; by hand, `render_hashes` of the
  two agree on all 512. With Lunar Magic as well, `lunar_magic_save.rs`
  (`lunar_magic_reads_an_lz3_build`) requires its `-ExportGFX` of the LC_LZ3 build to
  equal that of the LC_LZ2 one, and its save to keep `$0FFFEB`, and without `$0FFFFF` set,
  neither.
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
  (300 further scenarios each passed by hand, 2026-10-01), and
  `kobos_layer3_code_plays_as_lunar_magics` takes the corpus's SA-1 hacks with SA-1 Pack's
  bytes at the hook sites. By hand: the twelve base patches on the SA-1 reference ROM,
  `render_hashes` against the reference ROM (all 512 levels the same but `012`, `0F8`,
  and `101`, which draw as the LoROM build does), and the `tools/lunar-magic/with-kobo*`
  scripts on Extended Interactions (an SA-1 hack's sites go back to SA-1 Pack's bytes:
  `base-rom`, `KOBO_SA1_BASE`). Results (2026-10-01): on the SA-1 reference ROM saved
  once by Lunar Magic, the VRAM patch and the base pieces swapped in draw all 512
  levels as Lunar Magic's code does (and on vanilla saved once, as before). Extended
  Interactions' content moved into it (`transfer`, which takes an SA-1 hack into the
  SA-1 base): ExAnimation, taller levels, layer 3, and the sprite loader draw every level
  as Lunar Magic's code does; the base pieces differ in `01B` and `047`, the graphics
  loader in `00E`, `03B`, `0DE`, `180`, `189`, and `1DD`, and the VRAM patch in 18 levels,
  and the same content moved into the LoROM vanilla ROM differs in exactly the same
  levels: gaps of Kobo's code with this content, not of SA-1, left for the corpus sweep
  (step-2.md). Level `006` breaks (`BRK`) in both ROMs alike: its custom content needs
  the hack's own code.
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
- **Sprite loader**: `examples/sprite_probe.rs` writes a sprite list into a copy of a ROM
  (`list=new,mem=N,id:x:y[:extra]...`, at `$3F8000` through Lunar Magic's bank table), sets
  ROM and RAM bytes, carries the player along a `route=frame:x:y/...` (with `$13`
  counted as the game loop counts it), and prints each frame's sprite slots, load flags,
  and watched RAM (`play`), the frame each entry first loaded with the camera then
  (`spawns`), or where two ROMs differ (`compare`). Compare vanilla saved once by Lunar
  Magic with the same ROM with the loader group put back to the game's bytes and
  `sprites.asm` applied (`tools/lunar-magic/with-kobo-sprites`); `lunar_magic_save.rs` (`kobos_sprite_loader_spawns_as_lunar_magics`,
  `KOBO_LOADER_SEED` and `KOBO_LOADER_CASES` to vary it) does that for 24 seeded random
  scenarios, and must find no frame that differs. `SPRITE_LOADER_SITES` there is the group,
  which a save must leave as a build wrote it. By hand, Kaizo Kindergarten with Kobo's
  loader swapped in the same way (and PIXI's goal tape init kept) against the hack, every
  level, moving right and along a path up and down: the same.
- **ExAnimation in builds**: `tests/mwl_files.rs` (with `KOBO_MWL_DIR`) requires every
  level's list read from the ROM (but Lunar Magic 2.41's older lists, which Kobo does not
  read from a ROM) to equal the
  one in its MWL export, byte for byte when written again; `tests/lunar_magic_save.rs`
  builds level 105 with a list and settings, a global list, and file `60`, and requires
  them all after a save, the settings lost without `$03FDFF`, and the lists lost without
  the `JSL` at `$00A390`.
- **ExAnimation**: `examples/exanim_probe.rs run rom level frames` plays a level through
  the game loop and its NMI and prints, per frame, the VRAM words and CGRAM colours the
  frame changed and `$7FC000`-`$7FC0FF`; `ab a b level frames` prints the frames whose
  VRAM, CGRAM, or ExAnimation RAM differ between two ROMs. Chosen data goes into a copy in
  memory (`ANIM`, `GLOBAL`, `SETTINGS`, `PALETTE`), RAM before the load (`ENTRY`) and
  before frames (`SET`), and a word-numbered pattern at `$7EAD00` (`FILE`), so an upload
  shows where it came from. `tools/lunar-magic/with-kobo-exanim lm.smc out.sfc` swaps
  Kobo's code into a ROM with Lunar Magic's, keeping its tables. `tests/lunar_magic_exanimation.rs`
  (with `KOBO_LUNAR_MAGIC`) has Lunar Magic install its own on the clean ROM, swaps Kobo's
  in, and plays seeded random lists, settings, trigger states, and events for 96 frames
  in both (`KOBO_EXANIM_SEED`, `KOBO_EXANIM_CASES`, 16 by default; a failure names the
  first differing byte, the lists, and the events; `KOBO_EXANIM_KEEP=dir` writes the two
  ROMs there, for `exanim_probe ab` with chosen lists); the player's tile
  words are left out (lunar-magic-install.md). Every trigger on every type is drawn
  but `0F` on frame types; 400 cases each of seeds 1 and 7 passed on both mappings
  (2026-10-01). The same comparison, hack by hack, on
  every level with a list (and every 16th with a global one) of the corpus's 32 LoROM
  hacks with ExAnimation found no difference in 1,332 levels (2026-09-28; Invictus level
  `136` breaks to `BRK` in both; Extended Interactions and Super Diagonal Mario 2, being
  SA-1, were left out then: see "Lunar Magic's layout on SA-1" above).
- **Lunar Magic check**: `tools/lunar-magic/save-check built.sfc [level [project]]` has Lunar Magic
  3.70 save a copy of a ROM (exporting a level and importing it back) and runs `kobo diff`
  on the two: every level must read the same. `tests/lunar_magic_save.rs` does the same
  when `KOBO_LUNAR_MAGIC` is Lunar Magic 3.70's folder (`env.sh` sets it; Linux, with Wine
  and `xvfb-run`; CI has no Lunar Magic): a build of level `105` with Lunar Magic objects,
  a palette of its own, entrance settings, Map16 pages in three groups, changes to pages
  0 and 1, and page 2 per tileset must read back the same after a save, pages included;
  without the checked bytes below, only the level's `entrances` may differ. Lunar Magic's
  `-ExportAllMap16` of the same build must show every acts-like setting, every tileset's
  pages 0 and 1 and page 2, and every page the build lists as written (the file's layout is
  in [lunar-magic.md](lunar-magic.md)); without the marker at `$06F5FC` it shows the
  acts-like settings, page 2 per tileset, and pages past `$0F` wrong, and nothing else. A
  new step 2b piece adds its part to that build. A build of the whole vanilla import passes
  with levels `105` and `106`. The Lunar Magic features of step 2b are each to be checked
  this way. Once a build writes any of the bytes Kobo writes only because Lunar Magic
  checks them ([lunar-magic-install.md](lunar-magic-install.md#bytes-kobo-writes-because-lunar-magic-checks-them)),
  every feature's check runs twice, with those bytes and with them cleared back to what a
  build without them has, and anything Lunar Magic then does differently (installs, keeps,
  or drops) is recorded in that section.
- **Level data**: `tests/level_data.rs` decodes and encodes every level's object data,
  sprite list, and distinct background of the vanilla ROM and of every `KOBO_LM_ROMS` ROM
  but the locked ones, and requires the same objects, sprites, and tiles back, an encoding
  no longer than the stored one, and a stored length within the RATS block holding it. On
  vanilla all 538 object lists and 512 sprite lists but 18 object lists come out byte for
  byte ([smw.md](smw.md)). In the corpus, 70% to 100% of each ROM's lists do; the rest are
  Lunar Magic's encoding choices (every run on 2026-09-25 passed).
- **MWL files**: `tests/mwl_files.rs` reads Lunar Magic's MWL exports when `KOBO_MWL_DIR`
  is set, a directory of directories each holding one ROM (`.smc` or `.sfc`) and the MWL
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
  every one Lunar Magic opens (it refuses the seven locked ROMs and Smb2dx); all 21,504
  files passed on 2026-09-25, in two seconds:

  ```sh
  tools/lunar-magic/export-mwl ~/.local/share/kobo/mwl ~/.local/share/kobo/roms/*.smc \
    ~/.local/share/kobo/roms/corpus_more/*.smc
  KOBO_MWL_DIR=~/.local/share/kobo/mwl cargo test --release --test mwl_files -- --nocapture
  ```
  `vanilla_exports_import_and_build` imports all 512 vanilla exports into one project
  with `import_mwl`, builds it, and allows only those rewrites in `kobo diff` against
  vanilla: the background of the 276 levels on the shared empty level, `0C5`'s header,
  and layer 1 of eleven levels. It also compares the screen exits' bytes, which `kobo diff`
  puts in one format: the import writes them back in the game's format.
- **SA-1**: the oracle script reads SA-1 Pack's RAM map (`ram()` in `dump_levels.lua` is
  `RamMap::Sa1Pack` for what it touches, and the full-WRAM dump is laid out as vanilla's)
  and hooks the pointer lookup on the SA-1 too, where the level loader runs. With
  `KOBO_ORACLE_ROM` on the reference ROM from [sa1.md](sa1.md), the tile grids of all 512
  levels, the layer 3 tilemaps, and the sprite slots match `sa1-all/`, and the 13 frames in
  `sa1-video/` match at 94.9-99.7%. `render_hashes` against the vanilla ROM is the other
  check: the marker column must match on every level but the three boss arenas, and the
  differences in the drawn column are SA-1 Pack's own ([sa1.md](sa1.md)). The corpus has
  40 SA-1 hacks: `Super Diagonal Mario 2`, `corpus_more`'s `Extended Interactions`, and 38
  QLDC 2021 and 2022 entries, which are BPS patches: `KOBO_LM_ROMS` takes them as they are,
  while `render_hashes` and the CLI need `kobo bps apply` first. `render_hashes` on each says whether the code ran,
  not whether the pictures are right; what fails is in [known-gaps.md](known-gaps.md).
- **Picture hashes**: `cargo run --release --example render_hashes -- rom.smc` prints a SHA-1
  of every level's picture, with sprites drawn and again as markers without the player. A
  change to `expand` or `render` that should leave every picture alone is checked by diffing
  its output before and after, on the vanilla ROM and a handful of hacks from the corpus.
  It does not report nonfatal `LevelRender` diagnostics: a hash can describe a picture
  whose player or sprite passes failed. Use the CLI's warnings or inspect the returned
  diagnostics when checking execution coverage.

## Full hack render sweep

The 2026-09-22 sweep at revision `25cca50e847ee679c500e2a287ef3e71faf3322f` ran
`kobo level png` in release mode on every slot `000`–`1FF`, with default sprite and
player rendering. It recursively included the local hack collection's ROMs and all
129 BPS patches, including QLDC entries and development projects. All patches applied
successfully to the headerless vanilla ROM. Grouping identical headerless ROM SHA-1s
and excluding five unmodified vanilla copies left 173 distinct hacks.

All 88,576 slots were attempted: 88,522 PNGs, 54 fatal failures, and 209 PNGs with
warnings; no attempt reached the export script's 120-second timeout. The failures and
warnings affect ten hacks. Their status and investigation priorities are recorded in
[known-gaps.md](known-gaps.md#full-hack-render-sweep-2026-09-22). This was not an emulator
comparison or a visual review of every PNG, and includes slots that may not be playable.

The local, uncommitted output is `~/Pictures/Kobo-level-renders/2026-09-22/`:

- `manifest.json`: source paths, headerless ROM hashes, duplicate aliases, exclusions,
  and the renderer revision. Temporary patched-ROM paths no longer exist; reapply the
  source BPS patch (`kobo bps apply`) when reproducing one of those entries.
- `results.jsonl`: every attempted slot's status and complete CLI diagnostics.
- `run-report.md`, `diagnostics.tsv`: per-hack totals and the failures and warnings.
- `index.html`: the PNG gallery, with diagnostic filters and optional filters for
  pictures identical to vanilla or to another slot in the same hack.
- `scripts/`: the one-off export, gallery, reporting and verification scripts. The
  export script starts a fresh run; use a separate output directory to keep this snapshot.

To reproduce an individual slot with its diagnostics, apply its patch if needed, then run:

```sh
cargo run --release -- level png 105 /tmp/kobo-level-105.png -r /path/to/hack.sfc
```

The export verification checked PNG headers and dimensions, preview presence, all
512 results per hack, and gallery links. These checks establish output completeness,
not correctness of the rendered game state.

## Strict runs

`cargo test --workspace` skips the ROM-backed tests when no vanilla ROM is configured, so
a green default run says nothing about ROM compatibility. `KOBO_REQUIRE_ROM=1` turns that
skip into a failure. A configured vanilla ROM must have the reference headerless SHA-1, a
malformed configuration is an error rather than a skip, and an opt-in tier whose variable
is set but names no ROMs, dumps, or frames fails instead of passing without checking
anything.

## Parser mutation checks

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
