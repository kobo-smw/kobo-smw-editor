//! 8x8 tile graphics: SNES bit-plane formats and SMW's numbered GFX files.
//!
//! SMW stores GFX files `00` to `31` LC_LZ2-compressed; Lunar Magic can
//! recompress a hack's as LC_LZ3 ([`Compression`]). Vanilla files are
//! 3bpp or 2bpp; Lunar Magic may re-insert files as 4bpp. Each file has a
//! fixed tile count, so the bit depth follows from the decompressed size.
//! `GFX27` holds the Mode 7 boss tiles, whose pixels are packed three bits
//! each ([`GfxFormat::Packed3`]).
//!
//! `GFX32` (Mario) and `GFX33` (animated tiles) are LC_LZ2 too but have no
//! entry in the pointer tables: `CODE_00B888` loads their addresses as
//! immediates, which Lunar Magic rewrites when it moves the files.
//!
//! Lunar Magic exports 3bpp files converted to 4bpp and leaves other depths
//! as stored; [`GfxFile::to_lm_export`] reproduces that so exports can be
//! compared byte for byte.

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::compress::{Decompressed, LzError, lz2, lz3};
use crate::rom::{Rom, RomError};

/// Number of GFX files: `00` to `31` through the pointer tables, then
/// `32` and `33`.
pub const GFX_FILE_COUNT: u8 = 0x34;
/// Number of GFX files reachable through the pointer tables.
const GFX_TABLE_FILES: u8 = 0x32;

/// The three vanilla pointer tables, one byte of each pointer per table.
/// Lunar Magic keeps these tables in place and rewrites the entries.
pub const GFX_PTR_LO: SnesAddr = SnesAddr::new(0x00B992);
pub const GFX_PTR_HI: SnesAddr = SnesAddr::new(0x00B9C4);
pub const GFX_PTR_BANK: SnesAddr = SnesAddr::new(0x00B9F6);

/// Operands in `CODE_00B888`: `LDY #GFX33`, the `LDA #bank` after it, and
/// `LDA #GFX32` at `CODE_00B8D7`. The bank is set once, for both files.
pub const GFX33_PTR: SnesAddr = SnesAddr::new(0x00B88B);
pub const GFX32_33_BANK: SnesAddr = SnesAddr::new(0x00B890);
pub const GFX32_PTR: SnesAddr = SnesAddr::new(0x00B8D8);

/// `CODE_00B8DE`: the decompression routine, which every GFX file goes
/// through with its pointer in `$8A`-`$8C`.
const DECOMPRESS: SnesAddr = SnesAddr::new(0x00B8DE);

/// GFX file lists per tileset, 4 files each: FG1, FG2, BG1, FG3 for
/// objects and SP1 to SP4 for sprites.
pub const OBJECT_GFX_LIST: SnesAddr = SnesAddr::new(0x00A92B);
pub const SPRITE_GFX_LIST: SnesAddr = SnesAddr::new(0x00A8C3);
/// Rows in each list.
pub const GFX_LIST_ROWS: u8 = 26;

fn gfx_list_row(rom: &Rom, table: SnesAddr, index: u8) -> Result<[u8; 4], GfxError> {
    if index >= GFX_LIST_ROWS {
        return Err(GfxError::BadTileset(index));
    }
    let b = rom.read(table.add(4 * index as u32), 4)?;
    Ok([b[0], b[1], b[2], b[3]])
}

/// The layer GFX files (FG1, FG2, BG1, FG3) of an object tileset. Tilesets
/// 0 to 14 are the level tilesets; higher rows are boss and special levels.
pub fn object_tileset_files(rom: &Rom, tileset: u8) -> Result<[u8; 4], GfxError> {
    gfx_list_row(rom, OBJECT_GFX_LIST, tileset)
}

