//! Building a project into a ROM.
//!
//! A build starts from the clean ROM and writes what the project defines,
//! nothing else: a level the project does not list keeps the clean ROM's
//! data. The output depends on the clean ROM, the project files, and the
//! Kobo version alone.
//!
//! A build runs fixed [`Stage`]s in order, each on the image the one
//! before it left, and can keep a snapshot of the image after each in a
//! [`Cache`], keyed by a hash chained through the stages: the previous
//! key, the stage and its version, Kobo's version, and the stage's inputs.
//! A build starts again from the last stage whose key has a snapshot.
//!
//! Levels are in the game's own formats: layer data goes in RATS blocks in
//! the expanded ROM, and sprite lists stay where the clean ROM has them
//! when unchanged. A changed one goes in a RATS block too when the build
//! has Lunar Magic's layout, whose sprite data banks let a list be in any
//! bank, and otherwise in bank `$07`'s unused space, the only bank the game
//! reads them from. What only Lunar Magic's layout has, Map16 pages past 1
//! and the rest, is written in that layout, with Kobo's own code for it
//! ([`crate::install`]) installed first.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::asar::{Asar, AsarError, Patch};
use crate::compress::rle1;
use crate::compress::{lz2, lz3};
use crate::config::ConfigError;
use crate::entrance::{self, LevelSettings};
use crate::exanimation;
use crate::exgfx::{self, GraphicsList};
use crate::gfx;
use crate::image::IndexedImage;
use crate::install;
use crate::level::objects::{self, Layout, Object, ObjectError};
use crate::level::{self, Layer2Kind, LevelError, tables};
use crate::map16::GameTables;
use crate::map16::pages as map16_pages;
use crate::palette;
use crate::rats::{Contents, FreeSpace, FreeSpaceError};
use crate::rom::{Rom, RomError, RomIdentity};
use crate::source::SourceError;
use crate::source::level::{BACKGROUND_ROWS, BackgroundTiles, Layer2, Level};
use crate::source::map16::{self as page_source, GamePage, Map16Page, PageKind, Pipes};
use crate::source::project::{MANIFEST, Manifest};
use crate::sprites::{self, SpriteEncodeError};
use crate::tools::{self, Located, Tool, ToolError};
use sha1::{Digest, Sha1};

/// The size a project that writes anything and sets none is expanded to.
pub const DEFAULT_ROM_SIZE: usize = 0x10_0000;

/// Unused space in bank `$07`, all `$FF` in the US version (SMWDisX's
/// free space list), for sprite lists.
const BANK_07_FREE: [(u16, u16); 5] = [
    (0x80ED, 0x8100),
    (0xA179, 0xA600),
    (0xC226, 0xC300),
    (0xE76F, 0xF000),
    (0xFC90, 0x0000),
];

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(
        "GFX in LC_LZ3 (`[rom] lz3`) needs an SA-1 build, whose decompressor SA-1 Pack \
         supplies; Kobo has none of its own for a LoROM build"
    )]
    Lz3WithoutSa1,
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Source {
        path: PathBuf,
        #[source]
        source: SourceError,
    },
    #[error("the clean ROM must be the vanilla USA image; this one's SHA-1 is {0}")]
    NotClean(String),
    #[error("level {level:03X}: {message}")]
    Level { level: u16, message: String },
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error(transparent)]
    ReadLevel(#[from] LevelError),
    #[error(transparent)]
    FreeSpace(#[from] FreeSpaceError),
    #[error("patch {path}: {source}")]
    Patch {
        path: PathBuf,
        #[source]
        source: Box<AsarError>,
    },
    #[error(transparent)]
    Asar(Box<AsarError>),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{path}: {message}")]
    Gfx { path: PathBuf, message: String },
    #[error("Kobo's code for Lunar Magic's layout: {message}")]
    Install { message: String },
    #[error("Map16: {0}")]
    Map16(String),
    #[error("PIXI's compiled insert: {0}")]
    Pixi(String),
    #[error("the build cache at {path}: {source}")]
    Cache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn level_error(level: u16, message: impl std::fmt::Display) -> BuildError {
    BuildError::Level {
        level,
        message: message.to_string(),
    }
}

/// A loaded project.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Project {
    /// The folder the manifest's paths are relative to.
    pub root: PathBuf,
    pub manifest: Manifest,
    pub levels: Vec<(u16, Level)>,
    /// Map16 pages 2 to `$7F`.
    pub map16: Vec<(u8, Map16Page)>,
    /// What the project changes of Map16 pages 0 and 1, the game's.
    pub map16_game: Vec<(u8, GamePage)>,
    /// Object tilesets' own tiles: those of pages 0 and 1 the game keeps
    /// per tileset, and page 2 when it is per tileset.
    pub map16_tileset: Vec<(u8, Map16Page)>,
    /// BG Map16 pages, `$00` to `$FF` (table * 16 + page).
    pub map16_bg: Vec<(u8, Map16Page)>,
    /// What the project changes of the vertical pipes' colours and the
    /// diagonal pipes.
    pub pipes: Pipes,
    /// GFX files `00` to `33`, as images of colour indices.
    pub gfx: Vec<(u8, IndexedImage)>,
    /// ExGFX files, as the bytes the ROM holds decompressed.
    pub exgfx: Vec<(u16, Vec<u8>)>,
    /// Lunar Magic's global ExAnimation list.
    pub animation_global: Option<exanimation::List>,
    /// The uncompressed ExGFX files `60`-`63` ExAnimation reads.
    pub animation_files: Vec<(u16, Vec<u8>)>,
    /// A hack's PIXI insert carried as compiled code (`[pixi] compiled`).
    pub pixi_compiled: Option<CompiledPixi>,
}

/// A compiled PIXI insert and the files it was read from.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct CompiledPixi {
    pub insert: crate::pixi::Insert,
    /// Its file and its blocks' files, relative to the project's folder.
    pub files: Vec<PathBuf>,
}

impl CompiledPixi {
    /// Reads the insert from `file`, relative to the project's folder.
    pub fn load(root: &Path, file: &Path) -> Result<Self, BuildError> {
        let path = root.join(file);
        let text = fs::read_to_string(&path).map_err(|source| BuildError::Io {
            path: path.clone(),
            source,
        })?;
        let (compiled, _) = crate::source::pixi::Compiled::from_toml(&text)
            .map_err(|source| BuildError::Source { path, source })?;
        let folder = file.parent().unwrap_or(Path::new(""));
        let mut files = vec![file.to_path_buf()];
        let size_table = match &compiled.size_table {
            Some(table) => {
                let relative = folder.join(table);
                let path = root.join(&relative);
                let bytes = fs::read(&path).map_err(|source| BuildError::Io { path, source })?;
                files.push(relative);
                Some(bytes)
            }
            None => None,
        };
        let mut blocks = BTreeMap::new();
        for (&at, block) in &compiled.blocks {
            let relative = folder.join(block);
            let path = root.join(&relative);
            let bytes = fs::read(&path).map_err(|source| BuildError::Io { path, source })?;
            blocks.insert(at, bytes);
            files.push(relative);
        }
        Ok(Self {
            insert: crate::pixi::Insert {
                version: compiled.version,
                sprites_255: compiled.sprites_255,
                sites: compiled.sites,
                blocks,
                size_table,
            },
            files,
        })
    }
}

impl Project {
    pub fn load(dir: &Path) -> Result<Self, BuildError> {
        let read = |path: PathBuf| {
            fs::read_to_string(&path).map_err(|source| BuildError::Io { path, source })
        };
        let manifest_path = dir.join(MANIFEST);
        let (manifest, _) =
            Manifest::from_toml(&read(manifest_path.clone())?).map_err(|source| {
                BuildError::Source {
                    path: manifest_path,
                    source,
                }
            })?;
        let mut levels = Vec::new();
        for (&number, file) in &manifest.levels {
            let path = dir.join(file);
            let (level, _) = Level::from_toml(&read(path.clone())?)
                .map_err(|source| BuildError::Source { path, source })?;
            levels.push((number, level));
        }
        let pages = |list: &std::collections::BTreeMap<u8, PathBuf>, kind| {
            let mut out = Vec::new();
            for (&page, file) in list {
                if kind == PageKind::Foreground && page_source::GAME_PAGES.contains(&page) {
                    continue;
                }
                let path = dir.join(file);
                let (tiles, _) = Map16Page::from_toml(kind, page, &read(path.clone())?)
                    .map_err(|source| BuildError::Source { path, source })?;
                out.push((page, tiles));
            }
            Ok::<_, BuildError>(out)
        };
        let mut gfx = Vec::new();
        for (&index, file) in &manifest.gfx {
            let path = dir.join(file);
            let bytes = fs::read(&path).map_err(|source| BuildError::Io {
                path: path.clone(),
                source,
            })?;
            let image = IndexedImage::from_png(&bytes).map_err(|e| BuildError::Gfx {
                path: path.clone(),
                message: e.to_string(),
            })?;
            gfx.push((index, image));
        }
        let mut exgfx = Vec::new();
        for (&number, file) in &manifest.exgfx {
            let path = dir.join(&file.path);
            let bytes = fs::read(&path).map_err(|source| BuildError::Io {
                path: path.clone(),
                source,
            })?;
            let data = if file.is_binary() {
                bytes
            } else {
                let gfx_error = |message: String| BuildError::Gfx {
                    path: path.clone(),
                    message,
                };
                let image = IndexedImage::from_png(&bytes).map_err(|e| gfx_error(e.to_string()))?;
                let bpp = match file.bpp {
                    2 => gfx::Bpp::Two,
                    3 => gfx::Bpp::Three,
                    _ => gfx::Bpp::Four,
                };
                let count = image.height as usize / 8 * 16;
                let tiles = gfx::image_to_tiles(&image, count, bpp.colors()).map_err(gfx_error)?;
                gfx::GfxFormat::Planar(bpp).encode(&tiles)
            };
            exgfx.push((number, data));
        }
        let animation_global = match &manifest.animation_global {
            None => None,
            Some(file) => {
                let path = dir.join(file);
                let (list, _) = crate::source::animation::global_from_toml(&read(path.clone())?)
                    .map_err(|source| BuildError::Source { path, source })?;
                Some(list)
            }
        };
        let mut animation_files = Vec::new();
        for (&number, file) in &manifest.animation_files {
            let path = dir.join(file);
            let bytes = fs::read(&path).map_err(|source| BuildError::Io {
                path: path.clone(),
                source,
            })?;
            animation_files.push((number, bytes));
        }
        let map16 = pages(&manifest.map16, PageKind::Foreground)?;
        let map16_bg = pages(&manifest.map16_bg, PageKind::Background)?;
        let map16_tileset = pages(&manifest.map16_tileset, PageKind::Tileset)?;
        let mut map16_game = Vec::new();
        for (&page, file) in &manifest.map16 {
            if page_source::GAME_PAGES.contains(&page) {
                let path = dir.join(file);
                let (tiles, _) = GamePage::from_toml(page, &read(path.clone())?)
                    .map_err(|source| BuildError::Source { path, source })?;
                map16_game.push((page, tiles));
            }
        }
        let pixi_compiled = match &manifest.pixi_compiled {
            Some(file) => Some(CompiledPixi::load(dir, file)?),
            None => None,
        };
        let pipes = match &manifest.map16_pipes {
            Some(file) => {
                let path = dir.join(file);
                Pipes::from_toml(&read(path.clone())?)
                    .map_err(|source| BuildError::Source { path, source })?
                    .0
            }
            None => Pipes::default(),
        };
        Ok(Self {
            root: dir.to_path_buf(),
            manifest,
            levels,
            map16,
            map16_game,
            map16_tileset,
            map16_bg,
            pipes,
            gfx,
            exgfx,
            animation_global,
            animation_files,
            pixi_compiled,
        })
    }

    /// Whether the project has Lunar Magic's ExAnimation: a level's list or
    /// settings, the global list, or the files it reads. A build then
    /// installs Kobo's code for it (`exanimation.asm`).
    pub fn exanimation(&self) -> bool {
        self.animation_global.is_some()
            || !self.animation_files.is_empty()
            || self
                .levels
                .iter()
                .any(|(_, l)| l.animation.is_some() || l.animation_settings.is_some())
    }

