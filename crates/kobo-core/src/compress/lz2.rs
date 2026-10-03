//! LC_LZ2, the compression SMW uses for GFX files and other assets.
//!
//! Commands (see [`super`] for the chunk headers):
//! 0 direct copy, 1 byte fill, 2 word fill, 3 incrementing fill, 4 copy
//! from an earlier point in the output (big-endian 16-bit offset).

use super::{Decompressed, LONG_LEN, LzError, MAX_OUTPUT, SHORT_LEN, write_header};
use std::cmp::Reverse;

const DIRECT_COPY: u8 = 0;
const BYTE_FILL: u8 = 1;
const WORD_FILL: u8 = 2;
const INCREMENTING_FILL: u8 = 3;
const BACK_REFERENCE: u8 = 4;

/// Decompresses one LC_LZ2 stream from the start of `input`. Trailing
/// bytes after the terminator are ignored.
pub fn decompress(input: &[u8]) -> Result<Decompressed, LzError> {
    super::decompress(input, |cmd, len, start, reader, out| {
        match cmd {
            3 => {
                let b = reader.byte()?;
                out.extend((0..len).map(|i| b.wrapping_add(i as u8)));
            }
            4 => {
                let src = ((reader.byte()? as usize) << 8) | reader.byte()? as usize;
                if src >= out.len() {
                    return Err(LzError::BadBackRef {
                        src,
                        len: out.len(),
                        offset: start,
                    });
                }
                // Overlapping copies are allowed and repeat the pattern.
                for i in 0..len {
                    let b = out[src + i];
                    out.push(b);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    })
}

/// Compresses `data` into one LC_LZ2 stream, terminator included. The
/// output is a function of the input alone, and no stream of commands 0
/// to 4 is shorter.
///
/// The parse is optimal, by dynamic programming from the end of the
/// input backwards: the cost of encoding from a position is the cheapest
/// chunk that can start there plus the cost from where it ends. A fill
/// can end anywhere within the run it repeats, a back-reference anywhere
/// within the longest earlier match, and a direct copy anywhere, each up
/// to 1024 bytes; a range-minimum table over the costs already known
/// finds the best end for each command and header size in two lookups. A
/// back-reference costs the same whatever it points at, so the longest
/// match is all the parse needs, and a suffix array gives it exactly for
/// every position (`longest_matches`): the search gives nothing up. Time
/// is O(n log n) and memory O(n); 64 KiB takes a fraction of a second
/// even in a debug build.
///
/// Ties are broken by a fixed rule. The chunk chosen at each position is
/// the one leaving the fewest bytes to the end, then the fewest chunks
/// (less work for the game's routine), then the longest, then the lowest
/// command number. Which earlier copy a back-reference names does not
/// change the size; `longest_matches` says which it is.
///
/// Only what the game's routine (`CODE_00B8DE`) and [`decompress`] read
/// the same is written: commands 0 to 4, short headers for 1 to 32 bytes
/// and long ones for 33 to 1024, and back-references to bytes already
/// written (which may overlap the copy). Commands 5 to 7 are never used;
/// the game would take them as back-references, and a long header for 7
/// can be the terminator.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, LzError> {
    let n = data.len();
    if n > MAX_OUTPUT {
        return Err(LzError::InputTooLarge(n));
    }
    let matches = longest_matches(data);
    let runs = Runs::new(data);
    let chunk = optimal_parse(n, |i| {
        vec![
            (DIRECT_COPY, n - i, 0),
            (BYTE_FILL, runs.byte[i], 1),
            (WORD_FILL, runs.word(i), 2),
            (INCREMENTING_FILL, runs.incrementing[i], 1),
            (BACK_REFERENCE, matches[i].len, 2),
        ]
    });
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let (cmd, len) = chunk[i];
        write_header(&mut out, cmd, len);
        match cmd {
            DIRECT_COPY => out.extend_from_slice(&data[i..i + len]),
            BYTE_FILL | INCREMENTING_FILL => out.push(data[i]),
            WORD_FILL => out.extend_from_slice(&data[i..i + 2]),
            // The source is below `i`, so it fits in 16 bits.
            _ => out.extend_from_slice(&(matches[i].src as u16).to_be_bytes()),
        }
        i += len;
    }
    out.push(0xFF);
    Ok(out)
}

/// The cheapest chunk to start at each position of `n` bytes, as (tag,
/// length), from the ways `commands` gives a chunk to start there: its tag
/// (a command, or one the caller maps to a command and operand; the lower
/// wins a tie), how far it can reach, and its operand's bytes. Tag 0 is a
/// direct copy, which pays for its length instead and can always reach the
/// end. See [`compress`] for the parse and its tie-breaks.
pub(super) fn optimal_parse(
    n: usize,
    commands: impl Fn(usize) -> Vec<(u8, usize, usize)>,
) -> Vec<(u8, usize)> {
    // For each position: bytes and chunks from there to the end, and the
    // chunk that starts there (command, length).
    let mut cost = vec![(0, 0); n + 1];
    let mut chunk = vec![(DIRECT_COPY, 0); n];
    // A fill or back-reference costs the same whatever its length, so its
    // best end is the one with the least cost from there; a direct copy
    // costs its length as well, so its best is the least cost plus end.
    let mut fills = RangeMin::new(n + 1);
    let mut copies = RangeMin::new(n + 1);
    fills.set(n, key(0, 0, n));
    copies.set(n, key(n, 0, n));
    for i in (0..n).rev() {
        let mut best = None;
        for (cmd, reach, operand) in commands(i) {
            for (shortest, longest, header) in [(1, SHORT_LEN, 1), (SHORT_LEN + 1, LONG_LEN, 2)] {
                let longest = longest.min(reach);
                if longest < shortest {
                    continue;
                }
                let table = if cmd == DIRECT_COPY { &copies } else { &fills };
                let end = end_of(table.min(i + shortest, i + longest));
                let len = end - i;
                let payload = if cmd == DIRECT_COPY { len } else { operand };
                let (bytes, chunks) = cost[end];
                let candidate = (bytes + header + payload, chunks + 1, Reverse(len), cmd);
                if best.is_none_or(|best| candidate < best) {
                    best = Some(candidate);
                }
            }
        }
        let (bytes, chunks, Reverse(len), cmd) = best.expect("a direct copy always fits");
        cost[i] = (bytes, chunks);
        chunk[i] = (cmd, len);
        fills.set(i, key(bytes, chunks, i));
        copies.set(i, key(bytes + i, chunks, i));
    }
    chunk
}

/// Packs what a range query compares: bytes to the end, then chunks, then
/// the later position first. Positions and chunk counts fit in 17 bits.
fn key(bytes: usize, chunks: usize, pos: usize) -> u64 {
    ((bytes as u64) << 34) | ((chunks as u64) << 17) | (POS_MASK - pos as u64)
}

const POS_MASK: u64 = 0x1_FFFF;

fn end_of(key: u64) -> usize {
    (POS_MASK - (key & POS_MASK)) as usize
}

/// Minima of keys over ranges of positions, filled in from the last
/// position down. Level `k` holds the least of the `2^k` keys from each
/// position, which needs only positions already filled in, so a range of
/// up to 1024 is the lesser of two overlapping entries of one level.
struct RangeMin {
    levels: Vec<Vec<u64>>,
}

impl RangeMin {
    fn new(len: usize) -> Self {
        let levels = LONG_LEN.ilog2() as usize + 1;
        Self {
            levels: vec![vec![u64::MAX; len]; levels],
        }
    }

    fn set(&mut self, pos: usize, key: u64) {
        self.levels[0][pos] = key;
        for k in 1..self.levels.len() {
            let upper = self.levels[k - 1].get(pos + (1 << (k - 1)));
            let min = self.levels[k - 1][pos].min(upper.copied().unwrap_or(u64::MAX));
            self.levels[k][pos] = min;
        }
    }

    /// The least key from `first` to `last`, both included.
    fn min(&self, first: usize, last: usize) -> u64 {
        let k = (last + 1 - first).ilog2() as usize;
        self.levels[k][first].min(self.levels[k][last + 1 - (1 << k)])
    }
}

/// How far each fill could reach from each position.
pub(super) struct Runs {
    /// Bytes equal to the one at the position.
    pub byte: Vec<usize>,
    /// Bytes each one more than the last, wrapping at 256.
    incrementing: Vec<usize>,
    /// Bytes from the position on equal to the one two before them.
    repeats_two_back: Vec<usize>,
}

impl Runs {
    pub fn new(data: &[u8]) -> Self {
        let n = data.len();
        let mut runs = Self {
            byte: vec![0; n + 1],
            incrementing: vec![0; n + 1],
            repeats_two_back: vec![0; n + 1],
        };
        for i in (0..n).rev() {
            let next = data.get(i + 1);
            if next == Some(&data[i]) {
                runs.byte[i] = runs.byte[i + 1];
            }
            runs.byte[i] += 1;
            if next == Some(&data[i].wrapping_add(1)) {
                runs.incrementing[i] = runs.incrementing[i + 1];
            }
            runs.incrementing[i] += 1;
            if i >= 2 && data[i] == data[i - 2] {
                runs.repeats_two_back[i] = runs.repeats_two_back[i + 1] + 1;
            }
        }
        runs
    }

    /// A word fill's operand is the two bytes from the position, so it
    /// needs both to be there.
    pub fn word(&self, i: usize) -> usize {
        match self.repeats_two_back.get(i + 2) {
            Some(&repeats) => 2 + repeats,
            None => 0,
        }
    }
}

/// An earlier position the bytes from a position also start at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Match {
    /// How many bytes they share, at most 1024. The copy may run into
    /// the bytes it produces, since the decoders copy a byte at a time.
    pub len: usize,
    pub src: usize,
}