/// The sprite GFX files (SP1 to SP4) of a sprite tileset.
pub fn sprite_tileset_files(rom: &Rom, tileset: u8) -> Result<[u8; 4], GfxError> {
    gfx_list_row(rom, SPRITE_GFX_LIST, tileset)
}

/// Bits per pixel of a tile format.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bpp {
    Two,
    Three,
    Four,
}

impl Bpp {
    pub const fn bytes_per_tile(self) -> usize {
        match self {
            Self::Two => 16,
            Self::Three => 24,
            Self::Four => 32,
        }
    }

    pub const fn colors(self) -> usize {
        1 << self.bits()
    }

    pub const fn bits(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}

/// One 8x8 tile as palette indices, `pixels[y][x]`, x = 0 on the left.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tile8 {
    pub pixels: [[u8; 8]; 8],
}

impl Tile8 {
    /// Decodes one tile. `bytes` must be at least `bpp.bytes_per_tile()` long.
    pub fn decode(bpp: Bpp, bytes: &[u8]) -> Self {
        assert!(bytes.len() >= bpp.bytes_per_tile(), "tile data too short");
        let mut pixels = [[0u8; 8]; 8];
        for (y, row) in pixels.iter_mut().enumerate() {
            let planes: [u8; 4] = match bpp {
                Bpp::Two => [bytes[2 * y], bytes[2 * y + 1], 0, 0],
                Bpp::Three => [bytes[2 * y], bytes[2 * y + 1], bytes[16 + y], 0],
                Bpp::Four => [
                    bytes[2 * y],
                    bytes[2 * y + 1],
                    bytes[16 + 2 * y],
                    bytes[16 + 2 * y + 1],
                ],
            };
            for (x, px) in row.iter_mut().enumerate() {
                let bit = 7 - x;
                *px = planes
                    .iter()
                    .enumerate()
                    .map(|(p, plane)| ((plane >> bit) & 1) << p)
                    .sum();
            }
        }
        Self { pixels }
    }

    /// Encodes the tile. Pixel values above the format's range are masked.
    pub fn encode(&self, bpp: Bpp) -> Vec<u8> {
        let mut planes = [[0u8; 8]; 4];
        for (y, row) in self.pixels.iter().enumerate() {
            for (x, &px) in row.iter().enumerate() {
                for (p, plane) in planes.iter_mut().enumerate() {
                    plane[y] |= ((px >> p) & 1) << (7 - x);
                }
            }
        }
        let mut out = Vec::with_capacity(bpp.bytes_per_tile());
        let [p0, p1, p2, p3] = planes;
        out.extend(p0.iter().zip(&p1).flat_map(|(a, b)| [*a, *b]));
        match bpp {
            Bpp::Two => {}
            Bpp::Three => out.extend(p2),
            Bpp::Four => out.extend(p2.iter().zip(&p3).flat_map(|(a, b)| [*a, *b])),
        }
        out
    }
}

/// Decodes every whole tile in `data`. Trailing partial tiles are ignored.
pub fn decode_tiles(bpp: Bpp, data: &[u8]) -> Vec<Tile8> {
    data.chunks_exact(bpp.bytes_per_tile())
        .map(|c| Tile8::decode(bpp, c))
        .collect()
}

/// Decodes every whole tile of [`GfxFormat::Packed3`] data.
pub fn decode_packed3_tiles(data: &[u8]) -> Vec<Tile8> {
    (data.as_chunks::<PACKED3_BYTES_PER_TILE>().0.iter())
        .map(|tile| {
            let mut pixels = [[0u8; 8]; 8];
            for (row, &[high, middle, low]) in pixels.iter_mut().zip(tile.as_chunks::<3>().0) {
                let bits = u32::from_be_bytes([0, high, middle, low]);
                for (x, px) in row.iter_mut().enumerate() {
                    *px = (bits >> (21 - 3 * x) & 7) as u8;
                }
            }
            Tile8 { pixels }
        })
        .collect()
}

