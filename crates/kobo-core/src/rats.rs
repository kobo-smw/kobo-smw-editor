//! RATS tags and the free space between them.
//!
//! A RATS tag is the eight bytes before a block: `STAR`, then the block's
//! length minus one and that value's complement, both 16-bit little-endian.
//! Kobo treats runs of `$00` outside valid tags as free space and keeps
//! each block's contents within one bank. It places blocks first-fit in
//! a fixed order, so the same image and requests give the same addresses.
//! Its bank preferences and tag placement follow Asar's `freecode` and
//! `freedata`, but placement is not guaranteed to match Asar byte for byte.
//!
//! Asar 1.91 can skip a valid tag when advancing to a bank boundary and
//! overwrite zeros inside the protected block. Kobo preserves that block,
//! including after a rescan. Tags alone therefore do not guarantee safety
//! when a later tool runs, so a [`Snapshot`] of the blocks taken before it
//! runs finds any it damaged; `docs/toolchain.md` has the reproduction.

use std::fmt;
use std::ops::Range;

use thiserror::Error;

use crate::addr::{Mapping, PcAddr, SnesAddr};
use crate::rom::{Rom, RomError};

/// Length of a RATS tag.
pub const TAG_LEN: usize = 8;

/// The longest block a tag can protect.
pub const MAX_BLOCK_LEN: usize = 0x1_0000;

const MAGIC: &[u8; 4] = b"STAR";

/// Where the searches start: `$108000` under every mapping, the first byte
/// past the vanilla image.
const SEARCH_START: usize = 0x08_0000;

/// The length of the block protected by a valid tag at file offset `pc`.
/// A tag whose block would run past the end of the image is not one: the
/// bytes it claims are not there to protect.
pub fn tag_at(data: &[u8], pc: usize) -> Option<usize> {
    let tag = data.get(pc..pc.checked_add(TAG_LEN)?)?;
    let valid = &tag[..4] == MAGIC && tag[4] ^ tag[6] == 0xFF && tag[5] ^ tag[7] == 0xFF;
    let len = u16::from_le_bytes([tag[4], tag[5]]) as usize + 1;
    (valid && pc + TAG_LEN + len <= data.len()).then_some(len)
}

/// The tag for a block of `len` bytes, 1 to [`MAX_BLOCK_LEN`].
fn tag(len: usize) -> [u8; TAG_LEN] {
    let [lo, hi] = ((len - 1) as u16).to_le_bytes();
    [b'S', b'T', b'A', b'R', lo, hi, !lo, !hi]
}

/// A tagged block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RatsBlock {
    /// The first byte after the tag.
    pub start: SnesAddr,
    pub len: usize,
}

/// The tagged blocks from `$108000` on, in file order. As in Asar's walk,
/// a block is skipped whole, so a tag inside one is not another.
pub fn blocks(rom: &Rom) -> Vec<RatsBlock> {
    let data = rom.data();
    let mut found = Vec::new();
    let mut pc = SEARCH_START;
    while pc < data.len() {
        let Some(len) = tag_at(data, pc) else {
            pc += 1;
            continue;
        };
        if let Ok(start) = rom.mapping().pc_to_snes(PcAddr::new((pc + TAG_LEN) as u32)) {
            found.push(RatsBlock { start, len });
        }
        pc += TAG_LEN + len;
    }
    found
}

/// The byte a block is erased to, and free space is made of.
pub const FREE_BYTE: u8 = 0x00;

/// Every tagged block of an image, tag and contents, as they were before
/// a tool ran.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    blocks: Vec<Saved>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Saved {
    block: RatsBlock,
    /// The file offset of the tag.
    tag: usize,
    /// The tag and the contents.
    bytes: Vec<u8>,
}

/// A block a tool changed without releasing it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Damage {
    pub block: RatsBlock,
    /// The first and last changed bytes.
    pub changed: (SnesAddr, SnesAddr),
    /// Whether the block's tag still stands. If not, the tool erased or
    /// rewrote the tag but left part of the old contents behind.
    pub tagged: bool,
}

impl fmt::Display for Damage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (first, last) = self.changed;
        write!(
            f,
            "RATS block at {} (${:X} bytes) changed at {first}-{last}",
            self.block.start, self.block.len
        )?;
        if self.tagged {
            write!(f, " under its tag")
        } else {
            write!(f, ", its tag gone but its contents not erased")
        }
    }
}