/// The longest earlier match for every position of `data`.
///
/// In the sorted order of the suffixes, the ones sharing the most with a
/// given suffix are nearest it, and what they share only shrinks with
/// distance. So the longest earlier match is with the nearest suffix on
/// either side that starts earlier, found for every position by one pass
/// up the order and one down, each keeping a stack of candidates. The
/// match names that suffix; where both sides share as much (after the
/// 1024 cap), the one that starts lower.
pub(super) fn longest_matches(data: &[u8]) -> Vec<Match> {
    let n = data.len();
    let order = suffix_array(data);
    let shared = shared_prefixes(data, &order);
    let mut best = vec![Match { len: 0, src: 0 }; n];
    let mut pass = |ranks: &mut dyn Iterator<Item = usize>, step: &dyn Fn(usize) -> usize| {
        // Ranks whose suffixes start in increasing order, each with what
        // it shares with the rank pushed after it.
        let mut stack: Vec<(usize, usize)> = Vec::new();
        // What the top of the stack shares with the current rank.
        let mut common = usize::MAX;
        for rank in ranks {
            common = common.min(step(rank));
            while let Some(&(top, _)) = stack.last()
                && order[top] > order[rank]
            {
                stack.pop();
                if let Some(&(_, below)) = stack.last() {
                    common = common.min(below);
                }
            }
            if let Some(last) = stack.last_mut() {
                let found = Match {
                    len: common.min(LONG_LEN),
                    src: order[last.0],
                };
                let current = &mut best[order[rank]];
                if (found.len, Reverse(found.src)) > (current.len, Reverse(current.src)) {
                    *current = found;
                }
                last.1 = common;
            }
            stack.push((rank, 0));
            common = usize::MAX;
        }
    };
    // `shared[r]` is what ranks `r - 1` and `r` share.
    pass(&mut (0..n), &|rank| shared[rank]);
    pass(&mut (0..n).rev(), &|rank| {
        shared.get(rank + 1).copied().unwrap_or(0)
    });
    best
}

