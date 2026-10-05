//! Layer 1 and layer 2 object data.
//!
//! Object data is a five-byte header (the primary header on layer 1; on
//! layer 2 the game skips it) and then objects in drawing order, ended by
//! a first byte of `$FF`. An object is `NBBYYYYY bbbbXXXX` and more:
//! `BBbbbb` is its number, `YYYYY` and `XXXX` its place on the current
//! screen, and `N` moves the current screen on by one before it is placed.
//! Standard objects have a settings byte. Object `00` is an extended
//! object whose number is the third byte: `00` is a screen exit (a fourth
//! byte, the destination), `01` a screen jump, which sets the current
//! screen. `LoadLevelData` (`$0586F1`) reads them.
//!
//! On a vertical layer the game swaps the two nibbles holding the place
//! (`CODE_0585D8`) for every object but extended objects `00` and `01`:
//! the first byte's low nibble is the column, the second's the row, and
//! bit 4 of the first byte the right half of the 32-tile-wide screen.
//!
//! Lunar Magic adds objects `22`-`28` and `2D`, some longer than three
//! bytes, a five-byte screen exit (extended `02`), a vertical part to the
//! screen jump for levels taller than one screen (bits 0-3 of its second
//! byte, in units of 32 rows), and extended `03`, the same jump with the
//! two parts the other way round, for level heights past 16 of those.
//! Layouts are from the community's documentation of the format.
//!
//! [`decode`] turns the screens into absolute tile positions and drops
//! the screen jumps; [`encode`] chooses its own, so decoding what it
//! encodes gives the same objects, though not always the same bytes. The
//! game keeps the current screen in a byte, so data whose new-screen bits
//! carry it past [`MAX_SCREEN`] is refused rather than placed where the
//! game would not.

use thiserror::Error;

/// A first byte that ends the data.
const END: u8 = 0xFF;
/// The new-screen bit of an object's first byte.
const NEW_SCREEN: u8 = 0x80;

const EXT_SCREEN_EXIT: u8 = 0x00;
const EXT_SCREEN_JUMP: u8 = 0x01;
const EXT_LONG_EXIT: u8 = 0x02;
const EXT_TALL_SCREEN_JUMP: u8 = 0x03;

/// Rows in one unit of a screen jump's vertical part.
const JUMP_ROWS: u16 = 32;

/// The last screen the game's counter (`$1928`, one byte) can name.
pub const MAX_SCREEN: u16 = 0xFF;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObjectError {
    #[error("object data ends at byte {0} without a terminator")]
    Truncated(usize),
    #[error(
        "the object at byte {offset} moves on past screen {MAX_SCREEN}, the last the game's counter holds"
    )]
    PastLastScreen { offset: usize },
    #[error("object {index} is at ({x}, {y}), which a {layout:?} layer cannot place")]
    Unplaceable {
        index: usize,
        x: u16,
        y: u16,
        layout: Layout,
    },
    #[error("object {index} has number {number:02X}, which cannot be written as a {kind}")]
    BadNumber {
        index: usize,
        number: u8,
        kind: &'static str,
    },
    #[error("object {index} holds {len} bytes; a {kind} holds {expected}")]
    BadLength {
        index: usize,
        len: usize,
        kind: &'static str,
        expected: &'static str,
    },
}

/// How a layer's objects are placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Screens side by side, 16 tiles wide.
    Horizontal,
    /// Screens stacked, 16 tiles tall and 32 wide.
    Vertical,
}

/// What a screen jump may say about the vertical part of the position.
/// The game ignores it; Lunar Magic 3's expanded level heights read it.
/// Extended objects `02` and `03` are always read as Lunar Magic's: the
/// game's handlers for them are null pointers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Jumps {
    /// Screen jumps set the screen alone, as in the game.
    Vanilla,
    /// Screen jumps also set the vertical part, as Lunar Magic 3 reads
    /// them.
    Tall,
}

/// The object lists a level has: layer 1's, and layer 2's in a level mode
/// that has objects there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ObjectLayer {
    One,
    Two,
}

/// An object, placed at absolute tile coordinates where it has a place.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Object {
    /// A standard object (`01`-`21`, `2E`-`3F`) and its settings byte.
    Standard {
        number: u8,
        x: u16,
        y: u16,
        settings: u8,
    },
    /// An extended object (`04` and up; `02` and `03` are Lunar Magic's).
    Extended { number: u8, x: u16, y: u16 },
    /// A screen exit: on screen `screen`, to level or secondary exit
    /// `destination`, with flags `0000wush` (see [`ScreenExit`]).
    ScreenExit(ScreenExit),
    /// One of Lunar Magic's placed objects (`22`, `23`, `27`, `29`,
    /// `2D`), with the bytes after its second as they are stored.
    Lunar {
        number: u8,
        x: u16,
        y: u16,
        data: Vec<u8>,
    },
    /// An object with no place, stored as it is without its new-screen
    /// bit: Lunar Magic's settings objects (`24`-`26`, `28`) and its
    /// long screen exit (extended `02`).
    Unplaced(Vec<u8>),
}

