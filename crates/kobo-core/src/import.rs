//! Importing a ROM's levels into a project.
//!
//! Only levels that differ from the clean ROM are imported, unless all are
//! asked for, so a project holds no Nintendo level data its author did not
//! change. What cannot be carried over is reported, not dropped silently.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::addr::{PcAddr, SnesAddr};
use crate::compress::rle1;
use crate::entrance::{self, EntranceSettings, LevelSettings};
use crate::exanimation;
use crate::exgfx;
use crate::gfx;
use crate::image::IndexedImage;
use crate::level::objects::Object;
use crate::level::size::LevelSize;
use crate::level::{self, LEVEL_COUNT, Layer2Kind, LevelError, LevelFormat, LevelMode, tables};
use crate::map16::pages::{self as map16_pages, PAGE_GROUPS};
use crate::map16::{GameTables, Map16Tile, TILESET_COUNT};
use crate::mwl::{self, Mwl, MwlFile};
use crate::palette::{self, CustomPalette};
use crate::rats::{self, RatsBlock};
use crate::rom::Rom;
use crate::source::level::{
    BACKGROUND_ROWS, BackgroundTiles, Comments, Entrance, Layer2, Level, Sprites,
};
use crate::source::map16::{
    DEFAULT_ACTS, GamePage, GameTile, Map16Entry, Map16Page, PAGE_TILES, PageComments, PageKind,
    Pipes,
};
use crate::source::project::{ExGfxFile, MANIFEST, Manifest};
use crate::sprites::{self, SpriteError};

/// Lunar Magic's version string (see [`Rom::lunar_magic_version`]).
const LUNAR_MAGIC_MARKER: SnesAddr = SnesAddr::new(0x0FF0A0);

#[derive(Debug, Error)]
pub enum ImportError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error(transparent)]
    Sprites(#[from] SpriteError),
    #[error(transparent)]
    Rom(#[from] crate::rom::RomError),
    #[error("graphics: {0}")]
    Gfx(String),
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} already has a {MANIFEST}")]
    Exists(PathBuf),
    #[error(transparent)]
    Mwl(#[from] crate::mwl::MwlError),
    #[error("making the SA-1 base, which needs SA-1 Pack configured: {0}")]
    Base(Box<crate::build::BuildError>),
    #[error("level {0:X} is out of range (0 to 1FF)")]
    LevelNumber(u16),
    #[error("{} is level {level:03X}'s file", .file.display())]
    FileTaken { file: PathBuf, level: u16 },
    #[error("{path}: {source}")]
    Manifest {
        path: PathBuf,
        #[source]
        source: crate::source::SourceError,
    },
    #[error("the PIXI folder: {0}")]
    Pixi(#[from] crate::tools::ToolError),
    #[error("the Callisto project: {0}")]
    Callisto(#[from] crate::callisto::CallistoError),
    #[error("{path}: {source}")]
    Map16File {
        path: PathBuf,
        #[source]
        source: crate::map16_file::Map16FileError,
    },
    #[error("{path}: {source}")]
    Bps {
        path: PathBuf,
        #[source]
        source: crate::bps::BpsError,
    },
    #[error("{path}: {message}")]
    File { path: PathBuf, message: String },
}

/// A level read from a ROM, with notes on what it holds that a build
/// cannot write yet.
pub fn read_level(rom: &Rom, number: u16) -> Result<(Level, Vec<String>), ImportError> {
    let mut notes = Vec::new();
    let data = level::read_objects(rom, number)?;
    let header = data.header();
    let layer2 = match data.layer2 {
        level::Layer2::None => Layer2::None,
        level::Layer2::Objects(list) => Layer2::Objects(list.objects),
        level::Layer2::Background(addr) if addr.bank() == 0x0C => Layer2::VanillaBackground(addr),
        level::Layer2::Background(addr) => match level::read_background(rom, number)? {
            Some(bg) => match background_tiles(&bg) {
                Some(tiles) => Layer2::Background(tiles),
                None => {
                    notes.push(format!(
                        "its background at {addr} has flags {:02X?}, which Kobo does not read",
                        bg.flags
                    ));
                    Layer2::None
                }
            },
            None => Layer2::None,
        },
    };
    let list = sprites::read_sprites_at(rom, level::sprite_ptr(rom, number)?)?;
    let vertical = header.level_mode.layer1_vertical();
    let mut layer1 = data.layer1.objects;
    exits_in_game_format(&mut layer1, number);
    let format = LevelFormat::of(rom);
    let layout = entrance::Layout::of(rom);
    let entrances = level::read_entrances(rom)?
        .into_iter()
        .zip(0..)
        .filter(|(bytes, id)| bytes.in_use(format) && bytes.destination(*id, format) == number)
        .map(|(bytes, id)| {
            let [_, fa, fc, fe] = bytes.0;
            let settings = entrance::read_entrance_settings(rom, &layout, id, fe, vertical)?;
            Ok((id, [fa, fc, fe], settings))
        })
        .collect::<Result<Vec<_>, ImportError>>()?;
    let entrances = entrances_leading_to(entrances.into_iter(), &mut notes);
    let mut secondary = level::read_secondary_header(rom, number)?;
    // Lunar Magic 2's layer 2 scroll setting 8 is its 3's setting 3, as
    // its MWL export rewrites it.
    if layout.settings == Some(entrance::Version::Old) && secondary.layer2_scroll == 8 {
        secondary.layer2_scroll = 3;
    }
    let size = level_size(
        level::size::read_byte(rom, number).unwrap_or(0),
        header.level_mode,
        &mut notes,
    );
    let level = Level {
        header,
        size,
        entrance: secondary,
        settings: entrance::read_level_settings(rom, &layout, number, vertical)?,
        layer1,
        layer2,
        sprites: Sprites::from_entries(list.header, &list.sprites, vertical),
        entrances,
        palette: palette::lm_level_palette(rom, number)?,
        graphics: crate::exgfx::read_list(rom, number)?,
        animation: match exanimation::read_level(rom, number) {
            Ok(found) => found.map(|found| found.list),
            Err(e) => {
                notes.push(format!(
                    "its ExAnimation does not read ({e}); it is not imported"
                ));
                None
            }
        },
        animation_settings: exanimation::read_settings(rom, number)?
            .map(|s| s.0)
            .filter(|&b| b != exanimation::Settings::default_for(number).0),
    };
    notes.extend(animation_notes(level.animation.as_ref()));
    // Without Lunar Magic's layer 3 code nothing reads the settings, and a
    // build would install Kobo's for them: they are left out, but for what
    // a tide acts like (BG3's nibble), which its taller levels code reads
    // (QLDC 2021 `56_TheKazooBloccGosh`; docs/lunar-magic-install.md,
    // "Layer 3 settings").
    let mut level = level;
    if rom.read_u8(crate::exgfx::LAYER3_CHECK)? != crate::exgfx::JSL
        && let Some(list) = level.graphics.as_mut()
        && list.has_layer3()
    {
        use crate::exgfx::{EMPTY, slot};
        let bg3 = list.0[slot::BG3];
        let keep = bg3 != EMPTY
            && level::size::table(rom).is_some()
            && crate::exgfx::has_tide(rom, level.header.object_tileset, level.entrance.layer3)?;
        let mut others = *list;
        if keep {
            others.0[slot::BG3] &= 0x0FFF;
        }
        if others.has_layer3() {
            notes.push(
                "its graphics list has layer 3 settings the ROM has no code for, left out".into(),
            );
        }
        *list = list.without_layer3();
        if keep {
            list.0[slot::BG3] = bg3;
        }
    }
    level.graphics = level.graphics.filter(|l| l.is_used());
    if level.palette.as_ref().is_some_and(high_bits) {
        notes.push("its palette has colours with bit 15 set, which is not kept".into());
    }
    notes.extend(graphics_notes(Some(rom), &level)?);
    let lunar = level
        .layer1
        .iter()
        .chain(match &level.layer2 {
            Layer2::Objects(list) => list.as_slice(),
            _ => &[],
        })
        .filter(|o| matches!(o, Object::Lunar { .. } | Object::Unplaced(_)))
        .count();
    if lunar > 0 {
        notes.push(format!("{lunar} of Lunar Magic's objects, kept as bytes"));
    }
    Ok((level, notes))
}

/// Notes on an ExAnimation list a build refuses.
fn animation_notes(list: Option<&exanimation::List>) -> Vec<String> {
    list.and_then(exanimation::refusal)
        .map(|why| format!("its ExAnimation has what this build cannot write yet: {why}"))
        .into_iter()
        .collect()
}

/// Notes on a level's graphics list: each file it names that the ROM does
/// not have, whose slot loads nothing (as Lunar Magic's code and Kobo's
/// leave it); and what of its layer 3 settings a build refuses, when it has
/// any. Those only from a ROM with Lunar Magic's layer 3 code (a `JSL` at
/// `$00A01F`), which reads them, and with its table of which settings are
/// tides; from an MWL file, all but what depends on a tide, which the
/// build checks.
fn graphics_notes(rom: Option<&Rom>, level: &Level) -> Result<Vec<String>, crate::rom::RomError> {
    let mut notes = Vec::new();
    let Some(list) = level.graphics.as_ref() else {
        return Ok(notes);
    };
    if let Some(rom) = rom {
        for (slot, file) in list.files() {
            if file >= 0x80 && crate::exgfx::exgfx_addr(rom, file).ok().flatten().is_none() {
                notes.push(format!(
                    "its graphics list's {} names ExGFX{file:X}, which the ROM does not have: \
                     the slot loads nothing",
                    crate::exgfx::GraphicsList::SLOTS[slot]
                ));
            }
        }
    }
    if !list.has_layer3() {
        return Ok(notes);
    }
    let tide = match rom {
        Some(rom) if rom.read_u8(crate::exgfx::LAYER3_CHECK)? != crate::exgfx::JSL => {
            return Ok(notes);
        }
        Some(rom) => {
            crate::exgfx::has_tide(rom, level.header.object_tileset, level.entrance.layer3)?
        }
        None => false,
    };
    let s = list.layer3();
    if s.unknown {
        notes.push("its layer 3 settings have AN2's bit 12, which a build refuses".into());
    }
    if tide && s.advanced && level.header.level_mode.layer1_vertical() {
        notes.push(
            "its layer 3 settings are advanced in a vertical level with a tide, which a build \
             refuses"
                .into(),
        );
    }
    Ok(notes)
}

/// A level's size from Lunar Magic's size byte: a vertical level takes
/// none, and whether the screens are split between the layers is the
/// level's when layer 2 has objects (a build writes `T` then, as Lunar
/// Magic's save does), so a byte without it there is noted; `T` without
/// layer 2 objects is kept ([`LevelSize::split`]).
fn level_size(byte: u8, mode: LevelMode, notes: &mut Vec<String>) -> LevelSize {
    let size = LevelSize::from_byte(byte);
    if mode.layer1_vertical() {
        if !size.is_default() {
            notes.push(format!(
                "its level size byte {byte:02X} is one a vertical level ignores, and is not kept"
            ));
        }
        return LevelSize::default();
    }
    let objects = matches!(
        mode.layer2(),
        Layer2Kind::HorizontalObjects | Layer2Kind::VerticalObjects
    );
    if size.mode != 0 && !objects && LevelSize::byte_splits(byte) {
        return LevelSize {
            split: true,
            ..size
        };
    }
    if size.mode != 0 && (byte & 0x80 != 0) != objects {
        notes.push(format!(
            "its level size byte {byte:02X} splits the screens between the layers {} its layer 2 \
             does; a build writes the split its layer 2 says",
            if objects { "less than" } else { "more than" }
        ));
    }
    size
}

/// Puts the level's screen exits in the game's format where it can say
/// them, so a build needs Lunar Magic's exit code only for the rest (to the
/// other bank, or with `w`), which stay in its format.
fn exits_in_game_format(objects: &mut [Object], number: u16) {
    for object in objects {
        if let Object::ScreenExit(exit) = object
            && let Some(game) = exit.in_game_format(number)
        {
            *exit = game;
        }
    }
}

/// The secondary entrances that lead to a level, by number and
/// their bytes in the game's three tables. The game's format has 512, and
/// takes a destination's bit 8 from the entrance's number; one numbered in
/// the other bank, or past `1FF` up to the `1FFF` Lunar Magic's long exits
/// reach, builds with Kobo's exit code. One past that is dropped, with a
/// note.
fn entrances_leading_to(
    entrances: impl Iterator<Item = (u16, [u8; 3], EntranceSettings)>,
    notes: &mut Vec<String>,
) -> Vec<Entrance> {
    let mut kept = Vec::new();
    for (id, bytes, settings) in entrances {
        if id >= tables::MAX_ENTRANCES {
            notes.push(format!(
                "secondary entrance {id:03X} is past {:03X}, the last a screen exit can name; \
                 it is not imported",
                tables::MAX_ENTRANCES - 1
            ));
            continue;
        }
        kept.push(Entrance::from_bytes(id, bytes, settings));
    }
    kept
}

/// What an import did.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Report {
    /// Levels written.
    pub levels: Vec<u16>,
    /// Map16 pages written.
    pub map16: Vec<u8>,
    /// Notes, each naming its level.
    pub notes: Vec<String>,
    /// Ranges of the clean ROM's space the imported ROM changed, outside
    /// everything the import read: the ROM's changes Kobo does not model.
    pub unmodelled: Vec<(SnesAddr, usize)>,
    /// Tagged blocks past the clean ROM's end that nothing the import read
    /// lies in.
    pub unread_blocks: Vec<RatsBlock>,
}

/// Takes level `number` out of the project in `dir`: the manifest no
/// longer lists it, so it builds as the clean ROM has it, and its file is
/// deleted. Returns the file's path. A level the manifest does not list is
/// an error.
pub fn remove_level(dir: &Path, number: u16) -> Result<PathBuf, ImportError> {
    let manifest_path = dir.join(MANIFEST);
    let text = fs::read_to_string(&manifest_path).map_err(|source| ImportError::Io {
        path: manifest_path.clone(),
        source,
    })?;
    let (mut manifest, comments) =
        Manifest::from_toml(&text).map_err(|source| ImportError::Manifest {
            path: manifest_path.clone(),
            source,
        })?;
    let file = manifest
        .levels
        .remove(&number)
        .ok_or(ImportError::LevelNumber(number))?;
    fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| ImportError::Io {
        path: manifest_path,
        source,
    })?;
    let path = dir.join(&file);
    match fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(source) => Err(ImportError::Io { path, source }),
    }
}

