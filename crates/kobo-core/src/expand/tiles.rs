//! The tile grid the game expanded a level's objects into, with the Map16
//! definitions its tile numbers resolve to.

use std::collections::HashMap;

use crate::level::{Layer2Kind, LevelMode, PrimaryHeader};
use crate::map16::Map16Tile;
use crate::rom::Rom;

/// Bytes per plane of the tile grid.
pub const GRID_LEN: usize = 0x3800;
pub const SCREEN_ROWS: usize = 27;
pub const SCREEN_COLS: usize = 16;
pub(super) const SCREEN_LEN: usize = SCREEN_ROWS * SCREEN_COLS;

/// Lunar Magic 3's level sizes (Vitor Vilela's dynamic level patch):
/// the height in pixels of each size and how many screens of it fit the
/// tile planes. Size 0 is the vanilla layout.
pub const LEVEL_SIZES: [(u16, usize); 32] = crate::level::size::SIZES;

/// Screens of a horizontal level of `rows` tile rows that fit the planes,
/// per [`LEVEL_SIZES`]; `None` for a height Lunar Magic does not define.
pub fn max_screens(rows: usize) -> Option<usize> {
    LEVEL_SIZES
        .iter()
        .find(|(height, _)| *height as usize == rows * 16)
        .map(|(_, screens)| *screens)
}

/// Where a level keeps its layer 2 objects in the tile grid, per the
/// game's layer 2 upload dispatch (`CODE_058883`) and the per-mode screen
/// pointer tables at `$00BB08` and `$00BC16`. The layout is independent
/// of layer 1's: modes 3 and 4 pair a vertical layer 1 with a horizontal
/// layer 2, and modes 5 and 6 the reverse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Objects {
    /// Horizontal screens of 16 by `rows` tiles from plane offset `base`.
    /// A level with layer 2 objects splits the screens its height allows
    /// between the layers, layer 2's from `base` (`level::size`).
    /// Vanilla's 27-row levels give 16 screens each from `0x1B00`.
    Horizontal {
        base: usize,
        rows: usize,
        screens: usize,
    },
    /// 14 screens of 32x16 tiles (left and right halves) from `0x1C00`.
    Vertical,
}

impl Layer2Objects {
    /// The layout for a level mode and layer 1 row count, or `None` when
    /// the mode uploads no layer 2 objects (background tilemap modes and
    /// boss arenas) or the height is unknown.
    pub fn for_level(mode: LevelMode, rows: usize) -> Option<Self> {
        match mode.layer2() {
            Layer2Kind::HorizontalObjects => {
                let total = max_screens(rows)?;
                let layer1 = crate::level::size::layer2_screen((rows * 16) as u16);
                Some(Self::Horizontal {
                    base: layer1 * rows * SCREEN_COLS,
                    rows,
                    screens: total - layer1,
                })
            }
            Layer2Kind::VerticalObjects => Some(Self::Vertical),
            Layer2Kind::Background | Layer2Kind::None => None,
        }
    }

    /// Plane offset of a level-wide tile position, or `None` outside the
    /// buffer.
    pub fn offset(self, x: usize, y: usize) -> Option<usize> {
        match self {
            Self::Horizontal {
                base,
                rows,
                screens,
            } => (x / SCREEN_COLS < screens && y < rows).then(|| {
                base + (x / SCREEN_COLS) * rows * SCREEN_COLS + y * SCREEN_COLS + x % SCREEN_COLS
            }),
            Self::Vertical => (x < 32 && y / 16 < 14)
                .then(|| 0x1C00 + (y / 16) * 0x200 + (x / 16) * 0x100 + (y % 16) * 16 + x % 16),
        }
    }
}

