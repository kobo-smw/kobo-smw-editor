//! Lunar Magic's full Map16 export (`-ExportAllMap16`, "Save Map16 to
//! File" with all pages, a `.map16` file), read for import: projects that
//! keep their Map16 so, as Callisto's do (`import::import_callisto`).
//!
//! The file starts `LM16` and lists its sections from `$70` as offset and
//! size pairs (docs/lunar-magic.md): `$70` every tile's definition, 8 bytes
//! each, foreground `0`-`7FFF` and BG from `8000`; `$78` what tiles
//! `0`-`7FFF` act like, 2 bytes each; `$90` each object tileset's page 2
//! when it is per tileset (size 0 otherwise); `$98` each object tileset's
//! pages 0 and 1 (`$1000` bytes each); `$A0` the vertical pipes' four colour
//! sets in `MAP16AppTable`'s order (set 1 being page 1's own `$133`-`$13A`);
//! and `$A8` the diagonal pipes, as the ROM holds them. Only a full export
//! is read: a file of some pages has other sizes, and is refused.

use thiserror::Error;

use crate::map16::{Map16Tile, TILESET_COUNT};

/// Tiles a full export defines: the foreground's `0`-`7FFF`, then BG.
pub const TILE_COUNT: usize = 0x1_0000;
/// The first BG tile's index in the file.
pub const BG_START: usize = 0x8000;
/// What Lunar Magic writes for a tile it has nothing for (`$1004` four
/// times), which Kobo's sources take as an empty tile.
pub const EMPTY: [u8; 8] = [0x04, 0x10, 0x04, 0x10, 0x04, 0x10, 0x04, 0x10];

#[derive(Debug, Error)]
pub enum Map16FileError {
    #[error("not a Lunar Magic Map16 file (no `LM16` at the start)")]
    Magic,
    #[error("the file ends inside its table of sections")]
    Truncated,
    #[error("section at ${at:02X} ({offset:#x}, {len:#x} bytes) is outside the file")]
    Section {
        at: usize,
        offset: usize,
        len: usize,
    },
    #[error(
        "section at ${at:02X} is {len:#x} bytes, not {expected:#x}: not a full Map16 export \
         (save all pages, `-ExportAllMap16`)"
    )]
    Size {
        at: usize,
        len: usize,
        expected: usize,
    },
}

/// A full Map16 export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllMap16 {
    /// Every tile's definition by its number in the file: foreground
    /// `0`-`7FFF`, BG from [`BG_START`].
    pub tiles: Vec<Map16Tile>,
    /// What foreground tiles `0`-`7FFF` act like.
    pub acts: Vec<u16>,
    /// Each object tileset's own pages 0 and 1 (512 tiles), by tileset.
    pub tileset_pages: Vec<Vec<Map16Tile>>,
    /// Each object tileset's page 2 (256 tiles), when page 2 is per
    /// tileset.
    pub tileset_page2: Option<Vec<Vec<Map16Tile>>>,
    /// The vertical pipes' tiles `133`-`13A` in each of the four colour
    /// sets, in `MAP16AppTable`'s order.
    pub pipes: [[Map16Tile; 8]; 4],
    /// The diagonal pipes' tiles, as the ROM holds them at `$0D8A70`.
    pub diagonal: [Map16Tile; 8],
}