    /// Whether the project uses Lunar Magic's graphics formats: 4bpp GFX
    /// files, ExGFX, graphics lists, or the older lists and objects `24`
    /// and `25`. A build then installs Kobo's code for them
    /// (`graphics.asm`) and stores every GFX file the game keeps as 3bpp
    /// as 4bpp.
    pub fn lunar_magic_graphics(&self) -> bool {
        self.manifest.four_bpp
            || !self.exgfx.is_empty()
            || !self.manifest.bypass_lists.is_empty()
            || self.levels.iter().any(|(_, level)| {
                level.graphics.is_some() || level.layer1.iter().any(graphics_object)
            })
    }

    /// Whether a level's graphics list has Lunar Magic's layer 3 settings
    /// ([`GraphicsList::has_layer3`]). A build then installs Kobo's code
    /// for them (`layer3.asm`).
    pub fn lunar_magic_layer3(&self) -> bool {
        self.levels.iter().any(|(_, level)| {
            level
                .graphics
                .as_ref()
                .is_some_and(GraphicsList::has_layer3)
        })
    }

    /// How many secondary entrances the build's tables hold: the game's
    /// 512, or up to the last entrance in use past them, as Lunar Magic
    /// 3.70's save sizes them (docs/lunar-magic-install.md, "Entrances,
    /// exits, and midway points"). In use is the highest a level defines or
    /// a screen exit in Lunar Magic's format leads to
    /// ([`Object::lunar_magic_entrance`]), so that no exit reads past the
    /// tables; at most [`tables::MAX_ENTRANCES`], past which a build
    /// refuses the entrance.
    pub fn entrance_count(&self) -> u16 {
        self.levels
            .iter()
            .flat_map(|(_, level)| {
                let defined = level.entrances.iter().map(|e| e.id);
                defined.chain(exit_entrances(level))
            })
            .map(|id| id.saturating_add(1).min(tables::MAX_ENTRANCES))
            .fold(tables::ENTRANCE_COUNT, u16::max)
    }

    /// Whether a level has a secondary entrance past `1FF`, or a screen
    /// exit to one, which need the entrance tables moved to hold
    /// [`Project::entrance_count`] ([`install::ENTRANCES`]).
    pub fn many_entrances(&self) -> bool {
        self.entrance_count() > tables::ENTRANCE_COUNT
    }

    /// Whether the project has anything only Lunar Magic's layout holds,
    /// which needs Kobo's code for it installed: Map16 pages past 1, or
    /// Lunar Magic's objects, a background of its own, a level size, entrance settings,
    /// exits in its format, or a sprite list only its loader reads in a
    /// level. A build also installs it for more sprite data than bank `$07`
    /// has room for ([`Project::installs_lunar_magic`]).
    pub fn lunar_magic_layout(&self) -> bool {
        self.lunar_magic_graphics()
            || self.exanimation()
            || !self.map16.is_empty()
            || !self.map16_bg.is_empty()
            || self.map16_game.iter().any(|(_, page)| {
                page.tiles
                    .iter()
                    .any(|(&tile, t)| t.acts.is_some_and(|a| a != tile))
            })
            || self.tileset_page2()
            || self.manifest.gps.is_some()
            || self.manifest.pixi.is_some()
            || self.pixi_compiled.is_some()
            || self.levels.iter().any(|(number, level)| {
                level.entrances.iter().any(|e| e.id >> 8 != number >> 8)
                    || level.settings != LevelSettings::default()
                    // Lunar Magic's added layer 2 scroll settings, which
                    // its tables and camera give.
                    || (8..12).contains(&level.entrance.layer2_scroll)
                    || !level.size.is_default()
                    || level
                        .entrances
                        .iter()
                        .any(|e| e.settings != Default::default())
                    || level
                        .layer1
                        .iter()
                        .any(|o| matches!(o, Object::ScreenExit(e) if e.flags & 0x04 != 0))
                    || level
                        .sprites
                        .needs_lunar_magic(level.header.level_mode.layer1_vertical())
            })
            || self.levels.iter().any(|(_, level)| {
                level.layer1.iter().any(handled)
                    || level.palette.is_some()
                    || matches!(&level.layer2, Layer2::Background(_))
                    || matches!(&level.layer2, Layer2::Objects(list) if list.iter().any(handled))
            })
    }
}

impl Project {
    /// Whether the build writes one of levels `0CD`-`0CF`, through whose
    /// screen exits Choc Island 2 loads its rooms with those levels' banks,
    /// so it gives the rooms their own ([`install::CHOC_ISLAND`]).
    pub fn choc_island_rooms(&self) -> bool {
        self.levels.iter().any(|(n, _)| (0xCD..=0xCF).contains(n))
    }

    /// Whether a build on `clean` installs Kobo's code for Lunar Magic's
    /// layout: when the project has anything only that layout holds, or
    /// when its changed sprite lists would not fit bank `$07`, since the
    /// layout's sprite data banks let a list be anywhere.
    pub fn installs_lunar_magic(&self, clean: &Rom) -> bool {
        self.lunar_magic_layout() || !self.sprite_lists_fit_bank_07(Some(clean))
    }

    /// Whether the levels' sprite lists that differ from `clean`'s (all of
    /// them, without it) would fit in bank `$07`'s unused space, placed
    /// first-fit as a build places them.
    pub fn sprite_lists_fit_bank_07(&self, clean: Option<&Rom>) -> bool {
        let mut free: Vec<u32> = BANK_07_FREE
            .iter()
            .map(|&(start, end)| (if end == 0 { 0x1_0000 } else { end as u32 }) - start as u32)
            .collect();
        self.levels.iter().all(|(number, level)| {
            let vertical = level.header.level_mode.layer1_vertical();
            let (header, entries) = level.sprites.to_entries(vertical, false);
            // A list the encoder refuses fails the build anyway.
            let Ok(list) = sprites::encode(header, &entries, None) else {
                return true;
            };
            if clean.is_some_and(|clean| same_list(clean, *number, &list)) {
                return true;
            }
            match free.iter_mut().find(|n| **n as usize >= list.len()) {
                Some(n) => {
                    *n -= list.len() as u32;
                    true
                }
                None => false,
            }
        })
    }

    /// Whether page 2 is per object tileset: when a tileset file lists a
    /// tile of it.
    pub fn tileset_page2(&self) -> bool {
        self.map16_tileset
            .iter()
            .any(|(_, page)| page.tiles.keys().any(|&t| t >= 0x200))
    }
}

/// The build's stages, in the order they run (docs/step-2.md). The ones
/// of the plan still to come take their places between these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// The clean ROM, expanded to the project's size.
    Base,
    /// The blocks of a compiled PIXI insert, where PIXI put them in the
    /// hack, before anything else takes space: its code is not
    /// relocatable.
    SpriteBlocks,
    /// Kobo's code for Lunar Magic's layout, if the project uses it.
    Install,
    /// The project's early Asar patches.
    EarlyPatches,
    /// AddmusicK, with the project's music.
    Music,
    /// GFX files `00` to `33`.
    Graphics,
    /// Map16 pages past 1 and the acts-like tables.
    Map16,
    /// PIXI, with the project's sprites, or a compiled insert's sites:
    /// its size table sets how long each sprite entry of a level is.
    Sprites,
    /// GPS, with the project's blocks. It rewrites the acts-like table.
    Blocks,
    /// UberASM Tool, with the project's UberASM files, after PIXI and GPS,
    /// as it reads what PIXI leaves (docs/toolchain.md).
    UberAsm,
    /// The project's late Asar patches.
    LatePatches,
    /// The levels the project defines.
    Levels,
}

