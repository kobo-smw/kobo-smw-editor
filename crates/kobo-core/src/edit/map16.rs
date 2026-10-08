//! The project's foreground Map16 tiles, open for editing.
//!
//! A tile is edited as the level that shows it sees it: its four 8x8
//! tiles and what it acts like, for the level's object tileset. Where that
//! is written follows the page files' rules (`source::map16`):
//!
//! - Pages 0 and 1, the game's: what a tile acts like goes in the page's
//!   file; its graphics too, unless the game keeps the tile per object
//!   tileset, when they go in the file of the tileset whose file already
//!   lists that table's tiles, else the level's tileset's. A setting equal
//!   to the clean ROM's is left out, and a tile with nothing left is too.
//! - Page 2 when it is per tileset: what it acts like in page 2's file, its
//!   graphics in the level's tileset's file.
//! - Every other page: the page's file, an empty tile left out.
//!
//! A page or tileset with no file gets one, `map16/02.toml` or
//! `map16/tileset-3.toml` as import names them, added to the manifest on
//! saving. Undo keeps whole snapshots of the files, as a level's does.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::build::Project;
use crate::map16::{
    GameTables, Map16Tile, PIPE_COLOUR_TILES, TILESET_COUNT, diagonal_pipe_tiles, pipe_address,
    sharing_group, vanilla_map16,
};
use crate::rom::Rom;
use crate::source::map16::{
    DEFAULT_ACTS, GAME_PAGES, GamePage, Map16Entry, Map16Page, PAGES, PIPE_SETS, PageComments,
    PageKind, Pipes,
};
use crate::source::project::{MANIFEST, Manifest};

#[derive(Debug, Error)]
pub enum Map16EditError {
    #[error("tile {0:X} is past Map16 page 7F")]
    NoTile(u16),
    #[error("there is no object tileset {0:X}")]
    NoTileset(u8),
    #[error("{0}")]
    Source(#[from] crate::source::SourceError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        source: crate::source::SourceError,
    },
    #[error(transparent)]
    Rom(#[from] crate::rom::RomError),
    #[error(transparent)]
    Map16(#[from] crate::map16::Map16Error),
}

/// One file, its tiles, and its comments.
#[derive(Clone, PartialEq, Eq, Debug)]
struct PageFile<T> {
    /// Relative to the project's folder.
    path: PathBuf,
    tiles: T,
    comments: PageComments,
}

/// Every foreground Map16 file the project has, at one point in the
/// history.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Files {
    /// Pages 2 to `$7F`.
    pages: BTreeMap<u8, PageFile<Map16Page>>,
    /// Pages 0 and 1.
    game: BTreeMap<u8, PageFile<GamePage>>,
    tilesets: BTreeMap<u8, PageFile<Map16Page>>,
    /// Background pages, `$00` to `$FF` (table * 16 + page).
    background: BTreeMap<u8, PageFile<Map16Page>>,
    /// The pipes file, if the project has one or a change made it.
    pipes: Option<PipesFile>,
}

/// The pipes file: its place, its tiles, and the comments before them.
#[derive(Clone, PartialEq, Eq, Debug)]
struct PipesFile {
    path: PathBuf,
    pipes: Pipes,
    top: Vec<String>,
}

/// Whether object tileset `tileset` draws `tile` from the diagonal pipes'
/// table, which replaces those tiles in tilesets 0 and 7.
fn diagonal(tile: u16, tileset: u8) -> bool {
    matches!(tileset, 0 | 7) && diagonal_pipe_tiles().any(|t| t == tile)
}

impl Files {
    fn tileset_page2(&self) -> bool {
        self.tilesets
            .values()
            .any(|f| f.tiles.tiles.keys().any(|&t| t >= 0x200))
    }

    /// The tileset whose file holds the graphics `tileset` shows for the
    /// tiles of pages 0 and 1 the game keeps per tileset: the one sharing
    /// its table whose file lists such tiles, else itself.
    fn table_owner(&self, tables: &GameTables, tileset: u8) -> u8 {
        let group = tables
            .sharing()
            .into_iter()
            .find(|g| g.contains(&tileset))
            .unwrap_or_else(|| sharing_group(tileset).to_vec());
        group
            .into_iter()
            .find(|t| {
                self.tilesets
                    .get(t)
                    .is_some_and(|f| f.tiles.tiles.keys().any(|&n| n < 0x200))
            })
            .unwrap_or(tileset)
    }
}

/// A step in the history: its label, and the files before it.
#[derive(Clone, Debug)]
struct Step {
    label: String,
    files: Files,
}

/// A tile as an edit sets it, for a tileset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileChange {
    pub tile: u16,
    /// The object tileset of the level it is edited in, which decides
    /// which definition of a per-tileset tile it is.
    pub tileset: u8,
    pub entry: Map16Entry,
}