/// A hack to import, from its file: a ROM, headered or not, or a BPS
/// patch of the clean ROM, which is applied to it.
pub fn read_hack(path: &Path, clean: &Rom) -> Result<Rom, ImportError> {
    let is_patch = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bps"));
    if !is_patch {
        return Ok(Rom::load(path)?);
    }
    let patch = fs::read(path).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let patched = crate::bps::apply_to_rom(&patch, clean).map_err(|source| ImportError::Bps {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Rom::from_headerless(patched.data)?)
}

/// Imports a ROM's levels into a new project in `dir`: every level that
/// differs from what the project builds onto, or every level with `all`.
/// That is the clean ROM, or for an SA-1 ROM the clean ROM with SA-1 Pack
/// ([`crate::build::base_image`], which needs SA-1 Pack configured); what
/// differs from it outside the levels is reported, and so is a level whose
/// data does not read, which is left out.
pub fn import_rom(rom: &Rom, clean: &Rom, dir: &Path, all: bool) -> Result<Report, ImportError> {
    import_rom_with(
        rom,
        clean,
        dir,
        &Options {
            all,
            ..Options::default()
        },
    )
}

/// How [`import_rom_with`] imports.
#[derive(Clone, Copy, Default, Debug)]
pub struct Options<'a> {
    /// Every level, not only those that differ from the base.
    pub all: bool,
    /// The hack's PIXI folder: its inputs ([`crate::tools::PIXI_INPUTS`])
    /// become the project's sprites' sources (`[pixi] dir`), which PIXI
    /// builds. Without it, a hack's PIXI insert is carried as the compiled
    /// code the ROM holds (`[pixi] compiled`, [`crate::pixi`]).
    pub pixi: Option<&'a Path>,
}

/// [`import_rom`], with the PIXI folder the hack's sprites came from, if
/// there is one.
pub fn import_rom_with(
    rom: &Rom,
    clean: &Rom,
    dir: &Path,
    options: &Options,
) -> Result<Report, ImportError> {
    let all = options.all;
    // As large as the ROM, if that is more than a build's default.
    let manifest = Manifest {
        sa1: rom.mapping().is_sa1(),
        // An SA-1 ROM whose GFX are LC_LZ3 builds them so; a LoROM one's
        // are built as LC_LZ2, which the game's own routine reads.
        lz3: rom.mapping().is_sa1()
            && gfx::Compression::detect(rom).is_ok_and(|c| c == gfx::Compression::Lz3),
        four_bpp: exgfx::is_4bpp(rom),
        rom_size: (rom.len() > crate::build::DEFAULT_ROM_SIZE).then_some(rom.len()),
        ..Manifest::default()
    };
    let sa1_base;
    let base = if manifest.sa1 {
        sa1_base = crate::build::base_image(clean, &manifest)
            .map_err(|e| ImportError::Base(Box::new(e)))?;
        &sa1_base
    } else {
        clean
    };
    let manifest_path = dir.join(MANIFEST);
    if manifest_path.exists() {
        return Err(ImportError::Exists(dir.to_path_buf()));
    }
    let write = |path: &Path, text: String| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(path, text).map_err(|source| ImportError::Io {
            path: path.to_path_buf(),
            source,
        })
    };
    let mut report = Report::default();
    let mut manifest = manifest;
    for number in 0..LEVEL_COUNT {
        // A level whose data does not read is left out, so a build keeps
        // the clean ROM's, rather than the whole import failing for it.
        let (level, mut notes) = match read_level(rom, number) {
            Ok(read) => read,
            Err(e) => {
                report.notes.push(format!(
                    "level {number:03X}: not imported, a build keeps the clean ROM's: {e}"
                ));
                continue;
            }
        };
        let background = match level.layer2 {
            Layer2::VanillaBackground(addr) if background_changed(rom, base, number)? => Some(addr),
            _ => None,
        };
        if let Some(addr) = background {
            notes.push(format!(
                "its background at {addr} was changed in place, which this build cannot \
                 write yet; the level file names the clean ROM's"
            ));
        }
        if !all && background.is_none() && read_level(base, number)?.0 == level {
            continue;
        }
        let file = PathBuf::from("levels").join(format!("{number:03X}.toml"));
        write(&dir.join(&file), level.to_toml(&Comments::default()))?;
        manifest.levels.insert(number, file);
        report.levels.push(number);
        report.notes.extend(
            notes
                .into_iter()
                .map(|n| format!("level {number:03X}: {n}")),
        );
    }
    let (files, notes) = read_gfx(rom, base)?;
    report.notes.extend(notes);
    for (index, image) in files {
        let file = PathBuf::from("graphics").join(format!("GFX{index:02X}.png"));
        let png = image
            .to_png()
            .map_err(|e| ImportError::Gfx(e.to_string()))?;
        let path = dir.join(&file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, png).map_err(|source| ImportError::Io { path, source })?;
        manifest.gfx.insert(index, file);
    }
    let (files, notes) = read_exgfx_files(rom)?;
    report.notes.extend(notes);
    for (number, file) in files {
        let (name, bytes) = match file {
            ExGfxImport::Image(image, bpp) => {
                let png = image
                    .to_png()
                    .map_err(|e| ImportError::Gfx(e.to_string()))?;
                (
                    ExGfxFile {
                        path: PathBuf::from("graphics").join(format!("ExGFX{number:X}.png")),
                        bpp,
                    },
                    png,
                )
            }
            ExGfxImport::Bytes(bytes) => (
                ExGfxFile {
                    path: PathBuf::from("graphics").join(format!("ExGFX{number:X}.bin")),
                    bpp: 4,
                },
                bytes,
            ),
        };
        let path = dir.join(&name.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, bytes).map_err(|source| ImportError::Io { path, source })?;
        manifest.exgfx.insert(number, name);
    }
    manifest.bypass_lists = exgfx::read_old_lists(rom)?.into_iter().collect();
    match exanimation::read_global(rom) {
        Ok(Some(found)) => {
            let file = PathBuf::from("animation").join("global.toml");
            write(
                &dir.join(&file),
                crate::source::animation::global_to_toml(&found.list, &Comments::default()),
            )?;
            manifest.animation_global = Some(file);
            report.notes.extend(
                animation_notes(Some(&found.list))
                    .into_iter()
                    .map(|n| format!("global ExAnimation: {n}")),
            );
        }
        Ok(None) => {}
        Err(e) => report.notes.push(format!(
            "global ExAnimation: does not read ({e}); it is not imported"
        )),
    }
    for (number, bytes) in exanimation::read_alt_files(rom)? {
        let file = PathBuf::from("animation").join(format!("ExGFX{number:X}.bin"));
        let path = dir.join(&file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, bytes).map_err(|source| ImportError::Io { path, source })?;
        manifest.animation_files.insert(number, file);
    }
    let (background, notes) = read_map16_bg(rom, base)?;
    report.notes.extend(notes);
    let (foreground, notes) = read_map16(rom)?;
    report.notes.extend(notes);
    let map16 = Map16Files {
        background,
        foreground,
        game: read_map16_game(rom, base)?,
        pipes: read_pipes(rom, base)?,
    };
    write_map16_files(dir, &mut manifest, &mut report, map16)?;
    write_shared_palettes(
        dir,
        &mut manifest,
        &crate::source::palettes::SharedPalettes::changes(rom, base)?,
    )?;
    let insert = crate::pixi::read(rom, base)?;
    let mut manifest_comments = Comments::default();
    if let Some(folder) = options.pixi {
        let found = crate::tools::copy_pixi_inputs(folder, &dir.join("pixi"))?;
        manifest.pixi = Some(PathBuf::from("pixi"));
        // PIXI has refused `-d255spl` since 1.41, so a hack whose PIXI kept
        // the game's 128 load flags gets 255 sprites when built: its own
        // code may use the RAM PIXI then takes.
        if insert.as_ref().is_some_and(|(found, _)| !found.sprites_255) {
            report.notes.push(
                "sprites: the hack's PIXI keeps 128 load flags at $1938; PIXI 1.43 always \
                 moves them to $7FAF00 for 255 sprites a level and uses $1938 for its own \
                 tables, so check the hack's own code uses neither"
                    .into(),
            );
        }
        report.notes.push(format!(
            "sprites: PIXI's inputs ({}) copied from {} into pixi/, which the build runs \
             PIXI 1.43 on",
            found.join(", "),
            folder.display()
        ));
    } else if let Some((found, notes)) = &insert {
        let version = crate::pixi::version_name(found.version);
        // Code that cannot go where the hack has it cannot go anywhere: only
        // the size table, which is data, is carried then.
        let conflicts = crate::pixi::conflicts(found, base);
        let carried = if conflicts.is_empty() {
            found.clone()
        } else {
            crate::pixi::Insert {
                version: found.version,
                size_table: crate::pixi::size_table(rom),
                ..Default::default()
            }
        };
        let folder = PathBuf::from("pixi");
        let mut files: Vec<(PathBuf, &[u8])> = carried
            .blocks
            .iter()
            .map(|(&at, bytes)| (crate::source::pixi::block_file(at), bytes.as_slice()))
            .collect();
        let size_file = PathBuf::from("compiled").join("sizes.bin");
        if let Some(table) = &carried.size_table {
            files.push((size_file.clone(), table));
        }
        for (file, bytes) in files {
            let path = dir.join(&folder).join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| ImportError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&path, bytes).map_err(|source| ImportError::Io { path, source })?;
        }
        let compiled = crate::source::pixi::Compiled {
            version: carried.version,
            sprites_255: carried.sprites_255,
            sites: carried.sites.clone(),
            blocks: carried
                .blocks
                .keys()
                .map(|&at| (at, crate::source::pixi::block_file(at)))
                .collect(),
            size_table: carried.size_table.as_ref().map(|_| size_file.clone()),
        };
        // Kept in the files, so that whoever opens the project later learns
        // what the compiled insert is, what it cannot do, and how to leave it.
        let explained = compiled_pixi_comment(&version, !conflicts.is_empty(), manifest.sa1);
        let mut top = Comments::default();
        top.add(Comments::TOP, explained.clone());
        let file = folder.join("compiled.toml");
        write(&dir.join(&file), compiled.to_toml(&top))?;
        manifest.pixi_compiled = Some(file);
        manifest_comments.add("pixi", explained);
        if conflicts.is_empty() {
            // The blocks stay where the hack has them, which leaves the free
            // space around them in pieces; Kobo's own tables take whole
            // banks. A megabyte more gives them room, up to 4 MiB.
            let size = manifest.rom_size.unwrap_or(crate::build::DEFAULT_ROM_SIZE);
            if size < 0x40_0000 {
                manifest.rom_size = Some((rom.len().max(size) + 0x10_0000).min(0x40_0000));
            }
            report.notes.push(format!(
                "sprites: PIXI {version}'s insert ({} blocks, {} bytes, {} sites) carried as \
                 compiled code in pixi/compiled.toml; with the sprites' sources, import with \
                 `--pixi <folder>` to build them from source",
                found.blocks.len(),
                found.block_bytes(),
                found.sites.len()
            ));
            report
                .notes
                .extend(notes.iter().map(|n| format!("sprites: {n}")));
        } else {
            let at: Vec<String> = conflicts.iter().take(4).map(|a| a.to_string()).collect();
            report.notes.push(format!(
                "sprites: PIXI {version}'s code is not carried: {} of its blocks ({}{}) are \
                 where the base has something of its own{}; only its size table is, so the \
                 levels build with their sprites' extension bytes but the custom sprites have \
                 no code. With their sources, import with `--pixi <folder>`",
                conflicts.len(),
                at.join(", "),
                if conflicts.len() > 4 { ", ..." } else { "" },
                if manifest.sa1 {
                    " (SA-1 Pack 1.40's, where the hack has another version)"
                } else {
                    ""
                }
            ));
        }
    }
    write(&manifest_path, manifest.to_toml(&manifest_comments))?;
    if options.pixi.is_some() {
        report.notes.extend(check_pixi_sources(rom, clean, dir));
    }
    let mut read = read_spans(rom)?;
    // PIXI's insert, which the project carries or builds again.
    if let Some((insert, _)) = &insert {
        read.extend(crate::pixi::spans(rom, insert));
    }
    // The tag of a block that holds something read is read with it: an
    // SA-1 base reaches past the vanilla image, where such blocks go.
    for block in rats::blocks(rom) {
        let Ok(start) = rom.pc(block.start).map(|pc| pc.as_usize()) else {
            continue;
        };
        if read
            .iter()
            .any(|r| r.start < start + block.len && start < r.end)
        {
            read.push(start - rats::TAG_LEN..start);
        }
    }
    read.sort_by_key(|r| r.start);
    report.unmodelled = unmodelled(rom, base, &read);
    report.unread_blocks = rats::blocks(rom)
        .into_iter()
        .filter(|block| {
            let start = rom.pc(block.start).map_or(0, |pc| pc.as_usize());
            start >= base.len()
                && !read
                    .iter()
                    .any(|r| r.start < start + block.len && start < r.end)
        })
        .collect();
    Ok(report)
}

