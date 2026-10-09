//! Lunar Magic's graphics formats past the game's: GFX files `00`-`33`
//! stored as 4bpp, ExGFX files `80`-`FFF`, each level's graphics list, and
//! the older lists objects `24` and `25` name (docs/lunar-magic-install.md,
//! "Graphics"). Import reads and build writes them through this module;
//! Kobo's code for them is `asm/lunar-magic/graphics.asm`.

use crate::addr::SnesAddr;
use crate::gfx::{self, Compression, GfxError};
use crate::rom::{Rom, RomError};

/// `$EA` when the ROM's GFX files are stored as 4bpp.
pub const FOUR_BPP_CHECK: SnesAddr = SnesAddr::new(0x00AAD8);
/// `$EA` when the ROM has ExGFX, with a `JSL` at [`HEADER_HOOK`].
pub const EXGFX_CHECK: SnesAddr = SnesAddr::new(0x00AA47);
/// A `JSL` (`$22`) here when the ROM has per-level lists.
pub const HEADER_HOOK: SnesAddr = SnesAddr::new(0x0583B8);
/// A `JSL` here when the ROM has Lunar Magic's (or Kobo's) layer 3
/// settings code; with anything else, a save installs Lunar Magic's.
pub const LAYER3_CHECK: SnesAddr = SnesAddr::new(0x00A01F);
/// `"LM"` here makes Lunar Magic's save keep the lists as they are.
pub const LISTS_MARKER: SnesAddr = SnesAddr::new(0x0FF15C);
/// The byte that marks a patched site: `NOP` and `JSL`.
pub const NOP: u8 = 0xEA;
pub const JSL: u8 = 0x22;

/// The operand of the game's `CPY #$08` in `UploadGFXFile` (`$00AA8C`),
/// which with the `CPY #$1E` after it uploads `GFX08` on the overworld and
/// `GFX1E` everywhere with the tiles' fourth plane set (smw.md). Lunar
/// Magic's 4bpp install makes it [`UPPER_COLOURS_OFF`], a file never
/// uploaded there, and its GFX export checks it: with the game's `$08` it
/// exports `GFX08`'s and `GFX1E`'s upper-colour tiles and `GFX17`'s berry
/// with the plane set ([`upgraded_4bpp`]), as older versions' installs
/// left the byte (bisected 2026-10-09, docs/lunar-magic-install.md).
pub const UPPER_COLOURS: SnesAddr = SnesAddr::new(0x00_AA8D);
pub const UPPER_COLOURS_OFF: u8 = 0x32;

/// Whether a 4bpp ROM's upload still draws the upper-colour tiles so, as
/// Lunar Magic tells it ([`UPPER_COLOURS`]).
pub fn uploads_upper_colours(rom: &Rom) -> bool {
    rom.read_u8(UPPER_COLOURS)
        .is_ok_and(|b| b != UPPER_COLOURS_OFF)
}

/// The 3-byte pointer to level `000`'s list.
pub const LIST_POINTER: SnesAddr = SnesAddr::new(0x0FF7FF);
/// The 3-byte pointer to the block of ExGFX `100`-`FFF` pointers, with the
/// lists after them, and a second copy of it.
pub const BLOCK_POINTER: SnesAddr = SnesAddr::new(0x0FF873);
pub const BLOCK_POINTER_COPY: SnesAddr = SnesAddr::new(0x0FF937);
/// ExGFX `80`-`FF`, 3-byte pointers.
pub const EXGFX_80: SnesAddr = SnesAddr::new(0x0FF600);
/// The older lists objects `24` and `25` name, 4 bytes each.
pub const OLD_LISTS: SnesAddr = SnesAddr::new(0x0FF200);
pub const OLD_LIST_COUNT: usize = 0x100;

