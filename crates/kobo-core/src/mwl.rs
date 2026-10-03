//! Lunar Magic's MWL files: one level each, as its "Save Level to File"
//! and `-ExportLevel` write them and "Open Level From File" reads them.
//!
//! A file is a 64-byte header, a table of sections, and the sections. The
//! header is `"LM"`, the version as a little-endian word (`$0370` for
//! 3.70), the table's file offset and length, four flag bytes (bit 0 of
//! the first: a level of Super Mario Advance 2), and 48 bytes of free text.
//! The table has an offset and a length per section; all of these numbers
//! are 32-bit little-endian. Lunar Magic writes eight sections ([`Section`])
//! right after the header, one after another; all but the first and the
//! last start with an eight-byte [`SectionHeader`]. An MWL holds no
//! graphics, Map16, or shared palette, and not the sizes of PIXI's sprite
//! extension bytes. Lunar Magic writes a level in its current format
//! whatever the version of the ROM it comes from: screen exits in its own
//! format, screen jumps as Lunar Magic 3 reads them, backgrounds as 16-bit
//! tiles.
//!
//! The layouts are the community's documentation of the format, checked
//! against files Lunar Magic 3.70 wrote (docs/lunar-magic.md has what was
//! confirmed and how). [`MwlFile`] is the container, read and written byte
//! for byte; [`MwlFile::decode`] gives the level as typed data ([`Mwl`]),
//! and [`Mwl::to_file`] goes back, choosing its own object encoding.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::level::objects::{self, Jumps, Layout, ObjectData, ObjectError};
use crate::level::{
    BACKGROUND_TILES, FLAG_CUSTOM_BACKGROUND, FLAG_VANILLA_BACKGROUND, Layer2Kind, PrimaryHeader,
    SecondaryHeader,
};
use crate::palette::{Color15, Palette};
use crate::sprites::{self, SpriteDecodeError, SpriteEncodeError, SpriteList};

pub const MAGIC: [u8; 2] = *b"LM";
/// Bytes before the section table.
pub const HEADER_LEN: usize = 0x40;
pub const COMMENT_LEN: usize = 48;
/// The bytes of one entry of the section table.
const TABLE_ENTRY_LEN: usize = 8;
/// Bit 0 of the first flag byte: a level of Super Mario Advance 2.
pub const FLAG_SMA2: u8 = 0x01;

/// Where Lunar Magic keeps the per-level bytes a section carries. The
/// game's own tables are in [`crate::level::tables`].
pub mod tables {
    use crate::addr::SnesAddr;

    /// Lunar Magic's fifth secondary header byte, one per level.
    pub const SECONDARY_HEADER_5: SnesAddr = SnesAddr::new(0x05DE00);
    /// Lunar Magic 3.40's secondary header byte, one per level.
    pub const SECONDARY_HEADER_FA: SnesAddr = SnesAddr::new(0x06FA00);
    /// Lunar Magic 3's secondary header bytes, one per level each.
    pub const SECONDARY_HEADER_FC: SnesAddr = SnesAddr::new(0x06FC00);
    pub const SECONDARY_HEADER_FE: SnesAddr = SnesAddr::new(0x06FE00);
    /// Per-level animation settings, `PTLG----`.
    pub const ANIMATION_SETTINGS: SnesAddr = SnesAddr::new(0x03FE00);
    pub use crate::level::tables::ENTRANCES;
}

/// The sections Lunar Magic writes, in table order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    /// The level number and header bytes: [`LevelInfo`].
    LevelInfo,
    Layer1,
    Layer2,
    Sprites,
    Palette,
    /// The secondary entrances that lead into the level.
    Entrances,
    /// The level's ExAnimation data.
    Animation,
    /// The ExGFX file numbers: [`ExGfx`].
    ExGfx,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::LevelInfo,
        Section::Layer1,
        Section::Layer2,
        Section::Sprites,
        Section::Palette,
        Section::Entrances,
        Section::Animation,
        Section::ExGfx,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::LevelInfo => "level information",
            Section::Layer1 => "layer 1",
            Section::Layer2 => "layer 2",
            Section::Sprites => "sprites",
            Section::Palette => "palette",
            Section::Entrances => "secondary entrances",
            Section::Animation => "ExAnimation",
            Section::ExGfx => "ExGFX",
        }
    }
}

