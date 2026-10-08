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

- `eframe` with its OpenGL renderer (`glow`), one window, panels inside it. eframe 0.36
  defaults to wgpu; Kobo turns that off, since OpenGL needs no Vulkan or EGL and runs
  under Mesa's GLX on the development server (reviewed 2026-10-08). Either works on the
  three platforms; wgpu is a feature flag away if a driver gives trouble. egui
  is immediate mode: the whole interface is redrawn from the editor's state every frame,
  so there is no second copy of that state to keep in step. Its licence is MIT or
  Apache-2.0; it builds from Rust alone on all three platforms, and a level picture is a
  texture it draws like any other image.
- On Linux the window draws without vsync and the editor keeps its frames 1/60 s apart
  itself (`Startup::pace`). With vsync, Mesa's EGL on Wayland waits for the
  compositor's frame callback before a swap returns, with no time limit, and a
  compositor sends none to a window that is covered; winit reports no occlusion on
  Wayland, so eframe draws anyway. A frame drawn while the emulator Play opens covers
  the editor (the pointer leaving is enough) then held the window's thread until the
  editor was uncovered, and the desktop called it not responding (2026-10-08). A
  compositor shows each frame whole, so nothing tears without vsync there. The
  maintainer confirmed the fix on their desktop (2026-10-08).
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

### The level list by overworld level

- Reviewed 2026-10-08. The list groups each sublevel under the first overworld level (by
  number) whose screen exits reach it, directly or through other sublevels
  (`edit::reach`); an overworld level is one with a translevel (`000`-`024`,
  `101`-`13B`), whether or not the overworld has a tile for it. Levels only the game's
  code reaches (the credits' rooms, the bonus game) are listed apart as "not reached by an
  exit". Grouped is the default; *By number* lists the project's levels flat, in the
  ROM's order.
- "Level" names every one of the 512, as Lunar Magic does, though the community says
  "sublevel" for those under another; the grouping shows which is which.
- In step 4, once the overworld is a project's, the overworld's own tiles decide which
  levels are overworld levels, so that a hack that leaves translevels unused groups by
  what the player can enter.
- Levels are named as the overworld names them (`level::level_name`), from the clean
  ROM, whose overworld a build keeps while Kobo carries none of a project's: for a
  project imported from a hack, the game's names, not the hack's, though true of what
  the build makes. In step 4 the project's overworld names them.
- A level whose objects are those of the layer 1 data most of the clean ROM's levels
  point at (the "TEST" level at `$068000`, 277 of the 512) is left out until asked for
  (`reach::Placeholder`), whether or not the project lists it; a level with any object of
  its own shows. A real level the project never changed shows like any other. In step 4,
  with the overworld, what is hidden becomes what nothing reaches, from the overworld or
  from any level, rather than what holds the placeholder.

### Play from here

- Reviewed 2026-10-08. Play from here comes ahead of step 4, as a build
  (`kobo_core::playtest`, `kobo play`) opened with the emulator the system opens a ROM
  with; no emulator is driven, which stays step 4's (Mesen-S, bsnes-plus).
- The play ROM goes in the user's cache (`playtest::rom_path`: `kobo/play`, named after
  the project's folder and a hash of its path), not the project's folder; `kobo play`
  writes where `-o` says.
- It is for testing a level: power-on goes straight in (the title screen's load is
  hooked, game mode `$03`; "Nintendo Presents" runs a frame, dark, to set the screen up
  and decompress the player's graphics), and a death, a game over, or the level's end
  goes back into it rather than to the overworld, through the overworld's load once it
  has cleared the level's RAM (item memory among it, so collected coins come back; until
  2026-10-08 the hook came before that, and a coin collected on one life was gone, with
  its whole column, on the next). The title screen's hook uploads the level music
  bank, which an entry by screen exit never does, and sets the translevel (`$13BF`) the
  overworld would have entered by (a sublevel's is its overworld level's, `edit::reach`),
  which midway points and patches keep their state by.
- From a tile the play build adds a secondary entrance there; from the level's start it
  takes a screen exit to the level itself, so the main entrance is the level's own, its
  action too (until 2026-10-08 it put an entrance at the tile the main entrance's load
  left the player on, which for a level that starts out of a pipe, `0E9`, was inside
  it). How the game starts is the play settings' (`playtest::Settings`, the menu beside
  Play): the power-up, the switch palaces (`$1F27`-`$1F2A`), and the ON/OFF switch
  (`$14AF`, set again on each entry). Decided 2026-10-08 at the maintainer's request;
  more can join them (Yoshi, the item box) the same way.
- With kkevinm's Retry System in the project's UberASM Tool folder (its
  `retry_config/ram.asm`), the play build sets its respawn point to the entrance, as an
  entry from the overworld would have set it to the level's start: without that it was
  0, so a retry went to level `000`, and past a midway point to translevel 0's (the
  Romhack Races baserom's `13B` went to `0C5`). Kobo knows this one community resource, as
  it knows Callisto (maintainer, 2026-10-08).
- A play starts with four lives and the power-up chosen in the menu (F5 the last one).
- A click on the ground or a wall starts the player on its top: the first tile of page 1
  (`100`-`1FF`, by what it acts like) above the click with two free tiles over it, within
  16 rows; a click on an empty tile starts there. Checked in Mesen 2 on vanilla, an
  AddmusicK hack, and the SA-1 reference.

### The Objects tab along the level

