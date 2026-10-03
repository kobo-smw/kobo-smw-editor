//! BPS patches: reading and writing.
//!
//! A BPS patch is `BPS1`, three numbers (source size, target size, metadata
//! size), the metadata, a list of actions, and a footer of three CRC32s: the
//! source's, the target's, and the patch's own up to that last one. Numbers
//! are a variable-length encoding in which each byte carries seven bits, the
//! last has bit 7 set, and every continuation adds one to what follows, so
//! each value has exactly one encoding.
//!
//! An action is a number whose low two bits say what it does and whose rest
//! is its length minus one:
//!
//! - `SourceRead`: copy the source bytes at the current output offset.
//! - `TargetRead`: copy that many bytes from the patch.
//! - `SourceCopy`: copy from anywhere in the source.
//! - `TargetCopy`: copy from earlier in the output, byte by byte, so a copy
//!   may overlap what it is writing (a run).
//!
//! The copies are followed by a signed offset (bit 0 the sign, the rest the
//! magnitude) that moves a cursor of their own, relative to where the last
//! copy of the same kind ended.
//!
//! Patches in the wild are made against the headerless SMW image or against
//! one with a 512-byte copier header; [`apply_to_rom`] tells them apart by
//! the source CRC. [`create`] always writes a patch against the headerless
//! image.

use thiserror::Error;

use crate::rom::{COPIER_HEADER_LEN, Rom};

/// The largest target [`apply`] builds. A tiny patch can describe an
/// enormous target, so its declared size is checked against this before
/// anything is allocated.
pub const MAX_TARGET_LEN: usize = 64 << 20;

const MAGIC: &[u8; 4] = b"BPS1";
const FOOTER_LEN: usize = 12;

const SOURCE_READ: u64 = 0;
const TARGET_READ: u64 = 1;
const SOURCE_COPY: u64 = 2;
const TARGET_COPY: u64 = 3;

const ACTION_NAMES: [&str; 4] = ["SourceRead", "TargetRead", "SourceCopy", "TargetCopy"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BpsError {
    #[error("patch is {0} bytes, too short to be a BPS patch")]
    TooShort(usize),
    #[error("not a BPS patch (no BPS1 signature)")]
    BadMagic,
    #[error("patch CRC32 is {actual:08x}, its footer says {expected:08x}: the file is damaged")]
    PatchCrc { expected: u32, actual: u32 },
    #[error("number at patch offset {0} runs into the footer")]
    Truncated(usize),
    #[error("number at patch offset {0} overflows 64 bits")]
    Overflow(usize),
    #[error("metadata of {len} bytes at patch offset {offset} runs into the footer")]
    Metadata { offset: usize, len: u64 },
    #[error("source is {actual} bytes, the patch wants {expected}")]
    SourceSize { expected: u64, actual: usize },
    #[error("source CRC32 is {actual:08x}, the patch wants {expected:08x}")]
    SourceCrc { expected: u32, actual: u32 },
    #[error(
        "the patch's source ({size} bytes, CRC32 {crc:08x}) is not this ROM, with or without a copier header"
    )]
    SourceMismatch { size: u64, crc: u32 },
    #[error("target of {0} bytes is over the {MAX_TARGET_LEN}-byte limit")]
    TooLarge(u64),
    #[error("{action} at patch offset {offset} goes outside the {region}")]
    OutOfBounds {
        action: &'static str,
        offset: usize,
        region: &'static str,
    },
    #[error("the actions write {actual} bytes, the header says {expected}")]
    TargetSize { expected: u64, actual: usize },
    #[error("target CRC32 is {actual:08x}, the patch wants {expected:08x}")]
    TargetCrc { expected: u32, actual: u32 },
}

/// A patch's header and footer, checked against its own CRC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchInfo<'a> {
    pub source_size: u64,
    pub target_size: u64,
    pub metadata: &'a [u8],
    pub source_crc: u32,
    pub target_crc: u32,
    /// Offset of the first action.
    actions: usize,
}

