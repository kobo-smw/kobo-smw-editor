//! Everything loading a level yields.

use super::{Diagnostic, LevelTiles};
use crate::addr::SnesAddr;
use crate::palette::Color15;
use crate::ram::{self, Ram};
use crate::video::{LevelScene, VideoMemory};

/// A level as the game loaded and prepared it: the level itself, and the
/// machine it was loaded on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LoadedLevel {
    /// The expanded tile grid and its Map16 definitions.
    pub tiles: LevelTiles,
    /// What level preparation uploaded: graphics, tilemaps, and palette.
    pub video: VideoMemory,
    /// How the level is shown on entry: layer positions, screen setup,
    /// layer 3, the player, a boss arena's video bands.
    pub scene: LevelScene,
    /// The game's RAM after level preparation: what the sprite passes
    /// start from, and open to inspection through [`crate::ram`].
    pub ram: Ram,
    /// What went wrong without stopping the level from loading: the
    /// player's entrance pass giving up leaves `scene.player` empty.
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadedLevel {
    /// Where the game found the level's sprite data, honouring any Lunar
    /// Magic relocation.
    pub fn sprite_data_ptr(&self) -> SnesAddr {
        SnesAddr::new(self.ram.u24(ram::SPRITE_DATA_PTR))
    }

    /// The back area colour the game settled on.
    pub fn back_area_color(&self) -> Color15 {
        Color15(self.ram.u16(ram::BACKGROUND_COLOR))
    }
}
