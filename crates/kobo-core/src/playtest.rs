//! Play from here: a build of a project that goes from the title screen
//! straight into a level, with the player at a tile of the editor's
//! choosing. The level gets a secondary entrance there, placed by tile
//! (Lunar Magic's position method 2), in a copy of the project; the build
//! of that copy gets [`PATCH`], which on the title screen starts a game
//! and takes a screen exit to the entrance. The project itself is not
//! changed.

use thiserror::Error;

use crate::asar::{Asar, AsarError};
use crate::edit::{Workspace, WorkspaceError};
use crate::entrance::{Camera, EntranceSettings};
use crate::rom::{Rom, RomError};
use crate::source::level::Entrance;

/// Kobo's patch for it, `asm/playtest.asm`.
pub const PATCH: (&str, &str) = ("playtest.asm", include_str!("../asm/playtest.asm"));

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
}

/// The entrance at `start`'s tile, numbered `id`: in a horizontal level
/// the tile's screen and its column on it, in a vertical one its screen
/// down the level; the camera as the level's main entrance has it.
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
            slippery: level.settings.slippery,
            tile_position: Some(((x >> 3) as u8, (y >> 4) as u8)),
            relative,
            face_left: false,
            water: level.settings.water,
            overworld: None,
        },
    }
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
    let patch = crate::install::patch(PATCH)
        .define("entrance", format!("${id:03X}"))
        .define("powerup", format!("{}", start.powerup & 3))
        .define("time", format!("{}", level.header.time & 3));
    let mut rom = asar.patch(&rom, &patch).map_err(Box::new)?.rom;
    rom.fix_checksum()?;
    Ok(rom)
}