impl Stage {
    pub const ALL: [Stage; 12] = [
        Stage::Base,
        Stage::SpriteBlocks,
        Stage::Install,
        Stage::EarlyPatches,
        Stage::Music,
        Stage::Graphics,
        Stage::Map16,
        Stage::Sprites,
        Stage::Blocks,
        Stage::UberAsm,
        Stage::LatePatches,
        Stage::Levels,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::Base => "base",
            Stage::SpriteBlocks => "sprite blocks",
            Stage::Install => "install",
            Stage::EarlyPatches => "early patches",
            Stage::Music => "music",
            Stage::Graphics => "graphics",
            Stage::Sprites => "sprites",
            Stage::Blocks => "blocks",
            Stage::Map16 => "map16",
            Stage::UberAsm => "uberasm",
            Stage::LatePatches => "late patches",
            Stage::Levels => "levels",
        }
    }

    fn patches(self, project: &Project) -> &[PathBuf] {
        match self {
            Stage::EarlyPatches => &project.manifest.early_patches,
            Stage::LatePatches => &project.manifest.late_patches,
            _ => &[],
        }
    }

    /// Changes whenever what the stage writes for the same inputs does, so
    /// no snapshot of an older version is reused.
    fn version(self) -> u32 {
        1
    }

    /// The stage's inputs, as bytes that differ whenever its output would.
    /// A patch's inputs are every file in its folder and below, since a
    /// patch can include any of them; a tool's are its whole folder.
    fn inputs(self, clean: &Rom, project: &Project) -> Result<Vec<u8>, BuildError> {
        Ok(match self {
            Stage::Base => {
                let mut hash = Sha1::new();
                hash.update(clean.sha1());
                hash.update((rom_size(clean, project) as u64).to_le_bytes());
                hash.update([project.manifest.lz3 as u8]);
                if project.manifest.sa1 {
                    Tool::Sa1Pack.locate()?.hash_into(&mut hash)?;
                    Tool::Asar.locate()?.hash_into(&mut hash)?;
                }
                hash.finalize().to_vec()
            }
            Stage::Install => {
                let layout = project.installs_lunar_magic(clean);
                let choc = project.choc_island_rooms();
                if !layout && !choc {
                    return Ok(Vec::new());
                }
                let mut hash = Sha1::new();
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                let lunar_magic = if layout { install::LUNAR_MAGIC } else { &[] };
                let entrances = (layout && project.many_entrances()).then_some(&install::ENTRANCES);
                let graphics =
                    (layout && project.lunar_magic_graphics()).then_some(&install::GRAPHICS);
                let animation = (layout && project.exanimation()).then_some(&install::EXANIMATION);
                let layer3 = (layout && project.lunar_magic_layer3()).then_some(&install::LAYER3);
                let choc = choc.then_some(&install::CHOC_ISLAND);
                for (name, text) in std::iter::once(&install::MEMORY)
                    .chain(lunar_magic)
                    .chain(entrances)
                    .chain(graphics)
                    .chain(animation)
                    .chain(layer3)
                    .chain(choc)
                {
                    hash.update(name.as_bytes());
                    hash.update([0]);
                    hash.update(text.as_bytes());
                }
                if entrances.is_some() {
                    hash.update(project.entrance_count().to_le_bytes());
                }
                hash.finalize().to_vec()
            }
            // The pixels' indexes and the size: the palette is only for
            // viewing, and a build does not read it.
            Stage::Graphics => {
                let mut bytes = vec![
                    project.manifest.lz3 as u8,
                    u8::from(project.lunar_magic_graphics()),
                ];
                for (index, image) in &project.gfx {
                    bytes.push(*index);
                    bytes.extend(image.width.to_le_bytes());
                    bytes.extend(image.height.to_le_bytes());
                    bytes.extend(&image.pixels);
                }
                for (number, data) in &project.exgfx {
                    bytes.extend(number.to_le_bytes());
                    bytes.extend((data.len() as u64).to_le_bytes());
                    bytes.extend(data);
                }
                for (n, files) in &project.manifest.bypass_lists {
                    bytes.push(*n);
                    bytes.extend(files);
                }
                bytes
            }
            Stage::Map16 => {
                let mut bytes = Vec::new();
                let mut files = Vec::new();
                for (kind, list) in [
                    (PageKind::Foreground, &project.map16),
                    (PageKind::Background, &project.map16_bg),
                    (PageKind::Tileset, &project.map16_tileset),
                ] {
                    for (page, tiles) in list {
                        files.push((*page, kind as u8, tiles.to_toml(kind, &Default::default())));
                    }
                }
                for (page, tiles) in &project.map16_game {
                    files.push((*page, u8::MAX, tiles.to_toml(&Default::default())));
                }
                if !project.pipes.is_empty() {
                    files.push((0, u8::MAX - 1, project.pipes.to_toml(&[])));
                }
                for (page, kind, text) in files {
                    bytes.push(page);
                    bytes.push(kind);
                    bytes.extend((text.len() as u64).to_le_bytes());
                    bytes.extend(text.as_bytes());
                }
                bytes
            }
            Stage::EarlyPatches | Stage::LatePatches => {
                let patches = self.patches(project);
                if patches.is_empty() {
                    return Ok(Vec::new());
                }
                let mut hash = Sha1::new();
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                for patch in patches {
                    hash.update(patch.to_string_lossy().as_bytes());
                    hash.update([0]);
                    let folder = project.root.join(patch);
                    let folder = folder.parent().unwrap_or(&project.root);
                    tools::hash_tree(&mut hash, folder)?;
                }
                // Asar also looks for included files from the project's
                // folder, so what a patch can include from there is hashed.
                tools::hash_tree_except(&mut hash, &project.root, &|path| {
                    !patch_can_include(project, path)
                })?;
                hash.finalize().to_vec()
            }
            Stage::Music => {
                let Some(music) = &project.manifest.music else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                Tool::AddmusicK.locate()?.hash_into(&mut hash)?;
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                tools::hash_tree(&mut hash, &project.root.join(music))?;
                hash_callisto(&mut hash, project)?;
                hash.finalize().to_vec()
            }
            Stage::SpriteBlocks => {
                let Some(compiled) = &project.pixi_compiled else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                for (at, bytes) in &compiled.insert.blocks {
                    hash.update(at.raw().to_le_bytes());
                    hash.update((bytes.len() as u64).to_le_bytes());
                    hash.update(bytes);
                }
                hash.finalize().to_vec()
            }
            Stage::Sprites => {
                if let Some(compiled) = &project.pixi_compiled {
                    let mut bytes = vec![u8::from(compiled.insert.sprites_255)];
                    if let Some(table) = &compiled.insert.size_table {
                        bytes.extend(b"sizes");
                        bytes.extend(table);
                    }
                    for (at, site) in &compiled.insert.sites {
                        bytes.extend(at.raw().to_le_bytes());
                        bytes.extend((site.len() as u64).to_le_bytes());
                        bytes.extend(site);
                    }
                    return Ok(bytes);
                }
                let Some(files) = &project.manifest.pixi else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                Tool::Pixi
                    .locate_version(project.manifest.pixi_version.as_deref())?
                    .hash_into(&mut hash)?;
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                tools::hash_tree(&mut hash, &project.root.join(files))?;
                hash_callisto(&mut hash, project)?;
                hash.finalize().to_vec()
            }
            Stage::Blocks => {
                let Some(files) = &project.manifest.gps else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                Tool::Gps.locate()?.hash_into(&mut hash)?;
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                tools::hash_tree(&mut hash, &project.root.join(files))?;
                hash_callisto(&mut hash, project)?;
                hash.finalize().to_vec()
            }
            Stage::UberAsm => {
                let Some(files) = &project.manifest.uberasm else {
                    return Ok(Vec::new());
                };
                let mut hash = Sha1::new();
                Tool::UberAsm.locate()?.hash_into(&mut hash)?;
                Tool::Asar.locate()?.hash_into(&mut hash)?;
                tools::hash_tree(&mut hash, &project.root.join(files))?;
                hash_callisto(&mut hash, project)?;
                hash.finalize().to_vec()
            }
            Stage::Levels => {
                let mut bytes = Vec::new();
                for (number, level) in &project.levels {
                    bytes.extend(number.to_le_bytes());
                    let text = level.to_toml(&Default::default());
                    bytes.extend((text.len() as u64).to_le_bytes());
                    bytes.extend(text.as_bytes());
                }
                // ExAnimation's global list and files, written with the levels.
                if let Some(list) = &project.animation_global {
                    let data = list.to_bytes();
                    bytes.extend(b"global");
                    bytes.extend((data.len() as u64).to_le_bytes());
                    bytes.extend(data);
                }
                for (number, data) in &project.animation_files {
                    bytes.extend(number.to_le_bytes());
                    bytes.extend((data.len() as u64).to_le_bytes());
                    bytes.extend(data);
                }
                bytes
            }
        })
    }

    /// [`Stage::run`], with a compiled PIXI insert's free bytes masked
    /// while Asar or a tool runs ([`crate::pixi::mask_blocks`]).
    fn run_guarded(self, rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
        let runs_asar = matches!(
            self,
            Stage::Install
                | Stage::EarlyPatches
                | Stage::Music
                | Stage::Blocks
                | Stage::UberAsm
                | Stage::LatePatches
        );
        let Some(compiled) = project.pixi_compiled.as_ref().filter(|_| runs_asar) else {
            return self.run(rom, clean, project);
        };
        crate::pixi::mask_blocks(rom, &compiled.insert)?;
        self.run(rom, clean, project)?;
        crate::pixi::unmask_blocks(rom, &compiled.insert)?;
        Ok(())
    }

    fn run(self, rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
        let callisto_root = match self {
            Stage::EarlyPatches
            | Stage::LatePatches
            | Stage::Music
            | Stage::Sprites
            | Stage::Blocks
            | Stage::UberAsm => CallistoRoot::make(project)?,
            _ => None,
        };
        let callisto = callisto_root.as_ref().map(|c| c.header());
        match self {
            Stage::Base => {
                if project.manifest.sa1 {
                    if project.manifest.lz3 {
                        // SA-1 Pack puts in the decompressor this says;
                        // Lunar Magic reads it only with its settings
                        // marked present (docs/lunar-magic-install.md).
                        rom.write_u8(gfx::COMPRESSION_SETTING, gfx::COMPRESSION_LZ3)?;
                        rom.write_u8(gfx::SETTINGS_PRESENT, 0x00)?;
                    }
                    *rom = apply_sa1pack(rom, rom_size(clean, project))?;
                }
                rom.expand(rom_size(clean, project))?;
            }
            Stage::SpriteBlocks => {
                if let Some(compiled) = &project.pixi_compiled {
                    crate::pixi::write_blocks(rom, &compiled.insert)
                        .map_err(|e| BuildError::Pixi(e.to_string()))?;
                }
            }
            Stage::Install => {
                if project.installs_lunar_magic(clean) {
                    let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
                    *rom = install::apply_lunar_magic(&asar, rom)
                        .map_err(|e| BuildError::Asar(Box::new(e)))?;
                    if project.many_entrances() {
                        let count = project.entrance_count();
                        *rom =
                            install::apply_entrances(&asar, rom, count).map_err(|e| match e {
                                install::InstallError::Asar(e) => BuildError::Asar(Box::new(e)),
                                e => BuildError::Install {
                                    message: e.to_string(),
                                },
                            })?;
                    }
                    if project.lunar_magic_graphics() {
                        *rom = install::apply_graphics(&asar, rom)
                            .map_err(|e| BuildError::Asar(Box::new(e)))?;
                    }
                    if project.exanimation() {
                        *rom = install::apply_exanimation(&asar, rom)
                            .map_err(|e| BuildError::Asar(Box::new(e)))?;
                    }
                    if project.lunar_magic_layer3() {
                        *rom = install::apply_layer3(&asar, rom)
                            .map_err(|e| BuildError::Asar(Box::new(e)))?;
                    }
                }
                if project.choc_island_rooms() {
                    let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
                    *rom = install::apply_choc_island(&asar, rom)
                        .map_err(|e| BuildError::Asar(Box::new(e)))?;
                }
            }
            Stage::Graphics => {
                write_gfx(rom, clean, project)?;
                if project.lunar_magic_graphics() {
                    write_exgfx(rom, project)?;
                }
            }
            Stage::Map16 => {
                write_game_map16(rom, clean, project)?;
                write_pipes(rom, project)?;
                write_tileset_page2(rom, project)?;
                write_map16(rom, project)?;
                write_map16_bg(rom, clean, project)?;
            }
            Stage::EarlyPatches | Stage::LatePatches => {
                let patches = self.patches(project);
                if patches.is_empty() {
                    return Ok(());
                }
                let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
                for patch in patches {
                    let path = project.root.join(patch);
                    let mut spec = Patch::new(&path).include_path(&project.root);
                    // Callisto's header, where the patches find it by name.
                    if let Some(root) = &callisto_root {
                        spec = spec.include_path(root.path());
                    }
                    let patched = asar.patch(rom, &spec).map_err(|source| BuildError::Patch {
                        path: path.clone(),
                        source: Box::new(source),
                    })?;
                    *rom = patched.rom;
                }
            }
            Stage::Music => {
                if let Some(music) = &project.manifest.music {
                    let tool = Tool::AddmusicK.locate()?.path;
                    let asar = Tool::Asar.locate()?.path;
                    *rom = tools::addmusick(
                        rom,
                        &tool,
                        &project.root.join(music),
                        &asar,
                        callisto.as_deref(),
                    )?;
                }
            }
            Stage::Sprites => {
                if let Some(compiled) = &project.pixi_compiled {
                    crate::pixi::write_sites(rom, &compiled.insert)?;
                    if let Some(table) = &compiled.insert.size_table {
                        crate::pixi::write_size_table(rom, table)
                            .map_err(|e| BuildError::Pixi(e.to_string()))?;
                    }
                }
                if let Some(files) = &project.manifest.pixi {
                    let tool = Tool::Pixi
                        .locate_version(project.manifest.pixi_version.as_deref())?
                        .path;
                    let asar = Tool::Asar.locate()?.path;
                    *rom = tools::run_pixi(
                        rom,
                        &tool,
                        &project.root.join(files),
                        &asar,
                        callisto.as_deref(),
                    )?;
                }
            }
            Stage::Blocks => {
                if let Some(files) = &project.manifest.gps {
                    let tool = Tool::Gps.locate()?.path;
                    let asar = Tool::Asar.locate()?.path;
                    *rom = tools::gps(
                        rom,
                        &tool,
                        &project.root.join(files),
                        &asar,
                        callisto.as_deref(),
                    )?;
                }
            }
            Stage::UberAsm => {
                if let Some(files) = &project.manifest.uberasm {
                    let tool = Tool::UberAsm.locate()?.path;
                    let asar = Tool::Asar.locate()?.path;
                    *rom = tools::uberasm(
                        rom,
                        &tool,
                        &project.root.join(files),
                        &asar,
                        callisto.as_deref(),
                    )?;
                }
            }
            Stage::Levels => {
                // Sprite lists are compared with, and bank $07's space
                // taken from, the image the stage starts from: SA-1 Pack
                // has changed the clean ROM's by then.
                let before = Rom::from_bytes(rom.data().to_vec())?;
                let mut space = FreeSpace::scan(rom);
                let mut bank07 = Bank07::new(&before);
                for (number, level) in &project.levels {
                    write_level(rom, &before, &mut space, &mut bank07, *number, level)?;
                }
                write_entrances(rom, project)?;
                check_exits(rom, project)?;
                if project.lunar_magic_layout() {
                    write_level_settings(rom, project)?;
                    write_level_sizes(rom, project)?;
                }
                if project.lunar_magic_graphics() {
                    write_graphics_lists(rom, project)?;
                }
                if project.exanimation() {
                    write_animation(rom, project)?;
                }
            }
        }
        Ok(())
    }
}

/// A scratch copy of what a project with `[callisto]` can include, the
/// root Callisto's header names: every file a patch may include
/// ([`patch_can_include`]), with `callisto.asm` at its root and every
/// include of it by name pointed there ([`tools::point_callisto_includes`]),
/// as Callisto's own Asar would find it from anywhere. The patches find it
/// as an include path.
struct CallistoRoot {
    scratch: tools::Scratch,
}

impl CallistoRoot {
    fn make(project: &Project) -> Result<Option<Self>, BuildError> {
        if project.manifest.callisto.is_none() {
            return Ok(None);
        }
        let scratch = tools::Scratch::new("callisto")?;
        let root = std::path::absolute(&scratch.0).unwrap_or_else(|_| scratch.0.clone());
        tools::copy_tree_where(&project.root, &root, &|path| {
            patch_can_include(project, path)
        })?;
        let header = root.join(tools::CALLISTO_HEADER);
        tools::point_callisto_includes(&root, &header)?;
        let text = callisto_header(project, &root).expect("[callisto] is set");
        fs::write(&header, text).map_err(|source| BuildError::Io {
            path: header.clone(),
            source,
        })?;
        Ok(Some(Self { scratch }))
    }