/// A screen exit, extended object `00`: `000ppppp 0000wush 00000000
/// dddddddd`; or Lunar Magic's long screen exit, extended object `02`:
/// `000ppppp 00000000 00000010 dddddddd nnnnwush`, whose `nnnn` are bits 9
/// to 12 of the destination, for secondary entrances up to `1FFF`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenExit {
    /// The screen it applies to, 0 to 31.
    pub screen: u8,
    /// `w` (midway, or water in Lunar Magic's format), `u` (Lunar
    /// Magic's format), `s` (secondary exit), `h` (bit 8 of the
    /// destination, in Lunar Magic's format), and a long exit's `nnnn`
    /// above them. What the exit leaves in `$19D8` for its screen.
    pub flags: u8,
    /// The destination's low byte.
    pub destination: u8,
}

/// What a standard object's settings byte holds, from the game's handlers
/// (`CODE_0DA40F`'s table): a height and a width nibble, each one less than
/// the tiles it gives, a type in place of one of them, or a length in the
/// whole byte. The tileset-specific objects (`2E`-`3F`) differ by tileset
/// and are kept as they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Settings {
    HeightWidth,
    HeightType,
    TypeWidth,
    /// A height, and a low nibble the handler does not read.
    Height,
    /// A width, and a high nibble the handler does not read.
    Width,
    /// A length in the whole byte.
    Length,
    Raw,
}

impl Settings {
    pub fn of(number: u8) -> Self {
        match number {
            0x01..=0x0E | 0x14 | 0x16 | 0x18..=0x1B | 0x1D => Self::HeightWidth,
            0x0F | 0x12 | 0x13 | 0x15 | 0x1E => Self::HeightType,
            0x10 | 0x17 => Self::TypeWidth,
            0x11 | 0x1F => Self::Height,
            0x1C | 0x20 => Self::Width,
            0x21 => Self::Length,
            _ => Self::Raw,
        }
    }
}

impl ScreenExit {
    /// Lunar Magic's format (`u`), where `h` is bit 8 of the destination.
    pub const LUNAR_MAGIC: u8 = 0x04;
    /// In Lunar Magic's format, water for a secondary exit and the midway
    /// entrance for a normal one.
    pub const WATER: u8 = 0x08;
    /// A secondary exit, to a secondary entrance.
    pub const SECONDARY: u8 = 0x02;
    pub const HIGH: u8 = 0x01;
    /// A long exit's bits 9 to 12 of the destination.
    pub const LONG: u8 = 0xF0;

    /// An exit in Lunar Magic's format to `destination`, up to `1FFF`
    /// (secondary entrances past `1FF` take a long exit), with `flags`'
    /// `w` and `s`.
    pub fn lunar_magic(screen: u8, flags: u8, destination: u16) -> Self {
        Self {
            screen,
            flags: (flags & (Self::WATER | Self::SECONDARY))
                | Self::LUNAR_MAGIC
                | (destination >> 8 & 1) as u8
                | ((destination >> 9 & 0x0F) as u8) << 4,
            destination: destination as u8,
        }
    }

    /// The destination of an exit in Lunar Magic's format: `h` is its bit
    /// 8, and a long exit's `nnnn` its bits 9 to 12. Lunar Magic's code
    /// reads every exit in its format so (docs/lunar-magic.md).
    pub fn lunar_magic_destination(self) -> u16 {
        ((self.flags >> 4) as u16) << 9
            | ((self.flags & Self::HIGH) as u16) << 8
            | self.destination as u16
    }

    /// Whether the exit takes Lunar Magic's long form.
    pub fn is_long(self) -> bool {
        self.flags & Self::LONG != 0
    }

    /// The exit in Lunar Magic's format, which says the destination's bit
    /// 8 itself: in the game's, it is the current level's. Lunar Magic
    /// rewrites a level's exits so when it saves the level.
    pub fn in_lunar_magic_format(self, level: u16) -> Self {
        if self.flags & Self::LUNAR_MAGIC != 0 {
            return self;
        }
        let high = if level & 0x100 != 0 { Self::HIGH } else { 0 };
        Self {
            flags: (self.flags & !Self::HIGH) | Self::LUNAR_MAGIC | high,
            ..self
        }
    }

