//! Level lookup and the primary level header.

use thiserror::Error;

pub mod objects;
pub mod size;

use crate::addr::SnesAddr;
use crate::compress::{LzError, rle1};
use crate::palette::LevelPaletteSelect;
use crate::rom::{Rom, RomError};

use objects::{Jumps, Layout, ObjectData, ObjectError};

pub const LEVEL_COUNT: u16 = 0x200;

/// Vanilla level pointer tables. Lunar Magic keeps these in place.
pub mod tables {
    use crate::addr::SnesAddr;

    /// 3-byte layer 1 pointers, one per level.
    pub const LAYER1_PTRS: SnesAddr = SnesAddr::new(0x05E000);
    /// 3-byte layer 2 pointers, one per level.
    pub const LAYER2_PTRS: SnesAddr = SnesAddr::new(0x05E600);
    /// 2-byte sprite pointers into bank `$07`, one per level.
    pub const SPRITE_PTRS: SnesAddr = SnesAddr::new(0x05EC00);
    /// The four secondary header tables, one byte per level each.
    pub const SECONDARY_HEADERS: [SnesAddr; 4] = [
        SnesAddr::new(0x05F000),
        SnesAddr::new(0x05F200),
        SnesAddr::new(0x05F400),
        SnesAddr::new(0x05F600),
    ];
    /// The secondary entrance tables, one byte per entrance each:
    /// `$05F800` the destination level's low byte, `$05FA00` `bbffyyyy`,
    /// `$05FC00` `xxxSSSSS`, `$05FE00` the entrance action in bits 0-2
    /// and Lunar Magic's flags above (bit 3 the destination's bit 8).
    pub const ENTRANCES: [SnesAddr; 4] = [
        SnesAddr::new(0x05F800),
        SnesAddr::new(0x05FA00),
        SnesAddr::new(0x05FC00),
        SnesAddr::new(0x05FE00),
    ];
    /// Secondary entrances in the game's tables.
    pub const ENTRANCE_COUNT: u16 = 0x200;
    /// Secondary entrances a long screen exit can name (13 bits), and so
    /// the most moved tables hold.
    pub const MAX_ENTRANCES: u16 = 0x2000;
    /// Lunar Magic's bank bytes for the sprite pointers, one per level.
    pub const SPRITE_BANKS: SnesAddr = SnesAddr::new(0x0EF100);
    /// Lunar Magic's hook that takes a sprite list's bank from
    /// [`SPRITE_BANKS`] (`JSL` over the game's `LDA #$07 : STA $D0`): its
    /// own check for that table (docs/lunar-magic-install.md).
    pub const SPRITE_BANK_HOOK: SnesAddr = SnesAddr::new(0x05D8F5);
    /// Lunar Magic's exit hook, over the game's `BEQ : LDA #$01` choosing
    /// a destination's bit 8.
    pub const EXIT_HOOK: SnesAddr = SnesAddr::new(0x05D7CE);
    /// Lunar Magic's background hook, which reads [`LEVEL_FLAGS`].
    pub const BACKGROUND_HOOK: SnesAddr = SnesAddr::new(0x05803B);
    /// Lunar Magic's per-level flags, `bbBBVFCT`: `V` a background in the
    /// game's format, `C` Lunar Magic's own, `F` with high bytes.
    pub const LEVEL_FLAGS: SnesAddr = SnesAddr::new(0x0EF310);
    /// Where Lunar Magic 3 puts `JSL` to its expanded level loader.
    pub const TALL_LEVEL_HOOK: SnesAddr = SnesAddr::new(0x05D9A1);
}

/// Which level format a ROM's own code reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelFormat {
    /// Lunar Magic's tables are installed, among them the bank bytes of
    /// the sprite pointers.
    pub lunar_magic: bool,
    /// Whether screen jumps carry a vertical part: only the loader
    /// Lunar Magic 3 installs reads one.
    pub jumps: Jumps,
    /// Secondary entrances keep their destination's bit 8 in bit 3 of
    /// `$05FE00`, which the exit hook at `$05D7CE` reads, Lunar Magic's or
    /// Kobo's; the game's own code takes it from the entrance's number.
    pub entrances: bool,
}

impl LevelFormat {
    /// Read from the ROM: Lunar Magic decides piece by piece what it has
    /// installed, and every Lunar Magic ROM of the corpus, from 1.62 on,
    /// has the sprite bank hook along with the rest of its level tables
    /// (per-level flags, entrances in its format); exactly the 3.x ones
    /// have the tall level hook. Map16 pages past 1 are another piece
    /// ([`crate::map16::pages`]).
    pub fn of(rom: &Rom) -> Self {
        let byte = |addr| rom.read_u8(addr).ok();
        let lunar_magic = byte(tables::SPRITE_BANK_HOOK) == Some(0x22);
        let tall = lunar_magic && byte(tables::TALL_LEVEL_HOOK) == Some(0x22);
        Self {
            lunar_magic,
            jumps: if tall { Jumps::Tall } else { Jumps::Vanilla },
            entrances: byte(tables::EXIT_HOOK) == Some(0x22),
        }
    }
}

