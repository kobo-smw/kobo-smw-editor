# Kobo

An open-source Super Mario World ROM editor and build system.
Desktop app for Windows, Linux, and macOS.
Early stage: roadmap steps 1 to 3 are complete; step 4 is under way (`docs/step-4.md`).

## Principles

1. **Open source.** The community must be able to read, fork, and customise everything.
2. **ROMs are compiled from source.** A project is a directory of level files, sprites, blocks, ASM,
   graphics, overworld definitions, etc, and a manifest. The build takes a clean SMW ROM plus that
   directory and produces a ROM. Builds are repeatable and byte-identical for identical inputs.
   Projects are git-friendly.
3. **Everything is scriptable.** Every operation lives in a core library. The GUI and the CLI are
   thin shells over it. Batchable operations (build, import, export, validate, diff, render) are
   exposed on the CLI. Interactive editing is a GUI concern; programmatic editing goes through a
   scripting API, not CLI flags.
4. **Compatibility with existing tools**. The built ROM must use the same
   hijacks, tables, and data layouts that Lunar Magic installs, so PIXI, GPS, UberASM Tool, AddmusicK,
   Asar patches, and custom sprite display files (.ssc, .mwt, .mw2, .s16) keep working. Importing an 
   existing Lunar Magic hack (MWL, Map16, ExGFX, palettes) is a first-class feature. Exceptions may be
   tolerated but heavily discouraged and scrutinised.

## Guardrails

- **Reuse the open-source toolchain.** Orchestrate Asar, PIXI, GPS, UberASM Tool, and AddmusicK.
  Do not reimplement them. Pin tool versions.
- **Patch-based, not disassembly-based.** the build patches a vanilla ROM. Do not build on a full
  SMW disassembly (e.g. SMWDisX); it breaks every fixed-address patch in the ecosystem. Keep the
  architecture from precluding it, but do not pursue it.
- **The vanilla ROM never lives in a project.** Reference it by hash. Locate it through per-user
  config or an environment variable. Never commit or distribute ROM data. Support BPS patch output.
- **Canonical source formats are textual and merge-friendly.** One object or sprite per line where
  practical. Binary formats (MWL, Map16 exports, raw GFX) are import/export interop only.
  Round-trip fidelity against Lunar Magic exports is tested, not assumed.
- **ROM-side code is source.** Anything the editor installs into the ROM (level format expansion,
  custom sprite loading, and so on) is an Asar patch checked into this repo.
- **Determinism is a requirement.** Fixed tool order, deterministic freespace allocation, pinned
  versions. Design for incremental builds; AddmusicK is slow.
- **Support SA-1 from the start.** Address mapping (LoROM, SA-1, FastROM) is an abstraction, never
  hard-coded.
- **SMW only.** Keep game facts data-driven inside the library, but do not build a game-agnostic
  engine.
- **Clean room: look at Lunar Magic only to interoperate with it, and do the least that
  takes.** Hook addresses, table and block locations and formats, the pointers it keeps at
  fixed offsets in its code, the RAM values hooks leave behind, and the bytes Lunar Magic
  or another tool checks (a marker, the opcode before such a pointer) are matched exactly,
  no more than a check needs, each recorded and tested with and without. The code behind a
  hook is Kobo's own, written from community documentation (SMWCentral, SNESLab), Lunar
  Magic's readme and help, open tools' sources, byte diffs of ROMs before and after a Lunar
  Magic operation, bisecting which bytes it checks, and the memory effects of running Lunar
  Magic-saved ROMs. Never read Lunar Magic's instructions (a disassembly of the executable
  or of the code it puts in a ROM, or an instruction trace of that code), and never copy
  its code. `docs/clean-room.md` has why.
- **Scope discipline.** Do not chase Lunar Magic feature parity before shipping something usable.
- **License is MPL-2.0 across the board.** Application, core library, CLI, and ROM-side patches.
  New dependencies must be MPL-compatible; check each tool's license before adopting it.

## Roadmap (in order)

1. Core library and CLI that reads a vanilla or Lunar-Magic-modified ROM and renders any level to
   PNG. Validate parsers against real hacks.
2. Build pipeline with native level, Map16, ExGFX, and palette insertion in the Lunar Magic layout,
   plus MWL and ROM import, and the toolchain run in a fixed order. Lunar Magic opens and saves
   a Kobo build without loss, so what Kobo does not cover yet can be finished there.
3. GUI level editor.
4. Overworld, Layer 3, graphics and palette editing, emulator integration
   (play-from-level, Mesen-S / bsnes-plus debugging).

## Stack and layout