impl std::fmt::Display for Section {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MwlError {
    #[error("not an MWL file: it does not start with \"LM\"")]
    NotMwl,
    #[error("the file is {0} bytes, shorter than its 64-byte header")]
    Truncated(usize),
    #[error(
        "the section table ({len} bytes at {offset:#X}) does not fit the file ({file_len} bytes) or is not in 8-byte entries"
    )]
    BadTable {
        offset: u32,
        len: u32,
        file_len: usize,
    },
    #[error(
        "section {index} ({len} bytes at {offset:#X}) runs past the end of the file ({file_len} bytes)"
    )]
    BadSection {
        index: usize,
        offset: u32,
        len: u32,
        file_len: usize,
    },
    #[error("the file has no {0} section")]
    MissingSection(Section),
    #[error("the {section} section is {len} bytes; it must be {expected}")]
    SectionLength {
        section: Section,
        len: usize,
        expected: &'static str,
    },
    #[error("layer {layer}: {source}")]
    Objects {
        layer: u8,
        #[source]
        source: ObjectError,
    },
    #[error("sprites: {0}")]
    Sprites(#[source] SpriteDecodeError),
    #[error(
        "the sprite list ends at byte {len} of {section_len}: extension bytes need the sprite size table of the ROM it came from"
    )]
    SpriteLength { len: usize, section_len: usize },
    #[error("sprites: {0}")]
    SpriteEncode(#[source] SpriteEncodeError),
    #[error("a background has {0} tiles; it must have {BACKGROUND_TILES}")]
    BackgroundTiles(usize),
}

/// An MWL file as it is stored: its header and its sections, undecoded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MwlFile {
    /// The Lunar Magic version that wrote it, `$0370` for 3.70.
    pub version: u16,
    /// Bit 0 of the first byte is [`FLAG_SMA2`]; no other is known.
    pub flags: [u8; 4],
    /// Free text; Lunar Magic writes its name, version, and year.
    pub comment: [u8; COMMENT_LEN],
    /// The sections in table order, the first eight as in [`Section::ALL`].
    pub sections: Vec<Vec<u8>>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

impl MwlFile {
    /// Reads the container. Sections may be anywhere in the file, and may
    /// overlap; bytes no section covers are dropped.
    pub fn parse(bytes: &[u8]) -> Result<Self, MwlError> {
        if bytes.len() < HEADER_LEN {
            return Err(if MAGIC.starts_with(&bytes[..bytes.len().min(2)]) {
                MwlError::Truncated(bytes.len())
            } else {
                MwlError::NotMwl
            });
        }
        if bytes[..2] != MAGIC {
            return Err(MwlError::NotMwl);
        }
        let (offset, len) = (u32_at(bytes, 4), u32_at(bytes, 8));
        let table = (offset as usize)
            .checked_add(len as usize)
            .filter(|&end| end <= bytes.len() && (len as usize).is_multiple_of(TABLE_ENTRY_LEN))
            .map(|end| &bytes[offset as usize..end])
            .ok_or(MwlError::BadTable {
                offset,
                len,
                file_len: bytes.len(),
            })?;
        let sections = table
            .as_chunks::<TABLE_ENTRY_LEN>()
            .0
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let (offset, len) = (u32_at(entry, 0), u32_at(entry, 4));
                (offset as usize)
                    .checked_add(len as usize)
                    .and_then(|end| bytes.get(offset as usize..end))
                    .map(<[u8]>::to_vec)
                    .ok_or(MwlError::BadSection {
                        index,
                        offset,
                        len,
                        file_len: bytes.len(),
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            version: u16::from_le_bytes([bytes[2], bytes[3]]),
            flags: bytes[12..16].try_into().expect("four bytes"),
            comment: bytes[16..HEADER_LEN].try_into().expect("48 bytes"),
            sections,
        })
    }