#[derive(Debug, Error)]
pub enum LevelError {
    #[error("level {0:03X} is out of range (000 to 1FF)")]
    BadLevel(u16),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("level {level:03X} background: {source}")]
    Background {
        level: u16,
        #[source]
        source: LzError,
    },
    #[error("level {level:03X} layer {layer}: {source}")]
    Objects {
        level: u16,
        layer: u8,
        #[source]
        source: ObjectError,
    },
}

fn check(level: u16) -> Result<(), LevelError> {
    if level < LEVEL_COUNT {
        Ok(())
    } else {
        Err(LevelError::BadLevel(level))
    }
}

/// Where a level's layer 1 data (header plus objects) starts.
pub fn layer1_ptr(rom: &Rom, level: u16) -> Result<SnesAddr, LevelError> {
    check(level)?;
    Ok(rom.read_ptr(tables::LAYER1_PTRS.add(3 * level as u32))?)
}

/// What a level's layer 2 pointer refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Data {
    /// Object data with its own 5-byte header, like layer 1.
    Objects(SnesAddr),
    /// A pre-built background tilemap. The stored bank is `$FF`; the game
    /// substitutes bank `$0C`.
    Tilemap(SnesAddr),
}

pub fn layer2_ptr(rom: &Rom, level: u16) -> Result<Layer2Data, LevelError> {
    check(level)?;
    let raw = rom.read_ptr(tables::LAYER2_PTRS.add(3 * level as u32))?;
    Ok(if raw.bank() == 0xFF {
        Layer2Data::Tilemap(SnesAddr::from_bank_offset(0x0C, raw.offset()))
    } else {
        Layer2Data::Objects(raw)
    })
}

/// Where a level's sprite data (header plus sprites) starts: in bank
/// `$07`, or in the bank Lunar Magic's table gives.
pub fn sprite_ptr(rom: &Rom, level: u16) -> Result<SnesAddr, LevelError> {
    check(level)?;
    let offset = rom.read_u16(tables::SPRITE_PTRS.add(2 * level as u32))?;
    let bank = if LevelFormat::of(rom).lunar_magic {
        rom.read_u8(tables::SPRITE_BANKS.add(level as u32))?
    } else {
        0x07
    };
    Ok(SnesAddr::from_bank_offset(bank, offset))
}

/// What a level mode puts on layer 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer2Kind {
    /// A pre-built background tilemap, which the loader decodes into its
    /// own buffer and the game uploads whole.
    Background,
    /// Objects in the upper part of the tile grid, in horizontal screens.
    /// Modes `$03` and `$04` pair these with a vertical layer 1.
    HorizontalObjects,
    /// Objects in the upper part of the tile grid, in vertical screens.
    VerticalObjects,
    /// Nothing the loader builds: boss arenas draw their own layers, and
    /// the modes the game does not define load nothing.
    None,
}

/// A level mode (`$1925`, five bits of the primary header): the one
/// number that chooses a level's layer 2 and, through the ROM's per-mode
/// tables, its orientation, screen designation, and colour math. Those
/// tables are read by the game's own code; what is stated here is what
/// this library has to know without running it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct LevelMode(pub u8);

impl LevelMode {
    /// What the loader builds for layer 2. Background modes decode the
    /// tilemap (`LoadLevel`); object modes follow the layer 2 upload
    /// dispatch (`CODE_058883`) and the screen pointer tables at `$00BB08`
    /// and `$00BC16`, which choose the layout independently of layer 1's.
    /// Mode `$0F` (the dark rooms sharing the boss arenas' tilemap) has
    /// objects; the arenas themselves (`$09`, `$0B`, `$10`) have neither.
    pub fn layer2(self) -> Layer2Kind {
        match self.0 {
            0x00 | 0x0A | 0x0C | 0x0D | 0x0E | 0x11 | 0x1E => Layer2Kind::Background,
            0x01..=0x04 | 0x0F | 0x1F => Layer2Kind::HorizontalObjects,
            0x05..=0x08 => Layer2Kind::VerticalObjects,
            _ => Layer2Kind::None,
        }
    }

    /// Whether layer 1 is vertical: bit 0 of the game's per-mode table
    /// at `$058417` (`VerticalTable`), which `CODE_0584E3` stores to `$5B`.
    pub fn layer1_vertical(self) -> bool {
        matches!(self.0, 0x03 | 0x04 | 0x07 | 0x08 | 0x0A | 0x0D)
    }

    /// The mode's name, as Kobo writes it after the number: layer 1's
    /// orientation, then what layer 2 is, then what the per-mode tables
    /// do differently (docs/smw.md has them). A "solid" layer 2 is one the
    /// player stands on. `None` past `$1F`.
    pub fn name(self) -> Option<&'static str> {
        LEVEL_MODE_NAMES.get(self.0 as usize).copied()
    }
}

