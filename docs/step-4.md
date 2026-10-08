# Step 4: the overworld, layer 3, graphics, palettes, and the emulator

The plan for roadmap step 4, started 2026-10-08 straight after step 3, without the
maintainer: its decisions are taken as each piece lands, and the judgement calls among
them go to [review.md](review.md). Fold what stays true into the other docs as step 4
lands, and delete this file when it is done.

## Goal

What a hack is made of beyond its levels can be edited in the editor and built from the
project: the Map16 tiles, the graphics, the palettes, layer 3, ExAnimation, and the
overworld, each as text or indexed PNGs in the project, each built in Lunar Magic's
layout so that Lunar Magic and the toolchain still open the result. An emulator starts
from the editor where the user is working.

## Decisions

- **The editor edits project files, never the ROM.** As levels have `LevelDocument`, each
  kind of file gets a document in `kobo_core::edit`, its changes as `Edit`-like values,
  undo by snapshot, saved in Kobo's format. The editor stays a thin shell.
- **What a project does not hold is the clean ROM's, and editing it adds it.** The rule
  of levels and Map16 (a file leaves out what the build starts from) holds for every new
  kind of file: editing a game's GFX file, Map16 tile, or palette adds what is changed to
  the project, as choosing a game's level adds it, and nothing of Nintendo's is written
  that the user did not change.
- **Pictures still come from a build.** A Map16 tile, a graphics file, or a palette
  changed in the editor is shown in the level by building and loading it, as an object is.
- **The order is what a level needs first.** Map16 tiles, then the graphics and palettes
  they draw with, then ExAnimation and layer 3, which are a level's too; the overworld
  last, since it is the largest and touches the fewest levels' pictures. Emulator work
  comes in where it helps test the rest.

## Work order

1. **Map16 editor.** A page at a time: each tile's four 8x8 tiles (character, palette,
   flips, priority) and what it acts like, picked from the level's graphics; pages 0 and
   1 and the tileset tables listing only what changes. `edit::Map16Document` over the
   page files.
2. **Graphics.** Every GFX and ExGFX file the level loads, shown with a palette row of
   the level's; drawing in them (pencil, fill, pick), a game's file added to the project
   when changed; importing a PNG or `.bin` into a slot.
3. **Palettes.** The shared palettes (the game's colour tables, which Lunar Magic edits
   in place) as a project file the build writes there; the level's own palette editor
   kept as it is.
4. **ExAnimation** edited in the editor rather than its file.
5. **Layer 3**: a level's layer 3 tilemap file, edited as tiles on the canvas.
6. **The overworld**: read, a source format, built in Lunar Magic's layout, and edited
   (layers 1 and 2, the level tiles and their translevels, paths, events, sprites). The
   level list then groups and names levels by the project's own overworld (editor.md,
   "The level list by overworld level"). Started 2026-10-08 at the maintainer's word,
   after review: a research phase as step 2's was, then the build and the editor.
   The game's own format numbers translevels by the order of the level tiles and fixes
   its events' places (smw.md, "The overworld"), so editing it in place would renumber
   levels with every level tile moved: a project's overworld has to be built in Lunar
   Magic's layout, with Kobo's own code for each of its overworld hooks, found as step 2
   found the level ones. Lunar Magic saves an overworld only from its window, but
   `-TransferOverworld` copies one between ROMs on the command line, which lets byte
   diffs of a corpus hack's overworld moved into a clean ROM show what that layout holds.
7. **The emulator**: Play from here already opens one, the configured one if there is
   one (`play.emulator`); builds give it the labels of Kobo's and the project's code
   (`.sym`). Driving an emulator from the editor (breakpoints, watching RAM) stays to
   do, through `kobo_core::clean_room`.

## Done