/// Reads a patch's header and footer, checking its signature and its own CRC.
pub fn info(patch: &[u8]) -> Result<PatchInfo<'_>, BpsError> {
    // Signature, three one-byte numbers, footer.
    if patch.len() < MAGIC.len() + 3 + FOOTER_LEN {
        return Err(BpsError::TooShort(patch.len()));
    }
    if &patch[..4] != MAGIC {
        return Err(BpsError::BadMagic);
    }
    let end = patch.len() - FOOTER_LEN;
    let footer = |i: usize| u32::from_le_bytes(patch[end + i..end + i + 4].try_into().unwrap());
    let actual = crc32(&patch[..patch.len() - 4]);
    if actual != footer(8) {
        return Err(BpsError::PatchCrc {
            expected: footer(8),
            actual,
        });
    }
    let mut r = Reader { patch, pos: 4, end };
    let source_size = r.number()?;
    let target_size = r.number()?;
    let metadata_len = r.number()?;
    let metadata = usize::try_from(metadata_len)
        .ok()
        .and_then(|len| patch[..end].get(r.pos..r.pos.checked_add(len)?))
        .ok_or(BpsError::Metadata {
            offset: r.pos,
            len: metadata_len,
        })?;
    Ok(PatchInfo {
        source_size,
        target_size,
        metadata,
        source_crc: footer(0),
        target_crc: footer(4),
        actions: r.pos + metadata.len(),
    })
}

/// Applies a patch to exactly the source it was made against.
pub fn apply(patch: &[u8], source: &[u8]) -> Result<Vec<u8>, BpsError> {
    apply_checked(patch, &info(patch)?, source)
}

/// [`apply`] with the patch's [`info`] already read and its CRC checked.
fn apply_checked(patch: &[u8], info: &PatchInfo, source: &[u8]) -> Result<Vec<u8>, BpsError> {
    if info.source_size != source.len() as u64 {
        return Err(BpsError::SourceSize {
            expected: info.source_size,
            actual: source.len(),
        });
    }
    let actual = crc32(source);
    if actual != info.source_crc {
        return Err(BpsError::SourceCrc {
            expected: info.source_crc,
            actual,
        });
    }
    let target_len = usize::try_from(info.target_size)
        .ok()
        .filter(|&len| len <= MAX_TARGET_LEN)
        .ok_or(BpsError::TooLarge(info.target_size))?;
    let mut target = vec![0u8; target_len];
    let mut r = Reader {
        patch,
        pos: info.actions,
        end: patch.len() - FOOTER_LEN,
    };
    let (mut out, mut source_rel, mut target_rel) = (0usize, 0usize, 0usize);
    while r.pos < r.end {
        let offset = r.pos;
        let word = r.number()?;
        let action = word & 3;
        let bounds = |region| BpsError::OutOfBounds {
            action: ACTION_NAMES[action as usize],
            offset,
            region,
        };
        // Lengths are at most 2^62, so neither the cast nor the add overflows
        // on a 64-bit target; a 32-bit one saturates into the check below.
        let len = usize::try_from((word >> 2) + 1).unwrap_or(usize::MAX);
        let out_end = out
            .checked_add(len)
            .filter(|&e| e <= target_len)
            .ok_or(bounds("target"))?;
        match action {
            SOURCE_READ => {
                let from = source.get(out..out_end).ok_or(bounds("source"))?;
                target[out..out_end].copy_from_slice(from);
            }
            TARGET_READ => {
                let from = r
                    .pos
                    .checked_add(len)
                    .filter(|&e| e <= r.end)
                    .map(|e| &patch[r.pos..e])
                    .ok_or(bounds("patch"))?;
                target[out..out_end].copy_from_slice(from);
                r.pos += len;
            }
            SOURCE_COPY => {
                source_rel = r.relative(source_rel)?.ok_or(bounds("source"))?;
                let from = source_rel
                    .checked_add(len)
                    .and_then(|e| source.get(source_rel..e))
                    .ok_or(bounds("source"))?;
                target[out..out_end].copy_from_slice(from);
                source_rel += len;
            }
            _ => {
                target_rel = r.relative(target_rel)?.ok_or(bounds("target"))?;
                // Only what has been written can be read, a byte at a time.
                if target_rel >= out {
                    return Err(bounds("target written so far"));
                }
                if target_rel + len <= out {
                    target.copy_within(target_rel..target_rel + len, out);
                } else {
                    for i in 0..len {
                        target[out + i] = target[target_rel + i];
                    }
                }
                target_rel += len;
            }
        }
        out = out_end;
    }
    if out != target_len {
        return Err(BpsError::TargetSize {
            expected: info.target_size,
            actual: out,
        });
    }
    let actual = crc32(&target);
    if actual != info.target_crc {
        return Err(BpsError::TargetCrc {
            expected: info.target_crc,
            actual,
        });
    }
    Ok(target)
}

/// Which form of a ROM's clean image a patch turned out to be made against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceForm {
    /// The headerless image.
    Headerless,
    /// The image behind the copier header the ROM was loaded with.
    OwnHeader,
    /// The image behind a header of its size in 8 KiB units and zeros
    /// (`40 00 00 ...` for SMW), which headered dumps and the hacks made
    /// from them carry.
    SizeHeader,
    /// The image behind 512 zero bytes, as `apply_bps.py` writes.
    ZeroHeader,
}

