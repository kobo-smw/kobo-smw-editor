//! Lunar Magic's entrance settings: what its per-level tables add to a
//! level's main entrance and layers, the separate midway entrance, and a
//! secondary entrance's two further bytes (formats in the community's
//! level format documentation, effects in docs/lunar-magic-install.md).
//!
//! Every type here holds what changes the game and drops the rest: a
//! position's high bits without position method 2, a midway entrance's
//! settings without its separate flag. Lunar Magic 3's layout is the one
//! kept; older versions' tables are converted as Lunar Magic 3.70's MWL
//! export converts them.

use crate::addr::SnesAddr;
use crate::level::{self, LevelError};
use crate::rom::Rom;

pub mod tables {
    use crate::addr::SnesAddr;

    /// The per-level settings, one byte per level each: `$05DE00`
    /// `IWPXXtTT`, `$06FA00` `SHCvvvvv` (3.40), `$06FC00` `OFYYYYYY`,
    /// `$06FE00` `RL-ooooo` (3.00).
    pub const SETTINGS: [SnesAddr; 4] = [
        SnesAddr::new(0x05DE00),
        SnesAddr::new(0x06FA00),
        SnesAddr::new(0x06FC00),
        SnesAddr::new(0x06FE00),
    ];
    /// Lunar Magic 3's check for its per-level tables: a `JSL` over the
    /// game's `SEP #$30 : LDA $13BF`, where the entrance code ends.
    pub const SETTINGS_HOOK: SnesAddr = SnesAddr::new(0x05DA17);
    /// The midway entrance's hook: a `JSL` over the game's four `LSR`s of
    /// the midway screen. Its target holds, `$0A` bytes in, the address of
    /// the midway tables, four of 512 bytes one after another (three before
    /// 3.00): `IWHMXAAA`, `yyyyxxxx`, `RLE-ffbb`, `-FYYYYYY`.
    pub const MIDWAY_HOOK: SnesAddr = SnesAddr::new(0x05D9E3);
    /// Pointers to Lunar Magic 3's two further secondary entrance tables,
    /// `EFYYYYYY` and `RLW-----`, one byte per entrance each. A fresh
    /// install makes them 510 bytes long.
    pub const ENTRANCE_EXTRA_PTRS: [SnesAddr; 2] =
        [SnesAddr::new(0x05DC86), SnesAddr::new(0x05DC8B)];
    /// Entrances an extra table holds.
    pub const ENTRANCE_EXTRA_LEN: usize = 0x1FE;
}

/// A fresh install's settings bytes, in [`tables::SETTINGS`]'s order.
pub const DEFAULT_BYTES: [u8; 4] = [0x00, 0x20, 0x00, 0x1A];

/// Where layer 2 starts for an entrance that places the layers relative to
/// the player, a setting of the level's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    /// So that the background's last row shows when the player is at the
    /// level's bottom, for a background this many rows tall (1 to 32).
    Height(u8),
    /// This many rows from layer 1 (-15 to 15).
    Offset(i8),
    /// At 0.
    Absolute,
}

impl Background {
    fn from_bits(relative_to_fg: bool, bits: u8) -> Self {
        match (relative_to_fg, bits & 0x1F) {
            (false, n) => Self::Height(n + 1),
            (true, 0x10) => Self::Absolute,
            (true, n) => Self::Offset(signed5(n)),
        }
    }

    /// `O` and `ooooo`.
    fn to_bits(self) -> (bool, u8) {
        match self {
            Self::Height(rows) => (false, (rows.clamp(1, 32) - 1) & 0x1F),
            Self::Offset(rows) => (true, rows.clamp(-15, 15) as u8 & 0x1F),
            Self::Absolute => (true, 0x10),
        }
    }
}

/// A 5-bit two's complement number.
fn signed5(bits: u8) -> i8 {
    ((bits << 3) as i8) >> 3
}

/// A relative camera's offset in rows (-16 to 15), from its top bit `F`
/// and the four below it, which the entrance's `ff` and `bb` hold.
pub fn offset_rows(high: bool, low: u8) -> i8 {
    signed5((high as u8) << 4 | low & 0x0F)
}

