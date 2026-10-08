//! Play from here: a build of a project that goes from power-on straight
//! into a level, with the player at a tile of the editor's choosing. The
//! level gets a secondary entrance there, placed by tile (Lunar Magic's
//! position method 2), in a copy of the project; the build of that copy
//! gets [`PATCH`], which where the title screen would load starts a game
//! and takes a screen exit to the entrance. The project itself is not
//! changed.

use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::asar::{Asar, AsarError};
use crate::edit::{Workspace, WorkspaceError};
use crate::entrance::{Camera, EntranceSettings};
use crate::rom::{Rom, RomError};
use crate::source::level::Entrance;

/// Kobo's patch for it, `asm/playtest.asm`.
pub const PATCH: (&str, &str) = ("playtest.asm", include_str!("../asm/playtest.asm"));

/// Where kkevinm's Retry System, in a project's UberASM Tool folder, says
/// where it keeps its RAM: [`PATCH`] sets its respawn point to the
/// entrance, which it otherwise sets only on an entry from the overworld.
const RETRY_RAM: &str = "retry_config/ram.asm";

/// The entrance actions of a slippery level and a water level.
const SLIPPERY_ACTION: u8 = 5;
const WATER_ACTION: u8 = 7;

/// Where to start, and as what.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Start {
    pub level: u16,
    /// The tile the player stands on, in the level's tiles.
    pub x: u16,
    pub y: u16,
    /// 0 small, 1 big, 2 cape, 3 fire.
    pub powerup: u8,
}

/// Where the editor writes a project's play build: in `kobo/play` in the
/// user's cache directory, not the project's folder, named after the
/// folder and a hash of its path, so that each project has its own and an
/// emulator's saves beside it stay that project's.
pub fn rom_path(project: &Path) -> Option<PathBuf> {
    let project = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let hash = Sha1::digest(project.to_string_lossy().as_bytes());
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let file = format!(
        "{name}-{:02x}{:02x}{:02x}{:02x}.sfc",
        hash[0], hash[1], hash[2], hash[3]
    );
    Some(dirs::cache_dir()?.join("kobo").join("play").join(file))
}

#[derive(Debug, Error)]
pub enum PlaytestError {
    #[error("level {0:03X} is not in the project")]
    NoLevel(u16),
    #[error("every secondary entrance number in level {0:03X}'s half is taken")]
    NoEntrance(u16),
    #[error(transparent)]
    Build(#[from] WorkspaceError),
    #[error("the play patch: {0}")]
    Asar(#[from] Box<AsarError>),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("the level does not load: {0}")]
    Load(#[from] Box<crate::expand::ExpandError>),
}

/// The entrance at `start`'s tile, numbered `id`: in a horizontal level
/// the tile's screen and its column on it, in a vertical one its screen
/// down the level; the camera, water, and slipperiness as the level's main
/// entrance has them. The game's own water and slippery levels say so by
/// their entrance's action (7 and 5, `CODE_00A6CC`), which this entrance
/// cannot take, since 7 brings the player out of a pipe: it takes Lunar
/// Magic's bits for them instead, which its placing by tile installs the
/// code for anyway.
pub fn entrance(level: &crate::source::level::Level, start: &Start, id: u16) -> Entrance {
    let vertical = level.header.level_mode.layer1_vertical();
    let (screen, x, y) = if vertical {
        (start.y / 16, start.x & 31, start.y % 16)
    } else {
        (start.x / 16, start.x % 16, start.y & 1023)
    };
    let main = level.entrance;
    let camera = Camera::from_bits(
        main.fg_position,
        main.bg_position,
        level.settings.relative,
        false,
    );
    let (fg_position, bg_position, relative) = camera.to_bits(true);
    Entrance {
        id,
        screen: screen as u8,
        x: (x & 7) as u8,
        y: (y & 15) as u8,
        action: 0,
        fg_position,
        bg_position,
        settings: EntranceSettings {
            slippery: level.settings.slippery || main.entrance_action == SLIPPERY_ACTION,
            tile_position: Some(((x >> 3) as u8, (y >> 4) as u8)),
            relative,
            face_left: false,
            water: level.settings.water || main.entrance_action == WATER_ACTION,
            overworld: None,
        },
    }
}

/// The tile the level's main entrance puts the player on, by its load in
/// a build of `workspace`.
pub fn entrance_tile(workspace: &Workspace, level: u16) -> Result<(u16, u16), PlaytestError> {
    let (rom, _) = workspace.build_leaving_out(Some(level))?;
    let loaded = crate::expand::expand_level(&rom, level).map_err(Box::new)?;
    let x = loaded.ram.u16(crate::ram::PLAYER_X);
    let y = loaded.ram.u16(crate::ram::PLAYER_Y);
    Ok((x / 16, y / 16))
}

/// A build of `workspace` that starts at `start`.
pub fn build(workspace: &Workspace, start: &Start, asar: &Asar) -> Result<Rom, PlaytestError> {
    let level = workspace
        .level(start.level)
        .ok_or(PlaytestError::NoLevel(start.level))?;
    let id = workspace
        .free_entrance(start.level)
        .ok_or(PlaytestError::NoEntrance(start.level))?;
    let mut level = level.clone();
    level.entrances.push(entrance(&level, start, id));
    let mut copy = workspace.clone();
    copy.set_level(start.level, &level);
    let (rom, _) = copy.build_leaving_out(Some(start.level))?;
    let mut patch = crate::install::patch(PATCH)
        .define("entrance", format!("${id:03X}"))
        .define(
            "translevel",
            format!("${:02X}", translevel(workspace, start.level)),
        )
        .define("powerup", format!("{}", start.powerup & 3))
        .define("time", format!("{}", level.header.time & 3));
    let project = workspace.project();
    let retry = project
        .manifest
        .uberasm
        .as_ref()
        .map(|dir| project.root.join(dir).join(RETRY_RAM))
        .and_then(|path| std::fs::read(path).ok());
    patch = match retry {
        Some(text) => patch.file("retry_ram.asm", text).define("retry", "1"),
        None => patch.define("retry", "0"),
    };
    let mut rom = asar.patch(&rom, &patch).map_err(Box::new)?.rom;
    rom.fix_checksum()?;
    Ok(rom)
}

/// The translevel the game would have entered `level` by: its own, or for
/// a sublevel its overworld level's (`edit::reach`), or 0 for a level no
/// exit reaches. Midway points, the retry system's checkpoints, and the
/// Dragon Coins collected are kept by it.
fn translevel(workspace: &Workspace, level: u16) -> u8 {
    crate::level::translevel(level)
        .or_else(|| {
            let all: Vec<u16> = (0..0x200).collect();
            let group = crate::edit::reach::Reach::of(workspace, &all).group_of(level)?;
            crate::level::translevel(group)
        })
        .unwrap_or(0)
}