    /// The exit in the game's format, if it can be said there. It cannot
    /// for an exit in Lunar Magic's format whose `h` is not the level's bit
    /// 8, which leads to the other bank, or which has `w` (water, or the
    /// midway entrance): the game takes any of bits 1-3 for a secondary
    /// exit (`ExtOBJScreenExit` stores the byte shifted right into
    /// `UseSecondaryExit`) and reads nothing else of them.
    pub fn in_game_format(self, level: u16) -> Option<Self> {
        if self.flags & Self::LUNAR_MAGIC == 0 {
            return Some(self);
        }
        let high = self.flags & Self::HIGH != 0;
        let water = self.flags & Self::WATER != 0;
        (high == (level & 0x100 != 0) && !water && !self.is_long()).then_some(Self {
            flags: self.flags & !(Self::LUNAR_MAGIC | Self::HIGH),
            ..self
        })
    }
}

/// A long screen exit's screen from its first two bytes, as Lunar Magic's
/// code reads it once the game has swapped the nibbles of a vertical
/// layer's (`CODE_0585D8`); `None` where the nibble it does not read is set,
/// which the object is kept whole for.
fn long_exit_screen(a: u8, b: u8, layout: Layout) -> Option<u8> {
    let (screen, unread) = match layout {
        Layout::Horizontal => (a & 0x1F, b & 0x0F),
        Layout::Vertical => ((a & 0x10) | (b & 0x0F), a & 0x0F),
    };
    (unread == 0).then_some(screen)
}

impl Object {
    /// Whether this is a long screen exit kept as its bytes (one
    /// [`ScreenExit`] does not hold), which Kobo's exit code reads as Lunar
    /// Magic's does.
    pub fn is_raw_long_exit(&self) -> bool {
        matches!(self, Object::Unplaced(bytes) if bytes.len() == 5 && bytes[2] == EXT_LONG_EXIT)
    }

    /// The secondary entrance a screen exit in Lunar Magic's format (`u`
    /// and `s`) leads to, long or not, kept as its bytes or not, as Lunar
    /// Magic's code reads it ([`ScreenExit::lunar_magic_destination`]).
    /// `None` for anything else: a normal exit leads to a level, and a
    /// secondary one in the game's format to one of the game's 512
    /// entrances.
    pub fn lunar_magic_entrance(&self) -> Option<u16> {
        let exit = match self {
            Object::ScreenExit(exit) => *exit,
            Object::Unplaced(bytes) if self.is_raw_long_exit() => ScreenExit {
                screen: 0,
                flags: bytes[4],
                destination: bytes[3],
            },
            _ => return None,
        };
        let secondary = ScreenExit::LUNAR_MAGIC | ScreenExit::SECONDARY;
        (exit.flags & secondary == secondary).then(|| exit.lunar_magic_destination())
    }

    /// The number of a Lunar Magic object: one it places, or one of its
    /// settings objects with no place (`24`-`26`, `28`; not its long screen
    /// exit, an extended object).
    pub fn lunar_number(&self) -> Option<u8> {
        match self {
            Object::Lunar { number, .. } => Some(*number),
            Object::Unplaced(_) => Some(object_number(self)).filter(|&n| n != 0),
            _ => None,
        }
    }
}

/// Decoded object data.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ObjectData {
    /// The five bytes before the objects.
    pub header: [u8; 5],
    pub objects: Vec<Object>,
    /// Where each of `objects` starts in the data. A screen jump's bytes
    /// lie between the object before it and the one after.
    pub starts: Vec<usize>,
    /// Bytes read, including the terminator.
    pub len: usize,
}

/// The length of a Lunar Magic placed object, from its first bytes.
fn lunar_len(number: u8, bytes: &[u8]) -> Option<usize> {
    match number {
        0x22 | 0x23 => Some(4),
        0x2D => Some(5),
        0x27 | 0x29 => {
            let kind = *bytes.get(3)? >> 6;
            Some(match kind {
                0 | 1 => 5,
                2 => 6,
                _ if bytes[2] & 0x80 != 0 => 8,
                _ => 7,
            })
        }
        _ => None,
    }
}

fn is_lunar_placed(number: u8) -> bool {
    matches!(number, 0x22 | 0x23 | 0x27 | 0x29 | 0x2D)
}

fn is_lunar_unplaced(number: u8) -> bool {
    matches!(number, 0x24..=0x26 | 0x28)
}