/// A patch applied to a [`Rom`].
#[derive(Debug)]
pub struct Patched {
    /// The headerless target.
    pub data: Vec<u8>,
    pub source: SourceForm,
}

/// Applies a patch to a ROM's clean image, whichever way the patch was made.
///
/// The patch's source size and CRC pick the form, one of [`SourceForm`]'s.
/// The header a headered patch was made with cannot be read from the patch,
/// only checked, so the headers tried are the ROM's own and the two that
/// headered SMW images are seen with; `apply_bps.py` tried the file as it
/// was and a zero header. A target of a copier-header size loses its
/// header, so the result is always headerless.
pub fn apply_to_rom(patch: &[u8], rom: &Rom) -> Result<Patched, BpsError> {
    let info = info(patch)?;
    let finish = |source: &[u8], form| {
        let mut data = apply_checked(patch, &info, source)?;
        // A target of whole banks and a header is a headered image, whatever
        // the source was.
        if data.len() % 0x8000 == COPIER_HEADER_LEN {
            data.drain(..COPIER_HEADER_LEN);
        }
        Ok(Patched { data, source: form })
    };
    if info.source_size == rom.len() as u64 && info.source_crc == crc32(rom.data()) {
        return finish(rom.data(), SourceForm::Headerless);
    }
    if info.source_size == (COPIER_HEADER_LEN + rom.len()) as u64 {
        let mut size = [0; COPIER_HEADER_LEN];
        size[..2].copy_from_slice(&((rom.len() / 0x2000) as u16).to_le_bytes());
        let headers = [
            (SourceForm::OwnHeader, rom.copier_header()),
            (SourceForm::SizeHeader, Some(&size[..])),
            (SourceForm::ZeroHeader, Some(&[0; COPIER_HEADER_LEN][..])),
        ];
        for (form, header) in headers {
            let Some(header) = header else { continue };
            let source = [header, rom.data()].concat();
            if crc32(&source) == info.source_crc {
                return finish(&source, form);
            }
        }
    }
    Err(BpsError::SourceMismatch {
        size: info.source_size,
        crc: info.source_crc,
    })
}

struct Reader<'a> {
    patch: &'a [u8],
    pos: usize,
    /// Where the actions end and the footer starts.
    end: usize,
}

impl Reader<'_> {
    fn number(&mut self) -> Result<u64, BpsError> {
        let start = self.pos;
        let mut value = 0u64;
        let mut shift = 1u64;
        loop {
            if self.pos >= self.end {
                return Err(BpsError::Truncated(start));
            }
            let byte = self.patch[self.pos];
            self.pos += 1;
            value = u64::from(byte & 0x7F)
                .checked_mul(shift)
                .and_then(|v| v.checked_add(value))
                .ok_or(BpsError::Overflow(start))?;
            if byte & 0x80 != 0 {
                return Ok(value);
            }
            shift = shift
                .checked_mul(0x80)
                .filter(|&s| s <= 1 << 63)
                .ok_or(BpsError::Overflow(start))?;
            value = value.checked_add(shift).ok_or(BpsError::Overflow(start))?;
        }
    }

    /// Reads a signed offset and applies it to a copy cursor. `None` when
    /// it moves the cursor before the start or past what fits in a `usize`.
    fn relative(&mut self, cursor: usize) -> Result<Option<usize>, BpsError> {
        let n = self.number()?;
        let Ok(magnitude) = usize::try_from(n >> 1) else {
            return Ok(None);
        };
        Ok(if n & 1 == 0 {
            cursor.checked_add(magnitude)
        } else {
            cursor.checked_sub(magnitude)
        })
    }
}