/// The comment an import puts before `[pixi]` in the manifest and at the
/// top of `compiled.toml`: what a compiled insert is, what it cannot do, and
/// how a project moves to its sprites' sources. `size_only` when the code
/// could not be carried and only the size table was.
fn compiled_pixi_comment(version: &str, size_only: bool, sa1: bool) -> Vec<String> {
    let text = if size_only {
        format!(
            "The hack's custom sprites could not be carried. `kobo import` had no sources \
             for them, and the code PIXI {version} compiled for them sits where {} has \
             something of its own, and compiled code cannot move. Only PIXI's size table \
             is carried (pixi/compiled.toml, pixi/compiled/sizes.bin), so every level \
             builds with its sprites' extension bytes, but the custom sprites have no code \
             and will not work in play. To fix it, give the sprites' sources: import \
             again with `--pixi <folder>`, or give `dir` in place of `compiled`, a folder \
             holding every sprite the hack uses.",
            if sa1 {
                "SA-1 Pack 1.40, which this project builds on (the hack was made with \
                 another version),"
            } else {
                "the image this project builds on"
            }
        )
    } else {
        format!(
            "The hack's custom sprites, carried by `kobo import` as the code PIXI \
             {version} compiled for them, since the import had no sources for them. \
             pixi/compiled.toml lists PIXI's hooks, and pixi/compiled/ holds each block of \
             its code and tables as a .bin file, which a build writes back at the same \
             address; the ROM is up to a megabyte larger than the hack's, to leave room \
             around them. They cannot be edited, and code they call in other tools (UberASM Tool, \
             GPS, patches) is not carried, so a sprite may misbehave in play. They are a \
             stopgap, meant to be replaced with the sprites' sources: import again with \
             `--pixi <folder>`, or give `dir` in place of `compiled`. It is one or the \
             other, since PIXI installs its own code over the compiled one, so the folder \
             must hold every sprite the hack uses."
        )
    };
    wrap_comment(&text, 88)
}

/// `text` as `# ` comment lines of at most `width` characters.
fn wrap_comment(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::from("#");
    for word in text.split_whitespace() {
        if line.len() + 1 + word.len() > width && line != "#" {
            lines.push(std::mem::replace(&mut line, String::from("#")));
        }
        line.push(' ');
        line.push_str(word);
    }
    lines.push(line);
    lines
}

/// Builds the project in `dir` as far as PIXI, and compares the size table
/// PIXI makes from the project's sources with the hack's: an entry that
/// differs makes a level that places that sprite fail to build.
fn check_pixi_sources(rom: &Rom, clean: &Rom, dir: &Path) -> Vec<String> {
    use crate::build::{Project, Stage, build_through};
    let built = Project::load(dir).and_then(|p| build_through(clean, &p, Stage::Sprites));
    let built = match built {
        Ok(built) => built,
        Err(e) => {
            return vec![format!(
                "sprites: PIXI did not build the project's sources ({e}); a build fails until \
                 it does"
            )];
        }
    };
    let ours = sprites::pixi_size_table(&built).ok().flatten();
    let theirs = sprites::pixi_size_table(rom).ok().flatten();
    let (Some(ours), Some(theirs)) = (ours, theirs) else {
        return Vec::new();
    };
    let differ: Vec<String> = (0..0x400)
        .filter(|&i| ours[i].max(3) != theirs[i].max(3))
        .map(|i| format!("{:02X} with extra bits {}", i & 0xFF, i >> 8))
        .collect();
    if differ.is_empty() {
        return vec!["sprites: PIXI builds the sources with the hack's sprite sizes".to_string()];
    }
    vec![format!(
        "sprites: PIXI sizes {} sprites differently from the hack ({}{}); levels that place \
         them do not build",
        differ.len(),
        differ
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        if differ.len() > 8 { ", ..." } else { "" }
    )]
}

/// Whether the level's background, which [`read_level`] names by its
/// address in the clean ROM, is not what the clean ROM holds there: a
/// ROM may change one of Nintendo's backgrounds in place and keep the
/// pointer, which a level file cannot say yet.
fn background_changed(rom: &Rom, clean: &Rom, number: u16) -> Result<bool, ImportError> {
    let Some(bg) = level::read_background(rom, number)? else {
        return Ok(false);
    };
    let same = clean
        .read_tail(bg.address)
        .ok()
        .and_then(|stream| rle1::decompress(stream).ok())
        .is_some_and(|original| original.data == bg.data);
    Ok(!same)
}

/// A background in Lunar Magic's layout as a level file has it: 32 rows
/// from its own format with high bytes (`C` and `F`), 27 from the game's
/// behind a full pointer (`V`), whose tiles all take the flags' high nibble
/// as their high byte. Its own format without high bytes (`C` alone, from
/// older versions) comes through as 32 rows, the last five tile `$000`.
fn background_tiles(bg: &level::Background) -> Option<BackgroundTiles> {
    let flags = bg.flags?;
    let byte = |i: usize| bg.data.get(i).copied().unwrap_or(0) as u16;
    let mut tiles = vec![0; BACKGROUND_ROWS * 32];
    let (table, rows) = match (flags & 0x02 != 0, flags & 0x04 != 0, flags & 0x08 != 0) {
        (true, true, _) => {
            for (i, tile) in tiles.iter_mut().enumerate() {
                let (row, half, col) = (i / 32, i % 32 / 16, i % 16);
                let at = half * 512 + row * 16 + col;
                *tile = byte(1024 + at) << 8 | byte(at);
            }
            (flags >> 4, 32)
        }
        (custom, false, vanilla) if custom || vanilla => {
            let high = (flags as u16 >> 4) << 8;
            for row in 0..27 {
                for half in 0..2 {
                    for col in 0..16 {
                        tiles[row * 32 + half * 16 + col] =
                            high | byte(half * 432 + row * 16 + col);
                    }
                }
            }
            if custom { (flags >> 4, 32) } else { (0, 27) }
        }
        _ => return None,
    };
    Some(BackgroundTiles { table, rows, tiles })
}

/// A background from an MWL file, as [`background_tiles`] reads it from a
/// ROM. The file has the tiles of 32 rows, the left half's first, with the
/// flags of `$0EF310`: `C` and `F` for Lunar Magic's own format, the BG
/// Map16 table in the top nibble, and `V` for the game's (docs/lunar-magic.md).
fn background_from_mwl(flags: u8, mwl_tiles: &[u16]) -> Option<BackgroundTiles> {
    if mwl_tiles.len() != BACKGROUND_ROWS * 32 {
        return None;
    }
    let mut tiles = vec![0; BACKGROUND_ROWS * 32];
    for (i, &tile) in mwl_tiles.iter().enumerate() {
        let (half, row, col) = (i / 512, i % 512 / 16, i % 16);
        tiles[row * 32 + half * 16 + col] = tile;
    }
    let (custom, full, vanilla) = (flags & 0x02 != 0, flags & 0x04 != 0, flags & 0x08 != 0);
    // The file's own reading. From a ROM in Lunar Magic's older format (`C`
    // alone), 3.70 exports `C` and `F` with table 0 and the table folded
    // into the tiles, where a ROM import takes that high byte as the table;
    // the file alone cannot tell it from a table 0 background.
    let (table, rows) = match (custom, full, vanilla) {
        (true, true, _) => (flags >> 4, BACKGROUND_ROWS),
        (false, _, true) => (0, 27),
        _ => return None,
    };
    Some(BackgroundTiles { table, rows, tiles })
}

/// GFX files by number, as images, and notes on what was left out.
pub type GfxImport = (Vec<(u8, IndexedImage)>, Vec<String>);

/// An ExGFX file as a project holds it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExGfxImport {
    /// Tiles, and the bits a pixel takes.
    Image(IndexedImage, u8),
    /// Bytes as they are: a file used for other things than tiles of one
    /// depth (layer 3's tilemap, the animated tiles' source), or not a
    /// whole number of rows of 16 tiles.
    Bytes(Vec<u8>),
}

/// ExGFX files by number, and notes on what was left out.
pub type ExGfxFiles = (Vec<(u16, ExGfxImport)>, Vec<String>);

/// The ROM's ExGFX files, each as an image of the depth the levels' lists
/// use it at, or as bytes; and notes on those that cannot be read.
pub fn read_exgfx_files(rom: &Rom) -> Result<ExGfxFiles, ImportError> {
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    if !exgfx::has_exgfx(rom) {
        return Ok((out, notes));
    }
    let compression = match gfx::Compression::detect(rom) {
        Ok(c) => c,
        Err(gfx::GfxError::Locked) => {
            notes.push("the ROM is locked, so its ExGFX files are not imported".into());
            return Ok((out, notes));
        }
        Err(e) => return Err(ImportError::Gfx(e.to_string())),
    };
    // How each file is used: 4 (sprites, FG and BG), 2 (layer 3's files),
    // or 0 (anything else).
    let mut uses: BTreeMap<u16, BTreeSet<u8>> = BTreeMap::new();
    for number in 0..LEVEL_COUNT {
        let Some(list) = exgfx::read_list(rom, number)?.filter(|l| l.is_used()) else {
            continue;
        };
        for (s, file) in list.files() {
            let depth = match s {
                exgfx::slot::AN2 | exgfx::slot::LT3 => 0,
                exgfx::slot::LG1 | exgfx::slot::LG2 | exgfx::slot::LG3 | exgfx::slot::LG4 => 2,
                _ => 4,
            };
            uses.entry(file).or_default().insert(depth);
        }
    }
    for (_, files) in exgfx::read_old_lists(rom)? {
        for f in files {
            uses.entry(f as u16).or_default().insert(4);
        }
    }
    // The submaps' lists: a build writes the overworld's own files there,
    // as it carries no overworld graphics yet (roadmap step 4).
    let mut submaps = Vec::new();
    for n in exgfx::SUBMAP_LISTS..exgfx::SUBMAP_LISTS + 7 {
        let files = |l: &exgfx::GraphicsList| l.0.map(|w| w & 0xFFF);
        if exgfx::read_list(rom, n)?
            .is_some_and(|l| files(&l) != files(&exgfx::GraphicsList::SUBMAP_DEFAULT))
        {
            submaps.push(n - exgfx::SUBMAP_LISTS);
        }
    }
    if !submaps.is_empty() {
        let list: Vec<String> = submaps.iter().map(|n| n.to_string()).collect();
        notes.push(format!(
            "the graphics lists of submaps {} name other files than the overworld's own, \
             which builds write there: overworld graphics are not carried yet",
            list.join(", ")
        ));
    }
    for file in exgfx::EXGFX_FIRST..=exgfx::EXGFX_LAST {
        let data = match exgfx::read_exgfx(rom, file, compression) {
            Ok(Some(d)) => d.data,
            Ok(None) => continue,
            Err(e) => {
                notes.push(format!("{e}; not imported"));
                continue;
            }
        };
        let depths = uses.get(&file).cloned().unwrap_or_default();
        let depth = match depths.iter().copied().collect::<Vec<_>>()[..] {
            [] | [4] => Some(gfx::Bpp::Four),
            [2] => Some(gfx::Bpp::Two),
            _ => None,
        };
        let row = |bpp: gfx::Bpp| bpp.bytes_per_tile() * 16;
        out.push((
            file,
            match depth {
                Some(bpp) if !data.is_empty() && data.len() % row(bpp) == 0 => {
                    let tiles = gfx::decode_tiles(bpp, &data);
                    ExGfxImport::Image(gfx::tiles_to_image(&tiles, bpp.colors()), bpp.bits())
                }
                _ => ExGfxImport::Bytes(data),
            },
        ));
    }
    Ok((out, notes))
}

/// GFX files `00` to `33` that differ from `base`'s, as images, and notes on
/// those left out: all of a locked ROM's, and any stored in another format
/// than `base`'s. In a ROM with Lunar Magic's 4bpp files, a file the game
/// keeps as 3bpp is compared with Lunar Magic's 4bpp version of `base`'s.
pub fn read_gfx(rom: &Rom, base: &Rom) -> Result<GfxImport, ImportError> {
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    let reader = match gfx::GfxReader::new(rom) {
        Ok(reader) => reader,
        Err(gfx::GfxError::Locked) => {
            notes.push("the ROM is locked, so its GFX files are not imported".into());
            return Ok((out, notes));
        }
        Err(e) => return Err(ImportError::Gfx(e.to_string())),
    };
    let clean = gfx::GfxReader::new(base).map_err(|e| ImportError::Gfx(e.to_string()))?;
    let four_bpp = exgfx::is_4bpp(rom);
    for index in 0..gfx::GFX_FILE_COUNT {
        let theirs = match reader.read(index) {
            Ok(file) => file,
            Err(e) => {
                notes.push(format!("GFX{index:02X}: {e}; not imported"));
                continue;
            }
        };
        let ours = clean
            .read(index)
            .map_err(|e| ImportError::Gfx(e.to_string()))?;
        if four_bpp
            && exgfx::converts(ours.format)
            && theirs.format == gfx::GfxFormat::Planar(gfx::Bpp::Four)
        {
            // Stored as Lunar Magic stores the game's file in a 4bpp ROM,
            // which is what a 4bpp build writes for a file it does not list.
            if theirs.tile_count() != ours.tile_count() {
                notes.push(format!(
                    "GFX{index:02X} has {} tiles, where the game has {}; not imported",
                    theirs.tile_count(),
                    ours.tile_count()
                ));
            } else if theirs.data != exgfx::stored_4bpp(&ours) {
                out.push((index, gfx::tiles_to_image(&theirs.tiles(), 16)));
            }
        } else if theirs.format != ours.format {
            notes.push(format!(
                "GFX{index:02X} is stored as {:?}, where the game has {:?}; not imported",
                theirs.format, ours.format
            ));
        } else if theirs.tiles().len() != ours.tiles().len() {
            // The build keeps the game's size for each file.
            notes.push(format!(
                "GFX{index:02X} has {} tiles, where the game has {}; not imported",
                theirs.tiles().len(),
                ours.tiles().len()
            ));
        } else if theirs.data != ours.data {
            out.push((index, gfx::tiles_to_image(&theirs.tiles(), theirs.colors())));
        }
    }
    Ok((out, notes))
}