/// The top bit and the four below it of an offset in rows.
pub fn offset_bits(rows: i8) -> (bool, u8) {
    let bits = rows.clamp(-16, 15) as u8 & 0x1F;
    (bits & 0x10 != 0, bits & 0x0F)
}

/// A level's settings in Lunar Magic's per-level tables and midway tables,
/// beyond the game's secondary header ([`level::SecondaryHeader`]), which
/// holds the rest of the main entrance: the low bits of its position, and
/// its `ff` and `bb`, which a relative camera reads as its offset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelSettings {
    pub slippery: bool,
    pub water: bool,
    /// Position method 2, with the X tile's bits 3-4 and the Y tile's
    /// bits 4-9; method 1 takes the position from the game's tables.
    pub tile_position: Option<(u8, u8)>,
    /// Sprites spawn by Lunar Magic's smart rules, in range setting 0 to 3
    /// (Lunar Magic's sprite loader reads them).
    pub smart_spawn: bool,
    pub spawn_range: u8,
    /// Layer 2's vertical scroll setting, apart from its horizontal one
    /// (`S`, from Lunar Magic 3.40), which is then the header's nibble.
    pub layer2_vertical_scroll: Option<u8>,
    /// `H`: with `S`, $10 added to the horizontal setting, for the ones
    /// that move layer 2 by themselves.
    pub layer2_horizontal_high: bool,
    /// Lunar Magic sets the level's screen count itself.
    pub auto_screens: bool,
    /// The layers are placed relative to the player: layer 1 at the
    /// player's Y plus the offset `F:ff:bb` rows; this is `F`.
    pub relative: Option<bool>,
    pub face_left: bool,
    pub background: Background,
    pub midway: Midway,
}

impl Default for LevelSettings {
    fn default() -> Self {
        Self::from_bytes(DEFAULT_BYTES, [0; 4])
    }
}

/// The midway entrance's settings of Lunar Magic's own.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Midway {
    /// Bit 4 of the midway screen; the game's four are in `$05F400`.
    pub screen_high: bool,
    pub separate: Option<SeparateMidway>,
}

/// A midway entrance with settings apart from the main entrance's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeparateMidway {
    Entrance(MidwayEntrance),
    /// Another level's midway entrance.
    Redirect(u16),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MidwayEntrance {
    pub slippery: bool,
    pub water: bool,
    pub action: u8,
    /// Tile position: X 0 to 31, Y 0 to 1023.
    pub x: u8,
    pub y: u16,
    /// The game's initial positions, 0 to 3 each, or with `relative` the
    /// offset `F:ff:bb`.
    pub fg_position: u8,
    pub bg_position: u8,
    /// `F`, for a relative camera.
    pub relative: Option<bool>,
    pub face_left: bool,
}

impl LevelSettings {
    /// From the four settings bytes ([`tables::SETTINGS`]) and the midway
    /// entrance's four, in Lunar Magic 3's format.
    pub fn from_bytes(bytes: [u8; 4], midway: [u8; 4]) -> Self {
        let [de, fa, fc, fe] = bytes;
        let bit = |byte: u8, n: u8| byte >> n & 1 != 0;
        Self {
            slippery: bit(de, 7),
            water: bit(de, 6),
            tile_position: bit(de, 5).then_some((de >> 3 & 3, fc & 0x3F)),
            smart_spawn: bit(de, 2),
            spawn_range: de & 3,
            layer2_vertical_scroll: bit(fa, 7).then_some(fa & 0x1F),
            layer2_horizontal_high: bit(fa, 7) && bit(fa, 6),
            auto_screens: bit(fa, 5),
            relative: bit(fe, 7).then_some(bit(fc, 6)),
            face_left: bit(fe, 6),
            background: Background::from_bits(bit(fc, 7), fe),
            midway: Midway::from_bytes(midway),
        }
    }