/// The block's size: [`POINTERS_LEN`] bytes of ExGFX pointers, then
/// [`LIST_COUNT`] lists.
pub const BLOCK_LEN: usize = 0x6E00;
pub const POINTERS_LEN: usize = 0x2D00;
/// Lists: one per level, one per overworld submap (`200`-`206`), and one
/// more, as Lunar Magic 3.70 writes them.
pub const LIST_COUNT: u16 = 0x208;
/// The first submap's list.
pub const SUBMAP_LISTS: u16 = 0x200;

/// ExGFX file numbers.
pub const EXGFX_FIRST: u16 = 0x80;
pub const EXGFX_LAST: u16 = 0xFFF;
/// A slot's "no file" number.
pub const NO_FILE: u16 = 0x7F;

/// A slot word older versions of Lunar Magic write for an empty slot.
pub const EMPTY: u16 = 0xFFFF;

/// A pointer that names no file.
const NONE: u32 = 0xFF_FFFF;

/// A level's graphics list: 16 words in Lunar Magic's slot order
/// ([`GraphicsList::SLOTS`]), as stored. A word's low 12 bits are a file,
/// its high nibble settings.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GraphicsList(pub [u16; 16]);

/// Slots by index in a [`GraphicsList`].
pub mod slot {
    pub const AN2: usize = 0;
    pub const LT3: usize = 1;
    pub const BG3: usize = 2;
    pub const BG2: usize = 3;
    pub const FG3: usize = 4;
    pub const BG1: usize = 5;
    pub const FG2: usize = 6;
    pub const FG1: usize = 7;
    pub const SP4: usize = 8;
    pub const SP3: usize = 9;
    pub const SP2: usize = 10;
    pub const SP1: usize = 11;
    pub const LG4: usize = 12;
    pub const LG3: usize = 13;
    pub const LG2: usize = 14;
    pub const LG1: usize = 15;
}

/// AN2's bits: the list's files replace the tilesets' (`G`, "Super GFX
/// Bypass"), layer 3's files (`3`), and layer 3's tilemap (`T`).
pub const BYPASS: u16 = 0x8000;
pub const LAYER3_FILES: u16 = 0x4000;
pub const LAYER3_TILEMAP: u16 = 0x2000;

/// LT3's high nibble `DDFF`: the bytes of the tilemap `FF` loads (3, which
/// Lunar Magic's dialog does not offer, is not among them), and the VRAM
/// word address `DD` puts it at (docs/lunar-magic-install.md, "Layer 3 in
/// the lists").
pub const TILEMAP_SIZES: [u16; 3] = [0x2000, 0x1000, 0x800];
pub const TILEMAP_VRAM: [u16; 4] = [0x50A0, 0x5000, 0x5080, 0x5800];

/// Slots whose high nibble holds Lunar Magic's layer 3 settings
/// ([`Layer3Settings`]).
pub const SETTINGS_SLOTS: [usize; 8] = [
    slot::BG3,
    slot::SP1,
    slot::SP2,
    slot::SP3,
    slot::LG1,
    slot::LG2,
    slot::LG3,
    slot::LG4,
];

/// AN2's bit 12, whose effect was not found.
pub const UNKNOWN_BIT: u16 = 0x1000;

