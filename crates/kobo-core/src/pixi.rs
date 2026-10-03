//! What PIXI leaves in a ROM, found from PIXI's own tables, so that an
//! import can carry a hack's custom sprites as the compiled code the ROM
//! holds when their sources are not to be had.
//!
//! PIXI marks a ROM it has run on with `STSD` and its version at `$02FFE2`
//! and keeps pointers to its tables after them; it finds what an earlier
//! run inserted the same way, to clean it up (`clean_hack` in PIXI's
//! `src/sprite.cpp`). An insert is two kinds of thing:
//!
//! - RATS blocks: PIXI's own code and tables (the block `$02FFEE` points
//!   into), the size table, the per-level tables, every sprite's code, the
//!   shared routines, and the blocks those name in their `PROT` lists.
//!   Code is not relocatable, so a build writes each back where it was.
//! - Sites: the bytes PIXI writes at fixed addresses, each from its
//!   patches (`asm/main.asm` and the sprite types' patches, versions 1.02
//!   to 1.43). [`SITES`] lists them; an import carries a site only where
//!   the ROM's bytes are what PIXI writes there.
//!
//! Lunar Magic writes some of the same sites ([`Window::lunar_magic`];
//! docs/lunar-magic-install.md, "Sprites"). Its code there is not PIXI's
//! and must not be copied (clean room), so a jump at such a site is
//! carried only when it lands in PIXI's own block, and other bytes only
//! when they are the ones PIXI writes. Nothing here says where a jump that
//! fails the test goes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::addr::SnesAddr;
use crate::rats::{self, RatsBlock};
use crate::rom::{Rom, RomError};
use crate::sprites;

/// PIXI's mark: `STSD`, then its version byte and flags.
pub const HEADER: SnesAddr = SnesAddr::new(0x02FFE2);
const MARK: &[u8; 4] = b"STSD";
const VERSION: SnesAddr = SnesAddr::new(0x02FFE6);
const FLAGS: SnesAddr = SnesAddr::new(0x02FFE7);
/// Before 1.30, the banks of the four per-level tables, `$FF` for none.
const PER_LEVEL_BANKS: SnesAddr = SnesAddr::new(0x02FFEA);
/// The pointer to PIXI's table of the global sprites, in its main block.
const TABLE: SnesAddr = SnesAddr::new(0x02FFEE);
/// From 1.30, the pointer to the per-level sprites' level table.
const PER_LEVEL_TABLE: SnesAddr = SnesAddr::new(0x02FFF1);
/// From 1.30, the pointer to the global sprites' custom status pointers.
const STATUS_POINTERS: SnesAddr = SnesAddr::new(0x02FFFD);
/// The shared routines' pointers, `$FFFFFF` for none.
const ROUTINES: SnesAddr = SnesAddr::new(0x03E05C);
/// As many as PIXI 1.43 allows (`MAX_ROUTINES`); earlier versions took 100.
const ROUTINE_COUNT: usize = 310;

/// A sprite's entry in PIXI's tables: 16 bytes, with its init and main
/// pointers at 8 and 11.
const ENTRY: usize = 0x10;
const ENTRY_INIT: usize = 0x08;
const ENTRY_MAIN: usize = 0x0B;

/// What PIXI writes at a site, as an import tests the ROM's bytes there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Data of PIXI's, whatever its bytes; trailing bytes the base has
    /// are left out.
    Data,
    /// One of these byte strings.
    Exact(&'static [&'static [u8]]),
    /// A `JML` or `JSL` into a block, then `NOP`s.
    Jump,
    /// A `JML` into a block and a pointer to a table of this many pointers
    /// to sprite code: the hook of a sprite type's patch (cluster,
    /// extended, ...; PIXI's cleanup reads that many, `clean_sprite_generic`).
    JumpTable(usize),
    /// `JMP`s or `JSR`s into PIXI's code in bank `$02`'s unused space at
    /// [`BANK_02_CODE`].
    Local,
    /// The goal tape's init taking the extra bits: `LDA !extra_bits,X` first.
    GoalTape,
}

/// Which mappings PIXI writes a site on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum On {
    Both,
    LoRom,
    Sa1,
}

/// A site: where PIXI writes, how much at most, and what.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Window {
    pub at: SnesAddr,
    pub len: usize,
    pub shape: Shape,
    pub on: On,
    /// Whether Lunar Magic writes here too, so that bytes not PIXI's may be
    /// its code.
    pub lunar_magic: bool,
}

const fn site(at: u32, len: usize, shape: Shape) -> Window {
    Window {
        at: SnesAddr::new(at),
        len,
        shape,
        on: On::Both,
        lunar_magic: false,
    }
}

const fn lorom(window: Window) -> Window {
    Window {
        on: On::LoRom,
        ..window
    }
}

const fn sa1(window: Window) -> Window {
    Window {
        on: On::Sa1,
        ..window
    }
}

const fn shared(window: Window) -> Window {
    Window {
        lunar_magic: true,
        ..window
    }
}

/// Bank `$02`'s unused space, where PIXI puts some of its loader's code.
const BANK_02_CODE: std::ops::Range<u16> = 0xB5EC..0xB60E;

