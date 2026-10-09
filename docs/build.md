# Builds

What a Kobo build is, the decisions it rests on, and what it leaves to Lunar Magic.
Roadmap step 2 made it (2026-09-25 to 2026-10-05); the rules that follow from these
decisions are in [AGENTS.md](../AGENTS.md), the clean-room ones in
[clean-room.md](clean-room.md).

A project directory builds into a ROM from a clean SMW ROM. Levels, Map16, palettes,
ExGFX, ExAnimation, and the overworld are written natively in Lunar Magic's layout;
existing work is imported from ROMs, MWL files, Lunar Magic's Map16 exports, and Callisto
projects; Asar, PIXI, GPS, UberASM Tool, and AddmusicK run in a fixed order. What Kobo
does not cover yet (four of Lunar Magic's overworld Extra Options, the title screen's
demo moves, credits, messages) is finished in Lunar Magic on the built ROM.

## Lunar Magic and Kobo builds

- Required: Lunar Magic opens a Kobo-built ROM, shows every piece of Kobo-managed content
  correctly, and saves without losing any of it (`tools/lunar-magic/save-check`,
  [testing.md](testing.md)).
- Not a goal: pulling Lunar Magic edits back into a project. Import from a ROM exists for
  migration, and if it is stable a round trip falls out for supported features, but effort
  goes into supporting a feature, not into working around its absence.
- `kobo build` always overwrites its output. The ROM is a build artefact.
- A build does not carry the `Lunar Magic Version` string at `$0FF0A0`. The hook spike
  showed Lunar Magic neither reads it to decide what is installed nor needs it, and writes
  it on its own first save. A build in Lunar Magic's layout writes one byte there, `$00`,
  which the retry system checks for (maintainer, 2026-10-05; lunar-magic-install.md,
  register).
- A build always has a correct internal checksum; Lunar Magic warns that a ROM "may be
  Corrupt" otherwise.
- Kobo writes the layout of one Lunar Magic release, 3.70, and pins it for the Lunar Magic
  checks. It reads many: imports take ROMs from Lunar Magic 1.62 on, converting older
  layouts as 3.70 does.

## No base

- A build always starts from the clean ROM. No ROM or BPS belongs in a project.
- Baseroms are supported as projects: import from a ROM or a Callisto project, and
  template projects for widely used baseroms (`kobo new --template`). A template
  references the baserom's own release by URL and SHA-256; Kobo carries nothing of it.
- Import reports what it did not carry over: regions that differ from vanilla which Kobo
  does not model, tagged blocks no level uses, and patched hook sites.
- A level the project does not list keeps the clean ROM's content, so a project holds only
  the levels it defines, and no Nintendo level data unless its author changed it. An empty
  level file blanks a level.

## Source formats

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
- The shared palettes, the game's colour tables from `$00B0A0` (`BackAreaColors` to
  `OWSpecialColors`, 1009 colours, which Lunar Magic edits in place and exports whole as
  its shared palette), are one file (`[palettes] shared`, `source::palettes`): a table
  per section, named after the disassembly's, listing only the colours a project
  changes, by number in the table, as `#RRGGBB`; the rest are the clean ROM's. The
  graphics stage writes them in place. A ROM import and a Callisto import carry a
  hack's; Lunar Magic exports a build's as written, and its import of that export makes
  the same tables (`lunar_magic_save.rs`).
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

## Stages

Stages run in a fixed order, from rarely changed and slow to often changed and fast, so
that editing a level never reruns AddmusicK or moves a tool's code (`build::Stage`). The
constraints come from the tools ([toolchain.md](toolchain.md)):

1. `Base`: check the clean ROM's hash. For SA-1, apply SA-1 Pack (with `$0FFFEB` set
   first for LC_LZ3), then its 6 or 8 MiB patch. Expand to the manifest's size, filled
   with `$00`.
2. `SpriteBlocks`: a compiled PIXI insert's blocks where PIXI put them in the hack, before
   anything else takes space, since its code is not relocatable.
3. `Install`: Kobo's ROM-side patches, when the project uses Lunar Magic's layout, none of
   it in AddmusicK's ranges (`$0E8000`-`$0EF0FF`, `$0F8000`-`$0FF050`).