/// Lunar Magic's layer 3 settings ("Change Layer 3 Settings"), from the
/// high nibbles of a list's slots (the community's level format:
/// BG3 `AAAA`, SP1 `SCXX`, SP2 `HVYY`, SP3 `yyyy`, LG1 `vvvv`, LG2 `hhhh`,
/// LG3 `YYYY`, LG4 `yOIB`). Kobo's code for them is
/// `asm/lunar-magic/layer3.asm`; what each does is in
/// docs/lunar-magic-install.md, "Layer 3 settings".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Layer3Settings {
    /// `B`: layer 3 scrolls and is placed by these settings; without it
    /// only [`Layer3Settings::sprites_air`] does anything.
    pub advanced: bool,
    /// `Hhhhh` and `Vvvvv`: how layer 3 scrolls on each axis
    /// ([`crate::names::layer3_scroll`]).
    pub horizontal: u8,
    pub vertical: u8,
    /// `XX`: where layer 3 starts, 0, 4, 8, or 16 tiles of 16 pixels.
    pub x: u8,
    /// `YYyyyyYYYYy`: where layer 3 starts, or its offset from layer 1,
    /// in tiles of 16 pixels, -400 to 3FF.
    pub y: i16,
    /// `C`: layer 3 takes part in colour math.
    pub cgadsub: bool,
    /// `S`: layer 3 moves to the subscreen.
    pub subscreen: bool,
    /// `I`: layer 3's position is taken a frame late, as layer 1's is.
    pub sync_fix: bool,
    /// `O`: a sprite beyond the level's edges touches air, not water.
    pub sprites_air: bool,
    /// `AAAA`: what tides act like; no effect was seen.
    pub tides_act_as: u8,
    /// AN2's bit 12.
    pub unknown: bool,
}

impl Layer3Settings {
    /// The nibbles, in [`SETTINGS_SLOTS`] order, and AN2's bit 12.
    fn nibbles(&self) -> ([u8; 8], bool) {
        let y = self.y as u16 & 0x7FF;
        let h = self.horizontal & 0x1F;
        let v = self.vertical & 0x1F;
        (
            [
                self.tides_act_as & 0x0F,
                (self.subscreen as u8) << 3 | (self.cgadsub as u8) << 2 | self.x & 3,
                (h >> 4) << 3 | (v >> 4) << 2 | (y >> 9) as u8,
                (y >> 5) as u8 & 0x0F,
                v & 0x0F,
                h & 0x0F,
                (y >> 1) as u8 & 0x0F,
                (y as u8 & 1) << 3
                    | (self.sprites_air as u8) << 2
                    | (self.sync_fix as u8) << 1
                    | self.advanced as u8,
            ],
            self.unknown,
        )
    }

    fn from_nibbles(n: [u8; 8], unknown: bool) -> Self {
        let [bg3, sp1, sp2, sp3, lg1, lg2, lg3, lg4] = n;
        let y = (sp2 as u16 & 3) << 9 | (sp3 as u16) << 5 | (lg3 as u16) << 1 | (lg4 as u16) >> 3;
        Self {
            advanced: lg4 & 1 != 0,
            horizontal: (sp2 >> 3) << 4 | lg2,
            vertical: (sp2 >> 2 & 1) << 4 | lg1,
            x: sp1 & 3,
            y: ((y << 5) as i16) >> 5,
            cgadsub: sp1 & 4 != 0,
            subscreen: sp1 & 8 != 0,
            sync_fix: lg4 & 2 != 0,
            sprites_air: lg4 & 4 != 0,
            tides_act_as: bg3,
            unknown,
        }
    }
}

impl GraphicsList {
    pub const SLOTS: [&'static str; 16] = [
        "AN2", "LT3", "BG3", "BG2", "FG3", "BG1", "FG2", "FG1", "SP4", "SP3", "SP2", "SP1", "LG4",
        "LG3", "LG2", "LG1",
    ];

    /// A level's list with nothing set, as Lunar Magic 3.70 writes it for
    /// every level (and exports for the vanilla ROM's).
    pub const DEFAULT: Self = Self([
        0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0xFFFF, 0x7F, 0x7F, 0x7F, 0x2B, 0x2A, 0x29,
        0x28,
    ]);