    /// The file as Lunar Magic lays it out: the header, the table right
    /// after it, and the sections in order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let table_len = self.sections.len() * TABLE_ENTRY_LEN;
        let mut out = MAGIC.to_vec();
        out.extend(self.version.to_le_bytes());
        out.extend((HEADER_LEN as u32).to_le_bytes());
        out.extend((table_len as u32).to_le_bytes());
        out.extend(self.flags);
        out.extend(self.comment);
        let mut offset = HEADER_LEN + table_len;
        for section in &self.sections {
            out.extend((offset as u32).to_le_bytes());
            out.extend((section.len() as u32).to_le_bytes());
            offset += section.len();
        }
        for section in &self.sections {
            out.extend_from_slice(section);
        }
        out
    }

    pub fn section(&self, section: Section) -> Result<&[u8], MwlError> {
        self.sections
            .get(section as usize)
            .map(Vec::as_slice)
            .ok_or(MwlError::MissingSection(section))
    }

    /// Whether the level came from Super Mario Advance 2.
    pub fn is_sma2(&self) -> bool {
        self.flags[0] & FLAG_SMA2 != 0
    }

    /// Decodes the level. `sizes` is PIXI's sprite size table (see
    /// [`sprites::pixi_size_table`]) of the ROM the file came from, which
    /// the file does not record: without it, a sprite with extension bytes
    /// misreads the list, which is then an error.
    pub fn decode(&self, sizes: Option<&[u8]>) -> Result<Mwl, MwlError> {
        let info = LevelInfo::decode(self.section(Section::LevelInfo)?)?;
        let (header, data) = split(self, Section::Layer1)?;
        let layer1 = Layer1 {
            header,
            data: decode_objects(data, None, 1)?,
        };
        let mode = layer1.primary_header().level_mode;
        let (header, data) = split(self, Section::Layer2)?;
        let flags = header.0[0];
        let data = if flags & (FLAG_VANILLA_BACKGROUND | FLAG_CUSTOM_BACKGROUND) != 0 {
            if data.len() != 2 * BACKGROUND_TILES {
                return Err(MwlError::SectionLength {
                    section: Section::Layer2,
                    len: data.len() + SectionHeader::LEN,
                    expected: "8 + 2048 bytes for a background",
                });
            }
            let tiles = data.as_chunks().0.iter().map(|&t| u16::from_le_bytes(t));
            Layer2Data::Background(tiles.collect())
        } else if data.is_empty() {
            Layer2Data::Empty
        } else {
            let vertical = mode.layer2() == Layer2Kind::VerticalObjects;
            Layer2Data::Objects(decode_objects(data, Some(vertical), 2)?)
        };
        let layer2 = Layer2 { header, data };
        let (header, data) = split(self, Section::Sprites)?;
        let list = sprites::decode(data, sizes).map_err(MwlError::Sprites)?;
        if list.len != data.len() {
            return Err(MwlError::SpriteLength {
                len: list.len,
                section_len: data.len(),
            });
        }
        let sprites = Sprites { header, list };
        let (header, data) = split(self, Section::Palette)?;
        let palette = LevelPalette::decode(header, data)?;
        let (header, data) = split(self, Section::Entrances)?;
        if !data.len().is_multiple_of(SecondaryEntrance::LEN) {
            return Err(MwlError::SectionLength {
                section: Section::Entrances,
                len: data.len() + SectionHeader::LEN,
                expected: "8 bytes and 8 per entrance",
            });
        }
        let entrances = Entrances {
            header,
            entries: data
                .as_chunks()
                .0
                .iter()
                .map(SecondaryEntrance::from_bytes)
                .collect(),
        };
        let (header, data) = split(self, Section::Animation)?;
        let animation = Animation {
            header,
            data: data.to_vec(),
        };
        let exgfx = ExGfx::decode(self.section(Section::ExGfx)?)?;
        Ok(Mwl {
            version: self.version,
            flags: self.flags,
            comment: self.comment,
            info,
            layer1,
            layer2,
            sprites,
            palette,
            entrances,
            animation,
            exgfx,
            extra: self
                .sections
                .get(Section::ALL.len()..)
                .unwrap_or(&[])
                .to_vec(),
        })
    }
}