/// Encodes tiles as [`GfxFormat::Packed3`] data: each row 24 bits, the
/// leftmost pixel in the top three.
pub fn encode_packed3_tiles(tiles: &[Tile8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tiles.len() * PACKED3_BYTES_PER_TILE);
    for tile in tiles {
        for row in &tile.pixels {
            let bits = row.iter().enumerate().fold(0u32, |acc, (x, &px)| {
                acc | ((px as u32 & 7) << (21 - 3 * x))
            });
            out.extend(&bits.to_be_bytes()[1..]);
        }
    }
    out
}

impl GfxFormat {
    /// Tiles in this format.
    pub fn encode(self, tiles: &[Tile8]) -> Vec<u8> {
        match self {
            Self::Planar(bpp) => tiles.iter().flat_map(|t| t.encode(bpp)).collect(),
            Self::Packed3 => encode_packed3_tiles(tiles),
        }
    }

    /// Colours a pixel can be.
    pub fn colors(self) -> usize {
        match self {
            Self::Planar(bpp) => bpp.colors(),
            Self::Packed3 => 8,
        }
    }
}

/// Tiles as an indexed image, 16 to a row, previewed in grays.
pub fn tiles_to_image(tiles: &[Tile8], colors: usize) -> crate::image::IndexedImage {
    let rows = tiles.len().div_ceil(16);
    let (width, height) = (128, rows as u32 * 8);
    let mut pixels = vec![0; (width * height) as usize];
    for (i, tile) in tiles.iter().enumerate() {
        let (ox, oy) = (i % 16 * 8, i / 16 * 8);
        for (y, row) in tile.pixels.iter().enumerate() {
            for (x, &px) in row.iter().enumerate() {
                pixels[(oy + y) * width as usize + ox + x] = px;
            }
        }
    }
    crate::image::IndexedImage {
        width,
        height,
        pixels,
        palette: crate::image::grayscale(colors),
    }
}

/// The tiles of an image laid out as [`tiles_to_image`] lays them, `count`
/// of them, every pixel below `colors`.
pub fn image_to_tiles(
    image: &crate::image::IndexedImage,
    count: usize,
    colors: usize,
) -> Result<Vec<Tile8>, String> {
    let rows = count.div_ceil(16) as u32;
    if image.width != 128 || image.height != rows * 8 {
        return Err(format!(
            "is {}x{}, not 128x{} ({count} tiles, 16 to a row)",
            image.width,
            image.height,
            rows * 8
        ));
    }
    if let Some(px) = image.pixels.iter().find(|&&p| p as usize >= colors) {
        return Err(format!(
            "has colour index {px}, past the {colors} this file can hold"
        ));
    }
    Ok((0..count)
        .map(|i| {
            let (ox, oy) = (i % 16 * 8, i / 16 * 8);
            let mut pixels = [[0; 8]; 8];
            for (y, row) in pixels.iter_mut().enumerate() {
                for (x, px) in row.iter_mut().enumerate() {
                    *px = image.pixels[(oy + y) * 128 + ox + x];
                }
            }
            Tile8 { pixels }
        })
        .collect())
}

/// Re-lays 3bpp tile data out as 4bpp with an empty fourth plane. This is
/// the form Lunar Magic exports 3bpp files in.
pub fn convert_3bpp_to_4bpp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 24 * 32);
    for tile in data.as_chunks::<24>().0 {
        out.extend_from_slice(&tile[..16]);
        for &plane2 in &tile[16..24] {
            out.extend([plane2, 0]);
        }
    }
    out
}

/// Drops the fourth plane of 4bpp tile data, producing 3bpp.
pub fn convert_4bpp_to_3bpp(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 32 * 24);
    for tile in data.as_chunks::<32>().0 {
        out.extend_from_slice(&tile[..16]);
        out.extend(tile[16..32].iter().step_by(2));
    }
    out
}