    /// An overworld submap's, as Lunar Magic 3.70 writes them: the
    /// overworld's own files.
    pub const SUBMAP_DEFAULT: Self = Self([
        0x14, 0x7F, 0x7F, 0x7F, 0x1E, 0x08, 0x1D, 0x1C, 0x1D, 0x1C, 0x0F, 0x10, 0x2B, 0x2A, 0x29,
        0x28,
    ]);

    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut words = [0; 16];
        for (word, b) in words.iter_mut().zip(bytes.as_chunks::<2>().0) {
            *word = u16::from_le_bytes(*b);
        }
        Self(words)
    }

    pub fn to_bytes(self) -> [u8; 32] {
        let mut out = [0; 32];
        for (b, word) in out.as_chunks_mut::<2>().0.iter_mut().zip(self.0) {
            *b = word.to_le_bytes();
        }
        out
    }

    /// The slot's file.
    pub fn file(&self, slot: usize) -> u16 {
        self.0[slot] & 0x0FFF
    }

    pub fn bypass(&self) -> bool {
        self.0[slot::AN2] & BYPASS != 0
    }

    pub fn layer3_files(&self) -> bool {
        self.0[slot::AN2] & LAYER3_FILES != 0
    }

    pub fn layer3_tilemap(&self) -> bool {
        self.0[slot::AN2] & LAYER3_TILEMAP != 0
    }

    /// LT3's high nibble: where layer 3's tilemap goes (bits 2-3) and how
    /// much of it (bits 0-1).
    pub fn tilemap_settings(&self) -> u8 {
        (self.0[slot::LT3] >> 12) as u8
    }

    /// The layer 3 settings the level's load reads: a slot of `$FFFF`
    /// (an empty slot in older lists) gives its nibble as `F`, as it does
    /// to Lunar Magic's code and Kobo's.
    pub fn layer3(&self) -> Layer3Settings {
        let nibbles = SETTINGS_SLOTS.map(|s| (self.0[s] >> 12) as u8);
        Layer3Settings::from_nibbles(nibbles, self.0[slot::AN2] & UNKNOWN_BIT != 0)
    }

    /// Whether the list has layer 3 settings of its own: a nibble set in
    /// a slot that is not `$FFFF`, or AN2's bit 12.
    pub fn has_layer3(&self) -> bool {
        SETTINGS_SLOTS
            .iter()
            .any(|&s| self.0[s] != EMPTY && self.0[s] >> 12 != 0)
            || (self.0[slot::AN2] != EMPTY && self.0[slot::AN2] & UNKNOWN_BIT != 0)
    }

    /// The list without layer 3 settings of its own: the nibbles of the
    /// slots that are not `$FFFF` cleared.
    pub fn without_layer3(mut self) -> Self {
        for s in SETTINGS_SLOTS {
            if self.0[s] != EMPTY {
                self.0[s] &= 0x0FFF;
            }
        }
        if self.0[slot::AN2] != EMPTY {
            self.0[slot::AN2] &= !UNKNOWN_BIT;
        }
        self
    }

    /// Puts `settings` in the list's nibbles. A slot of `$FFFF` keeps its
    /// word, so the settings must have `F` there: `Err` names the slot
    /// when they do not.
    pub fn set_layer3(&mut self, settings: &Layer3Settings) -> Result<(), &'static str> {
        let (nibbles, unknown) = settings.nibbles();
        for (&s, n) in SETTINGS_SLOTS.iter().zip(nibbles) {
            if self.0[s] == EMPTY {
                if n != 0x0F {
                    return Err(Self::SLOTS[s]);
                }
            } else {
                self.0[s] = self.0[s] & 0x0FFF | (n as u16) << 12;
            }
        }
        if self.0[slot::AN2] == EMPTY {
            if !unknown {
                return Err(Self::SLOTS[slot::AN2]);
            }
        } else {
            self.0[slot::AN2] = self.0[slot::AN2] & !UNKNOWN_BIT | (unknown as u16) << 12;
        }
        Ok(())
    }

    /// Whether the list changes what a level loads: without `G`, `3`, `T`,
    /// or a setting, its files are never read.
    pub fn is_used(&self) -> bool {
        self.0[slot::AN2] & 0xF000 != 0 || self.has_layer3()
    }

    /// The files the level loads through the list: the slots its bits turn
    /// on, `7F` aside.
    pub fn files(&self) -> Vec<(usize, u16)> {
        let mut out = Vec::new();
        for (s, &word) in self.0.iter().enumerate() {
            let on = match s {
                slot::LT3 => self.layer3_tilemap(),
                slot::LG1 | slot::LG2 | slot::LG3 | slot::LG4 => self.layer3_files(),
                _ => self.bypass(),
            };
            if on && word != EMPTY && word & 0x0FFF != NO_FILE {
                out.push((s, word & 0x0FFF));
            }
        }
        out
    }
}