    fn path(&self) -> &Path {
        &self.scratch.0
    }

    /// The header's file.
    fn header(&self) -> PathBuf {
        std::path::absolute(&self.scratch.0)
            .unwrap_or_else(|_| self.scratch.0.clone())
            .join(tools::CALLISTO_HEADER)
    }
}

/// Callisto's `callisto.asm` for a project with `[callisto]`, which the
/// patches and the tools' files of a project imported from Callisto
/// include by name: what Callisto's documentation (v0.6.2) says the one it
/// generates gives them. `CALLISTO_ASSEMBLING` and the version, the
/// project's own header (`header`, Callisto's `callisto_header`), and
/// `incsrc_file` and `incbin_file`, which include a file by its path from
/// `root`, a copy of the project's root ([`CallistoRoot`]). It names the
/// root by its full path, which is only ever in the build's scratch
/// copies, never in the ROM.
pub fn callisto_header(project: &Project, root: &Path) -> Option<String> {
    let callisto = project.manifest.callisto.as_ref()?;
    let path = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut out = String::from(
        "; Callisto's callisto.asm, as Kobo gives it to a project with [callisto]\n\
         ; (kobo.toml). Written for each build; do not edit.\n\
         if not(defined(\"CALLISTO_INCLUDED\"))\n\
         !CALLISTO_INCLUDED = 1\n\
         !CALLISTO_ASSEMBLING = 1\n\
         !CALLISTO_VERSION = \"0.6.2\"\n\
         !CALLISTO_VERSION_MAJOR = 0\n\
         !CALLISTO_VERSION_MINOR = 6\n\
         !CALLISTO_VERSION_PATCH = 2\n",
    );
    out.push_str(&format!("!CALLISTO_ROOT = \"{}\"\n", path(root)));
    if let Some(header) = &callisto.header {
        out.push_str(&format!(
            "!CALLISTO_HEADER = \"{}\"\nincsrc \"!CALLISTO_HEADER\"\n",
            path(&root.join(header))
        ));
    }
    out.push_str(
        "macro incsrc_file(file_path)\n\
         \tincsrc \"!CALLISTO_ROOT/<file_path>\"\n\
         endmacro\n\
         macro incbin_file(file_path)\n\
         \tincbin \"!CALLISTO_ROOT/<file_path>\"\n\
         endmacro\n\
         endif\n",
    );
    Some(out)
}

/// What a tool's files of a project with `[callisto]` can also read:
/// through `incsrc_file`, any file a patch can include.
fn hash_callisto(hash: &mut Sha1, project: &Project) -> Result<(), BuildError> {
    if project.manifest.callisto.is_some() {
        hash.update(b"callisto");
        tools::hash_tree_except(hash, &project.root, &|path| {
            !patch_can_include(project, path)
        })?;
    }
    Ok(())
}

/// Whether a file of the project's folder, by its path relative to it, is
/// one a patch may include: not hidden (`.git`), not a ROM or patch image
/// (the build's own output among them), and not a file of the manifest or
/// one it gives another stage, whose changes would otherwise run the
/// patches, and the tools after them, again.
fn patch_can_include(project: &Project, path: &Path) -> bool {
    let hidden = path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    let image = path.extension().is_some_and(|e| {
        ["sfc", "smc", "bps", "ips"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    });
    let m = &project.manifest;
    let owned = path == Path::new(MANIFEST)
        || m.levels.values().any(|file| path == file)
        || [&m.map16, &m.map16_bg, &m.gfx]
            .iter()
            .any(|files| files.values().any(|file| path == file))
        || m.exgfx.values().any(|file| path == file.path)
        || m.animation_global.as_deref() == Some(path)
        || m.animation_files.values().any(|file| path == file)
        || m.music.as_ref().is_some_and(|dir| path.starts_with(dir))
        || m.uberasm.as_ref().is_some_and(|dir| path.starts_with(dir))
        || m.gps.as_ref().is_some_and(|dir| path.starts_with(dir))
        || m.pixi.as_ref().is_some_and(|dir| path.starts_with(dir))
        || project
            .pixi_compiled
            .as_ref()
            .is_some_and(|c| c.files.iter().any(|file| path == file));
    !(hidden || image || owned)
}

/// SA-1 Pack on the clean ROM, and its 6 or 8 MiB patch for a larger
/// image, run through Asar from SA-1 Pack's folder: the pinned release,
/// whose patches are at the top, or a configured folder of either that
/// or the repository's layout (`asm/`). SA-1 Pack applies to a clean ROM
/// only, before anything else (docs/toolchain.md).
fn apply_sa1pack(rom: &Rom, size: usize) -> Result<Rom, BuildError> {
    let folder = Tool::Sa1Pack.locate()?.path;
    let dir = if folder.join("sa1.asm").is_file() {
        folder
    } else {
        folder.join("asm")
    };
    let asar = Asar::configured().map_err(|e| BuildError::Asar(Box::new(e)))?;
    let mut rom = Rom::from_headerless(rom.data().to_vec())?;
    let mut patches = vec![dir.join("sa1.asm")];
    match size {
        s if s > 0x60_0000 => patches.push(dir.join("8mb.asm")),
        s if s > 0x40_0000 => patches.push(dir.join("6mb.asm")),
        _ => {}
    }
    for path in patches {
        let patched = asar
            .patch(&rom, &Patch::new(&path))
            .map_err(|source| BuildError::Patch {
                path: path.clone(),
                source: Box::new(source),
            })?;
        rom = patched.rom;
    }
    Ok(rom)
}

/// The size the project expands the ROM to: its own, or
/// [`DEFAULT_ROM_SIZE`] if it writes anything, for the free space its
/// levels take and the space AddmusicK requires past 512 KiB.
fn rom_size(clean: &Rom, project: &Project) -> usize {
    let m = &project.manifest;
    let writes = !project.levels.is_empty()
        || !project.gfx.is_empty()
        || project.lunar_magic_graphics()
        || project.lunar_magic_layout()
        || m.sa1
        || !m.early_patches.is_empty()
        || !m.late_patches.is_empty()
        || m.music.is_some()
        || m.uberasm.is_some()
        || m.gps.is_some()
        || m.pixi.is_some()
        || m.pixi_compiled.is_some();
    m.rom_size.unwrap_or(if writes {
        DEFAULT_ROM_SIZE
    } else {
        clean.len()
    })
}

/// What a build of `project` writes but a shell should warn of: each
/// graphics list slot naming an ExGFX file the project does not have, which
/// loads nothing, as in a ROM without the file (hacks have such lists, so a
/// build takes them; a hand-written project may have forgotten a file).
pub fn warnings(project: &Project) -> Vec<String> {
    let mut out = Vec::new();
    for (number, level) in &project.levels {
        let Some(list) = &level.graphics else {
            continue;
        };
        for (slot, file) in list.files() {
            if file >= exgfx::EXGFX_FIRST && !project.exgfx.iter().any(|(n, _)| *n == file) {
                out.push(format!(
                    "level {number:03X}: its graphics list's {} names ExGFX{file:X}, which the \
                     project does not have, so the slot loads nothing",
                    GraphicsList::SLOTS[slot]
                ));
            }
        }
    }
    out
}

/// The tools a build of `project` runs, in the order it first runs them.
pub fn tools(project: &Project) -> Vec<Tool> {
    let m = &project.manifest;
    let mut used = Vec::new();
    if m.sa1 {
        used.push(Tool::Sa1Pack);
    }
    // Without the clean ROM, every sprite list counts as changed.
    let asar = m.sa1
        || project.lunar_magic_layout()
        || project.choc_island_rooms()
        || !project.sprite_lists_fit_bank_07(None)
        || !m.early_patches.is_empty()
        || !m.late_patches.is_empty()
        || m.music.is_some()
        || m.pixi.is_some()
        || m.gps.is_some()
        || m.uberasm.is_some();
    if asar {
        used.push(Tool::Asar);
    }
    for (set, tool) in [
        (m.music.is_some(), Tool::AddmusicK),
        (m.pixi.is_some(), Tool::Pixi),
        (m.gps.is_some(), Tool::Gps),
        (m.uberasm.is_some(), Tool::UberAsm),
    ] {
        if set {
            used.push(tool);
        }
    }
    used
}

/// Finds every tool a build of `project` runs ([`tools`]), fetching the
/// pinned builds it has not got. A tool a user configured makes the build
/// depend on their copy, which [`Located::note`] says.
pub fn locate_tools(project: &Project) -> Result<Vec<Located>, BuildError> {
    Ok(tools(project)
        .into_iter()
        .map(|tool| match tool {
            Tool::Pixi => tool.locate_version(project.manifest.pixi_version.as_deref()),
            _ => tool.locate(),
        })
        .collect::<Result<_, _>>()?)
}

/// Each stage's key, chained from the one before.
pub fn stage_keys(clean: &Rom, project: &Project) -> Result<Vec<[u8; 20]>, BuildError> {
    let mut key = [0u8; 20];
    Stage::ALL
        .iter()
        .map(|&stage| {
            let mut hash = Sha1::new();
            hash.update(key);
            hash.update(stage.name());
            hash.update(stage.version().to_le_bytes());
            hash.update(env!("CARGO_PKG_VERSION"));
            hash.update(stage.inputs(clean, project)?);
            key = hash.finalize().into();
            Ok(key)
        })
        .collect()
}

/// Snapshots of the image after each stage, one file per key.
#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `KOBO_CACHE_DIR`, or `kobo/stages` in the user's cache directory.
    pub fn user() -> Option<Self> {
        match std::env::var_os("KOBO_CACHE_DIR").filter(|d| !d.is_empty()) {
            Some(dir) => Some(Self::new(dir)),
            None => dirs::cache_dir().map(|d| Self::new(d.join("kobo").join("stages"))),
        }
    }

    fn path(&self, key: &[u8; 20]) -> PathBuf {
        let name: String = key.iter().map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{name}.bin"))
    }

    fn get(&self, key: &[u8; 20]) -> Option<Vec<u8>> {
        fs::read(self.path(key)).ok()
    }

    fn put(&self, key: &[u8; 20], data: &[u8]) -> Result<(), BuildError> {
        let path = self.path(key);
        let fail = |source| BuildError::Cache {
            path: path.clone(),
            source,
        };
        fs::create_dir_all(&self.dir).map_err(fail)?;
        // Written whole under another name first, so a reader never sees
        // half a snapshot.
        let partial = path.with_extension(format!("{}.part", std::process::id()));
        fs::write(&partial, data).map_err(fail)?;
        fs::rename(&partial, &path).map_err(fail)
    }
}

/// The image a project with this manifest builds onto: the clean ROM
/// after the base stage (SA-1 Pack if the manifest says so, and the
/// expansion). Import compares a ROM with it to find what changed.
pub fn base_image(clean: &Rom, manifest: &Manifest) -> Result<Rom, BuildError> {
    let project = Project {
        root: PathBuf::from("."),
        manifest: manifest.clone(),
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    };
    let mut rom = Rom::from_bytes(clean.data().to_vec())?;
    Stage::Base.run(&mut rom, clean, &project)?;
    rom.fix_checksum()?;
    Ok(rom)
}

/// Builds a project onto a copy of the clean ROM, which must be the
/// vanilla image.
pub fn build(clean: &Rom, project: &Project) -> Result<Rom, BuildError> {
    build_cached(clean, project, None)
}

/// [`build`], reusing and keeping snapshots in `cache`.
pub fn build_cached(
    clean: &Rom,
    project: &Project,
    cache: Option<&Cache>,
) -> Result<Rom, BuildError> {
    if clean.identify() != RomIdentity::VanillaUsa {
        return Err(BuildError::NotClean(clean.sha1_hex()));
    }
    build_on(clean, project, cache)
}

