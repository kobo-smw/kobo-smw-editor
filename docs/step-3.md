# Step 3: the level editor

The plan for roadmap step 3 and the decisions it rests on, settled 2026-10-05. The rules
that follow from them are in [AGENTS.md](../AGENTS.md); this file has the reasoning and the
work order. Fold what stays true into the other docs as step 3 lands, and delete this file
when it is done.

## Goal

A desktop editor, `kobo-editor`, that opens a project and edits its levels: select, move,
resize, add, and delete objects and sprites, edit a level's header and entrances, undo,
save, and build. What it shows is what the game shows, because every picture comes from
the ROM's own loader on a build of the project. A user should rarely need to open a level
file by hand; when they do, the editor follows.

## Decisions

### Toolkit: egui

- `eframe` with its default OpenGL renderer (`glow`), one window, panels inside it. egui
  is immediate mode: the whole interface is redrawn from the editor's state every frame,
  so there is no second copy of that state to keep in step. Its licence is MIT or
  Apache-2.0; it builds from Rust alone on all three platforms, and a level picture is a
  texture it draws like any other image.
- `rfd` for file dialogs, through the XDG portal on Linux (no GTK to build against), and
  `notify` for watching the project's files. Both are MPL-compatible (MIT, CC0).
- `egui_kittest` tests the interface without a display, through AccessKit.
- Decided 2026-10-05; the open decision in AGENTS.md is closed.

### Crates

- `crates/kobo-editor` is the editor, a thin shell like `kobo-cli`: it holds what is on
  screen (the open level, the selection, the camera, the panels) and nothing a script
  would need. Every change to a project is a call into `kobo_core::edit`.
- The CLI does not depend on the editor, and the core depends on neither.

### The document is the level file

- `kobo_core::edit::LevelDocument` is one open level: its file, the `Level` and the user's
  `Comments` as `Level::from_toml` gives them, and the text last read or written. An edit
  changes the `Level`, moves the comments that stood before an entry with it, and saving
  writes `Level::to_toml`, so a file the editor saves is in Kobo's format, as `kobo fmt`
  would leave it.
- Edits are values (`edit::Edit`): insert, remove, move, and replace an object or a
  sprite, reorder an object, set the header. The scripting API will apply the same ones.
  Each is checked before it is applied (a position the level's object codec cannot
  encode is refused), so the document is never left half changed.
- Undo keeps whole snapshots of the level and its comments, one per edit, labelled with
  the edit. A level is small (a few KB), so snapshots are simpler and safer than inverse
  edits; a drag is one undo step.

### Outside edits: live reload

- The editor watches the project's files. When a level file changes on disk and the
  editor has no unsaved edits to it, it reads the file again and redraws; with unsaved
  edits it asks, keeping both until the user chooses. An outside edit is an undo step
  like any other.
- A file that does not parse leaves the last good level on screen with the error shown,
  and the editor follows again once the file parses.
- The source pane shows the level file as it would be saved, with the selected entry's
  line marked. It can be edited; the text is parsed as it is typed and applied when it
  parses. It is there for the few things the editor has no control for yet, and for
  seeing what the file looks like; the editor's own controls are the main way to edit.

### Pictures come from a build

- A change rebuilds the project in memory (`edit::Workspace`) and renders the level from
  that ROM with `render::render_level_with_control`. The stages before `Levels` come from
  the stage cache, so a rebuild is the levels stage alone: about 0.2 s for the whole
  vanilla project on the development machine, and a level renders in 0.3 s without
  sprites and 0.5 s with them (level `105`).
- Rendering runs on a worker thread under an `Operation`. A newer edit cancels the render
  in flight. While a render is pending, the old picture stays on screen and an edit in
  progress (a drag) is drawn as an outline over it.

### Which object drew a tile

- Selecting by clicking needs to know which object a tile belongs to, and only the ROM's
  loader knows what an object draws (slopes, pipes, custom objects). So the loader is
  watched: the bus notes which object's bytes it read last, and each tile written to the
  grid after that belongs to that object (`expand::ObjectMap`, in `LoadedLevel`). This
  uses only the addresses of data reads and writes, the memory effects the clean room
  allows, and works the same for any object, vanilla, Lunar Magic's, or a patch's.
- A tile's owner is the last object that wrote it, which is the one that shows. An
  object's footprint is every tile it wrote, for its outline. An object that writes no
  tile (a screen exit, some settings objects) is selected from the object list.
- Sprites are placed by their entry's tile; the objects the sprite capture drew for an
  entry (`CapturedSprite`) give its outline.

### Clean room in the editor

- The editor shows no CPU trace, read trace, RAM view, or routine address. A debugging
  panel added later goes through `kobo_core::clean_room` like every other output.

### Scope

- In step 3: open a project, the level list, the canvas (pan, zoom, layer and grid
  toggles, screen boundaries), select and move objects and sprites, resize objects, add
  them from a palette, delete, reorder, the inspector, the level header and entrances,
  undo and redo, save, live reload, the source pane, build, and diagnostics.
- Later: the visual diff against git, level thumbnails, the Map16, palette, and graphics
  editors, the overworld, and play-from-here (step 4).

## Work order

1. `kobo_core::edit`: `LevelDocument`, `Edit`, undo, comments that follow their entry,
   reloading from disk. Unit tests.
2. `expand::ObjectMap`: the owner of every grid tile, and each object's footprint.
   ROM-backed tests on vanilla levels.
3. `edit::Workspace`: the project in memory, rebuilt and rendered for a level.
4. `crates/kobo-editor`: the window, the level list, the canvas with a level picture,
   pan and zoom.
5. Selection and the inspector; moving, resizing, deleting, undo, save.
6. Live reload and the source pane.
7. The palette for adding objects and sprites; the header and entrances.
8. Build from the editor, diagnostics, and CI on all three platforms.