- **Rust** (pinned in `rust-toolchain.toml` and `mise.toml`), edition 2024, cargo workspace.
  - `crates/kobo-core`: the library. All logic lives here.
  - `crates/kobo-cli`: the `kobo` binary. Thin shell over the core; no logic of its own.
  - `crates/kobo-editor`: the `kobo-editor` binary, the level editor (egui). Also a thin
    shell: it holds what is on screen, and changes a project only through
    `kobo_core::edit`. Its tests drive the window headlessly (`egui_kittest`).
    `docs/editor.md` is how to use it; each module of it says what it holds (`canvas`,
    `inspector`, `levels`, `palette`, `outline`, `changes`, `find`, `build`, `play`,
    `project`, `start`, `commands`, `backgrounds`, `map16`, `graphics`, `palettes`, `layer3`, `overworld`, `overworld_lists`, `ram_watch`, `dialogs`, `preview` for the worker that
    draws, `thumbnails` for the levels' small pictures, `selection` for what is under the
    mouse).
- `kobo_core::addr` is the only place that knows how SNES addresses map to file offsets.
  Every ROM read takes a `SnesAddr` and goes through the ROM's `Mapping` (LoROM, SA-1, or
  SA-1 over 4 MiB); the bus follows an SA-1's bank registers (`SuperMmc`) once the game
  has written them.
  Conversions mirror Asar's conventions so addresses agree with the rest of the toolchain.
- `kobo_core::ram` is the only place that knows where the game keeps its variables. A
  `RamAddr` names a variable by its vanilla `$7E`/`$7F` address, a `RamMap` resolves it to a
  bus address (`Vanilla`, or `Sa1Pack` with its I-RAM and BW-RAM addresses and 22 sprite
  slots), and `Ram` is the memory itself: `bus.ram.u8(ram::LEVEL_MODE)`, never
  `wram[0x1925]`. Tables are indexed from their resolved start (`u8_at`). A `Ram` clone is a
  snapshot, which on an SA-1 cartridge includes the SA-1 (`cpu::sa1::Sa1`: its registers and
  its `Cpu`), so that restoring one never leaves it mid-handler; video memory is not part of
  it, since the game never reads it back.
- `kobo_core::cpu` has one 65816 core and two instances of it. `Cpu::run` takes IRQs from
  the `Bus` and hands over to `Bus::wait` when the CPU stops to wait (`WAI`, or a loop that
  changes nothing); `SmwBus` gives the SA-1 its turn there, through `Sa1View`, the bus as
  the SA-1 sees it, and failing that the NMI `Bus::vblank` offers, a bounded number of
  times. A wait nothing answers is `CpuError::Waiting`, not 200 million steps.
  `KOBO_CPU_TRACE=<n>` prints the last `n` instructions before a routine fails or a
  `KOBO_RAM_WATCH` address is written; `KOBO_VRAM_WATCH` reports writes to a VRAM word.
  Never trace a ROM Lunar Magic saved: that prints its code (clean room).
  `kobo_core::clean_room` is the one place that tells such a ROM (its marker, or code where
  every save writes its own) and decides what every output withholds once the process has
  loaded one: the trace, a watched write's instruction, CPU errors' addresses, read
  traces' instructions, a called routine's registers, the stacks in any RAM dump, and
  Map16 definitions the ROM's routine found outside its tables (`Map16Shown`). A
  new output that could show where a ROM's code is goes through it; the scripts in
  `tools/` check through `tools/clean_room.py`.
- `kobo_core::expand` runs ROM code. `machine` owns the CPU, the bus, and `Call` (the register
  state a routine is entered with; every call starts from reset registers, and
  `try_call_to` stops one at an address with the call still open); `Machine::interrupt`
  runs the ROM's NMI or IRQ handler whole, from its vector to its `RTI`, since patches
  replace both ends of them. `load` runs the loader phases in game mode `$11`'s own order,
  and `sprite_capture`, `player`, `boss`, `layer3`, and `map16` are the passes over the
  loaded level. `oam` reads a frame's objects as the PPU gets them: the ROM's OAM upload
  runs after every frame and the bus keeps what arrives at `$2102`-`$2104`, so the ROM
  decides which object is in front; do not read `$0200` and `$3F` instead. A sprite pass
  runs the ROM's whole NMI handler for it, on video memory of its own, since some sprites
  have no tiles until it has run; what a pass uploaded for its objects is kept with them
  (`SpriteScene::dynamic`). `routines` holds the ROM addresses. `expand_level` returns a
  `LoadedLevel` of four parts: `tiles` (`LevelTiles`: the grid, Map16 definitions, and layer
  layouts, which is what the level *is*, with `objects`, the `ObjectMap` of which object
  drew each tile, from the loader's data reads and writes as `cpu::watch` logs them), `video` (`VideoMemory`: VRAM, CGRAM, `BGnSC`,
  `OBSEL`), `scene` (`LevelScene`: screen setup, camera, layer 3, player, boss arena), and
  `ram`. A pass the CPU core gives up on is recorded as a `Diagnostic`
  (`LoadedLevel::diagnostics`, `SpriteScene::diagnostics`) instead of failing the level;
  `expand::summarize` turns them into one line per distinct error.