/// [`build_cached`] on any base image laid out as the vanilla ROM is, for
/// synthetic images in tests.
pub fn build_on(base: &Rom, project: &Project, cache: Option<&Cache>) -> Result<Rom, BuildError> {
    if project.manifest.lz3 && !project.manifest.sa1 {
        return Err(BuildError::Lz3WithoutSa1);
    }
    check_map16(base, project)?;
    // Keys hash every tool's folder, so they are only made for a cache.
    let keys = match cache {
        Some(_) => stage_keys(base, project)?,
        None => Vec::new(),
    };
    // The last stage with a snapshot, and the image it holds.
    let resumed = cache.and_then(|cache| {
        (0..keys.len())
            .rev()
            .find_map(|i| Some((i + 1, cache.get(&keys[i])?)))
    });
    let (first, mut rom) = match resumed {
        Some((next, data)) => (next, Rom::from_headerless(data)?),
        None => (0, Rom::from_headerless(base.data().to_vec())?),
    };
    for (i, stage) in Stage::ALL.iter().enumerate().skip(first) {
        stage.run_guarded(&mut rom, base, project)?;
        if let Some(cache) = cache {
            cache.put(&keys[i], rom.data())?;
        }
    }
    rom.fix_checksum()?;
    Ok(rom)
}

/// The image a build of the project leaves after `last`, for a check that
/// needs only the stages up to it: an import runs PIXI so, to compare the
/// size table it makes with a hack's.
pub fn build_through(clean: &Rom, project: &Project, last: Stage) -> Result<Rom, BuildError> {
    if clean.identify() != RomIdentity::VanillaUsa {
        return Err(BuildError::NotClean(clean.sha1_hex()));
    }
    let mut rom = Rom::from_headerless(clean.data().to_vec())?;
    for stage in Stage::ALL {
        stage.run_guarded(&mut rom, clean, project)?;
        if stage == last {
            break;
        }
    }
    Ok(rom)
}

/// Writes the project's Map16 pages into tables of their own, one for each
/// group of 16 pages the project uses, whole (Lunar Magic stops at a
/// group's last used page, and keeps a whole group when it saves), and
/// what their tiles act like into the acts-like tables: the one Kobo's
/// install made for tiles below `$4000`, and one made here for the rest
/// when a page needs it.
fn write_map16(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16.is_empty() {
        return Ok(());
    }
    let mut space = FreeSpace::scan(rom);
    let pages: BTreeMap<u8, &Map16Page> = project.map16.iter().map(|(n, p)| (*n, p)).collect();
    let tiles_of = |page: u8| {
        let first = page as u16 * page_source::PAGE_TILES;
        first..first + page_source::PAGE_TILES
    };
    let lower = SnesAddr::new(rom.read_u24(map16_pages::ACTS_LIKE)?);
    let mut upper = None;
    for (&page, tiles) in &pages {
        for tile in tiles_of(page) {
            let acts = tiles.tile(tile).acts;
            let at = if tile < 0x4000 {
                lower.add(2 * tile as u32)
            } else {
                let table = match upper {
                    Some(table) => table,
                    None => {
                        let table = space.alloc(rom, 0x8000, Contents::Data)?;
                        let default = page_source::DEFAULT_ACTS.to_le_bytes();
                        rom.write(table, &default.repeat(0x4000))?;
                        rom.write_u24(map16_pages::ACTS_LIKE_UPPER, table.raw() - 0x8000)?;
                        upper = Some(table);
                        table
                    }
                };
                table.add(2 * (tile - 0x4000) as u32)
            };
            rom.write_u16(at, acts)?;
        }
    }
    // A group's table is whole, as Lunar Magic allocates it: its editor
    // shows every page of a group with a table, and reads them all.
    for group in &map16_pages::PAGE_GROUPS {
        if !group.pages().any(|p| pages.contains_key(&p)) {
            continue;
        }
        let mut bytes = Vec::with_capacity(group.pages().count() * 0x800);
        for page in group.pages() {
            for tile in tiles_of(page) {
                let entry = pages.get(&page).map(|p| p.tile(tile)).unwrap_or_default();
                bytes.extend(entry.gfx.to_bytes());
            }
        }
        let table = place(rom, &mut space, &bytes)?;
        let (pointer, bank) = group.stored_for(tiles_of(*group.pages().start()).start, table);
        rom.write_u16(group.pointer, pointer)?;
        rom.write_u8(group.bank, bank)?;
    }
    Ok(())
}

/// Refuses what the project's Map16 files say that the ROM cannot hold:
/// graphics in page 0 or 1's file for a tile the game keeps per object
/// tileset, a tile of pages 0 and 1 that is the same in every tileset in a
/// tileset file, tiles of pages 0 and 1 in the files of two tilesets that
/// share the game's table of their own tiles, and page 2's graphics in its
/// page file when page 2 is per tileset.
fn check_map16(clean: &Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16_game.is_empty() && project.map16_tileset.is_empty() {
        return Ok(());
    }
    let game = GameTables::read(clean)?;
    for (page, tiles) in &project.map16_game {
        if let Some(tile) = tiles
            .tiles
            .iter()
            .find(|(tile, t)| t.gfx.is_some() && game.is_specific(**tile))
            .map(|(&tile, _)| tile)
        {
            return Err(BuildError::Map16(format!(
                "page {page:02X}'s tile {tile:03X} has graphics of its own in each object \
                 tileset; they go in the tileset files ([map16_tileset])"
            )));
        }
    }
    for (tileset, tiles) in &project.map16_tileset {
        if let Some(&tile) = tiles
            .tiles
            .keys()
            .find(|&&t| t < 0x200 && !game.is_specific(t))
        {
            return Err(BuildError::Map16(format!(
                "tileset {tileset:X}'s file lists tile {tile:03X}, which is the same in every \
                 tileset; it goes in page {:02X}'s file",
                tile >> 8
            )));
        }
    }
    let listing: Vec<u8> = project
        .map16_tileset
        .iter()
        .filter(|(_, tiles)| tiles.tiles.keys().any(|&t| t < 0x200))
        .map(|(tileset, _)| *tileset)
        .collect();
    for group in game.sharing() {
        let listed: Vec<String> = group
            .iter()
            .filter(|t| listing.contains(t))
            .map(|t| format!("{t:X}"))
            .collect();
        if listed.len() > 1 {
            let all: Vec<String> = group.iter().map(|t| format!("{t:X}")).collect();
            return Err(BuildError::Map16(format!(
                "tilesets {} share the game's table of their own tiles of pages 0 and 1, so \
                 only one of their files may list those tiles; {} do",
                all.join(", "),
                listed.join(" and ")
            )));
        }
    }
    if project.tileset_page2()
        && let Some((_, page)) = project.map16.iter().find(|(p, _)| *p == 0x02)
        && page.tiles.values().any(|e| e.gfx != Default::default())
    {
        return Err(BuildError::Map16(
            "page 2 is per tileset, so its graphics go in the tileset files; page 02's file \
             holds only what its tiles act like"
                .into(),
        ));
    }
    Ok(())
}

/// Writes what the project changes of the vertical pipes' colours and the
/// diagonal pipes where the game keeps them, as Lunar Magic edits them.
fn write_pipes(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let tiles = project
        .pipes
        .colours
        .iter()
        .map(|(&(set, tile), gfx)| (Some(set), tile, gfx))
        .chain(
            project
                .pipes
                .diagonal
                .iter()
                .map(|(&tile, gfx)| (None, tile, gfx)),
        );
    for (set, tile, gfx) in tiles {
        let at = crate::map16::pipe_address(set, tile)
            .expect("the pipes file lists only the pipes' tiles");
        rom.write(at, &gfx.to_bytes())?;
    }
    Ok(())
}

/// Writes what the project changes of pages 0 and 1 over the game's own
/// tables, in place, as Lunar Magic does: graphics into the common table
/// or, for a tile the game keeps per tileset, into the table of the
/// tileset whose file lists it, which the tilesets sharing that table
/// read too; and what the tiles act like into the acts-like table Kobo's
/// install made.
fn write_game_map16(rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16_game.is_empty() && project.map16_tileset.is_empty() {
        return Ok(());
    }
    let game = GameTables::read(clean)?;
    let acts_table = if map16_pages::installed(rom) {
        Some(SnesAddr::new(rom.read_u24(map16_pages::ACTS_LIKE)?))
    } else {
        None
    };
    for (_, tiles) in &project.map16_game {
        for (&tile, entry) in &tiles.tiles {
            if let Some(gfx) = entry.gfx {
                rom.write(game.address(0, tile), &gfx.to_bytes())?;
            }
            if let (Some(acts), Some(table)) = (entry.acts, acts_table) {
                rom.write_u16(table.add(2 * tile as u32), acts)?;
            }
        }
    }
    for (tileset, tiles) in &project.map16_tileset {
        for (&tile, entry) in tiles.tiles.range(..0x200) {
            rom.write(game.address(*tileset, tile), &entry.gfx.to_bytes())?;
        }
    }
    Ok(())
}

/// Writes page 2 of every object tileset into one table, when the project
/// has it per tileset, points Lunar Magic's layout at it, and turns it on.
/// A tileset its files leave out has an empty page 2.
fn write_tileset_page2(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    if !project.tileset_page2() {
        return Ok(());
    }
    let pages: BTreeMap<u8, &Map16Page> =
        project.map16_tileset.iter().map(|(t, p)| (*t, p)).collect();
    let mut bytes = Vec::with_capacity(map16_pages::TILESET_PAGE2_LEN);
    for tileset in 0..crate::map16::TILESET_COUNT {
        for tile in 0x200..0x300 {
            let entry = pages
                .get(&tileset)
                .map(|p| p.tile(tile))
                .unwrap_or_default();
            bytes.extend(entry.gfx.to_bytes());
        }
    }
    let mut space = FreeSpace::scan(rom);
    let at = place(rom, &mut space, &bytes)?;
    let (pointer, bank) = map16_pages::tileset_page2_stored_for(at);
    rom.write_u16(map16_pages::TILESET_PAGE2_POINTER, pointer)?;
    rom.write_u8(map16_pages::TILESET_PAGE2_BANK, bank)?;
    rom.write_u8(map16_pages::TILESET_PAGE2, map16_pages::TILESET_PAGE2_ON)?;
    Ok(())
}

/// Writes the project's GFX files, each in the format the clean ROM has
/// it in, LC_LZ2, and points the game's tables at them. `GFX32` and `GFX33`
/// share a bank, so a project that has either writes both into one block.
///
/// With Lunar Magic's graphics formats ([`Project::lunar_magic_graphics`]),
/// every file the game keeps as 3bpp is written as 4bpp, a file the
/// project does not list as Lunar Magic converts the game's
/// ([`exgfx::stored_4bpp`]), and a listed one takes 16 colours.
fn write_gfx(rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
    let lz3 = project.manifest.lz3;
    let four_bpp = project.lunar_magic_graphics();
    if project.gfx.is_empty() && !lz3 && !four_bpp {
        return Ok(());
    }
    let reader = gfx::GfxReader::new(clean).map_err(|e| gfx_error("the clean ROM", &e))?;
    let mut space = FreeSpace::scan(rom);
    let mut files = std::collections::BTreeMap::new();
    let compress = |name: &str, data: &[u8]| {
        if lz3 {
            lz3::compress(data)
        } else {
            lz2::compress(data)
        }
        .map_err(|e| gfx_error(name, &e))
    };
    let listed: BTreeMap<u8, &IndexedImage> =
        project.gfx.iter().map(|(i, image)| (*i, image)).collect();
    // With LC_LZ3 the game's routine reads every file so, and the clean
    // ROM's are LC_LZ2: all of them are written again.
    for index in 0..gfx::GFX_FILE_COUNT {
        let name = format!("GFX{index:02X}");
        let vanilla = reader.read(index).map_err(|e| gfx_error(&name, &e))?;
        let converts = four_bpp && exgfx::converts(vanilla.format);
        let format = if converts {
            gfx::GfxFormat::Planar(gfx::Bpp::Four)
        } else {
            vanilla.format
        };
        let data = match listed.get(&index) {
            Some(image) => {
                let tiles = gfx::image_to_tiles(image, vanilla.tile_count(), format.colors())
                    .map_err(|e| gfx_error(&name, &e))?;
                format.encode(&tiles)
            }
            None if converts => exgfx::stored_4bpp(&vanilla),
            None if lz3 => vanilla.data.clone(),
            None => continue,
        };
        files.insert(index, compress(&name, &data)?);
    }
    for (&index, stream) in files.iter().filter(|(i, _)| **i < 0x32) {
        let at = place(rom, &mut space, stream)?;
        let i = index as u32;
        rom.write_u8(gfx::GFX_PTR_LO.add(i), at.offset() as u8)?;
        rom.write_u8(gfx::GFX_PTR_HI.add(i), (at.offset() >> 8) as u8)?;
        rom.write_u8(gfx::GFX_PTR_BANK.add(i), at.bank())?;
    }
    if files.contains_key(&0x32) || files.contains_key(&0x33) {
        let stream = |index: u8| -> Result<Vec<u8>, BuildError> {
            match files.get(&index) {
                Some(s) => Ok(s.clone()),
                None => {
                    let f = reader
                        .read(index)
                        .map_err(|e| gfx_error("the clean ROM", &e))?;
                    Ok(clean.read(f.addr, f.compressed_len)?.to_vec())
                }
            }
        };
        let (a, b) = (stream(0x32)?, stream(0x33)?);
        let at = place(rom, &mut space, &[a.as_slice(), b.as_slice()].concat())?;
        rom.write_u16(gfx::GFX32_PTR, at.offset())?;
        rom.write_u16(gfx::GFX33_PTR, at.add(a.len() as u32).offset())?;
        rom.write_u8(gfx::GFX32_33_BANK, at.bank())?;
    }
    Ok(())
}

