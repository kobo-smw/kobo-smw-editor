//! Lunar Magic's level sizes (3.00, "ExLevel"): a byte per level that
//! trades a horizontal level's screens for height.
//!
//! The byte is `TB0MMMMM` (the community's MWL and level format
//! documentation): `MMMMM` picks one of 32 heights ([`SIZES`]), `B` makes
//! the camera go low enough to show the level's last row whole, and `T`
//! splits the screens between layer 1 and layer 2's objects, layer 2's
//! starting where [`layer2_screen`] says (the screen pointers a load
//! leaves, `exlevel_probe sizes`), where without it they start 16 screens
//! on. A vertical level takes no size. Bit 5 is kept clear in the table;
//! the running game has the new sprite system's bit there.
//!
//! The table is Lunar Magic's layout: 512 bytes, `$240` bytes before the
//! code the `JSL` at `$05DA8A` calls, which is its check for the piece
//! (docs/lunar-magic-install.md, "Taller levels", which has the byte diff
//! that shows it). A Lunar Magic save and
//! its MWL export take the table from there, and the save writes `T` for
//! the level saved as the level's layer 2 says.

use crate::addr::SnesAddr;
use crate::rom::Rom;

/// The heights in pixels of the 32 sizes and how many screens of each fit
/// the level's tile planes. Size 0 is the game's own: 27 rows, 32 screens.
pub const SIZES: [(u16, usize); 32] = [
    (0x01B0, 0x20),
    (0x01C0, 0x20),
    (0x01D0, 0x1E),
    (0x0200, 0x1C),
    (0x0220, 0x1A),
    (0x0250, 0x18),
    (0x0260, 0x17),
    (0x0280, 0x16),
    (0x02A0, 0x15),
    (0x02C0, 0x14),
    (0x02F0, 0x13),
    (0x0310, 0x12),
    (0x0340, 0x11),
    (0x0380, 0x10),
    (0x03B0, 0x0F),
    (0x0400, 0x0E),
    (0x0440, 0x0D),
    (0x04A0, 0x0C),
    (0x0510, 0x0B),
    (0x0590, 0x0A),
    (0x0630, 0x09),
    (0x0700, 0x08),
    (0x0800, 0x07),
    (0x0950, 0x06),
    (0x0B30, 0x05),
    (0x0E00, 0x04),
    (0x12A0, 0x03),
    (0x1C00, 0x02),
    (0x3800, 0x01),
    (0x0100, 0x38),
    (0x00F0, 0x3B),
    (0x00E0, 0x40),
];

/// The screen a level of `height` pixels with layer 2 objects starts
/// layer 2's at: the one whose start is nearest the middle of the tile
/// planes (`$1C00` bytes in), the lower of two as near, and at most 16
/// (observed: every size's screen pointers, examples/exlevel_probe.rs).
pub fn layer2_screen(height: u16) -> usize {
    let height = height as usize;
    ((0x1C00 + height / 2 - 1) / height).min(16)
}

/// The `JSL` whose presence tells Lunar Magic its taller levels piece is
/// in a ROM, and whose target the size table sits before.
pub const HOOK: SnesAddr = SnesAddr::new(0x05DA8A);
/// How far before the hook's code the table is.
pub const TABLE_OFFSET: u32 = 0x240;

const SPLIT: u8 = 0x80;
const BOTTOM_ROW: u8 = 0x40;
const MODE: u8 = 0x1F;

/// A level's size, as a level file has it. Whether its screens are split
/// between the layers follows from the level when layer 2 has objects
/// ([`LevelSize::to_byte`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LevelSize {
    /// One of the 32 sizes, 0 to 31.
    pub mode: u8,
    /// The camera goes low enough to show the last row whole.
    pub bottom_row: bool,
    /// `T` in a level whose layer 2 has no objects, which Lunar Magic
    /// sets on some: layer 2's screens then start where a level with
    /// objects has them, which is where a tide's rows are filled
    /// (docs/lunar-magic-install.md, "Layer 3 settings").
    pub split: bool,
}

impl LevelSize {
    /// From a table byte; `T` is dropped (see [`LevelSize::split`]).
    pub fn from_byte(byte: u8) -> Self {
        Self {
            mode: byte & MODE,
            bottom_row: byte & BOTTOM_ROW != 0,
            split: false,
        }
    }

    /// Whether a table byte has `T`.
    pub fn byte_splits(byte: u8) -> bool {
        byte & SPLIT != 0
    }

    /// The table byte, with `T` when layer 2 has objects or the size says.
    pub fn to_byte(self, layer2_objects: bool) -> u8 {
        (self.mode & MODE)
            | if self.bottom_row { BOTTOM_ROW } else { 0 }
            | if layer2_objects || self.split {
                SPLIT
            } else {
                0
            }
    }

    /// The size with a height of `rows` rows, if one has it.
    pub fn with_rows(rows: usize) -> Option<u8> {
        SIZES
            .iter()
            .position(|&(height, _)| height as usize == rows * 16)
            .map(|mode| mode as u8)
    }

