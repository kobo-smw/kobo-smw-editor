# Step 2: the build pipeline

The plan for roadmap step 2 and the decisions it rests on, settled 2026-09-25. The rules
that follow from them are in [AGENTS.md](../AGENTS.md); this file has the reasoning and the
work order. Fold what stays true into the other docs as step 2 lands, and delete this file
when it is done.

## Goal

A project directory builds into a ROM from a clean SMW ROM. Levels, Map16, palettes, and
ExGFX are written natively in Lunar Magic's layout; existing work is imported from MWL files
and from ROMs; Asar, PIXI, GPS, UberASM Tool, and AddmusicK run in a fixed order. What Kobo
does not cover yet (overworld, title screen, credits, messages) is finished in Lunar Magic
on the built ROM.

## Decisions

### Clean room: interoperability, and only that

- Purpose: Kobo looks at what Lunar Magic does for one reason, to interoperate with it, so
  that Lunar Magic and the tools around it can open, save, and extend a Kobo build. Kobo
  observes Lunar Magic only as far as that needs, and matches the least that a check
  requires. Anything learned that is not needed for interoperability is not used.
- Interface, which Kobo matches exactly: hook addresses, where each table and block lives
  and its format, the RAM values a hook leaves for other code (`$13D7`, `$7FC00B`), and the
  bytes Lunar Magic or another tool checks for.
- A pointer Lunar Magic keeps at a fixed offset in the code it installs is interface too,
  once documentation or a byte diff shows where it is: `read3($05D9E4)+$0A` (the midway
  tables), `$05DC86` and `$05DC8B` (the secondary entrance tables), `$0FF873` and
  `$0FF937` (ExGFX), `$0DE191`, `$0DE198`, `$0DE19F`, and `$05DC81` (the secondary entrance
  tables where Lunar Magic moved them). Kobo reads and writes such a pointer, and shapes its
  own code so the pointer sits at the same offset. Decided 2026-09-27.