/// See [`LevelMode::name`]; `names` has the rest of the library's names.
const LEVEL_MODE_NAMES: [&str; 0x20] = [
    "Horizontal, background",
    "Horizontal, layer 2",
    "Horizontal, solid layer 2",
    "Vertical, horizontal layer 2",
    "Vertical, horizontal solid layer 2",
    "Horizontal, vertical layer 2",
    "Horizontal, vertical solid layer 2",
    "Vertical, layer 2",
    "Vertical, solid layer 2",
    "Boss: Morton, Roy, Ludwig, Reznor",
    "Vertical, background",
    "Boss: Iggy, Larry",
    "Horizontal, dark background",
    "Vertical, dark background",
    "Horizontal, background, layer 3 in front",
    "Horizontal, layer 2, layer 3 in front",
    "Boss: Bowser",
    "Horizontal, background, spotlight",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Unused",
    "Horizontal, background, translucent layer 1",
    "Horizontal, translucent solid layer 2",
];

impl std::fmt::Display for LevelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${:02X}", self.0)
    }
}

/// The five bytes at the start of a level's layer 1 data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrimaryHeader {
    /// Background palette, 0 to 7.
    pub bg_palette: u8,
    /// Number of screens, 1 to 32.
    pub screens: u8,
    /// Back area colour, 0 to 7.
    pub back_area: u8,
    pub level_mode: LevelMode,
    pub layer3_priority: bool,
    /// Music, 0 to 7.
    pub music: u8,
    /// Sprite GFX set, 0 to 15.
    pub sprite_tileset: u8,
    /// Timer setting, 0 to 3.
    pub time: u8,
    /// Sprite palette, 0 to 7.
    pub sprite_palette: u8,
    /// Foreground palette, 0 to 7.
    pub fg_palette: u8,
    /// Item memory setting, 0 to 3.
    pub item_memory: u8,
    /// Vertical scroll setting, 0 to 3.
    pub vertical_scroll: u8,
    /// Object (FG/BG) tileset, 0 to 15.
    pub object_tileset: u8,
}

impl PrimaryHeader {
    pub fn from_bytes(b: [u8; 5]) -> Self {
        Self {
            bg_palette: b[0] >> 5,
            screens: (b[0] & 0x1F) + 1,
            back_area: b[1] >> 5,
            level_mode: LevelMode(b[1] & 0x1F),
            layer3_priority: b[2] & 0x80 != 0,
            music: (b[2] >> 4) & 0x07,
            sprite_tileset: b[2] & 0x0F,
            time: b[3] >> 6,
            sprite_palette: (b[3] >> 3) & 0x07,
            fg_palette: b[3] & 0x07,
            item_memory: b[4] >> 6,
            vertical_scroll: (b[4] >> 4) & 0x03,
            object_tileset: b[4] & 0x0F,
        }
    }

    pub fn to_bytes(self) -> [u8; 5] {
        [
            (self.bg_palette << 5) | ((self.screens - 1) & 0x1F),
            (self.back_area << 5) | (self.level_mode.0 & 0x1F),
            ((self.layer3_priority as u8) << 7) | (self.music << 4) | (self.sprite_tileset & 0x0F),
            (self.time << 6) | (self.sprite_palette << 3) | (self.fg_palette & 0x07),
            (self.item_memory << 6) | (self.vertical_scroll << 4) | (self.object_tileset & 0x0F),
        ]
    }

    pub fn palette_select(&self) -> LevelPaletteSelect {
        LevelPaletteSelect {
            fg: self.fg_palette,
            bg: self.bg_palette,
            sprite: self.sprite_palette,
            back_area: self.back_area,
        }
    }
}

/// A level's layer 2, as its level mode has it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Layer2 {
    Objects(ObjectData),
    /// A background tilemap at this address.
    Background(SnesAddr),
    /// The level mode loads nothing there.
    None,
}

/// A level's object data, read from the ROM.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelObjects {
    /// The layer 1 data, whose header is the primary header.
    pub layer1: ObjectData,
    pub layer2: Layer2,
}

impl LevelObjects {
    pub fn header(&self) -> PrimaryHeader {
        PrimaryHeader::from_bytes(self.layer1.header)
    }
}

/// The game's layer 3 settings, three for each object tileset
/// (`Layer3TilemapSettings`): what settings 1 to 3 of a level's secondary
/// header do (docs/smw.md, "Layer 3").
pub const LAYER3_SETTINGS: SnesAddr = SnesAddr::new(0x009F88);

/// What layer 3 setting `setting` (0 to 3) does in a level of object
/// tileset `tileset`, by the ROM's table: none, a tide moving or still, or
/// an image fixed or scrolling with the level.
pub fn layer3_setting(rom: &Rom, tileset: u8, setting: u8) -> Result<&'static str, RomError> {
    if setting == 0 {
        return Ok("None");
    }
    let at = u32::from(tileset & 0x0F) * 3 + u32::from((setting - 1) % 3);
    Ok(match rom.read_u8(LAYER3_SETTINGS.add(at))? {
        1 => "Tide, rising and falling",
        2 => "Tide, still",
        0x80 => "Fixed image, a sprite moves it",
        0x81 => "Image scrolling with the level",
        value if value & 0x80 != 0 => "Fixed image",
        _ => "Tide",
    })
}

