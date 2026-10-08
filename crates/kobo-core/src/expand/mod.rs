//! Expanding a level by running the ROM's own code on the headless CPU:
//! the level loader for the Map16 tile grid, level preparation for the
//! graphics and palettes it uploads, and the level loop for the sprites
//! and the player.
//!
//! The game keeps the grid in two byte planes: low bytes at `$7EC800` and
//! high bytes at `$7FC800`, `0x3800` bytes each. Horizontal levels store
//! each screen as 16 columns by 27 rows, screen after screen. Vertical
//! levels are 32 tiles wide; each screen is 16 rows stored as a left
//! half and a right half of 16 by 16 tiles.
//!
//! Every pass reaches the game's variables through [`crate::ram`] and
//! calls its routines through `machine`, so neither where a ROM keeps its
//! RAM nor how a routine is entered is decided here.

use thiserror::Error;

use crate::cpu::CpuError;
use crate::level::LevelError;

mod boss;
mod diagnostics;
#[cfg(test)]
mod install_tests;
mod layer3;
mod load;
mod load_flags;
mod loaded;
mod machine;
mod map16;
mod oam;
mod object_map;
mod overworld;
mod player;
mod routines;
mod sprite_capture;
mod tiles;

pub use diagnostics::{Diagnostic, Pass, summarize};
pub(crate) use load::expand_controlled;
pub use load::{
    Called, ENTRY_FRAME_COUNTER, PlayedFrame, ReadTrace, Registers, call_after_frames,
    call_in_level, decompress_gfx_file, enter_by_exit, expand_level, expand_level_traced,
    expand_level_with_control, map16_addresses, play_game_loop, play_game_loop_from,
    play_game_loop_lagging, play_level, play_level_entered, resolve_map16, secondary_entry,
};
pub use loaded::LoadedLevel;
pub use map16::{FG_PAGES, PAGE_TILES};
pub use oam::object_sizes;
pub use object_map::{ObjectMap, ObjectRef};
pub use overworld::{
    Beaten, LoadedOverworld, beat_level, beat_levels, end_event, load_overworld, load_overworld_on,
    load_overworld_on_passed, load_overworld_passed,
};
pub(crate) use sprite_capture::capture_controlled;
pub use sprite_capture::{LATE_SPRITE_FRAMES, capture_sprites, capture_sprites_with_control};
pub use tiles::{
    GRID_LEN, LAYER2_TILEMAP_LEN, LEVEL_SIZES, Layer2Objects, LevelTiles, PIPE_TILE_COUNT,
    PIPE_TILES, PIPE_VARIANTS, SCREEN_COLS, SCREEN_ROWS, max_screens,
};

#[derive(Debug, Error)]
pub enum ExpandError {
    #[error(transparent)]
    Operation(#[from] crate::operation::OperationError),
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error("level {level:03X}: {source}")]
    Cpu {
        level: u16,
        #[source]
        source: CpuError,
    },
    #[error("GFX{index:02X}: {source}")]
    Gfx {
        index: u8,
        #[source]
        source: CpuError,
    },
    #[error("the overworld did not load: game mode ${mode:02X} after {frames} frames")]
    Overworld { mode: u8, frames: u32 },
    #[error("level {level:03X}: unknown background layout, {len:#x} bytes per screen")]
    BackgroundLayout { level: u16, len: usize },
}
