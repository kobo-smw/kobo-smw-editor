//! LC_LZ3, which Lunar Magic can store a hack's GFX and ExGFX in. It is
//! the LZ format of Pokémon Gold, Silver, and Crystal, written from the
//! public descriptions of that format: pret/pokecrystal's
//! `home/decompress.asm` (its comments on the commands and the source
//! operand) and `tools/lzcomp`, its compressor, and the SMW community's
//! descriptions of "LC_LZ3" as the same format.
//!
//! Commands (see [`super`] for the chunk headers):
//! 0 direct copy, 1 byte fill, 2 word fill (the two bytes alternate), 3
//! zero fill (no operand), and three copies from the output written so
//! far: 4 as it is, 5 with each byte's bits reversed, 6 reading
//! backwards. A copy's source is one or two bytes:
//!
//! - bit 7 of the first byte clear: it and the next byte are a 15-bit
//!   big-endian offset from the start of the output;
//! - bit 7 set: its low seven bits count back from the last byte written,
//!   so `$80` is the last byte, `$81` the one before, and `$FF` 128 back.
//!   (pokecrystal computes the source as the output pointer minus the
//!   seven bits minus one.)
//!
//! Commands 4 and 5 read forwards a byte at a time, so they may run into
//! the bytes they write themselves; 6 reads from its source down, and
//! must not run below the start of the output.

use super::lz2::{Runs, longest_matches, optimal_parse};
use super::{Decompressed, LONG_LEN, LzError, MAX_OUTPUT, write_header};

const DIRECT_COPY: u8 = 0;
const BYTE_FILL: u8 = 1;
const WORD_FILL: u8 = 2;
const ZERO_FILL: u8 = 3;
const COPY: u8 = 4;
const BIT_REVERSED_COPY: u8 = 5;
const BACKWARDS_COPY: u8 = 6;

/// The parse's tag for a copy with a one-byte (relative) source; command
/// 4 with a two-byte one keeps its own number.
const NEAR_COPY: u8 = 8;

/// How far back a one-byte source reaches.
const NEAR: usize = 0x80;

/// The most `compress` takes: every source it writes must fit in 15 bits.
pub const MAX_INPUT: usize = 0x8000;
const _: () = assert!(MAX_INPUT <= MAX_OUTPUT);