/// The overworld's level names (US): a word per translevel
/// (`LevelNames`) whose high byte's low seven bits pick a first part
/// (`DATA_049C91`), its bits 4-7 a second (`DATA_049CCF`), and bits 0-3 a
/// third (`DATA_049CED`), each an offset into `LevelNameStrings`, whose
/// characters end at one with bit 7 set. The overworld leaves out a
/// first part that is only its end, and a second that is only a space
/// (`CODE_049D07`).
pub const LEVEL_NAMES: SnesAddr = SnesAddr::new(0x04A0FC);
pub const LEVEL_NAME_FIRST: SnesAddr = SnesAddr::new(0x049C91);
pub const LEVEL_NAME_SECOND: SnesAddr = SnesAddr::new(0x049CCF);
pub const LEVEL_NAME_THIRD: SnesAddr = SnesAddr::new(0x049CED);
pub const LEVEL_NAME_STRINGS: SnesAddr = SnesAddr::new(0x049AC5);

/// The tiles a name shows: the overworld writes no more than `$26` bytes
/// of them (`CODE_049D7F`).
const NAME_TILES: usize = 19;

/// A level's translevel, the number the overworld knows it by: levels
/// `000`-`024` and `101`-`13B` have one, `00`-`5F`.
pub fn translevel(level: u16) -> Option<u8> {
    match level {
        0x000..=0x024 => Some(level as u8),
        0x101..=0x13B => Some((level - 0x101 + 0x25) as u8),
        _ => None,
    }
}

/// A character of the level names, from its tile on layer 3: letters
/// from `00`, digits from `63`, and the wide letters of "YELLOW" and
/// "ILLUSION" the switch palace and forest names use.
fn name_char(tile: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    Some(match tile {
        0x00..=0x19 => LETTERS[usize::from(tile)],
        0x1C => "-",
        0x1F => " ",
        0x32 => " I",
        0x33 | 0x34 => "L",
        0x35 => "U",
        0x36 => "S",
        0x37 => "I",
        0x38 => "YE",
        0x39 | 0x3A => "L",
        0x3B => "O",
        0x3C => "W",
        0x5A => "#",
        0x5D => "'",
        0x63..=0x6C => DIGITS[usize::from(tile - 0x63)],
        _ => return None,
    })
}

/// The name the overworld shows for `level`, in the ROM's tables; `None`
/// for a level without a translevel, an empty name, or one with a tile
/// that is not a known character.
pub fn level_name(rom: &Rom, level: u16) -> Option<String> {
    let t = u32::from(translevel(level)?);
    let word = rom.read_u16(LEVEL_NAMES.add(2 * t)).ok()?;
    // The tiles of a part, as many as are left of the 19 a name shows.
    let mut shown = 0;
    let mut part = |table: SnesAddr, index: u16, skip: &dyn Fn(u8) -> bool| -> Option<String> {
        let at = u32::from(rom.read_u16(table.add(2 * u32::from(index))).ok()?);
        let mut text = String::new();
        if skip(rom.read_u8(LEVEL_NAME_STRINGS.add(at)).ok()?) {
            return Some(text);
        }
        for i in 0..64 {
            let tile = rom.read_u8(LEVEL_NAME_STRINGS.add(at + i)).ok()?;
            if shown < NAME_TILES {
                text += name_char(tile & 0x7F)?;
                shown += 1;
            }
            if tile & 0x80 != 0 {
                break;
            }
        }
        Some(text)
    };
    let mut name = part(LEVEL_NAME_FIRST, word >> 8 & 0x7F, &|t| t & 0x80 != 0)?;
    name += &part(LEVEL_NAME_SECOND, word >> 4 & 0x0F, &|t| t == 0x9F)?;
    name += &part(LEVEL_NAME_THIRD, word & 0x0F, &|_| false)?;
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// The game's layer 2 scroll rates for each of the secondary header's 16
/// settings: horizontal (`DATA_05D720`, into `$1413`) and vertical
/// (`DATA_05D710`, into `$1414`). `UpdateScreenPosition` moves layer 2 by
/// them: 0 not at all, 1 with layer 1, 2 at half its speed, and vertically 3
/// at a 32nd.
pub const LAYER2_SCROLL_X: SnesAddr = SnesAddr::new(0x05D720);
pub const LAYER2_SCROLL_Y: SnesAddr = SnesAddr::new(0x05D710);

/// How layer 2 scroll setting `setting` (0 to 15) moves layer 2, by the
/// ROM's tables.
pub fn layer2_scroll(rom: &Rom, setting: u8) -> Result<&'static str, RomError> {
    let at = u32::from(setting & 0x0F);
    let x = rom.read_u8(LAYER2_SCROLL_X.add(at))?;
    let y = rom.read_u8(LAYER2_SCROLL_Y.add(at))?;
    Ok(match (x, y) {
        (0, 0) => "Fixed",
        (0, 1) => "Fixed across, with layer 1 up and down",
        (0, 2) => "Fixed across, half speed up and down",
        (0, 3) => "Fixed across, a 32nd up and down",
        (1, 0) => "With layer 1 across, fixed up and down",
        (1, 1) => "With layer 1",
        (1, 2) => "With layer 1 across, half speed up and down",
        (1, 3) => "With layer 1 across, a 32nd up and down",
        (2, 0) => "Half speed across, fixed up and down",
        (2, 1) => "Half speed across, with layer 1 up and down",
        (2, 2) => "Half speed",
        (2, 3) => "Half speed across, a 32nd up and down",
        _ => "Other",
    })
}