/// A section's header and data.
fn split(file: &MwlFile, section: Section) -> Result<(SectionHeader, &[u8]), MwlError> {
    let bytes = file.section(section)?;
    let (header, data) =
        bytes
            .split_first_chunk::<{ SectionHeader::LEN }>()
            .ok_or(MwlError::SectionLength {
                section,
                len: bytes.len(),
                expected: "at least 8 bytes",
            })?;
    Ok((SectionHeader(*header), data))
}

/// Decodes object data; `vertical` is `None` for layer 1, whose own
/// header says. Screen jumps are read as Lunar Magic 3 writes them.
fn decode_objects(data: &[u8], vertical: Option<bool>, layer: u8) -> Result<ObjectData, MwlError> {
    let error = |source| MwlError::Objects { layer, source };
    let decode = |layout| objects::decode(data, layout, Jumps::Tall).map_err(error);
    let horizontal = decode(Layout::Horizontal)?;
    let vertical = vertical.unwrap_or_else(|| {
        PrimaryHeader::from_bytes(horizontal.header)
            .level_mode
            .layer1_vertical()
    });
    if vertical {
        decode(Layout::Vertical)
    } else {
        Ok(horizontal)
    }
}

fn layout(vertical: bool) -> Layout {
    if vertical {
        Layout::Vertical
    } else {
        Layout::Horizontal
    }
}

/// The eight bytes before a section's data. Bytes 4 to 6 are where the
/// data was in the ROM it was exported from, which importing ignores
/// (zero for none, and see [`Animation::source`]); byte 0 is the
/// section's own ([`Layer1::custom_palette`], [`Layer2::flags`],
/// [`Animation::settings`]); the rest are zero in every file seen.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SectionHeader(pub [u8; 8]);

impl SectionHeader {
    pub const LEN: usize = 8;

    /// Where the data came from, if the file says.
    pub fn source(self) -> Option<SnesAddr> {
        let addr = u32::from_le_bytes([self.0[4], self.0[5], self.0[6], 0]);
        (addr != 0).then_some(SnesAddr::new(addr))
    }
}

/// The level information section: 64 bytes, of which 18 are known.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelInfo {
    /// The level the file was saved from, which Lunar Magic imports it to
    /// unless told otherwise.
    pub level: u16,
    /// The game's four secondary header bytes.
    pub secondary: SecondaryHeader,
    /// Lunar Magic's fifth secondary header byte
    /// ([`tables::SECONDARY_HEADER_5`]), then two bytes zero in every file
    /// seen.
    pub secondary_lm: [u8; 3],
    /// The midway entrance's bytes in Lunar Magic's four tables, then one
    /// zero in every file seen.
    pub midway: [u8; 5],
    /// Lunar Magic 3's per-level bytes: [`tables::SECONDARY_HEADER_FC`] and
    /// [`tables::SECONDARY_HEADER_FE`], the level size byte (`TB0MMMMM`,
    /// see docs/lunar-magic.md), and [`tables::SECONDARY_HEADER_FA`].
    pub lm3: [u8; 4],
    /// The rest of the section, zero in every file seen.
    pub rest: Vec<u8>,
}

impl LevelInfo {
    const KNOWN: usize = 18;

    fn decode(bytes: &[u8]) -> Result<Self, MwlError> {
        if bytes.len() < Self::KNOWN {
            return Err(MwlError::SectionLength {
                section: Section::LevelInfo,
                len: bytes.len(),
                expected: "at least 18 bytes",
            });
        }
        Ok(Self {
            level: u16::from_le_bytes([bytes[0], bytes[1]]),
            secondary: SecondaryHeader::from_bytes(bytes[2..6].try_into().expect("four")),
            secondary_lm: bytes[6..9].try_into().expect("three"),
            midway: bytes[9..14].try_into().expect("five"),
            lm3: bytes[14..18].try_into().expect("four"),
            rest: bytes[Self::KNOWN..].to_vec(),
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.level.to_le_bytes().to_vec();
        out.extend(self.secondary.to_bytes());
        out.extend(self.secondary_lm);
        out.extend(self.midway);
        out.extend(self.lm3);
        out.extend_from_slice(&self.rest);
        out
    }
}

/// Layer 1: the primary header and objects, in the ROM's format.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Layer1 {
    pub header: SectionHeader,
    pub data: ObjectData,
}

impl Layer1 {
    pub fn primary_header(&self) -> PrimaryHeader {
        PrimaryHeader::from_bytes(self.data.header)
    }