- `render::render_level(rom, level, options)` is the one way to a level's picture (and
  `render_loaded` for a level already loaded): it owns the drawing order (layers, sprite
  scene, the player behind the sprites, compose, markers on top; a boss arena skips the
  sprite capture). The CLI and `tests/video_oracle.rs` both call it; do not rebuild that
  sequence in a shell. `render_overworld(rom, submap)` is the overworld's, from the
  ROM's own load (`expand::load_overworld_on`).
- `render` has one of each PPU primitive, all public so a GUI can redraw a tile or a viewport
  through them; extend these instead of adding a second:
  `Tilemap::pixel` reads any background tilemap (layer 3, an arena's layer 1),
  `character_pixel` any character data, `draw_objects` rasterises `SpriteObject`s,
  `tile_pixels` walks a tile for both the image and the layer drawers
  (`LevelLayers::draw_map16` and `draw_tile_ref`, styled by a `LayerStyle`), and
  `LevelLayers::compose` applies screen designation, colour math, and a fixed screen's
  `video::Window`. A boss arena is drawn into `LevelLayers` like any level.
- `kobo_core::operation::Operation` is the handle a long operation runs under: an
  instruction budget shared by both CPUs and every pass, cancellation, and a stage for
  progress. The `_with_control` variants of `expand_level`, `capture_sprites`,
  `render_level`, and `render_loaded` take one; cancellation and an exhausted budget fail
  the operation rather than returning a partial picture.
- `cpu::access::UnsupportedAccesses` is the bus's bounded report of hardware it does not
  model and a picture may be missing something for; it reaches the caller as
  `Diagnostic::Unsupported`. Accesses with nothing to model (open bus, read-only
  registers, controllers) are counted as stubs on the bus, never reported: classify a new
  register in `SmwBus::read_register` or `write_register` instead of letting it be reported.
- `kobo_core::level::LevelMode` is the only place that says what a level mode means to the
  library (`layer2()`: background, horizontal or vertical objects, or none). Do not match on
  mode numbers anywhere else; what the ROM's own per-mode tables decide is read from RAM.
- `kobo_core::rom::Rom` strips the 512-byte copier header and never writes one; `data()` is
  always headerless. Identity is by SHA-1 of the headerless image. Writes go through
  `SnesAddr` like reads; `expand` and `fix_checksum` keep the internal header true.
- `kobo_core::level::objects` is the one codec for object data: `decode` gives absolute tile
  positions and drops screen jumps, `encode` chooses its own. `level::read_objects`,
  `read_background`, and `sprite_ptr` find a level's data from the ROM's tables, vanilla or
  Lunar Magic (`LevelFormat`); do not run the loader to find it.
- `kobo_core::source` is the project's text formats (`kobo.toml`, level files, Map16 page
  files, the shared palettes; graphics are indexed PNGs through `image::IndexedImage` and
  `gfx::tiles_to_image`); `import` reads a ROM into them and `build` writes them onto the
  clean ROM. Kobo owns their formatting (`source::format_project`, `kobo fmt`):
  `Level::to_toml` of `from_toml` must give the same text back.
- `kobo_core::entrance` is the one place that knows Lunar Magic's entrance settings: the
  per-level tables, the midway tables, a secondary entrance's two further bytes, and how
  older versions' convert; import reads and build writes through it.
- `kobo_core::level::size` is the one place that knows Lunar Magic's level sizes: the 32
  heights, the size byte, where its table is, and how a size splits layer 1's screens
  from layer 2's; import reads and build writes through it.
- `kobo_core::map16::pages` is the one place that knows Lunar Magic's tables for Map16
  pages past 1, page 2 per tileset, BG Map16, and the acts-like tables, and
  `map16::GameTables` where the game keeps pages 0 and 1 (common and per tileset), and
  `map16::pipe_address` its other definitions for the pipes;
  import reads and build writes through them. A
  build that writes anything only Lunar Magic's layout holds installs Kobo's code for it
  first (`build::Stage::Install`, `kobo_core::install`).