impl AllMap16 {
    pub fn parse(data: &[u8]) -> Result<Self, Map16FileError> {
        if !data.starts_with(b"LM16") {
            return Err(Map16FileError::Magic);
        }
        let word = |at: usize| -> Result<usize, Map16FileError> {
            data.get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")) as usize)
                .ok_or(Map16FileError::Truncated)
        };
        let section = |at: usize, expected: Option<usize>| -> Result<&[u8], Map16FileError> {
            let (offset, len) = (word(at)?, word(at + 4)?);
            if let Some(expected) = expected
                && len != expected
            {
                return Err(Map16FileError::Size { at, len, expected });
            }
            data.get(offset..offset.saturating_add(len))
                .ok_or(Map16FileError::Section { at, offset, len })
        };
        let tiles_of = |bytes: &[u8]| -> Vec<Map16Tile> {
            bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|&c| Map16Tile::from_bytes(c))
                .collect()
        };
        let tileset_count = TILESET_COUNT as usize;
        let tiles = tiles_of(section(0x70, Some(TILE_COUNT * 8))?);
        let acts = section(0x78, Some(BG_START * 2))?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        let page2 = section(0x90, None)?;
        let tileset_page2 = match page2.len() {
            0 => None,
            len if len == tileset_count * 0x800 => Some(
                page2
                    .as_chunks::<0x800>()
                    .0
                    .iter()
                    .map(|p| tiles_of(p))
                    .collect(),
            ),
            len => {
                return Err(Map16FileError::Size {
                    at: 0x90,
                    len,
                    expected: tileset_count * 0x800,
                });
            }
        };
        let tileset_pages = section(0x98, Some(tileset_count * 0x1000))?
            .as_chunks::<0x1000>()
            .0
            .iter()
            .map(|p| tiles_of(p))
            .collect();
        let pipe_tiles = tiles_of(section(0xA0, Some(0x100))?);
        let pipes = std::array::from_fn(|set| std::array::from_fn(|i| pipe_tiles[set * 8 + i]));
        let diagonal_tiles = tiles_of(section(0xA8, Some(0x40))?);
        let diagonal = std::array::from_fn(|i| diagonal_tiles[i]);
        Ok(Self {
            tiles,
            acts,
            tileset_pages,
            tileset_page2,
            pipes,
            diagonal,
        })
    }

    /// Tile `index`'s definition, Lunar Magic's empty tile read as Kobo's
    /// (all zeros).
    pub fn definition(&self, index: usize) -> Map16Tile {
        empty_as_zero(self.tiles[index])
    }

    /// Where each run of `$1000` tiles (a group of 16 pages, or a BG
    /// table) ends as Lunar Magic allocates it: after the last tile it has
    /// something for. The file writes Lunar Magic's empty tile for every
    /// tile it has nothing for, allocated or not, so that tile is one of
    /// the table's before the end and Kobo's empty tile after it, as a ROM
    /// import reads the tables.
    pub fn allocated_ends(&self) -> Vec<usize> {
        let empty = Map16Tile::from_bytes(EMPTY);
        (0..TILE_COUNT)
            .step_by(0x1000)
            .map(|group| {
                (group..group + 0x1000)
                    .rposition(|i| self.tiles[i] != empty)
                    .map_or(group, |last| group + last + 1)
            })
            .collect()
    }
}

/// `tile`, or Kobo's empty tile where it is Lunar Magic's.
pub fn empty_as_zero(tile: Map16Tile) -> Map16Tile {
    if tile.to_bytes() == EMPTY {
        Map16Tile::default()
    } else {
        tile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full export of zeros, page 2 not per tileset, edited by `edit`.
    pub(crate) fn export(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let sections: [(usize, usize); 8] = [
            (0x70, TILE_COUNT * 8),
            (0x78, BG_START * 2),
            (0x80, 0),
            (0x88, 0),
            (0x90, 0),
            (0x98, 15 * 0x1000),
            (0xA0, 0x100),
            (0xA8, 0x40),
        ];
        let mut data = vec![0u8; 0xB0];
        data[..4].copy_from_slice(b"LM16");
        for (at, len) in sections {
            let offset = data.len();
            data[at..at + 4].copy_from_slice(&(offset as u32).to_le_bytes());
            data[at + 4..at + 8].copy_from_slice(&(len as u32).to_le_bytes());
            data.resize(offset + len, 0);
        }
        edit(&mut data);
        data
    }

    #[test]
    fn a_full_export_reads_by_its_sections() {
        let data = export(|d| {
            // Tile 2 and BG tile 8001, an acts-like setting, Lunar Magic's
            // empty tile at 3, tileset 1's tile 5, and the last pipe tile.
            d[0xB0 + 2 * 8] = 0x11;
            d[0xB0 + 0x8001 * 8] = 0x22;
            d[0xB0 + 3 * 8..0xB0 + 4 * 8].copy_from_slice(&EMPTY);
            let acts = 0xB0 + TILE_COUNT * 8;
            d[acts + 2 * 0x200..acts + 2 * 0x200 + 2].copy_from_slice(&0x25u16.to_le_bytes());
            let tilesets = acts + BG_START * 2;
            d[tilesets + 0x1000 + 5 * 8] = 0x33;
            let pipes = tilesets + 15 * 0x1000;
            d[pipes + 0xF8] = 0x44;
        });
        let file = AllMap16::parse(&data).unwrap();
        assert_eq!(file.tiles[2].to_bytes()[0], 0x11);
        assert_eq!(file.tiles[0x8001].to_bytes()[0], 0x22);
        assert_eq!(file.definition(3), Map16Tile::default());
        assert_eq!(file.acts[0x200], 0x25);
        assert_eq!(file.tileset_pages[1][5].to_bytes()[0], 0x33);
        assert_eq!(file.pipes[3][7].to_bytes()[0], 0x44);
        assert!(file.tileset_page2.is_none());
    }

    #[test]
    fn a_partial_export_is_refused() {
        let mut data = export(|_| {});
        data[0x74..0x78].copy_from_slice(&0x800u32.to_le_bytes());
        assert!(matches!(
            AllMap16::parse(&data),
            Err(Map16FileError::Size { at: 0x70, .. })
        ));
        assert!(matches!(
            AllMap16::parse(b"MWL1"),
            Err(Map16FileError::Magic)
        ));
    }
}