    /// Bit 0 of the section header: the level has a custom palette, which
    /// is the one in the file.
    pub fn custom_palette(&self) -> bool {
        self.header.0[0] & 0x01 != 0
    }
}

/// Layer 2, which the flags say is objects or a background.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Layer2 {
    pub header: SectionHeader,
    pub data: Layer2Data,
}

impl Layer2 {
    /// The level's byte of Lunar Magic's flags (`bbBBVFCT`,
    /// [`crate::level::tables::LEVEL_FLAGS`]). Lunar Magic 3.70 writes a
    /// background of the game's format as `V` and `F` (`$0C`), and one of
    /// its own as `C` and `F`: with the ROM's top nibble (`BB`, the BG
    /// Map16 bank) if it had `F`, and with that nibble moved into the
    /// tiles' high bytes (and cleared) if not.
    pub fn flags(&self) -> u8 {
        self.header.0[0]
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Layer2Data {
    /// Object data, with a header of its own the game skips.
    Objects(ObjectData),
    /// A background tilemap: the Map16 numbers of the left half's 32 rows
    /// of 16 and then the right half's (see
    /// [`crate::level::Background::tiles`]), without the bank the flags
    /// hold. Uncompressed, unlike in a ROM.
    Background(Vec<u16>),
    /// No data.
    Empty,
}

/// The sprite list, header byte first.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sprites {
    pub header: SectionHeader,
    pub list: SpriteList,
}

/// The level's colours: the custom palette if it has one (see
/// [`Layer1::custom_palette`]), otherwise the palette its header selects.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelPalette {
    pub header: SectionHeader,
    pub colors: Palette,
    /// Last in the file, where a ROM's custom palette has it first.
    pub back_area: Color15,
}

impl LevelPalette {
    /// 256 colours and the back area colour.
    const LEN: usize = 2 * 257;

    fn decode(header: SectionHeader, data: &[u8]) -> Result<Self, MwlError> {
        if data.len() != Self::LEN {
            return Err(MwlError::SectionLength {
                section: Section::Palette,
                len: data.len() + SectionHeader::LEN,
                expected: "8 + 514 bytes",
            });
        }
        let color = |i: usize| Color15(u16::from_le_bytes([data[2 * i], data[2 * i + 1]]));
        let mut colors = Palette::default();
        for (i, c) in colors.colors.iter_mut().enumerate() {
            *c = color(i);
        }
        Ok(Self {
            header,
            colors,
            back_area: color(256),
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header.0.to_vec();
        for color in self.colors.colors.iter().chain([&self.back_area]) {
            out.extend(color.to_le_bytes());
        }
        out
    }
}

/// The secondary entrances whose destination is the level.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entrances {
    pub header: SectionHeader,
    pub entries: Vec<SecondaryEntrance>,
}

/// A secondary entrance: its bytes in the game's tables
/// ([`tables::ENTRANCES`]) but the first, which the level implies, and in
/// two of Lunar Magic 3's. Their meaning is in docs/lunar-magic.md.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecondaryEntrance {
    pub id: u16,
    /// Its bytes at `$05FA00`, `$05FC00`, and `$05FE00`.
    pub tables: [u8; 3],
    /// Its bytes in Lunar Magic 3's two further tables.
    pub lm: [u8; 2],
    /// Zero in every file seen.
    pub unused: u8,
}

impl SecondaryEntrance {
    const LEN: usize = 8;

    fn from_bytes(b: &[u8; Self::LEN]) -> Self {
        Self {
            id: u16::from_le_bytes([b[0], b[1]]),
            tables: [b[2], b[3], b[4]],
            lm: [b[5], b[6]],
            unused: b[7],
        }
    }