impl Snapshot {
    pub fn take(rom: &Rom) -> Self {
        let blocks = blocks(rom)
            .into_iter()
            .map(|block| {
                let tag = rom
                    .pc(block.start)
                    .expect("found blocks are mapped")
                    .as_usize()
                    - TAG_LEN;
                let bytes = rom.data()[tag..tag + TAG_LEN + block.len].to_vec();
                Saved { block, tag, bytes }
            })
            .collect();
        Self { blocks }
    }

    /// The blocks, in file order.
    pub fn blocks(&self) -> impl Iterator<Item = RatsBlock> + '_ {
        self.blocks.iter().map(|saved| saved.block)
    }

    /// The blocks a tool damaged in producing `after`, in file order.
    ///
    /// A block is intact if its tag and contents are unchanged. It is
    /// released if its tag is gone, or `written` (the file ranges the
    /// tool reports writing) says the tool rewrote it, and each of its
    /// bytes is now [`FREE_BYTE`] or inside a tagged block of `after`:
    /// Asar's `autoclean` erases a block whole, and the same patch may put
    /// new blocks in the space. Any other change is damage, such as the
    /// write under a standing tag that Asar 1.91's bank-boundary search
    /// makes. A tool that does not report its writes passes no ranges; a
    /// block it rewrites in place under a tag of the same length then
    /// counts as damaged.
    pub fn check(&self, after: &Rom, written: &[Range<usize>]) -> Vec<Damage> {
        let data = after.data();
        let tagged: Vec<Range<usize>> = blocks(after)
            .into_iter()
            .filter_map(|block| {
                let start = after.pc(block.start).ok()?.as_usize();
                Some(start - TAG_LEN..start + block.len)
            })
            .collect();
        let mut damage = Vec::new();
        for saved in &self.blocks {
            let span = saved.tag..saved.tag + saved.bytes.len();
            let now = data.get(span.clone()).unwrap_or_default();
            if now == saved.bytes {
                continue;
            }
            let tag = saved.tag..saved.tag + TAG_LEN;
            let tag_stands = now.get(..TAG_LEN) == Some(&saved.bytes[..TAG_LEN])
                && !written
                    .iter()
                    .any(|w| w.start < tag.end && tag.start < w.end);
            let released = !tag_stands
                && now.len() == saved.bytes.len()
                && now
                    .iter()
                    .zip(span.clone())
                    .all(|(&byte, pc)| byte == FREE_BYTE || tagged.iter().any(|t| t.contains(&pc)));
            if released {
                continue;
            }
            let changed = |pc: &usize| data.get(*pc) != Some(&saved.bytes[pc - saved.tag]);
            let first = span.clone().find(changed).expect("a byte changed");
            let last = span.clone().rev().find(changed).expect("a byte changed");
            let snes = |pc: usize| {
                after
                    .mapping()
                    .pc_to_snes(PcAddr::new(pc as u32))
                    .expect("the block was mapped")
            };
            damage.push(Damage {
                block: saved.block,
                changed: (snes(first), snes(last)),
                tagged: tag_stands,
            });
        }
        damage
    }
}

/// What a block holds, which decides where it may go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contents {
    /// Code, which goes in a system bank (see [`SnesAddr::in_system_bank`]).
    Code,
    /// Data, which goes outside the system banks first, to save them for
    /// code, as Asar's `freedata` does.
    Data,
}

#[derive(Debug, Error)]
pub enum FreeSpaceError {
    #[error("a RATS block holds 1 to 65536 bytes, not {0}")]
    BadLength(usize),
    #[error("no free space for {len} bytes of {contents:?} within one bank")]
    Full { len: usize, contents: Contents },
    #[error(transparent)]
    Rom(#[from] RomError),
}

/// A run of free bytes, as file offsets.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Run {
    start: usize,
    end: usize,
}

/// An image's free space: the runs of `$00` from `$108000` on that no tag
/// covers. Scan it once a stage's writes to fixed addresses are done and
/// place everything else the stage writes through it: it does not see
/// what is written into free space behind its back.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreeSpace {
    mapping: Mapping,
    runs: Vec<Run>,
}

impl FreeSpace {
    pub fn scan(rom: &Rom) -> Self {
        let mapping = rom.mapping();
        let data = &rom.data()[..rom.len().min(mapping.max_rom_len())];
        let mut runs = Vec::new();
        let mut pc = SEARCH_START;
        while pc < data.len() {
            if let Some(len) = tag_at(data, pc) {
                pc += TAG_LEN + len;
                continue;
            }
            if data[pc] != 0 {
                pc += 1;
                continue;
            }
            let start = pc;
            while pc < data.len() && data[pc] == 0 {
                pc += 1;
            }
            runs.push(Run { start, end: pc });
        }
        Self { mapping, runs }
    }