/// Decompresses one LC_LZ3 stream from the start of `input`. Trailing
/// bytes after the terminator are ignored.
///
/// A copy whose source lies before the start of the output, or past the
/// bytes written so far, is [`LzError::BadBackRef`]; an offset before the
/// start is reported as the 16-bit offset it wraps to. So is a backwards
/// copy that would run below the start of the output, reported at the
/// first offset it cannot read.
pub fn decompress(input: &[u8]) -> Result<Decompressed, LzError> {
    super::decompress(input, |cmd, len, start, reader, out| {
        match cmd {
            ZERO_FILL => out.extend(std::iter::repeat_n(0, len)),
            COPY | BIT_REVERSED_COPY | BACKWARDS_COPY => {
                let written = out.len();
                let bad = |src: isize| LzError::BadBackRef {
                    src: (src as usize) & 0xFFFF,
                    len: written,
                    offset: start,
                };
                let first = reader.byte()?;
                let src = if first & 0x80 != 0 {
                    written as isize - 1 - (first & 0x7F) as isize
                } else {
                    (((first as usize) << 8) | reader.byte()? as usize) as isize
                };
                if src < 0 || src as usize >= written {
                    return Err(bad(src));
                }
                let src = src as usize;
                match cmd {
                    COPY => {
                        for i in 0..len {
                            let b = out[src + i];
                            out.push(b);
                        }
                    }
                    BIT_REVERSED_COPY => {
                        for i in 0..len {
                            let b = out[src + i].reverse_bits();
                            out.push(b);
                        }
                    }
                    _ => {
                        if len > src + 1 {
                            return Err(bad(-1));
                        }
                        for i in 0..len {
                            let b = out[src - i];
                            out.push(b);
                        }
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    })
}

/// Compresses `data` into one LC_LZ3 stream, terminator included. The
/// output is a function of the input alone, and no stream of commands 0
/// to 4 is shorter.
///
/// The parse is LC_LZ2's ([`super::lz2::compress`]), over these chunks:
/// a direct copy, a byte fill, a word fill, a zero fill (no operand,
/// where the byte is zero), a copy with a one-byte source from the
/// longest match among the 128 bytes before, and a copy with a two-byte
/// source from the longest match anywhere earlier. Ties go as LC_LZ2's
/// do; a near copy names the nearest of the equally long matches, a far
/// one the lowest.
///
/// The bit-reversed and backwards copies (commands 5 and 6) are never
/// written. Inputs over [`MAX_INPUT`] bytes are refused, since a far
/// copy's source must fit in 15 bits.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, LzError> {
    let n = data.len();
    if n > MAX_INPUT {
        return Err(LzError::InputTooLarge(n));
    }
    let far = longest_matches(data);
    let near = near_matches(data);
    let runs = Runs::new(data);
    let chunk = optimal_parse(n, |i| {
        let zeros = if data[i] == 0 { runs.byte[i] } else { 0 };
        vec![
            (DIRECT_COPY, n - i, 0),
            (BYTE_FILL, runs.byte[i], 1),
            (WORD_FILL, runs.word(i), 2),
            (ZERO_FILL, zeros, 0),
            (COPY, far[i].len, 2),
            (NEAR_COPY, near[i].0, 1),
        ]
    });
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let (tag, len) = chunk[i];
        let cmd = if tag == NEAR_COPY { COPY } else { tag };
        write_header(&mut out, cmd, len);
        match tag {
            DIRECT_COPY => out.extend_from_slice(&data[i..i + len]),
            BYTE_FILL => out.push(data[i]),
            WORD_FILL => out.extend_from_slice(&data[i..i + 2]),
            ZERO_FILL => {}
            // The source is below `i`, which is below 0x8000.
            COPY => out.extend_from_slice(&(far[i].src as u16).to_be_bytes()),
            _ => out.push(0x80 | (near[i].1 - 1) as u8),
        }
        i += len;
    }
    out.push(0xFF);
    Ok(out)
}

/// For each position, the longest match (at most 1024 bytes, possibly
/// running into the bytes it produces) that starts 1 to 128 bytes
/// before it, as (length, distance back): the nearest of equally long
/// ones, and (0, 0) where there is none.
fn near_matches(data: &[u8]) -> Vec<(usize, usize)> {
    let n = data.len();
    let mut best = vec![(0, 0); n];
    // Bytes from each position equal to the ones `d` before them.
    let mut same = vec![0; n + 1];
    for d in 1..=NEAR.min(n) {
        same[n] = 0;
        for i in (d..n).rev() {
            same[i] = if data[i] == data[i - d] {
                same[i + 1] + 1
            } else {
                0
            };
            let len = same[i].min(LONG_LEN);
            if len > best[i].0 {
                best[i] = (len, d);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &[u8]) -> Vec<u8> {
        let d = decompress(input).unwrap();
        assert_eq!(d.consumed, input.len());
        d.data
    }

    #[test]
    fn empty_stream() {
        assert_eq!(ok(&[0xFF]), b"");
    }

    #[test]
    fn shared_commands() {
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0xFF]), b"abc");
        assert_eq!(ok(&[0x20 | 0x03, b'A', 0xFF]), b"AAAA");
        assert_eq!(ok(&[0x40 | 0x04, b'A', b'B', 0xFF]), b"ABABA");
    }

    #[test]
    fn zero_fill_has_no_operand() {
        assert_eq!(ok(&[0x60 | 0x03, 0x00, b'x', 0xFF]), [0, 0, 0, 0, b'x']);
        // Long header: command 3, 1024 bytes.
        assert_eq!(ok(&[0xE0 | (3 << 2) | 0x03, 0xFF, 0xFF]), vec![0; 1024]);
    }

    #[test]
    fn copy_from_an_absolute_offset() {
        assert_eq!(
            ok(&[0x03, b'a', b'b', b'c', b'd', 0x80 | 0x01, 0x00, 0x01, 0xFF]),
            b"abcdbc"
        );
        // 15 bits: the high byte's bit 7 is the mode, not part of it.
        let mut input = vec![0xE0 | (1 << 2) | 0x03, 0xFF, 0x11];
        input.extend([0xE0 | (1 << 2) | 0x03, 0xFF, 0x22]);
        input.extend([0x00, 0x33, 0x80, 0x04, 0x00, 0xFF]);
        let data = ok(&input);
        assert_eq!(&data[2049..], [0x22]);
        assert_eq!(data[1024], 0x22);
    }

    #[test]
    fn copy_from_a_relative_offset() {
        // $80 is the last byte written, $81 the one before.
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0x80, 0x80, 0xFF]), b"abcc");
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0x81, 0x82, 0xFF]), b"abcab");
        // $FF reaches 128 back.
        let mut input = vec![0x1F];
        input.extend(0..32u8);
        input.extend([0x1F]);
        input.extend(32..64u8);
        input.extend([0x1F]);
        input.extend(64..96u8);
        input.extend([0x1F]);
        input.extend(96..128u8);
        input.extend([0x81, 0xFF, 0xFF]);
        assert_eq!(&ok(&input)[128..], [0, 1]);
    }

    #[test]
    fn copies_overlap_their_own_output() {
        // Forwards: the pattern repeats.
        assert_eq!(ok(&[0x01, b'a', b'b', 0x80 | 0x04, 0x81, 0xFF]), b"abababa");
        // Bit-reversed: bytes it wrote are reversed again.
        assert_eq!(
            ok(&[0x00, 0x01, 0xA0 | 0x02, 0x80, 0xFF]),
            [0x01, 0x80, 0x01, 0x80]
        );
    }

    #[test]
    fn bit_reversed_copy() {
        assert_eq!(
            ok(&[0x02, 0x01, 0x12, 0xF0, 0xA0 | 0x02, 0x00, 0x00, 0xFF]),
            [0x01, 0x12, 0xF0, 0x80, 0x48, 0x0F]
        );
        assert_eq!(ok(&[0x00, 0xC3, 0xA0, 0x80, 0xFF]), [0xC3, 0xC3]);
    }

    #[test]
    fn backwards_copy() {
        assert_eq!(
            ok(&[0x03, b'a', b'b', b'c', b'd', 0xC0 | 0x02, 0x80, 0xFF]),
            b"abcddcb"
        );
        assert_eq!(
            ok(&[0x03, b'a', b'b', b'c', b'd', 0xC0 | 0x02, 0x00, 0x02, 0xFF]),
            b"abcdcba"
        );
        // Long header: command 6, 4 bytes, down to offset 0.
        assert_eq!(
            ok(&[
                0x03,
                b'a',
                b'b',
                b'c',
                b'd',
                0xE0 | (6 << 2),
                0x03,
                0x00,
                0x03,
                0xFF
            ]),
            b"abcddcba"
        );
    }

    #[test]
    fn consumed_ignores_trailing_bytes() {
        let d = decompress(&[0x60, 0xFF, 0xAA, 0xBB]).unwrap();
        assert_eq!(d.data, [0]);
        assert_eq!(d.consumed, 2);
    }

    #[test]
    fn errors() {
        assert_eq!(decompress(&[]), Err(LzError::Truncated(0)));
        assert_eq!(decompress(&[0x02, b'a']), Err(LzError::Truncated(2)));
        assert_eq!(decompress(&[0x60]), Err(LzError::Truncated(1)));
        assert_eq!(decompress(&[0x00, b'a', 0x80]), Err(LzError::Truncated(3)));
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00]),
            Err(LzError::Truncated(4))
        );
        assert_eq!(
            decompress(&[0xE0 | (7 << 2), 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 7, offset: 0 })
        );
        // Past the bytes written, absolute.
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00, 0x01, 0xFF]),
            Err(LzError::BadBackRef {
                src: 1,
                len: 1,
                offset: 2
            })
        );
        // Nothing written yet.
        assert_eq!(
            decompress(&[0x80, 0x80, 0xFF]),
            Err(LzError::BadBackRef {
                src: 0xFFFF,
                len: 0,
                offset: 0
            })
        );
        // Before the start, relative.
        assert_eq!(
            decompress(&[0x01, b'a', b'b', 0xA0, 0x82, 0xFF]),
            Err(LzError::BadBackRef {
                src: 0xFFFF,
                len: 2,
                offset: 3
            })
        );
        // Backwards past the start.
        assert_eq!(
            decompress(&[0x01, b'a', b'b', 0xC0 | 0x02, 0x80, 0xFF]),
            Err(LzError::BadBackRef {
                src: 0xFFFF,
                len: 2,
                offset: 3
            })
        );
        assert_eq!(ok(&[0x01, b'a', b'b', 0xC0 | 0x01, 0x80, 0xFF]), b"abba");
    }

    #[test]
    fn output_limit() {
        let mut input = Vec::new();
        for _ in 0..65 {
            input.extend([0xE0 | (3 << 2) | 0x03, 0xFF]);
        }
        input.push(0xFF);
        assert_eq!(decompress(&input), Err(LzError::TooLarge));
    }

    /// The chunks of a stream, as (command, length, operand).
    fn chunks(stream: &[u8]) -> Vec<(u8, usize, Vec<u8>)> {
        let mut reader = super::super::Reader {
            input: stream,
            pos: 0,
        };
        let mut chunks = Vec::new();
        while let Some((cmd, len)) = reader.chunk().unwrap() {
            let operand = match cmd {
                0 => len,
                1 => 1,
                2 => 2,
                3 => 0,
                _ if reader.input[reader.pos] & 0x80 != 0 => 1,
                _ => 2,
            };
            chunks.push((cmd, len, reader.bytes(operand).unwrap().to_vec()));
        }
        chunks
    }

    /// Compresses `data`, checking that it compresses the same way twice,
    /// reads back whole, uses only commands 0 to 4, and is no larger than
    /// storing it.
    fn packed(data: &[u8]) -> Vec<u8> {
        let out = compress(data).unwrap();
        assert_eq!(compress(data).unwrap(), out);
        let back = decompress(&out).unwrap();
        assert!(back.data == data, "round trip differs");
        assert_eq!(back.consumed, out.len());
        for (cmd, ..) in chunks(&out) {
            assert!(cmd <= 4, "command {cmd}");
        }
        assert!(out.len() <= data.len() + 2 * data.len().div_ceil(1024) + 1);
        out
    }

    fn noise(len: usize, mut seed: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn compresses_empty_input_to_the_terminator() {
        assert_eq!(packed(&[]), [0xFF]);
    }

    #[test]
    fn uses_each_command() {
        assert_eq!(packed(&[7]), [0x00, 7, 0xFF]);
        assert_eq!(packed(&[7, 7]), [0x21, 7, 0xFF]);
        assert_eq!(packed(&[0]), [0x60, 0xFF]);
        assert_eq!(packed(&[0; 40]), [0xE0 | (3 << 2), 39, 0xFF]);
        assert_eq!(packed(&[1, 2, 1, 2, 1]), [0x44, 1, 2, 0xFF]);
        // Near: one byte of source.
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56];
        let data: Vec<u8> = pattern.iter().cycle().take(15).copied().collect();
        let mut want = vec![0x04];
        want.extend(pattern);
        want.extend([0x80 | 9, 0x80 | 4, 0xFF]);
        assert_eq!(packed(&data), want);
        // Far: beyond 128 bytes back, two bytes of source.
        let stretch = noise(64, 9);
        let mut data = stretch.clone();
        data.extend(noise(200, 10));
        data.extend(&stretch);
        let out = packed(&data);
        let last = chunks(&out).pop().unwrap();
        assert_eq!(last, (4, 64, vec![0x00, 0x00]));
    }

    #[test]
    fn prefers_a_near_source_and_the_nearest() {
        // The same 8 bytes at 0, 8, and 16: the copy at 16 names 8 back.
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56, 0x77, 0x01, 0xEE];
        let mut data = pattern.to_vec();
        data.extend([0xA5, 0x5A, 0xC3, 0x3C, 0x96, 0x69, 0x0F, 0xF0]);
        data.extend(pattern);
        let out = packed(&data);
        assert_eq!(chunks(&out).pop().unwrap(), (4, 8, vec![0x80 | 15]));
        // With a nearer copy, that one.
        let mut data = pattern.to_vec();
        data.extend(pattern);
        data.extend([0xA5]);
        data.extend(pattern);
        let out = packed(&data);
        assert_eq!(chunks(&out).pop().unwrap(), (4, 8, vec![0x80 | 8]));
    }

    #[test]
    fn switches_to_long_headers_after_32_bytes() {
        for (len, size) in [
            (1, 2),
            (32, 2),
            (33, 3),
            (1024, 3),
            (1025, 4),
            (2048, 5),
            (2049, 6),
        ] {
            assert_eq!(packed(&vec![0; len]).len(), size, "zero fill of {len}");
        }
        for (len, size) in [(1, 3), (32, 3), (33, 4), (1024, 4), (1025, 6)] {
            assert_eq!(packed(&vec![0x7E; len]).len(), size, "byte fill of {len}");
        }
        // A near copy that overlaps its own output.
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56];
        for (len, size) in [(4, 9), (32, 9), (33, 10), (1024, 10), (1025, 11)] {
            let data: Vec<u8> = pattern.iter().cycle().take(5 + len).copied().collect();
            assert_eq!(packed(&data).len(), size, "near copy of {len}");
        }
    }

    /// The cost of the parse `compress` documents, found by trying every
    /// command at every length, against every earlier position.
    fn brute_force_size(data: &[u8]) -> usize {
        let n = data.len();
        let mut best = vec![0; n + 1];
        for i in (0..n).rev() {
            let at = |k: usize| data[i + k];
            let run = |fits: &dyn Fn(usize) -> bool| (0..n - i).take_while(|&k| fits(k)).count();
            let mut options = vec![
                (n - i, usize::MAX),
                (run(&|k| at(k) == at(0)), 1),
                (
                    if i + 1 < n {
                        run(&|k| at(k) == at(k & 1))
                    } else {
                        0
                    },
                    2,
                ),
                (if at(0) == 0 { run(&|k| at(k) == 0) } else { 0 }, 0),
            ];
            for src in 0..i {
                let operand = if i - src <= NEAR { 1 } else { 2 };
                options.push((run(&|k| data[src + k] == at(k)), operand));
            }
            let mut here = usize::MAX;
            for (reach, operand) in options {
                for len in 1..=reach.min(LONG_LEN) {
                    let header = if len <= 32 { 1 } else { 2 };
                    let operand = if operand == usize::MAX { len } else { operand };
                    here = here.min(header + operand + best[i + len]);
                }
            }
            best[i] = here;
        }
        best[0] + 1
    }

    #[test]
    fn is_as_small_as_trying_everything() {
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        for case in 0..200 {
            let len = next() % 400;
            let symbols = 1 + case % 4;
            let mut data = Vec::new();
            while data.len() < len {
                let b = (next() % symbols) as u8;
                let run = if next() % 4 == 0 { next() % 50 } else { 1 };
                match next() % 3 {
                    0 => data.extend(std::iter::repeat_n(b, run)),
                    1 => data.extend((0..run).map(|k| b.wrapping_add(k as u8))),
                    _ => data.extend((0..run).map(|k| [b, 0x40][k & 1])),
                }
            }
            data.truncate(len);
            assert_eq!(
                packed(&data).len(),
                brute_force_size(&data),
                "case {case}: {data:02X?}"
            );
        }
    }

    #[test]
    fn stores_noise_as_it_is() {
        let data = noise(4096, 7);
        assert_eq!(packed(&data).len(), 4096 + 4 * 2 + 1);
    }

    #[test]
    fn takes_up_to_32_kib() {
        assert_eq!(
            compress(&vec![0; MAX_INPUT + 1]),
            Err(LzError::InputTooLarge(MAX_INPUT + 1))
        );
        assert_eq!(packed(&vec![0; MAX_INPUT]).len(), 32 * 2 + 1);
        // Noise with runs, repeats, and copies of earlier stretches, near
        // and far, up to the last offset a source can name.
        let mut data = noise(MAX_INPUT / 2, 3);
        for i in 0..MAX_INPUT / 2 {
            let b = match (i / 700) % 5 {
                0 => data[i * 7 % data.len()],
                1 => 0x5A,
                2 => 0,
                3 => data[data.len() - 100],
                _ => data[i],
            };
            data.push(b);
        }
        packed(&data);
    }
}