    fn to_bytes(self) -> [u8; 8] {
        let [lo, hi] = self.id.to_le_bytes();
        let [a, b, c] = self.tables;
        let [d, e] = self.lm;
        [lo, hi, a, b, c, d, e, self.unused]
    }
}

/// The level's ExAnimation data, as the ROM has it; global ExAnimation is
/// not in an MWL.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Animation {
    pub header: SectionHeader,
    pub data: Vec<u8>,
}

impl Animation {
    /// Where the data was: the ROM's pointer as it was, whose middle byte
    /// is zero when the level has none (`$0000FF` in Lunar Magic 3.x ROMs).
    pub fn source(&self) -> Option<SnesAddr> {
        self.header.source().filter(|a| a.offset() >> 8 != 0)
    }

    /// The level's byte of [`tables::ANIMATION_SETTINGS`].
    pub fn settings(&self) -> u8 {
        self.header.0[0]
    }
}

/// The level's graphics list ([`crate::exgfx`]), under its section's name.
pub use crate::exgfx::GraphicsList as ExGfx;

impl ExGfx {
    /// What Lunar Magic 3.70 exports for every level of the vanilla ROM.
    pub const VANILLA: Self = Self::DEFAULT;

    fn decode(bytes: &[u8]) -> Result<Self, MwlError> {
        if bytes.len() != 32 {
            return Err(MwlError::SectionLength {
                section: Section::ExGfx,
                len: bytes.len(),
                expected: "32 bytes",
            });
        }
        Ok(Self::from_bytes(bytes.try_into().expect("32 bytes")))
    }
}

/// An MWL file's level, decoded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mwl {
    pub version: u16,
    pub flags: [u8; 4],
    pub comment: [u8; COMMENT_LEN],
    pub info: LevelInfo,
    pub layer1: Layer1,
    pub layer2: Layer2,
    pub sprites: Sprites,
    pub palette: LevelPalette,
    pub entrances: Entrances,
    pub animation: Animation,
    pub exgfx: ExGfx,
    /// Sections past the eighth, which no known version writes.
    pub extra: Vec<Vec<u8>>,
}

