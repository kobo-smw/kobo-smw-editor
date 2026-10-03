//! Map16: 16x16 level tiles built from four 8x8 tiles.
//!
//! The game keeps a table of 0x200 pointers (`$7E0FBE`) to 8-byte tile
//! definitions. For layer 1 the pointers mix a common table with a
//! tileset-specific one, selected per tile by a bitmask. For layer 2 the
//! pointers all come from the BG table, which Lunar Magic numbers as tiles
//! `0x200` to `0x3FF`. This module reproduces that assembly for a vanilla
//! ROM. Lunar Magic's relocated pages are resolved by running its own
//! pointer routine during a level load: see `expand::LevelTiles::map16`.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::rom::{Rom, RomError};

/// One 8x8 tile reference as stored in SNES tilemaps: `yxpccctt tttttttt`
/// (y flip, x flip, priority, palette, 10-bit tile number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Tile8Ref(pub u16);

impl Tile8Ref {
    pub const fn new(tile: u16, palette: u8, priority: bool, flip_x: bool, flip_y: bool) -> Self {
        Self(
            (tile & 0x3FF)
                | (((palette & 7) as u16) << 10)
                | ((priority as u16) << 13)
                | ((flip_x as u16) << 14)
                | ((flip_y as u16) << 15),
        )
    }

    pub const fn tile(self) -> u16 {
        self.0 & 0x3FF
    }

    pub const fn palette(self) -> u8 {
        ((self.0 >> 10) & 7) as u8
    }

    pub const fn priority(self) -> bool {
        self.0 & 0x2000 != 0
    }

    pub const fn flip_x(self) -> bool {
        self.0 & 0x4000 != 0
    }

    pub const fn flip_y(self) -> bool {
        self.0 & 0x8000 != 0
    }
}

/// A 16x16 tile. Fields are in the game's storage order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Map16Tile {
    pub top_left: Tile8Ref,
    pub bottom_left: Tile8Ref,
    pub top_right: Tile8Ref,
    pub bottom_right: Tile8Ref,
}

impl Map16Tile {
    pub fn from_bytes(b: [u8; 8]) -> Self {
        let w = |i: usize| Tile8Ref(u16::from_le_bytes([b[i], b[i + 1]]));
        Self {
            top_left: w(0),
            bottom_left: w(2),
            top_right: w(4),
            bottom_right: w(6),
        }
    }

    pub fn to_bytes(self) -> [u8; 8] {
        let mut out = [0u8; 8];
        for (i, r) in self.refs().iter().enumerate() {
            out[2 * i..2 * i + 2].copy_from_slice(&r.0.to_le_bytes());
        }
        out
    }

    /// The four references in storage order.
    pub fn refs(&self) -> [Tile8Ref; 4] {
        [
            self.top_left,
            self.bottom_left,
            self.top_right,
            self.bottom_right,
        ]
    }

    /// The reference at a quadrant, `(x, y)` in 0..2.
    pub fn quadrant(&self, x: usize, y: usize) -> Tile8Ref {
        match (x, y) {
            (0, 0) => self.top_left,
            (0, _) => self.bottom_left,
            (_, 0) => self.top_right,
            _ => self.bottom_right,
        }
    }
}

/// Vanilla Map16 tables.
pub mod tables {
    use crate::addr::SnesAddr;

    /// 15 words: bank `$0D` offsets of each tileset's specific tile data.
    pub const TILESET_MAP16_LOC: SnesAddr = SnesAddr::new(0x058000);
    /// 64-byte bitmask over tiles 0x000 to 0x1FF, MSB first. A set bit means
    /// the tile comes from the common table, clear means tileset-specific.
    pub const TILESET_SPECIFIC_MASK: SnesAddr = SnesAddr::new(0x0581BB);
    /// Common layer 1 tile data.
    pub const MAP16_COMMON: SnesAddr = SnesAddr::new(0x0D8000);
    /// Diagonal pipe tiles patched over 1C4-1C7 and 1EC-1EF in tilesets 0 and 7.
    pub const DIAGONAL_PIPE_TILES: SnesAddr = SnesAddr::new(0x0D8A70);
    /// The vertical pipes' tiles `$133`-`$13A` take one of four sets of
    /// definitions by the screen they are on (`MAP16AppTable`, `$058776`):
    /// set 1 is page 1's own, and sets 0, 2, and 3 are these, eight tiles
    /// each. Lunar Magic edits all four in place (its Map16 editor's pipe
    /// button) and its full Map16 export holds them in this order.
    pub const PIPE_COLOUR_SETS: [(u8, SnesAddr); 3] = [
        (0, SnesAddr::new(0x0D8AB0)),
        (2, SnesAddr::new(0x0D8AF0)),
        (3, SnesAddr::new(0x0D8B30)),
    ];
    /// Layer 2 (BG) tile data, 0x200 tiles.
    pub const MAP16_BG_TILES: SnesAddr = SnesAddr::new(0x0D9100);
}