/// The game's layer 3 settings, three a object tileset
/// (`Layer3TilemapSettings`), which a level's layer 3 setting (1 to 3)
/// picks from: 1 and 2 are tides.
pub const LAYER3_TABLE: SnesAddr = SnesAddr::new(0x009F88);

/// Whether a level of this object tileset and layer 3 setting (the
/// secondary header's, 0 to 3) has a tide, by the ROM's table.
pub fn has_tide(rom: &Rom, tileset: u8, layer3: u8) -> Result<bool, RomError> {
    if layer3 == 0 || tileset > 0x0F {
        return Ok(false);
    }
    let setting = rom.read_u8(LAYER3_TABLE.add(tileset as u32 * 3 + layer3 as u32 - 1))?;
    Ok(matches!(setting, 1 | 2))
}

/// Whether the ROM stores its GFX files as 4bpp (Lunar Magic's check).
pub fn is_4bpp(rom: &Rom) -> bool {
    rom.read_u8(FOUR_BPP_CHECK).is_ok_and(|b| b == NOP)
}

/// Whether the ROM has ExGFX and per-level lists: Lunar Magic's checks,
/// and a list table to read.
pub fn has_exgfx(rom: &Rom) -> bool {
    rom.read_u8(EXGFX_CHECK).is_ok_and(|b| b == NOP)
        && rom.read_u8(HEADER_HOOK).is_ok_and(|b| b == JSL)
        && pointer(rom, LIST_POINTER).ok().flatten().is_some()
}

/// A 3-byte pointer, or `None` for one that names nothing.
fn pointer(rom: &Rom, at: SnesAddr) -> Result<Option<SnesAddr>, RomError> {
    let p = rom.read_u24(at)?;
    Ok((p != NONE && p != 0).then(|| SnesAddr::new(p)))
}

/// The list of level `n` (or submap list, `200` on), if the ROM has lists.
pub fn read_list(rom: &Rom, n: u16) -> Result<Option<GraphicsList>, RomError> {
    if !has_exgfx(rom) || n >= LIST_COUNT {
        return Ok(None);
    }
    // Lunar Magic 2.30 gave submaps their lists (its help's version
    // history); past the levels' lists, an older version's ROM has other
    // data.
    if n >= SUBMAP_LISTS && rom.saved_by_lunar_magic_before((2, 30)) {
        return Ok(None);
    }
    let Some(at) = pointer(rom, LIST_POINTER)? else {
        return Ok(None);
    };
    let bytes = rom.read(at.add(n as u32 * 32), 32)?;
    Ok(Some(GraphicsList::from_bytes(
        bytes.try_into().expect("32 bytes"),
    )))
}