/// Writes the project's ExGFX files and their pointers, the block that
/// holds the pointers of ExGFX `100` to `FFF` and the graphics lists, every
/// list as Lunar Magic 3.70 writes one with nothing set (the levels stage
/// writes the levels' own), and the older lists objects `24` and `25`
/// name, in Lunar Magic's layout ([`exgfx`]). The block goes first, as the
/// largest.
fn write_exgfx(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let mut space = FreeSpace::scan(rom);
    let mut block = vec![0xFF; exgfx::POINTERS_LEN];
    for n in 0..exgfx::LIST_COUNT {
        let submap = (exgfx::SUBMAP_LISTS..exgfx::SUBMAP_LISTS + 7).contains(&n);
        let list = if submap {
            GraphicsList::SUBMAP_DEFAULT
        } else {
            GraphicsList::DEFAULT
        };
        block.extend(list.to_bytes());
    }
    debug_assert_eq!(block.len(), exgfx::BLOCK_LEN);
    let at = space.alloc(rom, block.len(), Contents::Data)?;
    let mut pointers = Vec::new();
    for (number, data) in &project.exgfx {
        let name = format!("ExGFX{number:X}");
        let stream = lz2::compress(data).map_err(|e| gfx_error(&name, &e))?;
        pointers.push((*number, place(rom, &mut space, &stream)?));
    }
    for (number, file) in pointers {
        if number < 0x100 {
            let entry = exgfx::EXGFX_80.add((number - exgfx::EXGFX_FIRST) as u32 * 3);
            rom.write_ptr(entry, file)?;
        } else {
            let i = (number - 0x100) as usize * 3;
            block[i..i + 3].copy_from_slice(&file.raw().to_le_bytes()[..3]);
        }
    }
    rom.write(at, &block)?;
    rom.write_ptr(exgfx::BLOCK_POINTER, at)?;
    rom.write_ptr(exgfx::BLOCK_POINTER_COPY, at)?;
    rom.write_ptr(exgfx::LIST_POINTER, at.add(exgfx::POINTERS_LEN as u32))?;
    for (n, files) in &project.manifest.bypass_lists {
        rom.write(
            exgfx::OLD_LISTS.add(*n as u32 * 4),
            &exgfx::old_list_bytes(*files),
        )?;
    }
    Ok(())
}

/// Writes the ExAnimation lists, each in a RATS block of its own, through
/// the tables Kobo's code keeps where Lunar Magic's layout has them; each
/// listed level's settings; and the files `60`-`63`, uncompressed.
fn write_animation(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let Some(exanimation::Layout::Current { target, table }) = exanimation::layout(rom)? else {
        return Err(BuildError::Install {
            message: "ExAnimation's tables are not where Kobo's code keeps them".into(),
        });
    };
    let mut space = FreeSpace::scan(rom);
    for (number, level) in &project.levels {
        if let Some(list) = &level.animation {
            if let Some(why) = exanimation::refusal(list) {
                return Err(level_error(*number, format!("ExAnimation: {why}")));
            }
            let at = place(rom, &mut space, &list.to_bytes())?;
            rom.write_ptr(table.add(3 * *number as u32), at)?;
        }
        if let Some(byte) = level.animation_settings {
            rom.write_u8(exanimation::SETTINGS.add(*number as u32), byte)?;
        }
    }
    if let Some(list) = &project.animation_global {
        if let Some(why) = exanimation::refusal(list) {
            return Err(BuildError::Install {
                message: format!("the global ExAnimation list: {why}"),
            });
        }
        let at = place(rom, &mut space, &list.to_bytes())?;
        rom.write_u16(
            target.add(exanimation::GLOBAL_BANK_OFFSET),
            (at.bank() as u16) << 8,
        )?;
        rom.write_u16(target.add(exanimation::GLOBAL_LOW_OFFSET), at.offset())?;
    }
    for (number, bytes) in &project.animation_files {
        if bytes.is_empty() || bytes.len() > exanimation::ALT_FILE_MAX {
            return Err(BuildError::Install {
                message: format!(
                    "ExGFX{number:X} has {} bytes; an ExAnimation file has 1 to {:#X}",
                    bytes.len(),
                    exanimation::ALT_FILE_MAX
                ),
            });
        }
        let at = place(rom, &mut space, bytes)?;
        let entry = exanimation::ALT_FILES.add(3 * (number - exanimation::FIRST_ALT_FILE) as u32);
        rom.write_ptr(entry, at)?;
    }
    Ok(())
}

/// Writes the levels' graphics lists where [`write_exgfx`] put the lists.
fn write_graphics_lists(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let lists = SnesAddr::new(rom.read_u24(exgfx::LIST_POINTER)?);
    let layer3 = project.lunar_magic_layer3();
    for (number, level) in &project.levels {
        if let Some(list) = &level.graphics {
            check_graphics_list(project, *number, list)?;
            if layer3 {
                let tide =
                    exgfx::has_tide(rom, level.header.object_tileset, level.entrance.layer3)?;
                check_layer3(*number, level, &list.layer3(), tide)?;
            }
            rom.write(lists.add(*number as u32 * 32), &list.to_bytes())?;
        }
    }
    Ok(())
}

/// The most a slot's file may hold: what the buffer at `$7EAD00` takes,
/// more for the animated tiles' file (as much as Lunar Magic was seen to
/// use) and for layer 3's tilemap (which Kobo's code makes room for).
fn slot_limit(slot: usize) -> usize {
    match slot {
        exgfx::slot::AN2 => exanimation::AN2_LEN as usize,
        exgfx::slot::LT3 => 0x2000,
        _ => 0x1000,
    }
}

/// What a build refuses of a level's graphics list: layer 3's tilemap of
/// size 3, which Lunar Magic does not offer and whose load was not
/// matched; a file `32`-`7E`, which is not a GFX file; and an ExGFX file
/// too large for its slot (kept refused on
/// review: docs/lunar-magic-install.md, "Layer 3 in the lists").
fn check_graphics_list(
    project: &Project,
    number: u16,
    list: &GraphicsList,
) -> Result<(), BuildError> {
    if list.layer3_tilemap() && list.tilemap_settings() & 3 == 3 {
        return Err(level_error(
            number,
            "its graphics list gives layer 3's tilemap size 3 (`tilemap_size = 3`), \
             which Lunar Magic does not offer and loads otherwise than size 0; this build \
             cannot write it yet",
        ));
    }
    for (slot, file) in list.files() {
        let name = GraphicsList::SLOTS[slot];
        if file < 0x80 {
            if file >= 0x32 {
                return Err(level_error(
                    number,
                    format!(
                        "its graphics list's {name} names file {file:02X}, which is not a GFX file"
                    ),
                ));
            }
            continue;
        }
        // A file the project does not have loads nothing, as one the ROM
        // does not have loads nothing for Lunar Magic's code and Kobo's:
        // hacks have lists naming them (an import notes each, and
        // `warnings` reports each to a build).
        let Some((_, data)) = project.exgfx.iter().find(|(n, _)| *n == file) else {
            continue;
        };
        if data.len() > slot_limit(slot) {
            return Err(level_error(
                number,
                format!(
                    "its graphics list's {name} names ExGFX{file:X}, of {:#X} bytes, where the \
                     slot takes {:#X}",
                    data.len(),
                    slot_limit(slot)
                ),
            ));
        }
    }
    Ok(())
}

/// What a build refuses of the layer 3 settings a level's load reads
/// (docs/lunar-magic-install.md, "Layer 3 settings"): AN2's bit 12, whose
/// effect was not found, and `advanced` in a vertical level with a tide,
/// which Lunar Magic's code supports in ways not observed. (A tide at a
/// size of the level's own is refused with the level.)
fn check_layer3(
    number: u16,
    level: &Level,
    settings: &exgfx::Layer3Settings,
    tide: bool,
) -> Result<(), BuildError> {
    let refuse = |what: &str| {
        Err(level_error(
            number,
            format!("its layer 3 settings have {what}, which this build cannot write yet"),
        ))
    };
    if settings.unknown {
        return refuse("AN2's bit 12 (`unknown`)");
    }
    if tide && settings.advanced && level.header.level_mode.layer1_vertical() {
        return refuse("`advanced` in a vertical level with a tide");
    }
    Ok(())
}

fn gfx_error(name: &str, e: &dyn std::fmt::Display) -> BuildError {
    BuildError::Gfx {
        path: PathBuf::from(name),
        message: e.to_string(),
    }
}

/// Writes the project's BG Map16 pages into a table of their own, all 16
/// pages, for each BG Map16 table they are in, and points `$0EFD50` at
/// them. A tile a page leaves out is what the build starts from: on pages
/// 0 and 1 of the first table, the game's own, the clean ROM's tile, and
/// elsewhere an empty one, as Lunar Magic leaves a table's unused tiles.
fn write_map16_bg(rom: &mut Rom, clean: &Rom, project: &Project) -> Result<(), BuildError> {
    if project.map16_bg.is_empty() {
        return Ok(());
    }
    let mut space = FreeSpace::scan(rom);
    let pages: std::collections::BTreeMap<u8, &Map16Page> =
        project.map16_bg.iter().map(|(n, p)| (*n, p)).collect();
    for table in 0..16u8 {
        if !pages.keys().any(|&p| p >> 4 == table) {
            continue;
        }
        let mut bytes = Vec::with_capacity(16 * 0x800);
        for page in 0..16 {
            let number = table * 16 + page;
            let mut page_bytes = if number < 2 {
                let at = crate::map16::tables::MAP16_BG_TILES.add(number as u32 * 0x800);
                clean.read(at, 0x800)?.to_vec()
            } else {
                vec![0; 0x800]
            };
            if let Some(tiles) = pages.get(&number) {
                let first = number as u16 * page_source::PAGE_TILES;
                for (&tile, entry) in tiles.tiles.range(first..=first + 0xFF) {
                    let i = (tile - first) as usize * 8;
                    page_bytes[i..i + 8].copy_from_slice(&entry.gfx.to_bytes());
                }
            }
            bytes.extend(page_bytes);
        }
        let at = place(rom, &mut space, &bytes)?;
        rom.write_u24(map16_pages::BG_TABLES.add(3 * table as u32), at.raw())?;
    }
    Ok(())
}

/// Whether `rom` has `list` as level `number`'s sprite list already.
fn same_list(rom: &Rom, number: u16, list: &[u8]) -> bool {
    let Ok(at) = level::sprite_ptr(rom, number) else {
        return false;
    };
    let Ok(parsed) = sprites::read_sprites_at(rom, at) else {
        return false;
    };
    rom.read(at, parsed.len).is_ok_and(|bytes| bytes == list)
}