#[derive(Debug, Error)]
pub enum GfxError {
    #[error("GFX file index {0:02X} is out of range (00 to 33)")]
    BadIndex(u8),
    #[error("tileset {0} has no GFX list entry (0 to 25)")]
    BadTileset(u8),
    #[error(
        "GFX{index:02X} decompressed to {len} bytes, which is not 2, 3, or 4bpp for {tiles} tiles"
    )]
    BadSize { index: u8, len: usize, tiles: usize },
    #[error(
        "the ROM is locked: its GFX pointers are not the files' addresses until the ROM's own code has changed them"
    )]
    Locked,
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("ExGFX{file:X} at {addr}: {source}")]
    ExGfx {
        file: u16,
        addr: SnesAddr,
        #[source]
        source: LzError,
    },
    #[error("GFX{index:02X} at {addr}: {source}")]
    Decompress {
        index: u8,
        addr: SnesAddr,
        #[source]
        source: LzError,
    },
}

/// How a GFX file's decompressed bytes are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GfxFormat {
    /// Planar 8x8 tiles at the given depth.
    Planar(Bpp),
    /// 8x8 tiles of 3-bit pixels packed most significant bit first, three
    /// bytes to a row of eight. `CODE_00AB42` unpacks `GFX27` a pixel to a
    /// byte into the high bytes of VRAM, where Mode 7 keeps its characters.
    Packed3,
}

/// Bytes of one tile in [`GfxFormat::Packed3`].
const PACKED3_BYTES_PER_TILE: usize = 24;

/// Number of 8x8 tiles in a GFX file. Most files hold 128, a few layer 3
/// files 64, and the two outside the pointer tables what their RAM buffers
/// (`$7E2000` and `$7E7D00`) take.
pub fn gfx_file_tile_count(index: u8) -> Result<usize, GfxError> {
    match index {
        0x2F..=0x31 => Ok(64),
        0x00..=0x31 => Ok(128),
        0x32 => Ok(744),
        0x33 => Ok(384),
        _ => Err(GfxError::BadIndex(index)),
    }
}

/// Infers the stored format from the decompressed size. `GFX27` is as long
/// as a 3bpp file and is told apart by its number.
pub fn infer_format(index: u8, len: usize) -> Result<GfxFormat, GfxError> {
    let tiles = gfx_file_tile_count(index)?;
    if index == 0x27 && len == tiles * PACKED3_BYTES_PER_TILE {
        return Ok(GfxFormat::Packed3);
    }
    match len.checked_div(tiles).filter(|_| len.is_multiple_of(tiles)) {
        Some(16) => Ok(GfxFormat::Planar(Bpp::Two)),
        Some(24) => Ok(GfxFormat::Planar(Bpp::Three)),
        Some(32) => Ok(GfxFormat::Planar(Bpp::Four)),
        _ => Err(GfxError::BadSize { index, len, tiles }),
    }
}

/// Whether the ROM's author has locked it with Lunar Magic. A locked ROM
/// has a hook, a `JSL`, at the start of [`DECOMPRESS`], and its pointer
/// tables do not hold the files' addresses. The ROM's own code still
/// loads its levels, which is all rendering needs; the files cannot be
/// read out by number, which is what the lock is for.
///
/// Only the hook's opcode is read (clean room): in the corpus the locked
/// ROMs, and no other, have one there, SA-1 Pack's included. Lunar
/// Magic's LC_LZ3 is hooked at `$00B8E3` instead, and Kobo's code calls
/// [`DECOMPRESS`] without changing it.
pub fn is_locked(rom: &Rom) -> bool {
    rom.read(DECOMPRESS, 1).is_ok_and(|op| op[0] == 0x22)
}