- Reviewed 2026-10-08. The outline lists objects and sprites together by place by
  default (by column, a vertical level by row from where the player starts), under a
  heading per screen, since finding what is where is the common case. *Drawing order*,
  the level file's order, which Ctrl+[ and Ctrl+] change, is the other choice, for
  objects that overlap. Screen exits sort at their screen's start; what has no place is
  last.

### Screen counts are set by hand

- Reviewed 2026-10-08. The editor never sets a level's screen count by itself, as Lunar
  Magic does when it saves with "Auto-Set Number of Screens" on: the level file is what
  builds, and an edit changes only what it says. The flag is shown as what it is ("Lunar
  Magic sets screens"), and *Fit* beside the count sets it from the objects and sprites
  when the user asks (`edit::screens_used`), as one undo step.

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

Done (2026-10-05), on the `step-3-editor` branch:

1. `kobo_core::edit`: `LevelDocument`, `Edit`, undo (`amend` for a dragged value), comments
   that follow their entry, reloading from disk, the line of an entry, settings fields,
   sprites kept in screen order, exit targets, `copy_of`.
2. `expand::ObjectMap`, through `cpu::watch`: every vanilla level's drawn tiles owned.
3. `edit::Workspace`: preview, adding a level (`import::add_level`), `clean_level`;
   `edit::object_previews` for the palette's pictures.
4. `crates/kobo-editor`: the window, the level list (the game's own levels too), the canvas
   with pan, zoom, screens, grid, and sprites drawn, as IDs, or off.
5. Selection (click, shift-click, marquee), moving (drag, arrow keys), resizing by a
   handle, delete, reorder, the inspector (objects, sprites, screen exits, the header, the
   main entrance, level settings, secondary entrances), undo, save.
6. Live reload with a conflict prompt, and the source pane.
7. The palette (objects, extended objects, sprites) with pictures; placing; screen exits
   from the canvas's menu; copying a level.
8. Build to `build.sfc`, diagnostics, unsaved-edit prompt on close.

Since (2026-10-05 and 06, on master):

- Layer 2's objects selected and placed where the picture shows them; secondary entrances
  added (`Workspace::free_entrance`) and removed.
- The outline (every entry in drawing order, with find), the clipboard (copy, cut, paste
  at the mouse, duplicate, across levels), and reordering by key and menu.
- The start screen: the clean ROM chosen and recorded (`config::set_vanilla_rom`), recent
  projects, new projects empty, from a hack (`import::read_hack`), or from a baserom
  template; the Project, Edit, and View menus.
- The build window (`build::build_reporting`): stages as they run, a BPS patch, errors that
  open their level.
- Lunar Magic's level settings: the size (`Edit::SetSize`), the background, spawning, layer
  2's vertical scroll, the midway screen and a midway entrance of its own.
- Entrance markers (`expand::secondary_entry`), the minimap, and a level opening where the
  player starts.
- Changes since the last commit (`edit::diff`): listed, marked on the canvas, each taken
  back on its own.
- The command palette (Ctrl+K) and the shortcuts (F1).
- Pictures in the palette for sprites too (`edit::sprite_previews`), and the overview of
  every level as a picture card.
- Marks for sprites the capture gave up on; hex fields for extension bytes and Lunar Magic's
  and unplaced objects' data.
- Screen exits dragged to another screen; entrances dragged, the midway and those placed by
  tile (`entrance::tile_place`) too; layers 1, 2, and 3 shown or hidden
  (`RenderOptions::hidden_layers`).
- The sprite header (`Edit::SetSpriteSettings`), and layer 2's background chosen from
  pictures of every one of the game's (`level::game_backgrounds`, named in `names.toml`,
  `edit::background_preview`, `Edit::SetLayer2`).
- A level's graphics list slots and its palette's colours (`Edit::SetGraphics`,
  `Edit::SetPalette`, `palette::game_palette` to start from).
- Following a screen exit to where it leads (`Workspace::entrance_level`), back and forward
  between levels, and finding objects and sprites in every level (`edit::find`).
- Play from here (`kobo_core::playtest`, `asm/playtest.asm`, `kobo play`): checked in
  Mesen 2 on vanilla, a Lunar Magic hack, and an SA-1 one, from power-on into the
  level at the tile, and back there after a death and a game over; and on the Romhack
  Races template (2026-10-08), whose Retry System retries at the tile, and at the
  midway entrance once its midway point is touched.
- Names from the game's tables where it has them: levels by their overworld names
  (`level::level_name`), entrance actions, layer 2 scroll and layer 3 settings, palette
  settings' colours; what a tile acts like in the status bar; the project window; menus on
  the level list and the overview's cards.
- Every entrance's position by the game's places or by tile, and its camera at the game's
  positions or rows from the player (`entrance::Camera`).

Next:

- Positions in tiles past a vertical level's edge.
- ExAnimation, and the graphics and palette editors themselves (the tiles and colours a
  project's files hold), which are step 4's.

## Running it

- `cargo run -p kobo-editor -- path/to/project --level 105` opens a level; with no
  folder it asks for one. It finds the clean ROM as the CLI does.
- `--screenshot out.png` saves the window once the level is drawn and quits, which with
  `xvfb-run -a` shows the window on a machine without a display. The development server
  has Mesa's GLX but not `libxkbcommon-x11`, which winit loads for X11: unpack
  `libxkbcommon-x11-0` and `libxcb-xkb1` from Debian's packages
  (`apt-get download`, `dpkg-deb -x`) and point `LD_LIBRARY_PATH` at them, with a
  `libxkbcommon-x11.so` link to the `.so.0`.
- `cargo test -p kobo-editor` drives the window without any display or GPU.