/// First-fit placement in [`BANK_07_FREE`].
struct Bank07 {
    free: Vec<(u32, u32)>,
}

impl Bank07 {
    fn new(clean: &Rom) -> Self {
        let free = BANK_07_FREE
            .iter()
            .map(|&(start, end)| {
                let end = if end == 0 { 0x1_0000 } else { end as u32 };
                (0x07_0000 + start as u32, 0x07_0000 + end)
            })
            // Only what the clean ROM really has free.
            .filter(|&(start, end)| {
                clean
                    .read(SnesAddr::new(start), (end - start) as usize)
                    .is_ok_and(|bytes| bytes.iter().all(|&b| b == 0xFF))
            })
            .collect();
        Self { free }
    }

    fn alloc(&mut self, len: usize) -> Option<SnesAddr> {
        let run = self
            .free
            .iter_mut()
            .find(|(start, end)| (end - start) as usize >= len)?;
        let at = SnesAddr::new(run.0);
        run.0 += len as u32;
        Some(at)
    }
}

/// Writes one level: layer data in new RATS blocks, and its pointers and
/// secondary header over the base's. A sprite list the base already has
/// for the level keeps its place.
fn write_level(
    rom: &mut Rom,
    base: &Rom,
    space: &mut FreeSpace,
    bank07: &mut Bank07,
    number: u16,
    level: &Level,
) -> Result<(), BuildError> {
    let err = |message: &dyn std::fmt::Display| level_error(number, message);
    let mode = level.header.level_mode;
    let vertical = |v: bool| {
        if v {
            Layout::Vertical
        } else {
            Layout::Horizontal
        }
    };
    let layout1 = vertical(mode.layer1_vertical());
    layer2_matches_mode(level).map_err(|e| err(&e))?;
    check_size(level).map_err(|e| err(&e))?;
    check_objects(number, &level.layer1, true)?;
    // Screen jumps with a vertical part, where the ROM has the taller
    // levels' code (Kobo's, installed with Lunar Magic's layout).
    let jumps = level::LevelFormat::of(rom).jumps;
    let layer1 = objects::encode(
        level.header.to_bytes(),
        &exits_in_one_format(&level.layer1, number),
        layout1,
        jumps,
    )
    .map_err(|e: ObjectError| err(&format_args!("layer 1: {e}")))?;
    let at = place(rom, space, &layer1)?;
    rom.write_ptr(tables::LAYER1_PTRS.add(3 * number as u32), at)?;

    let layer2_ptr = tables::LAYER2_PTRS.add(3 * number as u32);
    match &level.layer2 {
        // The mode loads nothing from the pointer; the clean ROM's stays.
        Layer2::None => {}
        Layer2::Objects(list) => {
            check_objects(number, list, false)?;
            let layout = vertical(mode.layer2() == level::Layer2Kind::VerticalObjects);
            let bytes = objects::encode(level.header.to_bytes(), list, layout, jumps)
                .map_err(|e| err(&format_args!("layer 2: {e}")))?;
            let at = place(rom, space, &bytes)?;
            rom.write_ptr(layer2_ptr, at)?;
        }
        Layer2::VanillaBackground(addr) => {
            if addr.bank() != 0x0C {
                return Err(err(&format_args!(
                    "background {addr} is not in bank $0C, where the game reads backgrounds"
                )));
            }
            rom.write_ptr(layer2_ptr, SnesAddr::from_bank_offset(0xFF, addr.offset()))?;
        }
        Layer2::Background(bg) => {
            let (data, flags) = background_stream(bg).map_err(|e| err(&format_args!("{e}")))?;
            let stream = rle1::compress(&data).map_err(|e| err(&format_args!("{e}")))?;
            let at = place(rom, space, &stream)?;
            rom.write_ptr(layer2_ptr, at)?;
            rom.write_u8(tables::LEVEL_FLAGS.add(number as u32), flags)?;
        }
    }

    if let Some(palette) = &level.palette {
        let mut bytes = palette.back_area.0.to_le_bytes().to_vec();
        for color in palette.palette.colors {
            bytes.extend(color.0.to_le_bytes());
        }
        let at = place(rom, space, &bytes)?;
        rom.write_u24(
            palette::LM_LEVEL_PALETTE_PTRS.add(3 * number as u32),
            at.raw(),
        )?;
    }

    for (table, byte) in tables::SECONDARY_HEADERS
        .iter()
        .zip(level.entrance.to_bytes())
    {
        rom.write_u8(table.add(number as u32), byte)?;
    }

    let list = sprite_list(base, level).map_err(|e| err(&e))?;
    let at = if same_list(base, number, &list) {
        level::sprite_ptr(base, number)?
    } else if level::LevelFormat::of(base).lunar_magic {
        // Kobo's sprite bank code is installed: any bank will do.
        place(rom, space, &list)?
    } else {
        let at = bank07.alloc(list.len()).ok_or_else(|| {
            err(&format_args!(
                "its {}-byte sprite list does not fit in bank $07's unused space",
                list.len()
            ))
        })?;
        rom.write(at, &list)?;
        at
    };
    rom.write_u16(tables::SPRITE_PTRS.add(2 * number as u32), at.offset())?;
    if level::LevelFormat::of(base).lunar_magic {
        rom.write_u8(tables::SPRITE_BANKS.add(number as u32), at.bank())?;
    }
    Ok(())
}

/// A level's sprite list as a build writes it: in the new sprite system's
/// format when it needs that (Kobo's loader, which the project's layout
/// then installs, reads it), with extension bytes as PIXI's size table in
/// `rom` says, if it has one.
fn sprite_list(rom: &Rom, level: &Level) -> Result<Vec<u8>, String> {
    let vertical = level.header.level_mode.layer1_vertical();
    let count = level.sprites.list.len();
    let max = sprites::max_sprites(rom);
    if count > max {
        let with = if max == sprites::MAX_SPRITES {
            " without a 255-sprite loader such as PIXI's"
        } else {
            ""
        };
        return Err(format!(
            "it has {count} sprites; Lunar Magic's layout loads {max}{with}"
        ));
    }
    if let Some(sprite) = vertical
        .then(|| level.sprites.list.iter().find(|s| s.x > 0x1F))
        .flatten()
    {
        return Err(format!(
            "a sprite at X {} is past the 32 columns of a vertical level",
            sprite.x
        ));
    }
    let new = level.sprites.needs_new_system(vertical);
    let (header, entries) = level.sprites.to_entries(vertical, new);
    // The game's loader stops at the first sprite past the screen it
    // loads, and Lunar Magic's reads a list out of screen order wrongly
    // enough to crash the game (docs/lunar-magic-install.md, "Sprites").
    if let Some(i) = (1..entries.len()).find(|&i| entries[i].screen < entries[i - 1].screen) {
        return Err(format!(
            "sprite {i} is on screen {:02X}, before the screen of the one ahead of it; a \
             level's sprites must be in screen order",
            entries[i].screen
        ));
    }
    let sizes = sprites::pixi_size_table(rom).map_err(|e| e.to_string())?;
    sprites::encode(header, &entries, sizes).map_err(|e: SpriteEncodeError| format!("sprites: {e}"))
}

/// The level mode decides what the game loads on layer 2, so the file
/// must give it that: a background for a background mode, objects for an
/// object mode, and nothing for the modes that load nothing.
fn layer2_matches_mode(level: &Level) -> Result<(), String> {
    let mode = level.header.level_mode;
    let wants = match mode.layer2() {
        Layer2Kind::Background => "a background",
        Layer2Kind::HorizontalObjects | Layer2Kind::VerticalObjects => "objects",
        Layer2Kind::None => "no layer 2",
    };
    let has = match &level.layer2 {
        Layer2::None => "no layer 2",
        Layer2::Objects(_) => "objects",
        Layer2::VanillaBackground(_) | Layer2::Background(_) => "a background",
    };
    if wants == has {
        Ok(())
    } else {
        Err(format!(
            "level mode {mode} loads {wants} on layer 2, but the file has {has}"
        ))
    }
}

/// Refuses a screen exit to a secondary entrance past the tables the
/// build has, which Lunar Magic's code and Kobo's would read past them.
/// [`Project::entrance_count`] sizes the tables to hold every one a
/// project's exit names.
fn check_exits(rom: &Rom, project: &Project) -> Result<(), BuildError> {
    let count = level::entrance_count(rom);
    for (number, level) in &project.levels {
        if let Some(id) = exit_entrances(level).find(|&id| id >= count) {
            return Err(level_error(
                *number,
                format!(
                    "a screen exit leads to entrance {id:03X}, past the {count} entrances the tables hold"
                ),
            ));
        }
    }
    Ok(())
}

/// The secondary entrances a level's screen exits in Lunar Magic's format
/// lead to, on either layer ([`Object::lunar_magic_entrance`]).
fn exit_entrances(level: &Level) -> impl Iterator<Item = u16> + '_ {
    let layer2: &[Object] = match &level.layer2 {
        Layer2::Objects(list) => list,
        _ => &[],
    };
    level
        .layer1
        .iter()
        .chain(layer2)
        .filter_map(Object::lunar_magic_entrance)
}

/// Writes every project level's secondary entrances. A level's list is all
/// of them: an entrance the base ROM had leading to it that the list does
/// not name is cleared. One the base ROM has leading to a level the
/// project does not define stays that level's. In the game's format an
/// entrance's number gives its destination's bit 8, so it must match the
/// level's.
fn write_entrances(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let format = level::LevelFormat::of(rom);
    let layout = entrance::Layout::of(rom);
    let extra_count = entrance::extra_count(rom, &layout);
    let mut entrances = level::read_entrances(rom)?;
    let defined: Vec<u16> = project.levels.iter().map(|(n, _)| *n).collect();
    // With the exit hook installed, every entrance keeps its destination's
    // bit 8 in bit 3 of `$05FE00`, as Lunar Magic's first save sets them.
    let from_clean = level::LevelFormat {
        entrances: false,
        ..format
    };
    for (id, bytes) in (0..).zip(entrances.iter_mut()) {
        if bytes.in_use(from_clean) && defined.contains(&bytes.destination(id, from_clean)) {
            *bytes = level::EntranceBytes::default();
        } else if format.entrances && id < tables::ENTRANCE_COUNT {
            // Unused ones too, from `100` on, as Lunar Magic leaves them;
            // past `1FF` it leaves them clear.
            let high = bytes.destination(id, from_clean) >> 8 & 1;
            bytes.0[3] = bytes.0[3] & !0x08 | (high as u8) << 3;
        }
    }
    let mut owner: Vec<Option<u16>> = vec![None; entrances.len()];
    let mut extras = vec![[0u8; 2]; entrances.len()];
    for (number, level) in &project.levels {
        for entrance in &level.entrances {
            let id = entrance.id as usize;
            if id >= entrances.len() {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X} is past the {} entrances the tables hold",
                        entrance.id,
                        entrances.len()
                    ),
                ));
            }
            let base = entrances[id];
            if owner[id].is_none() && base.in_use(format) {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X} leads to level {:03X}, which the project does not define",
                        entrance.id,
                        base.destination(entrance.id, format)
                    ),
                ));
            }
            if let Some(other) = owner[id].replace(*number) {
                return Err(level_error(
                    *number,
                    format!("entrance {:03X} is also level {other:03X}'s", entrance.id),
                ));
            }
            if !format.entrances && entrance.id >> 8 != number >> 8 {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X} cannot lead here in the game's format, where its number gives the level's bit 8",
                        entrance.id
                    ),
                ));
            }
            if entrance.settings.overworld.is_some() {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X} exits to the overworld, which builds do not carry yet",
                        entrance.id
                    ),
                ));
            }
            let ([fa, fc, mut fe], extra) = entrance.to_bytes();
            if id >= extra_count && extra != [0, 0] {
                return Err(level_error(
                    *number,
                    format!(
                        "entrance {:03X}'s Lunar Magic settings do not fit its tables, which hold \
                         entrances 000 to {:03X}",
                        entrance.id,
                        extra_count - 1
                    ),
                ));
            }
            if format.entrances {
                fe = fe & !0x08 | ((number >> 8) as u8 & 1) << 3;
            }
            entrances[id] = level::EntranceBytes([*number as u8, fa, fc, fe]);
            extras[id] = extra;
        }
    }
    level::write_entrances(rom, &entrances)?;
    // Lunar Magic's two further tables, where Kobo's entrance code has
    // them; the defaults for the entrances no level names.
    if let Some(tables) = layout.extra {
        for (i, table) in tables.into_iter().enumerate() {
            let column: Vec<u8> = extras[..extra_count].iter().map(|e| e[i]).collect();
            rom.write(table, &column)?;
        }
    }
    Ok(())
}