/// Decodes object data: the five-byte header, then objects up to the
/// terminator. Trailing bytes are ignored.
pub fn decode(data: &[u8], layout: Layout, jumps: Jumps) -> Result<ObjectData, ObjectError> {
    let get = |i: usize| data.get(i).copied().ok_or(ObjectError::Truncated(i));
    let header: [u8; 5] = data
        .get(..5)
        .ok_or(ObjectError::Truncated(data.len()))?
        .try_into()
        .expect("five bytes");
    let mut objects = Vec::new();
    let mut starts = Vec::new();
    let mut i = 5;
    // The current screen, and the vertical part a tall jump sets.
    let (mut screen, mut high) = (0u16, 0u16);
    loop {
        let a = get(i)?;
        if a == END {
            return Ok(ObjectData {
                header,
                objects,
                starts,
                len: i + 1,
            });
        }
        let b = get(i + 1)?;
        let third = get(i + 2)?;
        if a & NEW_SCREEN != 0 {
            if screen >= MAX_SCREEN {
                return Err(ObjectError::PastLastScreen { offset: i });
            }
            screen += 1;
        }
        let number = ((a & 0x60) >> 1) | (b >> 4);
        let (lo, hi) = (a & 0x1F, b & 0x0F);
        let (x, y) = match layout {
            Layout::Horizontal => (screen * 16 + hi as u16, high * JUMP_ROWS + lo as u16),
            Layout::Vertical => (lo as u16, screen * 16 + hi as u16),
        };
        let unplaced = |len: usize| -> Result<Object, ObjectError> {
            get(i + len - 1)?;
            let mut bytes = data[i..i + len].to_vec();
            bytes[0] &= !NEW_SCREEN;
            Ok(Object::Unplaced(bytes))
        };
        let (object, len) = if number == 0 {
            match third {
                EXT_SCREEN_EXIT => {
                    let exit = ScreenExit {
                        screen: lo,
                        flags: hi,
                        destination: get(i + 3)?,
                    };
                    (Some(Object::ScreenExit(exit)), 4)
                }
                EXT_SCREEN_JUMP => {
                    screen = lo as u16;
                    if jumps == Jumps::Tall && layout == Layout::Horizontal {
                        high = hi as u16;
                    }
                    (None, 3)
                }
                EXT_TALL_SCREEN_JUMP => {
                    screen = hi as u16;
                    high = lo as u16;
                    (None, 3)
                }
                // As the screen exit, with what the second byte holds there
                // in the fifth. Unlike the screen exit it has its nibbles
                // swapped on a vertical layer, so the screen's low nibble
                // is the second byte's there; the other nibble is not read.
                // One without `u`, whose bits 9 to 12 nothing reads, or
                // with that nibble set, is kept whole.
                EXT_LONG_EXIT => {
                    let flags = get(i + 4)?;
                    match long_exit_screen(a, b, layout) {
                        Some(screen) if flags & ScreenExit::LUNAR_MAGIC != 0 => {
                            let exit = ScreenExit {
                                screen,
                                flags,
                                destination: get(i + 3)?,
                            };
                            (Some(Object::ScreenExit(exit)), 5)
                        }
                        _ => (Some(unplaced(5)?), 5),
                    }
                }
                number => (Some(Object::Extended { number, x, y }), 3),
            }
        } else if is_lunar_placed(number) {
            let len = lunar_len(number, &data[i..data.len().min(i + 4)])
                .ok_or(ObjectError::Truncated(data.len()))?;
            get(i + len - 1)?;
            let data = data[i + 2..i + len].to_vec();
            (Some(Object::Lunar { number, x, y, data }), len)
        } else if is_lunar_unplaced(number) {
            (Some(unplaced(3)?), 3)
        } else {
            let settings = third;
            (
                Some(Object::Standard {
                    number,
                    x,
                    y,
                    settings,
                }),
                3,
            )
        };
        if let Some(object) = object {
            objects.push(object);
            starts.push(i);
        }
        i += len;
    }
}

