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
  Play, and Build.
- **Left**: three tabs.
  - *Levels*: the project's levels as a player meets them: each overworld level (the
    levels the overworld enters, `000`-`024` and `101`-`13B`, by their names there),
    with the sublevels its screen exits lead to folded under it, then the levels no exit
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
  palette of its own starts from the game's colours for it), its ExAnimation (shown in the
  file), the secondary entrances into it, copying it, starting an empty level, or taking
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
  too, since the game's loader needs the list in screen order (docs/smw.md).
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
about. *Build and play* opens the ROM with what the system opens it with.

**Play from here** (the canvas's menu, with the power-up to start with, or F5 where the
mouse is; the command palette plays from the level's start) builds the project as the
editor has it into `play.sfc` beside the project's build and opens it. While the canvas's
menu is open, a PLAY marker shows where the player will start: standing where it was
opened, or, opened on the ground or a wall, on top of it. The game goes from power-on
straight to that tile, with no title screen (a moment of dark "Nintendo Presents", whose
game modes set the screen up), through a secondary entrance put there in a copy of the
level, which is water or slippery when the level is; it plays the level's own music, and
comes back there after a death, a game over (with four lives again), or the level's end;
the project is not changed. The pixelated fade on the way in is the game's own, as from
the overworld. `kobo play` does the same from the command line.

A level that does not build does not stop the others from drawing: the picture's build
leaves it out, as the game's own, and says so above the canvas.

## Running without a display

`--screenshot out.png` saves the window once the level is drawn and quits, with `--tab`
(`levels`, `objects`, `add`, `sprites`, `map16`, `changes`, `overview`, `backgrounds`),
`--select`, and
`--build` to set it up; under `xvfb-run -a` it needs no display. docs/step-3.md has what
the development server needs for it.