impl Mwl {
    /// Encodes the level. Objects and sprites are encoded as
    /// [`objects::encode`] and [`sprites::encode`] choose, so decoding the
    /// result gives this level back, though not always the same bytes.
    /// `sizes` is as for [`MwlFile::decode`].
    pub fn to_file(&self, sizes: Option<&[u8]>) -> Result<MwlFile, MwlError> {
        let encode = |data: &ObjectData, vertical: bool, layer: u8| {
            objects::encode(data.header, &data.objects, layout(vertical), Jumps::Tall)
                .map_err(|source| MwlError::Objects { layer, source })
        };
        let with_header = |header: SectionHeader, data: &[u8]| {
            let mut out = header.0.to_vec();
            out.extend_from_slice(data);
            out
        };
        let mode = self.layer1.primary_header().level_mode;
        let layer1 = encode(&self.layer1.data, mode.layer1_vertical(), 1)?;
        let layer2 = match &self.layer2.data {
            Layer2Data::Objects(data) => {
                encode(data, mode.layer2() == Layer2Kind::VerticalObjects, 2)?
            }
            Layer2Data::Background(tiles) => {
                if tiles.len() != BACKGROUND_TILES {
                    return Err(MwlError::BackgroundTiles(tiles.len()));
                }
                tiles.iter().flat_map(|t| t.to_le_bytes()).collect()
            }
            Layer2Data::Empty => Vec::new(),
        };
        let list = &self.sprites.list;
        let sprites =
            sprites::encode(list.header, &list.sprites, sizes).map_err(MwlError::SpriteEncode)?;
        let entrances: Vec<u8> = self
            .entrances
            .entries
            .iter()
            .flat_map(|e| e.to_bytes())
            .collect();
        let mut sections = vec![
            self.info.to_bytes(),
            with_header(self.layer1.header, &layer1),
            with_header(self.layer2.header, &layer2),
            with_header(self.sprites.header, &sprites),
            self.palette.to_bytes(),
            with_header(self.entrances.header, &entrances),
            with_header(self.animation.header, &self.animation.data),
            self.exgfx.0.iter().flat_map(|f| f.to_le_bytes()).collect(),
        ];
        sections.extend(self.extra.iter().cloned());
        Ok(MwlFile {
            version: self.version,
            flags: self.flags,
            comment: self.comment,
            sections,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::level::objects::Object;

    /// A small level in the layout Lunar Magic writes: level `105`, one
    /// cement block, a Koopa, a background, one entrance, two ExGFX files.
    pub(crate) fn sample() -> Vec<u8> {
        let mut info = vec![0x05, 0x01, 0x5B, 0x00, 0x9A, 0x00, 0x00, 0, 0];
        info.extend([0; 5]);
        info.extend([0x00, 0x1A, 0x00, 0x20]);
        info.resize(0x40, 0);
        let header = |b0: u8, source: u32| {
            let s = source.to_le_bytes();
            vec![b0, 0, 0, 0, s[0], s[1], s[2], 0]
        };
        let mut layer1 = header(0, 0x0688DD);
        layer1.extend([0x33, 0x40, 0x08, 0x80, 0x27]); // primary header, mode 0
        layer1.extend([0x98, 0xD2, 0x00, 0xFF]); // screen 1: cement at (18, 24)
        let mut layer2 = header(0x0C, 0xFFD900);
        layer2.extend((0..1024u16).flat_map(|i| (i % 0x200).to_le_bytes()));
        let mut sprites = header(0, 0x07C4CA);
        sprites.extend([0x00, 0x31, 0x61, 0x04, 0xFF]); // a Koopa
        let mut palette = header(0, 0);
        palette.extend((0..257u16).flat_map(|i| (i * 3).to_le_bytes()));
        let mut entrances = header(0, 0);
        entrances.extend([0xCB, 0x01, 0xA9, 0x08, 0x0E, 0x00, 0x00, 0x00]);
        let animation = header(0, 0);
        let mut exgfx: Vec<u8> = [0x7F_u16; 14]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        exgfx.extend([0x00, 0x01, 0xFF, 0xFF]);
        let sections = [
            info, layer1, layer2, sprites, palette, entrances, animation, exgfx,
        ];
        let mut out = b"LM".to_vec();
        out.extend(0x0370u16.to_le_bytes());
        out.extend(0x40u32.to_le_bytes());
        out.extend(0x40u32.to_le_bytes());
        out.extend([0; 4]);
        out.extend(b"|Lunar Magic 3.70|   test   |Defender of Relm");
        out.resize(0x40, b' ');
        let mut offset = 0x80;
        for s in &sections {
            out.extend((offset as u32).to_le_bytes());
            out.extend((s.len() as u32).to_le_bytes());
            offset += s.len();
        }
        for s in &sections {
            out.extend_from_slice(s);
        }
        out
    }

    #[test]
    fn the_container_is_read_and_written_byte_for_byte() {
        let bytes = sample();
        let file = MwlFile::parse(&bytes).unwrap();
        assert_eq!(file.version, 0x0370);
        assert_eq!(file.sections.len(), 8);
        assert!(!file.is_sma2());
        assert_eq!(file.to_bytes(), bytes);
    }

    #[test]
    fn sections_decode_to_typed_data() {
        let file = MwlFile::parse(&sample()).unwrap();
        let mwl = file.decode(None).unwrap();
        assert_eq!(mwl.info.level, 0x105);
        assert_eq!(mwl.info.secondary.entrance_y, 0x0B);
        assert_eq!(mwl.info.lm3, [0x00, 0x1A, 0x00, 0x20]);
        assert_eq!(mwl.layer1.primary_header().screens, 20);
        assert!(!mwl.layer1.custom_palette());
        assert_eq!(mwl.layer1.header.source(), Some(SnesAddr::new(0x0688DD)));
        assert_eq!(
            mwl.layer1.data.objects,
            [Object::Standard {
                number: 0x0D,
                x: 18,
                y: 24,
                settings: 0
            }]
        );
        assert_eq!(mwl.layer2.flags(), 0x0C);
        let Layer2Data::Background(tiles) = &mwl.layer2.data else {
            panic!("a background");
        };
        assert_eq!((tiles[0x1FF], tiles[0x200]), (0x1FF, 0));
        assert_eq!(mwl.sprites.list.sprites[0].id, 0x04);
        assert_eq!(mwl.palette.colors.colors[255], Color15(765));
        assert_eq!(mwl.palette.back_area, Color15(768));
        assert_eq!(mwl.palette.header.source(), None);
        assert_eq!(
            mwl.entrances.entries,
            [SecondaryEntrance {
                id: 0x1CB,
                tables: [0xA9, 0x08, 0x0E],
                lm: [0, 0],
                unused: 0
            }]
        );
        assert!(mwl.animation.data.is_empty());
        assert_eq!(mwl.exgfx.0[14..], [0x100, 0xFFFF]);
        // What Kobo encodes decodes to the same level; here, byte for byte.
        let again = mwl.to_file(None).unwrap();
        assert_eq!(again, file);
    }

    #[test]
    fn object_layer_2_follows_the_level_mode() {
        let mut file = MwlFile::parse(&sample()).unwrap();
        // Mode $07: vertical layers 1 and 2, the flags clear.
        file.sections[1][8 + 1] = 0x07;
        let mut objects = vec![0x00; 8];
        objects.extend([0, 0, 0, 0, 0, 0x93, 0x57, 0x20, 0xFF]);
        file.sections[2] = objects;
        let mwl = file.decode(None).unwrap();
        let Layer2Data::Objects(data) = &mwl.layer2.data else {
            panic!("objects");
        };
        assert_eq!(
            data.objects,
            [Object::Standard {
                number: 0x05,
                x: 19,
                y: 23,
                settings: 0x20
            }]
        );
        assert_eq!(mwl.to_file(None).unwrap(), file);
    }

    #[test]
    fn malformed_files_are_errors() {
        let bytes = sample();
        assert_eq!(MwlFile::parse(&bytes[..10]), Err(MwlError::Truncated(10)));
        assert_eq!(MwlFile::parse(b"L"), Err(MwlError::Truncated(1)));
        assert_eq!(MwlFile::parse(b"MW"), Err(MwlError::NotMwl));
        assert_eq!(MwlFile::parse(&[0; 0x40]), Err(MwlError::NotMwl));
        let mut bad = bytes.clone();
        bad[8] = 0x41; // the table's length
        assert!(matches!(
            MwlFile::parse(&bad),
            Err(MwlError::BadTable { .. })
        ));
        let mut bad = bytes.clone();
        bad[0x40 + 15] = 0xFF; // layer 1's length
        assert!(matches!(
            MwlFile::parse(&bad),
            Err(MwlError::BadSection { index: 1, .. })
        ));
        for n in 0..bytes.len() {
            let _ = MwlFile::parse(&bytes[..n]).map(|f| f.decode(None));
        }
        let mut file = MwlFile::parse(&bytes).unwrap();
        file.sections.truncate(5);
        assert_eq!(
            file.decode(None),
            Err(MwlError::MissingSection(Section::Entrances))
        );
        let mut file = MwlFile::parse(&bytes).unwrap();
        file.sections[3].pop();
        assert!(matches!(file.decode(None), Err(MwlError::Sprites(_))));
        let mut file = MwlFile::parse(&bytes).unwrap();
        file.sections[3].push(0);
        assert!(matches!(
            file.decode(None),
            Err(MwlError::SpriteLength { .. })
        ));
        let mut file = MwlFile::parse(&bytes).unwrap();
        file.sections[2].pop();
        assert!(matches!(
            file.decode(None),
            Err(MwlError::SectionLength {
                section: Section::Layer2,
                ..
            })
        ));
        let mut mwl = MwlFile::parse(&bytes).unwrap().decode(None).unwrap();
        mwl.layer2.data = Layer2Data::Background(vec![0; 3]);
        assert_eq!(mwl.to_file(None), Err(MwlError::BackgroundTiles(3)));
    }
}