- `kobo_core::exanimation` is the one place that knows Lunar Magic's ExAnimation (the
  list format, where its tables are, a level's and the overworld's, what builds
  refuse); `source::animation` is its text format, and `asm/lunar-magic/exanimation.asm`
  and `overworld-exanimation.asm` Kobo's code for it, over one engine
  (`exanimation-engine.asm`).
- `kobo_core::edit` is how anything changes a project: `LevelDocument` is an open level
  file (its `Level` and `Comments`), `Edit` a change to it as a value (the editor's and a
  script's alike), applied whole or not at all, and one undo step with the comments
  before an entry moving with it; `Map16Document` the project's foreground Map16 files,
  edited a tile at a time for a tileset and written where the page files' rules put
  it; `GraphicsDocument` a GFX or ExGFX file as its indexed image; `PalettesDocument`
  the shared palettes; `TilemapDocument` a level's layer 3 tilemap file;
  `GlobalAnimation` the global ExAnimation list;
  `Workspace` is the project in memory, built and a level rendered from it
  (`preview`). The editor makes no change any other way.
- `kobo_core::emulator` is how a build is watched while it plays: the Lua script Mesen 2
  runs (`mesen_script`, watched RAM after every frame, a pause on a watched write) and
  its report (`read_report`), which carry RAM alone.
- `kobo_core::stripe` is the one place that knows the game's stripe images (runs of VRAM
  words, layer 3's tilemaps above all): `Tilemap` is what one writes, read and written
  back as an image (the overworld's border).
- `kobo_core::names` holds the names Kobo writes after ids (objects by object set, extended
  objects, sprites, tilesets, music, the game's backgrounds) as data in `names.toml`; `LevelMode::name` names level
  modes. Take names from there; do not write lists of them elsewhere.
- `kobo_core::mwl` reads and writes MWL files: `MwlFile` is the container, byte for byte,
  and `Mwl` the level decoded through the codecs above. `kobo_core::map16_file` reads
  Lunar Magic's full Map16 export (`.map16`), which `import::map16_from_file` turns into
  page files as a ROM import would.
- `kobo_core::callisto` reads a Callisto project's configuration, and
  `import::import_callisto` makes it a project: a copy of its folder, with Kobo's files for
  its levels, Map16, and graphics, and `[callisto]`, through which a build gives the
  patches and the tools' files the `callisto.asm` they include (`build::callisto_header`,
  from Callisto's documentation; never Callisto's own Asar). `kobo_core::template` is the
  baserom templates (`kobo new --template`): a recipe in `src/templates/` of where the
  baserom's own release is, its SHA-256, and its setup steps, fetched into the tool cache
  and imported; Kobo carries nothing of the baserom.
- `kobo_core::pixi` is the one place that knows what PIXI leaves in a ROM: its sites and
  the blocks its tables lead to, read as PIXI's own cleanup reads them. An import without
  the hack's PIXI folder carries that insert as compiled code (`[pixi] compiled`,
  `source::pixi`), which a build writes back where PIXI put it; with it (`--pixi`), the
  project takes the folder's inputs as `[pixi] dir`.
- `kobo_core::playtest` builds a project to start in a level at a tile, or at its main
  entrance (the editor's Play from here, `kobo play`), as its `Settings` say: a secondary
  entrance by tile in a copy of the level, and Kobo's `asm/playtest.asm`, which hooks
  the title screen's and the overworld load's game modes to take a screen exit to it.
- `kobo_core::rats::FreeSpace` is the one way Kobo's library takes free space: everything it
  writes outside fixed addresses goes in a RATS-tagged block placed there. Kobo's own Asar
  patches take theirs with `freecode`/`freedata`, before anything else takes space. Asar
  interoperability limits and required toolchain checks are in `docs/toolchain.md`.
- `kobo_core::install` is Kobo's ROM-side code, Asar patches in `crates/kobo-core/asm/`,
  written clean-room: interface from docs/lunar-magic-install.md, implementation Kobo's
  own. Every patch includes `memory.asm` and names the game's variables through it
  (`$010B|!addr`), so the same source serves LoROM and SA-1 Pack. Fixed entry points jump to Kobo's code in RATS blocks; fixed table addresses hold
  data. Never shape code after Lunar Magic's beyond the bytes a check needs.
- `kobo_core::asar` is the one way Kobo runs Asar: `libasar` loaded at run time (never
  linked; LGPL), one patch at a time behind a process-wide lock. A patch that damages a
  RATS block it found fails (`rats::Snapshot`); every tool stage checks the same way.
- `kobo_core::tools` is the one way Kobo runs the toolchain's programs (AddmusicK, PIXI,
  GPS, UberASM Tool): on a copy of the ROM in a scratch folder, with the tool's folder
  copied in, Asar's library beside it, and the project's files laid over it. `Tool::locate` is the
  one way to find a tool: a configured path (`KOBO_UBERASM`, `tools.uberasm` in the config
  file, and so on) wins; else Asar, PIXI (`tools::pixi()`), and UberASM Tool come from the
  builds `crates/kobo-core/src/tools/pinned.toml` pins (PIXI 1.43, and 1.42 for a project that asks with
  `[pixi] version`: `Tool::locate_version`), downloaded from `kobo-smw/kobo-tools` on first use,
  checked by SHA-256, and cached per user (`KOBO_TOOL_CACHE`; `KOBO_OFFLINE` turns
  downloads off); SA-1 Pack comes the same way from its author's release
  (`tools/upstream.toml`). AddmusicK and GPS are only ever the user's. A stage's cache
  key hashes a pinned build's hash, or every file a configured tool's folder holds; a
  configured tool makes a build not reproducible elsewhere, which `Located::note` says
  and `kobo build` prints.

## Commands

```
cargo build --workspace
cargo test --workspace                       # tests whose tier is not configured skip
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo xtask verify [--strict] [--full]       # fmt, clippy, lint-tools, the tests, and what skipped, by tier
cargo xtask tiers                            # what this machine has for each test tier
cargo xtask baseline                         # write every level's picture hashes (tests/render_baselines.rs)
cargo xtask lint-tools                       # bash -n, shellcheck, and Python's parser on tools/
cargo run -p kobo-editor -- [dir] [--level 105] [--select 10] [--source]  # the level editor
cargo run -p kobo-editor -- dir --level 105 --screenshot out.png  # its window as a PNG, then quit (xvfb-run -a)
cargo run -- rom info [-r rom]               # header, checksum, hash, identity
cargo run -- rom expand 2M out.sfc [-r rom]  # a copy expanded, with the checksum fixed
cargo run -- rom rats [-r rom]               # RATS blocks from $108000 on, and free space
cargo run -- import hack.smc dir [--all] [--pixi folder]  # a ROM's changed (or all) levels as a new project
cargo run -- import level.mwl dir [--level 105] [--sizes-from hack.smc]  # an MWL file's level into a project
cargo run -- import callisto-project/ dir     # a Callisto project as a new project
cargo run -- new dir --template rhr           # a project from a baserom template (--list names them)
cargo run -- build [dir] [-o out.sfc] [--bps out.bps] [--sym]  # a project onto the clean ROM (a patch; labels)
                                             # rom info, import, build, diff: --json for scripts
cargo run -- fmt [dir] [--check]             # rewrite a project's files in Kobo's format
cargo run -- play [dir] --level 105 [--at 70,18] [--powerup 2] [--switches gybr] [--off] [-o play.sfc]  # a build that starts there
cargo run -- diff a.sfc b.sfc [--project dir] # levels that differ, however each ROM stores them
tools/lunar-magic/save-check built.sfc       # Lunar Magic saves a copy; every level must survive
tools/corpus-sweep out/ [--baseline old/results.json]  # import, build, diff, and save every corpus hack
tools/lunar-magic/fixture-lines vanilla-gfx rom.smc  # a Lunar Magic export fixture's lines (hashes) for a ROM
cargo run -- asm patch.asm out.sfc [-r rom] [-I dir] [-D name=value]  # an Asar patch on a copy, checksum fixed
cargo run -- gfx list|export|png [-r rom]    # GFX files: table, LM-layout .bin export, tile sheet
cargo run -- level info 105                  # primary header and data pointers
cargo run -- palette png --level 105 out.png # 16x16 swatch of the palette the level loaded
cargo run -- map16 png --level 105 out.png   # the level's Map16 tiles in colour (--layer 2: BG)
cargo run -- overworld png out.png [--submap 1]  # the overworld as a player there sees it
cargo run -- level png 105 out.png           # render a level by running the ROM's own loader
cargo run -- level png 105 out.png --markers # ID boxes instead of sprite graphics (--no-sprites: none)
cargo run -- level png 105 out.png --no-player # leave Mario out of the entrance (--hide 1,3: layer 2 alone)
cargo run -- level tiles|dump 105 [dir]      # the expanded Map16 grid as hex, or raw planes
cargo run -- level sprites|map16|wram|reads  # sprite list, resolved Map16, RAM dump, read trace
cargo run -- mwl info level.mwl [-r rom]     # an MWL file's sections (-r: its ROM's PIXI sprite sizes)
cargo run -- addr '$05E000' [--sa1]          # SNES <-> file offset
cargo run -- tools [list|fetch [tool]|path tool]  # where each tool comes from; fetch the pinned builds (pixi-1.42: a version)
cargo run -- bps apply hack.bps out.sfc [-r rom]   # patch the clean ROM, headered or not; output headerless
cargo run -- bps create hack.sfc out.bps [-r rom]  # patch from the clean ROM to a modified one
cargo run --release --example sprite_census -- rom.smc  # sprite numbers that draw nothing, by level
cargo run --release --example render_hashes -- rom.smc [level...]  # a hash per level picture, to diff across a change
cargo run --release --example tiles_diff -- a.sfc b.sfc  # levels whose load resolves differently, graphics aside
cargo run --release --example sprite_oracle -- rom.smc dumpdir...  # per-sprite scores against emulator frames
cargo run --release --example swap -- vram lm.smc out.sfc  # Kobo's code for one piece in place of Lunar Magic's
cargo run --release --example exanim_probe -- run rom.smc 105 64  # ExAnimation's VRAM/CGRAM/RAM, frame by frame
cargo run --release --example gfx_probe|layer3_probe|exlevel_probe|entry_probe|sprite_probe|exit_probe|contact_probe -- ...
                                             # compare two ROMs' memory as they play (docs/testing.md)
cargo run --release --example fuzz_inputs -- 10000      # seeded parser mutation cases, no ROM needed
```

Lunar Magic runs headlessly under Wine for reference exports, e.g.
`xvfb-run -a wine "Lunar Magic.exe" -ExportGFX rom.smc` (also `-ExportAllMap16`,
`-ExportSharedPalette`, `-ExportLevel`). Always run it on a copy of the ROM. Export hashes, never the exported bytes,
go in `crates/kobo-core/tests/fixtures/`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy with warnings denied, `cargo xtask
lint-tools --strict`, and tests on Linux, Windows, and macOS, and prints the tests that
skipped. Keep all three green.

## Test tiers and ROM configuration

- **`cargo xtask verify`** runs fmt, clippy, `lint-tools`, a table of the tiers this
  machine has, and the tests, then reports every test that skipped, by tier; run it before
  pushing. `cargo xtask tiers` is the table alone. A plain `cargo test --workspace` works
  too: kobo-core is optimised in dev builds, keeping debug assertions and overflow checks.
- **Unit tests** and the synthetic tests use no ROM and always run.
- **Every other test is in a tier** (`kobo_core::tiers::Tier`): the vanilla ROM, each
  tool, Lunar Magic, the hack corpus, MWL exports, emulator dumps, SingleStepTests, the
  SA-1 reference ROM. Each is set by its environment variable (`KOBO_SMW_ROM`,
  `KOBO_LM_ROMS`, `KOBO_LUNAR_MAGIC`, ...), else by the config file
  (`$XDG_CONFIG_HOME/kobo/config.toml`: `roms.smw`, `tools.<key>`, or the `[tests]`
  table). A test whose tier is missing skips (`common::skip`, which writes `KOBO_SKIP_LOG`
  for verify's report); `KOBO_REQUIRE_<TIER>` (`KOBO_REQUIRE_ROM`, `KOBO_REQUIRE_ASAR`,
  `KOBO_REQUIRE_LUNAR_MAGIC`, ...) or `KOBO_REQUIRE_ALL` makes that a failure, and
  `verify --strict` requires every tier the machine has set. A new test gates on a tier
  through `tests/common` (`vanilla()`, `tool()`, `lunar_magic()`, `lunar_magic_roms()`,
  `tier_path()`), never on an environment variable of its own. docs/testing.md has the
  table of tiers, what each enables, and how each one's data is made.
- Tools come from `Tool::locate_offline()`: the configured path, else the pinned build if
  `kobo tools fetch` has cached it. Tests never download. CI fetches the pinned builds on
  all three platforms, then runs the tests with `KOBO_OFFLINE=1` and requires Asar, PIXI,
  UberASM Tool, and SA-1 Pack.
- The vanilla reference is No-Intro "Super Mario World (USA)", headerless SHA-1
  `6b47bb75d16514b6a476aa0c73a683a2a4c18765`, checksum `$A0DA`.
- **Corpus tests report every failure** against `tests/fixtures/known_failures.toml` (by
  hack SHA-1, test, and level, with why; `tests/common/failures.rs`): a failure not listed
  fails the test, and so does a listed one that now passes, which comes off the list.
- **Pictures are pinned**: `tests/fixtures/render_hashes/` holds every level's picture
  hashes of the vanilla ROM and the SA-1 reference ROM (`tests/render_baselines.rs`). A
  change that should not change a picture passes as it is; one that does runs `cargo
  xtask baseline`, and the diff shows the levels. `KOBO_FULL_RENDER=1` (`verify --full`)
  makes the tests that draw a few levels of a build draw all 512. For a hack, diff
  `examples/render_hashes.rs` before and after, or sweep the corpus against an earlier
  sweep (`tools/corpus-sweep --baseline`).
- Lunar Magic exports (hashes in `tests/fixtures/`, `tools/lunar-magic/fixture-lines`) are
  the oracle for GFX, palette, Map16, and MWL files. Kobo's code is compared with Lunar
  Magic's by swapping one piece into a ROM Lunar Magic saved (`examples/swap.rs`, whose
  sites `tests/common/swap.rs` holds for the tests too) and running a probe on both.