/// CRC-32 (IEEE 802.3, reflected), the checksum BPS, zip, and PNG use.
pub fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |crc, &b| {
        CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8)
    })
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Makes a patch from `source` to `target`, with no metadata.
///
/// The output depends on nothing but the two inputs. It is greedy: at each
/// target offset it takes whichever of these saves the most patch bytes
/// once its own encoding is paid for, and otherwise adds the byte to a
/// pending `TargetRead`:
///
/// - a `SourceRead` of the bytes that are unchanged in place;
/// - a `SourceCopy` from one of the last 16 source offsets whose next four
///   bytes hash as the target's do (moved or duplicated data);
/// - a `TargetCopy` from one of the last 16 such offsets already written
///   (repeats, and runs such as expanded free space, by overlapping).
///
/// Ties go to that order, then to the most recent offset. A match has to
/// save two bytes, since breaking a `TargetRead` costs a header. Hash
/// chains keep the work per byte bounded: an 8 MiB image takes about a
/// second in a release build.
pub fn create(source: &[u8], target: &[u8]) -> Vec<u8> {
    let mut w = Writer {
        patch: MAGIC.to_vec(),
        source_rel: 0,
        target_rel: 0,
    };
    w.number(source.len() as u64);
    w.number(target.len() as u64);
    w.number(0);

    let mut source_index = Index::new(source.len());
    for pos in 0..source.len() {
        source_index.insert(source, pos);
    }
    let mut target_index = Index::new(target.len());
    let (mut pos, mut literal, mut indexed) = (0, 0, 0);
    while pos < target.len() {
        while indexed < pos {
            target_index.insert(target, indexed);
            indexed += 1;
        }
        match best_match(source, target, pos, &source_index, &target_index, &w) {
            Some(m) => {
                w.target_read(&target[literal..pos]);
                w.action(m, pos);
                pos += m.len;
                literal = pos;
            }
            None => pos += 1,
        }
    }
    w.target_read(&target[literal..]);

    let source_crc = crc32(source);
    let target_crc = crc32(target);
    w.patch.extend_from_slice(&source_crc.to_le_bytes());
    w.patch.extend_from_slice(&target_crc.to_le_bytes());
    let patch_crc = crc32(&w.patch);
    w.patch.extend_from_slice(&patch_crc.to_le_bytes());
    w.patch
}

/// Bytes hashed to find copy candidates.
const WINDOW: usize = 4;
/// Candidates tried per index at each target offset.
const CHAIN: usize = 16;
/// Patch bytes a match has to save over its own encoding.
const MIN_GAIN: usize = 2;
/// Marks the end of a hash chain.
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
struct Match {
    action: u64,
    /// Where a copy reads from.
    from: usize,
    len: usize,
}

fn best_match(
    source: &[u8],
    target: &[u8],
    pos: usize,
    source_index: &Index,
    target_index: &Index,
    w: &Writer,
) -> Option<Match> {
    let rest = &target[pos..];
    let mut best: Option<(Match, usize)> = None;
    let mut consider = |m: Match, cost: usize| {
        let gain = m.len.saturating_sub(cost);
        if gain >= MIN_GAIN && best.is_none_or(|(_, g)| gain > g) {
            best = Some((m, gain));
        }
    };
    if let Some(same) = source.get(pos..) {
        let len = common_prefix(same, rest);
        if len > 0 {
            let m = Match {
                action: SOURCE_READ,
                from: pos,
                len,
            };
            consider(m, number_len(header(m)));
        }
    }
    if rest.len() >= WINDOW {
        let window = &rest[..WINDOW];
        for (index, data, action, cursor) in [
            (source_index, source, SOURCE_COPY, w.source_rel),
            (target_index, target, TARGET_COPY, w.target_rel),
        ] {
            for from in index.chain(window).take(CHAIN) {
                let len = common_prefix(&data[from..], rest);
                if len < WINDOW {
                    continue; // a hash collision
                }
                let m = Match { action, from, len };
                consider(m, number_len(header(m)) + number_len(signed(from, cursor)));
            }
        }
    }
    best.map(|(m, _)| m)
}

fn header(m: Match) -> u64 {
    ((m.len as u64 - 1) << 2) | m.action
}

/// A copy cursor move, as the patch encodes it.
fn signed(to: usize, from: usize) -> u64 {
    if to >= from {
        ((to - from) as u64) << 1
    } else {
        (((from - to) as u64) << 1) | 1
    }
}

fn number_len(mut n: u64) -> usize {
    let mut len = 1;
    while n >= 0x80 {
        n = (n >> 7) - 1;
        len += 1;
    }
    len
}

/// The length of the common prefix, eight bytes at a time.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let len = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= len {
        let x = u64::from_le_bytes(a[i..i + 8].try_into().unwrap());
        let y = u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        if x != y {
            return i + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        i += 8;
    }
    while i < len && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Offsets by the hash of the [`WINDOW`] bytes there, newest first.
struct Index {
    bits: u32,
    head: Vec<u32>,
    prev: Vec<u32>,
}

impl Index {
    fn new(len: usize) -> Self {
        // Roughly one bucket per offset, within 1 KiB to 4 Mi buckets.
        let bits = (usize::BITS - len.leading_zeros()).clamp(10, 22);
        Self {
            bits,
            head: vec![NONE; 1 << bits],
            prev: vec![NONE; len.min(NONE as usize)],
        }
    }