    /// The free bytes left.
    pub fn free_bytes(&self) -> usize {
        self.runs.iter().map(|run| run.end - run.start).sum()
    }

    /// Places a block of `len` bytes, first-fit in file order, and writes
    /// its tag; the contents stay `$00` for the caller to write. Returns
    /// where they start. As in Asar, the contents stay in one bank, and
    /// when they would cross into the next, the tag goes in the last eight
    /// bytes of the bank and the contents start at the next one.
    pub fn alloc(
        &mut self,
        rom: &mut Rom,
        len: usize,
        contents: Contents,
    ) -> Result<SnesAddr, FreeSpaceError> {
        if !(1..=MAX_BLOCK_LEN).contains(&len) {
            return Err(FreeSpaceError::BadLength(len));
        }
        let first = |system: Option<bool>| {
            self.runs
                .iter()
                .enumerate()
                .find_map(|(index, run)| self.place(run, len, system).map(|tag| (index, tag)))
        };
        let (index, tag_pc) = match contents {
            Contents::Code => first(Some(true)),
            Contents::Data => first(Some(false)).or_else(|| first(None)),
        }
        .ok_or(FreeSpaceError::Full { len, contents })?;
        let at = self.snes(tag_pc);
        rom.write(at, &tag(len))?;
        let run = self.runs.remove(index);
        let after = Run {
            start: tag_pc + TAG_LEN + len,
            end: run.end,
        };
        let before = Run {
            start: run.start,
            end: tag_pc,
        };
        for part in [after, before] {
            if part.start < part.end {
                self.runs.insert(index, part);
            }
        }
        Ok(self.snes(tag_pc + TAG_LEN))
    }

    /// Where in a run a block's tag goes, if the block fits there with
    /// its contents in one bank, in a system bank or not if `system` says.
    fn place(&self, run: &Run, len: usize, system: Option<bool>) -> Option<usize> {
        let mut tag_pc = run.start;
        while tag_pc + TAG_LEN + len <= run.end {
            let contents = PcAddr::new((tag_pc + TAG_LEN) as u32);
            let bank_end = self
                .mapping
                .bank_end(contents)
                .expect("the image is mapped")
                .as_usize();
            let wanted =
                system.is_none_or(|s| self.snes(contents.as_usize()).in_system_bank() == s);
            if wanted && contents.as_usize() + len <= bank_end {
                return Some(tag_pc);
            }
            tag_pc = bank_end - TAG_LEN;
        }
        None
    }

    fn snes(&self, pc: usize) -> SnesAddr {
        self.mapping
            .pc_to_snes(PcAddr::new(pc as u32))
            .expect("the image is mapped")
    }
}