/// Address of the compressed data for a GFX file.
pub fn gfx_file_ptr(rom: &Rom, index: u8) -> Result<SnesAddr, GfxError> {
    if index >= GFX_FILE_COUNT {
        return Err(GfxError::BadIndex(index));
    }
    if is_locked(rom) {
        return Err(GfxError::Locked);
    }
    if index >= GFX_TABLE_FILES {
        let ptr = if index == 0x32 { GFX32_PTR } else { GFX33_PTR };
        // `LDA #` and `LDY #` operands: the address's low word.
        let bank = rom.read_u8(GFX32_33_BANK)?;
        return Ok(SnesAddr::new(
            ((bank as u32) << 16) | rom.read_u16(ptr)? as u32,
        ));
    }
    let i = index as u32;
    let lo = rom.read_u8(GFX_PTR_LO.add(i))?;
    let hi = rom.read_u8(GFX_PTR_HI.add(i))?;
    let bank = rom.read_u8(GFX_PTR_BANK.add(i))?;
    Ok(SnesAddr::new(
        ((bank as u32) << 16) | ((hi as u32) << 8) | lo as u32,
    ))
}

/// Lunar Magic 3.70's record of the format a ROM's GFX files are in, which
/// SA-1 Pack reads to choose its decompressor (docs/lunar-magic.md).
pub const COMPRESSION_SETTING: SnesAddr = SnesAddr::new(0x0FFFEB);
/// [`COMPRESSION_SETTING`] for LC_LZ3.
pub const COMPRESSION_LZ3: u8 = 0x02;
/// The last of Lunar Magic's settings bytes (`$0FFFE7`-`$0FFFFF`): with
/// `$FF` there it takes the settings as absent and a ROM's GFX as
/// LC_LZ2, whatever [`COMPRESSION_SETTING`] says; any other value, and it
/// reads that (observed with `-ExportGFX`, docs/lunar-magic-install.md).
pub const SETTINGS_PRESENT: SnesAddr = SnesAddr::new(0x0FFFFF);

/// How a ROM's GFX files are compressed. Lunar Magic changes all of them
/// at once and replaces the game's decompression routine to match.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Compression {
    Lz2,
    Lz3,
}

impl Compression {
    pub fn decompress(self, input: &[u8]) -> Result<Decompressed, LzError> {
        match self {
            Self::Lz2 => lz2::decompress(input),
            Self::Lz3 => lz3::decompress(input),
        }
    }

    /// The format a ROM's files are in. Nothing in the ROM states it (the
    /// routine Lunar Magic installs is its own), so this is the format in
    /// which more of the pointer tables' files come out whole. The two
    /// agree on a file that only copies and fills, which is why a count
    /// decides and not one file: in the corpus it is 46 to 50 of the 50
    /// for the format in use and at most 18 for the other.
    pub fn detect(rom: &Rom) -> Result<Self, GfxError> {
        let whole = |compression: Self| {
            (0..GFX_TABLE_FILES)
                .filter(|&index| decompress_file(rom, index, compression).is_ok())
                .count()
        };
        if is_locked(rom) {
            return Err(GfxError::Locked);
        }
        Ok(if whole(Self::Lz3) > whole(Self::Lz2) {
            Self::Lz3
        } else {
            Self::Lz2
        })
    }
}

impl std::fmt::Display for Compression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Lz2 => "LC_LZ2",
            Self::Lz3 => "LC_LZ3",
        })
    }
}

/// A decompressed GFX file in its stored format.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GfxFile {
    pub index: u8,
    pub format: GfxFormat,
    /// Bytes in the stored format.
    pub data: Vec<u8>,
    /// Where the compressed data was read from.
    pub addr: SnesAddr,
    /// Size of the compressed data including the terminator.
    pub compressed_len: usize,
    pub compression: Compression,
}

impl GfxFile {
    /// Bit depth for planar files, `None` for packed ones.
    pub fn bpp(&self) -> Option<Bpp> {
        match self.format {
            GfxFormat::Planar(bpp) => Some(bpp),
            GfxFormat::Packed3 => None,
        }
    }