    fn bucket(&self, window: &[u8]) -> usize {
        let w = u32::from_le_bytes(window[..WINDOW].try_into().unwrap());
        (w.wrapping_mul(0x9E37_79B1) >> (32 - self.bits)) as usize
    }

    fn insert(&mut self, data: &[u8], pos: usize) {
        if pos + WINDOW > data.len() || pos >= self.prev.len() {
            return;
        }
        let b = self.bucket(&data[pos..]);
        self.prev[pos] = self.head[b];
        self.head[b] = pos as u32;
    }

    fn chain(&self, window: &[u8]) -> impl Iterator<Item = usize> + '_ {
        let mut next = self.head[self.bucket(window)];
        std::iter::from_fn(move || {
            let pos = (next != NONE).then_some(next as usize)?;
            next = self.prev[pos];
            Some(pos)
        })
    }
}

struct Writer {
    patch: Vec<u8>,
    source_rel: usize,
    target_rel: usize,
}

impl Writer {
    fn number(&mut self, mut n: u64) {
        loop {
            let low = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                self.patch.push(0x80 | low);
                return;
            }
            self.patch.push(low);
            n -= 1;
        }
    }

    fn target_read(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.number(((bytes.len() as u64 - 1) << 2) | TARGET_READ);
            self.patch.extend_from_slice(bytes);
        }
    }

    fn action(&mut self, m: Match, pos: usize) {
        self.number(header(m));
        match m.action {
            SOURCE_COPY => {
                self.number(signed(m.from, self.source_rel));
                self.source_rel = m.from + m.len;
            }
            TARGET_COPY => {
                self.number(signed(m.from, self.target_rel));
                self.target_rel = m.from + m.len;
            }
            _ => debug_assert_eq!(m.from, pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random(seed: u64, len: usize) -> Vec<u8> {
        let mut s = seed | 1;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s as u8
            })
            .collect()
    }

    /// The actions of a valid patch, as (action, length).
    fn actions(patch: &[u8]) -> Vec<(u64, usize)> {
        let info = info(patch).unwrap();
        let mut r = Reader {
            patch,
            pos: info.actions,
            end: patch.len() - FOOTER_LEN,
        };
        let mut list = Vec::new();
        while r.pos < r.end {
            let word = r.number().unwrap();
            let len = (word >> 2) as usize + 1;
            match word & 3 {
                TARGET_READ => r.pos += len,
                SOURCE_COPY | TARGET_COPY => {
                    r.number().unwrap();
                }
                _ => {}
            }
            list.push((word & 3, len));
        }
        list
    }

    fn round_trip(source: &[u8], target: &[u8]) -> Vec<u8> {
        let patch = create(source, target);
        assert_eq!(apply(&patch, source).unwrap(), target);
        patch
    }

    /// Hand-assembles a patch, footer CRCs included.
    struct Build(Writer);

    impl Build {
        fn new(source: usize, target: usize) -> Self {
            let mut w = Writer {
                patch: MAGIC.to_vec(),
                source_rel: 0,
                target_rel: 0,
            };
            w.number(source as u64);
            w.number(target as u64);
            w.number(0);
            Build(w)
        }

        fn raw(mut self, n: u64) -> Self {
            self.0.number(n);
            self
        }

        fn bytes(mut self, b: &[u8]) -> Self {
            self.0.patch.extend_from_slice(b);
            self
        }

        fn finish(self, source: &[u8], target: &[u8]) -> Vec<u8> {
            let mut patch = self.0.patch;
            patch.extend_from_slice(&crc32(source).to_le_bytes());
            patch.extend_from_slice(&crc32(target).to_le_bytes());
            let crc = crc32(&patch);
            patch.extend_from_slice(&crc.to_le_bytes());
            patch
        }
    }

    fn refresh_crc(patch: &mut [u8]) {
        let n = patch.len();
        let crc = crc32(&patch[..n - 4]);
        patch[n - 4..].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn numbers_round_trip_and_reject_overflow() {
        for n in [0, 1, 0x7F, 0x80, 0x407F, 0x4080, u32::MAX as u64, u64::MAX] {
            let mut w = Writer {
                patch: Vec::new(),
                source_rel: 0,
                target_rel: 0,
            };
            w.number(n);
            assert_eq!(w.patch.len(), number_len(n), "{n:#x}");
            let end = w.patch.len();
            let mut r = Reader {
                patch: &w.patch,
                pos: 0,
                end,
            };
            assert_eq!(r.number(), Ok(n));
            let mut r = Reader {
                patch: &w.patch,
                pos: 0,
                end: end - 1,
            };
            assert_eq!(r.number(), Err(BpsError::Truncated(0)));
        }
        let long = [0u8; 11];
        let mut r = Reader {
            patch: &long,
            pos: 0,
            end: 11,
        };
        assert_eq!(r.number(), Err(BpsError::Overflow(0)));
    }

    #[test]
    fn each_action_applies() {
        let source = b"abcdefgh";
        let target = b"abcXYZZZZZcdefgh";
        let patch = Build::new(8, 16)
            .raw(((3 - 1) << 2) | SOURCE_READ) // abc
            .raw(((3 - 1) << 2) | TARGET_READ) // XYZ
            .bytes(b"XYZ")
            .raw(((4 - 1) << 2) | TARGET_COPY) // ZZZZ from the Z at 5, overlapping
            .raw(5 << 1)
            .raw(((4 - 1) << 2) | SOURCE_COPY) // cdef, cursor 0 to 2
            .raw(2 << 1)
            .raw(((2 - 1) << 2) | SOURCE_COPY) // gh, where the last copy ended
            .raw(0)
            .finish(source, target);
        assert_eq!(apply(&patch, source).unwrap(), target);
        let _ = info(&patch).unwrap();
    }

    #[test]
    fn negative_offsets_move_cursors_back() {
        let source = b"0123456789";
        let target = b"789012";
        let patch = Build::new(10, 6)
            .raw(((3 - 1) << 2) | SOURCE_COPY)
            .raw(7 << 1) // cursor 0 -> 7, ends at 10
            .raw(((3 - 1) << 2) | SOURCE_COPY)
            .raw((10 << 1) | 1) // 10 -> 0
            .finish(source, target);
        assert_eq!(apply(&patch, source).unwrap(), target);
    }

    #[test]
    fn create_picks_each_action() {
        let source = random(1, 4096);
        // Unchanged: one SourceRead.
        assert_eq!(
            actions(&round_trip(&source, &source)),
            [(SOURCE_READ, 4096)]
        );
        // New bytes: one TargetRead.
        let fresh = random(2, 100);
        assert_eq!(actions(&round_trip(&[], &fresh)), [(TARGET_READ, 100)]);
        // A moved block: a SourceCopy.
        let mut moved = source.clone();
        moved[..512].copy_from_slice(&source[2048..2560]);
        let list = actions(&round_trip(&source, &moved));
        assert_eq!(list, [(SOURCE_COPY, 512), (SOURCE_READ, 4096 - 512)]);
        // A run: one literal byte and a TargetCopy over itself.
        let mut run = source.clone();
        run[100..1100].fill(0xEE);
        let list = actions(&round_trip(&source, &run));
        assert!(list.contains(&(TARGET_COPY, 999)), "{list:?}");
        // A repeat of new data: a TargetCopy.
        let target = [fresh.as_slice(), &fresh].concat();
        let list = actions(&round_trip(&[], &target));
        assert_eq!(list, [(TARGET_READ, 100), (TARGET_COPY, 100)]);
    }

    #[test]
    fn round_trips_random_and_structured_inputs() {
        for seed in 1..40u64 {
            let a = random(seed, (seed as usize * 37) % 3000);
            let b = random(seed + 1000, (seed as usize * 53) % 3000);
            round_trip(&a, &b);
            round_trip(&b, &a);
        }
        // A ROM-like edit: changes, an insertion, a deletion, a moved and
        // a duplicated block, and expansion into filled free space.
        let source: Vec<u8> = random(7, 0x2_0000)
            .iter()
            .enumerate()
            .map(|(i, &b)| if i % 0x4000 < 0x1000 { 0xFF } else { b })
            .collect();
        let mut target = source.clone();
        target[0x100..0x140].copy_from_slice(&random(8, 0x40));
        target.splice(0x5000..0x5000, random(9, 0x333));
        target.drain(0x9000..0x9100);
        let block = source[0x12000..0x13000].to_vec();
        target[0x2000..0x3000].copy_from_slice(&block);
        target.extend_from_slice(&block);
        target.resize(0x4_0000, 0);
        let patch = round_trip(&source, &target);
        assert!(patch.len() < 0x800, "{} bytes", patch.len());
        // Shrinking.
        round_trip(&target, &source);
        round_trip(&source, &source[..0x1_0000]);
    }

    #[test]
    fn empty_and_equal_inputs() {
        assert_eq!(apply(&round_trip(&[], &[]), &[]).unwrap(), b"");
        round_trip(b"abc", &[]);
        round_trip(&[], b"abc");
        let data = random(3, 0x1_0000);
        assert!(round_trip(&data, &data).len() < 32);
    }

    #[test]
    fn create_is_deterministic() {
        let source = random(11, 0x8000);
        let mut target = source.clone();
        target[0x1000..0x1800].copy_from_slice(&random(12, 0x800));
        target.extend_from_slice(&source[..0x3000]);
        target.resize(0x1_0000, 0xFF);
        let patch = create(&source, &target);
        assert_eq!(create(&source, &target), patch);
        // Pins the output on every CI platform. A deliberate change to
        // `create` updates this value.
        assert_eq!(
            (patch.len(), crc32(&patch)),
            (2090, 0x2144_DF1C),
            "{:#010x}",
            crc32(&patch)
        );
    }

    /// Patches Floating IPS made (commit `ff216a7`, `--create --bps-delta`
    /// and `--bps-linear`) of these inputs, which Kobo must apply as the
    /// spec says: a check that does not rest on Kobo's own encoder.
    #[test]
    fn patches_from_flips_apply() {
        let source: Vec<u8> = (0..2048u32).map(|i| (i * 37 + (i >> 5)) as u8).collect();
        let mut target = source.clone();
        target[100..116].copy_from_slice(b"Kobo BPS vector!");
        let copied = source[1024..1280].to_vec();
        target[512..768].copy_from_slice(&copied);
        target.copy_within(1400..1500, 1500);
        target.extend([0x55; 64]);
        for patch in [
            &include_bytes!("../tests/fixtures/flips-delta.bps")[..],
            &include_bytes!("../tests/fixtures/flips-linear.bps")[..],
        ] {
            assert_eq!(apply(patch, &source).unwrap(), target);
        }
    }

    #[test]
    fn metadata_is_read_and_skipped() {
        let (source, target) = (b"source bytes".to_vec(), b"target bytes, longer".to_vec());
        let plain = create(&source, &target);
        // After the magic and the two sizes, a metadata length of 0 (0x80):
        // put three bytes of metadata there and sign the patch again.
        let at = 4 + number_len(source.len() as u64) + number_len(target.len() as u64);
        assert_eq!(plain[at], 0x80);
        let mut patch = plain[..at].to_vec();
        patch.push(0x83);
        patch.extend_from_slice(b"<a>");
        patch.extend_from_slice(&plain[at + 1..plain.len() - 4]);
        let crc = crc32(&patch);
        patch.extend_from_slice(&crc.to_le_bytes());
        assert_eq!(info(&patch).unwrap().metadata, b"<a>");
        assert_eq!(apply(&patch, &source).unwrap(), target);
    }

    #[test]
    fn crc_mismatches_are_errors() {
        let source = random(4, 1000);
        let fresh = random(40, 10);
        let mut target = source.clone();
        target[10..20].copy_from_slice(&fresh);
        let patch = create(&source, &target);

        let mut wrong_source = source.clone();
        wrong_source[500] ^= 1;
        assert!(matches!(
            apply(&patch, &wrong_source),
            Err(BpsError::SourceCrc { .. })
        ));
        assert!(matches!(
            apply(&patch, &source[1..]),
            Err(BpsError::SourceSize { .. })
        ));

        let mut damaged = patch.clone();
        damaged[8] ^= 0x40;
        assert!(matches!(
            apply(&damaged, &source),
            Err(BpsError::PatchCrc { .. })
        ));

        // A changed TargetRead byte, with the patch's own CRC fixed up.
        let at = patch.windows(10).position(|w| w == fresh).unwrap();
        let mut forged = patch.clone();
        forged[at] ^= 1;
        refresh_crc(&mut forged);
        assert!(matches!(
            apply(&forged, &source),
            Err(BpsError::TargetCrc { .. })
        ));
    }

    #[test]
    fn truncated_and_garbage_patches_are_errors() {
        let source = random(5, 3000);
        let mut target = random(6, 2000);
        target.extend_from_slice(&source[..1000]);
        let patch = create(&source, &target);
        for n in 0..patch.len() {
            assert!(apply(&patch[..n], &source).is_err(), "prefix {n}");
            let mut fixed = patch[..n].to_vec();
            if n >= 4 {
                refresh_crc(&mut fixed);
            }
            assert!(apply(&fixed, &source).is_err(), "prefix {n}, CRC fixed");
        }
        for seed in 0..200 {
            let mut garbage = random(seed, seed as usize * 3);
            if garbage.len() >= 4 {
                garbage[..4].copy_from_slice(MAGIC);
            }
            if garbage.len() >= 8 {
                refresh_crc(&mut garbage);
            }
            let _ = apply(&garbage, &source);
        }
        assert_eq!(apply(b"UPS1", &source), Err(BpsError::TooShort(4)));
        let mut not_bps = vec![0u8; 32];
        not_bps[..4].copy_from_slice(b"UPS1");
        assert_eq!(apply(&not_bps, &source), Err(BpsError::BadMagic));
    }

    #[test]
    fn malformed_actions_are_errors() {
        let source = b"abcdefgh";
        let out_of = |patch: Vec<u8>| match apply(&patch, source) {
            Err(BpsError::OutOfBounds { region, .. }) => region,
            other => panic!("{other:?}"),
        };
        // SourceRead past the end of the source.
        let t = [0u8; 10];
        let p = Build::new(8, 10)
            .raw((9 << 2) | SOURCE_READ)
            .finish(source, &t);
        assert_eq!(out_of(p), "source");
        // An action past the declared target.
        let p = Build::new(8, 4)
            .raw((4 << 2) | SOURCE_READ)
            .finish(source, b"abcd");
        assert_eq!(out_of(p), "target");
        // TargetRead past the actions.
        let p = Build::new(8, 4)
            .raw((3 << 2) | TARGET_READ)
            .bytes(b"ab")
            .finish(source, b"abcd");
        assert!(apply(&p, source).is_err());
        // SourceCopy before the start and past the end.
        let p = Build::new(8, 1)
            .raw(SOURCE_COPY)
            .raw(3) // -1
            .finish(source, b"a");
        assert_eq!(out_of(p), "source");
        let p = Build::new(8, 2)
            .raw((1 << 2) | SOURCE_COPY)
            .raw(7 << 1)
            .finish(source, b"hh");
        assert_eq!(out_of(p), "source");
        // TargetCopy of nothing written yet.
        let p = Build::new(8, 1)
            .raw(TARGET_COPY)
            .raw(0)
            .finish(source, b"a");
        assert_eq!(out_of(p), "target written so far");
        // Too few bytes written.
        let p = Build::new(8, 4)
            .raw((1 << 2) | SOURCE_READ)
            .finish(source, b"abcd");
        assert!(matches!(
            apply(&p, source),
            Err(BpsError::TargetSize { .. })
        ));
        // An enormous declared target is refused before it is allocated.
        let p = Build::new(8, MAX_TARGET_LEN + 1).finish(source, b"");
        assert!(matches!(apply(&p, source), Err(BpsError::TooLarge(_))));
    }

    /// A 32 KiB LoROM image.
    fn small_rom() -> Vec<u8> {
        let mut data = random(21, 0x8000);
        data[0x7FD5] = 0x20;
        data
    }

    #[test]
    fn applies_to_rom_images_with_and_without_a_header() {
        let clean = small_rom();
        let mut target = clean.clone();
        target[0x100..0x110].fill(0x42);
        target.resize(0x1_0000, 0);
        let rom = Rom::from_bytes(clean.clone()).unwrap();

        let plain = create(&clean, &target);
        let p = apply_to_rom(&plain, &rom).unwrap();
        assert_eq!((p.data, p.source), (target.clone(), SourceForm::Headerless));

        let zero = [&[0u8; COPIER_HEADER_LEN][..], &clean].concat();
        let headered_target = [&[0u8; COPIER_HEADER_LEN][..], &target].concat();
        let headered = create(&zero, &headered_target);
        let p = apply_to_rom(&headered, &rom).unwrap();
        assert_eq!((p.data, p.source), (target.clone(), SourceForm::ZeroHeader));

        // 32 KiB is four 8 KiB units.
        let mut size = [0u8; COPIER_HEADER_LEN];
        size[0] = 4;
        let sized = create(
            &[&size[..], &clean].concat(),
            &[&size[..], &target].concat(),
        );
        let p = apply_to_rom(&sized, &rom).unwrap();
        assert_eq!((p.data, p.source), (target.clone(), SourceForm::SizeHeader));

        // A ROM loaded with its own header matches a patch made with it.
        let own = [&[0x11u8; COPIER_HEADER_LEN][..], &clean].concat();
        let own_rom = Rom::from_bytes(own.clone()).unwrap();
        let with_own = create(&own, &[&[0x11u8; COPIER_HEADER_LEN][..], &target].concat());
        let p = apply_to_rom(&with_own, &own_rom).unwrap();
        assert_eq!((p.data, p.source), (target.clone(), SourceForm::OwnHeader));
        assert!(matches!(
            apply_to_rom(&with_own, &rom),
            Err(BpsError::SourceMismatch { .. })
        ));
        // Headerless and zero-headered patches apply to it as well.
        assert_eq!(apply_to_rom(&plain, &own_rom).unwrap().data, target);
        assert_eq!(apply_to_rom(&headered, &own_rom).unwrap().data, target);
    }
}
