//! SNES colours and the level palette the game assembles at load time.
//!
//! CGRAM holds 256 colours in 16 rows of 16. Rows 0 to 7 are used by
//! layers, rows 8 to 15 by sprites. Colour 0 of every row is transparent.
//! [`vanilla_level_palette`] reproduces what SMW's `LoadPalette` routine
//! writes for a level's header settings; it does not cover the extra
//! colours the game loads later for the player, Yoshi, or the status bar.

use crate::addr::SnesAddr;
use crate::rom::{Rom, RomError};

/// A 15-bit SNES colour, `0bbbbbgg gggrrrrr`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Color15(pub u16);

impl Color15 {
    pub const fn from_rgb5(r: u8, g: u8, b: u8) -> Self {
        Self(((b as u16 & 0x1F) << 10) | ((g as u16 & 0x1F) << 5) | (r as u16 & 0x1F))
    }

    pub const fn r(self) -> u8 {
        (self.0 & 0x1F) as u8
    }

    pub const fn g(self) -> u8 {
        ((self.0 >> 5) & 0x1F) as u8
    }

    pub const fn b(self) -> u8 {
        ((self.0 >> 10) & 0x1F) as u8
    }

    /// Expands each 5-bit channel to 8 bits by replicating the top bits.
    pub const fn to_rgb8(self) -> [u8; 3] {
        const fn expand(v: u8) -> u8 {
            (v << 3) | (v >> 2)
        }
        [expand(self.r()), expand(self.g()), expand(self.b())]
    }

    pub const fn to_le_bytes(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }
}

/// A full 256-entry CGRAM palette, indexed `row * 16 + column`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Palette {
    pub colors: [Color15; 256],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            colors: [Color15(0); 256],
        }
    }
}

impl Palette {
    pub fn get(&self, row: usize, col: usize) -> Color15 {
        self.colors[row * 16 + col]
    }

    pub fn set(&mut self, row: usize, col: usize, color: Color15) {
        self.colors[row * 16 + col] = color;
    }

    /// One palette row as 8-bit RGB, ready for image output.
    pub fn row_rgb8(&self, row: usize) -> [[u8; 3]; 16] {
        std::array::from_fn(|col| self.get(row, col).to_rgb8())
    }

    /// The palette in CGRAM byte order.
    pub fn to_cgram_bytes(&self) -> Vec<u8> {
        self.colors.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    /// A palette from 512 bytes of CGRAM.
    pub fn from_cgram(bytes: &[u8]) -> Self {
        let mut pal = Self::default();
        for (i, c) in pal.colors.iter_mut().enumerate() {
            if 2 * i + 1 < bytes.len() {
                *c = Color15(u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]));
            }
        }
        pal
    }
}

/// The header fields that select a level's palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LevelPaletteSelect {
    /// Foreground palette, 0 to 7. Fills rows 2 and 3, colours 2 to 7.
    pub fg: u8,
    /// Background palette, 0 to 7. Fills rows 0 and 1, colours 2 to 7.
    pub bg: u8,
    /// Sprite palette, 0 to 7. Fills rows 14 and 15, colours 2 to 7.
    pub sprite: u8,
    /// Back area colour, 0 to 7.
    pub back_area: u8,
}

/// Vanilla palette tables in bank `$00`.
pub mod tables {
    use crate::addr::SnesAddr;

    /// 8 back area colours.
    pub const BACK_AREA_COLORS: SnesAddr = SnesAddr::new(0x00B0A0);
    /// 8 background palettes of 12 colours (rows 0 to 1, colours 2 to 7).
    pub const BACKGROUND_PALETTES: SnesAddr = SnesAddr::new(0x00B0B0);
    /// Rows 0 to 1, colours 8 to 15 (layer 3 and status bar).
    pub const STATUS_BAR_COLORS: SnesAddr = SnesAddr::new(0x00B170);
    /// 8 foreground palettes of 12 colours (rows 2 to 3, colours 2 to 7).
    pub const FOREGROUND_PALETTES: SnesAddr = SnesAddr::new(0x00B190);
    /// Rows 4 to 13, colours 2 to 7.
    pub const STANDARD_COLORS: SnesAddr = SnesAddr::new(0x00B250);
    /// 8 sprite palettes of 12 colours (rows 14 to 15, colours 2 to 7).
    pub const SPRITE_COLORS: SnesAddr = SnesAddr::new(0x00B318);
    /// Rows 2 to 4 and 9 to 11, colours 9 to 15.
    pub const BERRY_COLORS: SnesAddr = SnesAddr::new(0x00B674);

    /// Bytes per FG, BG, or sprite palette entry.
    pub const PALETTE_STRIDE: u32 = 0x18;
}

/// Colour 1 of layer rows 0 to 7.
pub const LAYER_COLOR_1: Color15 = Color15(0x7FDD);
/// Colour 1 of sprite rows 8 to 15.
pub const SPRITE_COLOR_1: Color15 = Color15(0x7FFF);