/// Every site PIXI writes, versions 1.02 to 1.43, from its patches.
pub const SITES: &[Window] = &[
    // `main.asm`: the mark and the table pointers.
    site(0x02FFE2, 30, Shape::Data),
    // The level number in `$010B` (UberASM Tool writes the same).
    site(0x05D8B9, 3, Shape::Exact(&[&[0x20, 0x46, 0xDC]])),
    lorom(site(
        0x05DC46,
        7,
        Shape::Exact(&[&[0xA5, 0x0E, 0x8D, 0x0B, 0x01, 0x0A, 0x60]]),
    )),
    sa1(site(
        0x05DC46,
        7,
        Shape::Exact(&[&[0xA5, 0x0E, 0x8D, 0x0B, 0x61, 0x0A, 0x60]]),
    )),
    shared(site(0x01C089, 11, Shape::GoalTape)),
    shared(site(0x02A8D8, 3, Shape::Exact(&[&[0x4C, 0x78, 0xAB]]))),
    site(0x0182B3, 2, Shape::Exact(&[&[0xC2, 0x85]])),
    site(0x01D43E, 4, Shape::Exact(&[&[0x20, 0x33, 0x81, 0x6B]])),
    site(0x02A963, 5, Shape::Jump),
    site(0x02A94B, 5, Shape::Jump),
    site(0x018172, 5, Shape::Jump),
    site(0x0185C3, 5, Shape::Jump),
    site(0x07F785, 5, Shape::Jump),
    site(0x018151, 5, Shape::Jump),
    site(0x02A866, 4, Shape::Jump),
    lorom(site(0x02ABA0, 5, Shape::Jump)),
    sa1(site(0x02ABA2, 6, Shape::Jump)),
    site(0x02B395, 5, Shape::Jump),
    site(0x02AFFE, 5, Shape::Jump),
    site(0x0187A7, 4, Shape::Jump),
    site(0x018127, 4, Shape::Jump),
    site(0x02A9C9, 4, Shape::Jump),
    site(0x02A9A6, 5, Shape::Jump),
    lorom(site(0x019AFE, 4, Shape::Jump)),
    sa1(site(0x019B00, 4, Shape::Jump)),
    site(0x038996, 4, Shape::Jump),
    site(0x039C58, 4, Shape::Jump),
    // The size table's pointer and Lunar Magic's `$42` that enables it.
    site(0x0EF30C, 4, Shape::Data),
    shared(site(0x02A846, 5, Shape::Jump)),
    // The loader's 16-bit sprite data pointer.
    lorom(site(0x028B1D, 3, Shape::Local)),
    lorom(site(0x02AC7A, 6, Shape::Local)),
    lorom(site(0x02ACBA, 6, Shape::Local)),
    lorom(site(0x02B5EC, 0x22, Shape::Data)),
    lorom(site(0x02ABEF, 3, Shape::Local)),
    lorom(site(0x02A9DB, 3, Shape::Local)),
    // With the 255-sprite option, a `JML` over the load flags' clearing.
    // Without `!EXLEVEL`, PIXI writes `$7F` at `$02ABF3` to clear all 128;
    // Kobo's loader, in every build that carries an insert, does the same
    // (docs/lunar-magic-install.md, "Sprites"), so that is not carried.
    lorom(shared(site(0x02ABF2, 4, Shape::Jump))),
    // The 255-sprite option.
    lorom(site(0x02A856, 10, Shape::Jump)),
    lorom(site(0x02A936, 5, Shape::Jump)),
    lorom(site(0x02A8BB, 5, Shape::Jump)),
    lorom(site(0x02FAE9, 4, Shape::Jump)),
    lorom(site(0x01AC9C, 5, Shape::Jump)),
    lorom(site(0x02AB99, 5, Shape::Jump)),
    lorom(site(0x02D088, 5, Shape::Jump)),
    lorom(site(0x02FF15, 5, Shape::Jump)),
    lorom(site(0x038712, 5, Shape::Jump)),
    lorom(site(0x03B8BA, 5, Shape::Jump)),
    sa1(shared(site(
        0x02A9D7,
        3,
        Shape::Exact(&[&[0x4C, 0xEC, 0xB5]]),
    ))),
    sa1(site(0x02B5EC, 0x0F, Shape::Data)),
    // The shared routines' pointers.
    site(ROUTINES.raw(), 3 * ROUTINE_COUNT, Shape::Data),
    // The sprite types' patches.
    site(0x00A686, 7, Shape::JumpTable(0x80)),
    site(0x02F815, 4, Shape::Jump),
    site(0x029B1B, 7, Shape::JumpTable(0x80)),
    site(0x029633, 7, Shape::JumpTable(0x80)),
    site(0x028B6C, 7, Shape::JumpTable(0x3F)),
    site(0x029054, 7, Shape::JumpTable(0x3F)),
    site(0x0296C0, 7, Shape::JumpTable(0x3F)),
    site(0x0299D4, 7, Shape::JumpTable(0x1F)),
    site(0x028A7D, 4, Shape::Jump),
    site(0x02ADBA, 7, Shape::JumpTable(0x1F)),
];