/// Where ExGFX file `file` (`80`-`FFF`) is stored, if the ROM has it.
pub fn exgfx_addr(rom: &Rom, file: u16) -> Result<Option<SnesAddr>, RomError> {
    match file {
        EXGFX_FIRST..0x100 => pointer(rom, EXGFX_80.add((file - EXGFX_FIRST) as u32 * 3)),
        0x100..=EXGFX_LAST => match pointer(rom, BLOCK_POINTER)? {
            Some(block) => pointer(rom, block.add((file - 0x100) as u32 * 3)),
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

/// An ExGFX file read from a ROM.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExGfxData {
    pub data: Vec<u8>,
    pub addr: SnesAddr,
    /// Bytes of compressed data, with the terminator.
    pub compressed_len: usize,
}

/// ExGFX file `file`, decompressed, if the ROM has it.
pub fn read_exgfx(
    rom: &Rom,
    file: u16,
    compression: Compression,
) -> Result<Option<ExGfxData>, GfxError> {
    if !has_exgfx(rom) {
        return Ok(None);
    }
    let Some(addr) = exgfx_addr(rom, file)? else {
        return Ok(None);
    };
    let input = rom.read_tail(addr)?;
    let d = compression
        .decompress(input)
        .map_err(|source| GfxError::ExGfx { file, addr, source })?;
    Ok(Some(ExGfxData {
        data: d.data,
        addr,
        compressed_len: d.consumed,
    }))
}

/// The older lists that name any file: index and its four files, first
/// to fourth (FG1/SP1, FG2/SP2, BG1/SP3, FG3/SP4).
pub fn read_old_lists(rom: &Rom) -> Result<Vec<(u8, [u8; 4])>, RomError> {
    if !has_exgfx(rom) {
        return Ok(Vec::new());
    }
    let table = rom.read(OLD_LISTS, OLD_LIST_COUNT * 4)?;
    Ok(table
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, l)| **l != [0; 4])
        .map(|(i, &[a, b, c, d])| (i as u8, [d, c, b, a]))
        .collect())
}

/// An older list's four bytes as stored.
pub fn old_list_bytes(files: [u8; 4]) -> [u8; 4] {
    let [a, b, c, d] = files;
    [d, c, b, a]
}

/// GFX file `index` as Lunar Magic 3.70 stores it in a 4bpp ROM: a 3bpp
/// file converted as [`gfx::GfxFile::to_lm_export`] does, with `GFX17`'s
/// berry (tiles 0, 1, 16, 17) also using the upper colours, as the game
/// draws it; other files as they are.
pub fn stored_4bpp(file: &gfx::GfxFile) -> Vec<u8> {
    let mut out = file.to_lm_export();
    if file.index == 0x17 && file.bpp() == Some(gfx::Bpp::Three) {
        silhouette(&mut out, &BERRY);
    }
    out
}

/// `GFX17`'s berry, which the game draws in the upper colours.
const BERRY: [usize; 4] = [0, 1, 16, 17];

/// Sets the fourth plane of each of `tiles` to the tile's silhouette.
fn silhouette(data: &mut [u8], tiles: &[usize]) {
    for &tile in tiles {
        let Some(t) = data.get_mut(tile * 32..tile * 32 + 32) else {
            continue;
        };
        for y in 0..8 {
            t[16 + 2 * y + 1] = t[2 * y] | t[2 * y + 1] | t[16 + 2 * y];
        }
    }
}

/// A 4bpp GFX file of a ROM whose upload still draws the tiles the game
/// draws in the upper colours so ([`UPPER_COLOURS`] holds the game's
/// byte), as Lunar Magic 3.70 exports it and so stores it again: those
/// tiles' fourth plane set as [`stored_4bpp`] sets it (found with Kaizo
/// Mario and Kaizo Mario World 3 by its `-ExportGFX` and `-ImportGFX`).
pub fn upgraded_4bpp(file: &gfx::GfxFile) -> Vec<u8> {
    let mut out = file.data.clone();
    if file.bpp() == Some(gfx::Bpp::Four) {
        silhouette(
            &mut out,
            &gfx::upper_palette_tiles(file.index, file.tile_count()),
        );
        if file.index == 0x17 {
            silhouette(&mut out, &BERRY);
        }
    }
    out
}