/// Encodes object data. Each placed object is put on its screen with the
/// new-screen bit when that screen follows the current one, and with a
/// screen jump before it otherwise, or when the bit would make its first
/// byte `$FF`, the terminator.
pub fn encode(
    header: [u8; 5],
    objects: &[Object],
    layout: Layout,
    jumps: Jumps,
) -> Result<Vec<u8>, ObjectError> {
    let mut out = header.to_vec();
    let (mut screen, mut high) = (0u16, 0u16);
    for (index, object) in objects.iter().enumerate() {
        let bad_number = |kind| ObjectError::BadNumber {
            index,
            number: object_number(object),
            kind,
        };
        let (number, x, y, rest): (u8, u16, u16, &[u8]) = match object {
            Object::Standard {
                number,
                x,
                y,
                settings,
            } => {
                let valid = matches!(number, 0x01..=0x3F)
                    && !is_lunar_placed(*number)
                    && !is_lunar_unplaced(*number);
                if !valid {
                    return Err(bad_number("standard object"));
                }
                (*number, *x, *y, std::slice::from_ref(settings))
            }
            Object::Extended { number, x, y } => {
                if *number <= EXT_TALL_SCREEN_JUMP {
                    return Err(bad_number("placed extended object"));
                }
                (0, *x, *y, std::slice::from_ref(number))
            }
            Object::Lunar { number, x, y, data } => {
                if !is_lunar_placed(*number) {
                    return Err(bad_number("Lunar Magic placed object"));
                }
                let mut head = vec![0, 0];
                head.extend_from_slice(data);
                if lunar_len(*number, &head) != Some(data.len() + 2) {
                    return Err(ObjectError::BadLength {
                        index,
                        len: data.len() + 2,
                        kind: "Lunar Magic placed object",
                        expected: "the length its bytes declare",
                    });
                }
                (*number, *x, *y, &data[..])
            }
            Object::ScreenExit(exit) => {
                if exit.screen > 0x1F {
                    return Err(bad_number("screen exit"));
                }
                if exit.is_long() {
                    let (a, b) = match layout {
                        Layout::Horizontal => (exit.screen, 0),
                        Layout::Vertical => (exit.screen & 0x10, exit.screen & 0x0F),
                    };
                    out.extend([a, b, EXT_LONG_EXIT, exit.destination, exit.flags]);
                } else {
                    out.extend([exit.screen, exit.flags, EXT_SCREEN_EXIT, exit.destination]);
                }
                continue;
            }
            Object::Unplaced(bytes) => {
                let expected = match bytes.as_slice() {
                    [a, b, EXT_LONG_EXIT, ..] if a & 0x60 == 0 && b >> 4 == 0 => 5,
                    [a, b, ..] if is_lunar_unplaced(((a & 0x60) >> 1) | (b >> 4)) => 3,
                    _ => return Err(bad_number("unplaced object")),
                };
                if bytes.len() != expected || bytes[0] & NEW_SCREEN != 0 {
                    return Err(ObjectError::BadLength {
                        index,
                        len: bytes.len(),
                        kind: "unplaced object",
                        expected: "3 bytes, or 5 for a long screen exit, with no new-screen bit",
                    });
                }
                out.extend_from_slice(bytes);
                continue;
            }
        };
        let unplaceable = ObjectError::Unplaceable {
            index,
            x,
            y,
            layout,
        };
        let (to_screen, to_high, lo, hi) = match layout {
            Layout::Horizontal => (x / 16, y / JUMP_ROWS, y % JUMP_ROWS, x % 16),
            Layout::Vertical if x < 32 => (y / 16, 0, x, y % 16),
            Layout::Vertical => return Err(unplaceable),
        };
        if to_high > 0 && jumps == Jumps::Vanilla {
            return Err(unplaceable);
        }
        let a = ((number & 0x30) << 1) | lo as u8;
        let b = ((number & 0x0F) << 4) | hi as u8;
        let next = to_high == high && to_screen == screen + 1 && a | NEW_SCREEN != END;
        let a = if (to_screen, to_high) == (screen, high) {
            a
        } else if next {
            a | NEW_SCREEN
        } else {
            let jump = if to_screen <= 0x1F && to_high <= 0x0F {
                [to_screen as u8, to_high as u8, EXT_SCREEN_JUMP]
            } else if jumps == Jumps::Tall && to_screen <= 0x0F && to_high <= 0x1F {
                [to_high as u8, to_screen as u8, EXT_TALL_SCREEN_JUMP]
            } else {
                return Err(unplaceable);
            };
            out.extend(jump);
            a
        };
        (screen, high) = (to_screen, to_high);
        out.extend([a, b]);
        out.extend_from_slice(rest);
    }
    out.push(END);
    Ok(out)
}