/// The first instruction of PIXI's goal tape init: `LDA !extra_bits,X`,
/// at `$7FAB10` on LoROM, `$6040` (1.30 on) or `$400040` on SA-1.
const GOAL_TAPE: &[&[u8]] = &[
    &[0xBF, 0x10, 0xAB, 0x7F],
    &[0xBD, 0x40, 0x60],
    &[0xBF, 0x40, 0x00, 0x40],
];

const JML: u8 = 0x5C;
const JSL: u8 = 0x22;
const JMP: u8 = 0x4C;
const JSR: u8 = 0x20;
const NOP: u8 = 0xEA;

/// PIXI's insert in a ROM, as an import carries it and a build writes it.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Insert {
    /// PIXI's version byte (`$02FFE6`; [`version_name`]).
    pub version: u8,
    /// Whether the ROM loads 255 sprites a level: bit 0 of
    /// [`sprites::SPRITE_LIMIT_FLAGS`] clear, as PIXI's option leaves it.
    pub sprites_255: bool,
    /// Bytes at fixed addresses, in address order.
    pub sites: BTreeMap<SnesAddr, Vec<u8>>,
    /// RATS blocks' contents by where they start (the byte after the tag),
    /// in address order.
    pub blocks: BTreeMap<SnesAddr, Vec<u8>>,
    /// PIXI's size table alone, for a build to place where it has room:
    /// when the insert's code cannot go where the hack has it
    /// ([`conflicts`]), so that its levels still build with their sprites'
    /// extension bytes. The insert then has no blocks or sites.
    pub size_table: Option<Vec<u8>>,
}

impl Insert {
    /// The bytes of the blocks.
    pub fn block_bytes(&self) -> usize {
        self.blocks.values().map(Vec::len).sum()
    }
}

/// PIXI's version as it names it, from the byte it writes at `$02FFE6`:
/// `$02` for 1.02, `$19`-`$23` for 1.2.5 to 1.2.15, `$30`-`$32` for 1.30 to
/// 1.32, and 140 to 143 for 1.40 to 1.43.
pub fn version_name(byte: u8) -> String {
    match byte {
        0x00..=0x09 => format!("1.0{byte}"),
        0x14..=0x2F => format!("1.2.{}", byte - 0x14),
        0x30..=0x3F => format!("1.{byte:X}"),
        100..=199 => format!("1.{}", byte - 100),
        _ => format!("with version byte ${byte:02X}"),
    }
}

/// Whether PIXI has marked the ROM.
pub fn installed(rom: &Rom) -> bool {
    rom.read(HEADER, MARK.len()).is_ok_and(|b| b == MARK)
}

/// The tagged blocks past the vanilla image, to find the one an address
/// is in.
struct Blocks {
    /// File offset of the contents, and the block.
    list: Vec<(usize, RatsBlock)>,
}

impl Blocks {
    /// The ROM's blocks but those that start where the base has one: SA-1
    /// Pack's, on an SA-1 ROM, whose hooks a hack's SA-1 Pack puts at some
    /// of PIXI's sites, whatever its version. A block of PIXI's where the
    /// base has something else stays, for the build to refuse.
    fn of(rom: &Rom, base: &Rom) -> Self {
        let based: BTreeSet<SnesAddr> = rats::blocks(base).into_iter().map(|b| b.start).collect();
        let list = rats::blocks(rom)
            .into_iter()
            .filter(|b| !based.contains(&b.start))
            .filter_map(|b| Some((rom.pc(b.start).ok()?.as_usize(), b)))
            .collect();
        Self { list }
    }

    /// The block whose contents hold `addr`.
    fn holding(&self, rom: &Rom, addr: SnesAddr) -> Option<RatsBlock> {
        let pc = rom.pc(addr).ok()?.as_usize();
        let i = self.list.partition_point(|(start, _)| *start <= pc);
        let (start, block) = *self.list.get(i.checked_sub(1)?)?;
        (pc < start + block.len).then_some(block)
    }
}