/// The start of every suffix of `data`, in sorted order (a suffix that
/// is a prefix of another sorts first). By prefix doubling: suffixes
/// ranked by their first `k` bytes are ranked by their first `2k` by the
/// pair of ranks at `i` and `i + k`, with two stable counting sorts,
/// until every rank differs.
fn suffix_array(data: &[u8]) -> Vec<usize> {
    let n = data.len();
    if n == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..n).collect();
    // Rank 0 stands for the end of the data.
    let mut rank: Vec<usize> = data.iter().map(|&b| b as usize + 1).collect();
    let mut next = vec![0; n];
    let mut scratch = vec![0; n];
    let mut counts = vec![0; n.max(256) + 1];
    let mut k = 1;
    loop {
        let pair = |rank: &[usize], i: usize| (rank[i], rank.get(i + k).copied().unwrap_or(0));
        counting_sort(&order, &mut scratch, &mut counts, |i| pair(&rank, i).1);
        counting_sort(&scratch, &mut order, &mut counts, |i| rank[i]);
        next[order[0]] = 1;
        for r in 1..n {
            let (a, b) = (order[r - 1], order[r]);
            next[b] = next[a] + usize::from(pair(&rank, a) != pair(&rank, b));
        }
        std::mem::swap(&mut rank, &mut next);
        if rank[order[n - 1]] == n {
            return order;
        }
        k *= 2;
    }
}