- The overworld's research, first part (2026-10-08): the layout's sites
  (lunar-magic-install.md, "The overworld"), `expand::load_overworld` (a new game played
  by Kobo's machine to the overworld, the yardstick), and `overworld::Overworld::read`
  (layer 1 and its pages, translevels, directions, layer 2, names, events) for the game's
  layout and Lunar Magic's, equal to the load in every corpus hack that reaches an
  overworld but locked and older-version ones.
- The overworld built (2026-10-08, the load): `overworld::Changes` and its file
  (`source::overworld`, `[overworld] file`), the clean ROM's overworld in Lunar Magic's
  shape with a project's changes, written in that layout by `Stage::Overworld`
  (`Overworld::plan`), and Kobo's code for the load (`asm/lunar-magic/overworld.asm`).
  Every readable corpus hack's overworld builds, reads back the same, and loads as the
  hack's does (tests/overworld.rs).
- Import carries a hack's overworld (2026-10-09): `overworld.toml`, what it changes of
  the clean ROM's, with its path reveal speed; riff2's import builds to the same
  overworld and plays every level's events as riff2 does. Still left: Lunar Magic's
  other Extra Options.
- The overworld's graphics (2026-10-09): each submap's graphics list (`[graphics]`,
  Kobo's code for the `JSL` at `$00A140`), Lunar Magic's 14 overworld palettes
  (`[palettes.0xNN]`, Kobo's code for the `JSL` at `$00AD32`), and layer 1's 16x16
  tiles moved where Lunar Magic's layout keeps them, up to `$1FF` (`[tiles]`), all
  carried by import (lunar-magic-install.md, "The overworld"). Kaizo Kindergarten's
  import builds to the hack's tilemaps and graphics on every submap.
- The overworld's ExAnimation (2026-10-09): each submap's list and settings and the
  global list (`exanimation::OverworldAnimation`, `[animation.0xNN]` and
  `[animation.global]` in the overworld file), with Kobo's code for its three hooks and
  the level code's engine shared (`exanimation-engine.asm`); a list's AN2 file is the
  overworld's animated tiles' source. Kaizo Kindergarten's import animates as the hack
  on every submap, and events as triggers play as Lunar Magic's. Still left: the
  first frames after a submap change, and Lunar Magic's FG1-2 merge.
- The events' further tiles (2026-10-08): the game's list of 44 and Lunar Magic's
  tables of them, read (`Events::extras`), carried in the overworld file (an event's
  `extras`), and Kobo's code for the layout's two hooks of them (`$04E9F7`, `$04DCA5`),
  which made the layer 2 load's loop Kobo's too. Loads with events passed
  (`expand::load_overworld_passed`) and events' ends (`expand::end_event`) of Kobo's
  builds are the hacks' own in every corpus hack whose own code leaves them alone.
- Level names and where a new game starts (2026-10-08): Kobo's code for the layout's
  two name hooks (`$048E81`, `$049549`), and the players' start and the level tiles a
  new game opens carried (`[start]` in the overworld file). Kobo's builds of 33 of the
  corpus's `.smc` hacks now load as the hacks do, name and all; the rest draw names with
  code of their own.
- Entering levels (2026-10-08): Kobo's code for the layout's level number hook
  (`$05D8B1`): a translevel's level by the translevel, on either map.
- The overworld's tables and settings (2026-10-09): the tables kept in place
  (`overworld::TABLES`), the event tile data's split, and each translevel's settings
  (`level_flags`, with Kobo's code for the layout's save prompt and no-entry flags);
  Lunar Magic's transfer reads a Kobo build's overworld whole (two bytes it checks,
  found by bisecting), and warps from every star and pipe tile go where the hacks'
  do.
- The overworld drawn (2026-10-09): `render::render_overworld`, layers 1 and 2 of the
  map a player on a submap sees, from the ROM's own load; `kobo overworld png`. Kobo's
  build of the vanilla overworld draws as the game's on every map.
- The overworld window (2026-10-09): `edit::OverworldDocument` (the overworld file
  open, every change worked out again against the clean ROM's, undo by snapshot) and
  `Workspace::set_overworld`; the editor's window shows each map from a build, draws
  layer 1 and layer 2 tiles, sets level tiles' translevels and direction bytes, each
  level's event, and where a new game starts, renames levels, and shows and edits an
  event's layer 2 blocks and layer 1 tile (editor.md, "The overworld"). The events'
  further tiles, crushed tiles, the reveal list, and sprites are to come.

- The emulator to play in (2026-10-08, item 7): `play.emulator` in the config file, or
  `KOBO_EMULATOR`, opens Play's and Build and play's ROM in place of what the system
  opens a ROM with.
- Symbols for emulators' debuggers (2026-10-08, item 7): a build's labels of Kobo's and
  the project's code, kept through the stage cache, written beside the ROM as a WLA-DX
  `.sym` by `kobo build --sym`, the editor's builds, and play builds; Mesen 2 loads it
  (docs/build.md, "Stages").

- The Map16 window (2026-10-08): `edit::Map16Document` over the page, game page, and
  tileset files, its changes as `TileChange`s for a tileset, undo by snapshot, saving
  new files into the manifest, and `Workspace::set_map16` for builds of unsaved edits;
  the editor's window, with the level's own tiles to pick 8x8 tiles from (editor.md,
  "Map16"). Background Map16 and the pipes' colour sets and diagonal tiles followed
  the same day.
- The graphics window (2026-10-08): `edit::GraphicsDocument` over a GFX file (the
  clean ROM's until saved into the project) or a PNG ExGFX file, with pencil strokes,
  fills within a tile, undo, and `Workspace::set_graphics`; `edit::graphics::level_files`
  for the files a level loads (editor.md, "Graphics"); importing and exporting a file as
  an indexed PNG followed. Drawing in `.bin` ExGFX is not done: a file the project keeps
  as bytes has no colours to draw in.
- The shared palettes (2026-10-08): `source::palettes` (the game's 21 colour tables as
  a project changes them, `[palettes] shared`), written by the graphics stage, carried by
  ROM and Callisto imports, checked against Lunar Magic's shared palette export and
  import; `edit::PalettesDocument` and the editor's window (editor.md, "Shared palettes").
- A level's ExAnimation in the inspector (2026-10-08): `Edit::SetAnimation`, refusing a
  list a build would refuse, and `exanimation::Slot::refit_frames` keeping a slot's
  frames as its type, trigger, and count change; `edit::GlobalAnimation` and a window
  for the project's global list. The level's picture shows the frame the load leaves.
- Layer 3 tilemaps (2026-10-08): `edit::layer3` (where LT3's file goes, giving a level
  one with `T` set, `TilemapDocument` over the `.bin` file) and the editor's layer 3
  window (editor.md, "Layer 3"). Layer 3's own graphics files are drawn in through the
  graphics window; the game's stripe images (tides, the castle windows) are not
  editable, as they are the game's code's.
