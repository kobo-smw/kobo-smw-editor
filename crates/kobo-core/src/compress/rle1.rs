//! LC_RLE1, the run-length format of background tilemaps.
//!
//! A stream is chunks, each a header byte `RLLLLLLL`: with `R` clear the
//! next `L + 1` bytes are copied; with `R` set the next byte is repeated
//! `L + 1` times. The game's decompressor (`CODE_058126`) reads a chunk
//! and then stops if the next two bytes are `$FF $FF`, so the first chunk
//! is always read and no chunk may start with those two bytes, which rules
//! out a run of 128 `$FF`.

use super::{Decompressed, LzError, MAX_OUTPUT};

const RUN: u8 = 0x80;
const MAX_CHUNK: usize = 128;
const END: [u8; 2] = [0xFF, 0xFF];

/// Decompresses one stream from the start of `input`. Trailing bytes after
/// the terminator are ignored.
pub fn decompress(input: &[u8]) -> Result<Decompressed, LzError> {
    let byte = |i: usize| input.get(i).copied().ok_or(LzError::Truncated(i));
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let header = byte(i)?;
        let len = (header & 0x7F) as usize + 1;
        if out.len() + len > MAX_OUTPUT {
            return Err(LzError::TooLarge);
        }
        if header & RUN != 0 {
            out.extend(std::iter::repeat_n(byte(i + 1)?, len));
            i += 2;
        } else {
            let data = input
                .get(i + 1..i + 1 + len)
                .ok_or(LzError::Truncated(input.len()))?;
            out.extend_from_slice(data);
            i += 1 + len;
        }
        if input.get(i..i + 2) == Some(&END[..]) {
            return Ok(Decompressed {
                data: out,
                consumed: i + 2,
            });
        }
    }
}

/// A chunk of the encoding: a run or a copy of `len` bytes.
#[derive(Clone, Copy)]
enum Chunk {
    Run(usize),
    Copy(usize),
}

/// Compresses `data` into the shortest stream, choosing, where lengths tie,
/// the longest run and then the longest copy at each point. Empty input
/// and input over 64 KiB have no stream.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, LzError> {
    if data.len() > MAX_OUTPUT {
        return Err(LzError::InputTooLarge(data.len()));
    }
    if data.is_empty() {
        return Err(LzError::Truncated(0));
    }
    let n = data.len();
    // run[i]: how many bytes from i equal data[i], up to a chunk.
    let mut run = vec![0; n];
    for i in (0..n).rev() {
        run[i] = if i + 1 < n && data[i + 1] == data[i] {
            (run[i + 1] + 1).min(MAX_CHUNK)
        } else {
            1
        };
    }
    // cost[i]: the fewest bytes that encode data[i..]; choice[i]: how.
    let mut cost = vec![usize::MAX; n + 1];
    let mut choice = vec![Chunk::Copy(1); n];
    cost[n] = 0;
    for i in (0..n).rev() {
        let mut best = (usize::MAX, Chunk::Copy(1));
        let mut consider = |len: usize, chunk: Chunk, size: usize| {
            let total = size + cost[i + len];
            if total < best.0 {
                best = (total, chunk);
            }
        };
        for len in (1..=run[i]).rev() {
            // A run of 128 `$FF` starts with the terminator.
            if !(len == MAX_CHUNK && data[i] == 0xFF) {
                consider(len, Chunk::Run(len), 2);
            }
        }
        for len in (1..=MAX_CHUNK.min(n - i)).rev() {
            consider(len, Chunk::Copy(len), 1 + len);
        }
        (cost[i], choice[i]) = best;
    }
    let mut out = Vec::with_capacity(cost[0] + 2);
    let mut i = 0;
    while i < n {
        match choice[i] {
            Chunk::Run(len) => {
                out.extend([RUN | (len - 1) as u8, data[i]]);
                i += len;
            }
            Chunk::Copy(len) => {
                out.push((len - 1) as u8);
                out.extend_from_slice(&data[i..i + len]);
                i += len;
            }
        }
    }
    out.extend(END);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(data: &[u8]) -> Vec<u8> {
        let packed = compress(data).unwrap();
        let unpacked = decompress(&packed).unwrap();
        assert_eq!(unpacked.data, data);
        assert_eq!(unpacked.consumed, packed.len());
        packed
    }

    #[test]
    fn chunks() {
        assert_eq!(
            decompress(&[0x82, 0x25, 0x01, 0x10, 0x11, 0xFF, 0xFF, 0x99]).unwrap(),
            Decompressed {
                data: vec![0x25, 0x25, 0x25, 0x10, 0x11],
                consumed: 7,
            }
        );
        assert_eq!(round_trip(&[0x25; 3]), [0x82, 0x25, 0xFF, 0xFF]);
        assert_eq!(round_trip(&[1, 2, 3]), [0x02, 1, 2, 3, 0xFF, 0xFF]);
    }

    #[test]
    fn the_terminator_is_checked_only_between_chunks() {
        // A copy of $FF $FF is data; the first chunk is always read.
        let data = decompress(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
        assert_eq!(data.data, [0xFF, 0xFF]);
        let data = decompress(&[0xFF, 0x01, 0xFF, 0xFF]).unwrap();
        assert_eq!(data.data, [0x01; 128]);
        // So a run of 128 $FF is split.
        let packed = round_trip(&[0xFF; 128]);
        assert_ne!(packed[..2], END);
    }

    #[test]
    fn shortest_and_stable() {
        let mut data = vec![0x25; 300];
        data.extend(0..200u8);
        data.extend([7; 2]);
        data.extend(0..5);
        let packed = round_trip(&data);
        // 300 = 128 + 128 + 44 in runs, then copies with the pair inside.
        assert_eq!(packed.len(), 3 * 2 + (2 + 207) + 2);
        assert_eq!(compress(&data).unwrap(), packed);
        for len in [1, 127, 128, 129, 1000] {
            round_trip(&vec![0xFF; len]);
            round_trip(&(0..len).map(|i| (i * 7) as u8).collect::<Vec<_>>());
        }
    }

    #[test]
    fn limits() {
        assert_eq!(decompress(&[0x05, 1, 2]), Err(LzError::Truncated(3)));
        assert_eq!(decompress(&[0x81]), Err(LzError::Truncated(1)));
        assert!(compress(&[]).is_err());
        assert_eq!(
            compress(&vec![0; MAX_OUTPUT + 1]),
            Err(LzError::InputTooLarge(MAX_OUTPUT + 1))
        );
        let endless: Vec<u8> = std::iter::repeat_n([0xFF, 0x00], 600).flatten().collect();
        assert_eq!(decompress(&endless), Err(LzError::TooLarge));
    }
}