fn read_color(rom: &Rom, addr: SnesAddr) -> Result<Color15, RomError> {
    Ok(Color15(rom.read_u16(addr)?))
}

/// Copies `per_row` consecutive colours into each of `rows` rows starting
/// at (`row`, `col`), reading the source sequentially. Mirrors the game's
/// `LoadColors` helper.
fn load_colors(
    rom: &Rom,
    pal: &mut Palette,
    src: SnesAddr,
    row: usize,
    col: usize,
    per_row: usize,
    rows: usize,
) -> Result<(), RomError> {
    let mut addr = src;
    for r in row..row + rows {
        for c in col..col + per_row {
            pal.set(r, c, read_color(rom, addr)?);
            addr = addr.add(2);
        }
    }
    Ok(())
}

/// The back area colour for a selection.
pub fn vanilla_back_area_color(rom: &Rom, back_area: u8) -> Result<Color15, RomError> {
    read_color(
        rom,
        tables::BACK_AREA_COLORS.add(2 * (back_area & 0x0F) as u32),
    )
}

/// The palette a level of `rom` with header `header` loads, as a palette
/// of its own would hold it: the game's assembly and back area colour, to
/// start one from.
pub fn game_palette(
    rom: &Rom,
    header: &crate::level::PrimaryHeader,
) -> Result<CustomPalette, RomError> {
    Ok(CustomPalette {
        back_area: vanilla_back_area_color(rom, header.back_area)?,
        palette: vanilla_level_palette(rom, header.palette_select())?,
    })
}

/// Assembles the palette SMW's `LoadPalette` produces for a level.
///
/// Entries the routine does not touch are left at colour 0 (black).
pub fn vanilla_level_palette(rom: &Rom, sel: LevelPaletteSelect) -> Result<Palette, RomError> {
    use tables::*;
    let mut pal = Palette::default();
    for row in 0..8 {
        pal.set(row, 1, LAYER_COLOR_1);
        pal.set(row + 8, 1, SPRITE_COLOR_1);
    }
    load_colors(rom, &mut pal, STATUS_BAR_COLORS, 0, 8, 8, 2)?;
    load_colors(rom, &mut pal, STANDARD_COLORS, 4, 2, 6, 10)?;
    let entry = |base: SnesAddr, index: u8| base.add(PALETTE_STRIDE * (index & 0x07) as u32);
    load_colors(
        rom,
        &mut pal,
        entry(FOREGROUND_PALETTES, sel.fg),
        2,
        2,
        6,
        2,
    )?;
    load_colors(rom, &mut pal, entry(SPRITE_COLORS, sel.sprite), 14, 2, 6, 2)?;
    load_colors(
        rom,
        &mut pal,
        entry(BACKGROUND_PALETTES, sel.bg),
        0,
        2,
        6,
        2,
    )?;
    load_colors(rom, &mut pal, BERRY_COLORS, 2, 9, 7, 3)?;
    load_colors(rom, &mut pal, BERRY_COLORS, 9, 9, 7, 3)?;
    Ok(pal)
}

/// Lunar Magic's per-level custom palette table: 3-byte pointers at
/// `$0EF600`, one per level. A pointer of `$000000` or `$FFFFFF` means the
/// level uses the vanilla palette assembly. The data is `$202` bytes: the
/// back area colour followed by all 256 CGRAM colours.
pub const LM_LEVEL_PALETTE_PTRS: SnesAddr = SnesAddr::new(0x0EF600);

/// A custom level palette as Lunar Magic stores it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CustomPalette {
    pub back_area: Color15,
    pub palette: Palette,
}

/// The game's call to `CODE_05BE8A` after it assembles a level's palette,
/// which Lunar Magic's palette piece (and Kobo's) replaces with its own.
const PALETTE_HOOK: SnesAddr = SnesAddr::new(0x00A5BF);

/// Whether the ROM has a custom palette piece, Lunar Magic's or Kobo's:
/// the game's call at [`PALETTE_HOOK`] is not there. Only then does
/// [`LM_LEVEL_PALETTE_PTRS`] hold palette pointers.
pub fn has_level_palettes(rom: &Rom) -> bool {
    rom.read(PALETTE_HOOK, 4)
        .is_ok_and(|b| b != [0x22, 0x8A, 0xBE, 0x05])
}