## Reference material

- `~/.local/share/kobo/docs/smwdisx/`: the SMWDisX disassembly banks and `SMW_U.sym` (downloaded
  from GitHub, not committed). Use it to read how the game consumes a table; never build on it.
  SMW Central is behind a JavaScript challenge and cannot be fetched from tools.
- Asar 1.91 built from source (`~/src/asar`, tag `v1.91`): `~/.local/bin/asar`, and
  `libasar.so` in `~/.local/lib`, which `KOBO_ASAR_LIB` points the tests at. Without it,
  `kobo tools fetch` caches the pinned builds of Asar, PIXI, and UberASM Tool
  (`~/.cache/kobo/tools`), which the tests then find.
- .NET 8 SDK in `~/.dotnet` (user-local, from `dot.net/v1/dotnet-install.sh`); UberASM Tool
  built with it as x64 in `~/.local/share/kobo/tools/uberasm-x64` (docs/toolchain.md).
  `kobo-smw/kobo-tools` (its `build.py`, and a Docker image for Linux) makes the pinned builds.
- Lunar Magic 3.70, the version Kobo's builds target: `~/.local/share/kobo/tools/lunar-magic-3.70/`
  (from `fusoya.eludevisibility.org/lm/`). Run `x64/Lunar Magic.exe`, which needs only
  64-bit Wine; set `WINEDLLOVERRIDES="mscoree,mshtml="` so a new Wine prefix does not stop
  to offer Mono and Gecko. `Lunar Magic.chm` is its help file, which documents the
  command-line functions; its pages as text are in
  `~/.local/share/kobo/docs/lunar-magic-3.70-help/txt/` (the `info_*` pages are the
  technical ones). `tools/lunar-magic/` has a wrapper that runs it headlessly and a ROM
  diff that reports changed regions without printing Lunar Magic's code.