/// Bytes per plane of the layer 2 background tilemap buffer.
pub const LAYER2_TILEMAP_LEN: usize = 0x400;
/// The vertical pipe tiles whose definition the game picks by position,
/// and how many alternatives `MAP16AppTable` offers.
pub const PIPE_TILES: std::ops::RangeInclusive<u16> = 0x133..=0x13A;
pub const PIPE_TILE_COUNT: usize = 8;
pub const PIPE_VARIANTS: usize = 4;
/// Bytes per screen of a Lunar Magic 32-row background (16x32 tiles).
pub(super) const LM_TALL_SCREEN_LEN: usize = 0x200;

/// A level's expanded tile grid: what the level is, apart from how the
/// machine that loaded it was left (see [`super::LoadedLevel`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelTiles {
    pub level: u16,
    pub header: PrimaryHeader,
    /// Level mode as the game stored it.
    pub level_mode: LevelMode,
    /// Object tileset as the game stored it. Tileset 3 shifts layer 2
    /// object palettes up by four rows on upload.
    pub object_tileset: u8,
    /// True for vertical levels.
    pub vertical: bool,
    pub screens: usize,
    /// Rows per screen of the layer 1 grid: 27 for horizontal levels, 16
    /// for vertical ones, or the height Lunar Magic 3's expanded level
    /// format gave a horizontal level (`$13D7 / 16`, up to 448). Screens
    /// follow one another in the planes with a stride of `rows * 16`.
    pub rows: usize,
    pub low: Vec<u8>,
    pub high: Vec<u8>,
    /// Foreground Map16 definitions as the level's upload resolves them:
    /// every tile the ROM defines on the first [`super::FG_PAGES`] pages
    /// and on any page the object grid uses a tile of.
    /// Lunar Magic pages 2 and 3 are distinct from the same-numbered BG
    /// tiles, which live in `bg_map16`. Prefer [`LevelTiles::map16_at`],
    /// which also knows the position-dependent pipe tiles.
    pub map16: HashMap<u16, Map16Tile>,
    /// Where each of `map16` was read: the address the ROM's own Map16
    /// routine or pointer table gave. Not always a table: an output that
    /// shows definitions asks [`crate::clean_room::Map16Shown`] first.
    pub map16_sources: HashMap<u16, u32>,
    /// Vanilla definitions of the vertical pipe tiles `133`-`13A` by
    /// position: the game re-points them for every column (row in vertical
    /// levels) it uploads, choosing variant `(column / 8) % 4` from
    /// `MAP16AppTable`, so a pipe's colour depends on where it stands.
    /// Lunar Magic ROMs keep it: their upload resolves tiles through Lunar
    /// Magic's pointer routine, which reads the re-pointed entries. `None`
    /// only for a level made up without a ROM.
    pub pipe_map16: Option<[[Map16Tile; PIPE_TILE_COUNT]; PIPE_VARIANTS]>,
    /// BG Map16 definitions, indexed by the raw background tile number.
    /// Vanilla has 0x200 definitions; Lunar Magic backgrounds can use
    /// higher indices. Empty when the level has no decoded background.
    pub bg_map16: Vec<Map16Tile>,
    /// Where `bg_map16` starts, the address of background tile 0's
    /// definition as the upload reads it.
    pub bg_map16_at: Option<u32>,
    /// Layer 2 background tilemap planes, when the level uses a
    /// pre-built background instead of layer 2 objects. Raw tile numbers
    /// index `bg_map16`; `layer2_bg_tile` adds the legacy 0x200 display base.
    pub layer2_tilemap: Option<(Vec<u8>, Vec<u8>)>,
    /// Bytes per screen of the background planes: `0x1B0` (16x27, vanilla)
    /// or `0x200` (16x32, Lunar Magic's taller backgrounds).
    pub layer2_screen_len: usize,
}

