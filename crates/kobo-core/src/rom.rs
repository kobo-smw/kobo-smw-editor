//! Loading, identifying, and writing SMW ROM images.
//!
//! A [`Rom`] holds the headerless image. Copier headers (the 512-byte prefix
//! some dumps carry) are stripped on load and never written back: a saved
//! image is headerless. All reads and writes take SNES addresses and go
//! through the ROM's [`Mapping`].

use std::fmt;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::addr::{MapError, Mapping, PcAddr, SnesAddr};

/// Size of the optional copier header some dumps carry.
pub const COPIER_HEADER_LEN: usize = 512;

/// ROM bank size. Every valid image is a multiple of this.
pub const BANK_LEN: usize = 0x8000;

/// Offset of the internal header for LoROM and SA-1 images.
const INTERNAL_HEADER: PcAddr = PcAddr::new(0x7FC0);

/// The internal header's ROM size code.
const ROM_SIZE_CODE: SnesAddr = SnesAddr::new(0x00FFD7);

/// The internal header's checksum complement, followed by the checksum.
const CHECKSUM_COMPLEMENT: SnesAddr = SnesAddr::new(0x00FFDC);

/// What an image is expanded in: the tools only ever expand to multiples
/// of it, and a smaller step would leave a size whose checksum the
/// console does not define.
const EXPANSION_STEP: usize = 0x8_0000;

/// SHA-1 of the headerless No-Intro "Super Mario World (USA)" image.
pub const VANILLA_USA_SHA1: [u8; 20] = [
    0x6b, 0x47, 0xbb, 0x75, 0xd1, 0x65, 0x14, 0xb6, 0xa4, 0x76, 0xaa, 0x0c, 0x73, 0xa6, 0x83, 0xa2,
    0xa4, 0xc1, 0x87, 0x65,
];

#[derive(Debug, Error)]
pub enum RomError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("ROM size {0} is not a multiple of 32 KiB, with or without a copier header")]
    BadSize(usize),
    #[error("ROM is too small to contain an internal header")]
    TooSmall,
    #[error("unsupported cartridge mapping (map mode ${0:02X})")]
    UnsupportedMapping(u8),
    #[error("invalid {kind} size code ${code:02X}")]
    InvalidSizeCode { kind: &'static str, code: u8 },
    #[error(transparent)]
    Map(#[from] MapError),
    #[error("{len} bytes at {addr} ({pc}) run past the end of the ROM")]
    OutOfBounds {
        addr: SnesAddr,
        pc: PcAddr,
        len: usize,
    },
    #[error("cannot expand a {from}-byte ROM to {to} bytes: {why}")]
    Expand {
        from: usize,
        to: usize,
        why: &'static str,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// What a loaded ROM was recognised as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RomIdentity {
    /// Byte-identical to the vanilla USA release.
    VanillaUsa,
    /// Not a known vanilla image. Most likely a hack or an expanded ROM.
    Unknown,
}

/// The internal header at `$00FFC0`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InternalHeader {
    /// 21-character title, trailing spaces trimmed.
    pub title: String,
    pub map_mode: u8,
    pub cartridge_type: u8,
    pub rom_size_code: u8,
    pub sram_size_code: u8,
    pub region: u8,
    pub developer_id: u8,
    pub version: u8,
    pub checksum_complement: u16,
    pub checksum: u16,
}

impl InternalHeader {
    fn parse(bytes: &[u8]) -> Self {
        let title = String::from_utf8_lossy(&bytes[0x00..0x15])
            .trim_end()
            .to_string();
        Self {
            title,
            map_mode: bytes[0x15],
            cartridge_type: bytes[0x16],
            rom_size_code: bytes[0x17],
            sram_size_code: bytes[0x18],
            region: bytes[0x19],
            developer_id: bytes[0x1A],
            version: bytes[0x1B],
            checksum_complement: u16::from_le_bytes([bytes[0x1C], bytes[0x1D]]),
            checksum: u16::from_le_bytes([bytes[0x1E], bytes[0x1F]]),
        }
    }

    /// Declared ROM size in bytes.
    pub fn rom_size(&self) -> Result<usize, RomError> {
        Self::declared_size("ROM", self.rom_size_code)
    }

    /// Declared SRAM size in bytes.
    pub fn sram_size(&self) -> Result<usize, RomError> {
        if self.sram_size_code == 0 {
            Ok(0)
        } else {
            Self::declared_size("SRAM", self.sram_size_code)
        }
    }

    fn declared_size(kind: &'static str, code: u8) -> Result<usize, RomError> {
        1usize
            .checked_shl(code as u32 + 10)
            .ok_or(RomError::InvalidSizeCode { kind, code })
    }

    /// Whether the checksum and its complement agree with each other.
    pub fn checksum_pair_valid(&self) -> bool {
        self.checksum ^ self.checksum_complement == 0xFFFF
    }
}

pub struct Rom {
    data: Vec<u8>,
    copier_header: Option<Vec<u8>>,
    mapping: Mapping,
    source: Option<PathBuf>,
}

impl fmt::Debug for Rom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rom")
            .field("len", &self.data.len())
            .field("copier_header", &self.copier_header.is_some())
            .field("mapping", &self.mapping)
            .field("source", &self.source)
            .finish()
    }
}

