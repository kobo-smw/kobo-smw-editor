# The level editor

`kobo-editor` edits a project's levels. Everything it shows comes from a build of the
project, drawn by the ROM's own loader, so a level looks as it will in the game, custom
objects, ExGFX, and patches included. Every change goes through `kobo_core::edit`, the
same operations a script makes, and is written to the level's TOML file when saved.

```
cargo run --release -p kobo-editor -- [project] [--level 105]
```

## Starting

With no project, the start screen opens. It needs the clean Super Mario World (USA) ROM
first (`Choose the ROM…`, checked by SHA-1 and recorded in Kobo's config file, as `kobo`
uses it). Then it opens a project folder or a recent one, or makes a new project:

- **Empty**: a `kobo.toml` and nothing else; every level builds as the game has it until it
  is added.
- **From a hack**: a ROM or a BPS patch of the clean ROM, imported as `kobo import` does.
  A window then says what came and what did not: the hack's own code and patches, which a
  project carries only as their sources, and each level's notes. A hack its author locked
  (which Lunar Magic will not open either) is imported only once the user agrees, since
  its graphics and code stay behind and its levels will not look or play as the hack's.
- **From a baserom**: one of `kobo new`'s templates, downloaded from its authors' release.

## The window

- **Top bar**: the Project, Edit, View, and Git menus; undo and redo (⟲ ⟳, each naming
  the step it takes); back and forward between levels; Commands (Ctrl+K); Save (💾),
  Play, and Build. *Quit* (Project menu, Ctrl+Q, or the start screen's top bar) closes
  the editor as the window's own close does, asking first to save any unsaved edits.
- **Left**: three tabs.
  - *Levels*: the project's levels as a player meets them: each overworld level (the
    levels the overworld enters, `000`-`024` and `101`-`13B`, by their names there),
    with the sublevels its screen exits lead to folded under it, set in (choosing a
    level opens its group, and choosing it again folds it; a group, like the lists the
    headings below fold, slides open and shut), then the levels no exit
    reaches (the credits' rooms, the bonus game), then the game's own levels the project
    does not have, which build as the game has them and are added when chosen;
    *By number* lists the project's levels flat instead, in the ROM's order. The
    game's unused level numbers, which all hold its "TEST" level, are left out unless
    asked for. Each row has a small picture of the level (hovering shows it larger), its
    number and name (or tileset), and its screens, wide (↔) or tall (↕). The search finds
    levels by number, name, or tileset and, below them, every object and sprite of every
    level it names (Ctrl+Shift+F; `goomba`, `sprite 0F`, `extended 41`, `exit`); choosing
    one opens its level with it selected. A level's menu plays it from its start or takes
    it out of the project; *All levels as pictures* (View menu) shows every level as a
    card.
  - *Objects*: every object and sprite of the level, along the level as the player meets
    them, screen by screen (a vertical level from where the player starts), or in drawing
    order (later ones draw over earlier ones); all of them, or objects or sprites alone;
    found by name, number, or place. It reaches what the canvas cannot click: screen
    exits, Lunar Magic's settings objects, objects behind others.
  - *Add*: the palette. Objects, extended objects, and sprites, each drawn as the open level
    draws it (its tileset, sprite set, graphics, and palette); and Map16, a page at a time,
    for placing a tile directly. Choosing one places it at each click on the canvas, until
    Escape or a right click. In a level with layer 2 objects, the palette places on either.
- **Git menu**: *Changes since the last commit* opens a window of what differs from the
  level's file in git's last commit, each change taken back on its own; the canvas marks
  them while it is open (green added, amber changed with where it was, red removed).
  *Back to the last commit* puts the level as that commit has it, as one step undo takes
  back. The canvas's menu finds every object like what is selected in every level (the
  level list's search), or selects every one like it in the level.
- **Canvas**: the level, with screen boundaries, the grid (G), markers where the player
  enters (the start, the midway, each secondary entrance), and a minimap below. A mouse's
  wheel scrolls along the level (sideways, or up and down in a vertical level; with Shift
  the other way), Ctrl and the wheel zoom, and a trackpad pans freely. The status
  bar names the tile under the mouse: its place, screen, Map16 number, what it acts like
  when that is another tile, and the object that drew it.
  The view bar above it (screens ▥, grid ▦, entrances ⚑, layers 1 2 3, sprites drawn 🐢,
  as numbers, or hidden, the player 🏃, and zoom), and the View menu, show or hide
  layers 1, 2, and 3, the sprites, and the player; a hidden layer is left out as the PPU leaves out a layer neither screen has, so
  what is behind it shows.
- **Right**: the inspector. What is selected (with its picture, cut from the level's), or
  with nothing selected the level: its
  header (*Fit* sets the screens to as many as its objects and sprites reach; each palette
  setting shows its colours), the sprite settings, the background (a click shows every one of the game's as
  the level would draw it, to choose from), the main entrance, Lunar Magic's settings
  (size, background, spawning, the midway entrance), its graphics list (each slot's file,
  and whether they replace the tilesets'), its palette (a click on a colour changes it; a
  palette of its own starts from the game's colours for it), its ExAnimation (which of
  the game's and Lunar Magic's animations run, and its list's slots: type, trigger,
  frames, where they go, and the frames' words, with why a build would refuse the list;
  *The global list* opens the project's global list in a window of its own),
  the secondary entrances into it, copying it, starting an empty level, or taking
  it out of the project; and diagnostics.
- **Source** (top bar): the level's file beside the canvas, the selection's line marked.
  Typing in it applies as soon as the text reads.

## Editing on the canvas

- **Select**: click the tile an object drew (the editor knows which object drew each
  tile, from watching the loader), or a sprite. Shift adds; a drag on empty space selects
  what a box meets; Ctrl+A everything, Escape nothing; Tab and Shift+Tab the next and
  the one before in drawing order, which reaches what draws behind something else. F centres
  the view on the selection.
- **Move**: drag (with Ctrl held, copy to where it is dropped), or the arrow keys (Shift:
  16 tiles). The moved picture shows at once and
  the level is drawn again behind it. A sprite moved to another screen moves in the list
  too, since the game's loader needs the list in screen order (docs/smw.md). Nothing is
  moved or placed outside the level; an object or sprite a file already has past its
  edge (vanilla `108` has three ledges below its two screens), which the game loads but
  the level never shows, can be edited and moved back in, never further out. The
  inspector's diagnostics count them, and *Select them* selects them.
- **Resize**: the handle at a selected object's bottom right, for objects with a width,
  height, or length, and direct Map16 tiles.
- **Order**: Ctrl+[ and Ctrl+] send an object back and bring it forward in its list; with
  Shift, to the back and the front.
- **Clipboard**: Ctrl+C, Ctrl+X, Ctrl+V (where the mouse is), Ctrl+D (duplicate), within
  a level or across levels.
- **Screen exits**: each has a label at the top of its screen; drag it to another screen,
  or double-click it (or *Go to level* in the inspector) to open where it leads, the
  entrance it comes in by in view. The canvas's menu adds one to a screen without.
- **Entrances**: drag the start, the midway, or a secondary entrance; it snaps to where its
  settings can put the player (the game's table of places, or with Lunar Magic's position
  method 2 any tile). The game's own midway entrance moves from screen to screen. The
  inspector sets how each places the player (the game's places or by tile) and the camera
  (the game's positions, or rows from the player).
- **Right click**: the menu for what is under the mouse.
- **Change what it is**: the inspector's object or sprite button lists every one by name;
  typing finds one. The place and size stay.

The arrows beside the project's name (Alt+Left and Alt+Right, or the mouse's back and
forward buttons) go back to the level shown before and forward again.

Undo (Ctrl+Z) and redo (Ctrl+Shift+Z) work per level; the Edit menu undoes back several
steps at once, and puts the level back as its file has it (one step, which undo takes
back). Ctrl+S saves every changed level.

## Map16

The Map16 window (View menu; a right click on a tile in the palette's Map16 page, or
*Edit Map16 tile* in the canvas's menu) edits the project's Map16 tiles as the open level
shows them: its object tileset picks which definition of a tile the game keeps per
tileset is meant, and its graphics and palette draw them. *Background* shows the tiles
of the level's BG Map16 table instead (its own background's table, else the game's),
which have no acts-like setting. The vertical pipes' tiles (`133`-`13A`) have four
colour sets, chosen by the screen they stand on: set 1 is page 1's own, and sets 0, 2,
and 3 are chosen beside the tile and kept in the pipes file. In object tilesets 0 and 7
the diagonal pipes' tiles (`1C4`-`1C7`, `1EC`-`1EF`) are the game's diagonal pipe table's,
which changes go to. A click chooses a tile, a
double click (or *Place it*) places it on the level. The tile's four quarters are
chosen on its large picture; each has its 8x8 tile (typed, or clicked among the level's
1024, drawn in the quarter's palette), its palette row, its flips, and its priority. *Acts
like* is the tile whose behaviour it has. A dot marks each tile the project changes, and
*Back to the game's* puts one back (past page 1, empties it). The window says which
object tilesets show the same definition, since a change shows in all of them.

Changes go where the page files' rules put them (docs/build.md, "Source formats"): what
pages 0 and 1 change of the game's tiles in their page files, or in a tileset file for a
tile the game keeps per tileset (that of the tileset sharing its table that already has
one); page 2's graphics in the tileset file when page 2 is per tileset; and the whole
tile in its page's file past that. A file Kobo makes for this is named as import names
it and added to `kobo.toml` when saved. Setting a tile back as the clean ROM has it takes
it out of its file. The level is drawn again from a build with every change, saved or
not; Ctrl+S saves the Map16 with the levels, and undo takes back Map16 changes while they
are the last made. The Map16 files follow changes on disk while the window has no
unsaved edits.

## Graphics

The graphics window (View menu, or *Draw in the level's files* in the inspector's
graphics section) lists the files the open level loads, by slot (FG1 to FG3, BG1 to
BG3, SP1 to SP4, layer 3's LG1 to LG4, and AN2): its graphics list's when the list
replaces the tilesets' files, else its tilesets'. Any of the game's files opens by
number too. A file is drawn 16 tiles to a row in a row of the level's palette (sprites'
files start in row 9, the rest in row 2; a 2bpp file in a group of four of the first 32
colours), colour 0 left clear. The pencil draws with the left button and picks a colour
with the right; *Fill* fills an area of one colour within its 8x8 tile. A stroke is one
undo step, and the level is built and drawn again when it ends.

A file the project does not have is the clean ROM's as a build stores it (16 colours
where the project uses Lunar Magic's graphics formats), and saving adds it as
`graphics/GFXnn.png`. An ExGFX file the project holds as a PNG is drawn in the same way;
one held as `.bin` bytes is not, having no colours to draw in. *Export* saves the file as
an indexed PNG in the colours shown, to draw in another program; *Import* draws the file
from such a PNG (its size, 16 tiles to a row, within its colours) as one undo step.

## Layer 3

The layer 3 window (View menu) draws on the open level's layer 3 tilemap: the ExGFX file
its graphics list's LT3 slot loads as layer 3's tilemap (Lunar Magic's "T"). It shows
layer 3's tilemap, 64 tiles wide, in 32x32 screens, drawn with the level's own layer 3
tiles and colours, the rows the file does not reach or that stay under the status bar
dimmed. Drawing sets a cell to the tile chosen among the level's layer 3 tiles, with a
palette (0 to 7, groups of four of the first 32 colours), flips, and priority; a right
click on the map takes a cell's word. A level without a tilemap is given one: the first
free ExGFX number, a `$1000`-byte `.bin` file of blank tiles (`$38FC`, as the game's own
layer 3 is) placed under the status bar, with the list's `T` set and nothing else of
it changed (one undo step of the level). Saving writes the file and names it in
`kobo.toml`'s `[exgfx]`.

## Shared palettes

The shared palettes window (View menu) edits the game's colour tables, which every level
without a palette of its own draws from by its header's BG, FG, sprite, and back area
settings: each table's colours (by palette and row for the tables made of palettes),
the ones the open level's header picks outlined, a dot on each the project changes. A
chosen colour is set by its red, green, and blue (0 to 31 each), or put back as the
game has it. Saving writes `palettes/shared.toml` (docs/build.md, "Source formats"),
named in `kobo.toml` the first time.

## The overworld

The overworld window (View menu) shows the project's overworld as a player on the
chosen map sees it: the main map, or the submaps' map in one submap's graphics and
colours, drawn from a build of the project as it is in memory, as the ROM's own load
puts it up (`render::render_overworld`), dimmed while a newer one is being drawn.
Dragging draws layer 1's 16x16 tiles with the brush (a tile number, its page in the
high digit); a right click takes the tile under it as the brush and chooses its level
tile, whose level and name show below and whose name is edited there (19 tiles at most,
`\xNN` for a tile that is not a letter; Enter sets it). Each change is one undo step, a
stroke one in all. Saving writes `overworld.toml` (docs/build.md, "Source formats"),
named in `kobo.toml`'s `[overworld]` the first time; a project with an overworld file
builds it in Lunar Magic's layout (lunar-magic-install.md, "The overworld").

## The project

*This project…* (Project menu) shows what the project holds (its levels, Map16, graphics,
patches, and tools' folders), whether its build installs Kobo's code for Lunar Magic's
layout, and where each tool the build runs comes from: Kobo's pinned build, or a copy the
user set, which makes the build depend on that copy. `kobo.toml` is where these are set.

## Outside the editor

The editor watches the project's folder. A level file changed elsewhere (another editor,
`git checkout`, a script) is read again when the editor has no unsaved edits to it; with
edits, it asks which to keep. *Open the level file elsewhere* (Project menu) opens it in
the system's editor for it.

## Building

The Build window (Ctrl+B) builds the project as the editor has it, unsaved edits
included, into `build.sfc` and, with the box ticked, `build.bps`. It shows each stage as
it runs or comes from the stage cache. A failed build says why, and opens the level it was
about. *Build and play* opens the ROM in the emulator the config file names
(`[play] emulator`, or `KOBO_EMULATOR`), else with what the system opens it with. Beside the ROM
it writes `build.sym`, the labels of Kobo's code (`kobo_` and its patch, as
`kobo_graphics_load_graphics`) and of the project's patches, as a WLA-DX symbol file:
bsnes-plus and Mesen 2 load it beside a ROM of the same name (Mesen 2's default), so their
debuggers name that code. Tools that run as programs of their own (PIXI, GPS, UberASM
Tool, AddmusicK) give no labels, and nothing of Lunar Magic's is named.

**Play from here** (the canvas's menu, with the power-up to start with, or F5 where the
mouse is; the command palette plays from the level's start) builds the project as the
editor has it into a ROM of its own in the user's cache (`kobo/play`, one per project,
so the project's folder stays clean and an emulator's saves stay with it) and opens it,
in the configured emulator if there is one. While the canvas's
menu is open, a PLAY marker shows where the player will start: standing where it was
opened, or, opened on the ground or a wall, on top of it. The game goes from power-on
straight to that tile, with no title screen (a moment of dark "Nintendo Presents", whose
game modes set the screen up), through a secondary entrance put there in a copy of the
level, which is water or slippery when the level is; from the level's start (Play,
the level list's menu) it goes in by the level's own main entrance, out of a pipe if
that is how the level starts. It plays the level's own music, and comes back there
after a death, a game over (with four lives again), or the level's end, with the
level as it was at first (coins collected are back); the project is not changed. The
**▾** beside Play sets how the game starts, for every way of playing: the power-up
(which the menus' choice of one also sets), the switch palaces pressed, and the ON/OFF
switch. The play ROM has its `.sym` beside it too. A hack with kkevinm's Retry System (the Romhack Races
baserom's, and many others') retries there too, or at a midway point once the player has
touched one. The pixelated fade on the way in is the game's own, as from
the overworld. `kobo play` does the same from the command line.

A level that does not build does not stop the others from drawing: the picture's build
leaves it out, as the game's own, and says so above the canvas.

## Running without a display

`--screenshot out.png` saves the window once the level is drawn and quits, with `--tab`
(`levels`, `objects`, `add`, `sprites`, `map16`, `changes`, `overview`, `backgrounds`,
`map16-editor`, `graphics`, `palettes`, `layer3`, `overworld`),
`--select`, and `--build` to set it up; under `xvfb-run -a` it needs no display. The
development server has Mesa's GLX but not `libxkbcommon-x11`, which winit loads for X11:
unpack `libxkbcommon-x11-0` and `libxcb-xkb1` from Debian's packages (`apt-get
download`, `dpkg-deb -x`) and point `LD_LIBRARY_PATH` at them, with a
`libxkbcommon-x11.so` link to the `.so.0` (they are in `~/.local/share/kobo/xlibs` there).
`cargo test -p kobo-editor` drives the window without any display or GPU.

## How it is made

The editor was roadmap step 3, planned and settled on 2026-10-05 and finished on
2026-10-08. These are the decisions it rests on; the rules that follow from them are in
AGENTS.md.


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
- Decided 2026-10-05.

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
- An overworld level's name is its translevel's: the project's own overworld's when it
  has one (the overworld window renames levels), else the clean ROM's.
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