    /// The four settings bytes and the midway entrance's four.
    pub fn to_bytes(&self) -> ([u8; 4], [u8; 4]) {
        let flag = |b: bool, n: u8| (b as u8) << n;
        let (x_high, y_high) = self.tile_position.unwrap_or((0, 0));
        let de = flag(self.slippery, 7)
            | flag(self.water, 6)
            | flag(self.tile_position.is_some(), 5)
            | (x_high & 3) << 3
            | flag(self.smart_spawn, 2)
            | self.spawn_range & 3;
        let fa = match self.layer2_vertical_scroll {
            Some(v) => 0x80 | v & 0x1F,
            None => 0,
        } | flag(self.layer2_horizontal_high, 6)
            | flag(self.auto_screens, 5);
        let (relative_to_fg, bits) = self.background.to_bits();
        let fc = flag(relative_to_fg, 7) | flag(self.relative == Some(true), 6) | y_high & 0x3F;
        let fe = flag(self.relative.is_some(), 7) | flag(self.face_left, 6) | bits;
        ([de, fa, fc, fe], self.midway.to_bytes())
    }

    /// From a Lunar Magic 2 ROM's `$05DE00` byte (`IWPYX---`) and its three
    /// midway bytes, as Lunar Magic 3 converts them: in a horizontal level
    /// a position's extra Y bit becomes the Y tile's bit 4, in a vertical
    /// one the bit stays, as X's bit 4. The third midway byte is kept whole,
    /// as Lunar Magic 3's MWL export keeps it.
    pub fn from_old_bytes(de: u8, midway: [u8; 3], vertical: bool) -> Self {
        let (mut de, mut fc) = (de & 0xF8, 0);
        let [mut t1, t2, t3] = midway;
        let mut t4 = 0;
        if !vertical {
            fc = de >> 4 & 1;
            de &= !0x10;
            t4 = t1 >> 3 & 1;
            t1 &= !0x08;
        }
        let [_, fa, _, fe] = DEFAULT_BYTES;
        Self::from_bytes([de, fa, fc, fe], [t1, t2, t3, t4])
    }
}

impl Midway {
    fn from_bytes([t1, t2, t3, t4]: [u8; 4]) -> Self {
        let bit = |byte: u8, n: u8| byte >> n & 1 != 0;
        let separate = bit(t1, 5).then(|| {
            if bit(t3, 5) {
                SeparateMidway::Redirect((t3 as u16 & 1) << 8 | t2 as u16)
            } else {
                let relative = bit(t3, 7).then_some(bit(t4, 6));
                SeparateMidway::Entrance(MidwayEntrance {
                    slippery: bit(t1, 7),
                    water: bit(t1, 6),
                    action: t1 & 7,
                    x: (t1 & 0x08) << 1 | t2 & 0x0F,
                    y: (t4 as u16 & 0x3F) << 4 | (t2 >> 4) as u16,
                    fg_position: t3 >> 2 & 3,
                    bg_position: t3 & 3,
                    relative,
                    face_left: bit(t3, 6),
                })
            }
        });
        Self {
            screen_high: bit(t1, 4),
            separate,
        }
    }

    fn to_bytes(self) -> [u8; 4] {
        let flag = |b: bool, n: u8| (b as u8) << n;
        let m = flag(self.screen_high, 4);
        match self.separate {
            None => [m, 0, 0, 0],
            Some(SeparateMidway::Redirect(level)) => {
                [0x20 | m, level as u8, 0x20 | (level >> 8) as u8 & 1, 0]
            }
            Some(SeparateMidway::Entrance(e)) => [
                flag(e.slippery, 7)
                    | flag(e.water, 6)
                    | 0x20
                    | m
                    | (e.x & 0x10) >> 1
                    | e.action & 7,
                ((e.y & 0x0F) as u8) << 4 | e.x & 0x0F,
                flag(e.relative.is_some(), 7)
                    | flag(e.face_left, 6)
                    | (e.fg_position & 3) << 2
                    | e.bg_position & 3,
                flag(e.relative == Some(true), 6) | (e.y >> 4) as u8 & 0x3F,
            ],
        }
    }
}