/// The project's foreground Map16, open: the files as they are now and as
/// on disk, and the steps to undo and redo.
#[derive(Clone, Debug)]
pub struct Map16Document {
    root: PathBuf,
    tables: GameTables,
    /// The clean ROM's pages 0 and 1, per object tileset.
    clean: Vec<Vec<Map16Tile>>,
    /// The clean ROM's background tiles: table 0's pages 0 and 1.
    clean_background: Vec<Map16Tile>,
    /// The clean ROM's other definitions of the pipes' tiles: by colour
    /// set (0, 2, 3) or, with `None`, the diagonal pipes'.
    clean_pipes: BTreeMap<(Option<u8>, u16), Map16Tile>,
    files: Files,
    saved: Files,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl Map16Document {
    /// Opens the Map16 files of the project in `root`, whose clean ROM is
    /// `clean`.
    pub fn open(root: &Path, clean: &Rom) -> Result<Self, Map16EditError> {
        let manifest_path = root.join(MANIFEST);
        let (manifest, _) =
            Manifest::from_toml(&read(&manifest_path)?).map_err(|source| Map16EditError::File {
                path: manifest_path,
                source,
            })?;
        let mut files = Files::default();
        for (&page, file) in &manifest.map16 {
            let path = root.join(file);
            let text = read(&path)?;
            let err = |source| Map16EditError::File {
                path: path.clone(),
                source,
            };
            if GAME_PAGES.contains(&page) {
                let (tiles, comments) = GamePage::from_toml(page, &text).map_err(err)?;
                files.game.insert(
                    page,
                    PageFile {
                        path: file.clone(),
                        tiles,
                        comments,
                    },
                );
            } else {
                let (tiles, comments) =
                    Map16Page::from_toml(PageKind::Foreground, page, &text).map_err(err)?;
                files.pages.insert(
                    page,
                    PageFile {
                        path: file.clone(),
                        tiles,
                        comments,
                    },
                );
            }
        }
        for (&tileset, file) in &manifest.map16_tileset {
            let path = root.join(file);
            let (tiles, comments) = Map16Page::from_toml(PageKind::Tileset, tileset, &read(&path)?)
                .map_err(|source| Map16EditError::File {
                    path: path.clone(),
                    source,
                })?;
            files.tilesets.insert(
                tileset,
                PageFile {
                    path: file.clone(),
                    tiles,
                    comments,
                },
            );
        }
        for (&page, file) in &manifest.map16_bg {
            let path = root.join(file);
            let (tiles, comments) = Map16Page::from_toml(PageKind::Background, page, &read(&path)?)
                .map_err(|source| Map16EditError::File {
                    path: path.clone(),
                    source,
                })?;
            files.background.insert(
                page,
                PageFile {
                    path: file.clone(),
                    tiles,
                    comments,
                },
            );
        }
        if let Some(file) = &manifest.map16_pipes {
            let path = root.join(file);
            let (pipes, top) =
                Pipes::from_toml(&read(&path)?).map_err(|source| Map16EditError::File {
                    path: path.clone(),
                    source,
                })?;
            files.pipes = Some(PipesFile {
                path: file.clone(),
                pipes,
                top,
            });
        }
        let mut clean_pipes = BTreeMap::new();
        let sets = PIPE_SETS
            .iter()
            .flat_map(|&set| PIPE_COLOUR_TILES.map(move |t| (Some(set), t)))
            .chain(diagonal_pipe_tiles().map(|t| (None, t)));
        for (set, tile) in sets {
            let at = pipe_address(set, tile).expect("a pipe's tile");
            let bytes: [u8; 8] = clean.read(at, 8)?.try_into().expect("eight bytes");
            clean_pipes.insert((set, tile), Map16Tile::from_bytes(bytes));
        }
        let mut clean_tiles = Vec::new();
        let mut clean_background = Vec::new();
        for tileset in 0..TILESET_COUNT {
            let table = vanilla_map16(clean, tileset, false)?;
            clean_tiles.push(table.tiles[..0x200].to_vec());
            clean_background = table.tiles[0x200..0x400].to_vec();
        }
        Ok(Self {
            root: root.to_path_buf(),
            tables: GameTables::read(clean)?,
            clean: clean_tiles,
            clean_background,
            clean_pipes,
            saved: files.clone(),
            files,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Tile `tile` as a level of object tileset `tileset` gets it from
    /// the project.
    pub fn entry(&self, tile: u16, tileset: u8) -> Map16Entry {
        let files = &self.files;
        let tileset = tileset % TILESET_COUNT;
        match tile >> 8 {
            0 | 1 => {
                let game = files
                    .game
                    .get(&((tile >> 8) as u8))
                    .and_then(|f| f.tiles.tiles.get(&tile).copied())
                    .unwrap_or_default();
                let gfx = if diagonal(tile, tileset) {
                    Some(self.pipe(None, tile))
                } else if self.tables.is_specific(tile) {
                    let owner = files.table_owner(&self.tables, tileset);
                    files
                        .tilesets
                        .get(&owner)
                        .and_then(|f| f.tiles.tiles.get(&tile))
                        .map(|e| e.gfx)
                } else {
                    game.gfx
                };
                Map16Entry {
                    gfx: gfx.unwrap_or_else(|| self.clean_gfx(tile, tileset)),
                    acts: game.acts.unwrap_or(tile),
                }
            }
            2 if files.tileset_page2() => Map16Entry {
                gfx: files
                    .tilesets
                    .get(&tileset)
                    .map(|f| f.tiles.tile(tile).gfx)
                    .unwrap_or_default(),
                acts: files
                    .pages
                    .get(&2)
                    .map_or(DEFAULT_ACTS, |f| f.tiles.tile(tile).acts),
            },
            page => files
                .pages
                .get(&(page as u8))
                .map(|f| f.tiles.tile(tile))
                .unwrap_or_default(),
        }
    }

    /// Tile `tile` as the clean ROM has it for object tileset `tileset`:
    /// the game's, or past page 1 an empty tile.
    pub fn clean_entry(&self, tile: u16, tileset: u8) -> Map16Entry {
        if tile < 0x200 {
            Map16Entry {
                gfx: self.clean_gfx(tile, tileset % TILESET_COUNT),
                acts: tile,
            }
        } else {
            Map16Entry::default()
        }
    }

    fn clean_gfx(&self, tile: u16, tileset: u8) -> Map16Tile {
        if diagonal(tile, tileset) {
            return self.clean_pipes[&(None, tile)];
        }
        self.clean[usize::from(tileset)][usize::from(tile)]
    }

    /// A pipe's other definition of `tile`, as the project has it: in
    /// colour set `set` (0, 2, or 3), or with `None` the diagonal pipes'.
    pub fn pipe(&self, set: Option<u8>, tile: u16) -> Map16Tile {
        let changed = self.files.pipes.as_ref().and_then(|f| match set {
            Some(set) => f.pipes.colours.get(&(set, tile)),
            None => f.pipes.diagonal.get(&tile),
        });
        changed
            .or_else(|| self.clean_pipes.get(&(set, tile)))
            .copied()
            .unwrap_or_default()
    }

    /// Sets a vertical pipe's tile in colour set `set` (0, 2, or 3), as
    /// one undo step. Set 1 is page 1's own tiles.
    pub fn apply_pipe_colour(
        &mut self,
        label: impl Into<String>,
        set: u8,
        tile: u16,
        gfx: Map16Tile,
    ) {
        if !PIPE_SETS.contains(&set) || !PIPE_COLOUR_TILES.contains(&tile) {
            return;
        }
        let mut files = self.files.clone();
        self.set_pipe(&mut files, Some(set), tile, gfx);
        if files != self.files {
            self.undo.push(Step {
                label: label.into(),
                files: std::mem::replace(&mut self.files, files),
            });
            self.redo.clear();
        }
    }

    fn set_pipe(&self, files: &mut Files, set: Option<u8>, tile: u16, gfx: Map16Tile) {
        let file = files.pipes.get_or_insert_with(|| PipesFile {
            path: PathBuf::from("map16").join("pipes.toml"),
            pipes: Pipes::default(),
            top: Vec::new(),
        });
        let clean = self.clean_pipes.get(&(set, tile)).copied();
        match set {
            Some(set) if Some(gfx) == clean => {
                file.pipes.colours.remove(&(set, tile));
            }
            Some(set) => {
                file.pipes.colours.insert((set, tile), gfx);
            }
            None if Some(gfx) == clean => {
                file.pipes.diagonal.remove(&tile);
            }
            None => {
                file.pipes.diagonal.insert(tile, gfx);
            }
        }
        if file.pipes.is_empty() && self.saved.pipes.is_none() {
            files.pipes = None;
        }
    }

    /// Whether the project's files hold anything of tile `tile` for
    /// `tileset`: whether it differs from the clean ROM's.
    pub fn is_changed(&self, tile: u16, tileset: u8) -> bool {
        self.entry(tile, tileset) != self.clean_entry(tile, tileset)
    }

    /// The tilesets that show the same definition of tile `tile` as
    /// `tileset`, itself among them: those sharing its table for a tile
    /// the game keeps per tileset, every one for a tile it does not, and
    /// itself alone for page 2 when that is per tileset.
    pub fn shown_in(&self, tile: u16, tileset: u8) -> Vec<u8> {
        let tileset = tileset % TILESET_COUNT;
        if diagonal(tile, tileset) {
            return vec![0, 7];
        }
        match tile >> 8 {
            0 | 1 if self.tables.is_specific(tile) => self
                .tables
                .sharing()
                .into_iter()
                .find(|g| g.contains(&tileset))
                .unwrap_or_else(|| vec![tileset]),
            2 if self.files.tileset_page2() => vec![tileset],
            _ => (0..TILESET_COUNT).collect(),
        }
    }

    /// Background tile `tile` (its table times `$1000`, plus its number in
    /// the table) as the project has it.
    pub fn background(&self, tile: u16) -> Map16Tile {
        self.files
            .background
            .get(&((tile >> 8) as u8))
            .and_then(|f| f.tiles.tiles.get(&tile))
            .map_or_else(|| self.clean_background(tile), |e| e.gfx)
    }

    /// Background tile `tile` as the clean ROM has it: the game's in table
    /// 0's pages 0 and 1, else empty.
    pub fn clean_background(&self, tile: u16) -> Map16Tile {
        if tile < 0x200 {
            self.clean_background[usize::from(tile)]
        } else {
            Map16Tile::default()
        }
    }

    pub fn is_background_changed(&self, tile: u16) -> bool {
        self.background(tile) != self.clean_background(tile)
    }

    /// Sets background tiles, as one undo step called `label`. A tile set
    /// as the clean ROM has it leaves its file.
    pub fn apply_background(&mut self, label: impl Into<String>, changes: &[(u16, Map16Tile)]) {
        let mut files = self.files.clone();
        self.set_background(&mut files, changes);
        if files != self.files {
            self.undo.push(Step {
                label: label.into(),
                files: std::mem::replace(&mut self.files, files),
            });
            self.redo.clear();
        }
    }

    /// Sets background tiles as part of the last undo step.
    pub fn amend_background(&mut self, changes: &[(u16, Map16Tile)]) {
        if self.undo.is_empty() {
            return self.apply_background("Edit BG Map16", changes);
        }
        let mut files = self.files.clone();
        self.set_background(&mut files, changes);
        self.files = files;
        self.redo.clear();
    }

    fn set_background(&self, files: &mut Files, changes: &[(u16, Map16Tile)]) {
        for &(tile, gfx) in changes {
            let page = (tile >> 8) as u8;
            let file = files
                .background
                .entry(page)
                .or_insert_with(|| new_file(background_path(page), Map16Page::default()));
            if gfx == self.clean_background(tile) {
                file.tiles.tiles.remove(&tile);
            } else {
                let entry = Map16Entry {
                    gfx,
                    acts: DEFAULT_ACTS,
                };
                file.tiles.tiles.insert(tile, entry);
            }
        }
        let saved = &self.saved;
        files
            .background
            .retain(|p, f| !f.tiles.tiles.is_empty() || saved.background.contains_key(p));
    }

    /// Applies `changes` in order as one undo step called `label`. If one
    /// fails, none is applied.
    pub fn apply(
        &mut self,
        label: impl Into<String>,
        changes: &[TileChange],
    ) -> Result<(), Map16EditError> {
        let mut files = self.files.clone();
        for change in changes {
            self.set(&mut files, change)?;
        }
        if files != self.files {
            self.undo.push(Step {
                label: label.into(),
                files: std::mem::replace(&mut self.files, files),
            });
            self.redo.clear();
        }
        Ok(())
    }

    /// Applies `changes` as part of the last undo step, as
    /// [`LevelDocument::amend`](super::LevelDocument::amend) does: a value
    /// dragged in the editor is one step.
    pub fn amend(&mut self, changes: &[TileChange]) -> Result<(), Map16EditError> {
        if self.undo.is_empty() {
            return self.apply("Edit Map16", changes);
        }
        let mut files = self.files.clone();
        for change in changes {
            self.set(&mut files, change)?;
        }
        self.files = files;
        self.redo.clear();
        Ok(())
    }

    fn set(&self, files: &mut Files, change: &TileChange) -> Result<(), Map16EditError> {
        let TileChange {
            tile,
            tileset,
            entry,
        } = *change;
        if tileset >= TILESET_COUNT {
            return Err(Map16EditError::NoTileset(tileset));
        }
        let page = tile >> 8;
        if page > u16::from(*PAGES.end()) {
            return Err(Map16EditError::NoTile(tile));
        }
        let page = page as u8;
        match page {
            1 if diagonal(tile, tileset) => {
                // The diagonal pipes' table for the graphics; what it acts
                // like, the same in every tileset, in page 1's file.
                self.set_pipe(files, None, tile, entry.gfx);
                let file = files
                    .game
                    .entry(page)
                    .or_insert_with(|| new_file(page_path(page), GamePage::default()));
                let mut game = file.tiles.tiles.get(&tile).copied().unwrap_or_default();
                game.acts = (entry.acts != tile).then_some(entry.acts);
                if game.acts.is_none() && game.gfx.is_none() {
                    file.tiles.tiles.remove(&tile);
                } else {
                    file.tiles.tiles.insert(tile, game);
                }
            }
            0 | 1 => {
                let specific = self.tables.is_specific(tile);
                if specific {
                    let owner = files.table_owner(&self.tables, tileset);
                    let clean = self.clean_gfx(tile, owner);
                    let file = files
                        .tilesets
                        .entry(owner)
                        .or_insert_with(|| new_file(tileset_path(owner), Map16Page::default()));
                    if entry.gfx == clean {
                        file.tiles.tiles.remove(&tile);
                    } else {
                        let gfx = Map16Entry {
                            gfx: entry.gfx,
                            acts: DEFAULT_ACTS,
                        };
                        file.tiles.tiles.insert(tile, gfx);
                    }
                }
                let file = files
                    .game
                    .entry(page)
                    .or_insert_with(|| new_file(page_path(page), GamePage::default()));
                let mut game = file.tiles.tiles.get(&tile).copied().unwrap_or_default();
                game.acts = (entry.acts != tile).then_some(entry.acts);
                game.gfx =
                    (!specific && entry.gfx != self.clean_gfx(tile, tileset)).then_some(entry.gfx);
                if game.acts.is_none() && game.gfx.is_none() {
                    file.tiles.tiles.remove(&tile);
                } else {
                    file.tiles.tiles.insert(tile, game);
                }
            }
            2 if files.tileset_page2() => {
                let file = files
                    .tilesets
                    .entry(tileset)
                    .or_insert_with(|| new_file(tileset_path(tileset), Map16Page::default()));
                set_or_remove(
                    &mut file.tiles,
                    tile,
                    Map16Entry {
                        gfx: entry.gfx,
                        acts: DEFAULT_ACTS,
                    },
                );
                let file = files
                    .pages
                    .entry(2)
                    .or_insert_with(|| new_file(page_path(2), Map16Page::default()));
                set_or_remove(
                    &mut file.tiles,
                    tile,
                    Map16Entry {
                        gfx: Map16Tile::default(),
                        acts: entry.acts,
                    },
                );
            }
            _ => {
                let file = files
                    .pages
                    .entry(page)
                    .or_insert_with(|| new_file(page_path(page), Map16Page::default()));
                set_or_remove(&mut file.tiles, tile, entry);
            }
        }
        // A file this session made that has nothing left is dropped.
        let saved = &self.saved;
        files
            .pages
            .retain(|p, f| !f.tiles.tiles.is_empty() || saved.pages.contains_key(p));
        files
            .game
            .retain(|p, f| !f.tiles.tiles.is_empty() || saved.game.contains_key(p));
        files
            .tilesets
            .retain(|t, f| !f.tiles.tiles.is_empty() || saved.tilesets.contains_key(t));
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.files != self.saved
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let files = std::mem::replace(&mut self.files, step.files);
        self.redo.push(Step {
            label: step.label,
            files,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let files = std::mem::replace(&mut self.files, step.files);
        self.undo.push(Step {
            label: step.label,
            files,
        });
        true
    }

    /// Writes every file that changed since it was read or last saved, and
    /// the manifest when a file is new to it. Returns the files written.
    pub fn save(&mut self) -> Result<Vec<PathBuf>, Map16EditError> {
        let mut written = Vec::new();
        let mut write = |file: &Path, text: String| -> Result<(), Map16EditError> {
            let path = self.root.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| Map16EditError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&path, text).map_err(|source| Map16EditError::Io {
                path: path.clone(),
                source,
            })?;
            written.push(path);
            Ok(())
        };
        for (page, file) in &self.files.pages {
            if self.saved.pages.get(page) != Some(file) {
                write(
                    &file.path,
                    file.tiles.to_toml(PageKind::Foreground, &file.comments),
                )?;
            }
        }
        for (page, file) in &self.files.game {
            if self.saved.game.get(page) != Some(file) {
                write(&file.path, file.tiles.to_toml(&file.comments))?;
            }
        }
        for (tileset, file) in &self.files.tilesets {
            if self.saved.tilesets.get(tileset) != Some(file) {
                write(
                    &file.path,
                    file.tiles.to_toml(PageKind::Tileset, &file.comments),
                )?;
            }
        }
        for (page, file) in &self.files.background {
            if self.saved.background.get(page) != Some(file) {
                write(
                    &file.path,
                    file.tiles.to_toml(PageKind::Background, &file.comments),
                )?;
            }
        }
        let new_pages: Vec<(u8, PathBuf)> = self
            .files
            .pages
            .iter()
            .map(|(p, f)| (*p, &f.path))
            .chain(self.files.game.iter().map(|(p, f)| (*p, &f.path)))
            .filter(|(p, _)| !self.saved.pages.contains_key(p) && !self.saved.game.contains_key(p))
            .map(|(p, path)| (p, path.clone()))
            .collect();
        let new_tilesets: Vec<(u8, PathBuf)> = self
            .files
            .tilesets
            .iter()
            .filter(|(t, _)| !self.saved.tilesets.contains_key(t))
            .map(|(t, f)| (*t, f.path.clone()))
            .collect();
        let mut new_pipes = None;
        if let Some(file) = &self.files.pipes
            && self.saved.pipes.as_ref() != Some(file)
        {
            write(&file.path, file.pipes.to_toml(&file.top))?;
            if self.saved.pipes.is_none() {
                new_pipes = Some(file.path.clone());
            }
        }
        let new_background: Vec<(u8, PathBuf)> = self
            .files
            .background
            .iter()
            .filter(|(p, _)| !self.saved.background.contains_key(p))
            .map(|(p, f)| (*p, f.path.clone()))
            .collect();
        if !new_pages.is_empty()
            || !new_tilesets.is_empty()
            || !new_background.is_empty()
            || new_pipes.is_some()
        {
            let manifest_path = self.root.join(MANIFEST);
            let (mut manifest, comments) =
                Manifest::from_toml(&read(&manifest_path)?).map_err(|source| {
                    Map16EditError::File {
                        path: manifest_path.clone(),
                        source,
                    }
                })?;
            manifest.map16.extend(new_pages);
            manifest.map16_tileset.extend(new_tilesets);
            manifest.map16_bg.extend(new_background);
            if new_pipes.is_some() {
                manifest.map16_pipes = new_pipes;
            }
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| {
                Map16EditError::Io {
                    path: manifest_path.clone(),
                    source,
                }
            })?;
            written.push(manifest_path);
        }
        self.saved = self.files.clone();
        Ok(written)
    }

    /// Puts the document's Map16 into `project`, in place of what it read
    /// from the files, so that a build has the unsaved edits.
    pub fn apply_to(&self, project: &mut Project) {
        let files = &self.files;
        project.map16 = files
            .pages
            .iter()
            .map(|(p, f)| (*p, f.tiles.clone()))
            .collect();
        project.map16_game = files
            .game
            .iter()
            .map(|(p, f)| (*p, f.tiles.clone()))
            .collect();
        project.map16_tileset = files
            .tilesets
            .iter()
            .map(|(t, f)| (*t, f.tiles.clone()))
            .collect();
        for (p, f) in files.pages.iter() {
            project.manifest.map16.insert(*p, f.path.clone());
        }
        for (p, f) in files.game.iter() {
            project.manifest.map16.insert(*p, f.path.clone());
        }
        for (t, f) in files.tilesets.iter() {
            project.manifest.map16_tileset.insert(*t, f.path.clone());
        }
        project.pipes = files
            .pipes
            .as_ref()
            .map(|f| f.pipes.clone())
            .unwrap_or_default();
        if let Some(file) = &files.pipes {
            project.manifest.map16_pipes = Some(file.path.clone());
        }
        project.map16_bg = files
            .background
            .iter()
            .map(|(p, f)| (*p, f.tiles.clone()))
            .collect();
        for (p, f) in files.background.iter() {
            project.manifest.map16_bg.insert(*p, f.path.clone());
        }
    }
}

fn new_file<T>(path: PathBuf, tiles: T) -> PageFile<T> {
    PageFile {
        path,
        tiles,
        comments: PageComments::default(),
    }
}

fn page_path(page: u8) -> PathBuf {
    PathBuf::from("map16").join(format!("{page:02X}.toml"))
}

fn background_path(page: u8) -> PathBuf {
    PathBuf::from("map16").join(format!("bg-{page:02X}.toml"))
}

fn tileset_path(tileset: u8) -> PathBuf {
    PathBuf::from("map16").join(format!("tileset-{tileset:X}.toml"))
}

/// Lists `entry` as tile `tile`, or leaves it out when it is the empty
/// tile a file's leaving it out means.
fn set_or_remove(page: &mut Map16Page, tile: u16, entry: Map16Entry) {
    if entry == Map16Entry::default() {
        page.tiles.remove(&tile);
    } else {
        page.tiles.insert(tile, entry);
    }
}

fn read(path: &Path) -> Result<String, Map16EditError> {
    fs::read_to_string(path).map_err(|source| Map16EditError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map16::Tile8Ref;

    fn gfx(n: u16) -> Map16Tile {
        Map16Tile {
            top_left: Tile8Ref::new(n, 2, false, false, false),
            bottom_left: Tile8Ref::new(n + 0x10, 2, false, false, false),
            top_right: Tile8Ref::new(n + 1, 2, false, false, false),
            bottom_right: Tile8Ref::new(n + 0x11, 2, true, false, false),
        }
    }

    /// A document over no files, with a made-up clean ROM's pages 0 and 1:
    /// tiles `$100` to `$10F` kept per tileset, tilesets 0 and 7 sharing
    /// a table.
    fn document() -> Map16Document {
        let mut mask = [0xFFu8; 0x40];
        mask[0x20] = 0;
        mask[0x21] = 0;
        let mut specific = [0u16; TILESET_COUNT as usize];
        for (i, s) in specific.iter_mut().enumerate() {
            *s = if i == 7 { 0 } else { i as u16 * 0x100 };
        }
        let tables = GameTables::from_parts(mask, specific);
        let clean = (0..TILESET_COUNT)
            .map(|t| (0..0x200).map(|n| gfx(n + u16::from(t))).collect())
            .collect();
        Map16Document {
            root: PathBuf::new(),
            tables,
            clean,
            clean_background: (0..0x200).map(|n| gfx(0x300 + n)).collect(),
            clean_pipes: diagonal_pipe_tiles()
                .map(|t| ((None, t), gfx(0x500 + t)))
                .chain(PIPE_COLOUR_TILES.map(|t| ((Some(0), t), gfx(0x600 + t))))
                .collect(),
            files: Files::default(),
            saved: Files::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn change(doc: &mut Map16Document, tile: u16, tileset: u8, entry: Map16Entry) {
        let change = TileChange {
            tile,
            tileset,
            entry,
        };
        doc.apply("Edit", &[change]).unwrap();
    }

    #[test]
    fn a_common_tile_goes_in_its_page_file_and_back_out() {
        let mut doc = document();
        let entry = Map16Entry {
            gfx: gfx(0x80),
            acts: 0x25,
        };
        change(&mut doc, 0x045, 3, entry);
        assert_eq!(doc.entry(0x045, 3), entry);
        // Every tileset shows it: the game keeps one definition.
        assert_eq!(doc.entry(0x045, 9), entry);
        let game = &doc.files.game[&0].tiles.tiles[&0x045];
        assert_eq!((game.acts, game.gfx), (Some(0x25), Some(gfx(0x80))));

        // Acting like itself again, it keeps only its graphics.
        let back = Map16Entry {
            acts: 0x045,
            ..entry
        };
        change(&mut doc, 0x045, 3, back);
        assert_eq!(doc.files.game[&0].tiles.tiles[&0x045].acts, None);
        // And as the clean ROM has it, nothing; the new file goes too.
        let clean = doc.clean_entry(0x045, 3);
        change(&mut doc, 0x045, 3, clean);
        assert!(doc.files.game.is_empty());
        assert!(!doc.is_modified());
    }

    #[test]
    fn a_tile_kept_per_tileset_goes_in_its_tables_file() {
        let mut doc = document();
        let entry = Map16Entry {
            gfx: gfx(0x90),
            acts: 0x105,
        };
        change(&mut doc, 0x105, 0, entry);
        assert_eq!(doc.files.tilesets[&0].tiles.tiles[&0x105].gfx, gfx(0x90));
        // It acts like itself, so page 1's file has nothing of it.
        assert!(doc.files.game.is_empty());
        // Tileset 7 shares tileset 0's table, so shows it; 1 does not.
        assert_eq!(doc.entry(0x105, 7).gfx, gfx(0x90));
        assert_eq!(doc.entry(0x105, 1).gfx, gfx(0x106));
        assert_eq!(doc.shown_in(0x105, 7), [0, 7]);
        // Edited from tileset 7, it stays in tileset 0's file.
        let again = Map16Entry {
            gfx: gfx(0x91),
            ..entry
        };
        change(&mut doc, 0x105, 7, again);
        assert!(!doc.files.tilesets.contains_key(&7));
        assert_eq!(doc.files.tilesets[&0].tiles.tiles[&0x105].gfx, gfx(0x91));
    }

    #[test]
    fn page_2_per_tileset_splits_graphics_from_acts() {
        let mut doc = document();
        // A page 2 tile in a tileset file makes page 2 per tileset.
        let mut page = Map16Page::default();
        page.tiles.insert(
            0x200,
            Map16Entry {
                gfx: gfx(1),
                acts: DEFAULT_ACTS,
            },
        );
        doc.files
            .tilesets
            .insert(4, new_file(tileset_path(4), page));
        let entry = Map16Entry {
            gfx: gfx(0x40),
            acts: 0x25,
        };
        change(&mut doc, 0x210, 4, entry);
        assert_eq!(doc.entry(0x210, 4), entry);
        assert_eq!(doc.entry(0x210, 5).gfx, Map16Tile::default());
        assert_eq!(doc.entry(0x210, 5).acts, 0x25);
        assert_eq!(
            doc.files.pages[&2].tiles.tiles[&0x210].gfx,
            Map16Tile::default()
        );
    }

    #[test]
    fn the_diagonal_pipes_tiles_go_in_the_pipes_file_in_tilesets_0_and_7() {
        let mut doc = document();
        assert_eq!(doc.entry(0x1C4, 7).gfx, gfx(0x6C4), "the diagonal pipe's");
        let entry = Map16Entry {
            gfx: gfx(0x11),
            acts: 0x130,
        };
        change(&mut doc, 0x1C4, 0, entry);
        assert_eq!(doc.entry(0x1C4, 7), entry, "tileset 7 shares it");
        assert_eq!(
            doc.files.pipes.as_ref().unwrap().pipes.diagonal[&0x1C4],
            gfx(0x11)
        );
        let game = &doc.files.game[&1].tiles.tiles[&0x1C4];
        assert_eq!((game.acts, game.gfx), (Some(0x130), None));
        // Tileset 1 has the page's own tile.
        assert_eq!(doc.entry(0x1C4, 1).gfx, gfx(0x1C5));
        // A colour set's tile.
        doc.apply_pipe_colour("Edit", 0, 0x133, gfx(0x22));
        assert_eq!(doc.pipe(Some(0), 0x133), gfx(0x22));
        doc.apply_pipe_colour("Back", 0, 0x133, gfx(0x733));
        assert!(
            !doc.files
                .pipes
                .as_ref()
                .unwrap()
                .pipes
                .colours
                .contains_key(&(0, 0x133))
        );
    }

    #[test]
    fn background_tiles_leave_their_file_as_the_clean_rom_has_them() {
        let mut doc = document();
        // Table 0's page 1, the game's: the clean ROM's until changed.
        assert_eq!(doc.background(0x105), gfx(0x405));
        doc.apply_background("Edit", &[(0x105, gfx(1)), (0x1203, gfx(2))]);
        assert_eq!(doc.background(0x105), gfx(1));
        assert_eq!(
            doc.files.background[&0x12].path,
            Path::new("map16/bg-12.toml")
        );
        assert!(doc.is_background_changed(0x1203));
        doc.apply_background(
            "Back",
            &[(0x105, gfx(0x405)), (0x1203, Map16Tile::default())],
        );
        assert!(doc.files.background.is_empty());
        assert!(!doc.is_modified());
    }

    #[test]
    fn pages_past_2_hold_whole_tiles_and_undo_takes_them_back() {
        let mut doc = document();
        let entry = Map16Entry {
            gfx: gfx(0x20),
            acts: 0x130,
        };
        change(&mut doc, 0x512, 0, entry);
        assert_eq!(doc.files.pages[&5].path, Path::new("map16/05.toml"));
        assert_eq!(doc.entry(0x512, 6), entry);
        assert_eq!(doc.undo_label(), Some("Edit"));
        assert!(doc.undo());
        assert_eq!(doc.entry(0x512, 6), Map16Entry::default());
        assert!(doc.redo());
        assert_eq!(doc.entry(0x512, 6), entry);
        let past = TileChange {
            tile: 0x8000,
            tileset: 0,
            entry,
        };
        assert!(doc.apply("Edit", &[past]).is_err());
    }
}
