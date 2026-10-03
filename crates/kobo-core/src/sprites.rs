//! Level sprite lists.
//!
//! A level's sprite data is a header byte `SBNMMMMM` (buoyancy, buoyancy
//! without layer 2 interaction, Lunar Magic's "new sprite system" flag,
//! sprite memory) followed by entries of three bytes:
//! `yyyyEESY XXXXssss NNNNNNNN` (Y low nibble, extra bits, screen high
//! bit, Y high bit; X, screen low nibble; sprite number). In vertical
//! levels the game reads the `Y` field as the X position and `X` plus the
//! screen as the Y position.
//!
//! In the original format `$FF` ends the list. When the header's `N` bit is
//! set (Lunar Magic 3.00 and later; the flag is per level, not per ROM),
//! `$FF` introduces a command: `$00`-`$7F` sets the upper bits of the Y
//! position for every following sprite, `$FE` ends the list, and `$FF` is
//! an ordinary sprite whose first byte is `$FF`. Sprites inserted with PIXI
//! can carry extension bytes; their count comes from a size table PIXI
//! installs.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::level::{self, LevelError};
use crate::rom::{Rom, RomError};

/// PIXI's marker byte and pointer to its per-sprite data size table.
pub const PIXI_SIZE_TABLE_MARKER: SnesAddr = SnesAddr::new(0x0EF30F);
pub const PIXI_SIZE_TABLE_PTR: SnesAddr = SnesAddr::new(0x0EF30C);
pub const PIXI_MARKER_VALUE: u8 = 0x42;
/// The ROM's sprite-list cursor is 16-bit. Never scan past its addressable stream.
const MAX_LIST_BYTES: usize = 0x1_0000;

/// Header bit 5: the list uses Lunar Magic's command format.
const HEADER_NEW_SPRITE_SYSTEM: u8 = 0x20;
const CMD_END: u8 = 0xFE;
const CMD_LITERAL: u8 = 0xFF;