/// The object number an object is written with, for errors.
fn object_number(object: &Object) -> u8 {
    match object {
        Object::Standard { number, .. } | Object::Lunar { number, .. } => *number,
        Object::Extended { .. } | Object::ScreenExit(_) => 0,
        Object::Unplaced(bytes) => match bytes.as_slice() {
            [a, b, ..] => ((a & 0x60) >> 1) | (b >> 4),
            _ => 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: [u8; 5] = [1, 2, 3, 4, 5];

    fn data(objects: &[u8]) -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        bytes.extend_from_slice(objects);
        bytes.push(END);
        bytes
    }

    fn round_trip(objects: &[Object], layout: Layout, jumps: Jumps) -> Vec<u8> {
        let bytes = encode(HEADER, objects, layout, jumps).unwrap();
        let decoded = decode(&bytes, layout, jumps).unwrap();
        assert_eq!(decoded.objects, objects);
        assert_eq!(decoded.len, bytes.len());
        bytes
    }

    #[test]
    fn horizontal_objects_are_placed_on_their_screens() {
        // Coins (05) on screen 0, a new-screen cement block (0D), and a
        // screen jump to 3 before an extended object.
        let bytes = data(&[
            0x0A, 0x53, 0x12, // coins at (3, 10)
            0x98, 0xD2, 0x00, // new screen: cement at (16 + 2, 24)
            0x03, 0x00, 0x01, // jump to screen 3
            0x05, 0x04, 0x2D, // extended object 2D at (48 + 4, 5)
        ]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects,
            [
                Object::Standard {
                    number: 0x05,
                    x: 3,
                    y: 10,
                    settings: 0x12
                },
                Object::Standard {
                    number: 0x0D,
                    x: 18,
                    y: 24,
                    settings: 0
                },
                Object::Extended {
                    number: 0x2D,
                    x: 52,
                    y: 5
                },
            ]
        );
        assert_eq!(decoded.len, bytes.len());
        assert_eq!(
            encode(HEADER, &decoded.objects, Layout::Horizontal, Jumps::Vanilla).unwrap(),
            bytes
        );
    }

    #[test]
    fn vertical_layers_swap_the_place() {
        // Row in the second byte, column and half in the first.
        let bytes = data(&[0x80 | 0x13, 0x57, 0x20]);
        let decoded = decode(&bytes, Layout::Vertical, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects,
            [Object::Standard {
                number: 0x05,
                x: 19,
                y: 16 + 7,
                settings: 0x20
            }]
        );
        let bytes = round_trip(&decoded.objects, Layout::Vertical, Jumps::Vanilla);
        assert_eq!(bytes[5..8], [0x93, 0x57, 0x20]);
        assert!(matches!(
            encode(
                HEADER,
                &[Object::Extended {
                    number: 0x40,
                    x: 32,
                    y: 0
                }],
                Layout::Vertical,
                Jumps::Vanilla
            ),
            Err(ObjectError::Unplaceable { .. })
        ));
    }

    #[test]
    fn screen_exits_keep_their_own_screen() {
        let bytes = data(&[0x80 | 0x04, 0x03, 0x00, 0x21, 0x05, 0x14, 0x40]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Vanilla).unwrap();
        assert_eq!(
            decoded.objects[0],
            Object::ScreenExit(ScreenExit {
                screen: 4,
                flags: 3,
                destination: 0x21
            })
        );
        // The exit's new-screen bit still moved the screen on.
        assert_eq!(
            decoded.objects[1],
            Object::Standard {
                number: 0x01,
                x: 20,
                y: 5,
                settings: 0x40
            }
        );
        round_trip(&decoded.objects, Layout::Horizontal, Jumps::Vanilla);
    }

    #[test]
    fn exits_in_lunar_magic_format() {
        let exit = ScreenExit {
            screen: 7,
            flags: 0x02,
            destination: 0xCB,
        };
        assert_eq!(exit.in_lunar_magic_format(0x105).flags, 0x07);
        assert_eq!(exit.in_lunar_magic_format(0x005).flags, 0x06);
        let lunar = ScreenExit {
            flags: 0x04,
            ..exit
        };
        assert_eq!(lunar.in_lunar_magic_format(0x105), lunar);
        // Back to the game's format when `h` is the level's bit 8.
        assert_eq!(
            exit.in_lunar_magic_format(0x105).in_game_format(0x105),
            Some(exit)
        );
        assert_eq!(
            exit.in_lunar_magic_format(0x005).in_game_format(0x005),
            Some(exit)
        );
        assert_eq!(exit.in_game_format(0x105), Some(exit));
        assert_eq!(
            exit.in_lunar_magic_format(0x105).in_game_format(0x005),
            None
        );
        // `w` has no place in the game's format.
        let water = ScreenExit {
            flags: 0x0C,
            ..exit
        };
        assert_eq!(water.in_game_format(0x005), None);
    }

    #[test]
    fn a_first_byte_of_ff_becomes_a_jump() {
        // Object 3x at row 31 of the next screen would start with $FF.
        let objects = [
            Object::Standard {
                number: 0x01,
                x: 0,
                y: 0,
                settings: 0,
            },
            Object::Standard {
                number: 0x3F,
                x: 16,
                y: 31,
                settings: 0,
            },
        ];
        let bytes = round_trip(&objects, Layout::Horizontal, Jumps::Vanilla);
        assert_eq!(bytes[8..], [0x01, 0x00, 0x01, 0x7F, 0xF0, 0x00, END]);
    }

    #[test]
    fn tall_levels_jump_vertically() {
        let objects = [
            Object::Standard {
                number: 0x01,
                x: 20,
                y: 70,
                settings: 0,
            },
            Object::Standard {
                number: 0x01,
                x: 3,
                y: 32 * 20 + 1,
                settings: 0,
            },
        ];
        let bytes = round_trip(&objects, Layout::Horizontal, Jumps::Tall);
        // Screen 1, part 2; then extended 03 for part 20 of screen 0.
        assert_eq!(bytes[5..8], [0x01, 0x02, 0x01]);
        assert_eq!(bytes[11..14], [20, 0x00, 0x03]);
        assert!(matches!(
            encode(HEADER, &objects, Layout::Horizontal, Jumps::Vanilla),
            Err(ObjectError::Unplaceable { index: 0, .. })
        ));
    }

    #[test]
    fn lunar_magic_objects_have_their_own_lengths() {
        let bytes = data(&[
            0x40 | 0x02,
            0x23,
            0x11,
            0x55, // 22: direct Map16, 4 bytes
            0x40 | 0x03,
            0x74,
            0x00,
            0x01,
            0x23, // 27 single tile, 5 bytes
            0x40,
            0x75,
            0x11,
            0x41,
            0x23, // 27 tiles unstretched, 5 bytes
            0x40,
            0x75,
            0x00,
            0x81,
            0x23,
            0x11, // 27 stretched tiles, 6 bytes
            0x40,
            0x76,
            0x05,
            0xC0,
            0x00,
            0x11,
            0x02, // 27 multi-screen, 7
            0x40,
            0x77,
            0x85,
            0xC0,
            0x00,
            0x11,
            0x02,
            0x81, // 27 conditional, 8
            0x40,
            0xD8,
            0x01,
            0x02,
            0x03, // 2D, 5 bytes
            0x80 | 0x40 | 0x05,
            0x60,
            0x21, // 26, the music bypass: no place
            0x01,
            0x00,
            0x02,
            0x10,
            0x3E, // extended 02, a long exit
        ]);
        let decoded = decode(&bytes, Layout::Horizontal, Jumps::Tall).unwrap();
        let lengths: Vec<_> = decoded
            .objects
            .iter()
            .map(|o| match o {
                Object::Lunar { data, .. } => data.len() + 2,
                Object::Unplaced(bytes) => bytes.len(),
                Object::ScreenExit(exit) if exit.is_long() => 5,
                _ => 0,
            })
            .collect();
        assert_eq!(lengths, [4, 5, 5, 6, 7, 8, 5, 3, 5]);
        assert_eq!(decoded.objects[7], Object::Unplaced(vec![0x45, 0x60, 0x21]));
        round_trip(&decoded.objects, Layout::Horizontal, Jumps::Tall);
    }

    #[test]
    fn long_exits_read_as_lunar_magics_code_reads_them() {
        let data = |a: u8, b: u8| {
            let mut bytes = vec![0; 5];
            bytes.extend([a, b, 0x02, 0x20, 0x17, END]);
            bytes
        };
        let objects = |a, b, layout| decode(&data(a, b), layout, Jumps::Tall).unwrap().objects;
        let long = |screen| {
            Object::ScreenExit(ScreenExit {
                screen,
                flags: 0x17,
                destination: 0x20,
            })
        };
        // Screen 3 and its secondary entrance 320 (riff2's level 040). On a
        // vertical layer the game swaps the low nibbles first, so the
        // screen's low nibble is the second byte's.
        assert_eq!(objects(0x03, 0x00, Layout::Horizontal), [long(3)]);
        assert_eq!(objects(0x13, 0x00, Layout::Horizontal), [long(0x13)]);
        assert_eq!(objects(0x00, 0x03, Layout::Vertical), [long(3)]);
        assert_eq!(objects(0x10, 0x03, Layout::Vertical), [long(0x13)]);
        let Object::ScreenExit(exit) = long(3) else {
            unreachable!()
        };
        assert_eq!(exit.lunar_magic_destination(), 0x320);
        assert_eq!(
            ScreenExit::lunar_magic(3, ScreenExit::SECONDARY, 0x320),
            exit
        );
        assert!(exit.in_game_format(0x105).is_none());
        // The nibble the code does not read is kept, with the object, and
        // so is one without `u` (Super Dram World has them).
        assert_eq!(
            objects(0x03, 0x04, Layout::Horizontal),
            [Object::Unplaced(vec![0x03, 0x04, 0x02, 0x20, 0x17])]
        );
        let mut without_u = data(0x03, 0x00);
        without_u[9] = 0xB3;
        let raw = decode(&without_u, Layout::Horizontal, Jumps::Tall)
            .unwrap()
            .objects;
        assert_eq!(raw, [Object::Unplaced(vec![0x03, 0x00, 0x02, 0x20, 0xB3])]);
        assert!(raw[0].is_raw_long_exit());
        // The entrance an exit leads to, for the size of the tables: with
        // `u` and `s` only, kept as bytes or not.
        assert_eq!(long(3).lunar_magic_entrance(), Some(0x320));
        assert_eq!(raw[0].lunar_magic_entrance(), None);
        let raw_secondary = Object::Unplaced(vec![0x03, 0x04, 0x02, 0x20, 0x17]);
        assert_eq!(raw_secondary.lunar_magic_entrance(), Some(0x320));
        let normal = ScreenExit::lunar_magic(3, 0, 0x320);
        assert_eq!(Object::ScreenExit(normal).lunar_magic_entrance(), None);
        let game = ScreenExit {
            screen: 3,
            flags: ScreenExit::SECONDARY,
            destination: 0x20,
        };
        assert_eq!(Object::ScreenExit(game).lunar_magic_entrance(), None);
        round_trip(&raw, Layout::Horizontal, Jumps::Tall);
        for layout in [Layout::Horizontal, Layout::Vertical] {
            let bytes = round_trip(&[long(0x13)], layout, Jumps::Tall);
            assert_eq!(bytes.len(), 5 + 5 + 1);
        }
        // Without bits 9 to 12, it is the four-byte exit Lunar Magic's code
        // leaves the same.
        let short = objects(0x03, 0x00, Layout::Horizontal);
        let short = match &short[0] {
            Object::ScreenExit(e) => ScreenExit { flags: 0x07, ..*e },
            _ => unreachable!(),
        };
        let bytes = encode(
            [0; 5],
            &[Object::ScreenExit(short)],
            Layout::Horizontal,
            Jumps::Tall,
        )
        .unwrap();
        assert_eq!(bytes[5..], [0x03, 0x07, 0x00, 0x20, END]);
    }

    #[test]
    fn malformed_data_is_an_error() {
        assert_eq!(
            decode(&[1, 2, 3], Layout::Horizontal, Jumps::Vanilla),
            Err(ObjectError::Truncated(3))
        );
        let mut bytes = data(&[0x40, 0x77, 0x85, 0xC0]);
        bytes.pop();
        assert!(matches!(
            decode(&bytes, Layout::Horizontal, Jumps::Tall),
            Err(ObjectError::Truncated(_))
        ));
        let bad = |object| encode(HEADER, &[object], Layout::Horizontal, Jumps::Vanilla);
        assert!(matches!(
            bad(Object::Standard {
                number: 0x22,
                x: 0,
                y: 0,
                settings: 0
            }),
            Err(ObjectError::BadNumber { .. })
        ));
        assert!(matches!(
            bad(Object::Extended {
                number: 0x01,
                x: 0,
                y: 0
            }),
            Err(ObjectError::BadNumber { .. })
        ));
        assert!(matches!(
            bad(Object::Lunar {
                number: 0x27,
                x: 0,
                y: 0,
                data: vec![0x00, 0xC0, 0x00]
            }),
            Err(ObjectError::BadLength { .. })
        ));
        assert!(matches!(
            bad(Object::Standard {
                number: 0x01,
                x: 16 * 40,
                y: 0,
                settings: 0
            }),
            Err(ObjectError::Unplaceable { .. })
        ));
    }

    #[test]
    fn new_screen_bits_stop_at_the_games_last_screen() {
        // One object per screen, each moving the screen on: 255 of them
        // reach screen 255; a 256th has nowhere to go, and thousands
        // must not overflow the arithmetic.
        let advance = |count: usize| {
            let mut objects = Vec::new();
            for _ in 0..count {
                objects.extend([0x80 | 0x10, 0x10, 0x00]);
            }
            data(&objects)
        };
        let decoded = decode(&advance(255), Layout::Horizontal, Jumps::Vanilla).unwrap();
        assert_eq!(decoded.objects.len(), 255);
        assert!(matches!(
            decoded.objects[254],
            Object::Standard { x, y: 16, .. } if x == 255 * 16
        ));
        assert_eq!(
            decode(&advance(256), Layout::Horizontal, Jumps::Vanilla),
            Err(ObjectError::PastLastScreen {
                offset: 5 + 255 * 3
            })
        );
        assert!(matches!(
            decode(&advance(4096), Layout::Vertical, Jumps::Tall),
            Err(ObjectError::PastLastScreen { .. })
        ));
    }
}