4. `EarlyPatches`: the project's early Asar patches.
5. `Music`: AddmusicK, which needs `$0E8000` untouched and everything before it
   RATS-tagged.
6. `Graphics`: GFX files and ExGFX.
7. `Map16`: Map16 pages and the acts-like tables. GPS rewrites that table, and PIXI and GPS
   refuse a ROM without its pointer at `$06F624`.
8. `Sprites`, `Blocks`, `UberAsm`: PIXI (with `-meimei-off`), GPS, UberASM Tool, in that
   order: UberASM Tool reads the flag PIXI sets at `$0FFFE0`.
9. `LatePatches`: the project's late Asar patches (patches that hook the tools' code).
10. `Levels`: after PIXI, whose size table sets the length of each sprite entry.

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

A build also gives the labels of the code it put in the ROM (`build::Symbol`, from each
Asar patch's labels as `asar::collecting_labels` gathers them): Kobo's install patches'
named `kobo_` and the patch, the project's patches' as they are, and the play patch's
`kobo_playtest_`. The stage cache keeps them beside each snapshot (`.sym`), so a build
from the cache has the same. `kobo build --sym`, the editor's build, and play builds
write them beside the ROM as a WLA-DX symbol file, which Mesen 2 loads by default
(checked 2026-10-08: `emu.getLabelAddress` finds a patch's label) and bsnes-plus loads by
name.

## Lunar Magic's layout, piece by piece

A build that writes nothing only Lunar Magic's layout holds (and whose changed sprite
lists fit bank `$07`) leaves `$06F600` at `$FF` and installs nothing; Lunar Magic's first
save installs itself and keeps Kobo's data. Otherwise the build installs Kobo's code
(`build::Stage::Install`, `kobo_core::install`, `Project::installs_lunar_magic`).

A save decides piece by piece whether Lunar Magic's install is there, and installs each
missing piece over whatever is at its sites, resetting its tables
([lunar-magic-install.md](lunar-magic-install.md#how-a-save-decides-what-to-install)). So
a feature brings the pieces whose tables it writes, as Kobo's clean-room code with the
check for each met, and leaves the rest for Lunar Magic's first save to install. Where a
check is on Lunar Magic's own code or a save always retargets the hook, Kobo's code runs
until the first save and Lunar Magic's after it, reading the same data. Which piece each
feature needs, and each piece's check, is in `kobo_core::install`'s documentation; what
each does, how it was found, and how it was checked against Lunar Magic's, in
lunar-magic-install.md's sections:

| Feature | Kobo's code (`asm/lunar-magic/`) | lunar-magic-install.md |
|---|---|---|
| Map16 pages 2 to `7F`, page 2 per tileset, acts-like settings, GPS's slots | `map16.asm`, `actslike.asm` | "Map16 pages and the acts-like chain", "Custom block actions, observed" |
| Lunar Magic's objects (`22`-`29`, `2D`), long screen exits | `objects.asm` | "Placed objects" |
| Level number, per-level tables | `level.asm` | "Per-level tables" |
| Backgrounds and BG Map16 | `background.asm` | "Backgrounds and the level load's uploads" |
| Custom palettes | `palette.asm` | "Custom palettes" |
| Exits, secondary entrances, entrance settings, midway points, layer 2 scroll settings | `exits.asm`, `entrance.asm`, `entrances.asm` | "Entrances, exits, and midway points", "Layer 2 scroll settings", "The sites a save keeps with the marker" |
| Sprite lists in any bank, the sprite loader | `sprite-banks.asm`, `sprites.asm` | "Sprites" |
| Taller levels, Choc Island 2's rooms | `exlevel.asm`, `choc-island.asm` | "Taller levels" |
| The VRAM patch, 4bpp GFX, ExGFX, per-level graphics lists, objects `24` and `25` | `vram.asm`, `graphics.asm` | "Graphics" |
| Layer 3 settings and tides | `layer3.asm` | "Layer 3 settings" |
| ExAnimation | `exanimation.asm` | "ExAnimation" |

Every patch includes `memory.asm`, so the same sources serve LoROM and SA-1 Pack
([sa1.md](sa1.md), and lunar-magic-install.md, "On an SA-1 ROM").

## What builds refuse

What observation did not settle, or what would not work under Lunar Magic's code either,
builds refuse, with a message naming the level. Each is described where its feature is:

- More than 128 sprites without a 255-sprite loader, and a sprite list out of screen
  order, which Lunar Magic's loader crashes on (lunar-magic-install.md, "Sprites").
- A midway entrance that redirects in a loop, secondary entrances that exit to the
  overworld, Lunar Magic settings for entrances `1FE` and `1FF`, and an exit past the
  tables ("Entrances, exits, and midway points").
- A size a vertical level would ignore, and objects on screens past the room a size
  leaves either layer ("Taller levels").
- Layer 3: AN2's bit 12, `advanced` with a tide in a vertical level, and a layer 3
  tilemap of size 3; files larger than their buffers ("Layer 3 settings", "Still to
  find").
- ExAnimation trigger `0F` on the types that upload frames ("ExAnimation").
- Two tileset files for tilesets that share the game's table (above).
- LC_LZ3 GFX on a LoROM build, which would need an LC_LZ3 decompressor of Kobo's own; a
  LoROM hack with LC_LZ3 GFX imports and builds with LC_LZ2, which reads the same. Kept
  so (maintainer, 2026-10-03): LoROM loses about 5% of the GFX's space; revisit if a
  project runs short of it. Graphics editing (roadmap step 4) writes LC_LZ2 as the
  build does, and did not need it.

Locked ROMs are out of scope: their levels are hidden or encoded by the hack's own code,
and import leaves out what it cannot read, with a note. The corpus sweep's latest results,
and what each hack still has refused, are in [testing.md](testing.md).

## What is left to Lunar Magic

- The title screen's demo moves, the credits, and messages: builds keep them as the
  game has them, and Lunar Magic edits them on the built ROM. A project's overworld
  (`[overworld] file`, its layers and 16x16 tiles, level tiles, names, events, start,
  settings, graphics lists, palettes, ExAnimation, border, the title screen's layer 3,
  and the tables the game keeps in place) is built in
  Lunar Magic's layout (lunar-magic-install.md, "The overworld"), and an import carries
  a hack's, with Lunar Magic's Extra Options but four (the default clouds, saving after
  the intro message, Luigi's map position, and lightning colours from the ROM), which
  are left to Lunar Magic.
- Builds are not FastROM ([known-gaps.md](known-gaps.md)).
- Lunar Magic's GUI operations (overworld save, message and title screen edits,
  ExAnimation, custom palettes, the VRAM patch options) have not been tried on a Kobo
  build: every Lunar Magic check so far runs its command line. They may write inside
  Kobo's install. lunar-magic-install.md ("Unknowns") has how to check them.

## Risks

- Lunar Magic's layout changing between versions: read many, write one.
- Import losing data silently. The import report and raw fields cover it.
- What a render does not show: exits, entrances, midway points, and secondary headers need
  the Lunar Magic check or emulator entry tests ([testing.md](testing.md)).
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
  .NET outside Windows ([toolchain.md](toolchain.md)).
- Pieces left for Lunar Magic's first save are installed after the tools ran in the
  build. PIXI picks its `!EXLEVEL` code from the version digits at `$0FF0B4`, which
  Kobo's builds leave `$FF`, which PIXI reads as a Lunar Magic 3 ROM: it assembles the
  same code as for a ROM Lunar Magic has saved. A save of a PIXI build keeps PIXI's goal
  tape hook at `$01C089` and every other site PIXI wrote, and level `0EB`'s secret-exit
  goal tape still works (checked 2026-09-28 with Kobo's sprite loader in, whose check
  keeps Lunar Magic's own group out, `tests/lunar_magic_save.rs`). Before Kobo's sprite
  loader went in, the save installed Lunar Magic's code over `$01C089` and the goal tape
  gave the normal exit; a piece that leaves a group of Lunar Magic's to its first save
  must be checked the same way against the tools that ran before.
- The corpus is ROMs, not projects; every test project is made by importing a hack or
  exporting from Lunar Magic under Wine, and only hashes are committed.
- Drifting towards Lunar Magic parity.