/// BG Map16 pages from Lunar Magic's tables: the pages of each table that
/// lie in the RATS block it starts in, the last as far as the block goes,
/// with their tiles but empty ones; and the first table's pages 0 and 1,
/// the game's own, wherever that table is, with only the tiles that differ
/// from `clean`'s, which is what a build keeps for a tile they leave out.
pub fn read_map16_bg(rom: &Rom, clean: &Rom) -> Result<Map16Import, ImportError> {
    use crate::map16::tables::MAP16_BG_TILES;
    let (mut out, notes) = (Vec::new(), Vec::new());
    let blocks = rats_spans(rom);
    let page_len = PAGE_TILES as usize * 8;
    // A table ends at the tile after its last used one, so a page may end
    // early: its tiles past the block are empty. A page's tiles are listed
    // where they differ from `base`'s: the clean ROM's for the game's own
    // pages, else empty ones.
    let read_page = |at: SnesAddr, number: u8, len: usize| -> Result<Map16Page, ImportError> {
        let mut bytes = rom.read(at, len - len % 8)?.to_vec();
        bytes.resize(page_len, 0);
        let base = if number < 2 {
            clean
                .read(
                    MAP16_BG_TILES.add(number as u32 * page_len as u32),
                    page_len,
                )?
                .to_vec()
        } else {
            vec![0; page_len]
        };
        let mut page = Map16Page::default();
        for (i, (def, theirs)) in bytes.chunks(8).zip(base.chunks(8)).enumerate() {
            if def != theirs {
                let entry = Map16Entry {
                    gfx: Map16Tile::from_bytes(def.try_into().expect("8 bytes")),
                    ..Map16Entry::default()
                };
                page.tiles
                    .insert(number as u16 * PAGE_TILES + i as u16, entry);
            }
        }
        Ok(page)
    };
    for table in 0..16u8 {
        let Some(at) = map16_pages::bg_table(rom, table)? else {
            continue;
        };
        // The game's own table, at its address or a mirror of it
        // (`$8D9100`, as a FastROM hack may store it).
        if table == 0 && rom.pc(at).ok() == rom.pc(MAP16_BG_TILES).ok() {
            for page in 0..2u8 {
                let tiles = read_page(at.add(page as u32 * page_len as u32), page, page_len)?;
                if !tiles.tiles.is_empty() {
                    out.push((page, tiles));
                }
            }
            continue;
        }
        let Some(block) = rom
            .pc(at)
            .ok()
            .and_then(|pc| blocks.iter().find(|b| b.contains(&pc.as_usize())).cloned())
        else {
            continue;
        };
        for page in 0..16u8 {
            let start = at.add(page as u32 * page_len as u32);
            let Ok(pc) = rom.pc(start) else { break };
            if pc.as_usize() >= block.end {
                break;
            }
            let number = table * 16 + page;
            let tiles = read_page(start, number, page_len.min(block.end - pc.as_usize()))?;
            if !tiles.tiles.is_empty() {
                out.push((number, tiles));
            }
        }
    }
    Ok((out, notes))
}

/// Map16 pages by number, and notes on what was left out.
pub type Map16Import = (Vec<(u8, Map16Page)>, Vec<String>);

/// Everything of a hack's Map16 a project holds, as an import read it.
#[derive(Clone, Default, Debug)]
pub struct Map16Files {
    pub background: Vec<(u8, Map16Page)>,
    pub foreground: Vec<(u8, Map16Page)>,
    pub game: GameMap16,
    pub pipes: Pipes,
}

/// Writes `map16`'s page, tileset, and pipes files into the project in
/// `dir` and lists them in `manifest`.
fn write_map16_files(
    dir: &Path,
    manifest: &mut Manifest,
    report: &mut Report,
    map16: Map16Files,
) -> Result<(), ImportError> {
    for (page, tiles) in map16.background {
        let file = PathBuf::from("map16").join(format!("bg-{page:02X}.toml"));
        write_text(
            &dir.join(&file),
            tiles.to_toml(PageKind::Background, &PageComments::default()),
        )?;
        manifest.map16_bg.insert(page, file);
    }
    for (page, tiles) in map16.foreground {
        let file = PathBuf::from("map16").join(format!("{page:02X}.toml"));
        write_text(
            &dir.join(&file),
            tiles.to_toml(PageKind::Foreground, &PageComments::default()),
        )?;
        manifest.map16.insert(page, file);
        report.map16.push(page);
    }
    report.notes.extend(map16.game.notes);
    for (page, tiles) in map16.game.pages {
        let file = PathBuf::from("map16").join(format!("{page:02X}.toml"));
        write_text(&dir.join(&file), tiles.to_toml(&PageComments::default()))?;
        manifest.map16.insert(page, file);
        report.map16.push(page);
    }
    for (tileset, tiles) in map16.game.tilesets {
        let file = PathBuf::from("map16").join(format!("tileset-{tileset:X}.toml"));
        write_text(
            &dir.join(&file),
            tiles.to_toml(PageKind::Tileset, &PageComments::default()),
        )?;
        manifest.map16_tileset.insert(tileset, file);
    }
    if !map16.pipes.is_empty() {
        let file = PathBuf::from("map16").join("pipes.toml");
        write_text(&dir.join(&file), map16.pipes.to_toml(&[]))?;
        manifest.map16_pipes = Some(file);
    }
    report.map16.sort_unstable();
    Ok(())
}

/// Writes `text` to `path`, making its folder.
/// Writes what a hack changes of the game's shared colour tables as
/// `palettes/shared.toml`, if anything, and names it in the manifest.
fn write_shared_palettes(
    dir: &Path,
    manifest: &mut Manifest,
    palettes: &crate::source::palettes::SharedPalettes,
) -> Result<(), ImportError> {
    if palettes.is_empty() {
        return Ok(());
    }
    let file = PathBuf::from("palettes").join("shared.toml");
    write_text(&dir.join(&file), palettes.to_toml(&[]))?;
    manifest.shared_palettes = Some(file);
    Ok(())
}

fn write_text(path: &Path, text: impl AsRef<[u8]>) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, text).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Map16 pages 2 to `$7F` from Lunar Magic's tables: the pages of each
/// group of 16 that lie in the RATS block its table starts in (Lunar Magic
/// allocates a group up to the last tile it uses, so the last page may
/// stop early, its other tiles empty), with what their tiles
/// act like, but empty tiles ([`Map16Entry::default`]), which a page file
/// leaves out, and pages with nothing else, which a build writes for the
/// pages of a group a project does not list.
/// A group with no table where 2.52 and later keep it, or one in no RATS
/// block, is in an older Lunar Magic's layout ([`read_map16_older`]).
pub fn read_map16(rom: &Rom) -> Result<Map16Import, ImportError> {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    if !map16_pages::installed(rom) {
        return Ok((out, notes));
    }
    let blocks = rats_spans(rom);
    // With page 2 per tileset, the game reads its graphics from the
    // tilesets' table (read_map16_game), and only what it acts like here.
    let tileset_page2 = map16_pages::tileset_page2(rom)?;
    let page_span = |group: &map16_pages::PageGroup, page: u8| -> Option<Range<usize>> {
        let at = group.definition(rom, page as u16 * PAGE_TILES).ok()??;
        let pc = rom.pc(at).ok()?.as_usize();
        Some(pc..pc + PAGE_TILES as usize * 8)
    };
    let mut older = Vec::new();
    for group in &PAGE_GROUPS {
        let first = *group.pages().start();
        let block = group
            .definition(rom, first as u16 * PAGE_TILES)?
            .and_then(|_| page_span(group, first))
            .and_then(|span| blocks.iter().find(|b| b.contains(&span.start)).cloned());
        let Some(block) = block else {
            older.push(group.pages());
            continue;
        };
        // The pages past the block's end have no definitions, but what
        // their tiles act like is in the acts-like tables all the same, as
        // for a group Lunar Magic has not allocated.
        let mut ended = false;
        for page in group.pages() {
            ended |= !page_span(group, page)
                .is_some_and(|span| block.start <= span.start && span.start < block.end);
            let mut tiles = Map16Page::default();
            let first = page as u16 * PAGE_TILES;
            for tile in first..first + PAGE_TILES {
                let gfx = if ended || (tileset_page2 && page == 0x02) {
                    Map16Tile::default()
                } else {
                    let at = group.definition(rom, tile)?.expect("the group has a table");
                    // Past the block's end, where a group's last page may stop.
                    let inside = rom.pc(at).is_ok_and(|pc| pc.as_usize() + 8 <= block.end);
                    if inside {
                        Map16Tile::from_bytes(rom.read(at, 8)?.try_into().expect("8 bytes"))
                    } else {
                        Map16Tile::default()
                    }
                };
                let entry = Map16Entry {
                    gfx,
                    acts: map16_pages::acts_like(rom, tile)?.unwrap_or(DEFAULT_ACTS),
                };
                if entry != Map16Entry::default() {
                    tiles.tiles.insert(tile, entry);
                }
            }
            if !tiles.tiles.is_empty() {
                out.push((page, tiles));
            }
        }
    }
    // A group without a table beside groups in the current layout is one
    // Lunar Magic has not allocated, but what its tiles act like is in the
    // acts-like tables all the same.
    if older.len() == PAGE_GROUPS.len() {
        let (pages, more) = read_map16_older(rom, &older, &blocks, tileset_page2)?;
        out.extend(pages);
        notes.extend(more);
    } else {
        for page in older.into_iter().flatten() {
            let mut tiles = Map16Page::default();
            let first = page as u16 * PAGE_TILES;
            for tile in first..first + PAGE_TILES {
                let acts = map16_pages::acts_like(rom, tile)?.unwrap_or(DEFAULT_ACTS);
                if acts != DEFAULT_ACTS {
                    tiles.tiles.insert(
                        tile,
                        Map16Entry {
                            acts,
                            ..Map16Entry::default()
                        },
                    );
                }
            }
            if !tiles.tiles.is_empty() {
                out.push((page, tiles));
            }
        }
    }
    out.sort_by_key(|(page, _)| *page);
    Ok((out, notes))
}

/// Map16 pages past 1 in an older Lunar Magic's layout (before 2.52, or a
/// ROM a later one saved without saving its Map16): read through the ROM's
/// own Map16 routine, which finds a tile in its version's layout. A page is
/// the ROM's where its first tile lies past the game's 512 KiB, outside the
/// acts-like tables (which a 16-page install's routine reads for the pages
/// it does not have), and not where an earlier page's is (a 64-page
/// install's routine reads pages `40` and up as `00` and up, and unused
/// pages may share one empty page). A group of 16 pages keeps to the RATS
/// block its first page is in, up to the block's end (a later page in
/// another block is not the group's), or to no block, a page whole: what
/// Lunar Magic 3.70's full export of the ROM shows (ten corpus hacks, Lunar
/// Magic 1.62 to 2.52, 2026-10-04).
fn read_map16_older(
    rom: &Rom,
    groups: &[std::ops::RangeInclusive<u8>],
    blocks: &[Range<usize>],
    tileset_page2: bool,
) -> Result<Map16Import, ImportError> {
    const GAME_END: usize = 0x8_0000;
    const PAGE_BYTES: usize = PAGE_TILES as usize * 8;
    let mut out = Vec::new();
    let mut notes = Vec::new();
    // Pages 0 and 1 first, which only say where those are. Pages `40` and
    // up came with 2.50, two versions before the current layout, and the
    // routines of older ones read them as something else.
    let pages: Vec<u8> = [0, 1]
        .into_iter()
        .chain(groups.iter().flat_map(|g| g.clone()).filter(|&p| p < 0x40))
        .collect();
    let tiles: Vec<u16> = pages
        .iter()
        .flat_map(|&p| p as u16 * PAGE_TILES..(p as u16 + 1) * PAGE_TILES)
        .collect();
    let addresses = match crate::expand::map16_addresses(rom, 0x105, &tiles) {
        Ok(addresses) => addresses,
        Err(e) => {
            notes.push(format!(
                "Map16 pages past 1 are in an older Lunar Magic's layout, which the ROM's own \
                 routine did not read ({e}); not imported"
            ));
            return Ok((out, notes));
        }
    };
    let pc = |at: Option<SnesAddr>| at.and_then(|a| rom.pc(a).ok()).map(|p| p.as_usize());
    let mut acts = Vec::new();
    for pointer in [map16_pages::ACTS_LIKE, map16_pages::ACTS_LIKE_UPPER] {
        let table = rom.read_u24(pointer)?;
        if let Some(start) = (table >> 16 != 0xFF)
            .then(|| pc(Some(SnesAddr::new(table))))
            .flatten()
        {
            acts.push(start..start + 0x8000);
        }
    }
    let mut read = Vec::new();
    let mut seen = BTreeSet::new();
    // A group's pages are in the RATS block of the first the ROM has, up
    // to its end, or in none.
    let mut group_block: Option<(u8, Option<&Range<usize>>)> = None;
    for (i, &page) in pages.iter().enumerate() {
        let at = &addresses[i * PAGE_TILES as usize..(i + 1) * PAGE_TILES as usize];
        let Some(start) = pc(at[0]) else {
            continue;
        };
        if !seen.insert(start)
            || page < 2
            || start < GAME_END
            || acts
                .iter()
                .any(|a| a.start < start + PAGE_BYTES && start < a.end)
        {
            continue;
        }
        let block = blocks.iter().find(|b| b.contains(&start));
        match group_block {
            Some((group, first)) if group == page / 16 => {
                if block != first {
                    continue;
                }
            }
            _ => group_block = Some((page / 16, block)),
        }
        let end = block.map_or(start + PAGE_BYTES, |b| b.end);
        let mut entries = Map16Page::default();
        for (j, &tile_at) in at.iter().enumerate() {
            let tile = page as u16 * PAGE_TILES + j as u16;
            let inside = pc(tile_at).is_some_and(|p| p >= start && p + 8 <= end);
            let gfx = match tile_at {
                Some(a) if inside && !(tileset_page2 && page == 0x02) => {
                    Map16Tile::from_bytes(rom.read(a, 8)?.try_into().expect("8 bytes"))
                }
                _ => Map16Tile::default(),
            };
            // The upper acts-like table's pointer is something else in an
            // older layout, and may lead outside the image.
            let acts = map16_pages::acts_like(rom, tile).ok().flatten();
            let entry = Map16Entry {
                gfx,
                acts: acts.unwrap_or(DEFAULT_ACTS),
            };
            if entry != Map16Entry::default() {
                entries.tiles.insert(tile, entry);
            }
        }
        if !entries.tiles.is_empty() {
            out.push((page, entries));
            read.push(page);
        }
    }
    if !read.is_empty() {
        let list: Vec<String> = read.iter().map(|p| format!("{p:02X}")).collect();
        notes.push(format!(
            "Map16 pages {} are in an older Lunar Magic's layout, read through the ROM's own \
             Map16 routine",
            list.join(", ")
        ));
    }
    Ok((out, notes))
}