/// Stably sorts `from` into `to` by a key below `counts.len()`.
fn counting_sort(
    from: &[usize],
    to: &mut [usize],
    counts: &mut [usize],
    key: impl Fn(usize) -> usize,
) {
    counts.fill(0);
    for &i in from {
        counts[key(i)] += 1;
    }
    let mut total = 0;
    for count in counts.iter_mut() {
        (*count, total) = (total, total + *count);
    }
    for &i in from {
        let slot = &mut counts[key(i)];
        to[*slot] = i;
        *slot += 1;
    }
}

/// For each rank of `order`, the bytes its suffix shares with the one
/// ranked just before it (0 for the first). Kasai's algorithm: taking
/// the suffixes by where they start, each shares at most one byte fewer
/// with its predecessor than the suffix before it did.
fn shared_prefixes(data: &[u8], order: &[usize]) -> Vec<usize> {
    let n = data.len();
    let mut rank = vec![0; n];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r;
    }
    let mut shared = vec![0; n];
    let mut h = 0;
    for i in 0..n {
        if rank[i] == 0 {
            h = 0;
            continue;
        }
        let j = order[rank[i] - 1];
        while i + h < n && j + h < n && data[i + h] == data[j + h] {
            h += 1;
        }
        shared[rank[i]] = h;
        h = h.saturating_sub(1);
    }
    shared
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
    fn direct_copy() {
        assert_eq!(ok(&[0x02, b'a', b'b', b'c', 0xFF]), b"abc");
    }

    #[test]
    fn byte_fill() {
        assert_eq!(ok(&[0x20 | 0x03, b'A', 0xFF]), b"AAAA");
    }

    #[test]
    fn word_fill() {
        assert_eq!(ok(&[0x40 | 0x04, b'A', b'B', 0xFF]), b"ABABA");
    }

    #[test]
    fn incrementing_fill() {
        assert_eq!(ok(&[0x60 | 0x03, 0x10, 0xFF]), [0x10, 0x11, 0x12, 0x13]);
        // Wraps around at 256.
        assert_eq!(ok(&[0x60 | 0x02, 0xFE, 0xFF]), [0xFE, 0xFF, 0x00]);
    }

    #[test]
    fn back_reference() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x01, 0x00, 0x00, 0xFF]),
            b"abcab"
        );
    }

    #[test]
    fn overlapping_back_reference_repeats() {
        assert_eq!(
            ok(&[0x02, b'a', b'b', b'c', 0x80 | 0x04, 0x00, 0x01, 0xFF]),
            b"abcbcbcb"
        );
    }

    #[test]
    fn long_direct_copy() {
        let payload: Vec<u8> = (0..=255).chain(0..1).collect();
        let mut input = vec![0xE0 | 0x01, 0x00]; // long header, command 0
        input.extend(&payload);
        input.push(0xFF);
        assert_eq!(ok(&input), payload);
    }

    #[test]
    fn long_byte_fill() {
        // Long header for command 1 with length 0x3FF + 1 = 1024.
        assert_eq!(
            ok(&[0xE0 | (1 << 2) | 0x03, 0xFF, 0x7E, 0xFF]),
            vec![0x7E; 1024]
        );
    }

    #[test]
    fn consumed_ignores_trailing_bytes() {
        let d = decompress(&[0x00, b'x', 0xFF, 0xAA, 0xBB]).unwrap();
        assert_eq!(d.data, b"x");
        assert_eq!(d.consumed, 3);
    }

    #[test]
    fn errors() {
        assert_eq!(decompress(&[]), Err(LzError::Truncated(0)));
        assert_eq!(decompress(&[0x02, b'a']), Err(LzError::Truncated(2)));
        assert_eq!(decompress(&[0x00, b'a']), Err(LzError::Truncated(2)));
        assert_eq!(
            decompress(&[0xA0, 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 5, offset: 0 })
        );
        assert_eq!(
            decompress(&[0xE0 | (7 << 2), 0x00, 0xFF]),
            Err(LzError::BadCommand { cmd: 7, offset: 0 })
        );
        assert_eq!(
            decompress(&[0x00, b'a', 0x80, 0x00, 0x05, 0xFF]),
            Err(LzError::BadBackRef {
                src: 5,
                len: 1,
                offset: 2
            })
        );
    }

    #[test]
    fn output_limit() {
        // 65 fills of 1024 bytes exceed 64 KiB.
        let mut input = Vec::new();
        for _ in 0..65 {
            input.extend([0xE0 | (1 << 2) | 0x03, 0xFF, 0x00]);
        }
        input.push(0xFF);
        assert_eq!(decompress(&input), Err(LzError::TooLarge));
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
        let mut reader = super::super::Reader {
            input: &out,
            pos: 0,
        };
        while let Some((cmd, len)) = reader.chunk().unwrap() {
            assert!(cmd <= 4, "command {cmd}");
            reader.pos += match cmd {
                0 => len,
                1 | 3 => 1,
                _ => 2,
            };
        }
        assert!(out.len() <= data.len() + 2 * data.len().div_ceil(1024) + 1);
        out
    }

    /// Bytes from a xorshift generator, which nothing compresses.
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
    fn breaks_ties_the_same_way() {
        // One byte: a direct copy and a fill cost the same; the lower
        // command wins.
        assert_eq!(packed(&[7]), [0x00, 7, 0xFF]);
        assert_eq!(packed(&[7, 7]), [0x21, 7, 0xFF]);
        assert_eq!(packed(&[0x10, 0x11]), [0x61, 0x10, 0xFF]);
        assert_eq!(
            packed(&[1, 1, 1, 1, 5, 6, 7, 8, 9, 0x41, 0x42, 0x41, 0x42, 0x41]),
            [0x23, 1, 0x64, 5, 0x44, 0x41, 0x42, 0xFF]
        );
        // Five bytes and a copy of three of them, or eight bytes as they
        // are: the same size, and one chunk is fewer than two.
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56];
        let data: Vec<u8> = pattern.iter().chain(&pattern[..3]).copied().collect();
        let mut want = vec![0x07];
        want.extend(&data);
        want.push(0xFF);
        assert_eq!(packed(&data), want);
        // 1025 bytes as 1024 and 1, or 993 and 32, or ...: the longest
        // first, then a direct copy over a fill of one byte.
        assert_eq!(
            packed(&[0x7E; 1025]),
            [0xE0 | (1 << 2) | 0x03, 0xFF, 0x7E, 0x00, 0x7E, 0xFF]
        );
    }

    /// Sizes at the header boundaries, for each command.
    #[test]
    fn switches_to_long_headers_after_32_bytes() {
        for name in ["byte", "incrementing"] {
            let fill = |i: usize| if name == "byte" { 0x7E } else { i as u8 };
            for (len, size) in [
                (1, 3),
                (2, 3),
                (32, 3),
                (33, 4),
                (1024, 4),
                (1025, 6),
                (1056, 6),
                (1057, 7),
                (2048, 7),
            ] {
                let data: Vec<u8> = (0..len).map(fill).collect();
                assert_eq!(packed(&data).len(), size, "{name} fill of {len}");
            }
        }
        let word = |i: usize| [0x12, 0x34][i & 1];
        // Two bytes are a direct copy; odd lengths end on the first byte.
        for (len, size) in [(1, 3), (2, 4), (3, 4), (32, 4), (33, 5), (1024, 5)] {
            let data: Vec<u8> = (0..len).map(word).collect();
            assert_eq!(packed(&data).len(), size, "word fill of {len}");
        }
        for (len, size) in [(1025, 7), (1056, 8), (1057, 9)] {
            let data: Vec<u8> = (0..len).map(word).collect();
            assert_eq!(packed(&data).len(), size, "word fill of {len}");
        }
        // Five bytes, then a back-reference that overlaps its own output.
        // Past 1024, the direct copy takes the extra byte instead.
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56];
        for (len, size) in [(4, 10), (32, 10), (33, 11), (1024, 11), (1025, 12)] {
            let data: Vec<u8> = pattern.iter().cycle().take(5 + len).copied().collect();
            assert_eq!(packed(&data).len(), size, "back-reference of {len}");
        }
    }

    #[test]
    fn back_references_repeat_what_they_overlap() {
        let pattern = [0x12, 0x9A, 0x34, 0xF0, 0x56];
        let data: Vec<u8> = pattern.iter().cycle().take(15).copied().collect();
        let mut want = vec![0x04];
        want.extend(pattern);
        want.extend([0x80 | 9, 0x00, 0x00, 0xFF]);
        assert_eq!(packed(&data), want);
        // A back-reference to a later copy of a stretch of noise, past
        // runs that cross both header boundaries.
        let stretch = noise(1100, 1);
        let mut data = stretch.clone();
        data.extend([0x00; 1030]);
        data.extend([0x11; 33]);
        data.extend((0..1030).map(|i| [0x12, 0x34][i & 1]));
        data.extend((0..1030).map(|i| i as u8));
        data.extend(&stretch);
        let alone = packed(&stretch).len() - 1;
        // Fills of 1024 and 6, 33, 1024 and 6, 1024 and 6, then two
        // back-references of 1024 and 76.
        let rest = (3 + 2) + 3 + (4 + 3) + (3 + 2) + (4 + 4);
        assert_eq!(packed(&data).len(), alone + rest + 1);
    }

    /// The chunks of the parse `compress` documents, found by trying every
    /// command at every length, against every earlier position.
    fn brute_force_parse(data: &[u8]) -> Vec<(u8, usize)> {
        let n = data.len();
        // How far each command reaches from `i`.
        let reach = |i: usize, cmd: u8| {
            let at = |k: usize| data[i + k];
            let run = |fits: &dyn Fn(usize) -> bool| (0..n - i).take_while(|&k| fits(k)).count();
            match cmd {
                0 => n - i,
                1 => run(&|k| at(k) == at(0)),
                2 if i + 1 < n => run(&|k| at(k) == at(k & 1)),
                2 => 0,
                3 => run(&|k| at(k) == at(0).wrapping_add(k as u8)),
                _ => (0..i)
                    .map(|src| run(&|k| data[src + k] == at(k)))
                    .max()
                    .unwrap_or(0),
            }
        };
        let mut best = vec![(0, 0, Reverse(0), 0); n + 1];
        for i in (0..n).rev() {
            let mut here = None;
            for cmd in 0..=4 {
                for len in 1..=reach(i, cmd).min(LONG_LEN) {
                    let header = if len <= SHORT_LEN { 1 } else { 2 };
                    let operand = [len, 1, 2, 1, 2][cmd as usize];
                    let (bytes, chunks, ..) = best[i + len];
                    let candidate = (bytes + header + operand, chunks + 1, Reverse(len), cmd);
                    if here.is_none_or(|here| candidate < here) {
                        here = Some(candidate);
                    }
                }
            }
            best[i] = here.unwrap();
        }
        let mut chunks = Vec::new();
        let mut i = 0;
        while i < n {
            let (.., Reverse(len), cmd) = best[i];
            chunks.push((cmd, len));
            i += len;
        }
        chunks
    }

    #[test]
    fn parses_as_trying_everything_does() {
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        for case in 0..200 {
            // Few distinct bytes and runs of them, so that matches, fills,
            // and ties are everywhere.
            let len = next() % 300;
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
            let out = packed(&data);
            let mut reader = super::super::Reader {
                input: &out,
                pos: 0,
            };
            let mut chunks = Vec::new();
            while let Some((cmd, len)) = reader.chunk().unwrap() {
                chunks.push((cmd, len));
                reader.pos += [len, 1, 2, 1, 2][cmd as usize];
            }
            assert_eq!(chunks, brute_force_parse(&data), "case {case}: {data:02X?}");
        }
    }

    #[test]
    fn stores_noise_as_it_is() {
        let data = noise(4096, 7);
        assert_eq!(packed(&data).len(), 4096 + 4 * 2 + 1);
    }

    #[test]
    fn takes_up_to_64_kib() {
        assert_eq!(
            compress(&vec![0; MAX_OUTPUT + 1]),
            Err(LzError::InputTooLarge(MAX_OUTPUT + 1))
        );
        assert_eq!(packed(&vec![0; MAX_OUTPUT]).len(), 64 * 3 + 1);
        // Noise with runs, repeats, and copies of earlier stretches.
        let mut data = noise(MAX_OUTPUT / 2, 3);
        for i in 0..MAX_OUTPUT / 2 {
            let b = match (i / 700) % 4 {
                0 => data[i * 7 % data.len()],
                1 => 0x5A,
                2 => i as u8,
                _ => data[i],
            };
            data.push(b);
        }
        packed(&data);
    }
}