/// The game's background tilemaps (in bank `$0C`) that `rom`'s levels
/// use, each with the levels that use it, the most used first. A level
/// that does not read is left out.
pub fn game_backgrounds(rom: &Rom) -> Vec<(SnesAddr, Vec<u16>)> {
    let mut found: Vec<(SnesAddr, Vec<u16>)> = Vec::new();
    for level in 0..LEVEL_COUNT {
        let Ok(LevelObjects {
            layer2: Layer2::Background(addr),
            ..
        }) = read_objects(rom, level)
        else {
            continue;
        };
        if addr.bank() != 0x0C {
            continue;
        }
        match found.iter_mut().find(|(a, _)| *a == addr) {
            Some((_, levels)) => levels.push(level),
            None => found.push((addr, vec![level])),
        }
    }
    found.sort_by_key(|(addr, levels)| (std::cmp::Reverse(levels.len()), addr.raw()));
    found
}

/// Reads a level's layer 1 and layer 2 object data. The level mode says
/// what layer 2 is, as it does for the game's loader.
pub fn read_objects(rom: &Rom, level: u16) -> Result<LevelObjects, LevelError> {
    let jumps = LevelFormat::of(rom).jumps;
    let decode = |addr, layout, layer| {
        objects::decode(rom.read_tail(addr)?, layout, jumps).map_err(|source| LevelError::Objects {
            level,
            layer,
            source,
        })
    };
    let layer1 = decode(layer1_ptr(rom, level)?, Layout::Horizontal, 1)?;
    let mode = PrimaryHeader::from_bytes(layer1.header).level_mode;
    let layer1 = if mode.layer1_vertical() {
        decode(layer1_ptr(rom, level)?, Layout::Vertical, 1)?
    } else {
        layer1
    };
    let layer2 = match (mode.layer2(), layer2_ptr(rom, level)?) {
        (Layer2Kind::HorizontalObjects, Layer2Data::Objects(addr)) => {
            Layer2::Objects(decode(addr, Layout::Horizontal, 2)?)
        }
        (Layer2Kind::VerticalObjects, Layer2Data::Objects(addr)) => {
            Layer2::Objects(decode(addr, Layout::Vertical, 2)?)
        }
        (Layer2Kind::Background, Layer2Data::Objects(addr) | Layer2Data::Tilemap(addr)) => {
            Layer2::Background(addr)
        }
        _ => Layer2::None,
    };
    Ok(LevelObjects { layer1, layer2 })
}

/// A level's background tilemap, decompressed.
///
/// In the game's format the stream holds the low bytes of the left half
/// (16 columns by 27 rows) and then of the right, and the Map16 page is 1
/// if the data is at or past `$0CE8FE`, else 0 (`CODE_05801E`); the
/// decompressor may write a byte past the 864 it needs. Lunar Magic
/// keeps that for vanilla backgrounds (flag `V`) and has its own: 32 rows
/// to a half, with the high bytes in a second block of the same size in
/// the same stream when flag `F` is set, and the flags' top nibble for a
/// high byte otherwise. A pointer with bank `$FF` is the game's format
/// whatever the flags say.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Background {
    /// Where the stream starts.
    pub address: SnesAddr,
    /// Whether the pointer had bank `$FF`.
    pub bank_ff: bool,
    /// Lunar Magic's flags for the level, where it has them.
    pub flags: Option<u8>,
    /// The decompressed stream.
    pub data: Vec<u8>,
    /// The stream's length, terminator included.
    pub stream_len: usize,
}

/// Columns in each half of a background tilemap.
pub const BACKGROUND_COLUMNS: usize = 16;
/// Rows in each half as Lunar Magic lays a background out, and as
/// [`Background::tiles`] gives it; the game's format has 27.
pub const BACKGROUND_ROWS: usize = 32;
/// Tiles in a background: two halves.
pub const BACKGROUND_TILES: usize = 2 * BACKGROUND_ROWS * BACKGROUND_COLUMNS;
const GAME_BACKGROUND_ROWS: usize = 27;
/// Where the game's backgrounds start using Map16 page 1.
const BACKGROUND_PAGE_1: SnesAddr = SnesAddr::new(0x0CE8FE);
/// Lunar Magic's level flags ([`tables::LEVEL_FLAGS`]) for backgrounds.
pub const FLAG_VANILLA_BACKGROUND: u8 = 0x08;
pub const FLAG_HIGH_BYTES: u8 = 0x04;
pub const FLAG_CUSTOM_BACKGROUND: u8 = 0x02;