/// Reads PIXI's insert from `rom`, with notes on what it leaves out.
/// `base` is the image the ROM's project builds on (the clean ROM, or SA-1
/// Pack's), against which a site counts as written. `None` if PIXI has not
/// marked the ROM or its main block is not there.
pub fn read(rom: &Rom, base: &Rom) -> Result<Option<(Insert, Vec<String>)>, RomError> {
    if !installed(rom) {
        return Ok(None);
    }
    let blocks = Blocks::of(rom, base);
    let ptr = |addr: SnesAddr| rom.read_u24(addr).ok().map(SnesAddr::new);
    let Some(main) = ptr(TABLE).and_then(|t| blocks.holding(rom, t)) else {
        return Ok(None);
    };
    let version = rom.read_u8(VERSION)?;
    let flags = rom.read_u8(FLAGS)?;
    let mut notes = Vec::new();
    let mut found: BTreeSet<SnesAddr> = BTreeSet::new();
    let mut queue: VecDeque<RatsBlock> = VecDeque::new();
    let mut take = |addr: Option<SnesAddr>, queue: &mut VecDeque<RatsBlock>| {
        if let Some(block) = addr.and_then(|a| blocks.holding(rom, a))
            && found.insert(block.start)
        {
            queue.push_back(block);
        }
    };
    take(Some(main.start), &mut queue);

    let sa1 = rom.mapping().is_sa1();
    let mut sites = BTreeMap::new();
    let mut foreign = Vec::new();
    for window in SITES {
        match window.on {
            On::LoRom if sa1 => continue,
            On::Sa1 if !sa1 => continue,
            _ => {}
        }
        let (Ok(now), Ok(was)) = (
            rom.read(window.at, window.len),
            base.read(window.at, window.len),
        ) else {
            continue;
        };
        // A site PIXI has written over with another, in the order the
        // patches have them.
        let covered = |(at, bytes): (&SnesAddr, &Vec<u8>)| {
            (at.raw()..at.raw() + bytes.len() as u32).contains(&window.at.raw())
        };
        if now == was || sites.iter().any(covered) {
            continue;
        }
        let in_main = |target: SnesAddr| blocks.holding(rom, target) == Some(main);
        let in_block = |target: SnesAddr| {
            if window.lunar_magic {
                in_main(target)
            } else {
                blocks.holding(rom, target).is_some()
            }
        };
        let long =
            |at: usize| SnesAddr::new(u32::from_le_bytes([now[at], now[at + 1], now[at + 2], 0]));
        let kept: Option<&[u8]> = match window.shape {
            Shape::Data => {
                let last = (0..window.len).rev().find(|&i| now[i] != was[i]);
                last.map(|last| &now[..=last])
            }
            Shape::Exact(variants) => variants
                .iter()
                .find(|v| now.starts_with(v))
                .map(|v| &now[..v.len()]),
            Shape::GoalTape => GOAL_TAPE.iter().any(|p| now.starts_with(p)).then_some(now),
            Shape::Jump => {
                let pads = now[4..].iter().take_while(|&&b| b == NOP).count();
                ([JML, JSL].contains(&now[0]) && in_block(long(1))).then(|| {
                    take(Some(long(1)), &mut queue);
                    &now[..4 + pads]
                })
            }
            Shape::JumpTable(count) => {
                (now[0] == JML && in_block(long(1)) && in_block(long(4))).then(|| {
                    take(Some(long(1)), &mut queue);
                    take(Some(long(4)), &mut queue);
                    // The table's pointers to each sprite's code.
                    if let Ok(bytes) = rom.read(long(4), 3 * count) {
                        for p in bytes.as_chunks::<3>().0 {
                            take(
                                Some(SnesAddr::new(u32::from_le_bytes([p[0], p[1], p[2], 0]))),
                                &mut queue,
                            );
                        }
                    }
                    now
                })
            }
            Shape::Local => now
                .as_chunks::<3>()
                .0
                .iter()
                .all(|i| {
                    [JMP, JSR].contains(&i[0])
                        && BANK_02_CODE.contains(&u16::from_le_bytes([i[1], i[2]]))
                })
                .then_some(now),
        };
        match kept {
            Some(bytes) => {
                sites.insert(window.at, bytes.to_vec());
            }
            None => foreign.push(window.at.to_string()),
        }
    }
    if !foreign.is_empty() {
        notes.push(format!(
            "{} {} hold{} bytes PIXI does not write there (another tool's, or a hook this \
             version of PIXI does not have), which are not carried",
            if foreign.len() == 1 {
                "PIXI's site at"
            } else {
                "PIXI's sites at"
            },
            foreign.join(", "),
            if foreign.len() == 1 { "s" } else { "" }
        ));
    }
    // The size table, and the shared routines.
    if rom.read_u8(sprites::PIXI_SIZE_TABLE_MARKER).ok() == Some(sprites::PIXI_MARKER_VALUE) {
        take(ptr(sprites::PIXI_SIZE_TABLE_PTR), &mut queue);
    }
    for i in 0..ROUTINE_COUNT {
        let routine = ptr(ROUTINES.add(3 * i as u32));
        if routine.is_some_and(|r| r.raw() != 0xFF_FFFF) {
            take(routine, &mut queue);
        }
    }
    // The global sprites' table, and its sprites' code.
    let entries =
        |table: SnesAddr,
         len: usize,
         queue: &mut VecDeque<RatsBlock>,
         take: &mut dyn FnMut(Option<SnesAddr>, &mut VecDeque<RatsBlock>)| {
            for i in 0..len / ENTRY {
                let entry = table.add((i * ENTRY) as u32);
                take(ptr(entry.add(ENTRY_INIT as u32)), queue);
                take(ptr(entry.add(ENTRY_MAIN as u32)), queue);
            }
        };
    // Per-level sprites (`B0`-`BF`) leave the global table `B0`-`BF`'s
    // entries short of it before 1.30.
    let per_level = flags & 1 != 0 || version < 2;
    if let Some(table) = ptr(TABLE) {
        let len = if per_level && version < 0x30 {
            0xF00
        } else {
            0x1000
        };
        entries(table, len, &mut queue, &mut take);
    }
    if per_level && version < 0x30 {
        // A whole bank a table, of which PIXI's cleanup reads each entry's
        // main pointer up to the first that is `$FFFFFF`.
        for bank in rom.read(PER_LEVEL_BANKS, 4)?.to_vec() {
            if bank == 0xFF {
                continue;
            }
            let table = SnesAddr::from_bank_offset(bank, 0x8000);
            take(Some(table), &mut queue);
            for offset in (ENTRY_MAIN..0x8000).step_by(ENTRY) {
                let main = ptr(table.add(offset as u32));
                if main.is_none_or(|m| m.raw() == 0xFF_FFFF) {
                    break;
                }
                take(main, &mut queue);
            }
        }
    } else if let Some(levels) =
        ptr(PER_LEVEL_TABLE).filter(|p| per_level && ![0, 0xFF_FFFF].contains(&p.raw()))
    {
        take(Some(levels), &mut queue);
        // The table's `PROT` list names the sprites' table and their custom
        // status pointers, in that order, last.
        // Each is a block of its own, which PIXI's cleanup reads whole.
        if let Some(table) = ptr(SnesAddr::new(levels.raw().wrapping_sub(16)))
            && let Some(block) = blocks.holding(rom, table).filter(|b| b.start == table)
        {
            take(Some(table), &mut queue);
            entries(block.start, block.len, &mut queue, &mut take);
        }
        if let Some(status) = ptr(SnesAddr::new(levels.raw().wrapping_sub(8)))
            && let Some(block) = blocks.holding(rom, status).filter(|b| b.start == status)
        {
            take(Some(status), &mut queue);
            // Five pointers and a byte a sprite.
            for i in 0..block.len / 16 {
                for p in 0..5 {
                    take(ptr(block.start.add((i * 16 + p * 3) as u32)), &mut queue);
                }
            }
        }
    }
    // The global sprites' custom status pointers (1.30 on): 15 bytes a
    // sprite, in PIXI's main block.
    if version >= 0x30
        && let Some(status) = ptr(STATUS_POINTERS).filter(|p| p.raw() != 0xFF_FFFF)
    {
        take(Some(status), &mut queue);
        for i in 0..0x100 * 15 / 3 {
            take(ptr(status.add(3 * i as u32)), &mut queue);
        }
    }
    // Every block, and those its `PROT` list names.
    let mut out = BTreeMap::new();
    while let Some(block) = queue.pop_front() {
        let bytes = rom.read(block.start, block.len)?;
        for p in protected(bytes) {
            take(Some(p), &mut queue);
        }
        out.insert(block.start, bytes.to_vec());
    }
    let sprites_255 = rom.read_u8(sprites::SPRITE_LIMIT_FLAGS)? & 1 == 0
        && base.read_u8(sprites::SPRITE_LIMIT_FLAGS)? & 1 != 0;
    Ok(Some((
        Insert {
            version,
            sprites_255,
            sites,
            blocks: out,
            size_table: None,
        },
        notes,
    )))
}