    /// Whether this is the game's own size: 27 rows, the camera stopping
    /// where the game's does.
    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    pub fn height(self) -> u16 {
        SIZES[(self.mode & MODE) as usize].0
    }

    pub fn rows(self) -> usize {
        self.height() as usize / 16
    }

    /// The screen layer 2's objects start at, when the level has them.
    pub fn layer2_screen(self) -> usize {
        layer2_screen(self.height())
    }

    /// Screens of this height that fit the level's tile planes.
    pub fn screens(self) -> usize {
        SIZES[(self.mode & MODE) as usize].1
    }
}

/// Where a ROM's size table is: `$240` bytes before the code the `JSL` at
/// [`HOOK`] calls. `None` without one. The target's bank is kept as it is:
/// on an SA-1 cartridge banks `$80`-`$BF` are not a mirror of `$00`-`$3F`
/// but the ROM's third and fourth megabytes, where Lunar Magic puts its
/// code in a ROM that large (QLDC 2022 `26_Heraga`: `JSL $80DEF1`).
pub fn table(rom: &Rom) -> Option<SnesAddr> {
    if rom.read_u8(HOOK).ok()? != 0x22 {
        return None;
    }
    let target = rom.read_u24(HOOK.add(1)).ok()?;
    let at = SnesAddr::new(target.checked_sub(TABLE_OFFSET)?);
    rom.read(at, 0x200).ok().map(|_| at)
}

/// A level's size byte, `None` for a ROM without the table.
pub fn read_byte(rom: &Rom, level: u16) -> Option<u8> {
    let at = table(rom)?;
    rom.read_u8(at.add(level as u32 & 0x1FF)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_byte_splits_into_its_parts() {
        let size = LevelSize::from_byte(0xDB);
        assert_eq!(
            size,
            LevelSize {
                mode: 0x1B,
                bottom_row: true,
                split: false,
            }
        );
        assert_eq!(size.height(), 0x1C00);
        assert_eq!(size.rows(), 448);
        assert_eq!(size.screens(), 2);
        assert_eq!(size.to_byte(true), 0xDB);
        assert_eq!(size.to_byte(false), 0x5B);
        assert!(LevelSize::from_byte(0x80).is_default());
        assert_eq!(LevelSize::from_byte(0x20), LevelSize::default());
    }

    /// A 3 MiB image with the hook at `$05DA8A` calling `target`, and a
    /// size table before it whose entry for level `105` is `$05`.
    fn with_table(sa1: bool, target: u32) -> Rom {
        let mut data = vec![0u8; 0x30_0000];
        data[0x7FD5] = if sa1 { 0x23 } else { 0x20 };
        let mut rom = Rom::from_bytes(data).unwrap();
        let [low, high, bank, _] = target.to_le_bytes();
        rom.write(HOOK, &[0x22, low, high, bank]).unwrap();
        rom.write_u8(SnesAddr::new(target - TABLE_OFFSET + 0x105), 0x05)
            .unwrap();
        rom
    }

    #[test]
    fn the_table_is_found_in_the_bank_the_hook_names() {
        // SA-1: $80DEF1 is in the ROM's third megabyte, not bank $00
        // (QLDC 2022 26_Heraga). LoROM: $80 and up mirror $00 and up.
        let rom = with_table(true, 0x80_DEF1);
        assert_eq!(table(&rom), Some(SnesAddr::new(0x80_DCB1)));
        assert_eq!(read_byte(&rom, 0x105), Some(0x05));
        assert_eq!(read_byte(&rom, 0x106), Some(0x00));
        assert_eq!(rom.read_u8(SnesAddr::new(0x00_DDB6)).unwrap(), 0x00);
        let rom = with_table(false, 0x90_DEF1);
        assert_eq!(read_byte(&rom, 0x105), Some(0x05));
        assert_eq!(rom.read_u8(SnesAddr::new(0x10_DDB6)).unwrap(), 0x05);
    }

    #[test]
    fn every_height_names_one_size() {
        for (mode, &(height, _)) in SIZES.iter().enumerate() {
            assert_eq!(height % 16, 0);
            assert_eq!(LevelSize::with_rows(height as usize / 16), Some(mode as u8));
        }
        assert_eq!(LevelSize::with_rows(30), None);
    }

    #[test]
    fn layer_2_starts_nearest_the_middle() {
        // Observed on Lunar Magic 3.70's code for each size with T; sizes
        // 0E and 10 did not load with the level tried.
        let seen = [
            16, 16, 15, 14, 13, 12, 12, 11, 11, 10, 10, 9, 9, 8, 0, 7, 0, 6, 6, 5, 5, 4, 3, 3, 3,
            2, 2, 1, 0, 16, 16, 16,
        ];
        for (mode, &screen) in seen.iter().enumerate() {
            if mode == 0x0E || mode == 0x10 {
                continue;
            }
            let size = LevelSize {
                mode: mode as u8,
                bottom_row: false,
                split: false,
            };
            assert_eq!(size.layer2_screen(), screen, "size {mode:02X}");
        }
    }
}
