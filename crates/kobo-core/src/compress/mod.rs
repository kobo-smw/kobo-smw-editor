//! Compression formats used inside the ROM: LC_LZ2, which the game and
//! most hacks use, and LC_LZ3, which Lunar Magic can store a hack's
//! graphics in instead.
//!
//! Both are a sequence of chunks, each starting with a header byte: the
//! top three bits are the command and the low five bits are the length
//! minus one. A command of 7 marks a long header: the command moves to
//! bits 2 to 4 and the length becomes ten bits spanning the low two bits
//! and the following byte. A header byte of `$FF` ends the stream. The
//! formats differ in what commands 3 and up do.
//!
//! LC_RLE1, the simpler format of background tilemaps, is [`rle1`]. LC_LZ2 and LC_RLE1
//! have encoders as well, and LC_LZ3 one for Kobo's SA-1 builds that store GFX in it.

use thiserror::Error;

pub mod lz2;
pub mod lz3;
pub mod rle1;

/// Output offsets are 16-bit, so no stream can address more than this.
pub const MAX_OUTPUT: usize = 0x1_0000;

/// The longest chunk a short header can hold, and a long one.
const SHORT_LEN: usize = 32;
const LONG_LEN: usize = 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LzError {
    #[error("compressed data ends at offset {0} without a terminator")]
    Truncated(usize),
    #[error("unknown command {cmd} at offset {offset}")]
    BadCommand { cmd: u8, offset: usize },
    #[error(
        "back-reference to output offset {src} at input offset {offset} points outside the {len} bytes produced so far"
    )]
    BadBackRef {
        src: usize,
        len: usize,
        offset: usize,
    },
    #[error("decompressed output exceeds {MAX_OUTPUT} bytes")]
    TooLarge,
    #[error("{0} bytes is more than one stream can hold ({MAX_OUTPUT})")]
    InputTooLarge(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decompressed {
    pub data: Vec<u8>,
    /// Number of input bytes consumed, including the terminator.
    pub consumed: usize,
}

/// A stream being read, chunk by chunk.
struct Reader<'a> {
    input: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, LzError> {
        let b = self
            .input
            .get(self.pos)
            .copied()
            .ok_or(LzError::Truncated(self.pos))?;
        self.pos += 1;
        Ok(b)
    }

    fn bytes(&mut self, len: usize) -> Result<&[u8], LzError> {
        let src = self
            .input
            .get(self.pos..self.pos + len)
            .ok_or(LzError::Truncated(self.input.len()))?;
        self.pos += len;
        Ok(src)
    }

    /// The next chunk's command and length, or `None` at the terminator.
    fn chunk(&mut self) -> Result<Option<(u8, usize)>, LzError> {
        let header = self.byte()?;
        if header == 0xFF {
            return Ok(None);
        }
        Ok(Some(if header >> 5 == 7 {
            let len = (((header & 0x03) as usize) << 8) | self.byte()? as usize;
            ((header >> 2) & 0x07, len + 1)
        } else {
            (header >> 5, (header & 0x1F) as usize + 1)
        }))
    }
}

/// Appends the header of a chunk of `len` bytes (1 to 1024): short up to
/// 32 bytes, long above. `cmd` is below 7, since a long header for 7 can
/// be `$FF`, the terminator.
fn write_header(out: &mut Vec<u8>, cmd: u8, len: usize) {
    debug_assert!(cmd < 7 && (1..=LONG_LEN).contains(&len));
    let n = len - 1;
    if len <= SHORT_LEN {
        out.push((cmd << 5) | n as u8);
    } else {
        out.extend([0xE0 | (cmd << 2) | (n >> 8) as u8, n as u8]);
    }
}

/// Decompresses one stream from the start of `input`. Commands 0 (direct
/// copy), 1 (byte fill), and 2 (word fill) are the same in both formats;
/// `command` does the rest, returning false for one it does not have.
fn decompress(
    input: &[u8],
    mut command: impl FnMut(u8, usize, usize, &mut Reader, &mut Vec<u8>) -> Result<bool, LzError>,
) -> Result<Decompressed, LzError> {
    let mut out = Vec::new();
    let mut reader = Reader { input, pos: 0 };
    loop {
        let start = reader.pos;
        let Some((cmd, len)) = reader.chunk()? else {
            return Ok(Decompressed {
                data: out,
                consumed: reader.pos,
            });
        };
        if out.len() + len > MAX_OUTPUT {
            return Err(LzError::TooLarge);
        }
        match cmd {
            0 => out.extend_from_slice(reader.bytes(len)?),
            1 => {
                let b = reader.byte()?;
                out.extend(std::iter::repeat_n(b, len));
            }
            2 => {
                let pair = [reader.byte()?, reader.byte()?];
                out.extend((0..len).map(|i| pair[i & 1]));
            }
            _ => {
                if !command(cmd, len, start, &mut reader, &mut out)? {
                    return Err(LzError::BadCommand { cmd, offset: start });
                }
            }
        }
    }
}