/// A secondary entrance's settings of Lunar Magic's own: in its
/// `$05FE00` byte (`IPXXDAAA`, the action and destination aside) and its
/// two further tables' (`EFYYYYYY`, `RLW-----`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct EntranceSettings {
    pub slippery: bool,
    /// Position method 2, with the X tile's bits 3-4 and the Y tile's
    /// bits 4-9.
    pub tile_position: Option<(u8, u8)>,
    /// The layers relative to the player, by `F:bb:ff` rows; this is `F`.
    pub relative: Option<bool>,
    pub face_left: bool,
    pub water: bool,
    /// An exit to the overworld instead (`E`), whose tables hold other
    /// things (`--ETDAAA` in `$05FE00`): its bytes at `$05FA00`,
    /// `$05FC00`, and `$05FE00`, kept as they are.
    pub overworld: Option<[u8; 3]>,
}

impl EntranceSettings {
    /// From the entrance's bytes at `$05FA00`, `$05FC00`, and `$05FE00`
    /// and in the two further tables.
    pub fn from_bytes(tables: [u8; 3], extra: [u8; 2]) -> Self {
        let [_, _, fe] = tables;
        let [e5, e6] = extra;
        let bit = |byte: u8, n: u8| byte >> n & 1 != 0;
        if bit(e5, 7) {
            return Self {
                overworld: Some(tables),
                ..Self::default()
            };
        }
        Self {
            slippery: bit(fe, 7),
            tile_position: bit(fe, 6).then_some((fe >> 4 & 3, e5 & 0x3F)),
            relative: bit(e6, 7).then_some(bit(e5, 6)),
            face_left: bit(e6, 6),
            water: bit(e6, 5),
            overworld: None,
        }
    }

    /// Its bits of the `$05FE00` byte, and its two further bytes.
    pub fn to_bytes(self) -> (u8, [u8; 2]) {
        let flag = |b: bool, n: u8| (b as u8) << n;
        if self.overworld.is_some() {
            return (0, [0x80, 0]);
        }
        let (x_high, y_high) = self.tile_position.unwrap_or((0, 0));
        let fe = flag(self.slippery, 7) | flag(self.tile_position.is_some(), 6) | (x_high & 3) << 4;
        let e5 = flag(self.relative == Some(true), 6) | y_high & 0x3F;
        let e6 = flag(self.relative.is_some(), 7) | flag(self.face_left, 6) | flag(self.water, 5);
        (fe, [e5, e6])
    }

    /// From a Lunar Magic 2 ROM's `$05FE00` byte (`IPYXDAAA`), as Lunar
    /// Magic 3 converts it: into a horizontal level the extra Y bit becomes
    /// the Y tile's bit 4.
    pub fn from_old_byte(fe: u8, vertical: bool) -> Self {
        let (fe, e5) = if vertical {
            (fe, 0)
        } else {
            (fe & !0x20, fe >> 5 & 1)
        };
        Self::from_bytes([0, 0, fe], [e5, 0])
    }
}

/// Which of Lunar Magic's entrance tables a ROM has, and in which format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// `$05DE00` in Lunar Magic 3's format (and `$06FC00`, `$06FE00`),
    /// or 2's (`IWPYX---`), or none.
    pub settings: Option<Version>,
    /// `$06FA00`, from Lunar Magic 3.40 on.
    pub scroll: bool,
    /// The midway tables.
    pub midway: Option<SnesAddr>,
    /// The two further secondary entrance tables.
    pub extra: Option<[SnesAddr; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    Old,
    Three,
}

impl Layout {
    /// Read from the ROM's hooks: the per-level tables are Lunar Magic's
    /// with its sprite bank hook ([`level::LevelFormat`]), in 3's format
    /// with the `$05DA17` check (Kobo's builds too); the midway tables with
    /// the midway hook. `$06FA00` holds `$FF` before 3.40, which a version
    /// marker tells; Kobo's builds have none and write it.
    pub fn of(rom: &Rom) -> Self {
        let byte = |addr| rom.read_u8(addr).ok();
        let three = byte(tables::SETTINGS_HOOK) == Some(0x22);
        let lunar_magic = level::LevelFormat::of(rom).lunar_magic;
        let settings = match (three, lunar_magic) {
            (true, _) => Some(Version::Three),
            (false, true) => Some(Version::Old),
            (false, false) => None,
        };
        // Kobo's builds have no version string; one that does not parse as
        // `major.minor` counts as older than 3.40.
        let version = rom.lunar_magic_version().map(|v| parse_version(&v));
        let midway = (byte(tables::MIDWAY_HOOK) == Some(0x22))
            .then(|| read_ptr(rom, tables::MIDWAY_HOOK.add(1)))
            .flatten()
            .and_then(|target| read_ptr(rom, target.add(0x0A)));
        let extra = three
            .then(|| {
                let [a, b] = tables::ENTRANCE_EXTRA_PTRS.map(|p| read_ptr(rom, p));
                Some([a?, b?])
            })
            .flatten();
        Self {
            settings,
            scroll: three && version.is_none_or(|v| v.is_some_and(|v| v >= (3, 40))),
            midway,
            extra,
        }
    }
}

