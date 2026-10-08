//! The game's stripe images: lists of runs of VRAM words, which
//! `LoadScrnImage` uploads (each found through `StripeImages`, `$0084D0`),
//! for layer 3's tilemaps above all (the overworld's border, the title
//! screen). Each run is a 4-byte header, the VRAM word address high byte
//! first, then the direction (bit 7: down, else across), a repeat bit (bit
//! 6: the one word that follows, repeated), and the length in bytes less
//! one in 14 bits; then the run's bytes. `$FF` ends the image. A repeated
//! run writes its bytes' count halved, rounded up, of words.
//!
//! [`Tilemap`] is what an image leaves: a word for each cell of a 32-wide
//! tilemap it writes, `None` where it writes none, within the rows kept.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StripeError {
    #[error("the stripe image runs past its data")]
    Truncated,
}

/// A tilemap's cells, 32 to a row, from a VRAM word address on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tilemap {
    pub base: u16,
    pub cells: Vec<Option<u16>>,
}

impl Tilemap {
    /// `rows` rows of 32 cells from `base`, none written.
    pub fn new(base: u16, rows: usize) -> Self {
        Self {
            base,
            cells: vec![None; 32 * rows],
        }
    }

    /// The tilemap the stripe image `bytes` leaves, and the image's length
    /// with its `$FF`.
    pub fn read(base: u16, rows: usize, bytes: &[u8]) -> Result<(Self, usize), StripeError> {
        let mut map = Self::new(base, rows);
        let get = |i: usize| bytes.get(i).copied().ok_or(StripeError::Truncated);
        let mut at = 0;
        loop {
            let hi = get(at)?;
            if hi & 0x80 != 0 {
                return Ok((map, at + 1));
            }
            let vram = u16::from_be_bytes([hi, get(at + 1)?]);
            let flags = get(at + 2)?;
            let len = (usize::from(flags & 0x3F) << 8 | usize::from(get(at + 3)?)) + 1;
            let down = flags & 0x80 != 0;
            let repeat = flags & 0x40 != 0;
            at += 4;
            let words = len.div_ceil(2);
            for i in 0..words {
                let word = if repeat {
                    u16::from_le_bytes([get(at)?, get(at + 1)?])
                } else {
                    u16::from_le_bytes([get(at + 2 * i)?, get(at + 2 * i + 1)?])
                };
                let step = if down { 32 * i } else { i };
                // A write past the rows kept (a whole layer cleared, say)
                // is left out.
                let cell = usize::from(vram.wrapping_sub(base)) + step;
                if let Some(slot) = map.cells.get_mut(cell) {
                    *slot = Some(word);
                }
            }
            at += if repeat { 2 } else { len };
        }
    }

    /// A stripe image that leaves this tilemap: a run across each stretch
    /// of written cells in a row, repeated where its words are one word
    /// three or more times, and `$FF`.
    pub fn to_stripe(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut push = |vram: u16, repeat: bool, words: &[u16]| {
            // A repeated run's bytes are odd, 2n-1 for n words, as the
            // game's and Lunar Magic's are: with an even count, the game's
            // two passes over it (the low bytes, then the high) also write
            // the high byte of the word after it.
            let len = 2 * words.len() - 1 - usize::from(repeat);
            out.extend(vram.to_be_bytes());
            out.push(if repeat { 0x40 } else { 0 } | (len >> 8) as u8);
            out.push(len as u8);
            if repeat {
                out.extend(words[0].to_le_bytes());
            } else {
                out.extend(words.iter().flat_map(|w| w.to_le_bytes()));
            }
        };
        for (r, row) in self.cells.chunks(32).enumerate() {
            let mut x = 0;
            while x < row.len() {
                let Some(word) = row[x] else {
                    x += 1;
                    continue;
                };
                // A stretch of written cells: repeats of 3 or more on
                // their own, the rest as words.
                let same = row[x..].iter().take_while(|c| **c == Some(word)).count();
                let vram = self.base + (32 * r + x) as u16;
                if same >= 3 {
                    push(vram, true, &vec![word; same]);
                    x += same;
                    continue;
                }
                let mut end = x;
                while end < row.len() && row[end].is_some() {
                    let w = row[end];
                    if row[end..].iter().take_while(|c| **c == w).count() >= 3 {
                        break;
                    }
                    end += 1;
                }
                let words: Vec<u16> = row[x..end].iter().map(|c| c.unwrap()).collect();
                push(vram, false, &words);
                x = end;
            }
        }
        out.push(0xFF);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilemap_reads_back_from_its_stripe() {
        let mut map = Tilemap::new(0x5000, 4);
        for x in 0..32 {
            map.cells[x] = Some(0x38FE);
        }
        map.cells[33] = Some(0x3891);
        map.cells[34] = Some(0x3892);
        for y in 1..4 {
            map.cells[32 * y + 31] = Some(0x7895);
        }
        let bytes = map.to_stripe();
        let (back, len) = Tilemap::read(0x5000, 4, &bytes).unwrap();
        assert_eq!(back, map);
        assert_eq!(len, bytes.len());
    }

    #[test]
    fn runs_down_and_repeats_read() {
        // A repeated run down a column, then two words across.
        let bytes = [
            0x50, 0x01, 0xC0, 0x05, 0xFE, 0x38, 0x50, 0x02, 0x00, 0x03, 0x91, 0x38, 0x92, 0x38,
            0xFF,
        ];
        let (map, len) = Tilemap::read(0x5000, 4, &bytes).unwrap();
        assert_eq!(len, bytes.len());
        assert_eq!(map.cells[1], Some(0x38FE));
        assert_eq!(map.cells[32 + 1], Some(0x38FE));
        assert_eq!(map.cells[64 + 1], Some(0x38FE));
        assert_eq!(map.cells[2], Some(0x3891));
        assert_eq!(map.cells[3], Some(0x3892));
        assert_eq!(map.cells[0], None);
    }
}