#[derive(Debug, Error)]
pub enum SpriteError {
    #[error(transparent)]
    Level(#[from] LevelError),
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("sprite data at {0} runs past the end of the ROM")]
    Truncated(SnesAddr),
    #[error("sprite data at {0} has no terminator within 64 KiB")]
    TooLarge(SnesAddr),
    #[error("sprite data at {0} uses the unknown command $FF ${1:02X} at offset {2}")]
    UnknownCommand(SnesAddr, u8, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpriteHeader {
    /// Sprite memory setting, 0 to 31.
    pub memory: u8,
    /// Sprite buoyancy enabled.
    pub buoyancy: bool,
    /// Buoyancy without layer 2 interaction.
    pub buoyancy_no_layer2: bool,
    /// Lunar Magic's "new sprite system" flag: `$FF` starts a command
    /// rather than ending the list.
    pub new_sprite_system: bool,
}

impl SpriteHeader {
    pub fn to_byte(self) -> u8 {
        (self.buoyancy as u8) << 7
            | (self.buoyancy_no_layer2 as u8) << 6
            | if self.new_sprite_system {
                HEADER_NEW_SPRITE_SYSTEM
            } else {
                0
            }
            | (self.memory & 0x1F)
    }

    pub fn from_byte(h: u8) -> Self {
        SpriteHeader {
            memory: h & 0x1F,
            buoyancy: h & 0x80 != 0,
            buoyancy_no_layer2: h & 0x40 != 0,
            new_sprite_system: h & HEADER_NEW_SPRITE_SYSTEM != 0,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpriteEntry {
    /// Sprite number, 0 to 255.
    pub id: u8,
    /// Extra bits, 0 to 3.
    pub extra_bits: u8,
    /// Screen number, 0 to 31.
    pub screen: u8,
    /// X within the screen, 0 to 15.
    pub x: u8,
    /// Y, 0 to 31 in the original format; the Y position jump in effect
    /// supplies bits 5 and up in Lunar Magic's format.
    pub y: u16,
    /// Extension bytes following the entry, if the ROM defines any.
    pub extension: Vec<u8>,
}

impl SpriteEntry {
    /// The entry's first byte, `yyyyEESY`, with Y's bits 5 and up left
    /// to the Y jump in effect.
    pub fn first_byte(&self) -> u8 {
        let y = self.y as u8 & 0x1F;
        (y & 0x0F) << 4 | (self.extra_bits & 0x03) << 2 | (self.screen & 0x10) >> 3 | y >> 4
    }

    /// Level-wide tile coordinates. Horizontal levels place the sprite at
    /// (`screen * 16 + x`, `y`); vertical levels read the fields the other
    /// way round, at (`y`, `screen * 16 + x`), as `LoadSprFromLevel` does.
    pub fn tile_position(&self, vertical: bool) -> (usize, usize) {
        let along = self.screen as usize * 16 + self.x as usize;
        if vertical {
            (self.y as usize, along)
        } else {
            (along, self.y as usize)
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpriteList {
    pub header: SpriteHeader,
    pub sprites: Vec<SpriteEntry>,
    /// Bytes of sprite data consumed, including the terminator.
    pub len: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SpriteEncodeError {
    #[error("sprite {index} has {field} {value}, more than its field holds")]
    OutOfRange {
        index: usize,
        field: &'static str,
        value: u16,
    },
    #[error("sprite {index} starts with $FF, which ends a list in the original format")]
    EndsList { index: usize },
    #[error("sprite {index} ({id:02X}) has {len} extension bytes; the size table says {expected}")]
    Extension {
        index: usize,
        id: u8,
        len: usize,
        expected: usize,
    },
}

/// Encodes a sprite list, the inverse of the parser. In the original
/// format a sprite's Y must fit in five bits and its first byte must not
/// be `$FF`. With the header's new sprite system flag, a Y jump goes
/// before each sprite whose Y bits 5 and up differ from the last one's,
/// a first byte of `$FF` is written `$FF $FF`, and `$FF $FE` ends the
/// list. `sizes` is PIXI's size table, which extension bytes must match.
pub fn encode(
    header: SpriteHeader,
    sprites: &[SpriteEntry],
    sizes: Option<&[u8]>,
) -> Result<Vec<u8>, SpriteEncodeError> {
    let mut out = vec![header.to_byte()];
    let mut y_high = 0;
    for (index, sprite) in sprites.iter().enumerate() {
        let max_y = if header.new_sprite_system {
            0x7F << 5 | 0x1F
        } else {
            0x1F
        };
        for (field, value, max) in [
            ("screen", sprite.screen as u16, 0x1F),
            ("X", sprite.x as u16, 0x0F),
            ("extra bits", sprite.extra_bits as u16, 0x03),
            ("Y", sprite.y, max_y),
        ] {
            if value > max {
                return Err(SpriteEncodeError::OutOfRange {
                    index,
                    field,
                    value,
                });
            }
        }
        let expected = entry_size(sizes, sprite.extra_bits, sprite.id) - 3;
        if sprite.extension.len() != expected {
            return Err(SpriteEncodeError::Extension {
                index,
                id: sprite.id,
                len: sprite.extension.len(),
                expected,
            });
        }
        if header.new_sprite_system && sprite.y >> 5 != y_high {
            y_high = sprite.y >> 5;
            out.extend([0xFF, y_high as u8]);
        }
        let b0 = sprite.first_byte();
        if b0 == 0xFF {
            if !header.new_sprite_system {
                return Err(SpriteEncodeError::EndsList { index });
            }
            out.push(CMD_LITERAL);
        }
        out.extend([b0, sprite.x << 4 | (sprite.screen & 0x0F), sprite.id]);
        out.extend_from_slice(&sprite.extension);
    }
    if header.new_sprite_system {
        out.extend([0xFF, CMD_END]);
    } else {
        out.push(0xFF);
    }
    Ok(out)
}

/// Whether a list needs Lunar Magic's new sprite system: a sprite whose Y
/// needs a Y jump (row 32 and down), or whose first byte is `$FF`, which
/// ends a list in the game's format.
pub fn needs_new_system(sprites: &[SpriteEntry]) -> bool {
    sprites
        .iter()
        .any(|s| s.y > 0x1F || s.first_byte() == CMD_LITERAL)
}

/// The most sprites the game's loader reads from a list: it indexes the
/// list with an 8-bit register. Lunar Magic's loader, and Kobo's in its
/// layout, read [`MAX_SPRITES`].
pub const GAME_MAX_SPRITES: usize = 84;
/// The most sprites a level's list can have in Lunar Magic's layout
/// without a tool's larger load flags table (PIXI's 255-sprite option): the
/// game keeps 128 load flags.
pub const MAX_SPRITES: usize = 128;
/// The most with a 255-sprite loader, such as PIXI's (which moves the load
/// flags to `$7FAF00`), which a ROM declares by clearing bit 0 of
/// [`SPRITE_LIMIT_FLAGS`] (Lunar Magic's help, "Custom sprite size").
pub const MAX_SPRITES_EXTENDED: usize = 255;
/// Bit 0 clear: the ROM loads up to 255 sprites a level.
pub const SPRITE_LIMIT_FLAGS: SnesAddr = SnesAddr::new(0x0FFFE0);

/// The most sprites a level's list can have in `rom`: [`MAX_SPRITES`], or
/// [`MAX_SPRITES_EXTENDED`] when the ROM says it loads 255.
pub fn max_sprites(rom: &Rom) -> usize {
    match rom.read_u8(SPRITE_LIMIT_FLAGS) {
        Ok(flags) if flags & 1 == 0 => MAX_SPRITES_EXTENDED,
        _ => MAX_SPRITES,
    }
}

/// An entry's size in bytes: 3, or what PIXI's size table says if more.
/// A table too short to hold the entry says nothing about it.
fn entry_size(sizes: Option<&[u8]>, extra_bits: u8, id: u8) -> usize {
    sizes
        .and_then(|t| t.get((extra_bits as usize) << 8 | id as usize))
        .map_or(3, |&n| n as usize)
        .max(3)
}

/// Why sprite data in memory does not parse.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SpriteDecodeError {
    #[error("sprite data ends at byte {0} without a terminator")]
    Truncated(usize),
    #[error("sprite data has no terminator within 64 KiB")]
    TooLarge,
    #[error("sprite data uses the unknown command $FF ${0:02X} at offset {1}")]
    UnknownCommand(u8, usize),
}

/// Parses a sprite list held in memory, header byte first, as
/// [`read_sprites_at`] does in a ROM; `sizes` is PIXI's size table (see
/// [`pixi_size_table`]). Trailing bytes are ignored.
pub fn decode(data: &[u8], sizes: Option<&[u8]>) -> Result<SpriteList, SpriteDecodeError> {
    parse_with(
        |i| {
            if i >= MAX_LIST_BYTES {
                return Err(ParseError::TooLarge);
            }
            data.get(i).copied().ok_or(ParseError::Need(i + 1))
        },
        sizes,
    )
    .map_err(|error| match error {
        ParseError::Need(n) => SpriteDecodeError::Truncated(n - 1),
        ParseError::TooLarge => SpriteDecodeError::TooLarge,
        ParseError::UnknownCommand(b, at) => SpriteDecodeError::UnknownCommand(b, at),
    })
}

/// PIXI's data-size table, if installed: one byte per (extra bits, sprite
/// number), giving the total entry size.
pub fn pixi_size_table(rom: &Rom) -> Result<Option<&[u8]>, RomError> {
    // The marker need not exist in a small vanilla image. Once present,
    // a broken pointer is corrupt input, not a ROM without PIXI.
    if rom.read_u8(PIXI_SIZE_TABLE_MARKER).ok() != Some(PIXI_MARKER_VALUE) {
        return Ok(None);
    }
    let ptr = rom.read_ptr(PIXI_SIZE_TABLE_PTR)?;
    rom.read(ptr, 0x400).map(Some)
}

/// Parses a level's sprite list from the pointer tables, Lunar Magic's
/// bank bytes included (see [`level::sprite_ptr`]).
pub fn read_sprites(rom: &Rom, level: u16) -> Result<SpriteList, SpriteError> {
    read_sprites_at(rom, level::sprite_ptr(rom, level)?)
}

/// Parses a sprite list starting at `start` (the header byte).
pub fn read_sprites_at(rom: &Rom, start: SnesAddr) -> Result<SpriteList, SpriteError> {
    let sizes = pixi_size_table(rom)?;
    parse_with(
        |i| {
            if i >= MAX_LIST_BYTES {
                return Err(ParseError::TooLarge);
            }
            rom.read_u8(start.add(i as u32))
                .map_err(|_| ParseError::Need(i + 1))
        },
        sizes,
    )
    .map_err(|error| match error {
        ParseError::Need(n) => {
            debug_assert!(n <= MAX_LIST_BYTES);
            SpriteError::Truncated(start)
        }
        ParseError::TooLarge => SpriteError::TooLarge(start),
        ParseError::UnknownCommand(b, at) => SpriteError::UnknownCommand(start, b, at),
    })
}

enum ParseError {
    /// The data must be at least this long to continue.
    Need(usize),
    TooLarge,
    UnknownCommand(u8, usize),
}

/// A single forward pass; the ROM reader and slice tests share the parser.
fn parse_with(
    mut get: impl FnMut(usize) -> Result<u8, ParseError>,
    sizes: Option<&[u8]>,
) -> Result<SpriteList, ParseError> {
    let header = SpriteHeader::from_byte(get(0)?);
    let mut sprites = Vec::new();
    let mut i = 1;
    let mut y_high = 0u16;
    loop {
        if i >= MAX_LIST_BYTES {
            return Err(ParseError::TooLarge);
        }
        let mut b0 = get(i)?;
        if b0 == 0xFF {
            if !header.new_sprite_system {
                i += 1;
                break;
            }
            let cmd = get(i + 1)?;
            i += 2;
            match cmd {
                CMD_END => break,
                CMD_LITERAL => b0 = 0xFF,
                0x00..=0x7F => {
                    y_high = (cmd as u16) << 5;
                    continue;
                }
                _ => return Err(ParseError::UnknownCommand(cmd, i - 2)),
            }
        } else {
            i += 1;
        }
        // `i` now points at the second byte of the entry.
        let b1 = get(i)?;
        let id = get(i + 1)?;
        let extra_bits = (b0 >> 2) & 0x03;
        let size = entry_size(sizes, extra_bits, id);
        let extension = (2..size - 1)
            .map(|k| get(i + k))
            .collect::<Result<_, _>>()?;
        sprites.push(SpriteEntry {
            id,
            extra_bits,
            screen: ((b0 & 0x02) << 3) | (b1 & 0x0F),
            x: b1 >> 4,
            y: y_high | ((b0 & 0x01) << 4 | (b0 >> 4)) as u16,
            extension,
        });
        i += size - 1;
    }
    Ok(SpriteList {
        header,
        sprites,
        len: i,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unterminated_stream_is_bounded_and_read_once() {
        let mut reads = 0;
        let result = parse_with(
            |i| {
                reads += 1;
                if i >= MAX_LIST_BYTES {
                    return Err(ParseError::TooLarge);
                }
                Ok(0)
            },
            None,
        );
        assert!(matches!(result, Err(ParseError::TooLarge)));
        assert!(reads <= MAX_LIST_BYTES + 1);
    }

    #[test]
    fn installed_pixi_table_must_be_readable() {
        let mut bytes = vec![0; 0x80000];
        bytes[0x7FD5] = 0x20;
        let mapping = crate::Mapping::LoRom;
        let at = mapping
            .snes_to_pc(PIXI_SIZE_TABLE_MARKER)
            .unwrap()
            .as_usize();
        bytes[at] = PIXI_MARKER_VALUE;
        let at = mapping.snes_to_pc(PIXI_SIZE_TABLE_PTR).unwrap().as_usize();
        bytes[at..at + 3].copy_from_slice(&[0, 0x80, 0x20]);
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(matches!(
            read_sprites_at(&rom, SnesAddr::new(0x008000)),
            Err(SpriteError::Rom(RomError::OutOfBounds { .. }))
        ));
    }

    fn parse_sprites(data: &[u8], sizes: Option<&[u8]>) -> Result<SpriteList, ParseError> {
        parse_with(
            |i| data.get(i).copied().ok_or(ParseError::Need(i + 1)),
            sizes,
        )
    }

    fn parse(data: &[u8]) -> SpriteList {
        match parse_sprites(data, None) {
            Ok(l) => l,
            Err(ParseError::Need(n)) => panic!("parser wanted {n} bytes of {}", data.len()),
            Err(ParseError::TooLarge) => panic!("sprite stream too large"),
            Err(ParseError::UnknownCommand(b, at)) => panic!("unknown command {b:02X} at {at}"),
        }
    }

    #[test]
    fn entry_fields() {
        // Y = 0x15 (high bit set, low nibble 5), extra bits 2, screen 0x13,
        // X = 7, sprite 0x2A.
        let b0 = (0x5 << 4) | (2 << 2) | 0x02 | 0x01;
        let b1 = (7 << 4) | 0x3;
        let list = parse(&[0x00, b0, b1, 0x2A, 0xFF]);
        let entry = &list.sprites[0];
        assert_eq!(entry.extra_bits, 2);
        assert_eq!(entry.screen, 0x13);
        assert_eq!(entry.x, 7);
        assert_eq!(entry.y, 0x15);
        assert_eq!(entry.tile_position(false), (0x137, 0x15));
        assert_eq!(entry.tile_position(true), (0x15, 0x137));
        assert_eq!(list.len, 5);
    }

    #[test]
    fn original_format_ends_at_ff_even_with_fe_following() {
        // Kaizo Kindergarten (Lunar Magic 3.03) level 105: an empty list
        // followed by the next RATS block.
        let list = parse(&[0x00, 0xFF, b'S', b'T', b'A', b'R', 0x0D, 0x00, 0xF2, 0xFF]);
        assert!(list.sprites.is_empty());
        assert_eq!(list.len, 2);
        assert!(!list.header.new_sprite_system);
        let list = parse(&[0x80, 0x31, 0x61, 0x35, 0xFF, 0xFE]);
        assert_eq!(list.sprites.len(), 1);
        assert_eq!(list.len, 5);
    }

    #[test]
    fn new_format_commands() {
        // Level 106 of a Lunar Magic 3.31 hack, abridged: two sprites on
        // screen 1, a Y jump to rows 32+, two more, a jump back, one on
        // screen 2, then the end marker.
        let data = [
            0x20, 0xF1, 0x21, 0x72, 0xC0, 0x61, 0x98, 0xFF, 0x01, 0x40, 0x81, 0x99, 0x00, 0x71,
            0x72, 0xFF, 0x00, 0xF1, 0x32, 0x98, 0xFF, 0xFE,
        ];
        let list = parse(&data);
        assert!(list.header.new_sprite_system);
        assert_eq!(list.len, data.len());
        let ys: Vec<u16> = list.sprites.iter().map(|s| s.y).collect();
        assert_eq!(ys, [31, 12, 36, 32, 31]);
        let xs: Vec<usize> = list
            .sprites
            .iter()
            .map(|s| s.tile_position(false).0)
            .collect();
        assert_eq!(xs, [0x12, 0x16, 0x18, 0x17, 0x23]);
    }

    #[test]
    fn new_format_literal_ff_and_unknown_command() {
        let list = parse(&[0x20, 0xFF, 0xFF, 0x45, 0x0A, 0xFF, 0xFE]);
        assert_eq!(list.sprites.len(), 1);
        let s = &list.sprites[0];
        assert_eq!(
            (s.y, s.extra_bits, s.screen, s.x, s.id),
            (0x1F, 3, 0x15, 4, 0x0A)
        );
        assert_eq!(list.len, 7);
        assert!(matches!(
            parse_sprites(&[0x20, 0xFF, 0x80, 0xFF, 0xFE], None),
            Err(ParseError::UnknownCommand(0x80, 1))
        ));
    }

    #[test]
    fn extension_bytes_from_size_table() {
        let mut sizes = vec![0u8; 0x400];
        sizes[2 << 8 | 0x2A] = 5;
        let data = [0x00, 0x08, 0x00, 0x2A, 0xAA, 0xBB, 0x00, 0x00, 0x01, 0xFF];
        let list = parse_sprites(&data, Some(&sizes)).ok().unwrap();
        assert_eq!(list.sprites[0].extension, [0xAA, 0xBB]);
        assert_eq!(list.sprites[1].id, 1);
        assert!(list.sprites[1].extension.is_empty());
        assert_eq!(list.len, data.len());
    }

    #[test]
    fn encoding_is_the_inverse() {
        let old = [0x80, 0x31, 0x61, 0x35, 0x08, 0x00, 0x2A, 0xFF];
        let list = parse(&old);
        assert_eq!(encode(list.header, &list.sprites, None).unwrap(), old);
        let new = [
            0x20, 0xF1, 0x21, 0x72, 0xFF, 0x01, 0x40, 0x81, 0x99, 0xFF, 0x00, 0xFF, 0xFF, 0x45,
            0x0A, 0xFF, 0xFE,
        ];
        let list = parse(&new);
        assert_eq!(encode(list.header, &list.sprites, None).unwrap(), new);

        let mut sizes = vec![0u8; 0x400];
        sizes[2 << 8 | 0x2A] = 5;
        let data = [0x00, 0x08, 0x00, 0x2A, 0xAA, 0xBB, 0xFF];
        let list = parse_sprites(&data, Some(&sizes)).ok().unwrap();
        assert_eq!(
            encode(list.header, &list.sprites, Some(&sizes)).unwrap(),
            data
        );
        assert!(matches!(
            encode(list.header, &list.sprites, None),
            Err(SpriteEncodeError::Extension { expected: 0, .. })
        ));
    }

    #[test]
    fn the_original_format_has_limits() {
        let header = SpriteHeader::from_byte(0);
        let sprite = SpriteEntry {
            id: 1,
            extra_bits: 3,
            screen: 0x1F,
            x: 0,
            y: 0x1F,
            extension: vec![],
        };
        assert_eq!(
            encode(header, std::slice::from_ref(&sprite), None),
            Err(SpriteEncodeError::EndsList { index: 0 })
        );
        let tall = SpriteEntry { y: 0x20, ..sprite };
        assert!(matches!(
            encode(header, &[tall], None),
            Err(SpriteEncodeError::OutOfRange { field: "Y", .. })
        ));
    }

    #[test]
    fn decodes_from_memory() {
        let data = [0x80, 0x31, 0x61, 0x35, 0xFF, 0x12];
        assert_eq!(decode(&data, None).unwrap(), parse(&data));
        assert_eq!(
            decode(&data[..3], None),
            Err(SpriteDecodeError::Truncated(3))
        );
        assert_eq!(
            decode(&[0x20, 0xFF, 0x80], None),
            Err(SpriteDecodeError::UnknownCommand(0x80, 1))
        );
        // A size table too short for an entry says nothing about it.
        let list = decode(&[0x00, 0x08, 0x00, 0x2A, 0xFF], Some(&[5; 0x100][..])).unwrap();
        assert!(list.sprites[0].extension.is_empty());
    }

    #[test]
    fn asks_for_more_bytes() {
        assert!(matches!(parse_sprites(&[], None), Err(ParseError::Need(1))));
        assert!(matches!(
            parse_sprites(&[0x20, 0xFF], None),
            Err(ParseError::Need(3))
        ));
        assert!(matches!(
            parse_sprites(&[0x00, 0x08], None),
            Err(ParseError::Need(3))
        ));
    }
}