/// A Lunar Magic version string (`3.70`) as its major and minor numbers,
/// the minor in hundredths as the versions number them.
fn parse_version(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.split_once('.')?;
    let minor = match minor.len() {
        1 => minor.parse::<u32>().ok()? * 10,
        2 => minor.parse().ok()?,
        _ => return None,
    };
    Some((major.parse().ok()?, minor))
}

/// A 3-byte pointer, or `None` where it is `$FFFFFF` or cannot be read.
fn read_ptr(rom: &Rom, at: SnesAddr) -> Option<SnesAddr> {
    let bytes = rom.read(at, 3).ok()?;
    let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]);
    (raw != 0xFF_FFFF).then_some(SnesAddr::new(raw))
}

/// A level's settings as the ROM holds them, converted to Lunar Magic 3's
/// layout; the defaults where it has none.
pub fn read_level_settings(
    rom: &Rom,
    layout: &Layout,
    level: u16,
    vertical: bool,
) -> Result<LevelSettings, LevelError> {
    let byte = |table: SnesAddr| rom.read_u8(table.add(level as u32));
    let midway = |n: u32| match layout.midway {
        Some(at) => rom.read_u8(at.add(n * 0x200 + level as u32)),
        None => Ok(0),
    };
    Ok(match layout.settings {
        None => LevelSettings::default(),
        Some(Version::Old) => LevelSettings::from_old_bytes(
            byte(tables::SETTINGS[0])?,
            [midway(0)?, midway(1)?, midway(2)?],
            vertical,
        ),
        Some(Version::Three) => {
            let mut bytes = [0; 4];
            for (b, table) in bytes.iter_mut().zip(tables::SETTINGS) {
                *b = byte(table)?;
            }
            if !layout.scroll {
                bytes[1] = DEFAULT_BYTES[1];
            }
            LevelSettings::from_bytes(bytes, [midway(0)?, midway(1)?, midway(2)?, midway(3)?])
        }
    })
}

/// How many entrances Lunar Magic's two further tables hold: a fresh
/// install's [`tables::ENTRANCE_EXTRA_LEN`], or with the main tables moved
/// past 512 entrances ([`level::entrance_count`]), as many as those and the
/// further tables' own RATS blocks hold.
pub fn extra_count(rom: &Rom, layout: &Layout) -> usize {
    let count = level::entrance_count(rom) as usize;
    match layout.extra {
        Some(tables) if count > level::tables::ENTRANCE_COUNT as usize => tables
            .iter()
            .map(|&t| level::block_len(rom, t).unwrap_or(tables::ENTRANCE_EXTRA_LEN))
            .fold(count, usize::min),
        Some(_) => tables::ENTRANCE_EXTRA_LEN,
        None => 0,
    }
}