/// Whether a GFX file the game keeps as 3bpp is stored as 4bpp in a 4bpp
/// ROM: every 3bpp file (all but `27`, the Mode 7 file, and the 2bpp ones).
pub fn converts(format: gfx::GfxFormat) -> bool {
    format == gfx::GfxFormat::Planar(gfx::Bpp::Three)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_bits() {
        let mut list = GraphicsList::DEFAULT;
        assert!(!list.is_used());
        assert!(list.files().is_empty());
        list.0[slot::AN2] = 0x8000 | 0x7F;
        list.0[slot::FG1] = 0x100;
        assert!(list.is_used() && list.bypass() && !list.layer3_files());
        assert_eq!(list.files(), [(slot::FG1, 0x100)]);
        list.0[slot::AN2] |= LAYER3_FILES;
        assert_eq!(list.files().len(), 5);
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::LG4] |= 0x1000;
        assert!(list.is_used());
        assert!(list.has_layer3() && list.layer3().advanced);
        assert_eq!(GraphicsList::from_bytes(&list.to_bytes()), list);
        assert_eq!(old_list_bytes([1, 2, 3, 4]), [4, 3, 2, 1]);
    }

    #[test]
    fn layer3_nibbles() {
        // Kaizo Kindergarten's level 15A: B, S, I, Y = 4; and 01A's X,
        // Y, and scrolling.
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::SP1] |= 0x8000;
        list.0[slot::LG4] |= 0x3000;
        list.0[slot::LG3] |= 0x2000;
        let s = list.layer3();
        assert!(s.advanced && s.subscreen && s.sync_fix && !s.cgadsub);
        assert_eq!((s.y, s.x, s.horizontal, s.vertical), (4, 0, 0, 0));
        let mut again = GraphicsList::DEFAULT;
        again.set_layer3(&s).unwrap();
        assert_eq!(again, list);
        // Every nibble back as it was, the top bits of Y signed.
        let mut list = GraphicsList::DEFAULT;
        for (i, &sl) in SETTINGS_SLOTS.iter().enumerate() {
            list.0[sl] |= (((i * 5 + 3) & 0x0F) as u16) << 12;
        }
        let s = list.layer3();
        let mut again = GraphicsList::DEFAULT;
        again.set_layer3(&s).unwrap();
        assert_eq!(again, list);
        // Y's top bit (SP2 bit 13) is its sign.
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::SP2] |= 0x2000;
        assert_eq!(list.layer3().y, -0x400);
        // An empty slot reads as F and takes only F.
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::SP2] = EMPTY;
        assert!(!list.has_layer3());
        let s = list.layer3();
        assert_eq!((s.horizontal, s.vertical), (0x10, 0x10));
        assert!(list.clone().set_layer3(&s).is_ok());
        assert_eq!(list.set_layer3(&Layer3Settings::default()), Err("SP2"));
    }

    #[test]
    fn an_older_versions_4bpp_files_take_the_upper_colours_as_3_70_stores_them() {
        let file = |index: u8| gfx::GfxFile {
            index,
            format: gfx::GfxFormat::Planar(gfx::Bpp::Four),
            data: (0..128 * 32).map(|i| (i % 7) as u8).collect(),
            addr: SnesAddr::new(0x10_8000),
            compressed_len: 0,
            compression: gfx::Compression::Lz2,
        };
        let plane3 = |data: &[u8], tile: usize| -> Vec<u8> {
            (0..8).map(|y| data[tile * 32 + 16 + 2 * y + 1]).collect()
        };
        let silhouette = |data: &[u8], tile: usize| -> Vec<u8> {
            let t = &data[tile * 32..];
            (0..8)
                .map(|y| t[2 * y] | t[2 * y + 1] | t[16 + 2 * y])
                .collect()
        };
        let berry = file(0x17);
        let up = upgraded_4bpp(&berry);
        assert_eq!(plane3(&up, 16), silhouette(&berry.data, 16));
        assert_eq!(up[2 * 32..16 * 32], berry.data[2 * 32..16 * 32]);
        assert_eq!(up[18 * 32..], berry.data[18 * 32..]);
        let whole = upgraded_4bpp(&file(0x1E));
        assert!((0..128).all(|t| plane3(&whole, t) == silhouette(&whole, t)));
        assert_eq!(upgraded_4bpp(&file(0x00)), file(0x00).data);
    }
}