impl Background {
    /// The tilemap as Map16 numbers: the left half's 32 rows of 16, then
    /// the right half's, as Lunar Magic's MWL files hold it. Formats with
    /// 27 rows leave the rest 0, as do tiles past the end of the stream.
    pub fn tiles(&self) -> Vec<u16> {
        let flags = self.flags.unwrap_or(0);
        let custom = !self.bank_ff
            && flags & FLAG_VANILLA_BACKGROUND == 0
            && flags & FLAG_CUSTOM_BACKGROUND != 0;
        let byte = |i: usize| self.data.get(i).copied().unwrap_or(0) as u16;
        let mut tiles = vec![0; BACKGROUND_TILES];
        if custom && flags & FLAG_HIGH_BYTES != 0 {
            for (i, tile) in tiles.iter_mut().enumerate() {
                *tile = byte(BACKGROUND_TILES + i) << 8 | byte(i);
            }
            return tiles;
        }
        let high = if custom {
            flags as u16 >> 4
        } else {
            (self.address >= BACKGROUND_PAGE_1) as u16
        };
        let half = GAME_BACKGROUND_ROWS * BACKGROUND_COLUMNS;
        for side in 0..2 {
            for i in 0..half {
                let at = side * BACKGROUND_ROWS * BACKGROUND_COLUMNS + i;
                tiles[at] = high << 8 | byte(side * half + i);
            }
        }
        tiles
    }
}

/// Whether the ROM has Lunar Magic's per-level flags
/// ([`tables::LEVEL_FLAGS`]): its background hook, a `JML` over the game's
/// `CMP #$FF : BNE` at `$05803B`, reads them, Lunar Magic's or Kobo's.
pub fn has_level_flags(rom: &Rom) -> bool {
    rom.read_u8(tables::BACKGROUND_HOOK).ok() == Some(0x5C)
}

/// Reads and decompresses a level's background tilemap, if its level
/// mode has one.
pub fn read_background(rom: &Rom, level: u16) -> Result<Option<Background>, LevelError> {
    let Layer2::Background(_) = read_objects(rom, level)?.layer2 else {
        return Ok(None);
    };
    read_background_at(rom, level, layer2_ptr(rom, level)?).map(Some)
}

/// Reads and decompresses the background tilemap a level's layer 2
/// pointer names, whatever its level mode.
pub fn read_background_at(
    rom: &Rom,
    level: u16,
    pointer: Layer2Data,
) -> Result<Background, LevelError> {
    let (address, bank_ff) = match pointer {
        Layer2Data::Objects(address) => (address, false),
        Layer2Data::Tilemap(address) => (address, true),
    };
    let unpacked = rle1::decompress(rom.read_tail(address)?)
        .map_err(|source| LevelError::Background { level, source })?;
    let flags = has_level_flags(rom)
        .then(|| rom.read_u8(tables::LEVEL_FLAGS.add(level as u32)))
        .transpose()?;
    Ok(Background {
        address,
        bank_ff,
        flags,
        data: unpacked.data,
        stream_len: unpacked.consumed,
    })
}

/// A level's secondary header: one byte from each table in
/// [`tables::SECONDARY_HEADERS`], `hhhhyyyy 33AAAxxx MMMMffbb NUVEEEEE`.
/// Lunar Magic adds bits to the positions from other tables of its own.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SecondaryHeader {
    /// Layer 2 scroll setting, 0 to 15.
    pub layer2_scroll: u8,
    /// Main entrance Y, 0 to 15.
    pub entrance_y: u8,
    /// Layer 3 setting, 0 to 3.
    pub layer3: u8,
    /// Main entrance action, 0 to 7.
    pub entrance_action: u8,
    /// Main entrance X, 0 to 7.
    pub entrance_x: u8,
    /// Midway entrance screen, 0 to 15.
    pub midway_screen: u8,
    /// Foreground and background initial positions, 0 to 3 each.
    pub fg_position: u8,
    pub bg_position: u8,
    /// Skip the No-Yoshi intro room.
    pub no_yoshi_intro: bool,
    /// Bit 6 of the fourth byte, whose use is unknown.
    pub unknown: bool,
    /// Vertical positioning flag.
    pub vertical_position: bool,
    /// Main entrance screen, 0 to 31.
    pub entrance_screen: u8,
}

impl SecondaryHeader {
    pub fn from_bytes(b: [u8; 4]) -> Self {
        Self {
            layer2_scroll: b[0] >> 4,
            entrance_y: b[0] & 0x0F,
            layer3: b[1] >> 6,
            entrance_action: (b[1] >> 3) & 0x07,
            entrance_x: b[1] & 0x07,
            midway_screen: b[2] >> 4,
            fg_position: (b[2] >> 2) & 0x03,
            bg_position: b[2] & 0x03,
            no_yoshi_intro: b[3] & 0x80 != 0,
            unknown: b[3] & 0x40 != 0,
            vertical_position: b[3] & 0x20 != 0,
            entrance_screen: b[3] & 0x1F,
        }
    }