/// A secondary entrance's settings as the ROM holds them, from its
/// `$05FE00` byte; `vertical` is its destination's orientation.
pub fn read_entrance_settings(
    rom: &Rom,
    layout: &Layout,
    id: u16,
    fe: u8,
    vertical: bool,
) -> Result<EntranceSettings, LevelError> {
    Ok(match (layout.settings, layout.extra) {
        (Some(Version::Three), extra) => {
            let mut bytes = [0; 2];
            if let Some(tables) = extra
                && (id as usize) < extra_count(rom, layout)
            {
                for (b, table) in bytes.iter_mut().zip(tables) {
                    *b = rom.read_u8(table.add(id as u32))?;
                }
            }
            let tables = level::entrance_tables(rom);
            let fa = rom.read_u8(tables[1].add(id as u32))?;
            let fc = rom.read_u8(tables[2].add(id as u32))?;
            EntranceSettings::from_bytes([fa, fc, fe], bytes)
        }
        (Some(Version::Old), _) => EntranceSettings::from_old_byte(fe, vertical),
        (None, _) => EntranceSettings::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_as_numbers() {
        assert_eq!(parse_version("3.70"), Some((3, 70)));
        assert_eq!(parse_version("3.4"), Some((3, 40)));
        assert_eq!(parse_version("2.53"), Some((2, 53)));
        assert!(parse_version("3.70") > parse_version("3.40"));
        assert!(parse_version("3.33") < Some((3, 40)));
        assert_eq!(parse_version("3"), None);
        assert_eq!(parse_version("x.40"), None);
    }

    #[test]
    fn defaults_are_a_fresh_install() {
        let s = LevelSettings::default();
        assert_eq!(s.background, Background::Height(27));
        assert!(s.auto_screens && s.relative.is_none() && s.midway.separate.is_none());
        assert_eq!(s.to_bytes(), (DEFAULT_BYTES, [0; 4]));
    }

    #[test]
    fn bytes_round_trip_what_they_keep() {
        // Kaizo Kindergarten's level 101: method 2, relative with F set,
        // a separate midway entrance.
        let bytes = [0x20, 0x20, 0x41, 0x9A];
        let midway = [0x20, 0x61, 0x86, 0x41];
        let s = LevelSettings::from_bytes(bytes, midway);
        assert_eq!(s.tile_position, Some((0, 1)));
        assert_eq!(s.relative, Some(true));
        let Some(SeparateMidway::Entrance(m)) = s.midway.separate else {
            panic!("{s:?}")
        };
        assert_eq!((m.x, m.y, m.relative), (1, 0x16, Some(true)));
        assert_eq!(s.to_bytes(), (bytes, midway));
        // Bits without effect are dropped: X and Y high bits without
        // method 2, F without R, the midway entrance without H.
        let s = LevelSettings::from_bytes([0x18, 0x20, 0x45, 0x1A], [0x08, 0x61, 0x86, 0x41]);
        assert_eq!(s.to_bytes(), ([0x00, 0x20, 0x00, 0x1A], [0; 4]));
    }

    #[test]
    fn separate_layer2_settings_keep_h_only_with_s() {
        // S, H, the auto screens bit, vertical $16.
        let s = LevelSettings::from_bytes([0, 0xF6, 0, 0x1A], [0; 4]);
        assert_eq!(s.layer2_vertical_scroll, Some(0x16));
        assert!(s.layer2_horizontal_high);
        assert_eq!(s.to_bytes().0[1], 0xF6);
        // H without S means nothing to Lunar Magic.
        let s = LevelSettings::from_bytes([0, 0x60, 0, 0x1A], [0; 4]);
        assert!(!s.layer2_horizontal_high);
        assert_eq!(s.to_bytes().0[1], 0x20);
    }

    #[test]
    fn background_placements() {
        for (o, bits, want) in [
            (false, 0x1A, Background::Height(27)),
            (false, 0x00, Background::Height(1)),
            (true, 0x1A, Background::Offset(-6)),
            (true, 0x0F, Background::Offset(15)),
            (true, 0x10, Background::Absolute),
        ] {
            assert_eq!(Background::from_bits(o, bits), want);
            assert_eq!(want.to_bits(), (o, bits));
        }
    }

    #[test]
    fn redirected_midway() {
        let m = Midway::from_bytes([0x30, 0x05, 0x21, 0x00]);
        assert!(m.screen_high);
        assert_eq!(m.separate, Some(SeparateMidway::Redirect(0x105)));
        assert_eq!(m.to_bytes(), [0x30, 0x05, 0x21, 0x00]);
    }

    #[test]
    fn old_versions_move_the_y_bit_in_horizontal_levels() {
        // Grand Poo World 2 level 001's midway entrance, and a main
        // entrance with method 2 and the Y bit.
        let s = LevelSettings::from_old_bytes(0x30, [0x2C, 0x12, 0x0A], false);
        assert_eq!(
            s.to_bytes(),
            ([0x20, 0x20, 0x01, 0x1A], [0x24, 0x12, 0x0A, 0x01])
        );
        let s = LevelSettings::from_old_bytes(0x30, [0x28, 0x31, 0x0A], true);
        assert_eq!(
            s.to_bytes(),
            ([0x30, 0x20, 0x00, 0x1A], [0x28, 0x31, 0x0A, 0x00])
        );
        let e = EntranceSettings::from_old_byte(0x60, false);
        assert_eq!(e.to_bytes(), (0x40, [0x01, 0x00]));
    }

    #[test]
    fn entrance_settings() {
        // Kaizo Kindergarten's entrance 0C5: method 2 with X bit 3, F set.
        let e = EntranceSettings::from_bytes([0x24, 0x04, 0x50], [0x40, 0x00]);
        assert_eq!(e.tile_position, Some((1, 0)));
        assert_eq!(e.relative, None);
        assert_eq!(e.to_bytes(), (0x50, [0x00, 0x00]));
        let e = EntranceSettings::from_bytes([0x24, 0x04, 0x90], [0x40, 0xE0]);
        assert!(e.slippery && e.face_left && e.water);
        assert_eq!(e.relative, Some(true));
        assert_eq!(e.to_bytes(), (0x80, [0x40, 0xE0]));
    }
}

/// Where the game puts the player for a main entrance's position settings
/// (method 1, the game's own; `CODE_05D796`): the X setting picks the low
/// byte of his X from `$05D750` (and, in a vertical level, the high byte
/// from `$05D758`), the Y setting his Y from `$05D730` and `$05D740`, and
/// the screen setting is the high byte of his X in a horizontal level and
/// of his Y in a vertical one. Read from a ROM, which a patch may have
/// changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MainEntranceTables {
    x_low: [u8; 8],
    x_high: [u8; 8],
    y: [u16; 16],
}

impl MainEntranceTables {
    pub fn read(rom: &Rom) -> Result<Self, crate::rom::RomError> {
        let bytes = |addr: u32, len: usize| rom.read(SnesAddr::new(addr), len);
        let (x_low, x_high) = (bytes(0x05_D750, 8)?, bytes(0x05_D758, 8)?);
        let (y_low, y_high) = (bytes(0x05_D730, 16)?, bytes(0x05_D740, 16)?);
        Ok(Self {
            x_low: x_low.try_into().expect("eight bytes"),
            x_high: x_high.try_into().expect("eight bytes"),
            y: std::array::from_fn(|i| u16::from(y_high[i]) << 8 | u16::from(y_low[i])),
        })
    }

    /// The player's place, in level pixels, for a screen (0 to 31), an X
    /// setting (0 to 7), and a Y setting (0 to 15).
    pub fn position(&self, screen: u8, x: u8, y: u8, vertical: bool) -> (u32, u32) {
        let (x, y) = (usize::from(x & 7), usize::from(y & 15));
        let screen = u32::from(screen & 0x1F);
        if vertical {
            let px = u32::from(self.x_high[x]) << 8 | u32::from(self.x_low[x]);
            (px, screen << 8 | u32::from(self.y[y] & 0xFF))
        } else {
            (screen << 8 | u32::from(self.x_low[x]), u32::from(self.y[y]))
        }
    }

    /// The settings (screen, X, Y) whose place is nearest (`px`, `py`).
    pub fn nearest(&self, px: i32, py: i32, vertical: bool) -> (u8, u8, u8) {
        let mut best = (0, 0, 0);
        let mut distance = i64::MAX;
        for screen in 0..32u8 {
            for x in 0..8u8 {
                for y in 0..16u8 {
                    let (ex, ey) = self.position(screen, x, y, vertical);
                    let (dx, dy) = (i64::from(ex as i32 - px), i64::from(ey as i32 - py));
                    let d = dx * dx + dy * dy;
                    if d < distance {
                        distance = d;
                        best = (screen, x, y);
                    }
                }
            }
        }
        best
    }
}
