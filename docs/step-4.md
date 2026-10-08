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
   "The level list by overworld level").
7. **The emulator**: Play from here already opens one; debugging (Mesen 2,
   bsnes-plus) through `kobo_core::clean_room`.

## Done

- The Map16 window (2026-10-08): `edit::Map16Document` over the page, game page, and
  tileset files, its changes as `TileChange`s for a tileset, undo by snapshot, saving
  new files into the manifest, and `Workspace::set_map16` for builds of unsaved edits;
  the editor's window, with the level's own tiles to pick 8x8 tiles from (editor.md,
  "Map16"). Background Map16 and the pipes file are not edited yet.
- The graphics window (2026-10-08): `edit::GraphicsDocument` over a GFX file (the
  clean ROM's until saved into the project) or a PNG ExGFX file, with pencil strokes,
  fills within a tile, undo, and `Workspace::set_graphics`; `edit::graphics::level_files`
  for the files a level loads (editor.md, "Graphics"). Importing a PNG or `.bin` into a
  slot, and drawing in `.bin` ExGFX, are not done yet.
- The shared palettes (2026-10-08): `source::palettes` (the game's 21 colour tables as
  a project changes them, `[palettes] shared`), written by the graphics stage, carried by
  ROM and Callisto imports, checked against Lunar Magic's shared palette export and
  import; `edit::PalettesDocument` and the editor's window (editor.md, "Shared palettes").
- A level's ExAnimation in the inspector (2026-10-08): `Edit::SetAnimation`, refusing a
  list a build would refuse, and `exanimation::Slot::refit_frames` keeping a slot's
  frames as its type, trigger, and count change. The global list is still edited in its
  file, and the level's picture shows the frame the load leaves.