/// Writes Lunar Magic's per-level settings and midway tables for every
/// level, a fresh install's defaults for those the project does not
/// define, where Kobo's entrance code reads them (`asm/lunar-magic/
/// entrance.asm`), whose camera code also runs Lunar Magic's added layer 2
/// scroll settings.
fn write_level_settings(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let layout = entrance::Layout::of(rom);
    let midway = layout.midway.ok_or_else(|| BuildError::Install {
        message: "the midway tables are not where Kobo's entrance code keeps them".into(),
    })?;
    let count = level::LEVEL_COUNT as usize;
    let (defaults, _) = LevelSettings::default().to_bytes();
    let mut settings: Vec<[u8; 4]> = vec![defaults; count];
    let mut midways = vec![[0u8; 4]; count];
    // A midway entrance that redirects, through other levels' or not, back
    // to its own level loops forever when the game enters it: Lunar Magic's
    // help warns of that for its own code, which chains redirects as
    // Kobo's does. Levels the
    // project does not define get no midway settings, so a chain ends there.
    let redirect = |n: u16| {
        project
            .levels
            .iter()
            .find(|(m, _)| *m == n)
            .and_then(|(_, l)| match l.settings.midway.separate {
                Some(entrance::SeparateMidway::Redirect(to)) => Some(to),
                _ => None,
            })
    };
    for (number, _) in &project.levels {
        let mut seen = vec![*number];
        let mut at = *number;
        while let Some(to) = redirect(at) {
            if seen.contains(&to) {
                return Err(level_error(
                    *number,
                    format!("its midway entrance redirects in a loop, through level {to:03X}"),
                ));
            }
            seen.push(to);
            at = to;
        }
    }
    for (number, level) in &project.levels {
        let (bytes, midway) = level.settings.to_bytes();
        settings[*number as usize] = bytes;
        midways[*number as usize] = midway;
    }
    for (i, table) in entrance::tables::SETTINGS.into_iter().enumerate() {
        let column: Vec<u8> = settings.iter().map(|b| b[i]).collect();
        rom.write(table, &column)?;
    }
    for i in 0..4 {
        let column: Vec<u8> = midways.iter().map(|b| b[i]).collect();
        rom.write(midway.add(i as u32 * 0x200), &column)?;
    }
    Ok(())
}

/// Writes Lunar Magic's level size table (`level::size`), where Kobo's
/// taller levels code has it (`asm/lunar-magic/exlevel.asm`), for the levels
/// the project defines; the others keep the base's. The split between
/// the layers (`T`) is written as Lunar Magic's save writes it: when layer 2
/// has objects.
fn write_level_sizes(rom: &mut Rom, project: &Project) -> Result<(), BuildError> {
    let table = level::size::table(rom).ok_or_else(|| BuildError::Install {
        message: "the level size table is not where Kobo's taller levels code keeps it".into(),
    })?;
    for (number, level) in &project.levels {
        let objects = matches!(
            level.header.level_mode.layer2(),
            level::Layer2Kind::HorizontalObjects | level::Layer2Kind::VerticalObjects
        );
        rom.write_u8(table.add(*number as u32), level.size.to_byte(objects))?;
    }
    Ok(())
}

/// A level's size, which only a horizontal level takes, must leave room
/// for its objects: past the screens its height allows, the screen
/// pointers are 0, and objects there are written over work RAM (Heraga's
/// level `1E3` loads under Lunar Magic's code with layer 2 objects past
/// their room, and crashes under Kobo's). With layer 2 objects, layer 1 has
/// the screens before layer 2's, and layer 2 the rest. The header's screen
/// count may say more: Lunar Magic's taller levels code, and Kobo's, load
/// such a level the same (QLDC 2021 `22_FerpyMcFrosting`;
/// docs/lunar-magic-install.md, "Taller levels").
fn check_size(level: &Level) -> Result<(), String> {
    let size = level.size;
    if size.is_default() {
        return Ok(());
    }
    let mode = level.header.level_mode;
    if mode.layer1_vertical() {
        return Err(format!(
            "it has a level size ({} rows), which a vertical level does not take",
            size.rows()
        ));
    }
    // The last screen a layer's placed objects are on.
    let last = |objects: &[Object], vertical: bool| {
        objects
            .iter()
            .filter_map(|o| match o {
                Object::Standard { x, y, .. }
                | Object::Extended { x, y, .. }
                | Object::Lunar { x, y, .. } => Some(if vertical { *y } else { *x } as usize / 16),
                _ => None,
            })
            .max()
    };
    let past = |layer: u8, last: Option<usize>, room: usize| match last {
        Some(last) if last >= room => Err(format!(
            "its layer {layer} has objects on screen {last}, and {} rows leave room for {room}",
            size.rows()
        )),
        _ => Ok(()),
    };
    match (mode.layer2(), &level.layer2) {
        (
            kind @ (Layer2Kind::HorizontalObjects | Layer2Kind::VerticalObjects),
            Layer2::Objects(list),
        ) => {
            let split = size.layer2_screen();
            past(1, last(&level.layer1, false), split)?;
            past(
                2,
                last(list, kind == Layer2Kind::VerticalObjects),
                size.screens().saturating_sub(split),
            )
        }
        _ => past(1, last(&level.layer1, false), size.screens()),
    }
}

/// A background as its stream holds it, and the level flags that say
/// how: 32 rows in Lunar Magic's own format (`C` and `F`, the table in the
/// high nibble), low bytes then high bytes, each two halves of 32 rows of
/// 16; 27 rows in the game's format behind a full pointer (`V`, the tiles'
/// one high byte in the high nibble), low bytes, two halves of 27 rows.
fn background_stream(bg: &BackgroundTiles) -> Result<(Vec<u8>, u8), String> {
    let rows = bg.rows;
    if bg.table > 0xF {
        return Err(format!("its BG Map16 table is 0 to F, not {:X}", bg.table));
    }
    let tile = |half: usize, row: usize, col: usize| bg.tiles[row * 32 + half * 16 + col];
    let cells = || {
        (0..2).flat_map(move |half| {
            (0..rows).flat_map(move |row| (0..16).map(move |col| (half, row, col)))
        })
    };
    if rows == BACKGROUND_ROWS {
        let low = cells().map(|(h, r, c)| tile(h, r, c) as u8);
        let high = cells().map(|(h, r, c)| (tile(h, r, c) >> 8) as u8);
        return Ok((low.chain(high).collect(), bg.table << 4 | 0x06));
    }
    let high = tile(0, 0, 0) >> 8;
    if bg.table != 0 || high > 0xF || cells().any(|(h, r, c)| tile(h, r, c) >> 8 != high) {
        return Err(format!(
            "a {rows}-row background has table 0 and one high byte, 0 to F, for all its tiles"
        ));
    }
    let low = cells().map(|(h, r, c)| tile(h, r, c) as u8).collect();
    Ok((low, 0x08 | (high as u8) << 4))
}

/// Lunar Magic's objects Kobo's code ([`crate::install`]) handles: those
/// that place tiles (`22`, `23`, `27`, `29`), its music bypass (`26`), its
/// user object (`2D`), and a long screen exit kept as its bytes.
fn handled(object: &Object) -> bool {
    matches!(
        object.lunar_number(),
        Some(0x22 | 0x23 | 0x27 | 0x29 | 0x26 | 0x28 | 0x2D)
    ) || graphics_object(object)
        || object.is_raw_long_exit()
}

/// Lunar Magic's older graphics bypass, objects `24` (the lists) and `25`
/// (the animated tiles' file), which Kobo's graphics code reads from layer
/// 1's data.
fn graphics_object(object: &Object) -> bool {
    matches!(object, Object::Unplaced(_)) && matches!(object.lunar_number(), Some(0x24 | 0x25))
}

/// A level's objects with every screen exit in Lunar Magic's format when
/// any is: Lunar Magic's save gives an exit in the game's format the `s`
/// bit of the level's exit in its own, which turns a normal exit secondary
/// (lunar-magic.md). A level with none in its format keeps the game's.
fn exits_in_one_format(list: &[Object], number: u16) -> Vec<Object> {
    let lunar = |o: &Object| matches!(o, Object::ScreenExit(e) if e.flags & objects::ScreenExit::LUNAR_MAGIC != 0);
    if !list.iter().any(lunar) {
        return list.to_vec();
    }
    list.iter()
        .map(|o| match o {
            Object::ScreenExit(exit) => Object::ScreenExit(exit.in_lunar_magic_format(number)),
            other => other.clone(),
        })
        .collect()
}

/// Lunar Magic's other objects need code of its own Kobo does not install
/// yet: its long screen exits. A screen exit in its format builds with
/// Kobo's exit and entrance code. The graphics bypass objects (`24`, `25`)
/// are read from layer 1 alone.
fn check_objects(number: u16, list: &[Object], layer1: bool) -> Result<(), BuildError> {
    let refused = |o: &Object| match o {
        Object::Unplaced(_) if graphics_object(o) && !layer1 => {
            Some("Lunar Magic's graphics bypass, which it reads from layer 1 only,")
        }
        Object::Lunar { .. } | Object::Unplaced(_) if !handled(o) => Some("one of Lunar Magic's"),
        _ => None,
    };
    match list
        .iter()
        .enumerate()
        .find_map(|(i, o)| Some((i, refused(o)?)))
    {
        Some((i, what)) => Err(level_error(
            number,
            format!("object {i} is {what}, which this build cannot write yet"),
        )),
        None => Ok(()),
    }
}

fn place(rom: &mut Rom, space: &mut FreeSpace, bytes: &[u8]) -> Result<SnesAddr, BuildError> {
    let at = space.alloc(rom, bytes.len(), Contents::Data)?;
    rom.write(at, bytes)?;
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::RomIdentity;

    fn vanilla() -> Option<Rom> {
        let rom = Rom::load(crate::config::vanilla_rom_path().ok()?).ok()?;
        (rom.identify() == RomIdentity::VanillaUsa).then_some(rom)
    }

    /// The tables hold the last entrance in use, defined or named by an
    /// exit, and an exit past the tables a ROM has is refused.
    #[test]
    fn entrance_tables_hold_the_last_entrance_in_use() {
        let Some(clean) = vanilla() else {
            eprintln!("skipping: no vanilla ROM configured");
            return;
        };
        let (mut level, _) = crate::import::read_level(&clean, 0x105).unwrap();
        let mut project = Project {
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        assert_eq!(project.entrance_count(), 0x200);
        assert!(!project.many_entrances());
        check_exits(&clean, &project).unwrap();

        // A normal exit, long or not, names a level, not an entrance.
        let exit = |flags, to| Object::ScreenExit(objects::ScreenExit::lunar_magic(4, flags, to));
        level.layer1.push(exit(0, 0x3FF));
        project.levels[0].1 = level.clone();
        assert_eq!(project.entrance_count(), 0x200);
        // An exit to an entrance no level defines counts.
        level
            .layer1
            .push(exit(objects::ScreenExit::SECONDARY, 0x5FF));
        project.levels[0].1 = level.clone();
        assert_eq!(project.entrance_count(), 0x600);
        // And so does an entrance no exit names.
        level.entrances[0].id = 0x7FE;
        project.levels[0].1 = level.clone();
        assert_eq!(project.entrance_count(), 0x7FF);
        assert!(project.many_entrances());
        // At most 2000, past which the build refuses the entrance.
        level.entrances[0].id = 0x2000;
        project.levels[0].1 = level;
        assert_eq!(project.entrance_count(), 0x2000);

        // The clean ROM's tables hold 512: the exit to 5FF reads past them.
        let error = check_exits(&clean, &project).unwrap_err().to_string();
        assert!(
            error.contains("entrance 5FF, past the 512 entrances"),
            "{error}"
        );
    }
}