impl LevelTiles {
    /// Map16 tile number at a horizontal-level position.
    pub fn tile(&self, screen: usize, x: usize, y: usize) -> u16 {
        let i = screen * self.screen_len() + y * SCREEN_COLS + x;
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// Bytes per screen in the layer 1 planes.
    pub fn screen_len(&self) -> usize {
        if self.vertical {
            0x200
        } else {
            self.rows * SCREEN_COLS
        }
    }

    /// Buffer offset of a level-wide tile position, for either orientation.
    pub fn offset(&self, x: usize, y: usize) -> usize {
        if self.vertical {
            (y / 16) * 0x200 + (x / 16) * 0x100 + (y % 16) * 16 + (x % 16)
        } else {
            (x / SCREEN_COLS) * self.screen_len() + y * SCREEN_COLS + (x % SCREEN_COLS)
        }
    }

    /// Map16 tile number at a level-wide position.
    pub fn tile_at(&self, x: usize, y: usize) -> u16 {
        let i = self.offset(x, y);
        self.low[i] as u16 | ((self.high[i] as u16) << 8)
    }

    /// The foreground definition of tile number `n` standing at level
    /// tile position (`x`, `y`): the pipe tiles `133`-`13A` take the
    /// variant the game's upload picked for that column (row in a vertical
    /// level); everything else comes from `map16`.
    pub fn map16_at(&self, n: u16, x: usize, y: usize) -> Option<&Map16Tile> {
        if let Some(variants) = &self.pipe_map16
            && PIPE_TILES.contains(&n)
        {
            let along = if self.vertical { y } else { x };
            return Some(&variants[(along / 8) % PIPE_VARIANTS][(n - PIPE_TILES.start()) as usize]);
        }
        self.map16.get(&n)
    }

    /// The foreground Map16 definitions by tile number, from `0` to the
    /// end of the last page that has one, with `None` where the ROM
    /// defines no tile: the sheet Lunar Magic's Map16 editor shows for
    /// layer 1. A vanilla pipe tile shows the variant the loader's pointer
    /// table was left with; see [`LevelTiles::map16_at`] for its
    /// position-dependent one.
    pub fn foreground_map16(&self) -> Vec<Option<Map16Tile>> {
        let pages = self
            .map16
            .keys()
            .map(|&n| n as usize / super::PAGE_TILES + 1)
            .max()
            .unwrap_or(0);
        (0..pages * super::PAGE_TILES)
            .map(|n| self.map16.get(&(n as u16)).copied())
            .collect()
    }

    /// [`Self::foreground_map16`] as an output may show it: `None` too
    /// where [`crate::clean_room::Map16Shown`] withholds a definition.
    pub fn foreground_map16_shown(&self, rom: &Rom) -> Vec<Option<Map16Tile>> {
        let shown = crate::clean_room::Map16Shown::new(rom);
        let mut tiles = self.foreground_map16();
        for (n, tile) in tiles.iter_mut().enumerate() {
            let n = n as u16;
            if self
                .map16_sources
                .get(&n)
                .is_none_or(|&at| !shown.foreground(n, at))
            {
                *tile = None;
            }
        }
        tiles
    }

    /// `bg_map16` as an output may show it: `None` where
    /// [`crate::clean_room::Map16Shown`] withholds a definition.
    pub fn bg_map16_shown(&self, rom: &Rom) -> Vec<Option<Map16Tile>> {
        let shown = crate::clean_room::Map16Shown::new(rom);
        self.bg_map16
            .iter()
            .enumerate()
            .map(|(n, &tile)| {
                self.bg_map16_at
                    .is_some_and(|base| shown.background(n as u16, base))
                    .then_some(tile)
            })
            .collect()
    }

    /// Whether layer 2 shows the decoded background tilemap. A buffer held
    /// under any other mode is not displayed: boss arenas and the dark
    /// rooms sharing their tilemap never upload it.
    pub fn shows_background(&self) -> bool {
        self.layer2_tilemap.is_some() && self.level_mode.layer2() == Layer2Kind::Background
    }

    /// How this level's layer 2 objects are laid out, if it has any.
    pub fn layer2_objects(&self) -> Option<Layer2Objects> {
        if self.layer2_tilemap.is_some() {
            return None;
        }
        // Modes 3 and 4 pair a vertical layer 1 with a vanilla horizontal
        // layer 2.
        let rows = if self.vertical {
            SCREEN_ROWS
        } else {
            self.rows
        };
        Layer2Objects::for_level(self.level_mode, rows)
    }

    /// Map16 tile number of the layer 2 object at a level-wide position.
    /// The game resolves these through the same Map16 pointer table as
    /// layer 1, so they index `map16`, not `bg_map16`. `None` when the
    /// level's layer 2 is not objects or the position is outside the
    /// layer 2 buffer.
    pub fn layer2_object_tile(&self, x: usize, y: usize) -> Option<u16> {
        let i = self.layer2_objects()?.offset(x, y)?;
        Some(self.low[i] as u16 | ((self.high[i] as u16) << 8))
    }

    /// Palette bits the layer 2 object upload ORs into every tile: bit 2
    /// (rows 4-7) in object tileset 3, where `CODE_058B8D` ORs `$1000`
    /// into the tilemap words; nothing otherwise.
    pub fn layer2_palette_mask(&self) -> u8 {
        if self.object_tileset == 3 { 4 } else { 0 }
    }

    /// Width and height of the captured level in tiles. Some headers
    /// declare more screens than fit in the object buffer (notably
    /// unused vertical levels); only complete captured screens count.
    pub fn size(&self) -> (usize, usize) {
        let len = self.low.len().min(self.high.len());
        if self.vertical {
            (32, self.screens.min(len / 0x200) * 16)
        } else {
            (
                self.screens.min(len / self.screen_len()) * SCREEN_COLS,
                self.rows,
            )
        }
    }

    /// Map16 tile number (BG numbering, `0x200` upwards) at a position in
    /// the layer 2 background tilemap, which is two screens of 16 by 27
    /// tiles laid out like the main buffer. Returns `None` for levels
    /// whose layer 2 is objects.
    pub fn layer2_bg_tile(&self, screen: usize, x: usize, y: usize) -> Option<u16> {
        let (lo, hi) = self.layer2_tilemap.as_ref()?;
        let i = (screen % 2) * self.layer2_screen_len + y * SCREEN_COLS + x;
        Some(0x200 + lo[i] as u16 + ((hi[i] as u16) << 8))
    }

    /// Rows in the layer 2 background: 27, or 32 for Lunar Magic's taller
    /// backgrounds.
    pub fn layer2_bg_rows(&self) -> usize {
        self.layer2_screen_len / SCREEN_COLS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer2_objects_start_after_layer_1s_share_of_the_screens() {
        let horizontal = |rows: usize| match Layer2Objects::for_level(LevelMode(0x02), rows) {
            Some(Layer2Objects::Horizontal {
                base,
                rows,
                screens,
            }) => (base, rows, screens),
            other => panic!("{other:?}"),
        };
        assert_eq!(horizontal(27), (0x1B00, 27, 16));
        assert_eq!(horizontal(47), (0x1D60, 47, 9)); // 19 screens: 10 + 9
        assert_eq!(horizontal(74), (0x1BC0, 74, 6));
        assert_eq!(horizontal(298), (0x2540, 298, 1));
        assert_eq!(horizontal(448), (0x1C00, 448, 1));
        assert_eq!(Layer2Objects::for_level(LevelMode(0x02), 50), None);
        assert_eq!(Layer2Objects::for_level(LevelMode(0x00), 27), None);
        let layout = Layer2Objects::for_level(LevelMode(0x01), 47).unwrap();
        assert_eq!(layout.offset(17, 3), Some(0x1D60 + 0x2F0 + 3 * 16 + 1));
        assert_eq!(layout.offset(9 * 16, 0), None);
        assert_eq!(layout.offset(0, 47), None);
    }
}