/// The vertical pipes' colours and the diagonal pipes as a ROM has them
/// against `clean`: each tile whose definition differs.
pub fn read_pipes(rom: &Rom, clean: &Rom) -> Result<Pipes, ImportError> {
    let mut out = Pipes::default();
    let definition = |rom: &Rom, at: SnesAddr| -> Result<Map16Tile, ImportError> {
        Ok(Map16Tile::from_bytes(
            rom.read(at, 8)?.try_into().expect("8 bytes"),
        ))
    };
    for set in crate::source::map16::PIPE_SETS {
        for tile in crate::map16::PIPE_COLOUR_TILES {
            let at = crate::map16::pipe_address(Some(set), tile).expect("a pipe tile");
            let gfx = definition(rom, at)?;
            if gfx != definition(clean, at)? {
                out.colours.insert((set, tile), gfx);
            }
        }
    }
    for tile in crate::map16::diagonal_pipe_tiles() {
        let at = crate::map16::pipe_address(None, tile).expect("a pipe tile");
        let gfx = definition(rom, at)?;
        if gfx != definition(clean, at)? {
            out.diagonal.insert(tile, gfx);
        }
    }
    Ok(out)
}

/// What a ROM changes of Map16 pages 0 and 1, the game's own tables, and
/// the object tilesets' own tiles ([`read_map16_game`]).
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct GameMap16 {
    /// Pages 0 and 1, where a tile's graphics or what it acts like differ
    /// from the clean ROM's.
    pub pages: Vec<(u8, GamePage)>,
    /// By object tileset: the tiles of pages 0 and 1 the game keeps per
    /// tileset that differ from the clean ROM's, under the first tileset
    /// of those sharing a table; and page 2 when it is per tileset.
    pub tilesets: Vec<(u8, Map16Page)>,
    pub notes: Vec<String>,
}

/// Map16 pages 0 and 1 as a ROM has them against `clean`: each tile whose
/// graphics or whose acts-like setting (in Lunar Magic's table) differs;
/// and, when page 2 is per tileset, each tileset's page 2 but for empty
/// tiles.
pub fn read_map16_game(rom: &Rom, clean: &Rom) -> Result<GameMap16, ImportError> {
    let mut out = GameMap16::default();
    let mut pages = [GamePage::default(), GamePage::default()];
    if map16_pages::installed(rom) {
        for tile in 0..0x200u16 {
            if let Some(acts) = map16_pages::acts_like(rom, tile)?.filter(|&a| a != tile) {
                pages[tile as usize >> 8].tiles.insert(
                    tile,
                    GameTile {
                        acts: Some(acts),
                        gfx: None,
                    },
                );
            }
        }
    }
    let game = GameTables::read(clean)?;
    let mut tilesets: std::collections::BTreeMap<u8, Map16Page> = Default::default();
    if GameTables::read(rom)? != game {
        out.notes.push(
            "the game's Map16 tables for pages 0 and 1 are laid out differently from the clean \
             ROM's; their graphics are not imported"
                .into(),
        );
    } else {
        let definition = |rom: &Rom, at: SnesAddr| -> Result<Map16Tile, ImportError> {
            Ok(Map16Tile::from_bytes(
                rom.read(at, 8)?.try_into().expect("8 bytes"),
            ))
        };
        let sharing = game.sharing();
        for tile in 0..0x200u16 {
            if !game.is_specific(tile) {
                let at = game.address(0, tile);
                let gfx = definition(rom, at)?;
                if gfx != definition(clean, at)? {
                    pages[tile as usize >> 8].tiles.entry(tile).or_default().gfx = Some(gfx);
                }
                continue;
            }
            for group in &sharing {
                let at = game.address(group[0], tile);
                let gfx = definition(rom, at)?;
                if gfx != definition(clean, at)? {
                    tilesets.entry(group[0]).or_default().tiles.insert(
                        tile,
                        Map16Entry {
                            gfx,
                            ..Map16Entry::default()
                        },
                    );
                }
            }
        }
    }
    if map16_pages::tileset_page2(rom)? {
        let first = map16_pages::tileset_page2_definition(rom, 0, 0x200)?;
        let in_block = rom.pc(first).ok().is_some_and(|pc| {
            rats_spans(rom).iter().any(|b| {
                b.contains(&pc.as_usize())
                    && pc.as_usize() + map16_pages::TILESET_PAGE2_LEN <= b.end
            })
        });
        if in_block {
            for tileset in 0..TILESET_COUNT {
                for tile in 0x200..0x300u16 {
                    let at = map16_pages::tileset_page2_definition(rom, tileset, tile)?;
                    let gfx = Map16Tile::from_bytes(rom.read(at, 8)?.try_into().expect("8 bytes"));
                    if gfx != Map16Tile::default() {
                        tilesets.entry(tileset).or_default().tiles.insert(
                            tile,
                            Map16Entry {
                                gfx,
                                ..Map16Entry::default()
                            },
                        );
                    }
                }
            }
        } else if first.bank() != 0 {
            // Bank $00 is a fresh install's: no table, and no tiles.
            out.notes.push(format!(
                "Map16 page 2 is per tileset, but its table at {first} is in no RATS block, an \
                 older Lunar Magic's layout; not imported"
            ));
        }
    }
    for (page, tiles) in pages.into_iter().enumerate() {
        if !tiles.tiles.is_empty() {
            out.pages.push((page as u8, tiles));
        }
    }
    out.tilesets = tilesets.into_iter().collect();
    Ok(out)
}

/// The file ranges of the data of the ROM's RATS blocks.
fn rats_spans(rom: &Rom) -> Vec<Range<usize>> {
    rats::blocks(rom)
        .into_iter()
        .filter_map(|b| {
            rom.pc(b.start)
                .ok()
                .map(|s| s.as_usize()..s.as_usize() + b.len)
        })
        .collect()
}

/// The file ranges of everything [`read_level`] reads for every level,
/// the tables included, and the header bytes a build rewrites.
fn read_spans(rom: &Rom) -> Result<Vec<Range<usize>>, ImportError> {
    let mut spans = Vec::new();
    let mut add = |addr: SnesAddr, len: usize| {
        if let Ok(pc) = rom.pc(addr) {
            spans.push(pc.as_usize()..pc.as_usize() + len);
        }
    };
    let count = LEVEL_COUNT as usize;
    add(tables::LAYER1_PTRS, 3 * count);
    add(tables::LAYER2_PTRS, 3 * count);
    add(tables::SPRITE_PTRS, 2 * count);
    for table in tables::SECONDARY_HEADERS {
        add(table, count);
    }
    for table in level::entrance_tables(rom) {
        add(table, level::entrance_count(rom) as usize);
    }
    if LevelFormat::of(rom).lunar_magic {
        add(tables::SPRITE_BANKS, count);
        add(LUNAR_MAGIC_MARKER, 64);
    }
    if level::has_level_flags(rom) {
        add(tables::LEVEL_FLAGS, count);
    }
    // The shared palettes, which a project holds as what it changes.
    add(
        crate::source::palettes::START,
        2 * usize::from(crate::source::palettes::COLOURS),
    );
    if map16_pages::installed(rom) {
        let blocks = rats_spans(rom);
        for group in &PAGE_GROUPS {
            add(group.pointer, 2);
            add(group.bank, 1);
            // What read_map16 takes: the pages in the RATS block the
            // group's table starts in.
            let first = *group.pages().start() as u16 * PAGE_TILES;
            let start = group.definition(rom, first).ok().flatten();
            let Some(pc) = start.and_then(|at| rom.pc(at).ok()).map(|pc| pc.as_usize()) else {
                continue;
            };
            if let Some(block) = blocks.iter().find(|b| b.contains(&pc)) {
                let end = (pc + group.pages().count() * 0x800).min(block.end);
                add(start.expect("found above"), end - pc);
            }
        }
        add(map16_pages::TILESET_PAGE2, 1);
        add(map16_pages::TILESET_PAGE2_POINTER, 2);
        add(map16_pages::TILESET_PAGE2_BANK, 1);
        if map16_pages::tileset_page2(rom)? {
            let at = map16_pages::tileset_page2_definition(rom, 0, 0x200)?;
            add(at, map16_pages::TILESET_PAGE2_LEN);
        }
        add(map16_pages::ACTS_LIKE, 3);
        add(map16_pages::ACTS_LIKE_UPPER, 3);
        if let Ok(table) = rom.read_u24(map16_pages::ACTS_LIKE) {
            add(SnesAddr::new(table), 0x8000);
        }
        let upper = rom.read_u24(map16_pages::ACTS_LIKE_UPPER)?;
        if upper >> 16 != 0xFF {
            add(SnesAddr::new(upper + 0x8000), 0x8000);
        }
    }
    // Pages 0 and 1: the game's tables, which read_map16_game compares,
    // and the diagonal pipes and the pipe colours between them (read_pipes).
    let game = GameTables::read(rom)?;
    add(crate::map16::tables::DIAGONAL_PIPE_TILES, 0x100);
    add(crate::map16::tables::TILESET_SPECIFIC_MASK, 0x40);
    add(
        crate::map16::tables::TILESET_MAP16_LOC,
        2 * TILESET_COUNT as usize,
    );
    for tileset in 0..TILESET_COUNT {
        for tile in 0..0x200 {
            if tileset == 0 || game.is_specific(tile) {
                add(game.address(tileset, tile), 8);
            }
        }
    }
    // BG Map16: the table pointers, and what read_map16_bg takes, each
    // table in the RATS block it starts in.
    if rom.read_u8(map16_pages::BG_MAP16_HOOK)? == 0x22 {
        add(map16_pages::BG_TABLES, 3 * 16);
        let blocks = rats_spans(rom);
        for table in 0..16u8 {
            let Some(at) = map16_pages::bg_table(rom, table)? else {
                continue;
            };
            let Ok(pc) = rom.pc(at).map(|pc| pc.as_usize()) else {
                continue;
            };
            if let Some(block) = blocks.iter().find(|b| b.contains(&pc)) {
                add(at, (16 * 0x800).min(block.end - pc));
            }
        }
    }
    // Custom palettes: the pointers, and each level's $202 bytes.
    if palette::has_level_palettes(rom) {
        add(palette::LM_LEVEL_PALETTE_PTRS, 3 * count);
        for number in 0..LEVEL_COUNT {
            let entry = palette::LM_LEVEL_PALETTE_PTRS.add(3 * number as u32);
            if palette::lm_level_palette(rom, number)?.is_some() {
                add(SnesAddr::new(rom.read_u24(entry)?), 0x202);
            }
        }
    }
    if let Ok(reader) = gfx::GfxReader::new(rom) {
        add(gfx::GFX_PTR_LO, 0x32);
        add(gfx::GFX_PTR_HI, 0x32);
        add(gfx::GFX_PTR_BANK, 0x32);
        add(gfx::GFX32_PTR, 2);
        add(gfx::GFX33_PTR, 2);
        add(gfx::GFX32_33_BANK, 1);
        for index in 0..gfx::GFX_FILE_COUNT {
            if let Ok(file) = reader.read(index) {
                add(file.addr, file.compressed_len);
            }
        }
    }
    // ExGFX, the lists, and the older lists.
    if exgfx::has_exgfx(rom) {
        add(exgfx::LIST_POINTER, 3);
        add(exgfx::BLOCK_POINTER, 3);
        add(exgfx::BLOCK_POINTER_COPY, 3);
        add(exgfx::EXGFX_80, 3 * 0x80);
        add(exgfx::OLD_LISTS, 4 * exgfx::OLD_LIST_COUNT);
        if let Ok(block) = rom.read_u24(exgfx::BLOCK_POINTER) {
            add(SnesAddr::new(block), exgfx::BLOCK_LEN);
        }
        if let Ok(compression) = gfx::Compression::detect(rom) {
            for file in exgfx::EXGFX_FIRST..=exgfx::EXGFX_LAST {
                if let Ok(Some(d)) = exgfx::read_exgfx(rom, file, compression) {
                    add(d.addr, d.compressed_len);
                }
            }
        }
    }
    for (at, len) in crate::exanimation::spans(rom) {
        add(at, len);
    }
    // Choc Island 2's rooms' banks, which a build installs with levels
    // 0CD-0CF (choc-island.asm): the hook and the block its code is in.
    let hook = crate::install::CHOC_ISLAND_HOOK;
    if rom.read_u8(hook)? == 0x22 {
        add(hook, 4);
        let target = SnesAddr::new(rom.read_u24(hook.add(1))?);
        if let Ok(pc) = rom.pc(target).map(|pc| pc.as_usize())
            && let Some(block) = rats_spans(rom).into_iter().find(|b| b.contains(&pc))
        {
            add(target, block.end - pc);
        }
    }
    // The ROM size code, and the checksum and its complement.
    add(SnesAddr::new(0x00FFD7), 1);
    add(SnesAddr::new(0x00FFDC), 4);
    // A level whose data does not read was left out of the import, and
    // what it has read so far counts as read.
    for number in 0..LEVEL_COUNT {
        let mut level_spans = || -> Result<(), ImportError> {
            let data = level::read_objects(rom, number)?;
            add(level::layer1_ptr(rom, number)?, data.layer1.len);
            if let (level::Layer2::Objects(list), level::Layer2Data::Objects(addr)) =
                (&data.layer2, level::layer2_ptr(rom, number)?)
            {
                add(addr, list.len);
            }
            if let Some(bg) = level::read_background(rom, number)? {
                add(bg.address, bg.stream_len);
            }
            let at = level::sprite_ptr(rom, number)?;
            add(at, sprites::read_sprites_at(rom, at)?.len);
            Ok(())
        };
        let _ = level_spans();
    }
    spans.sort_by_key(|r| r.start);
    Ok(spans)
}