#[cfg(test)]
// One range of written bytes is a list of one, not the range's contents.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    const MIB: usize = 0x10_0000;
    const BANK: usize = 0x8000;

    /// An image of `len` bytes, all `$00` past the vanilla-sized head,
    /// which is `$FF`.
    fn image(map_mode: u8, len: usize) -> Rom {
        let mut data = vec![0xFF; len];
        data[SEARCH_START..].fill(0);
        data[0x7FD5] = map_mode;
        Rom::from_bytes(data).unwrap()
    }

    fn pc(rom: &Rom, addr: SnesAddr) -> usize {
        rom.pc(addr).unwrap().as_usize()
    }

    #[test]
    fn tags_are_checked_against_their_complement() {
        // Room for the block the tag claims, which a tag must have.
        let mut data = vec![0; 10 + 0x1234];
        data[2..10].copy_from_slice(&tag(0x1234));
        assert_eq!(&data[2..10], b"STAR\x33\x12\xCC\xED");
        assert_eq!(tag_at(&data, 2), Some(0x1234));
        assert_eq!(tag_at(&data, 3), None);
        assert_eq!(tag_at(&data, 9), None);
        data[9] ^= 1;
        assert_eq!(tag_at(&data, 2), None);
        assert_eq!(tag(MAX_BLOCK_LEN)[4..], [0xFF, 0xFF, 0x00, 0x00]);
    }

    #[test]
    fn a_tag_whose_block_runs_past_the_image_is_not_one() {
        let mut rom = image(0x20, 0x10_0000);
        let end = rom.len();
        // Claims $100 bytes where 16 remain: not a block, and nothing to
        // snapshot or to slice past the end for.
        let mut data = rom.data().to_vec();
        data[end - 0x10..end - 0x08].copy_from_slice(&[b'S', b'T', b'A', b'R', 0xFF, 0, 0, 0xFF]);
        // One that fits exactly is a block.
        data[end - 0x20..end - 0x18].copy_from_slice(&tag(0x10));
        rom = Rom::from_bytes(data).unwrap();
        assert_eq!(tag_at(rom.data(), end - 0x10), None);
        assert_eq!(tag_at(rom.data(), end - 0x20), Some(0x10));
        let blocks = blocks(&rom);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].len, 0x10);
        let snapshot = Snapshot::take(&rom);
        assert_eq!(snapshot.blocks().count(), 1);
        assert!(snapshot.check(&rom, &[]).is_empty());
    }

    #[test]
    fn walk_skips_whole_blocks() {
        let mut rom = image(0x20, MIB);
        let inner = SnesAddr::new(0x108010);
        rom.write(SnesAddr::new(0x108000), &tag(0x40)).unwrap();
        rom.write(inner, &tag(4)).unwrap();
        rom.write(SnesAddr::new(0x118000), &tag(2)).unwrap();
        assert_eq!(
            blocks(&rom),
            [
                RatsBlock {
                    start: SnesAddr::new(0x108008),
                    len: 0x40
                },
                RatsBlock {
                    start: SnesAddr::new(0x118008),
                    len: 2
                },
            ]
        );
    }

    #[test]
    fn free_space_is_untagged_zeros_within_a_bank() {
        let mut rom = image(0x20, MIB);
        // A tagged block of zeros is not free, and a stray byte splits a run.
        rom.write(SnesAddr::new(0x108000), &tag(0x100)).unwrap();
        rom.write_u8(SnesAddr::new(0x108200), 0x42).unwrap();
        let space = FreeSpace::scan(&rom);
        let runs: Vec<_> = space.runs.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(runs, [(0x80108, 0x80200), (0x80201, MIB)]);
        assert_eq!(space.free_bytes(), MIB / 2 - 0x109);
    }

    #[test]
    fn allocation_is_first_fit_and_tagged() {
        let mut rom = image(0x20, MIB);
        rom.write_u8(SnesAddr::new(0x108020), 0x42).unwrap();
        let mut space = FreeSpace::scan(&rom);
        let a = space.alloc(&mut rom, 0x10, Contents::Code).unwrap();
        let b = space.alloc(&mut rom, 0x20, Contents::Code).unwrap();
        let c = space.alloc(&mut rom, 0x08, Contents::Data).unwrap();
        assert_eq!(a, SnesAddr::new(0x108008));
        assert_eq!(b, SnesAddr::new(0x108029));
        assert_eq!(c, SnesAddr::new(0x108051));
        assert_eq!(tag_at(rom.data(), pc(&rom, a) - TAG_LEN), Some(0x10));
        let found: Vec<_> = blocks(&rom).iter().map(|b| b.start).collect();
        assert_eq!(found, [a, b, c]);
        // A rescan of the result carries on where the allocator is.
        let mut copy = Rom::from_bytes(rom.data().to_vec()).unwrap();
        let mut rescanned = FreeSpace::scan(&copy);
        assert_eq!(rescanned, space);
        assert_eq!(
            rescanned.alloc(&mut copy, 0x300, Contents::Code).unwrap(),
            space.alloc(&mut rom, 0x300, Contents::Code).unwrap()
        );
        assert_eq!(copy.data(), rom.data());
    }

    #[test]
    fn contents_never_cross_a_bank() {
        let mut rom = image(0x20, MIB);
        let mut space = FreeSpace::scan(&rom);
        let a = space.alloc(&mut rom, 0x10, Contents::Code).unwrap();
        // The rest of bank $10 is too short: the tag goes at its end.
        let b = space.alloc(&mut rom, BANK - 0x10, Contents::Code).unwrap();
        let c = space.alloc(&mut rom, 0x20, Contents::Code).unwrap();
        let d = space.alloc(&mut rom, BANK, Contents::Code).unwrap();
        assert_eq!(a, SnesAddr::new(0x108008));
        assert_eq!(b, SnesAddr::new(0x118000));
        assert_eq!(tag_at(rom.data(), 0x87FF8), Some(BANK - 0x10));
        // What the move left behind is still free.
        assert_eq!(c, SnesAddr::new(0x108020));
        assert_eq!(d, SnesAddr::new(0x128000));
        assert!(matches!(
            space.alloc(&mut rom, BANK + 1, Contents::Data),
            Err(FreeSpaceError::Full { .. })
        ));
        assert!(matches!(
            space.alloc(&mut rom, 0, Contents::Data),
            Err(FreeSpaceError::BadLength(0))
        ));
    }

    #[test]
    fn bank_boundary_search_preserves_tagged_zeros() {
        // Asar 1.91 overwrites the second block's last eight bytes with
        // its third tag for this sequence. See docs/toolchain.md.
        for rescan in [false, true] {
            let mut rom = image(0x20, MIB);
            let mut space = FreeSpace::scan(&rom);
            let a = space.alloc(&mut rom, BANK, Contents::Code).unwrap();
            let b = space
                .alloc(&mut rom, BANK - TAG_LEN, Contents::Code)
                .unwrap();
            assert_eq!(a, SnesAddr::new(0x118000));
            assert_eq!(b, SnesAddr::new(0x128008));
            let protected_end = pc(&rom, b) + BANK - TAG_LEN;
            let before = rom.data()[..protected_end].to_vec();

            if rescan {
                rom = Rom::from_bytes(rom.data().to_vec()).unwrap();
                space = FreeSpace::scan(&rom);
            }
            let c = space.alloc(&mut rom, BANK, Contents::Code).unwrap();
            assert_eq!(c, SnesAddr::new(0x148000));
            assert_eq!(&rom.data()[..protected_end], before);
            assert_eq!(blocks(&rom).len(), 3);
            assert_eq!(tag_at(rom.data(), pc(&rom, c) - TAG_LEN), Some(BANK));
        }
    }

    #[test]
    fn lorom_data_goes_past_the_system_banks_first() {
        let mut rom = image(0x20, 4 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(data, SnesAddr::new(0x408000));
        assert_eq!(tag_at(rom.data(), 0x1FFFF8), Some(0x100));
        assert_eq!(code, SnesAddr::new(0x108008));
        // Code never goes there, and data falls back to the system banks.
        let mut rom = image(0x20, MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        assert_eq!(data, SnesAddr::new(0x108008));
    }

    #[test]
    fn sa1_images() {
        // Up to 4 MiB, every bank is a system bank, to the last.
        let mut rom = image(0x23, 4 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0x100, Contents::Data).unwrap();
        assert_eq!(data, SnesAddr::new(0x108008));
        let mut rom = image(0x23, 4 * MIB);
        let last = pc(&rom, SnesAddr::new(0xBF8000));
        rom.write(SnesAddr::new(0x108000), &vec![1; last - SEARCH_START])
            .unwrap();
        let mut space = FreeSpace::scan(&rom);
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(code, SnesAddr::new(0xBF8008));

        // Past 4 MiB, data goes in the HiROM view's 64 KiB banks first.
        let mut rom = image(0x23, 8 * MIB);
        let mut space = FreeSpace::scan(&rom);
        let data = space.alloc(&mut rom, 0xC000, Contents::Data).unwrap();
        let more = space.alloc(&mut rom, 0x8000, Contents::Data).unwrap();
        let code = space.alloc(&mut rom, 0x100, Contents::Code).unwrap();
        assert_eq!(data, SnesAddr::new(0xC00000));
        assert_eq!(more, SnesAddr::new(0xC10000));
        assert_eq!(code, SnesAddr::new(0x108008));
        assert_eq!(pc(&rom, more), 0x410000);
        assert!(matches!(
            space.alloc(&mut rom, MAX_BLOCK_LEN + 1, Contents::Data),
            Err(FreeSpaceError::BadLength(_))
        ));
    }

    /// Two blocks of `$11` at `$108008` and `$118008`, `$100` bytes each.
    fn two_blocks() -> (Rom, Snapshot) {
        let mut rom = image(0x20, MIB);
        for tag_at in [0x108000, 0x118000] {
            rom.write(SnesAddr::new(tag_at), &tag(0x100)).unwrap();
            rom.write(SnesAddr::new(tag_at + 8), &[0x11; 0x100])
                .unwrap();
        }
        let snapshot = Snapshot::take(&rom);
        (rom, snapshot)
    }

    #[test]
    fn a_snapshot_holds_every_block() {
        let (rom, snapshot) = two_blocks();
        assert_eq!(snapshot.blocks().collect::<Vec<_>>(), blocks(&rom));
        assert_eq!(snapshot.blocks[1].tag, 0x88000);
        assert_eq!(snapshot.blocks[1].bytes.len(), TAG_LEN + 0x100);
        assert_eq!(snapshot.check(&rom, &[]), []);
    }

    #[test]
    fn erasing_a_block_whole_is_not_damage() {
        let (mut rom, snapshot) = two_blocks();
        rom.write(SnesAddr::new(0x118000), &[FREE_BYTE; TAG_LEN + 0x100])
            .unwrap();
        // Writes outside the blocks are not the check's business.
        rom.write(SnesAddr::new(0x128000), &[0x22; 0x10]).unwrap();
        assert_eq!(snapshot.check(&rom, &[]), []);
    }

    #[test]
    fn released_space_may_hold_new_blocks() {
        // As Asar's autoclean then freecode leave it: erased, and a new,
        // shorter block at the same place.
        let (mut rom, snapshot) = two_blocks();
        let at = SnesAddr::new(0x108000);
        rom.write(at, &[FREE_BYTE; TAG_LEN + 0x100]).unwrap();
        rom.write(at, &tag(0x80)).unwrap();
        rom.write(at.add(8), &[0x22; 0x80]).unwrap();
        assert_eq!(snapshot.check(&rom, &[]), []);

        // A new block of the same length is only told apart from a write
        // under the old tag by the tool's report of rewriting the tag.
        let (mut rom, snapshot) = two_blocks();
        rom.write(at.add(8), &[0x22; 0x100]).unwrap();
        assert_eq!(snapshot.check(&rom, &[0x80000..0x80108]), []);
        let damage = snapshot.check(&rom, &[0x80008..0x80108]);
        assert_eq!(damage.len(), 1);
        assert!(damage[0].tagged);
    }

    #[test]
    fn a_write_under_a_standing_tag_is_damage() {
        let (mut rom, snapshot) = two_blocks();
        rom.write(SnesAddr::new(0x118010), &[0; 4]).unwrap();
        rom.write(SnesAddr::new(0x118100), &[0x33]).unwrap();
        let damage = snapshot.check(&rom, &[]);
        assert_eq!(
            damage,
            [Damage {
                block: RatsBlock {
                    start: SnesAddr::new(0x118008),
                    len: 0x100
                },
                changed: (SnesAddr::new(0x118010), SnesAddr::new(0x118100)),
                tagged: true,
            }]
        );
        assert_eq!(
            damage[0].to_string(),
            "RATS block at $118008 ($100 bytes) changed at $118010-$118100 under its tag"
        );
    }

    #[test]
    fn a_lost_tag_with_contents_left_is_damage() {
        let (mut rom, snapshot) = two_blocks();
        rom.write(SnesAddr::new(0x108000), &[FREE_BYTE; TAG_LEN])
            .unwrap();
        let damage = snapshot.check(&rom, &[]);
        assert_eq!(damage.len(), 1);
        assert!(!damage[0].tagged);
        assert_eq!(
            damage[0].changed,
            (SnesAddr::new(0x108000), SnesAddr::new(0x108007))
        );
    }

    #[test]
    fn asar_bank_boundary_overwrite_is_damage() {
        // The layout of docs/toolchain.md's reproduction, and the bytes
        // Asar 1.91 writes into it: its third tag goes in the second
        // block's last eight bytes.
        let mut rom = image(0x20, MIB);
        let mut space = FreeSpace::scan(&rom);
        space.alloc(&mut rom, BANK, Contents::Code).unwrap();
        space
            .alloc(&mut rom, BANK - TAG_LEN, Contents::Code)
            .unwrap();
        let snapshot = Snapshot::take(&rom);
        rom.write(SnesAddr::new(0x12FFF8), &tag(BANK)).unwrap();
        rom.write(SnesAddr::new(0x138000), &[0; BANK]).unwrap();
        let damage = snapshot.check(&rom, &[0x97FF8..0xA0000]);
        assert_eq!(
            damage,
            [Damage {
                block: RatsBlock {
                    start: SnesAddr::new(0x128008),
                    len: BANK - TAG_LEN
                },
                changed: (SnesAddr::new(0x12FFF8), SnesAddr::new(0x12FFFF)),
                tagged: true,
            }]
        );
    }
}