    pub fn to_bytes(self) -> [u8; 4] {
        [
            (self.layer2_scroll << 4) | (self.entrance_y & 0x0F),
            (self.layer3 << 6) | ((self.entrance_action & 0x07) << 3) | (self.entrance_x & 0x07),
            (self.midway_screen << 4)
                | ((self.fg_position & 0x03) << 2)
                | (self.bg_position & 0x03),
            (self.no_yoshi_intro as u8) << 7
                | (self.unknown as u8) << 6
                | (self.vertical_position as u8) << 5
                | (self.entrance_screen & 0x1F),
        ]
    }
}

pub fn read_secondary_header(rom: &Rom, level: u16) -> Result<SecondaryHeader, LevelError> {
    check(level)?;
    let mut bytes = [0; 4];
    for (byte, table) in bytes.iter_mut().zip(tables::SECONDARY_HEADERS) {
        *byte = rom.read_u8(table.add(level as u32))?;
    }
    Ok(SecondaryHeader::from_bytes(bytes))
}

/// A secondary entrance's four bytes, one from each of
/// [`tables::ENTRANCES`].
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct EntranceBytes(pub [u8; 4]);

impl EntranceBytes {
    /// The level it leads to: the low byte from `$05F800`, and bit 8 from
    /// the entrance's own number in the game (`CODE_05D796` indexes the
    /// tables with the destination's high byte), or from bit 3 of
    /// `$05FE00` where Lunar Magic keeps it.
    pub fn destination(self, id: u16, format: LevelFormat) -> u16 {
        let high = if format.entrances {
            (self.0[3] >> 3) as u16 & 1
        } else {
            id >> 8 & 1
        };
        high << 8 | self.0[0] as u16
    }

    /// Whether the entrance holds anything: the game's tables leave the
    /// unused ones zero. Lunar Magic sets bit 3 of `$05FE00` in every
    /// entrance from `100` on, used or not, so that bit does not count.
    pub fn in_use(self, format: LevelFormat) -> bool {
        let mut bytes = self.0;
        if format.entrances {
            bytes[3] &= !0x08;
        }
        bytes != [0; 4]
    }
}

/// Where the ROM keeps the secondary entrance tables: the game's
/// [`tables::ENTRANCES`], or where Lunar Magic moved them. Lunar Magic keeps
/// pointers to them at fixed addresses the community's level format page
/// documents, `read3($0DE191)`, `read3($0DE198)`, `read3($0DE19F)`, and
/// `read3($05DC81)` (docs/clean-room.md: such pointers are interface),
/// and they are taken when each is the game's table (in either half of the
/// banks) or the start of a RATS block. A ROM with more than 512 entrances
/// has them there (Super Riff World 2).
pub fn entrance_tables(rom: &Rom) -> [SnesAddr; 4] {
    const POINTERS: [u32; 4] = [0x0DE191, 0x0DE198, 0x0DE19F, 0x05DC81];
    let tables: Option<Vec<SnesAddr>> = POINTERS
        .iter()
        .zip(tables::ENTRANCES)
        .map(|(&at, game)| {
            let table = SnesAddr::new(rom.read_u24(SnesAddr::new(at)).ok()?);
            let games = table.raw() & 0x7F_FFFF == game.raw();
            (games || block_len(rom, table).is_some()).then_some(table)
        })
        .collect();
    match tables {
        Some(at) => [at[0], at[1], at[2], at[3]],
        None => tables::ENTRANCES,
    }
}

/// The length of the RATS block starting at `at`, if one does.
pub(crate) fn block_len(rom: &Rom, at: SnesAddr) -> Option<usize> {
    let pc = rom.pc(at).ok()?.as_usize();
    crate::rats::tag_at(rom.data(), pc.checked_sub(8)?)
}

/// How many secondary entrances the tables hold: the game's 512, or where
/// Lunar Magic moved them, as many as the shortest of their RATS blocks
/// holds, up to the [`tables::MAX_ENTRANCES`] a long screen exit can name.
pub fn entrance_count(rom: &Rom) -> u16 {
    entrance_tables(rom)
        .into_iter()
        .map(|table| block_len(rom, table))
        .collect::<Option<Vec<usize>>>()
        .and_then(|lens| lens.into_iter().min())
        .map_or(tables::ENTRANCE_COUNT, |len| {
            len.clamp(
                tables::ENTRANCE_COUNT as usize,
                tables::MAX_ENTRANCES as usize,
            ) as u16
        })
}