/// The addresses a block's `PROT` list names: Asar writes `PROT`, a byte
/// count, and three bytes an address at the start of a block whose code
/// uses other blocks, then `STOP` and a zero (Asar's `WalkMetadata`).
fn protected(bytes: &[u8]) -> Vec<SnesAddr> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(name) = bytes.get(at..at + 4) {
        if !name.iter().all(u8::is_ascii_uppercase) || name == b"STOP" {
            break;
        }
        let Some(&len) = bytes.get(at + 4) else { break };
        let Some(contents) = bytes.get(at + 5..at + 5 + len as usize) else {
            break;
        };
        if name == b"PROT" {
            out.extend(
                contents
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| SnesAddr::new(u32::from_le_bytes([p[0], p[1], p[2], 0]))),
            );
        }
        at += 5 + len as usize;
    }
    out
}

/// The insert's blocks that cannot go where the hack has them in a build
/// on `base`: where the base has a block or anything but free space. On an
/// SA-1 hack made with another SA-1 Pack than the base's, PIXI's blocks are
/// where SA-1 Pack 1.40 has its own.
pub fn conflicts(insert: &Insert, base: &Rom) -> Vec<SnesAddr> {
    let tagged: Vec<(usize, usize)> = rats::blocks(base)
        .into_iter()
        .filter_map(|b| {
            let start = base.pc(b.start).ok()?.as_usize();
            Some((start - rats::TAG_LEN, start + b.len))
        })
        .collect();
    insert
        .blocks
        .iter()
        .filter(|&(&at, bytes)| {
            let Ok(start) = base.pc(at) else { return true };
            let (from, to) = (
                start.as_usize() - rats::TAG_LEN,
                start.as_usize() + bytes.len(),
            );
            let used = base
                .data()
                .get(from..to.min(base.len()))
                .is_some_and(|b| b.iter().any(|&x| x != 0));
            used || tagged.iter().any(|&(f, t)| f < to && from < t)
        })
        .map(|(&at, _)| at)
        .collect()
}

/// The size table of `rom`'s PIXI, as [`Insert::size_table`] carries it.
pub fn size_table(rom: &Rom) -> Option<Vec<u8>> {
    sprites::pixi_size_table(rom)
        .ok()
        .flatten()
        .map(<[u8]>::to_vec)
}