/// Reads a level's Lunar Magic custom palette, if the ROM has one.
pub fn lm_level_palette(rom: &Rom, level: u16) -> Result<Option<CustomPalette>, RomError> {
    if !has_level_palettes(rom) {
        return Ok(None);
    }
    let entry = LM_LEVEL_PALETTE_PTRS.add(3 * level as u32);
    let Ok(ptr) = rom.read_u24(entry) else {
        return Ok(None);
    };
    if ptr == 0 || ptr == 0xFF_FFFF {
        return Ok(None);
    }
    let addr = SnesAddr::new(ptr);
    let Ok(bytes) = rom.read(addr, 0x202) else {
        return Ok(None);
    };
    let back_area = Color15(u16::from_le_bytes([bytes[0], bytes[1]]));
    let mut palette = Palette::default();
    for i in 0..256 {
        palette.colors[i] = Color15(u16::from_le_bytes([bytes[2 + 2 * i], bytes[3 + 2 * i]]));
    }
    Ok(Some(CustomPalette { back_area, palette }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{Mapping, PcAddr};

    #[test]
    fn color_channels_and_expansion() {
        let c = Color15::from_rgb5(31, 0, 16);
        assert_eq!(c.0, 0x401F);
        assert_eq!((c.r(), c.g(), c.b()), (31, 0, 16));
        assert_eq!(c.to_rgb8(), [255, 0, 132]);
        assert_eq!(Color15(0x7FFF).to_rgb8(), [255, 255, 255]);
        assert_eq!(Color15(0).to_rgb8(), [0, 0, 0]);
    }

    /// A LoROM image whose every 16-bit word in bank 0 equals its own file
    /// offset, so a colour tells us exactly where it was read from.
    fn offset_rom() -> Rom {
        let mut data = vec![0u8; 0x8000];
        for i in (0..0x8000).step_by(2) {
            data[i..i + 2].copy_from_slice(&(i as u16).to_le_bytes());
        }
        let h = 0x7FC0;
        data[h + 0x15] = 0x20;
        Rom::from_bytes(data).unwrap()
    }

    fn pc(addr: SnesAddr) -> u16 {
        Mapping::LoRom.snes_to_pc(addr).unwrap().raw() as u16
    }

    #[test]
    fn level_palette_layout() {
        let rom = offset_rom();
        let sel = LevelPaletteSelect {
            fg: 3,
            bg: 5,
            sprite: 6,
            back_area: 2,
        };
        let pal = vanilla_level_palette(&rom, sel).unwrap();
        let at = |a: SnesAddr, n: u32| pc(a.add(2 * n));
        use tables::*;
        // Fixed colour 1.
        assert_eq!(pal.get(0, 1), LAYER_COLOR_1);
        assert_eq!(pal.get(7, 1), LAYER_COLOR_1);
        assert_eq!(pal.get(8, 1), SPRITE_COLOR_1);
        assert_eq!(pal.get(15, 1), SPRITE_COLOR_1);
        // Colour 0 untouched.
        assert_eq!(pal.get(0, 0), Color15(0));
        // Status bar: rows 0-1 cols 8-F, sequential source.
        assert_eq!(pal.get(0, 8).0, at(STATUS_BAR_COLORS, 0));
        assert_eq!(pal.get(1, 15).0, at(STATUS_BAR_COLORS, 15));
        // Standard: rows 4-13 cols 2-7.
        assert_eq!(pal.get(4, 2).0, at(STANDARD_COLORS, 0));
        assert_eq!(pal.get(5, 2).0, at(STANDARD_COLORS, 6));
        assert_eq!(pal.get(13, 7).0, at(STANDARD_COLORS, 59));
        // FG palette 3: rows 2-3 cols 2-7.
        let fg = FOREGROUND_PALETTES.add(3 * PALETTE_STRIDE);
        assert_eq!(pal.get(2, 2).0, at(fg, 0));
        assert_eq!(pal.get(3, 7).0, at(fg, 11));
        // BG palette 5: rows 0-1 cols 2-7.
        let bg = BACKGROUND_PALETTES.add(5 * PALETTE_STRIDE);
        assert_eq!(pal.get(0, 2).0, at(bg, 0));
        assert_eq!(pal.get(1, 7).0, at(bg, 11));
        // Sprite palette 6: rows 14-15 cols 2-7.
        let sp = SPRITE_COLORS.add(6 * PALETTE_STRIDE);
        assert_eq!(pal.get(14, 2).0, at(sp, 0));
        assert_eq!(pal.get(15, 7).0, at(sp, 11));
        // Berries: rows 2-4 and 9-11, cols 9-15, same source twice.
        assert_eq!(pal.get(2, 9).0, at(BERRY_COLORS, 0));
        assert_eq!(pal.get(4, 15).0, at(BERRY_COLORS, 20));
        assert_eq!(pal.get(9, 9).0, at(BERRY_COLORS, 0));
        assert_eq!(pal.get(11, 15).0, at(BERRY_COLORS, 20));
        // Gaps stay black.
        assert_eq!(pal.get(2, 8), Color15(0));
        assert_eq!(pal.get(5, 9), Color15(0));
        // Back area colour 2.
        assert_eq!(
            vanilla_back_area_color(&rom, 2).unwrap().0,
            at(BACK_AREA_COLORS, 2)
        );
        let _ = PcAddr::new(0);
    }
}