- Mesen 2: `~/.local/share/kobo/tools/mesen2/Mesen`, built from source against the system
  libstdc++ (`~/src/Mesen2`, `USE_GCC=true make`, with SDL2 built into `~/.local/sdl2`
  and X11 headers unpacked from Debian's packages into `~/src/x11dev`; .NET SDK in
  `~/.dotnet`). It runs with `DOTNET_ROOT=~/.dotnet` and
  `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1` (no libicu here), under `xvfb-run -a`. The
  official 2.1.1 binary in `tools/mesen/` bundles GCC 12's libstdc++ and aborts with
  `std::bad_cast` at startup about half the time; do not use it. Headless use is
  `Mesen --testRunner script.lua rom.sfc --timeout=N`; the script ends with
  `emu.stop(code)`. Lua enums are lower-camel-cased C++ names: `emu.memType.snesMemory`,
  `emu.eventType.endFrame`. Scripts need `Debug.ScriptWindow.AllowIoOsAccess` and a
  controller on `Snes.Port1` in `~/.config/Mesen2/settings.json`.

## Knowledge base

This file is loaded into every agent context, so it holds rules, layout, and pointers. Facts
live in `docs/`: read the file for the area you are working in before you start, and record
what you find out there in the same change (or in the module's documentation, when it
describes that module's code rather than the game). Do not grow this file with them.

- `docs/smw.md`: the vanilla game. ROM tables (level pointers, GFX, palettes, Map16), how a
  level is entered and loaded and which routines `expand` runs, the tile grid and layer 2
  layouts, level modes, screen designation and colour math, layer 2 and 3 positions, the
  player and sprite capture passes, boss arenas and their window.
- `docs/lunar-magic.md`: what Lunar Magic changes in a ROM. Map16 pages and the `$06F540`
  routine, BG Map16 tables, per-level flags, expanded level heights, custom palettes, sprite
  data formats and PIXI extension bytes, the 255-sprite load flags.
- `docs/sa1.md`: SA-1 Pack. How its two processors hand work over and how the bus schedules
  them, the RAM it moves, MaxTile and the OAM, the work RAM port, SA-1 DMA, images over
  4 MiB, the reference ROM, what it changes in the vanilla levels, what is not modelled.
- `docs/testing.md`: running the tests, the tiers and their configuration, known failures
  and picture baselines, and each check: the emulator oracle and its capture modes, the
  CPU suite, the corpus checks, the corpus sweep, Lunar Magic's save, and comparing Kobo's
  code with Lunar Magic's. `docs/testing-log.md` holds past runs' results.
- `docs/known-gaps.md`: what a rendered level does not reproduce.
- `docs/toolchain.md`: what Asar, PIXI, GPS, UberASM Tool, AddmusicK, and SA-1 Pack require of a
  ROM, where they put things, what makes their output vary, their licences.
- `docs/build.md`: what a build is and the decisions it rests on: Lunar Magic and Kobo
  builds, no base, the source formats, the stages, Lunar Magic's layout piece by piece,
  what builds refuse, what is left to Lunar Magic, and the risks.
- `docs/clean-room.md`: why Kobo looks at Lunar Magic only to interoperate with it, what
  evidence that allows, and how Kobo's outputs keep to it.
- `docs/editor.md`: the editor, as its users meet it, and the decisions it rests on.
  `docs/step-4.md`: the plan for roadmap step 4, its decisions and work order, until
  step 4 is done.
- `docs/review.md`: decisions taken without the maintainer, and settings left refused,
  waiting for a batch review. Add to it rather than stopping to ask.
- `docs/clean-room-audit.md`: the 2026-10-03 audit of everything Kobo knew of Lunar Magic:
  how the clean room was broken before, what was removed for it, and what stays and why.

## Decisions

- **Rust core.** Chosen for single-binary distribution, compile-time address typing, C FFI to
  Asar, and the ability to expose the core to Python, Lua, JS, and WebAssembly later.
- **Headless 65816 core for object rendering.** `kobo_core::cpu` executes the ROM's own
  level-loading routines rather than re-implementing every object; validated against emulator
  dumps of every vanilla level. Small formats (LC_LZ2 and LC_LZ3, GFX, palettes) are hand-written because
  the build must also encode them.

- **Projects build from the clean ROM alone.** No ROM or BPS in a project; baseroms are
  supported as imported or template projects. Levels a project does not list keep the clean
  ROM's content. `kobo build` always overwrites its output; pulling Lunar Magic edits back
  into a project is not a goal.
- **Source formats are TOML** (`kobo.toml`, `format = N`), edited through `toml_edit` so
  comments survive: one object or sprite per line in data order, numeric ids with the name
  as a Kobo-owned trailing comment, raw hex for what the library cannot interpret, one
  central level table (`0x105 = "file.toml"`), `#RRGGBB` palettes (channels x8), indexed PNG
  graphics. Round-trip fidelity is semantic, not byte-exact.
- **Builds run fixed stages**, rarely changed first and levels last, with a snapshot per
  stage keyed by a chained input hash (kept in the user's cache folder, `kobo/stages`, or
  `KOBO_CACHE_DIR`). Output is identical on all three platforms, except
  where a tool orders files by directory listing, which may vary by file system.
- **The editor is egui** (`eframe`, its OpenGL renderer), decided 2026-10-05: one
  binary in Rust alone on all three platforms, and an immediate-mode interface drawn from
  the editor's state with no second copy of it. `docs/editor.md` has the reasoning.
- **Tools come from pinned builds**: `kobo-smw/kobo-tools` builds Asar, PIXI, and UberASM
  Tool from pinned upstream commits for Linux x64, Windows x64, and macOS (arm64 and x64)
  and publishes them with their sources; Kobo pins a release's hashes
  (`crates/kobo-core/src/tools/pinned.toml`, written by its `build.py pins`) and fetches the build for its
  platform on first use. A `[tools]` path or environment variable overrides one, and the
  build report notes it. AddmusicK, SA-1 Pack, and GPS have no licence and are never
  bundled; SA-1 Pack is fetched by hash from its author's GitHub release
  (`tools/upstream.toml`), the other two are the user's own. GPS runs unmodified, so
  Kobo's bank `$06` code has the shape it patches.

## Open decisions

None at present.

## Prior art to know

- Lunar Helper / Callisto (build orchestration), Lunar Monitor (auto-export for git).
- pokeemerald + Porymap (the source-first editor model for another game).
- SMWCentral documentation of Lunar Magic's ROM formats and hijacks.