/// Bytes that differ from the clean ROM within its length and fall in no
/// span, gathered into ranges; differences fewer than 16 bytes apart are
/// one range.
fn unmodelled(rom: &Rom, clean: &Rom, read: &[Range<usize>]) -> Vec<(SnesAddr, usize)> {
    let mut covered = vec![false; clean.len()];
    for span in read {
        for flag in covered
            .iter_mut()
            .take(span.end.min(clean.len()))
            .skip(span.start)
        {
            *flag = true;
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let (a, b) = (rom.data(), clean.data());
    for i in (0..clean.len().min(a.len())).filter(|&i| a[i] != b[i] && !covered[i]) {
        match ranges.last_mut() {
            Some(last) if i - last.end < 16 => last.end = i + 1,
            _ => ranges.push(i..i + 1),
        }
    }
    ranges
        .into_iter()
        .filter_map(|r| {
            let at = rom.mapping().pc_to_snes(PcAddr::new(r.start as u32)).ok()?;
            Some((at, r.len()))
        })
        .collect()
}

/// Which parts of a level differ between two ROMs.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LevelDiff {
    pub level: u16,
    /// `header`, `size`, `entrance`, `layer1`, `layer2`, `background` (the
    /// tiles of a background tilemap, wherever it is), `sprites`,
    /// `entrances`, `palette`, `graphics` (its graphics list), or the
    /// error reading the level from one of them.
    pub parts: Vec<String>,
}

/// Compares every level of two ROMs as Kobo reads them, whatever their
/// layouts: the same level moved or re-encoded is no difference, and nor
/// is a screen exit in the game's format and in Lunar Magic's.
pub fn diff_levels(a: &Rom, b: &Rom) -> Vec<LevelDiff> {
    let mut diffs = Vec::new();
    for level in 0..LEVEL_COUNT {
        // Screen exits in one format, apart from the other objects, by
        // screen: the game keeps one per screen whatever their place among
        // the objects, and Lunar Magic's save moves and sorts them.
        let read = |rom| -> Result<(Level, Option<Vec<u16>>), ImportError> {
            let (mut l, _) = read_level(rom, level)?;
            let (mut exits, others): (Vec<Object>, Vec<Object>) = l
                .layer1
                .into_iter()
                .partition(|o| matches!(o, Object::ScreenExit(_)));
            for object in &mut exits {
                if let Object::ScreenExit(exit) = object {
                    *exit = exit.in_lunar_magic_format(level);
                }
            }
            exits.sort_by_key(|o| match o {
                Object::ScreenExit(exit) => exit.screen,
                _ => 0,
            });
            l.layer1 = others;
            l.layer1.extend(exits);
            let background = level::read_background(rom, level)?.map(|bg| bg.tiles());
            Ok((l, background))
        };
        let parts: Vec<String> = match (read(a), read(b)) {
            (Ok((x, xbg)), Ok((y, ybg))) => [
                ("header", x.header != y.header),
                ("size", x.size != y.size),
                ("entrance", x.entrance != y.entrance),
                ("settings", x.settings != y.settings),
                ("layer1", x.layer1 != y.layer1),
                ("layer2", x.layer2 != y.layer2),
                ("background", xbg != ybg),
                ("sprites", x.sprites != y.sprites),
                ("entrances", x.entrances != y.entrances),
                ("palette", x.palette != y.palette),
                ("graphics", x.graphics != y.graphics),
                (
                    "animation",
                    x.animation != y.animation || x.animation_settings != y.animation_settings,
                ),
            ]
            .into_iter()
            .filter(|(_, differs)| *differs)
            .map(|(part, _)| part.to_owned())
            .collect(),
            (x, y) => [x.err(), y.err()]
                .into_iter()
                .flatten()
                .map(|e| e.to_string())
                .collect(),
        };
        if !parts.is_empty() {
            diffs.push(LevelDiff { level, parts });
        }
    }
    diffs
}

/// A level from an MWL file, as level `number`, with notes on what it
/// holds that a build cannot write yet. A background comes through as the
/// clean ROM's when the file says it came from there and its tiles are
/// that background's.
pub fn level_from_mwl(
    mwl: &Mwl,
    number: u16,
    clean: &Rom,
) -> Result<(Level, Vec<String>), ImportError> {
    let mut notes = Vec::new();
    let header = mwl.layer1.primary_header();
    let mode = header.level_mode;
    let vertical = mode.layer1_vertical();
    // A file Lunar Magic 2 saved, with its layout of the settings bytes.
    let old = mwl.version < 0x0300;
    let layer2 = match (&mwl.layer2.data, mode.layer2()) {
        (mwl::Layer2Data::Objects(data), level::Layer2Kind::HorizontalObjects)
        | (mwl::Layer2Data::Objects(data), level::Layer2Kind::VerticalObjects) => {
            Layer2::Objects(data.objects.clone())
        }
        (mwl::Layer2Data::Background(tiles), level::Layer2Kind::Background) => {
            let vanilla = mwl.layer2.header.source().and_then(|at| {
                let at = match at.bank() {
                    0xFF => SnesAddr::from_bank_offset(0x0C, at.offset()),
                    _ => at,
                };
                let pointer = level::Layer2Data::Tilemap(at);
                let bg = level::read_background_at(clean, mwl.info.level, pointer).ok()?;
                (at.bank() == 0x0C && bg.tiles() == *tiles).then_some(at)
            });
            let flags = mwl.layer2.header.0[0];
            match (vanilla, background_from_mwl(flags, tiles)) {
                (Some(at), _) => Layer2::VanillaBackground(at),
                (None, Some(bg)) => Layer2::Background(bg),
                (None, None) => {
                    notes.push(format!(
                        "its background, with flags {flags:02X}, is in a form not imported yet"
                    ));
                    Layer2::None
                }
            }
        }
        // Boss levels export their pointer's background, which the game
        // does not load.
        (_, level::Layer2Kind::None) => Layer2::None,
        (data, _) => {
            let has = match data {
                mwl::Layer2Data::Objects(_) => "layer 2 objects",
                mwl::Layer2Data::Background(_) => "a background",
                mwl::Layer2Data::Empty => "no layer 2",
            };
            notes.push(format!(
                "it has {has}, which is not what level mode {mode} loads; its layer 2 is not \
                 imported"
            ));
            Layer2::None
        }
    };
    let palette = mwl.layer1.custom_palette().then(|| CustomPalette {
        back_area: mwl.palette.back_area,
        palette: mwl.palette.colors.clone(),
    });
    if palette.as_ref().is_some_and(high_bits) {
        notes.push("its palette has colours with bit 15 set, which is not kept".into());
    }
    let entrances = mwl.entrances.entries.iter().map(|e| {
        let settings = if old {
            EntranceSettings::from_old_byte(e.tables[2], vertical)
        } else {
            EntranceSettings::from_bytes(e.tables, e.lm)
        };
        (e.id, e.tables, settings)
    });
    let entrances = entrances_leading_to(entrances, &mut notes);
    let mut layer1 = mwl.layer1.data.objects.clone();
    exits_in_game_format(&mut layer1, number);
    let list = &mwl.sprites.list;
    // Files from before Lunar Magic 3 have none of its per-level bytes.
    let [fc, fe, _, fa] = match mwl.info.lm3 {
        [0, 0, 0, 0] => {
            let [_, fa, fc, fe] = entrance::DEFAULT_BYTES;
            [fc, fe, 0, fa]
        }
        bytes => bytes,
    };
    let [m1, m2, m3, m4, _] = mwl.info.midway;
    let (settings, mut secondary) = if old {
        // Lunar Magic 2's layout, converted as from a ROM of its; its layer
        // 2 scroll setting 8 is Lunar Magic 3's setting 3.
        let settings =
            LevelSettings::from_old_bytes(mwl.info.secondary_lm[0], [m1, m2, m3], vertical);
        (settings, mwl.info.secondary)
    } else {
        let bytes = [mwl.info.secondary_lm[0], fa, fc, fe];
        (
            LevelSettings::from_bytes(bytes, [m1, m2, m3, m4]),
            mwl.info.secondary,
        )
    };
    if old && secondary.layer2_scroll == 8 {
        secondary.layer2_scroll = 3;
    }
    let size = level_size(mwl.info.lm3[2], mode, &mut notes);
    let level = Level {
        header,
        size,
        entrance: secondary,
        settings,
        layer1,
        layer2,
        sprites: Sprites::from_entries(list.header, &list.sprites, mode.layer1_vertical()),
        entrances,
        palette,
        graphics: Some(mwl.exgfx).filter(|l| l.is_used()),
        animation: match mwl.animation.data.as_slice() {
            [] => None,
            data => match exanimation::List::parse(data) {
                Ok((list, _)) => Some(list),
                Err(e) => {
                    notes.push(format!(
                        "its ExAnimation does not read ({e}); it is not imported"
                    ));
                    None
                }
            },
        },
        animation_settings: Some(mwl.animation.settings())
            .filter(|&b| b != exanimation::Settings::default_for(number).0),
    };
    notes.extend(graphics_notes(None, &level)?);
    notes.extend(animation_notes(level.animation.as_ref()));
    Ok((level, notes))
}

/// Whether a palette has a colour with bit 15 set, which the SNES ignores
/// and a level file does not keep.
fn high_bits(p: &CustomPalette) -> bool {
    std::iter::once(p.back_area)
        .chain(p.palette.colors)
        .any(|c| c.0 & 0x8000 != 0)
}

/// Writes `level` into the project in `dir` as level `number`, creating
/// the project if there is none: in the file the manifest lists for it,
/// or in `levels/NNN.toml`, keeping that file's comments, and the
/// manifest again, keeping its own. Returns the level file's path.
pub fn add_level(dir: &Path, number: u16, level: &Level) -> Result<PathBuf, ImportError> {
    if number >= LEVEL_COUNT {
        return Err(ImportError::LevelNumber(number));
    }
    let manifest_path = dir.join(MANIFEST);
    let (mut manifest, manifest_comments) = match fs::read_to_string(&manifest_path) {
        Ok(text) => Manifest::from_toml(&text).map_err(|source| ImportError::Manifest {
            path: manifest_path.clone(),
            source,
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(source) => {
            return Err(ImportError::Io {
                path: manifest_path,
                source,
            });
        }
    };
    let file = match manifest.levels.get(&number) {
        Some(file) => file.clone(),
        None => {
            let file = PathBuf::from("levels").join(format!("{number:03X}.toml"));
            if let Some((&other, _)) = manifest.levels.iter().find(|(_, f)| **f == file) {
                return Err(ImportError::FileTaken { file, level: other });
            }
            file
        }
    };
    let path = dir.join(&file);
    // A file already there keeps its comments, if it is a level file.
    let comments = fs::read_to_string(&path)
        .ok()
        .and_then(|text| Level::from_toml(&text).ok())
        .map(|(_, comments)| comments)
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, level.to_toml(&comments)).map_err(|source| ImportError::Io {
        path: path.clone(),
        source,
    })?;
    manifest.levels.insert(number, file);
    fs::write(&manifest_path, manifest.to_toml(&manifest_comments)).map_err(|source| {
        ImportError::Io {
            path: manifest_path,
            source,
        }
    })?;
    Ok(path)
}

/// Imports an MWL file into the project in `dir`, as `level` or the level
/// it was saved from, creating the project if there is none. The level
/// goes in the file the manifest lists for it, or in `levels/NNN.toml`,
/// keeping that file's comments; the manifest is written again.
pub fn import_mwl(
    bytes: &[u8],
    clean: &Rom,
    dir: &Path,
    level: Option<u16>,
) -> Result<Report, ImportError> {
    import_mwl_sized(bytes, clean, dir, level, None)
}

/// [`import_mwl`] of a file whose sprites have extension bytes as `sizes`
/// says: PIXI's sprite size table ([`sprites::pixi_size_table`]) of the ROM
/// the file came from, which the file does not hold.
pub fn import_mwl_sized(
    bytes: &[u8],
    clean: &Rom,
    dir: &Path,
    level: Option<u16>,
    sizes: Option<&[u8]>,
) -> Result<Report, ImportError> {
    let mwl = MwlFile::parse(bytes)?.decode(sizes)?;
    let number = level.unwrap_or(mwl.info.level);
    if number >= LEVEL_COUNT {
        return Err(ImportError::LevelNumber(number));
    }
    let (source, notes) = level_from_mwl(&mwl, number, clean)?;
    add_level(dir, number, &source)?;
    Ok(Report {
        levels: vec![number],
        notes: notes
            .into_iter()
            .map(|n| format!("level {number:03X}: {n}"))
            .collect(),
        ..Report::default()
    })
}

/// How [`import_callisto`] imports.
#[derive(Clone, Default, Debug)]
pub struct CallistoOptions {
    /// The PIXI version the project's sprites are written for, if not the
    /// one Kobo pins by default.
    pub pixi_version: Option<String>,
    /// Where the project came from, noted at the top of its manifest.
    pub origin: Option<String>,
    /// Whose its resources are, noted there too.
    pub rights: Option<String>,
    /// Files and folders of the source, by their path from its root, the
    /// project leaves out (a baserom's scripts for Callisto, say).
    pub exclude: Vec<PathBuf>,
}

/// File extensions a project never holds: ROM images and patches of them
/// (docs/build.md, "No base"), and programs, which are the user's own.
const NOT_SOURCE: &[&str] = &["smc", "sfc", "bps", "ips", "exe", "dll", "so", "dylib"];

/// Imports the Callisto project in `source` (`crate::callisto`) into a new
/// project in `dir`, against the clean ROM. The project is the Callisto
/// project's folder as it was, so that every file its patches and tools
/// include is where they look, but for what Kobo builds from its own
/// files instead: the levels' MWL files become level files, the full Map16
/// export page files, the `Graphics` and `ExGraphics` folders graphics, and
/// the global ExAnimation patch its list; the configuration, ROM images and
/// patches of them, and programs are left out. The patches run before the
/// tools, as Callisto's do, and `[callisto]` gives them and the tools'
/// files the `callisto.asm` they include (`build::callisto_header`). What
/// a project cannot hold yet is reported.
pub fn import_callisto(
    source: &Path,
    clean: &Rom,
    dir: &Path,
    options: &CallistoOptions,
) -> Result<Report, ImportError> {
    use crate::callisto::{Project, ToolKind};
    let project = Project::find(source)?;
    if dir.join(MANIFEST).exists() {
        return Err(ImportError::Exists(dir.to_path_buf()));
    }
    let root = &project.root;
    let read = |path: &Path| {
        let path = root.join(path);
        fs::read(&path).map_err(|source| ImportError::Io { path, source })
    };
    let mut report = Report::default();
    let mut manifest = Manifest::default();
    let mut skip: Vec<PathBuf> = options.exclude.clone();
    if let Ok(config) = project.config.strip_prefix(root) {
        skip.push(config.to_path_buf());
    }
    let step = |name: &str| project.builds(name);
    if let Some(patch) = project
        .initial_patch
        .as_ref()
        .filter(|_| step("InitialPatch"))
    {
        let patched =
            crate::bps::apply_to_rom(&read(patch)?, clean).map_err(|source| ImportError::Bps {
                path: patch.clone(),
                source,
            })?;
        let initial = Rom::from_headerless(patched.data)?;
        manifest.sa1 = initial.mapping().is_sa1();
        manifest.four_bpp = exgfx::is_4bpp(&initial);
        if initial.len() > crate::build::DEFAULT_ROM_SIZE {
            manifest.rom_size = Some(initial.len());
        }
        report.notes.push(format!(
            "its initial patch ({}) is not carried: a build starts from the clean ROM{} and \
             installs Kobo's own code for Lunar Magic's layout",
            patch.display(),
            if manifest.sa1 { " with SA-1 Pack" } else { "" }
        ));
        let map_mode = initial.read_u8(SnesAddr::new(0x00FFD5))?;
        if !manifest.sa1 && map_mode & 0x10 != 0 {
            report.notes.push(
                "the initial patch makes the ROM FastROM; Kobo's builds are not, so code the \
                 patches put in banks $80 and up runs at SlowROM speed"
                    .into(),
            );
        }
    }
    if step("Patches") {
        for patch in &project.patches {
            if !root.join(patch).is_file() {
                return Err(ImportError::File {
                    path: patch.clone(),
                    message: "a patch the configuration lists, not in the project".into(),
                });
            }
        }
        manifest.early_patches = project.patches.clone();
    }
    if step("Modules") && !project.modules.is_empty() {
        report.notes.push(format!(
            "{} Callisto modules are not carried: Kobo does not build modules",
            project.modules.len()
        ));
    }
    manifest.callisto = Some(crate::source::project::Callisto {
        header: project.callisto_header.clone(),
    });
    for tool in &project.tools {
        if !step(&tool.name) {
            continue;
        }
        let folder = tool.directory.clone();
        match tool.kind() {
            Some(ToolKind::Pixi) => {
                // PIXI's own files would be laid over Kobo's PIXI: only
                // the project's inputs are taken.
                crate::tools::copy_pixi_inputs(&root.join(&folder), &dir.join(&folder))?;
                skip.push(folder.clone());
                manifest.pixi = Some(folder);
            }
            Some(ToolKind::Gps) => manifest.gps = Some(folder),
            Some(ToolKind::UberAsm) => manifest.uberasm = Some(folder),
            Some(ToolKind::AddmusicK) => manifest.music = Some(folder),
            None => report.notes.push(format!(
                "the build order runs {} ({}), which Kobo does not run; not carried",
                tool.name, tool.executable
            )),
        }
    }
    if let Some(version) = &options.pixi_version
        && manifest.pixi.is_some()
    {
        let tool = crate::tools::Tool::Pixi;
        if tool.pinned_version(Some(version)).is_some() {
            if tool.version() != Some(version.as_str()) {
                manifest.pixi_version = Some(version.clone());
            }
        } else {
            report.notes.push(format!(
                "its sprites are written for PIXI {version}, which Kobo does not pin ({}); \
                 a build runs PIXI {} unless a path to {version} is configured",
                tool.pinned_versions().join(", "),
                tool.version().unwrap_or("as configured")
            ));
        }
    }
    // Lunar Magic keeps the graphics in folders beside the ROM, and
    // Callisto's output folder, when it is not the root, holds the build
    // and Lunar Magic's own files for it.
    let beside = project
        .output_rom
        .as_ref()
        .and_then(|rom| rom.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    if !beside.as_os_str().is_empty() {
        skip.push(beside.clone());
    }
    copy_project(root, dir, &skip)?;
    // Kobo's own files, after the copy, so that one of the project's of
    // the same name gives way.
    if let Some(levels) = project.resources.get("levels").filter(|_| step("Levels")) {
        skip.push(levels.clone());
        let mut files: Vec<PathBuf> = fs::read_dir(root.join(levels))
            .map_err(|source| ImportError::Io {
                path: root.join(levels),
                source,
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mwl")))
            .collect();
        files.sort();
        for path in files {
            let bytes = fs::read(&path).map_err(|source| ImportError::Io {
                path: path.clone(),
                source,
            })?;
            let mwl = MwlFile::parse(&bytes)?.decode(None)?;
            let number = mwl.info.level;
            let (level, notes) = level_from_mwl(&mwl, number, clean)?;
            let file = PathBuf::from("levels").join(format!("{number:03X}.toml"));
            write_text(&dir.join(&file), level.to_toml(&Comments::default()))?;
            manifest.levels.insert(number, file);
            report.levels.push(number);
            report.notes.extend(
                notes
                    .into_iter()
                    .map(|n| format!("level {number:03X}: {n}")),
            );
            if level.sprites.list.iter().any(|s| !s.extension.is_empty()) {
                report.notes.push(format!(
                    "level {number:03X}: its sprites were read without PIXI's size table, \
                     which an MWL file does not hold; check their extra bytes"
                ));
            }
        }
        remove_copied(dir, levels)?;
    }
    if let Some(path) = project.resources.get("map16").filter(|_| step("Map16")) {
        if project.text_map16 {
            report.notes.push(format!(
                "its Map16 ({}) is in the text format, which Kobo does not read; not carried",
                path.display()
            ));
        } else {
            let file = crate::map16_file::AllMap16::parse(&read(path)?).map_err(|source| {
                ImportError::Map16File {
                    path: path.clone(),
                    source,
                }
            })?;
            write_map16_files(
                dir,
                &mut manifest,
                &mut report,
                map16_from_file(&file, clean)?,
            )?;
            remove_copied(dir, path)?;
        }
    }
    if let Some(path) = project
        .resources
        .get("shared_palettes")
        .filter(|_| step("SharedPalettes"))
    {
        // Lunar Magic's export: the tables' bytes as the ROM has them.
        let theirs = read(path)?;
        let ours = crate::source::palettes::read_all(clean)?;
        let colours = theirs
            .as_chunks::<2>()
            .0
            .iter()
            .zip(&ours)
            .enumerate()
            .map(|(n, (word, clean))| {
                (
                    n as u16,
                    crate::palette::Color15(u16::from_le_bytes(*word)),
                    clean,
                )
            })
            .filter(|(_, colour, clean)| colour != *clean)
            .map(|(n, colour, _)| (n, colour))
            .collect();
        let palettes = crate::source::palettes::SharedPalettes { colours };
        write_shared_palettes(dir, &mut manifest, &palettes)?;
    }
    if step("Graphics") && root.join(&beside).join("Graphics").is_dir() {
        let folder = beside.join("Graphics");
        let (files, notes) = gfx_from_folder(&root.join(&folder), clean, manifest.four_bpp)?;
        report.notes.extend(notes);
        for (index, image) in files {
            let file = PathBuf::from("graphics").join(format!("GFX{index:02X}.png"));
            let png = image
                .to_png()
                .map_err(|e| ImportError::Gfx(e.to_string()))?;
            write_text(&dir.join(&file), png)?;
            manifest.gfx.insert(index, file);
        }
        remove_copied(dir, &folder)?;
    }
    if step("ExGraphics") && root.join(&beside).join("ExGraphics").is_dir() {
        let folder = beside.join("ExGraphics");
        for (number, path) in exgfx_files(&root.join(&folder))? {
            let bytes = fs::read(&path).map_err(|source| ImportError::Io {
                path: path.clone(),
                source,
            })?;
            match number {
                0x60..=0x63 => {
                    let file = PathBuf::from("animation").join(format!("ExGFX{number:X}.bin"));
                    write_text(&dir.join(&file), bytes)?;
                    manifest.animation_files.insert(number, file);
                }
                exgfx::EXGFX_FIRST..=exgfx::EXGFX_LAST => {
                    // 4bpp tiles if it is whole rows of them, as the ROM
                    // import takes a file no graphics list says the depth
                    // of; its bytes as they are otherwise.
                    let row = gfx::Bpp::Four.bytes_per_tile() * 16;
                    let file = if !bytes.is_empty() && bytes.len() % row == 0 {
                        let tiles = gfx::decode_tiles(gfx::Bpp::Four, &bytes);
                        let png = gfx::tiles_to_image(&tiles, 16)
                            .to_png()
                            .map_err(|e| ImportError::Gfx(e.to_string()))?;
                        let file = PathBuf::from("graphics").join(format!("ExGFX{number:X}.png"));
                        write_text(&dir.join(&file), png)?;
                        file
                    } else {
                        let file = PathBuf::from("graphics").join(format!("ExGFX{number:X}.bin"));
                        write_text(&dir.join(&file), bytes)?;
                        file
                    };
                    manifest
                        .exgfx
                        .insert(number, ExGfxFile { path: file, bpp: 4 });
                }
                _ => report.notes.push(format!(
                    "{}: not an ExGFX file a project holds; not carried",
                    path.display()
                )),
            }
        }
        if !manifest.exgfx.is_empty() {
            manifest.four_bpp = true;
        }
        remove_copied(dir, &folder)?;
    }
    if let Some(path) = project
        .resources
        .get("global_exanimation")
        .filter(|_| step("GlobalExAnimation"))
    {
        // Callisto keeps it as a patch of the clean ROM.
        let patched =
            crate::bps::apply_to_rom(&read(path)?, clean).map_err(|source| ImportError::Bps {
                path: path.clone(),
                source,
            })?;
        let rom = Rom::from_headerless(patched.data)?;
        match exanimation::read_global(&rom) {
            Ok(Some(found)) => {
                let file = PathBuf::from("animation").join("global.toml");
                write_text(
                    &dir.join(&file),
                    crate::source::animation::global_to_toml(&found.list, &Comments::default()),
                )?;
                manifest.animation_global = Some(file);
                report.notes.extend(
                    animation_notes(Some(&found.list))
                        .into_iter()
                        .map(|n| format!("global ExAnimation: {n}")),
                );
            }
            Ok(None) => {}
            Err(e) => report.notes.push(format!(
                "global ExAnimation ({}): does not read ({e}); not carried",
                path.display()
            )),
        }
    }
    for (key, step_name, what) in [
        ("overworld", "Overworld", "the overworld"),
        ("titlescreen", "TitleScreen", "the title screen"),
        ("credits", "Credits", "the credits"),
    ] {
        if let Some(path) = project.resources.get(key).filter(|_| step(step_name)) {
            report.notes.push(format!(
                "{what} ({}): not carried, as Kobo does not build it yet and a project holds \
                 no patches of the ROM; finish it in Lunar Magic on the built ROM",
                path.display()
            ));
        }
    }
    let mut comments = Comments::default();
    let mut top = Vec::new();
    if let Some(origin) = &options.origin {
        top.extend(wrap_comment(&format!("Made from {origin}."), 88));
    }
    if let Some(rights) = &options.rights {
        top.extend(wrap_comment(rights, 88));
    }
    comments.add(Comments::TOP, top);
    write_text(&dir.join(MANIFEST), manifest.to_toml(&comments))?;
    report.levels.sort_unstable();
    Ok(report)
}

/// Copies the project in `from` to `to`, but for the paths in `skip`
/// (from `from`), hidden files and folders, and [`NOT_SOURCE`] files.
/// Folders left with nothing are not made.
fn copy_project(from: &Path, to: &Path, skip: &[PathBuf]) -> Result<(), ImportError> {
    fn walk(root: &Path, at: &Path, to: &Path, skip: &[PathBuf]) -> Result<(), ImportError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| ImportError::Io { path, source }
        };
        let mut entries: Vec<_> = fs::read_dir(at)
            .map_err(io(at))?
            .collect::<Result<_, _>>()
            .map_err(io(at))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("under the root");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || skip.iter().any(|s| relative == s) {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, to, skip)?;
            } else if !path
                .extension()
                .is_some_and(|e| NOT_SOURCE.iter().any(|x| e.eq_ignore_ascii_case(x)))
            {
                let target = to.join(relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(io(parent))?;
                }
                fs::copy(&path, &target).map_err(io(&path))?;
            }
        }
        Ok(())
    }
    walk(from, from, to, skip)
}

/// Removes from the project in `dir` what it copied of `path`, a source
/// Kobo's own files replace.
fn remove_copied(dir: &Path, path: &Path) -> Result<(), ImportError> {
    let target = dir.join(path);
    let result = if target.is_dir() {
        fs::remove_dir_all(&target)
    } else if target.is_file() {
        fs::remove_file(&target)
    } else {
        return Ok(());
    };
    result.map_err(|source| ImportError::Io {
        path: target,
        source,
    })
}

/// The `ExGFX<hex>.bin` files of a folder, by number.
fn exgfx_files(folder: &Path) -> Result<Vec<(u16, PathBuf)>, ImportError> {
    let mut out = Vec::new();
    for entry in fs::read_dir(folder).map_err(|source| ImportError::Io {
        path: folder.to_path_buf(),
        source,
    })? {
        let path = entry
            .map_err(|source| ImportError::Io {
                path: folder.to_path_buf(),
                source,
            })?
            .path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let number = name
            .strip_prefix("ExGFX")
            .and_then(|n| n.strip_suffix(".bin"))
            .and_then(|n| u16::from_str_radix(n, 16).ok());
        if let Some(number) = number {
            out.push((number, path));
        }
    }
    out.sort();
    Ok(out)
}

/// GFX files `00` to `33` from Lunar Magic's export of them (`GFXnn.bin`,
/// `-ExportGFX`) in `folder` that differ from the clean ROM's, as images.
/// In a 4bpp project a file the game keeps as 3bpp is compared with Lunar
/// Magic's 4bpp version of the clean ROM's and kept as 4bpp; otherwise the
/// export's 4bpp form of such a file is taken back to 3bpp.
pub fn gfx_from_folder(
    folder: &Path,
    clean: &Rom,
    four_bpp: bool,
) -> Result<GfxImport, ImportError> {
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    let reader = gfx::GfxReader::new(clean).map_err(|e| ImportError::Gfx(e.to_string()))?;
    for index in 0..gfx::GFX_FILE_COUNT {
        let path = folder.join(format!("GFX{index:02X}.bin"));
        let Ok(theirs) = fs::read(&path) else {
            continue;
        };
        let ours = reader
            .read(index)
            .map_err(|e| ImportError::Gfx(e.to_string()))?;
        let (expected, image) = if four_bpp && exgfx::converts(ours.format) {
            let tiles = gfx::decode_tiles(gfx::Bpp::Four, &theirs);
            (exgfx::stored_4bpp(&ours), gfx::tiles_to_image(&tiles, 16))
        } else {
            let tiles = match ours.format {
                gfx::GfxFormat::Planar(gfx::Bpp::Three) => {
                    gfx::decode_tiles(gfx::Bpp::Three, &gfx::convert_4bpp_to_3bpp(&theirs))
                }
                gfx::GfxFormat::Planar(bpp) => gfx::decode_tiles(bpp, &theirs),
                gfx::GfxFormat::Packed3 => gfx::decode_packed3_tiles(&theirs),
            };
            (
                ours.to_lm_export(),
                gfx::tiles_to_image(&tiles, ours.colors()),
            )
        };
        if theirs.len() != expected.len() {
            notes.push(format!(
                "GFX{index:02X} ({}) is {} bytes, where the game's is {}; not carried",
                path.display(),
                theirs.len(),
                expected.len()
            ));
        } else if theirs != expected {
            out.push((index, image));
        }
    }
    Ok((out, notes))
}

/// A full Map16 export's tiles as a project holds them, against the clean
/// ROM: as [`read_map16`], [`read_map16_bg`], [`read_map16_game`], and
/// [`read_pipes`] read a ROM: Lunar Magic's empty tile is a tile of a
/// page or BG table where Lunar Magic would have allocated it, and Kobo's
/// empty tile elsewhere ([`AllMap16::allocated_ends`]).
///
/// [`AllMap16::allocated_ends`]: crate::map16_file::AllMap16::allocated_ends
pub fn map16_from_file(
    file: &crate::map16_file::AllMap16,
    clean: &Rom,
) -> Result<Map16Files, ImportError> {
    use crate::map16::tables::MAP16_BG_TILES;
    use crate::map16_file::{BG_START, empty_as_zero};
    let definition = |rom: &Rom, at: SnesAddr| -> Result<Map16Tile, ImportError> {
        Ok(Map16Tile::from_bytes(
            rom.read(at, 8)?.try_into().expect("8 bytes"),
        ))
    };
    let ends = file.allocated_ends();
    let allocated = |index: usize| {
        if index < ends[index / 0x1000] {
            file.tiles[index]
        } else {
            file.definition(index)
        }
    };
    let mut out = Map16Files::default();
    // Pages 0 and 1: what tiles act like, and the graphics of the common
    // tiles here and of each tileset's own in its file.
    let game = GameTables::read(clean)?;
    let mut pages = [GamePage::default(), GamePage::default()];
    let mut tilesets: BTreeMap<u8, Map16Page> = BTreeMap::new();
    for tile in 0..0x200u16 {
        let acts = file.acts[tile as usize];
        if acts != tile {
            pages[tile as usize >> 8].tiles.insert(
                tile,
                GameTile {
                    acts: Some(acts),
                    gfx: None,
                },
            );
        }
        if !game.is_specific(tile) {
            let gfx = file.tileset_pages[0][tile as usize];
            if gfx != definition(clean, game.address(0, tile))? {
                pages[tile as usize >> 8].tiles.entry(tile).or_default().gfx = Some(gfx);
            }
            continue;
        }
        for group in game.sharing() {
            let gfx = file.tileset_pages[group[0] as usize][tile as usize];
            if gfx != definition(clean, game.address(group[0], tile))? {
                tilesets.entry(group[0]).or_default().tiles.insert(
                    tile,
                    Map16Entry {
                        gfx,
                        ..Map16Entry::default()
                    },
                );
            }
        }
    }
    if let Some(page2) = &file.tileset_page2 {
        for (tileset, tiles) in page2.iter().enumerate() {
            for (i, &gfx) in tiles.iter().enumerate() {
                let gfx = empty_as_zero(gfx);
                if gfx != Map16Tile::default() {
                    tilesets.entry(tileset as u8).or_default().tiles.insert(
                        0x200 + i as u16,
                        Map16Entry {
                            gfx,
                            ..Map16Entry::default()
                        },
                    );
                }
            }
        }
    }
    for (page, tiles) in pages.into_iter().enumerate() {
        if !tiles.tiles.is_empty() {
            out.game.pages.push((page as u8, tiles));
        }
    }
    out.game.tilesets = tilesets.into_iter().collect();
    // Pages 2 to 7F, but page 2's graphics when it is per tileset.
    for page in 2..0x80u16 {
        let mut tiles = Map16Page::default();
        for tile in page * PAGE_TILES..(page + 1) * PAGE_TILES {
            let gfx = if page == 2 && file.tileset_page2.is_some() {
                Map16Tile::default()
            } else {
                allocated(tile as usize)
            };
            let entry = Map16Entry {
                gfx,
                acts: file.acts[tile as usize],
            };
            if entry != Map16Entry::default() {
                tiles.tiles.insert(tile, entry);
            }
        }
        if !tiles.tiles.is_empty() {
            out.foreground.push((page as u8, tiles));
        }
    }
    // BG Map16: table 0's pages 0 and 1 against the game's, the rest
    // against empty tiles.
    for page in 0..0x80u16 {
        let mut tiles = Map16Page::default();
        for i in 0..PAGE_TILES {
            let n = page * PAGE_TILES + i;
            let gfx = allocated(BG_START + n as usize);
            let base = if page < 2 {
                definition(clean, MAP16_BG_TILES.add(8 * n as u32))?
            } else {
                Map16Tile::default()
            };
            if gfx != base {
                // Keyed as the background pages are: the tile's number
                // in its table plus $1000 times the table.
                tiles.tiles.insert(
                    n,
                    Map16Entry {
                        gfx,
                        ..Map16Entry::default()
                    },
                );
            }
        }
        if !tiles.tiles.is_empty() {
            out.background.push((page as u8, tiles));
        }
    }
    // The pipes: colour sets 0, 2, and 3 (set 1 is page 1's own tiles),
    // and the diagonal pipes.
    for (set_index, set) in file.pipes.iter().enumerate() {
        let set_index = set_index as u8;
        if !crate::source::map16::PIPE_SETS.contains(&set_index) {
            continue;
        }
        for (i, &gfx) in set.iter().enumerate() {
            let tile = *crate::map16::PIPE_COLOUR_TILES.start() + i as u16;
            let at = crate::map16::pipe_address(Some(set_index), tile).expect("a pipe tile");
            if gfx != definition(clean, at)? {
                out.pipes.colours.insert((set_index, tile), gfx);
            }
        }
    }
    for (i, tile) in crate::map16::diagonal_pipe_tiles().enumerate() {
        let gfx = file.diagonal[i];
        let at = crate::map16::pipe_address(None, tile).expect("a pipe tile");
        if gfx != definition(clean, at)? {
            out.pipes.diagonal.insert(tile, gfx);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::objects::ScreenExit;

    fn blank_rom() -> Rom {
        let mut bytes = vec![0; 0x80000];
        bytes[0x7FD5] = 0x20;
        Rom::from_headerless(bytes).unwrap()
    }

    fn project_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kobo-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn exits_go_to_the_game_format_when_it_can_say_them() {
        let exit = |flags| {
            Object::ScreenExit(ScreenExit {
                screen: 3,
                flags,
                destination: 0xFC,
            })
        };
        // `u` and `h` from Lunar Magic's export of a level in bank 1. An
        // exit to bank 0, and one with `w`, stay in its format.
        let mut objects = vec![exit(0x05), exit(0x07), exit(0x04), exit(0x0F), exit(0x0D)];
        exits_in_game_format(&mut objects, 0x105);
        assert_eq!(
            objects,
            [exit(0x00), exit(0x02), exit(0x04), exit(0x0F), exit(0x0D)]
        );
    }

    #[test]
    fn mwl_import_places_the_level_and_keeps_comments() {
        let clean = blank_rom();
        let bytes = crate::mwl::tests::sample();
        let dir = project_dir("placement");
        // A new project, under the level the file names.
        let report = import_mwl(&bytes, &clean, &dir, None).unwrap();
        assert_eq!(report.levels, [0x105]);
        assert!(dir.join("levels/105.toml").exists());

        // A level already listed keeps its file, and both files their
        // comments.
        let manifest = "# my hack\nformat = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n\
                        # the second\n0x106 = \"a/custom.toml\"\n";
        fs::write(dir.join(MANIFEST), manifest).unwrap();
        let level = fs::read_to_string(dir.join("levels/105.toml")).unwrap();
        fs::create_dir_all(dir.join("a")).unwrap();
        fs::write(dir.join("a/custom.toml"), format!("# kept\n\n{level}")).unwrap();
        import_mwl(&bytes, &clean, &dir, Some(0x106)).unwrap();
        let text = fs::read_to_string(dir.join("a/custom.toml")).unwrap();
        assert!(text.starts_with("# kept\n"), "{text}");
        assert!(!dir.join("levels/106.toml").exists());
        let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
        assert!(
            text.contains("# my hack") && text.contains("# the second"),
            "{text}"
        );

        // Another level's file is not taken over, and a level past 1FF is
        // refused before anything is written.
        fs::write(
            dir.join(MANIFEST),
            "format = 1\n\n[levels]\n0x106 = \"levels/107.toml\"\n",
        )
        .unwrap();
        assert!(matches!(
            import_mwl(&bytes, &clean, &dir, Some(0x107)),
            Err(ImportError::FileTaken { level: 0x106, .. })
        ));
        assert!(matches!(
            import_mwl(&bytes, &clean, &dir, Some(0x200)),
            Err(ImportError::LevelNumber(0x200))
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mwl_settings_a_level_file_cannot_hold_are_reported() {
        let clean = blank_rom();
        let mut mwl = MwlFile::parse(&crate::mwl::tests::sample())
            .unwrap()
            .decode(None)
            .unwrap();
        mwl.exgfx = mwl::ExGfx::VANILLA;
        let (level, notes) = level_from_mwl(&mwl, 0x105, &clean).unwrap();
        let count = notes.len();
        // A graphics list comes through when it changes what the level
        // loads.
        let mut changed = mwl.clone();
        changed.exgfx.0[7] = 0x80;
        assert_eq!(level_from_mwl(&changed, 0x105, &clean).unwrap().0, level);
        changed.exgfx.0[0] |= 0x8000;
        let (other, notes) = level_from_mwl(&changed, 0x105, &clean).unwrap();
        assert_eq!(other.graphics, Some(changed.exgfx));
        assert_eq!(notes.len(), count);
        // ExAnimation comes through, and its settings where they are not
        // what a build gives the level anyway.
        let mut changed = mwl.clone();
        let list = [
            0x01, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00,
            0x20, 0x00, 0xAD,
        ];
        changed.animation.data = list.to_vec();
        changed.animation.header.0[0] = 0x40;
        let (other, notes) = level_from_mwl(&changed, 0x105, &clean).unwrap();
        assert_eq!(other.animation.unwrap().to_bytes(), list);
        assert_eq!(other.animation_settings, Some(0x40));
        assert_eq!(notes.len(), count);
        // The level size comes through; its top bit follows from the level.
        let mut changed = mwl.clone();
        changed.info.lm3[2] = 0x80;
        let (same, notes) = level_from_mwl(&changed, 0x105, &clean).unwrap();
        assert_eq!((same, notes.len()), (level.clone(), count));
        changed.info.lm3[2] = 0x47;
        let (taller, notes) = level_from_mwl(&changed, 0x105, &clean).unwrap();
        assert_eq!(
            taller.size,
            LevelSize {
                mode: 7,
                bottom_row: true,
                split: false,
            }
        );
        assert_eq!(notes.len(), count);
        // The midway settings are the level file's.
        let mut changed = mwl.clone();
        changed.info.midway[0] = 0x20;
        let (other, notes) = level_from_mwl(&changed, 0x105, &clean).unwrap();
        assert_ne!(other, level);
        assert_eq!(notes.len(), count);
    }
}