    /// Colours a tile of this file can hold.
    pub fn colors(&self) -> usize {
        self.bpp().map_or(8, Bpp::colors)
    }

    pub fn tile_count(&self) -> usize {
        self.data.len()
            / self
                .bpp()
                .map_or(PACKED3_BYTES_PER_TILE, Bpp::bytes_per_tile)
    }

    pub fn tiles(&self) -> Vec<Tile8> {
        match self.format {
            GfxFormat::Planar(bpp) => decode_tiles(bpp, &self.data),
            GfxFormat::Packed3 => decode_packed3_tiles(&self.data),
        }
    }

    /// The bytes Lunar Magic's `-ExportGFX` would write for this file.
    ///
    /// 3bpp files are converted to 4bpp. Tiles listed by
    /// [`upper_palette_tiles`] get their fourth plane set to the tile's
    /// silhouette so they use colours 8 to 15 of their palette row.
    pub fn to_lm_export(&self) -> Vec<u8> {
        match self.format {
            GfxFormat::Planar(Bpp::Three) => {
                let mut out = convert_3bpp_to_4bpp(&self.data);
                for tile in upper_palette_tiles(self.index, self.tile_count()) {
                    let t = &mut out[tile * 32..tile * 32 + 32];
                    for y in 0..8 {
                        t[16 + 2 * y + 1] = t[2 * y] | t[2 * y + 1] | t[16 + 2 * y];
                    }
                }
                out
            }
            GfxFormat::Planar(_) | GfxFormat::Packed3 => self.data.clone(),
        }
    }
}

/// Tiles of a 3bpp file whose fourth bit plane Lunar Magic sets to the
/// tile's silhouette on export, so they use colours 8 to 15 of their
/// palette row.
///
/// The game does this itself when uploading to VRAM (`UploadGFXFile` in
/// bank `$00`): for files `01`, `17`, and `31` the first 16x16 block
/// (tiles 0, 1, 16, 17) is flagged, which is the berry using the colours
/// loaded from `BerryColors`; `GFX1E` is flagged in full, as is `GFX08`
/// when the object tileset is `$11` or above. Lunar Magic's export matches
/// that for `01`, `31`, and `1E`, skips `17`, and flags a fixed subset of
/// `GFX08`. This function reproduces Lunar Magic's export behaviour; use
/// [`vram_upper_palette_tiles`] for what the game puts in VRAM.
pub fn upper_palette_tiles(index: u8, tile_count: usize) -> Vec<usize> {
    const GFX08: [usize; 24] = [
        55, 56, 57, 58, 59, 71, 72, 73, 74, 75, 86, 87, 88, 89, 90, 91, 96, 110, 111, 112, 122,
        123, 126, 127,
    ];
    match index {
        0x01 | 0x31 => vec![0, 1, 16, 17],
        0x08 => GFX08.to_vec(),
        0x1E => (0..tile_count).collect(),
        _ => Vec::new(),
    }
}

/// Tiles the game uploads with the fourth plane set to the silhouette,
/// per `UploadGFXFile`. `tileset` is the level's object tileset.
pub fn vram_upper_palette_tiles(index: u8, tileset: u8, tile_count: usize) -> Vec<usize> {
    match index {
        0x01 | 0x17 | 0x31 => vec![0, 1, 16, 17],
        0x1E => (0..tile_count).collect(),
        0x08 if tileset >= 0x11 => (0..tile_count).collect(),
        _ => Vec::new(),
    }
}

/// Reads and decompresses GFX file `index` (`00` to `33`), in whichever
/// format the ROM's files are in.
pub fn read_gfx_file(rom: &Rom, index: u8) -> Result<GfxFile, GfxError> {
    GfxReader::new(rom)?.read(index)
}

/// Reads multiple files from one immutable ROM, detecting its compression
/// once. Reuse this for exports, tile sets, and file browsers.
pub struct GfxReader<'r> {
    rom: &'r Rom,
    compression: Compression,
}