/// Places a size table in free space, and points Lunar Magic's size table
/// pointer at it with the `$42` that enables it.
pub fn write_size_table(rom: &mut Rom, table: &[u8]) -> Result<(), WriteError> {
    let mut space = rats::FreeSpace::scan(rom);
    let at = space
        .alloc(rom, table.len(), rats::Contents::Data)
        .map_err(|e| WriteError::Space(e.to_string()))?;
    rom.write(at, table)?;
    rom.write(sprites::PIXI_SIZE_TABLE_PTR, &at.raw().to_le_bytes()[..3])?;
    rom.write_u8(sprites::PIXI_SIZE_TABLE_MARKER, sprites::PIXI_MARKER_VALUE)?;
    Ok(())
}

/// Why a build cannot write an insert.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(
        "PIXI's block at {at} (${len:X} bytes) would overwrite something the build put there first"
    )]
    Taken { at: SnesAddr, len: usize },
    #[error("PIXI's block at {at} (${len:X} bytes) is not a block a RATS tag can hold")]
    BadBlock { at: SnesAddr, len: usize },
    #[error("PIXI's size table: {0}")]
    Space(String),
    #[error(transparent)]
    Rom(#[from] RomError),
}

/// Writes the insert's blocks, each with its tag, where PIXI put them. The
/// space must be free: zero, and under no tag.
pub fn write_blocks(rom: &mut Rom, insert: &Insert) -> Result<(), WriteError> {
    let free = rats::FreeSpace::scan(rom);
    for (&at, bytes) in &insert.blocks {
        let len = bytes.len();
        if !(1..=rats::MAX_BLOCK_LEN).contains(&len) {
            return Err(WriteError::BadBlock { at, len });
        }
        let tag_at = rom
            .pc(at)
            .map_err(RomError::from)?
            .as_usize()
            .checked_sub(rats::TAG_LEN)
            .ok_or(WriteError::BadBlock { at, len })?;
        if !free.contains(tag_at..tag_at + rats::TAG_LEN + len) {
            return Err(WriteError::Taken { at, len });
        }
        let [lo, hi] = ((len - 1) as u16).to_le_bytes();
        let tag = [b'S', b'T', b'A', b'R', lo, hi, !lo, !hi];
        let tag_addr = rom
            .mapping()
            .pc_to_snes(crate::addr::PcAddr::new(tag_at as u32))
            .map_err(RomError::from)?;
        rom.write(tag_addr, &tag)?;
        rom.write(at, bytes)?;
    }
    Ok(())
}

/// What a free byte is replaced with in an insert's blocks while another
/// program runs on the image ([`mask_blocks`]).
const MASK: u8 = 0xFF;

/// Replaces the free bytes (`$00`) inside the insert's blocks with
/// [`MASK`], so that Asar and the tools built on it take no space there.
/// Asar 1.91 searches inside a tagged block that starts at a bank's start
/// (docs/toolchain.md, "Asar 1.91 RATS boundary limitation"); Kobo's own
/// blocks are placed where it does not, but a carried block is where PIXI
/// put it. [`unmask_blocks`] puts the bytes back.
///
/// It also masks the free bytes between a block's end and its bank's end
/// when there are fewer than a tag's eight: Asar 1.91's search on an SA-1
/// image takes a tag there and puts the contents across the bank's end
/// (`trypcfreespace` checks only that the tag starts in the bank's last
/// eight bytes).
pub fn mask_blocks(rom: &mut Rom, insert: &Insert) -> Result<(), RomError> {
    for (&at, bytes) in &insert.blocks {
        let masked: Vec<u8> = bytes
            .iter()
            .map(|&b| if b == 0 { MASK } else { b })
            .collect();
        rom.write(at, &masked)?;
        for pc in short_gap(rom, at, bytes.len()) {
            if rom.data()[pc] == 0 {
                rom.write(pc_addr(rom, pc)?, &[MASK])?;
            }
        }
    }
    Ok(())
}

/// Puts back the free bytes [`mask_blocks`] replaced, where they still
/// hold the mask.
pub fn unmask_blocks(rom: &mut Rom, insert: &Insert) -> Result<(), RomError> {
    for (&at, bytes) in &insert.blocks {
        let now: Vec<u8> = rom.read(at, bytes.len())?.to_vec();
        let back: Vec<u8> = now
            .iter()
            .zip(bytes)
            .map(|(&n, &b)| if b == 0 && n == MASK { 0 } else { n })
            .collect();
        rom.write(at, &back)?;
        for pc in short_gap(rom, at, bytes.len()) {
            if rom.data()[pc] == MASK {
                rom.write(pc_addr(rom, pc)?, &[0])?;
            }
        }
    }
    Ok(())
}

/// The file offsets from the end of the block at `at` to its bank's end,
/// if fewer than a tag's length.
fn short_gap(rom: &Rom, at: SnesAddr, len: usize) -> std::ops::Range<usize> {
    let Ok(start) = rom.pc(at) else { return 0..0 };
    let end = start.as_usize() + len;
    let Ok(bank_end) = rom
        .mapping()
        .bank_end(crate::addr::PcAddr::new(start.as_usize() as u32))
    else {
        return 0..0;
    };
    let bank_end = bank_end.as_usize().min(rom.len());
    if end < bank_end && bank_end - end < rats::TAG_LEN {
        end..bank_end
    } else {
        0..0
    }
}