/// The tiles the vertical pipes' colour sets define, in order.
pub const PIPE_COLOUR_TILES: std::ops::RangeInclusive<u16> = 0x133..=0x13A;

/// The tiles the diagonal pipes replace in object tilesets 0 and 7, in the
/// order their definitions are kept.
pub fn diagonal_pipe_tiles() -> impl Iterator<Item = u16> {
    (0x1C4..=0x1C7).chain(0x1EC..=0x1EF)
}

/// Where the game keeps the other definition of `tile` in colour set `set`
/// (0, 2, or 3) or, with `None`, the diagonal pipes'.
pub fn pipe_address(set: Option<u8>, tile: u16) -> Option<SnesAddr> {
    match set {
        Some(set) => {
            let (_, at) = tables::PIPE_COLOUR_SETS.iter().find(|(s, _)| *s == set)?;
            PIPE_COLOUR_TILES
                .contains(&tile)
                .then(|| at.add(8 * (tile - PIPE_COLOUR_TILES.start()) as u32))
        }
        None => diagonal_pipe_tiles()
            .position(|t| t == tile)
            .map(|i| tables::DIAGONAL_PIPE_TILES.add(8 * i as u32)),
    }
}

pub const TILESET_COUNT: u8 = 15;
pub const FG_TILE_COUNT: usize = 0x200;
pub const BG_TILE_COUNT: usize = 0x200;

#[derive(Debug, Error)]
pub enum Map16Error {
    #[error("tileset {0} is out of range (0 to 14)")]
    BadTileset(u8),
    #[error(transparent)]
    Rom(#[from] RomError),
}

/// The 0x400 Map16 tiles visible to a level: 0x000-0x1FF for layer 1 in
/// the given tileset, 0x200-0x3FF for layer 2.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Map16Table {
    pub tileset: u8,
    pub tiles: Vec<Map16Tile>,
}

impl Map16Table {
    pub fn get(&self, index: u16) -> Option<&Map16Tile> {
        self.tiles.get(index as usize)
    }

    /// Raw bytes of a tile range, as Lunar Magic exports them.
    pub fn bytes(&self, range: std::ops::Range<usize>) -> Vec<u8> {
        self.tiles[range]
            .iter()
            .flat_map(|t| t.to_bytes())
            .collect()
    }
}

/// Where the game keeps the definitions of pages 0 and 1, which Lunar
/// Magic rewrites in place: tiles the mask at
/// [`tables::TILESET_SPECIFIC_MASK`] marks as common in one table, the
/// rest in a table per object tileset, which some tilesets share.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameTables {
    mask: [u8; FG_TILE_COUNT / 8],
    specific: [u16; TILESET_COUNT as usize],
}

impl GameTables {
    pub fn read(rom: &Rom) -> Result<Self, RomError> {
        let mut mask = [0; FG_TILE_COUNT / 8];
        mask.copy_from_slice(rom.read(tables::TILESET_SPECIFIC_MASK, FG_TILE_COUNT / 8)?);
        let mut specific = [0; TILESET_COUNT as usize];
        for (i, pointer) in specific.iter_mut().enumerate() {
            *pointer = rom.read_u16(tables::TILESET_MAP16_LOC.add(2 * i as u32))?;
        }
        Ok(Self { mask, specific })
    }

    /// Whether tile `tile` (below `$200`) has a definition per tileset.
    pub fn is_specific(&self, tile: u16) -> bool {
        let t = tile as usize;
        self.mask[t / 8] & (0x80 >> (t % 8)) == 0
    }