impl Rom {
    /// Loads a ROM image from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RomError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| RomError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut rom = Self::from_bytes(bytes)?;
        rom.source = Some(path.to_path_buf());
        Ok(rom)
    }

    /// Builds a ROM from raw file contents, stripping a copier header if
    /// one is present.
    pub fn from_bytes(mut bytes: Vec<u8>) -> Result<Self, RomError> {
        let copier_header = match bytes.len() % BANK_LEN {
            0 => None,
            COPIER_HEADER_LEN => {
                let header = bytes.drain(..COPIER_HEADER_LEN).collect();
                Some(header)
            }
            _ => return Err(RomError::BadSize(bytes.len())),
        };
        Self::new(bytes, copier_header)
    }

    /// Builds a ROM from an image known to have no copier header, such as
    /// a tool's output: a length of a whole number of banks plus 512
    /// bytes is a size error, not a header.
    pub fn from_headerless(bytes: Vec<u8>) -> Result<Self, RomError> {
        if !bytes.len().is_multiple_of(BANK_LEN) {
            return Err(RomError::BadSize(bytes.len()));
        }
        Self::new(bytes, None)
    }

    fn new(bytes: Vec<u8>, copier_header: Option<Vec<u8>>) -> Result<Self, RomError> {
        if bytes.len() < INTERNAL_HEADER.as_usize() + 0x20 {
            return Err(RomError::TooSmall);
        }
        let map_mode = bytes[INTERNAL_HEADER.as_usize() + 0x15];
        let mapping = Mapping::from_map_mode(map_mode, bytes.len())
            .ok_or(RomError::UnsupportedMapping(map_mode))?;
        let rom = Self {
            data: bytes,
            copier_header,
            mapping,
            source: None,
        };
        crate::clean_room::check(&rom);
        Ok(rom)
    }

    /// The headerless image.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Size of the headerless image in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn has_copier_header(&self) -> bool {
        self.copier_header.is_some()
    }

    /// The copier header the image was loaded with, if it had one.
    pub fn copier_header(&self) -> Option<&[u8]> {
        self.copier_header.as_deref()
    }

    pub fn mapping(&self) -> Mapping {
        self.mapping
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn internal_header(&self) -> InternalHeader {
        let start = INTERNAL_HEADER.as_usize();
        InternalHeader::parse(&self.data[start..start + 0x20])
    }

    /// Computes the checksum the internal header should hold for this image.
    ///
    /// For sizes that are not a power of two, the tail past the largest
    /// power of two is counted repeatedly, as if mirrored up to that size.
    pub fn compute_checksum(&self) -> u16 {
        let len = self.data.len();
        let sum = |slice: &[u8]| {
            slice
                .iter()
                .fold(0u16, |sum, &b| sum.wrapping_add(b as u16))
        };
        let head_len = if len.is_power_of_two() {
            len
        } else {
            1usize << (usize::BITS - 1 - len.leading_zeros())
        };
        let (head, tail) = self.data.split_at(head_len);
        let mut total = sum(head);
        if !tail.is_empty() {
            let repeats = (head_len / tail.len()) as u16;
            total = total.wrapping_add(sum(tail).wrapping_mul(repeats));
        }
        total
    }

    pub fn sha1(&self) -> [u8; 20] {
        Sha1::digest(&self.data).into()
    }

    pub fn sha1_hex(&self) -> String {
        self.sha1().iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn identify(&self) -> RomIdentity {
        if self.sha1() == VANILLA_USA_SHA1 {
            RomIdentity::VanillaUsa
        } else {
            RomIdentity::Unknown
        }
    }

    /// The Lunar Magic version that last saved this ROM, from the marker
    /// Lunar Magic writes at `$0FF0A0` ("Lunar Magic Version 3.21 ...").
    pub fn lunar_magic_version(&self) -> Option<String> {
        const MARKER: SnesAddr = SnesAddr::new(0x0FF0A0);
        const PREFIX: &[u8] = b"Lunar Magic Version ";
        let bytes = self.read(MARKER, 40).ok()?;
        let rest = bytes.strip_prefix(PREFIX)?;
        let end = rest
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b'.'))
            .unwrap_or(rest.len());
        (end > 0).then(|| String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    /// Whether a Lunar Magic older than `version` (major, and minor in
    /// hundredths: `(1, 90)`) last saved this ROM, by its version string.
    /// Kobo's builds have none, and count as the newest.
    pub fn saved_by_lunar_magic_before(&self, version: (u32, u32)) -> bool {
        self.lunar_magic_version()
            .and_then(|v| crate::entrance::parse_version(&v))
            .is_some_and(|v| v < version)
    }

    pub fn pc(&self, addr: SnesAddr) -> Result<PcAddr, MapError> {
        self.mapping.snes_to_pc(addr)
    }

    /// The file range of `len` bytes from a SNES address. They are
    /// contiguous in the file, whether or not they cross a bank.
    fn span(&self, addr: SnesAddr, len: usize) -> Result<Range<usize>, RomError> {
        let pc = self.pc(addr)?;
        let start = pc.as_usize();
        start
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .map(|end| start..end)
            .ok_or(RomError::OutOfBounds { addr, pc, len })
    }

    /// Reads `len` bytes starting at a SNES address.
    pub fn read(&self, addr: SnesAddr, len: usize) -> Result<&[u8], RomError> {
        Ok(&self.data[self.span(addr, len)?])
    }

    /// The remaining file bytes from a mapped address, checked against
    /// the actual image length. Useful for terminated compressed streams.
    pub fn read_tail(&self, addr: SnesAddr) -> Result<&[u8], RomError> {
        let pc = self.pc(addr)?;
        self.data
            .get(pc.as_usize()..)
            .ok_or(RomError::OutOfBounds { addr, pc, len: 0 })
    }

    pub fn read_u8(&self, addr: SnesAddr) -> Result<u8, RomError> {
        Ok(self.read(addr, 1)?[0])
    }

    pub fn read_u16(&self, addr: SnesAddr) -> Result<u16, RomError> {
        let b = self.read(addr, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    /// Reads a 24-bit little-endian value, typically a long pointer.
    pub fn read_u24(&self, addr: SnesAddr) -> Result<u32, RomError> {
        let b = self.read(addr, 3)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], 0]))
    }

    /// Reads a 24-bit pointer and wraps it as an address.
    pub fn read_ptr(&self, addr: SnesAddr) -> Result<SnesAddr, RomError> {
        Ok(SnesAddr::new(self.read_u24(addr)?))
    }

    /// Writes bytes starting at a SNES address, contiguously in the file
    /// as [`Rom::read`] reads them. Nothing is written if any of them
    /// would land past the end of the image.
    pub fn write(&mut self, addr: SnesAddr, bytes: &[u8]) -> Result<(), RomError> {
        let span = self.span(addr, bytes.len())?;
        self.data[span].copy_from_slice(bytes);
        Ok(())
    }

    pub fn write_u8(&mut self, addr: SnesAddr, value: u8) -> Result<(), RomError> {
        self.write(addr, &[value])
    }

    pub fn write_u16(&mut self, addr: SnesAddr, value: u16) -> Result<(), RomError> {
        self.write(addr, &value.to_le_bytes())
    }

    /// Writes the low 24 bits of a value, little-endian.
    pub fn write_u24(&mut self, addr: SnesAddr, value: u32) -> Result<(), RomError> {
        self.write(addr, &value.to_le_bytes()[..3])
    }

    /// Writes a 24-bit pointer to an address.
    pub fn write_ptr(&mut self, addr: SnesAddr, target: SnesAddr) -> Result<(), RomError> {
        self.write_u24(addr, target.raw())
    }

    /// Expands the image to `len` bytes, as Asar does: the new space is
    /// `$00`, which every tool takes for free space, and the size code at
    /// `$00FFD7` declares the size rounded up to a power of two. The size
    /// is a multiple of 512 KiB that is a power of two or the sum of two
    /// (3 MiB, 6 MiB), the sizes whose checksum the console defines, and
    /// within what the mapping addresses. An SA-1 image crosses 4 MiB
    /// only through SA-1 Pack's 6 and 8 MiB patches, which set the
    /// cartridge up for it. Expanding to the current size changes nothing.
    pub fn expand(&mut self, len: usize) -> Result<(), RomError> {
        let from = self.data.len();
        let refuse = |why| Err(RomError::Expand { from, to: len, why });
        if len < from {
            return refuse("an image never shrinks");
        }
        if len == from {
            return Ok(());
        }
        if !len.is_multiple_of(EXPANSION_STEP) || len.count_ones() > 2 {
            return refuse(
                "the size must be a multiple of 512 KiB made of at most two powers of two",
            );
        }
        if self.mapping == Mapping::Sa1Rom && len > Mapping::Sa1Rom.max_rom_len() {
            return refuse("SA-1 Pack's 6 or 8 MiB patch takes an SA-1 image past 4 MiB");
        }
        if len > self.mapping.max_rom_len() {
            return refuse("the mapping does not address that much");
        }
        self.data.resize(len, 0x00);
        let code = len.next_power_of_two().trailing_zeros() - 10;
        self.write_u8(ROM_SIZE_CODE, code as u8)
    }

    /// Writes the checksum and its complement into the internal header.
    /// The pair is reset to `$FFFF` and `$0000` first, as Asar does, so
    /// the result does not depend on what it held before; the two always
    /// add the same to the sum.
    pub fn fix_checksum(&mut self) -> Result<(), RomError> {
        self.write(CHECKSUM_COMPLEMENT, &[0xFF, 0xFF, 0x00, 0x00])?;
        let checksum = self.compute_checksum();
        self.write_u16(CHECKSUM_COMPLEMENT, !checksum)?;
        self.write_u16(CHECKSUM_COMPLEMENT.add(2), checksum)
    }

    /// Writes the headerless image to disk.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), RomError> {
        let path = path.as_ref();
        fs::write(path, &self.data).map_err(|source| RomError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal 32 KiB image with a plausible LoROM header.
    fn fake_rom(map_mode: u8) -> Vec<u8> {
        let mut data = vec![0u8; BANK_LEN];
        let h = INTERNAL_HEADER.as_usize();
        data[h..h + 0x15].copy_from_slice(b"FAKE ROM             ");
        data[h + 0x15] = map_mode;
        data[h + 0x17] = 0x05;
        data
    }

    #[test]
    fn strips_copier_header() {
        let mut bytes = vec![0xAAu8; COPIER_HEADER_LEN];
        bytes.extend(fake_rom(0x20));
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(rom.has_copier_header());
        assert_eq!(rom.len(), BANK_LEN);
        assert_eq!(rom.internal_header().title, "FAKE ROM");
    }

    #[test]
    fn accepts_headerless() {
        let rom = Rom::from_bytes(fake_rom(0x20)).unwrap();
        assert!(!rom.has_copier_header());
        assert_eq!(rom.mapping(), Mapping::LoRom);
    }

    #[test]
    fn detects_sa1() {
        let rom = Rom::from_bytes(fake_rom(0x23)).unwrap();
        assert_eq!(rom.mapping(), Mapping::Sa1Rom);
    }

    #[test]
    fn rejects_odd_sizes() {
        assert!(matches!(
            Rom::from_bytes(vec![0; 1000]),
            Err(RomError::BadSize(1000))
        ));
    }

    #[test]
    fn rejects_hirom() {
        assert!(matches!(
            Rom::from_bytes(fake_rom(0x21)),
            Err(RomError::UnsupportedMapping(0x21))
        ));
    }

    #[test]
    fn malformed_sizes_and_overflowing_reads_are_errors() {
        let mut data = fake_rom(0x20);
        data[0x7FD7] = 0xFF;
        data[0x7FD8] = 0xFF;
        let rom = Rom::from_bytes(data).unwrap();
        assert!(matches!(
            rom.internal_header().rom_size(),
            Err(RomError::InvalidSizeCode { .. })
        ));
        assert!(matches!(
            rom.internal_header().sram_size(),
            Err(RomError::InvalidSizeCode { .. })
        ));
        assert!(matches!(
            rom.read(SnesAddr::new(0x008001), usize::MAX),
            Err(RomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            rom.read_tail(SnesAddr::new(0x028000)),
            Err(RomError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn reads_go_through_mapping() {
        let mut data = fake_rom(0x20);
        data[0x1234] = 0x42;
        data[0x1235] = 0x43;
        data[0x1236] = 0x44;
        let rom = Rom::from_bytes(data).unwrap();
        let addr = SnesAddr::new(0x009234);
        assert_eq!(rom.read_u8(addr).unwrap(), 0x42);
        assert_eq!(rom.read_u16(addr).unwrap(), 0x4342);
        assert_eq!(rom.read_u24(addr).unwrap(), 0x444342);
        assert!(matches!(
            rom.read_u8(SnesAddr::new(0x018000)),
            Err(RomError::OutOfBounds { .. })
        ));
        assert!(matches!(
            rom.read_u8(SnesAddr::new(0x000000)),
            Err(RomError::Map(_))
        ));
    }

    #[test]
    fn writes_go_through_mapping() {
        let mut data = fake_rom(0x20);
        data.resize(4 * BANK_LEN, 0);
        let mut rom = Rom::from_bytes(data).unwrap();
        let addr = SnesAddr::new(0x81FFFE);
        rom.write_u16(addr, 0xBEEF).unwrap();
        assert_eq!(&rom.data()[0xFFFE..0x10000], &[0xEF, 0xBE]);
        rom.write_ptr(SnesAddr::new(0x038000), SnesAddr::new(0x1C8123))
            .unwrap();
        assert_eq!(&rom.data()[0x18000..0x18003], &[0x23, 0x81, 0x1C]);
        assert_eq!(
            rom.read_ptr(SnesAddr::new(0x038000)).unwrap(),
            SnesAddr::new(0x1C8123)
        );
        // A write that would run off the end leaves the image alone.
        assert!(matches!(
            rom.write(SnesAddr::new(0x03FFFF), &[1, 2]),
            Err(RomError::OutOfBounds { len: 2, .. })
        ));
        assert_eq!(rom.data()[0x1FFFF], 0);
        assert!(matches!(
            rom.write_u8(SnesAddr::new(0x7E0000), 1),
            Err(RomError::Map(_))
        ));
    }

    #[test]
    fn expansion_pads_with_zeros_and_declares_the_size() {
        const MIB: usize = 0x10_0000;
        let mut data = fake_rom(0x20);
        data.resize(MIB / 2, 0xFF);
        let mut rom = Rom::from_bytes(data).unwrap();
        rom.expand(MIB / 2).unwrap();
        assert_eq!(rom.internal_header().rom_size_code, 0x05);
        for (len, code) in [(MIB, 0x0A), (3 * MIB, 0x0C), (4 * MIB, 0x0C)] {
            rom.expand(len).unwrap();
            assert_eq!(rom.len(), len);
            assert_eq!(rom.internal_header().rom_size_code, code);
        }
        assert!(rom.data()[MIB / 2..].iter().all(|&b| b == 0));
        assert_eq!(rom.data()[MIB / 2 - 1], 0xFF);
        let refused = |rom: &mut Rom, len| matches!(rom.expand(len), Err(RomError::Expand { .. }));
        assert!(refused(&mut rom, 2 * MIB));
        assert!(refused(&mut rom, 6 * MIB));

        let mut rom = Rom::from_bytes(fake_rom(0x20)).unwrap();
        assert!(refused(&mut rom, MIB + BANK_LEN));
        assert!(refused(&mut rom, 7 * MIB / 2));
        rom.expand(3 * MIB / 2).unwrap();
        assert_eq!(rom.internal_header().rom_size_code, 0x0B);
    }

    #[test]
    fn sa1_expansion_stops_at_four_mib() {
        const MIB: usize = 0x10_0000;
        let mut rom = Rom::from_bytes(fake_rom(0x23)).unwrap();
        rom.expand(4 * MIB).unwrap();
        assert!(matches!(rom.expand(8 * MIB), Err(RomError::Expand { .. })));
        // Past SA-1 Pack's 6 MiB patch, up to 8 MiB.
        let mut data = fake_rom(0x23);
        data.resize(6 * MIB, 0);
        let mut rom = Rom::from_bytes(data).unwrap();
        assert_eq!(rom.mapping(), Mapping::BigSa1Rom);
        rom.expand(8 * MIB).unwrap();
        assert_eq!(rom.internal_header().rom_size_code, 0x0D);
    }

    #[test]
    fn fixed_checksum_is_valid_and_stable() {
        let mut data = fake_rom(0x20);
        data[0x100] = 0x5A;
        data[0x7FDC..0x7FE0].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let mut rom = Rom::from_bytes(data).unwrap();
        rom.expand(0x18_0000).unwrap();
        rom.fix_checksum().unwrap();
        let h = rom.internal_header();
        assert!(h.checksum_pair_valid());
        assert_eq!(h.checksum, rom.compute_checksum());
        let before = rom.data().to_vec();
        rom.fix_checksum().unwrap();
        assert_eq!(rom.data(), &before[..]);
    }

    #[test]
    fn checksum_power_of_two() {
        let mut data = fake_rom(0x20);
        data[0] = 0xFF;
        data[1] = 0x02;
        let rom = Rom::from_bytes(data).unwrap();
        let expected: u32 = rom.data().iter().map(|&b| b as u32).sum();
        assert_eq!(rom.compute_checksum(), expected as u16);
    }

    #[test]
    fn checksum_mirrors_tail() {
        // 3 banks: 2 banks of head, 1 bank of tail counted twice.
        let mut data = fake_rom(0x20);
        data.extend(vec![0u8; BANK_LEN]);
        data.extend(vec![1u8; BANK_LEN]);
        let rom = Rom::from_bytes(data).unwrap();
        let head: u32 = rom.data()[..2 * BANK_LEN].iter().map(|&b| b as u32).sum();
        let expected = head + 2 * BANK_LEN as u32;
        assert_eq!(rom.compute_checksum(), expected as u16);
    }

    #[test]
    fn the_lunar_magic_version_that_saved_a_rom() {
        let mut data = fake_rom(0x20);
        data.resize(16 * BANK_LEN, 0);
        let rom = |text: &[u8]| {
            let mut data = data.clone();
            data[0x7_F0A0..0x7_F0A0 + text.len()].copy_from_slice(text);
            Rom::from_bytes(data).unwrap()
        };
        let old = rom(b"Lunar Magic Version 1.62 Public");
        assert_eq!(old.lunar_magic_version().as_deref(), Some("1.62"));
        assert!(old.saved_by_lunar_magic_before((1, 90)));
        assert!(!old.saved_by_lunar_magic_before((1, 62)));
        assert!(!rom(b"Lunar Magic Version 3.70").saved_by_lunar_magic_before((1, 90)));
        // Kobo's builds have no version string.
        assert!(!rom(b"").saved_by_lunar_magic_before((9, 99)));
    }
}