fn pc_addr(rom: &Rom, pc: usize) -> Result<SnesAddr, RomError> {
    Ok(rom
        .mapping()
        .pc_to_snes(crate::addr::PcAddr::new(pc as u32))?)
}

/// Writes the insert's sites, and clears bit 0 of
/// [`sprites::SPRITE_LIMIT_FLAGS`] for its 255-sprite option.
pub fn write_sites(rom: &mut Rom, insert: &Insert) -> Result<(), RomError> {
    for (&at, bytes) in &insert.sites {
        rom.write(at, bytes)?;
    }
    if insert.sprites_255 {
        let flags = rom.read_u8(sprites::SPRITE_LIMIT_FLAGS)?;
        rom.write_u8(sprites::SPRITE_LIMIT_FLAGS, flags & !1)?;
    }
    Ok(())
}

/// The file ranges an insert covers in `rom`: its sites, and its blocks
/// with their tags.
pub fn spans(rom: &Rom, insert: &Insert) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    for (&at, bytes) in &insert.sites {
        if let Ok(pc) = rom.pc(at) {
            out.push(pc.as_usize()..pc.as_usize() + bytes.len());
        }
    }
    if insert.sprites_255
        && let Ok(pc) = rom.pc(sprites::SPRITE_LIMIT_FLAGS)
    {
        out.push(pc.as_usize()..pc.as_usize() + 1);
    }
    for (&at, bytes) in &insert.blocks {
        if let Ok(pc) = rom.pc(at) {
            out.push(pc.as_usize() - rats::TAG_LEN..pc.as_usize() + bytes.len());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 MiB LoROM image, `$FF` up to the vanilla image's end and `$00`
    /// after, and a block's tag and contents written into it.
    fn image() -> Rom {
        let mut data = vec![0xFF; 0x10_0000];
        data[0x8_0000..].fill(0);
        data[0x7FC0..0x7FD5].copy_from_slice(b"SUPER MARIOWORLD     ");
        data[0x7FD5] = 0x20;
        data[0x7FD7] = 0x0A;
        Rom::from_bytes(data).unwrap()
    }

    fn block(rom: &mut Rom, at: u32, bytes: &[u8]) {
        let [lo, hi] = ((bytes.len() - 1) as u16).to_le_bytes();
        let tag = [b'S', b'T', b'A', b'R', lo, hi, !lo, !hi];
        rom.write(SnesAddr::new(at - 8), &tag).unwrap();
        rom.write(SnesAddr::new(at), bytes).unwrap();
    }

    fn long(at: u32) -> [u8; 3] {
        [at as u8, (at >> 8) as u8, (at >> 16) as u8]
    }

    #[test]
    fn reads_what_pixis_tables_lead_to() {
        let base = image();
        let mut rom = image();
        // PIXI's main block: its global table, sprite 00's main pointer
        // into a code block; a hook into it.
        let main = 0x108008;
        let mut table = vec![0u8; 0x1000];
        table[ENTRY_MAIN..ENTRY_MAIN + 3].copy_from_slice(&long(0x118008));
        table.extend([0x6B; 16]);
        block(&mut rom, main, &table);
        // The sprite's code, whose PROT list names its data.
        let mut code = b"PROT\x03".to_vec();
        code.extend(long(0x128008));
        code.extend(b"STOP\0");
        code.push(0x6B);
        block(&mut rom, 0x118008, &code);
        block(&mut rom, 0x128008, &[1, 2, 3]);
        // A block nothing of PIXI's names.
        block(&mut rom, 0x138008, &[0x6B]);
        rom.write(HEADER, b"STSD").unwrap();
        rom.write(VERSION, &[143, 0]).unwrap();
        rom.write(TABLE, &long(main)).unwrap();
        let hook = main + 0x1000;
        let mut jml = vec![JML];
        jml.extend(long(hook));
        jml.push(NOP);
        rom.write(SnesAddr::new(0x02A963), &jml).unwrap();
        // At a site Lunar Magic also writes, a jump that is not into
        // PIXI's block is not PIXI's.
        let mut foreign = vec![JML];
        foreign.extend(long(0x138008));
        foreign.push(NOP);
        rom.write(SnesAddr::new(0x02A846), &foreign).unwrap();

        let (insert, notes) = read(&rom, &base).unwrap().unwrap();
        assert_eq!(insert.version, 143);
        let blocks: Vec<u32> = insert.blocks.keys().map(|a| a.raw()).collect();
        assert_eq!(blocks, [main, 0x118008, 0x128008]);
        assert_eq!(insert.blocks[&SnesAddr::new(0x128008)], [1, 2, 3]);
        let sites: Vec<u32> = insert.sites.keys().map(|a| a.raw()).collect();
        assert_eq!(sites, [0x02A963, 0x02FFE2]);
        assert_eq!(insert.sites[&SnesAddr::new(0x02A963)], jml);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("$02A846"), "{notes:?}");
        // The same jump into PIXI's block is carried.
        let mut ours = vec![JML];
        ours.extend(long(hook));
        ours.push(NOP);
        rom.write(SnesAddr::new(0x02A846), &ours).unwrap();
        let (insert, notes) = read(&rom, &base).unwrap().unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(insert.sites[&SnesAddr::new(0x02A846)], ours);

        // Written onto the base, the insert is the ROM's again, but for the
        // block nothing of PIXI's names.
        let mut again = image();
        write_blocks(&mut again, &insert).unwrap();
        write_sites(&mut again, &insert).unwrap();
        let (again_insert, _) = read(&again, &base).unwrap().unwrap();
        assert_eq!(again_insert, insert);
        // A block's space must be free.
        let error = write_blocks(&mut again, &insert).unwrap_err().to_string();
        assert!(error.contains("$108008"), "{error}");
    }

    /// A block the base has (SA-1 Pack's, on an SA-1 ROM) is not PIXI's,
    /// nor is a hook into it.
    #[test]
    fn the_bases_blocks_are_not_pixis() {
        let mut base = image();
        block(&mut base, 0x118008, &[0x6B; 4]);
        let mut rom = image();
        // The hack's has another length: another version of it.
        block(&mut rom, 0x118008, &[0x6B; 6]);
        block(&mut rom, 0x108008, &[0x6B; 16]);
        rom.write(HEADER, b"STSD").unwrap();
        rom.write(VERSION, &[143, 0]).unwrap();
        rom.write(TABLE, &long(0x108008)).unwrap();
        let mut jsl = vec![JSL];
        jsl.extend(long(0x118008));
        jsl.push(NOP);
        rom.write(SnesAddr::new(0x018172), &jsl).unwrap();
        let (insert, notes) = read(&rom, &base).unwrap().unwrap();
        let blocks: Vec<u32> = insert.blocks.keys().map(|a| a.raw()).collect();
        assert_eq!(blocks, [0x108008]);
        assert!(!insert.sites.contains_key(&SnesAddr::new(0x018172)));
        assert!(notes[0].contains("$018172"), "{notes:?}");
    }

    /// Masking takes a block's free bytes, and a gap too short for a tag
    /// before its bank's end, out of what Asar can take, and puts them back.
    #[test]
    fn masking_round_trips() {
        let mut rom = image();
        // Contents up to a byte short of bank $10's end.
        let at = 0x108008;
        let bytes = [[0x6B, 0x00].repeat(0x3FFB), vec![0]].concat();
        block(&mut rom, at, &bytes);
        let insert = Insert {
            blocks: BTreeMap::from([(SnesAddr::new(at), bytes.clone())]),
            ..Insert::default()
        };
        let before = rom.data().to_vec();
        mask_blocks(&mut rom, &insert).unwrap();
        let masked = rom.read(SnesAddr::new(at), bytes.len() + 1).unwrap();
        assert!(!masked.contains(&0));
        unmask_blocks(&mut rom, &insert).unwrap();
        assert_eq!(rom.data(), &before[..]);
    }

    /// A block where the base has one, or anything but free space, cannot
    /// go where the hack has it; past the base's end, any can. Its size
    /// table alone goes where a build has room.
    #[test]
    fn conflicts_and_the_size_table_alone() {
        let mut base = image();
        block(&mut base, 0x118008, &[0x6B; 4]);
        let insert = Insert {
            blocks: BTreeMap::from([
                (SnesAddr::new(0x108008), vec![1]),
                (SnesAddr::new(0x118010), vec![2]),
                (SnesAddr::new(0x308008), vec![3]),
            ]),
            ..Insert::default()
        };
        assert_eq!(conflicts(&insert, &base), [SnesAddr::new(0x118010)]);
        // The vanilla-sized clean ROM has nothing past its end.
        let small = Rom::from_bytes(image().data()[..0x8_0000].to_vec()).unwrap();
        assert!(conflicts(&insert, &small).is_empty());

        let mut rom = image();
        let table: Vec<u8> = (0..0x400).map(|i| (i % 7) as u8).collect();
        write_size_table(&mut rom, &table).unwrap();
        assert_eq!(sprites::pixi_size_table(&rom).unwrap().unwrap(), &table[..]);
        assert_eq!(size_table(&rom).unwrap(), table);
    }

    #[test]
    fn a_rom_without_pixi_has_no_insert() {
        let rom = image();
        assert!(read(&rom, &image()).unwrap().is_none());
        // Marked, but with its table pointing nowhere.
        let mut rom = image();
        rom.write(HEADER, b"STSD").unwrap();
        assert!(read(&rom, &image()).unwrap().is_none());
    }

    #[test]
    fn version_names() {
        assert_eq!(version_name(0x02), "1.02");
        assert_eq!(version_name(0x1D), "1.2.9");
        assert_eq!(version_name(0x23), "1.2.15");
        assert_eq!(version_name(0x32), "1.32");
        assert_eq!(version_name(143), "1.43");
    }

    #[test]
    fn prot_lists() {
        let mut bytes = b"PROT\x06".to_vec();
        bytes.extend(long(0x108000));
        bytes.extend(long(0x118000));
        bytes.extend(b"STOP\0\x6B");
        assert_eq!(
            protected(&bytes),
            [SnesAddr::new(0x108000), SnesAddr::new(0x118000)]
        );
        assert!(protected(&[0x6B]).is_empty());
        assert!(protected(b"PROT\x09\x00").is_empty());
    }
}