    /// How many tiles before `tile` are in the same table as it.
    fn rank(&self, tile: u16) -> u32 {
        let specific = self.is_specific(tile);
        (0..tile)
            .filter(|&t| self.is_specific(t) == specific)
            .count() as u32
    }

    /// The address of tile `tile`'s definition (below `$200`) in object
    /// tileset `tileset`, as the game reads it before the diagonal pipes
    /// and the pipe colours replace some.
    pub fn address(&self, tileset: u8, tile: u16) -> SnesAddr {
        if self.is_specific(tile) {
            let table = self.specific[tileset as usize % TILESET_COUNT as usize];
            SnesAddr::from_bank_offset(0x0D, table).add(8 * self.rank(tile))
        } else {
            tables::MAP16_COMMON.add(8 * self.rank(tile))
        }
    }

    /// The object tilesets by the table of their own tiles they share,
    /// each list in order and the lists by their first tileset.
    pub fn sharing(&self) -> Vec<Vec<u8>> {
        let mut groups: Vec<Vec<u8>> = Vec::new();
        for tileset in 0..TILESET_COUNT {
            let table = self.specific[tileset as usize];
            match groups
                .iter_mut()
                .find(|g| self.specific[g[0] as usize] == table)
            {
                Some(group) => group.push(tileset),
                None => groups.push(vec![tileset]),
            }
        }
        groups
    }
}

fn read_tile(rom: &Rom, addr: SnesAddr) -> Result<Map16Tile, RomError> {
    let b = rom.read(addr, 8)?;
    Ok(Map16Tile::from_bytes(b.try_into().expect("8 bytes")))
}

/// Builds the vanilla Map16 table for a tileset, mirroring the game's
/// pointer setup at level load. `apply_pipe_override` reproduces the
/// diagonal pipe patch the game applies for tilesets 0 and 7; Lunar
/// Magic's export leaves those eight tiles as the tileset data has them.
pub fn vanilla_map16(
    rom: &Rom,
    tileset: u8,
    apply_pipe_override: bool,
) -> Result<Map16Table, Map16Error> {
    use tables::*;
    if tileset >= TILESET_COUNT {
        return Err(Map16Error::BadTileset(tileset));
    }
    let mut specific = SnesAddr::from_bank_offset(
        0x0D,
        rom.read_u16(TILESET_MAP16_LOC.add(2 * tileset as u32))?,
    );
    let mut common = MAP16_COMMON;
    let mask = rom.read(TILESET_SPECIFIC_MASK, FG_TILE_COUNT / 8)?;
    let mut tiles = Vec::with_capacity(FG_TILE_COUNT + BG_TILE_COUNT);
    for t in 0..FG_TILE_COUNT {
        let from_common = mask[t / 8] & (0x80 >> (t % 8)) != 0;
        let src = if from_common {
            &mut common
        } else {
            &mut specific
        };
        tiles.push(read_tile(rom, *src)?);
        *src = src.add(8);
    }
    if apply_pipe_override && (tileset == 0 || tileset == 7) {
        let mut addr = DIAGONAL_PIPE_TILES;
        for t in diagonal_pipe_tiles() {
            tiles[t as usize] = read_tile(rom, addr)?;
            addr = addr.add(8);
        }
    }
    for i in 0..BG_TILE_COUNT {
        tiles.push(read_tile(rom, MAP16_BG_TILES.add(8 * i as u32))?);
    }
    Ok(Map16Table { tileset, tiles })
}

/// Lunar Magic's tables for foreground pages 2 to `$7F` and for what every
/// tile acts like, at the fixed addresses its layout keeps their pointers
/// (docs/lunar-magic-install.md). Kobo's Map16 routine and acts-like chain
/// (`asm/lunar-magic/`) read the same tables.
pub mod pages {
    use crate::addr::SnesAddr;
    use crate::rom::{Rom, RomError};

    /// Not `$FF` once the Map16 routine and the acts-like chain are
    /// installed, Lunar Magic's or Kobo's.
    pub const INSTALLED: SnesAddr = SnesAddr::new(0x06F600);
    /// 24-bit pointer to what tiles `$0000`-`$3FFF` act like, 2 bytes a tile.
    pub const ACTS_LIKE: SnesAddr = SnesAddr::new(0x06F624);
    /// 24-bit pointer, less `$8000`, to what tiles `$4000`-`$7FFF` act like;
    /// bank `$FF` for none.
    pub const ACTS_LIKE_UPPER: SnesAddr = SnesAddr::new(0x06F63A);