- Bytes Lunar Magic checks before it treats a piece as installed are interface when Kobo
  needs that piece kept: a marker, or the opcode of an instruction around a pointer. Kobo
  writes exactly the bytes the check needs, no more, and records each case, with how it was
  found and what it makes Lunar Magic do, in
  [lunar-magic-install.md](lunar-magic-install.md#bytes-kobo-writes-because-lunar-magic-checks-them).
  Such bytes can tell Lunar Magic more than Kobo means, so every one is tested with and
  without it ([testing.md](testing.md)). Decided 2026-09-27.
- Implementation, which Kobo writes itself: the code behind each hook, and every routine.
  Where a checked byte is an instruction's, the instruction is Kobo's own and does its work
  in Kobo's code.
- Allowed evidence: SMWCentral and SNESLab documentation, Lunar Magic's readme and help
  file, the sources of open tools (PIXI, GPS, UberASM Tool, SA-1 Pack), byte diffs of a ROM
  before and after a Lunar Magic operation, bisecting which bytes make Lunar Magic keep or
  export something (printing addresses only), and running Lunar Magic-saved ROMs to
  observe which addresses they read and write, with what values.
- Not allowed: reading Lunar Magic's instructions, as a disassembly of the ROM or the
  executable or as an instruction trace (`KOBO_CPU_TRACE` over its code), and copying its
  code or any bytes a check does not need. Nor is anything else that shows where its
  instructions are or what they do: the address in a CPU error, the instruction behind a
  watched write or a data read, the registers a routine of its code returns, the stack.
  Bytes of its code seen by accident and not used (a tool printing a changed range whole)
  need no record; fix the tool that printed them.
- Step 1 (2026-09-14 to 09-23) ran before the rule covered the code Lunar Magic puts in a
  ROM, and traced it; so did later work through the outputs above, until 2026-10-02.
  Everything Kobo knew of Lunar Magic was audited on 2026-10-03: what came that way was
  removed, with the code that relied on it, and every fact kept cites an allowed source
  ([clean-room-audit.md](clean-room-audit.md)).
- Kobo's own outputs keep to it (decided 2026-10-02). `kobo_core::clean_room` takes a ROM
  as Lunar Magic's by its marker, or by code in two areas every save fills whatever is
  there (`$03BB00`, `$03BCA0`), so a hack whose marker was removed is still one. Once a process has loaded such a ROM, nothing it prints or writes says where an
  instruction of a ROM's code is: no trace, no address in a CPU error or for an
  instruction fetched from unmapped memory, no instruction behind a watched write or a
  data read, no registers from a called routine, which may only be entered at the game's
  code or a hook, and no stack bytes in a RAM dump; `kobo asm` withholds what a patch
  prints. The scripts check the same way (`tools/clean_room.py`): `romdiff.py` and
  `sites.py` decode a hook's jump only against a ROM Lunar Magic did not save,
  `trace_writes.lua` refuses one, and the emulator oracle stops only at the game's
  instructions or a hook. A debugging aid added later goes through the module, or says
  why it shows nothing of a ROM's code.
- Why: matching what another program checks, in order to interoperate with it, is the
  position the law has long recognised (*Sega v. Accolade*, 1992, where copying Sega's
  small lockout code so games would run was fair use; the EU Software Directive allows even
  decompiling for interoperability). Kobo stays well inside it by never reading Lunar
  Magic's code at all. The concern beyond that is provenance and trust. Hook code is small
  and constrained, so code written after reading Lunar Magic's comes out nearly the same,
  and an agent with a disassembly in context reproduces it. Every patch has to be
  licensable under MPL-2.0, and the project depends on the community's trust (compare
  Wine's contribution policy, or ReactOS halting for an audit in 2006). Stating the purpose,
  and keeping to the least it needs, is what makes that position clear.
- Where Lunar Magic recognises a piece only by its code, beyond bytes a check needs, Kobo
  does not match it; Lunar Magic then installs its own over Kobo's in the copy it saves,
  which the next decision allows.

### Lunar Magic and Kobo builds

- Required: Lunar Magic opens a Kobo-built ROM, shows every piece of Kobo-managed content
  correctly, and saves without losing any of it.
- Not a goal: pulling Lunar Magic edits back into a project. Import from a ROM exists for
  migration, and if it is stable a round trip falls out for supported features, but effort
  goes into supporting a feature, not into working around its absence.
- `kobo build` always overwrites its output. The ROM is a build artefact.
- A build does not carry the `Lunar Magic Version` string at `$0FF0A0`. The hook spike
  showed Lunar Magic neither reads it to decide what is installed nor needs it, and writes
  it on its own first save.
- A build always has a correct internal checksum; Lunar Magic warns that a ROM "may be
  Corrupt" otherwise.
- Kobo writes the layout of the current Lunar Magic release and pins that release for the
  Lunar Magic checks.

### No base

- A build always starts from the clean ROM. No ROM or BPS belongs in a project.
- Baseroms are supported as projects: import from a ROM, and template projects for widely
  used baseroms. A template references third-party patches by URL and hash where their
  licence does not allow redistribution.
- Import reports what it did not carry over: regions that differ from vanilla which Kobo
  does not model, and patched hook sites.
- A level the project does not list keeps the clean ROM's content, so a project holds only
  the levels it defines, and no Nintendo level data unless its author changed it. An empty
  level file blanks a level.

### Source formats

- `kobo.toml` is the manifest, with `format = N`. Kobo refuses a newer format and migrates
  an older one.
- TOML throughout, edited through `toml_edit` so that comments survive. Kobo owns the
  formatting; `kobo fmt` is idempotent.
- One object or sprite per line, as an inline table. File order is data order, which is
  draw order; it is never sorted.
- Positions are absolute tile coordinates in decimal. Fields are decoded where the library
  knows their meaning (an object's size nibbles as width and height, or length) and raw hex
  where it does not, including data Kobo can build but does not interpret; a comment can
  say what it is believed to be.
- Ids are the numbers the game or tool uses. Kobo writes the name as a trailing comment on
  the entry's line and refreshes it; a user's comments go on their own lines.
- A level table maps numbers to files, one `0x105 = "world1/yoshis-island-1.toml"` per
  line. It is the only place level numbers live; file names and folders are free. Numbers
  are explicit because they leak into the overworld's translevels, hard-coded levels, ASM,
  UberASM lists, and save files. `auto` entries for sublevels can come once exits and the
  overworld refer to levels by file.
- Palettes are `#RRGGBB` with each channel the SNES 5-bit value times 8, one 16-colour row
  per line, rows labelled by generated comments.
- Graphics are indexed PNG: the pixel index is the colour index, the PNG's palette is only
  a preview. `.bin` is import and export.
- Map16 pages past 1 are one file per page, listed in the manifest's `[map16]` table like
  levels, one tile per line keyed by its number: what it acts like, and its four 8x8
  tiles in reading order as `"TTT P xyp"` (`source::map16`). A tile a file does not list
  is empty and acts like `$130`, as a fresh Lunar Magic install has it; import leaves
  empty tiles out. BG Map16 pages are the same without `acts`, in `[map16_bg]`, page
  `P` being page `P % 16` of table `P / 16`. Pages 0 and 1, the game's own tables, list
  only the tiles a project changes, with `acts`, `gfx`, or both; what a file leaves out
  is the clean ROM's. So do BG Map16 table 0's pages 0 and 1, the game's background
  tiles. One rule throughout (decided with the maintainer, 2026-10-03): a tile a file
  leaves out is what the build starts from, which keeps Nintendo's tiles out of
  projects. The tiles the game keeps per object tileset are in tileset files
  (`[map16_tileset]`, gfx only), as is page 2 when it is per tileset; tilesets that
  share the game's table share one file's tiles, and the build refuses two; page 2 is
  per tileset exactly when a tileset file lists a tile of page 2, with no switch. Both
  stay implicit (maintainer, 2026-10-03), and the manifest notes after a tileset file
  which other tilesets its pages 0 and 1 also are (`map16::TILESET_SHARING`). The
  vertical pipes' colour sets 0, 2, and 3 and the diagonal pipes, which are neither a
  page nor a tileset's table, are a file of their own (`[map16_pipes] file`,
  `source::map16::Pipes`), listing only what a project changes (maintainer, 2026-10-03).
- A background of the level's own is `[layer2]` `table`, `rows` (32 for Lunar Magic's own
  format, 27 for the game's behind a full pointer), and `tiles`, one line per row of the
  left half's 16 tiles then the right half's.
- Round-trip fidelity is semantic: decoding, encoding, and decoding again gives the same
  objects in the same order, and the level renders the same. Kobo's encoder chooses its own
  new-screen bits and screen jumps.

### The build

- Stages run in a fixed order, from rarely changed and slow to often changed and fast, so
  that editing a level never reruns AddmusicK or moves a tool's code. The tool survey
  ([toolchain.md](toolchain.md)) fixed the constraints; open to revision if more turn up:
  1. Check the clean ROM's hash. For SA-1, apply SA-1 Pack (with `$0FFFEB` set first for
     LC_LZ3), then its 6 or 8 MiB patch. Expand to the manifest's size, filled with `$00`.
  2. Kobo's ROM-side patches, none of it in AddmusicK's ranges (`$0E8000`-`$0EF0FF`,
     `$0F8000`-`$0FF050`), when the project uses Lunar Magic's layout.
  3. User Asar patches, early group.
  4. AddmusicK: it needs `$0E8000` untouched and everything before it RATS-tagged.
  5. Graphics and ExGFX, palettes, Map16 and the acts-like table: GPS rewrites that table,
     and PIXI and GPS refuse a ROM without its pointer at `$06F624`.
  6. PIXI (with `-meimei-off`), GPS, UberASM Tool, in that order: UberASM Tool reads the
     flag PIXI sets at `$0FFFE0`.
  7. User Asar patches, late group (patches that hook the tools' code).
  8. Levels, after PIXI, whose size table sets the length of each sprite entry.
- Every stage runs on the previous stage's snapshot. PIXI, GPS, and SA-1 Pack do not
  repeat their output when run over it.
- Every table a tool repoints and every hook site a tool takes over leads to a RATS block
  of its own, since Asar's `autoclean` erases the whole block behind the old target.
- A ROM snapshot is kept after each stage, keyed by the hash of the previous key, the
  stage's inputs, and its tool version, in the user's cache directory. A build reruns from
  the first stage whose key changed. That a cached build equals a clean one is tested.
- The output is a function of the clean ROM's hash, the project files, the Kobo version, and
  the tool versions alone, and is the same on Windows, Linux, and macOS; CI checks it.
  The exception is what a tool orders by directory listing (PIXI's shared routines, GPS's
  routines, UberASM Tool's library files): a build that uses those is repeatable on one
  file system but may differ on another. Decided 2026-09-25 that this is acceptable for
  now; identical output everywhere is a nice-to-have there, not a requirement.
- Kobo allocates free space first-fit in a fixed order and tags every block with RATS for
  interoperability. Kobo's own Asar patches are the exception: they take space with
  `freecode` and `freedata`, which puts it in RATS-tagged blocks too. They run straight
  after the base image, before anything else takes space, so Asar places them the same way
  for a given base, and `kobo_core::asar` checks each against the blocks already there, as
  it does any patch. Decided 2026-09-27. A tag does not stop a later tool from writing into a block
  ([toolchain.md](toolchain.md#asar-191-rats-boundary-limitation)), so every tool stage
  checks the blocks that were there before it with a `rats::Snapshot` and fails the
  build on damage, as Asar patches already do.

### Tools

| Tool | Licence | Source | Notes |
|---|---|---|---|
| Asar 1.91 | LGPL-3.0+ | C++ | Dynamic linking is fine; PIXI and UberASM Tool use 1.91 too |
| PIXI 1.43 | GPL-3.0 | C++, CMake | Builds on Linux; its CFG editor's resources are Nintendo data |
| UberASM Tool 2.1 (Fernap) | GPL-3.0 | C# | Built for x86 .NET 8, which Linux and macOS lack; needs an x64 rebuild |
| AddmusicK 1.0.11 (AddMusicKFF) | none | C++, Makefile | Builds on Linux; holds SMW samples and music |
| SA-1 Pack 1.40 | none | Asar patch | Holds code attributed to Lunar Magic |
| GPS 1.4.4 | none | C++ | No repository; release on the Wayback Machine; builds on Linux |

- A companion repository builds each licensed tool from a pinned upstream commit on CI for
  all three platforms and publishes the builds with their sources, leaving out PIXI's CFG
  editor and its Nintendo resources, and with UberASM Tool rebuilt for x64 with a native
  `libasar`. Kobo downloads the one for its platform on first use, checks its SHA-256, and
  caches it per user. Done (2026-09-27): `kobo-smw/kobo-tools`, release `r1`, pinned in
  `tools/pinned.toml`; [toolchain.md](toolchain.md#kobos-pinned-builds) has how.
- A `[tools]` path overrides a tool, for people developing it; the build is then marked as
  not reproducible: `kobo build` prints a note for each tool a build takes from a
  configured path (`Located::note`), AddmusicK, GPS, and SA-1 Pack included.
- AddmusicK, SA-1 Pack, and GPS have no licence, and AddmusicK contains Nintendo data:
  never bundled. Decided with the maintainer (2026-10-03): Kobo fetches SA-1 Pack 1.40
  by hash from its author's GitHub release (`VitorVilela7/SMW-SA1-Pack` `v1.40`,
  `SA1-Pack-140.zip`, Asar patches that serve every platform), which is not
  redistribution; AddmusicK (whose release has Windows programs only, with an Asar of its
  own) and GPS (on the Wayback Machine only) stay the user's own configured copy.
  Asking their maintainers to add a licence would help, but nothing waits on it.
- GPS runs unmodified, as the user supplies it, and Kobo's bank `$06` code has the shape
  GPS patches (decided 2026-09-25). A licence would let the companion repository patch GPS
  to use the documented `JSL` slots instead.
- Each Kobo release pins one set of tool versions. Per-project pins can come later.

## Prework

1. Development environment: Wine with the current Lunar Magic release (for its
   command-line exports and the Lunar Magic checks) and Asar.
2. The hook spike. Done: [lunar-magic.md](lunar-magic.md) has what Lunar Magic installs
   and how it decides. Lunar Magic keeps Kobo's code behind a one-time hook site that
   jumps to it, but decides piece by piece whether its install is there, mostly by a
   `JSL` at one hook site per piece, and a piece it installs resets that piece's tables
   ([lunar-magic-install.md](lunar-magic-install.md#how-a-save-decides-what-to-install)).
   The spike first read this as one gate at `$06F600`, which covers bank `$06` only.
3. Write-side research. The tools are done ([toolchain.md](toolchain.md)). For Lunar
   Magic, [lunar-magic.md](lunar-magic.md) has the footprint and the hook sites, and
   [lunar-magic-install.md](lunar-magic-install.md) the one-time set piece by piece: the
   vanilla code each replaces, the feature (Map16 pages and the acts-like chain, taller
   levels, backgrounds, exits and midway points, the sprite loader, per-level tables,
   3.70's game loop hook), the fixed operands and slots other tools use, and the RAM a
   level load leaves. A save keeps foreign code in the set's areas and never reinstalls
   it, but rewrites four areas next to it (`$03BB00`, `$03BCA0`, `$05DD30`, `$0EF510`)
   and six table pointers inside Kobo's code. Still to find: whether Lunar Magic's
   restorable code reads the RAM the set leaves, the overworld's behaviour, what block
   tools other than GPS check in the help file's slots (which 3.70 does not run), and
   older versions' pieces. Tile changes in play, block contact, scrolling, and the
   special exits now play as Lunar Magic's (lunar-magic-install.md, "Unknowns";
   2026-10-02, when exits without `u` took Lunar Magic's translevel rule).
4. ROM writing. Done: writes through `SnesAddr` and `Mapping`, expansion, header and
   checksum (`Rom`), and `rats::FreeSpace`, tested on synthetic LoROM and SA-1 images.
   Its bank preferences and tag placement follow Asar's, with a deliberate difference:
   Kobo preserves tagged zeros in a bank-boundary case where Asar 1.91 overwrites them.
   The regression test covers allocation both in one stage and after a rescan;
   [toolchain.md](toolchain.md#asar-191-rats-boundary-limitation) has the reproduction.
5. A level reader and writer for layer 1 and 2 objects, background tilemaps, headers, and
   sprite lists, checked by round trip on every level of vanilla and the corpus and by
   extending `fuzz_inputs`. Done for the binary formats: `level::objects`,
   `sprites::encode`, `compress::rle1`, `level::SecondaryHeader`, and `level::read_objects`
   and `read_background` find a level's data from the ROM's tables alone, vanilla or Lunar
   Magic ([lunar-magic.md](lunar-magic.md)). Locked ROMs are out of scope. Interpreting
   Lunar Magic's Map16 objects and custom backgrounds as tiles comes with their 2b features.
6. BPS reading and writing. Done: `kobo_core::bps` applies a patch with every CRC and
   bound checked (`apply_to_rom` takes one made against the headerless or the
   copier-headered image and returns the headerless target) and creates one
   deterministically, smaller than the distributed patch for every corpus hack it was
   tried on; `kobo bps apply|create` on the CLI. `KOBO_LM_ROMS` takes `.bps` entries, so
   the QLDC entries are listed as they are distributed ([testing.md](testing.md)).
7. An LC_LZ2 compressor that always produces the same output. Done:
   `compress::lz2::compress`, an optimal parse over commands 0-4 (dynamic programming,
   longest matches from a suffix array) with a fixed tie-break. It writes only what the
   game's routine and Kobo's decoder read alike ([smw.md](smw.md)); the vanilla GFX files
   come out 121,663 bytes against 130,317, and the game reads them back.
8. Asar integration. Done: `kobo_core::asar` loads `libasar` at run time (LGPL-3.0),
   checks its API version, and applies a patch to a `Rom` in memory, from disk or from
   in-memory files, with include paths and defines and the checksum left to Kobo; its
   errors, warnings, prints, labels, and writes come back as values, one patch at a time
   behind a process-wide lock. Every patch is guarded by a `rats::Snapshot`: a block
   that was there before and is changed without being released fails the patch, which
   catches the boundary corruption ([toolchain.md](toolchain.md#asar-191-rats-boundary-limitation)).
   At the library boundary Kobo checks what Asar does not: an image longer than its
   buffer is refused before the call, and the result is read as the headerless image
   it is, padded to a whole bank. CI tests against Asar 1.91 built from source (now the
   pinned build from `kobo-smw/kobo-tools`) on Linux, Windows, and
   macOS and runs the tests against it. `kobo asm` applies one patch.
9. Name tables for objects, sprites, tilesets, and level modes, as data in the library.
   Done: `kobo_core::names` (from `names.toml`: standard objects by object set, Lunar
   Magic's objects, extended objects, sprites, tilesets, music) and `LevelMode::name`,
   checked against the ROM's dispatch and per-mode tables (`tests/names.rs`).
10. Build checks: import, rebuild, and compare `render_hashes`; for a hack whose ASM and
    custom sprites the rebuild lacks, compare `LevelTiles` and layers 1 and 2 without
    sprites instead. A synthetic base image lets CI build without ROM data.

## Work order

- 2a progress: the level format, manifest, import from ROM, and build are in
  (`kobo_core::source`, `import`, `build`), and the milestone holds: every vanilla level
  imported with `kobo import --all`, built with its layer data in RATS blocks at `$108000`
  and up, re-imports as no changes and renders the same picture on all 512 levels
  (`render_hashes`, 2026-09-25). Sprite lists stay in bank `$07`, where the game reads
  them: unchanged ones keep their place and changed ones go in the bank's unused space
  (4.5 KiB), until 2b's Lunar Magic layout lifts that. The build runs `build::Stage`s
  with snapshots keyed by a chained hash (`build::Cache`, in the user's cache directory);
  a cached build equals an uncached one, and a synthetic base image lets CI check the
  output is the same on every platform. An import reports the ROM's changes it did not
  carry over: ranges of the clean ROM's space that differ outside every level's data and
  the tables it reads, and tagged blocks past the clean ROM that no level uses (Kaizo
  Mario: 148 ranges, 137 KB, Lunar Magic's install among them). `kobo import` takes an
  MWL file too, adding the level to a project: all 512 vanilla exports imported and built
  differ from vanilla only where Lunar Magic changed them on export. Both importers put
  screen exits in the game's format where it can say them, and note an exit to the other
  bank; an MWL import notes each setting a level file cannot hold yet (midway entrance,
  Lunar Magic 3 and ExGFX bytes, ExAnimation) where it is not vanilla's. Level files carry
  Kobo's names (`kobo_core::names`) as trailing comments. `kobo build --bps` also writes
  the build as a BPS patch against the clean ROM. Still to do in 2a: builds applying
  Kobo's own patches (`kobo_core::install`), which wait for the first 2b feature.
- SA-1 builds: `[rom] sa1 = true` has the base stage apply SA-1 Pack (`tools.sa1pack` or
  `KOBO_SA1PACK`, run through Asar, never bundled) and its 6 or 8 MiB patch for a larger
  image; above 4 MiB the manifest takes only `6M` and `8M`, the sizes those patches make.
  An empty SA-1 project builds byte for byte what `asar sa1.asm` makes of vanilla, and at
  6 and 8 MiB what `asar 6mb.asm`/`8mb.asm` make after it but for the checksum, which Asar
  leaves at the 1 MiB image's and Kobo computes (checked 2026-09-27, SA-1 Pack 1.40); every
  level of that ROM imported and built back as an SA-1 project renders the same
  picture on all 512 levels. Import of an SA-1 ROM compares it with the SA-1 base
  (`build::base_image`), inside `import::import_rom`. `[rom] lz3 = true` stores the GFX
  as LC_LZ3: the build sets `$0FFFEB` to `$02` before SA-1 Pack, which then puts in its
  LC_LZ3 decompressor, and `$0FFFFF`, without which Lunar Magic reads `$0FFFEB` as LC_LZ2,
  and writes every GFX file again with `compress::lz3` (116,124 bytes against LC_LZ2's
  121,663). The game's routine reads every file back, all 512 levels draw as in the
  LC_LZ2 build, Lunar Magic exports every file the same from both, and its save keeps
  the setting; an SA-1 ROM with LC_LZ3 GFX imports with it set. LoROM builds refuse it:
  they would need an LC_LZ3 decompressor of Kobo's own, and a LoROM hack with LC_LZ3
  GFX imports and builds with LC_LZ2, which reads the same. The encoder leaves out
  LC_LZ3's reversed and backwards copies. Kept so (maintainer, 2026-10-03): what LoROM
  loses is about 5% of the GFX's space; revisit if a project runs short of it, or with
  graphics editing (roadmap step 4).
- Secondary entrances are in the level they lead to (`[entrances]`), read from a ROM or
  an MWL file and written in the game's format, where an entrance's number must share its
  level's bit 8; a level's list replaces what the base ROM had leading to it, and may not
  take one the base ROM gives a level the project does not define. An import drops an
  entrance numbered past `1FF` and keeps one numbered in the other bank for the build to
  refuse, noting both.
- 2c progress, ahead of 2b where nothing waits on Lunar Magic's layout: the build runs
  the project's Asar patches (`[patches] early` and `late`) and AddmusicK (`[music] dir`,
  laid over the user's AddmusicK folder, `tools.addmusick` or `KOBO_ADDMUSICK`), each
  checked with `rats::Snapshot`, each stage keyed by every file it can read. For a patch
  that is its own folder and the project's, where Asar also looks for included files, less
  hidden files, ROM and patch images, and the files the manifest gives other stages. A vanilla
  build with AddmusicK's default music is deterministic, and Lunar Magic saves it keeping
  AddmusicK's code and data. UberASM Tool runs too (`[uberasm] dir`, `tools.uberasm`),
  as an x64 build on Linux ([toolchain.md](toolchain.md)); Lunar Magic keeps its hooks.
- 2a: the pipeline with vanilla formats. Its builds leave `$06F600` at `$FF` and write
  nothing in Lunar Magic's layout, so Lunar Magic's first save installs itself and keeps
  Kobo's data, as the spike showed for a relocated level. Manifest and level table, the
  level reader and writer, ROM writing and the allocator, the staged build and its cache, BPS output, Asar
  for Kobo's own patches, and import from MWL files and ROMs. Milestone: every vanilla
  level imported as text, rebuilt into expanded space, and rendering as vanilla does.
  The MWL reader and writer are done (`kobo_core::mwl`, `kobo mwl info`), checked on
  21,504 files Lunar Magic 3.70 exported from vanilla and 41 hacks
  ([lunar-magic.md](lunar-magic.md#mwl-files)); importing one into a project waits for
  the source format. An MWL is Lunar Magic's view of a level, not the ROM's: it rewrites
  screen exits, objects `3C`-`3F` in tileset 4, and pre-3.00 header bits on export, so
  import from an MWL and from the ROM can differ in those.
- 2b progress: `kobo_core::install` holds Kobo's clean-room patches (`asm/lunar-magic/`),
  applied through Asar and not yet used by builds. `map16.asm`: the Map16 routine behind
  `$06F540`, `$06F5D0`, and `$06F5E4` (each a `JML` to Kobo's code; the page tables are
  data at their fixed addresses), the seven hooks that call it, and tile generation that
  sets a page outright. Vanilla with it installed renders all 512 levels as vanilla does,
  and tiles on pages 2 to `7F`, and page 2 per tileset, resolve from tables where Lunar
  Magic's layout puts their pointers (`tests/install.rs`). The overworld's layer 1, whose
  build goes through the row uploads, draws as vanilla's (`expand::install_tests`); how
  Lunar Magic stores overworld pages past 0 is not known. The patches take their
  addresses through `memory.asm`, LoROM's or SA-1 Pack's (lunar-magic-install.md, "On an
  SA-1 ROM").
  `actslike.asm`: the gate, the acts-like chain, and the custom block actions, with
  GPS's entry blocks and slots where GPS expects them. Its behaviour was learned from a
  Lunar Magic-saved ROM by playing it against a logging GPS block
  (`examples/contact_probe.rs`), and matches it in every scenario tried; vanilla with both
  pieces renders all 512 levels as vanilla does. `tests/install.rs` plays sprites into a
  custom tile in a horizontal and a vertical level with a logging routine in the slots,
  and follows a 17-link chain to its end and a looping one to cement.
- 2b does not start with the whole one-time set. A save decides piece by piece whether
  Lunar Magic's install is there and installs each missing piece over whatever is at its
  sites, resetting its tables. So a feature brings the pieces whose tables it writes, as
  Kobo's clean-room code with the check for each met (a `JSL` at the piece's hook site,
  `$06F600` for bank `$06`), and leaves the rest for Lunar Magic's first save to install:
  per-level tables and midway points need `$05DA17`, sprite data banks `$05D8F5`, BG
  Map16 `$058DA4`, taller levels `$05DA8A`. Where a check is on Lunar Magic's own code
  (`$05803B`'s, on `$0EF510`) or a save always retargets the hook, Kobo's code runs
  until the first save and Lunar Magic's after it, reading the same data. In bank `$06`,
  Kobo's Map16 routine and acts-like chain give the same pictures and RAM as Lunar
  Magic's on every level of Kaizo Kindergarten's content, and its Yoshi's berry hooks
  give 3.70's berry check (the chain, vertical levels, layer 2), which a save keeps, as
  with the gate set it never installs its own. The acts-like table pointer at
  `$06F624` is part of it, and GPS also patches the code around it (the entry slots from
  `$06F690`, the compare chain at `$06F67B` and `$06F717`, the exit at `$06F602`), so
  Kobo's code there has the shape GPS expects.
- 2b progress, Map16 pages 2 to `$7F`: page files, import from a ROM (every page of each
  group Lunar Magic allocated), and a
  build that installs Kobo's bank `$06` code (`Stage::Install`) and writes the pages in
  whole groups of 16 (Lunar Magic allocates a group only up to its last used page, which
  its save keeps either way), and the acts-like tables (`Stage::Map16`, after
  AddmusicK). Kaizo Kindergarten's 94 pages import, build, and import again as the same
  text, and Lunar Magic saves the build keeping all of them (2026-09-26). The levels that
  place those tiles use Lunar Magic's objects, which builds still refuse: they come next.
- 2b progress, Map16 pages 0 and 1 and page 2 per tileset: page files for pages 0 and 1
  and tileset files, imported from a ROM (what differs from the clean ROM) and built in
  place in the game's tables, with what those tiles act like in Lunar Magic's table (which
  needs Kobo's install; graphics alone do not), and page 2 per tileset as one table of 15
  pages with `$06F547` = `$06`. Every LoROM hack of the corpus and the QLDC entries (173)
  imports and builds to the same pages 0 and 1 in every tileset and the same acts-like
  settings, and imports again the same; none uses page 2 per tileset, so that is checked
  on a synthetic project, through the game's tile uploads in levels of four tilesets. Lunar
  Magic saves a build keeping all of it, and its full Map16 export of a build shows every
  tile and setting as written, which needed a marker at `$06F5FC` that builds lacked: before
  it, Lunar Magic's editor showed every acts-like setting and every page past `$0F` of a
  Kobo build wrong (2026-09-27). MWL files hold no Map16.
- 2b progress, Lunar Magic's objects: builds take `22`, `23`, `27`, `29` (tiles), `26`
  (music), and `2D` (user), with Kobo's code for them (`objects.asm`), whose behaviour was
  learned case by case from the grids Lunar Magic's code leaves. Kaizo Kindergarten
  imported (337 levels, 89 pages; one level with a time limit bypass and some secondary
  entrances in Lunar Magic's format left out) builds to the same tile grid on every level
  as the hack, and Lunar Magic saves the build keeping every level. The time limit bypass
  (`28`) builds since 2026-09-27: every form sets the timer as Lunar Magic's code does,
  from the overworld and through an exit, and Kaizo Kindergarten's level `14A` builds with
  the hack's grid and timer. The graphics bypasses (`24`, `25`) build with Kobo's
  graphics loader (below), and long screen exits (extended `02`) since 2026-10-02, with
  secondary entrances past `1FF` in tables a build moves to hold `$2000`
  (`entrances.asm`; lunar-magic-install.md, "Entrances, exits, and midway points"):
  riff2 imports and builds every level but the 13 its PIXI sprite sizes block, every one
  the same as the hack's and through Lunar Magic's save, and enters every entrance past
  `1FF` as the hack does.
- 2b progress, backgrounds and BG Map16: level files hold backgrounds, import reads Lunar
  Magic's formats and BG Map16 tables, and builds write them with Kobo's level number,
  background, and BG Map16 code, meeting Lunar Magic's checks so that its save keeps the
  flags and tables. Kaizo Kindergarten built by Kobo resolves every level as the hack does,
  before and after Lunar Magic saves the build; the pictures still need its graphics and
  palettes. An import keeps the ROM's size when it is over the default. An MWL import
  takes a background in Lunar Magic's layout too, as a ROM import would on all 1439 of the
  corpus's but the six in its older format (`C` without `F`: Kaizo Mario 1, Kaizo Mario
  World 3), which the export leaves in table 0 (`tests/mwl_files.rs`). BG Map16 tables
  are written a whole bank each, where Lunar Magic stops after the last tile used.
- 2b progress, custom palettes: a level file's `[palette]` (`back_area` and 16 rows of 16
  colours), from ROMs and MWL files, built with Kobo's palette hook. Kaizo Kindergarten's
  334 palettes build to the hack's CGRAM, and survive Lunar Magic's save.
- GFX files `00`-`33` in the game's own formats: the manifest's `[gfx]` table of indexed
  PNGs (16 tiles to a row, the pixel the colour index), imported where a ROM's differ
  from the base, built as LC_LZ2 with the game's pointer tables repointed (`GFX32` and
  `GFX33` share a bank and move together). Most Lunar Magic 3 hacks store their files as
  4bpp, which only Lunar Magic's graphics loader reads; those, ExGFX, and per-level
  graphics lists come with Kobo's loader (below; Kaizo Kindergarten: 2 of its files are
  in the game's formats, 47 are not). Lunar Magic 3.70 importing a level
  (`-ImportLevel`) into a build with a replaced `GFX01` left every GFX pointer and size as
  the build wrote them (2026-09-27 review). A resized file is left out of an import, with a
  note, as the build keeps each file's size.
- 2b progress, exits and secondary entrances in Lunar Magic's format: Kobo's exit hooks,
  installed when a level has an exit in that format or an entrance numbered in the other
  bank; entrances then keep their destination's bit 8. Kaizo Kindergarten imports and
  builds, entrances included, but for the levels with objects builds refuse (its graphics
  and time limit bypasses), and `kobo diff` finds every level the same as the hack's.
  The entrance settings below build exits with `w` (water, or the midway entrance).
- 2b progress, entrance settings: level files hold Lunar Magic's per-level settings
  (position method 2, layers relative to the player, the background's placement, slippery,
  water, sprite spawning) in `[entrance]`, a separate midway entrance in `[midway]`, and a
  secondary entrance's own; import reads them from ROMs of any Lunar Magic version,
  converted as Lunar Magic 3.70 converts them, and from MWL files, and builds write them
  with Kobo's entrance code (`kobo_core::entrance`, `entrance.asm`). Every corpus ROM's
  settings read the same from the ROM as from its MWL export; Kaizo Kindergarten built by
  Kobo enters every level as the hack does, from each kind of entrance, and Lunar Magic
  saves the build keeping every setting. Lunar Magic's added layer 2 scroll settings
  build with Kobo's camera since 2026-10-01 (below). Builds refuse
  secondary entrances that exit to the overworld, midway redirects that loop, and Lunar
  Magic settings for entrances `1FE` and `1FF`, which its tables do not hold. Kobo's
  `$05D9E3` routine sets a separate midway entrance up whole, as Lunar Magic's does, so a
  saved build, whose `$05DA17` is then Lunar Magic's, enters it the same (checked with
  levels 104, 105, and 108). `smart_spawn` and `spawn_range` (`tTT`, which the
  entrance code leaves in `$0BF4`) are read by Kobo's sprite loader (below); `face_left`
  changes nothing observed and is kept; `auto_screens` is the editor's. The code keeps vanilla's WRAM addresses
  (`$0BDA`-`$0BDC`, `$1B93`), which an SA-1 version needs as defines.
  - Lunar Magic's save and MWL export keep a secondary entrance's two further bytes (the Y
    tile's high bits, `relative`, `face_left`, `water`) only with a marker at `$03BD9C` and
    the vanilla tables' pointers where its code keeps them (`$05DC81`, `$0DE191`-`$0DE1A1`),
    which `entrance.asm` writes; without them a save writes new, empty tables. They are in
    the register of checked bytes ([lunar-magic-install.md](lunar-magic-install.md)).
- 2b progress, sprite lists in any bank: Kobo's sprite data bank code (`sprite-banks.asm`,
  the `JSL` at `$05D8F5` Lunar Magic checks, the table at `$0EF100`) is part of every
  install, and a build with it puts a changed sprite list in a RATS block; a project whose
  lists would not fit bank `$07`'s unused space takes Lunar Magic's layout for that alone
  (`Project::installs_lunar_magic`). A level plays the same with its list in a RATS
  block as in bank `$07`, Kaizo Kindergarten imports and builds with every level the same
  as the hack's, and Lunar Magic's save keeps the table, the hook, and Kobo's code
  (2026-09-28).
- 2b progress, the sprite loader: Kobo's (`sprites.asm`) takes the whole group Lunar
  Magic installs for its loader, with the `JSL` at `$02AF3D` that makes a save keep it:
  the new sprite system's lists, 128 sprites, the spawn ranges and smart spawning of a
  level's `tTT`, the rows that spawn as the camera moves up and down, the despawn range,
  the Y jumps' positions, and the goal tape's extra bits, learned from what Lunar Magic's
  loader does with chosen lists and camera paths (`examples/sprite_probe.rs`). On vanilla
  saved by Lunar Magic, its loader and Kobo's spawn the same sprites in the same slots on
  every frame of 600 random scenarios (`tests/lunar_magic_save.rs` plays 24 of them);
  Kaizo Kindergarten, ValuableAndBeautiful, and SMW_2022-4-9 with Kobo's loader swapped
  in play every level as the hacks do, and so does Luminescent moved into a Lunar Magic
  3.70 ROM, whose taller levels use the new sprite system (the hack's own ROM, saved by
  3.30, erases one Sumo Brother a frame earlier in level `156` than 3.70's loader and
  Kobo's do). Kaizo Kindergarten built by Kobo survives Lunar Magic's save with every
  level the same.
  Builds write a list in the new sprite system's format when it needs it, take up to 128
  sprites, and refuse a list out of screen order (Lunar Magic's loader crashes on one);
  extension bytes build with the size table PIXI leaves, once PIXI runs before the
  levels, and an MWL import takes that table from a ROM (`--sizes-from`). 255 sprites
  come with the PIXI stage (below).
- 2b progress, taller levels: a level file's `rows` and `bottom_row` (`[header]`,
  `level::size`), from ROMs (the table `$240` bytes before the code the `JSL` at `$05DA8A`
  calls) and MWL files, built with Kobo's taller levels code (`exlevel.asm`, in every
  build that uses Lunar Magic's layout: the RAM screen tables, the height's bounds, the
  object loader's steps and screen jumps with a vertical part, the camera's bottom, the
  ground shaking), whose `$05DA8A` a save keeps along with the table. Learned from what
  Lunar Magic's code does with every size (`examples/exlevel_probe.rs`), and checked by
  swapping Kobo's in for Lunar Magic's on seven hacks moved into a 3.70 ROM
  (`tools/lunar-magic/with-kobo-exlevel`: the same RAM after every load, pictures, play,
  camera, and entrances); Luminescent's and ValuableAndBeautiful's levels built by Kobo
  resolve as the hacks do, and survive a Lunar Magic save (2026-09-28). `entrance.asm`
  places the layers by the level's height. Builds refuse a size a vertical level would
  ignore and objects on screens past the room a size leaves either layer (they would
  be written over low work RAM; no unlocked corpus ROM has any). A header screen count
  past that room builds since 2026-10-02:
  with Kobo's code swapped into QLDC 2021 `22_FerpyMcFrosting`, whose seven such levels
  have their objects within it, they load and play as under Lunar Magic's.
- Remaining in 2b, each Lunar Magic runtime code reimplemented from observation, by the
  method the pieces above used (a probe that runs the ROM's own code with chosen state,
  then an A/B comparison with Kobo's code swapped in; `examples/*_probe.rs`,
  `tools/lunar-magic/with-kobo`, `install-gate.py` for the save's checks):
  - Lunar Magic's added layer 2 scroll settings (8 to 11, `S`, `H`): done (2026-10-01).
    Kobo's entrance code sets them up and its camera at `$00F79D` runs every rate and
    the moving ones as Lunar Magic's does, frame by frame; a save keeps them, and Kobo's
    camera, with `"LM"` at `$05DD7C` (lunar-magic-install.md, "Layer 2 scroll settings";
    the marker's trade-off in review.md). The eight further sites the marker keeps
    (2026-10-02): Kobo's code at the five whose Lunar Magic code changes play (face left
    and the slanted pipe, the end fade's colours, layer 2's interaction a frame behind,
    a vertical level's camera), the game's at three where nothing was seen to differ
    (lunar-magic-install.md, "The sites a save keeps with the marker"). Finding the first
    meant running game mode `$11`'s `$009708` in Kobo's loader, which Lunar Magic hooks:
    that also fixed the pictures of Lunar Magic hacks whose entrances place layer 2
    relative to the player (Kaizo Kindergarten 153 levels, Akogare2 26, Luminescent 30;
    the ones checked now match Mesen's frames).
  - The VRAM patch's lag path and stripe upload, which Kobo's had left as the game has
    them (2026-10-02): a lagging frame keeps the layers' scroll registers in Lunar
    Magic's, and now in Kobo's (`$0081E2`); stripe images and the game loop hook change
    nothing Kobo's patch does not already match but the player's tile words
    (lunar-magic-install.md, "Graphics").
  - Taller levels: the castle "No Yoshi" intro, Choc Island 2's rooms, and the credits
    with a taller size, observed in Mesen (2026-10-01): the same tables as Lunar
    Magic's but for the offsets it leaves at the level's size (review.md). That found
    Choc Island 2's rooms crashing in any build that writes `0CD`-`0CF`, now fixed
    (`choc-island.asm`). The one entrance corner left then no longer reproduces
    (2026-10-04). Kaizo Kindergarten has
    no taller level; Luminescent and ValuableAndBeautiful were the build checks, the other
    five corpus hacks with taller levels the swap checks only.
  - The VRAM patch and the graphics loader, for PIXI, ExGFX, 4bpp GFX, per-level graphics
    lists, and the graphics bypass objects `24` and `25`. They are three pieces
    ([lunar-magic-install.md](lunar-magic-install.md), "Graphics"): the VRAM patch, a
    group a save installs whole unless `$00A5A2` is a `JML`, keeping the graphics tables;
    4bpp GFX, which Lunar Magic reads as such when `$00AAD8` is `$EA`; and ExGFX with the
    lists, which it reads with `$00AA47` = `$EA`, a `JSL` at `$0583B8`, and the pointer at
    `$0FF873`, and keeps as they are with `"LM"` at `$0FF15C`. The layout, the lists' slots
    in VRAM and their load order, where ExGFX `100`-`FFF` are, and how the patch streams
    the tilemaps are recorded there (2026-09-27). Kobo's VRAM patch (`vram.asm`, in every
    build that uses Lunar Magic's layout) goes at the group's own sites and leaves
    `$00A5A2` to Lunar Magic, whose first save puts its own in place; it leaves the same
    tilemaps as Lunar Magic's after every vanilla level's load and, frame by frame, along
    seven camera paths (2026-09-28). Kobo's graphics loader (`graphics.asm`, in a build
    that uses any of them: `[graphics] bpp = 4`, `[exgfx]`, `[bypass_lists]`, a level's
    `[graphics]`, objects `24` and `25`) stays after a save, and loads the same VRAM under
    Lunar Magic's VRAM patch as under Kobo's. Lunar Magic's layer 3 settings in the
    lists come with Kobo's layer 3 code (`layer3.asm`, `[graphics.layer3]`), which plays
    as Lunar Magic 3.70's frame by frame; builds refuse only what observation did not
    settle (what tides act like, AN2's bit 12, `advanced` in a tide level, a layer 3
    tilemap of size 3; reviewed 2026-10-04, lunar-magic-install.md). Kaizo Kindergarten imports and builds whole
    (2026-10-01): every level reads back as the hack's (`kobo diff`) and survives a Lunar
    Magic save (`save-check`), and its content moved into a Lunar Magic 3.70 ROM plays
    every level's layer 3 the same under Kobo's code as under Lunar Magic's. Against that
    ROM the build draws 421 of 512 levels the same; none of the other 91 has layer 3
    settings (taller levels among them; not the pipes, which Kaizo Kindergarten leaves
    as the game has them, and which builds carry since 2026-10-01).
    Left of the layer 3 settings (lunar-magic-install.md, "Layer 3 settings", reviewed
    2026-10-04):
    AN2's bit 12 (no effect seen, no corpus ROM sets it), `advanced` with a tide in a
    vertical level, and a layer 3 tilemap of size 3. A tide in a level with a size of
    its own builds since 2026-10-02, the rule for its rows found. The overworld's hook (`$00A153`) is
    not Kobo's: builds keep the overworld as the game has it. The player's tile words
    (`$6040`-`$6187`, `$67F0`) come from the direct page a level's load leaves
    (`$0C`-`$0D`, read by the game's player graphics setup) and from scratch RAM: a
    whole build leaves them as the vanilla ROM does on every frame, where a Lunar
    Magic-saved ROM does not (review.md).
  - ExAnimation: done but for trigger `0F` on the types that upload frames, which
    Lunar Magic's code makes upload its own scratch (refused, reviewed 2026-10-04). Kobo's
    code (`exanimation.asm`) runs every other type and trigger as Lunar Magic's does,
    frame by frame (rotations under every trigger since 2026-10-01); level files hold a level's list and
    settings (`[animation]`), the manifest the global list and the files `60`-`63`
    (`source::animation`); import reads them from ROMs (Lunar Magic 2.30 to 3.51; not
    2.41's older format since 2026-10-03, [known-gaps.md](known-gaps.md)) and MWL files,
    which Lunar Magic's export converts it in, and builds
    write them in Lunar Magic's layout, which its save keeps (checks: a `JSL` at `$00A390`,
    `$03FDFF` = `$00`). Every corpus ROM's lists read the same from the ROM as from its
    MWL exports; Kaizo Kindergarten builds with them, plays them as the hack does, and
    keeps them across a save.
  - Yoshi's tongue and berry acts-like hooks (3.70's `$02BA9E` and the rest): done
    (2026-10-01). Kobo's check follows the chain, reads vertical levels and layer 2,
    and places a layer 2 berry's mushroom on layer 1, as Lunar Magic's does in every
    scenario tried, where it does not hang ([lunar-magic-install.md](lunar-magic-install.md),
    "Yoshi and berries"; what it leaves out in [review.md](review.md)).
- The unattended run (2026-09-27 to 2026-10-01, PRs #36-#51) landed the VRAM patch and
  graphics loader, PIXI, the sprite loader, ExAnimation, taller levels, layer 3 settings,
  Map16 pages 0 and 1, the time limit bypass, LC_LZ3 for SA-1, and the companion build
  repository. It stopped there on purpose; what it decided without review is in
  [review.md](review.md). What is left of step 2, not started unless said:
  1. **SA-1.** Done (2026-10-01): Lunar Magic's SA-1 install observed, every patch on
     SA-1 Pack's memory map through `memory.asm`, SA-1 projects build with Lunar Magic's
     layout, and the build, install, and Lunar Magic checks run on both
     (lunar-magic-install.md, "On an SA-1 ROM", and sa1.md; reviewed 2026-10-04).
  2. **A corpus build sweep.** Done (2026-10-01, `tools/corpus-sweep`; how to run it in
     [testing.md](testing.md)): the 47 `KOBO_LM_ROMS` hacks and the 128 QLDC patches (135
     LoROM, 40 SA-1; Lunar Magic 1.62 to 3.51), each imported, built with every level a
     build refuses left out, compared with the hack, and saved by Lunar Magic. 174 import
     (Baby Kaizo World 3 did not: one level's object data has no terminator, and the
     import stopped for it; an import now leaves such a level out with a note, and
     Baby Kaizo World 3 imports 235 levels, four left out, 2026-10-02). Of their 14,217 changed
     levels, 13,276 build; 85 hacks build whole, and 62 of those also survive the save.
     Every build reads back as its hack (`kobo diff`), but for Grand Poo World 2, whose
     512 level pointers all name `$068000` while the game loads its levels from elsewhere:
     Kobo imports none of its levels. Lunar Magic sees none of them either, so it is
     treated as a locked ROM ([lunar-magic.md](lunar-magic.md), 2026-10-02). What blocks the rest, by hacks and levels:
     - Sprite entries longer than the vanilla size table says (74 hacks, 522 levels; the
       only blocker of 47 hacks): a PIXI hack's project had no `[pixi]`, so the build
       sized sprites by vanilla's table. Done (2026-10-03, below): an import carries the
       hack's PIXI insert as compiled code, or takes its PIXI folder as source.
     - Graphics lists naming ExGFX the project does not have (15 hacks, 111 levels): the
       six locked ROMs, whose ExGFX are not imported (Invictus alone 81 levels), and
       levels naming a file the hack does not have either (QLDC 2021 `08_Lizstar`: ExGFX
       `FF`, a null pointer), which build since 2026-10-02, the slot loading nothing as
       under Lunar Magic's code (nine hacks; an import notes each).
     - Lunar Magic's added layer 2 scroll rates (11 hacks, 23 levels; item 3), and tide
       levels with `advanced` or `tides_act_as` (12 hacks, 17 levels; item 4).
     - Lunar Magic's own objects (6 hacks, 134 levels: Invictus, riff2, both Super Dram
       Worlds, QLDC 2022 `05_Bumpty`, SMW_2021-4-24), and objects `24` and `25` in layer 2
       (4 hacks, 15 levels). Every one of the latter is in a locked ROM (Invictus, both
       Super Dram Worlds, SMW_2021-4-3, Baby Kaizo World 3), whose added objects are the
       lock's own bytes (`54 41 52`, "TAR"), so they are out of scope. riff2's, and
       Bumpty's, are long screen exits, which build since 2026-10-02; what is left of the
       others is in locked ROMs (Bumpty and SMW_2021-4-24 are locked too).
     - More screens than the level's size leaves room for (5 hacks, 82 levels): a screen
       count past it builds since 2026-10-02, and the objects past it in three SA-1 hacks
       (74 levels) were a misread size table: on SA-1 the hook's bank `$80` is the ROM's
       third megabyte, not bank `$00`'s mirror (lunar-magic-install.md, "Taller levels").
       Read where the hook says (2026-10-02), Heraga and Bench-kun import 8 and 24 changed
       levels, not 300 and 408, and the SA-1 hacks refuse no level for their size. Objects
       past a layer's edge (2 hacks, 14 levels; and QLDC 2022 `05_Bumpty`'s level `105`
       once its long exits build): all in locked ROMs. More than 128 sprites without PIXI's
       loader (3 hacks, 6 levels); sprites out of screen order (1 hack, 5 levels); a
       midway entrance that redirects in a loop (1 hack, 3 levels; QLDC 2021
       `84_TickTockClock`, whose levels redirect to `106`, which redirects to itself: Lunar
       Magic's help warns that such a cycle never ends, so it stays refused). The 9 entrances to an
       undefined level were all to levels the sweep had left out.
     Lunar Magic's save of the builds (`save-check`, level `105` re-imported) changes
     level `105` in 41 of 174. 39 lose bit 12 of SP4 in the graphics list, which Lunar
     Magic 3.70 clears in every level it saves (`$F020` becomes `$E020`), and one of them
     also has its screen count raised by one: Lunar Magic's Auto-Set Number of Screens
     counts an object's extent, and that level has one reaching a screen past its count;
     the hack's own save does the same (lunar-magic.md, 2026-10-02). 2 have screen exits turned
     secondary, a real loss: Lunar Magic's save does that to the game-format exits of a
     level that also has an exit in its own format ([lunar-magic.md](lunar-magic.md)),
     and Kobo's import writes 262 levels of 68 hacks so. Builds now write every exit of
     such a level in Lunar Magic's format, and both hacks' level `105` survives the save
     (2026-10-02).
     `render_hashes` draws 5,400 of the 13,276 built levels as the hack does; every hack
     keeps 68 KB and more of changes the import does not carry (Lunar Magic's own code
     among them), so the pictures do not single out a blocker.
     What the sweep suggests taking next, by what it unblocks for its cost: a PIXI
     hack's sprites (74 hacks; done 2026-10-03, below); then Lunar Magic's objects and
     the scroll rates (item 3). Grand Poo World 2's level data and SP4's bit 12 were observed
     (2026-10-02; lunar-magic.md): the first is hidden as a lock hides it, the second
     changes nothing a load leaves.
     Run again on 2026-10-03, after the PIXI import, long exits, tides, and the size table
     read where its hook says: all 175 import, 163 build whole (85 before), 13,678 of
     13,766 changed levels build (the count fell with the SA-1 size tables read right),
     and every build reads back as its hack. What is refused is in the locked ROMs
     (Invictus, Baby Kaizo World 3, both Super Dram Worlds, SMW_2021-4-3 and -4-24, QLDC
     2022 `05_Bumpty`: objects past a layer's edge or rows, objects `24` and `25` on layer
     2, and the entrances to the levels those leave out), but for what stays refused on
     purpose: more than 128 sprites without a 255-sprite loader (QLDC 2021 `34_idol`, QLDC
     2022 `30_Fellipe_R`, and QLDC 2021 `48_JamesD28`, whose PIXI code an SA-1 import
     cannot carry), `34_idol`'s lists out of screen order, and the midway loops. Lunar
     Magic's save keeps level `105` in 118 builds; in the other 57 it changes it only as
     it does every level it re-imports (SP4's bit 12 in 53, the header's
     screen count in 7), which now shows in 18 more hacks because their level `105`
     builds. Map16 pages in an older Lunar Magic's layout (12 hacks, 1.62 to 2.52), which
     imports had left out, import since 2026-10-04 through the ROM's own Map16 routine, as
     Lunar Magic 3.70's export shows them (lunar-magic.md); more of those hacks' levels
     draw as the hacks (apes 5 to 30, Grand Poo World 21 to 44, Learn 2 Kaizo 4 to 72).
  3. **Lunar Magic's added layer 2 scroll rates**: done (2026-10-01), above.
  4. **Refusals that observation did not settle**, each in review.md with how to settle
     it: four layer 3 settings, out-of-order sprite lists, taller-level corners
     (ExAnimation's triggers and rotations: done, 2026-10-01, but `0F` on frame types).
     Tides' `tides_act_as` and `advanced`: built since 2026-10-01, and a tide in a level
     with a size of its own since 2026-10-02 (QLDC 2021 `56_TheKazooBloccGosh` and
     `30_Galaer` build whole, their tide levels loading as the hacks'), but `advanced`
     with a tide in a vertical level; AN2's bit 12 stays refused. Out-of-order sprite lists stay refused: the one hack with them
     crashes in those levels under its own code (review.md).
     The vertical pipes' colours and the diagonal pipes: carried since 2026-10-01 (the
     pipes file).
  5. **Yoshi's tongue and berries**: done (above). The player's tile words in a whole
     build: settled, as the vanilla ROM has them (review.md).
  6. **Tools left as the user's own:** AddmusicK and GPS are not fetched (no licence, no
     stable source for every platform). SA-1 Pack 1.40 is fetched by hash from its
     GitHub release (`tools/upstream.toml`, done 2026-10-03), and CI runs it on all three
     platforms.
  7. **Baserom template projects** ("No base", above): not started.
  8. **Closing step 2:** fold what stays true of this file into the other docs and delete
     it, as its header says.
- 2b then takes Lunar Magic-layout features one at a time, each through its source format,
  import from MWL and ROM, build, the Lunar Magic check, and the corpus check together, so
  neither direction anchors the format: Map16 pages 2 and up and background Map16, custom
  palettes, ExGFX, expanded level sizes, the sprite data formats (new sprite system, 255
  sprites, PIXI extension bytes), secondary entrances and exits. Each is checked with Lunar
  Magic saving the build, and with a hack's content transferred by Lunar Magic's command
  line into a Lunar Magic ROM with Kobo's pieces swapped in (`tools/lunar-magic/with-kobo`).
- 2c progress, GPS: `[gps] dir` (its `list.txt`, `blocks/`, `routines/`), laid over the
  user's GPS folder (`tools.gps` or `KOBO_GPS`, never bundled), runs as the blocks stage
  after Map16, on Kobo's bank `$06` code, which it patches unchanged. PIXI refuses a ROM
  without Lunar Magic's VRAM patch (`$00F6E4` a `JML`), which Kobo's builds now have.
- 2c progress, the companion build repository: `kobo-smw/kobo-tools` builds Asar, PIXI, and
  UberASM Tool from pinned commits for Linux x64, Windows x64, and macOS arm64 and x64,
  and publishes them with their sources; Kobo fetches the one for its platform on first
  use (`Tool::locate`, `kobo tools fetch`), checked against the hashes in
  `tools/pinned.toml`, and CI tests against those builds, offline, on all three platforms.
  PIXI's runs as the sprites stage (below).
- 2c: running the tools (user Asar patches, PIXI, GPS, UberASM Tool, AddmusicK) and SA-1
  builds with SA-1 Pack. PIXI and GPS need the
  acts-like chain, which is done, and PIXI Lunar Magic's VRAM patch at `$00F6E4`, a hook
  a save restores.
- PIXI hacks imported (2026-10-03; the maintainer decided both ways): `kobo import
  --pixi folder` takes a hack's PIXI folder as the project's sprites' source, and checks
  that PIXI 1.43 makes the hack's size table from it; without it, the import carries the
  insert PIXI left in the ROM as compiled code (`[pixi] compiled`, `kobo_core::pixi`,
  toolchain.md "PIXI 1.43"), which builds write back where PIXI put it before anything
  else takes space. The 74 corpus hacks the sprite sizes blocked (PIXI 1.02 to 1.42; 47
  LoROM, 27 SA-1) all import and build: 63 carry their code, and 11 SA-1 QLDC entries
  made on an older SA-1 Pack, whose PIXI blocks are where SA-1 Pack 1.40 has its own,
  their size table alone. Levels refused across them went from 847 to 46 (none for
  sprite sizes), 68 build whole (none did), and `kobo diff` finds every build the same
  as its hack. Each of the 63 imports its own build back to the same insert, and Lunar
  Magic's save keeps it (checked on six); its save changes level `105` only as it does
  any level it re-imports (SP4's bit 12, the screen count), which shows now that level
  `105` builds in more of them. Played in Mesen against the hacks, ten levels of five of them
  (PIXI 1.02, 1.2.9, 1.2.13, and 1.32, LoROM and SA-1) have the same sprites in the same
  slots, doing the same, 240 frames in; what differs is the status bar, the hacks' own
  patch, which the import reports and does not carry.
- 2c progress, PIXI: `[pixi] dir` (its `list.txt`, `sprites/`, `routines/`, ...), laid
  over the pinned PIXI 1.43 (or `tools.pixi`, `KOBO_PIXI`), runs as the sprites stage
  after Map16 and before GPS, with MeiMei off; the levels stage sizes sprite entries by
  its table (toolchain.md), and a level may have 255 sprites, since PIXI's option
  declares a 255-sprite loader at `$0FFFE0`. A Lunar Magic save keeps everything PIXI
  wrote, and a secret-exit goal tape still gives the secret exit (Risks, below).

## Risks

- The one-time set's behaviour has to be worked out without reading Lunar Magic's code:
  from documentation (the help file documents the GFX decompression routine, the screen
  exit routine, and the Map16 acts-like code's contract and `JSL` slots; see
  [lunar-magic.md](lunar-magic.md)), tool sources, and the memory effects of running
  Lunar Magic-saved ROMs.
- Lunar Magic operations the spike did not try, the GUI's options especially, may write
  inside what Lunar Magic takes to be its own code at the fixed addresses, which in a Kobo
  build is Kobo's.
- Clean-room contamination, from an agent or contributor working from Lunar Magic's code.
- Lunar Magic's layout changing between versions: read many, write one.
- Import losing data silently. The import report and raw fields cover it.
- What a render does not show: exits, entrances, midway points, and secondary headers need
  the Lunar Magic check or emulator entry tests.
- The code GPS patches has to have a shape GPS's source describes, which pulls Kobo's
  bank `$06` code towards Lunar Magic's. It is written from GPS's source, the vanilla
  disassembly, and observed behaviour only, and reviewed with that in mind.
- AddmusicK overwrites `$0FF035`-`$0FF050`, which Lunar Magic's install fills. Lunar
  Magic's first save of a build with music writes its bytes back over AddmusicK's unused
  `$55` filler there and leaves AddmusicK's code and data alone
  ([lunar-magic.md](lunar-magic.md)). Every save rewrites `$0FF035`, and bytes below it
  that are not `$FF`, from the state of the one-time code; what they record is unknown
  ([lunar-magic-install.md](lunar-magic-install.md)).
- Lunar Magic's layout fixes operands inside code: the Map16 page table pointers in the
  `$06F540` routine, the secondary entrance table pointers at `$05DC81`-`$05DC8D` and
  `$0DE191`-`$0DE1A1`, which every save rewrites, so Kobo's code has to put them there.
- The tools: licences (three have none), directory-order dependence, and UberASM Tool on
  .NET outside Windows.
- Pieces left for Lunar Magic's first save are installed after the tools ran in the
  build. PIXI picks its `!EXLEVEL` code from the version digits at `$0FF0B4`, which
  Kobo's builds leave `$FF`, which PIXI reads as a Lunar Magic 3 ROM: it assembles the
  same code as for a ROM Lunar Magic has saved. A save of a PIXI build keeps PIXI's goal
  tape hook at `$01C089` and every other site PIXI wrote, and level `0EB`'s secret-exit
  goal tape still works (checked 2026-09-28 with Kobo's sprite loader in, whose check
  keeps Lunar Magic's own group out, tests/lunar_magic_save.rs). Before Kobo's sprite
  loader went in, the save installed Lunar Magic's code over `$01C089` and the goal tape
  gave the normal exit; a piece that leaves a group of Lunar Magic's to its first save
  must be checked the same way against the tools that ran before.
- The corpus is ROMs, not projects; every test project is made by exporting from Lunar
  Magic under Wine, and only hashes are committed.
- Drifting towards Lunar Magic parity.