/// Every secondary entrance's bytes, by entrance number, from
/// [`entrance_tables`], as many as [`entrance_count`] says.
pub fn read_entrances(rom: &Rom) -> Result<Vec<EntranceBytes>, LevelError> {
    let count = entrance_count(rom) as usize;
    let mut entrances = vec![EntranceBytes::default(); count];
    for (i, table) in entrance_tables(rom).iter().enumerate() {
        let bytes = rom.read(*table, count)?;
        for (entrance, &byte) in entrances.iter_mut().zip(bytes) {
            entrance.0[i] = byte;
        }
    }
    Ok(entrances)
}

/// Writes every secondary entrance's bytes where [`entrance_tables`] says,
/// as many as there are, up to [`entrance_count`].
pub fn write_entrances(rom: &mut Rom, entrances: &[EntranceBytes]) -> Result<(), LevelError> {
    let count = entrances.len().min(entrance_count(rom) as usize);
    for (i, table) in entrance_tables(rom).iter().enumerate() {
        let column: Vec<u8> = entrances[..count].iter().map(|e| e.0[i]).collect();
        rom.write(*table, &column)?;
    }
    Ok(())
}

pub fn read_primary_header(rom: &Rom, level: u16) -> Result<PrimaryHeader, LevelError> {
    let ptr = layer1_ptr(rom, level)?;
    let b = rom.read(ptr, 5)?;
    Ok(PrimaryHeader::from_bytes(b.try_into().expect("5 bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let h = PrimaryHeader {
            bg_palette: 5,
            screens: 20,
            back_area: 3,
            level_mode: LevelMode(0x1E),
            layer3_priority: true,
            music: 6,
            sprite_tileset: 0xB,
            time: 2,
            sprite_palette: 7,
            fg_palette: 1,
            item_memory: 3,
            vertical_scroll: 1,
            object_tileset: 0xE,
        };
        let b = h.to_bytes();
        assert_eq!(b[0], 0b101_10011);
        assert_eq!(b[1], 0b011_11110);
        assert_eq!(b[2], 0b1110_1011);
        assert_eq!(b[3], 0b10_111_001);
        assert_eq!(b[4], 0b1101_1110);
        assert_eq!(PrimaryHeader::from_bytes(b), h);
    }

    #[test]
    fn secondary_header_round_trip() {
        for bytes in [[0x00; 4], [0xFF; 4], [0x5A, 0xC3, 0x96, 0x3C]] {
            assert_eq!(SecondaryHeader::from_bytes(bytes).to_bytes(), bytes);
        }
        let h = SecondaryHeader::from_bytes([0x21, 0b10_011_101, 0x96, 0xA5]);
        assert_eq!((h.layer2_scroll, h.entrance_y), (2, 1));
        assert_eq!((h.layer3, h.entrance_action, h.entrance_x), (2, 3, 5));
        assert_eq!((h.midway_screen, h.fg_position, h.bg_position), (9, 1, 2));
        assert!(h.no_yoshi_intro && !h.unknown && h.vertical_position);
        assert_eq!(h.entrance_screen, 5);
    }

    #[test]
    fn background_tiles_in_each_format() {
        let game = |address: u32, flags| Background {
            address: SnesAddr::new(address),
            bank_ff: true,
            flags,
            data: (0..864).map(|i| i as u8).collect(),
            stream_len: 0,
        };
        // 27 rows a half, the page by address, rows 27 to 31 blank.
        let tiles = game(0x0CE8FE, Some(0x06)).tiles();
        assert_eq!(tiles.len(), BACKGROUND_TILES);
        assert_eq!((tiles[0], tiles[431], tiles[432]), (0x100, 0x1AF, 0));
        assert_eq!(tiles[512], 0x100 | (432 % 256) as u16);
        assert_eq!(game(0x0CE8FD, None).tiles()[1], 0x001);
        // Lunar Magic's own: the flags' top nibble for a high byte, or
        // high bytes after the low ones.
        let custom = Background {
            bank_ff: false,
            ..game(0x108000, Some(0x32))
        };
        assert_eq!(custom.tiles()[1], 0x301);
        let full = Background {
            flags: Some(0x06),
            data: (0..2048).map(|i| (i / 4) as u8).collect(),
            ..custom
        };
        assert_eq!(full.tiles()[1023], 0xFFFF);
        assert_eq!(full.tiles()[4], 0x0101);
    }

    #[test]
    fn level_modes_choose_layer_2() {
        let kind = |mode| LevelMode(mode).layer2();
        assert_eq!(kind(0x00), Layer2Kind::Background);
        assert_eq!(kind(0x0A), Layer2Kind::Background);
        assert_eq!(kind(0x01), Layer2Kind::HorizontalObjects);
        assert_eq!(kind(0x03), Layer2Kind::HorizontalObjects); // vertical layer 1
        assert_eq!(kind(0x0F), Layer2Kind::HorizontalObjects);
        assert_eq!(kind(0x07), Layer2Kind::VerticalObjects);
        for boss in [0x09, 0x0B, 0x10] {
            assert_eq!(kind(boss), Layer2Kind::None);
        }
        assert_eq!(LevelMode(0x0C).to_string(), "$0C");
    }
}