    /// 16 pages that share a table: tile `n`'s definition is at the table's
    /// pointer (plus 1 if kept less one) plus `n * 8` in 16 bits, in the
    /// pointer's bank.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct PageGroup {
        pub first_page: u8,
        pub pointer: SnesAddr,
        pub bank: SnesAddr,
        pub less_one: bool,
    }

    pub const PAGE_GROUPS: [PageGroup; 8] = {
        const fn g(first_page: u8, pointer: u32, bank: u32, less_one: bool) -> PageGroup {
            PageGroup {
                first_page,
                pointer: SnesAddr::new(pointer),
                bank: SnesAddr::new(bank),
                less_one,
            }
        }
        [
            g(0x00, 0x06F553, 0x06F557, false),
            g(0x10, 0x06F55C, 0x06F560, false),
            g(0x20, 0x06F567, 0x06F56B, true),
            g(0x30, 0x06F570, 0x06F574, true),
            g(0x40, 0x06F594, 0x06F598, false),
            g(0x50, 0x06F59D, 0x06F5A1, false),
            g(0x60, 0x06F5A8, 0x06F5AC, true),
            g(0x70, 0x06F5B1, 0x06F5B5, true),
        ]
    };

    impl PageGroup {
        /// The group's pages past 1.
        pub fn pages(&self) -> std::ops::RangeInclusive<u8> {
            self.first_page.max(2)..=self.first_page + 15
        }

        /// Where the group's table has tile `first`, given the table's
        /// address, as the pointer and bank to store.
        pub fn stored_for(&self, first: u16, at: SnesAddr) -> (u16, u8) {
            let pointer = at
                .offset()
                .wrapping_sub(first.wrapping_mul(8))
                .wrapping_sub(self.less_one as u16);
            (pointer, at.bank())
        }

        /// The address of tile `tile`'s definition, or `None` if the
        /// group has no table (bank `$00`, as a fresh install leaves it).
        pub fn definition(&self, rom: &Rom, tile: u16) -> Result<Option<SnesAddr>, RomError> {
            let bank = rom.read_u8(self.bank)?;
            if bank == 0 {
                return Ok(None);
            }
            let pointer = rom.read_u16(self.pointer)?;
            let offset = pointer
                .wrapping_add(self.less_one as u16)
                .wrapping_add(tile.wrapping_mul(8));
            Ok(Some(SnesAddr::from_bank_offset(bank, offset)))
        }
    }

    /// 16 3-byte pointers to the BG Map16 tables, each tile 0 of its 16
    /// pages; `$000000` for none. A fresh install has the game's table,
    /// `$0D9100`, first.
    pub const BG_TABLES: SnesAddr = SnesAddr::new(0x0EFD50);
    /// Lunar Magic's hook in the background column upload that reads
    /// [`BG_TABLES`]: a `JSL` when its BG Map16 piece is installed.
    pub const BG_MAP16_HOOK: SnesAddr = SnesAddr::new(0x058DA4);

    /// BG Map16 table `table`'s address, if it has one and the ROM has the
    /// piece that reads them.
    pub fn bg_table(rom: &Rom, table: u8) -> Result<Option<SnesAddr>, RomError> {
        if rom.read_u8(BG_MAP16_HOOK)? != 0x22 {
            return Ok(None);
        }
        let at = rom.read_u24(BG_TABLES.add(3 * table as u32))?;
        Ok((at != 0).then_some(SnesAddr::new(at)))
    }

    /// Whether the ROM has Lunar Magic's layout for pages past 1.
    pub fn installed(rom: &Rom) -> bool {
        rom.read_u8(INSTALLED).is_ok_and(|b| b != 0xFF)
    }

    /// Not zero when page 2 has a table per object tileset (Lunar Magic's
    /// "tileset specific" page 2, off in new ROMs).
    pub const TILESET_PAGE2: SnesAddr = SnesAddr::new(0x06F547);
    /// The value a build writes at [`TILESET_PAGE2`] to turn it on: Lunar
    /// Magic 3.70's editor takes page 2 as per tileset only with `$06`
    /// there, when the marker at [`ACTS_LIKE_MARKER`] is set
    /// (docs/lunar-magic-install.md).
    pub const TILESET_PAGE2_ON: u8 = 0x06;
    /// `"LM"`, which Lunar Magic checks before it reads the acts-like
    /// tables through [`ACTS_LIKE`] and [`TILESET_PAGE2`] as its current
    /// layout has them; Kobo's install writes it.
    pub const ACTS_LIKE_MARKER: SnesAddr = SnesAddr::new(0x06F5FC);
    /// The per-tileset page 2 table's pointer, less `$1000`: tileset `t`'s
    /// tile `n` is at it plus `n * 8` plus `t * $800`, in 16 bits, in the
    /// bank at [`TILESET_PAGE2_BANK`].
    pub const TILESET_PAGE2_POINTER: SnesAddr = SnesAddr::new(0x06F586);
    pub const TILESET_PAGE2_BANK: SnesAddr = SnesAddr::new(0x06F58A);
    /// Bytes of the per-tileset page 2 table: a page for each of the 15
    /// object tilesets.
    pub const TILESET_PAGE2_LEN: usize = 15 * 0x800;

    /// Whether page 2 is per tileset, in Lunar Magic's layout.
    pub fn tileset_page2(rom: &Rom) -> Result<bool, RomError> {
        Ok(installed(rom) && rom.read_u8(TILESET_PAGE2)? != 0)
    }

    /// The per-tileset page 2 table's address, given where it is to go, as
    /// the pointer and bank to store.
    pub fn tileset_page2_stored_for(at: SnesAddr) -> (u16, u8) {
        (at.offset().wrapping_sub(0x1000), at.bank())
    }

    /// Where tileset `tileset`'s tile `tile` (`$200`-`$2FF`) is defined
    /// when page 2 is per tileset.
    pub fn tileset_page2_definition(
        rom: &Rom,
        tileset: u8,
        tile: u16,
    ) -> Result<SnesAddr, RomError> {
        let pointer = rom.read_u16(TILESET_PAGE2_POINTER)?;
        let bank = rom.read_u8(TILESET_PAGE2_BANK)?;
        let offset = pointer
            .wrapping_add(tile.wrapping_mul(8))
            .wrapping_add((tileset as u16 & 0x0F) << 11);
        Ok(SnesAddr::from_bank_offset(bank, offset))
    }

    /// What tile `tile` acts like, from the tables, or `None` where there
    /// is no table.
    pub fn acts_like(rom: &Rom, tile: u16) -> Result<Option<u16>, RomError> {
        let at = if tile < 0x4000 {
            SnesAddr::new(rom.read_u24(ACTS_LIKE)?)
        } else {
            let pointer = rom.read_u24(ACTS_LIKE_UPPER)?;
            if pointer >> 16 == 0xFF {
                return Ok(None);
            }
            SnesAddr::new(pointer)
        };
        Ok(Some(rom.read_u16(SnesAddr::new(
            (at.raw() + 2 * tile as u32) & 0xFF_FFFF,
        ))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ref_fields() {
        let r = Tile8Ref(0x1C70);
        assert_eq!(r.tile(), 0x070);
        assert_eq!(r.palette(), 7);
        assert!(!r.priority());
        assert!(!r.flip_x());
        assert!(!r.flip_y());
        let r = Tile8Ref::new(0x3FF, 5, true, true, true);
        assert_eq!(r.0, 0xF7FF);
        assert_eq!(
            Tile8Ref::new(0x1234, 0xF, false, false, false).0,
            0x1E34 & 0x1FFF
        );
    }

    #[test]
    fn map16_tile_round_trip() {
        let b = [0x70, 0x1C, 0x72, 0x1C, 0x71, 0x1C, 0x73, 0x1C];
        let t = Map16Tile::from_bytes(b);
        assert_eq!(t.top_left.tile(), 0x70);
        assert_eq!(t.bottom_left.tile(), 0x72);
        assert_eq!(t.top_right.tile(), 0x71);
        assert_eq!(t.bottom_right.tile(), 0x73);
        assert_eq!(t.quadrant(1, 0), t.top_right);
        assert_eq!(t.quadrant(0, 1), t.bottom_left);
        assert_eq!(t.to_bytes(), b);
    }
}