impl<'r> GfxReader<'r> {
    pub fn new(rom: &'r Rom) -> Result<Self, GfxError> {
        Ok(Self {
            rom,
            compression: Compression::detect(rom)?,
        })
    }

    pub fn compression(&self) -> Compression {
        self.compression
    }

    pub fn read(&self, index: u8) -> Result<GfxFile, GfxError> {
        decompress_file(self.rom, index, self.compression)
    }
}

fn decompress_file(rom: &Rom, index: u8, compression: Compression) -> Result<GfxFile, GfxError> {
    let addr = gfx_file_ptr(rom, index)?;
    let input = rom.read_tail(addr)?;
    let d = compression
        .decompress(input)
        .map_err(|source| GfxError::Decompress {
            index,
            addr,
            source,
        })?;
    let format = infer_format(index, d.data.len())?;
    Ok(GfxFile {
        index,
        format,
        data: d.data,
        addr,
        compressed_len: d.consumed,
        compression,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_outside_the_file_return_errors() {
        let mut bytes = vec![0; 0x8000];
        bytes[0x7FD5] = 0x20;
        bytes[0x39C4] = 0x80;
        bytes[0x39F6] = 2;
        let rom = Rom::from_bytes(bytes).unwrap();
        // Detection probes every pointer and must tolerate corrupt ones too.
        let reader = GfxReader::new(&rom).unwrap();
        assert!(matches!(
            reader.read(0),
            Err(GfxError::Rom(RomError::OutOfBounds { .. }))
        ));
    }

    // A tile whose pixel value equals (x + y) & mask, for testing planes.
    fn gradient(mask: u8) -> Tile8 {
        let mut t = Tile8::default();
        for (y, row) in t.pixels.iter_mut().enumerate() {
            for (x, px) in row.iter_mut().enumerate() {
                *px = ((x + y) as u8) & mask;
            }
        }
        t
    }

    #[test]
    fn decode_2bpp_single_pixels() {
        // Row 0: leftmost pixel has plane 0 set; row 1: rightmost has plane 1.
        let mut bytes = [0u8; 16];
        bytes[0] = 0x80;
        bytes[3] = 0x01;
        let t = Tile8::decode(Bpp::Two, &bytes);
        assert_eq!(t.pixels[0][0], 1);
        assert_eq!(t.pixels[1][7], 2);
        assert_eq!(t.pixels.iter().flatten().filter(|&&p| p != 0).count(), 2);
    }

    #[test]
    fn decode_3bpp_third_plane() {
        let mut bytes = [0u8; 24];
        bytes[16 + 5] = 0x10; // row 5, x = 3, plane 2
        let t = Tile8::decode(Bpp::Three, &bytes);
        assert_eq!(t.pixels[5][3], 4);
    }

    #[test]
    fn decode_4bpp_fourth_plane() {
        let mut bytes = [0u8; 32];
        bytes[16 + 2 * 7 + 1] = 0x01; // row 7, x = 7, plane 3
        bytes[16 + 2 * 7] = 0x01; // plane 2
        bytes[2 * 7] = 0x01; // plane 0
        let t = Tile8::decode(Bpp::Four, &bytes);
        assert_eq!(t.pixels[7][7], 0b1101);
    }

    #[test]
    fn tiles_encode_and_image_round_trip() {
        let mut tiles = Vec::new();
        for n in 0..20u8 {
            let mut t = Tile8::default();
            for y in 0..8 {
                for x in 0..8 {
                    t.pixels[y][x] = (n + y as u8 * 3 + x as u8) % 8;
                }
            }
            tiles.push(t);
        }
        for format in [GfxFormat::Planar(Bpp::Three), GfxFormat::Packed3] {
            let bytes = format.encode(&tiles);
            let back = match format {
                GfxFormat::Planar(bpp) => decode_tiles(bpp, &bytes),
                GfxFormat::Packed3 => decode_packed3_tiles(&bytes),
            };
            assert_eq!(back, tiles, "{format:?}");
        }
        let image = tiles_to_image(&tiles, 8);
        let png = image.to_png().unwrap();
        let read = crate::image::IndexedImage::from_png(&png).unwrap();
        assert_eq!(read, image);
        assert_eq!(image_to_tiles(&read, 20, 8).unwrap(), tiles);
        assert!(image_to_tiles(&read, 20, 4).is_err());
    }

    #[test]
    fn packed3_pixels_run_across_byte_boundaries() {
        let mut bytes = [0u8; 24];
        // Row 0 holds pixels 0 to 7 in order (octal 01234567); row 7 ends
        // on a 5.
        bytes[..3].copy_from_slice(&0o01234567u32.to_be_bytes()[1..]);
        bytes[23] = 0b101;
        let tiles = decode_packed3_tiles(&bytes);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].pixels[0], [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(tiles[0].pixels[7], [0, 0, 0, 0, 0, 0, 0, 5]);
    }

    #[test]
    fn encode_decode_round_trips() {
        for (bpp, mask) in [(Bpp::Two, 3), (Bpp::Three, 7), (Bpp::Four, 15)] {
            let t = gradient(mask);
            let bytes = t.encode(bpp);
            assert_eq!(bytes.len(), bpp.bytes_per_tile());
            assert_eq!(Tile8::decode(bpp, &bytes), t, "{bpp:?}");
        }
    }

    #[test]
    fn conversion_3bpp_4bpp_round_trips() {
        let t = gradient(7);
        let three = t.encode(Bpp::Three);
        let four = convert_3bpp_to_4bpp(&three);
        assert_eq!(four, t.encode(Bpp::Four));
        assert_eq!(convert_4bpp_to_3bpp(&four), three);
    }

    #[test]
    fn a_locked_rom_has_no_gfx_pointers() {
        let mut data = vec![0u8; 0x10000];
        data[0x7FC0 + 0x15] = 0x20;
        let plain = Rom::from_bytes(data.clone()).unwrap();
        assert!(!is_locked(&plain));
        assert!(!matches!(gfx_file_ptr(&plain, 0), Err(GfxError::Locked)));
        // `JSL $019000` at `$00B8DE`.
        data[0x38DE..0x38E2].copy_from_slice(&[0x22, 0x00, 0x90, 0x01]);
        let locked = Rom::from_bytes(data).unwrap();
        assert!(is_locked(&locked));
        assert!(matches!(gfx_file_ptr(&locked, 0), Err(GfxError::Locked)));
    }

    #[test]
    fn format_inference() {
        use GfxFormat::{Packed3, Planar};
        assert_eq!(infer_format(0x00, 3072).unwrap(), Planar(Bpp::Three));
        assert_eq!(infer_format(0x00, 4096).unwrap(), Planar(Bpp::Four));
        assert_eq!(infer_format(0x28, 2048).unwrap(), Planar(Bpp::Two));
        assert_eq!(infer_format(0x27, 3072).unwrap(), Packed3);
        assert_eq!(infer_format(0x32, 23808).unwrap(), Planar(Bpp::Four));
        assert_eq!(infer_format(0x33, 9216).unwrap(), Planar(Bpp::Three));
        assert_eq!(infer_format(0x2F, 1024).unwrap(), Planar(Bpp::Two));
        assert_eq!(infer_format(0x30, 1536).unwrap(), Planar(Bpp::Three));
        assert!(matches!(
            infer_format(0x00, 3000),
            Err(GfxError::BadSize {
                index: 0,
                len: 3000,
                tiles: 128
            })
        ));
        assert!(matches!(
            infer_format(0x34, 4096),
            Err(GfxError::BadIndex(0x34))
        ));
    }
}
