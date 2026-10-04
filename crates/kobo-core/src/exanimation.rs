//! Lunar Magic's ExAnimation: animated tiles and colours, a list per level
//! and a global one, with their triggers (docs/lunar-magic-install.md,
//! "ExAnimation"). The one place that knows its data format and tables;
//! import reads and build writes through it, and Kobo's code for it is
//! `asm/lunar-magic/exanimation.asm`.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::rats;
use crate::rom::{Rom, RomError};

/// Each level's settings byte, `PTLG----`.
pub const SETTINGS: SnesAddr = SnesAddr::new(0x03FE00);
/// The 3-byte pointers to the uncompressed ExGFX files `60`-`63`.
pub const ALT_FILES: SnesAddr = SnesAddr::new(0x03BCC0);
/// The level load's hook, whose target holds the tables' addresses.
pub const LOAD_HOOK: SnesAddr = SnesAddr::new(0x0583AD);
/// The NMI's upload hook: a `JSL` here is what Lunar Magic checks.
pub const CHECK: SnesAddr = SnesAddr::new(0x00A390);
/// From the load hook's target: the level table's address.
pub const TABLE_OFFSET: u32 = 0xEA;
/// From the load hook's target: the global list's bank times `$100`.
pub const GLOBAL_BANK_OFFSET: u32 = 0x5B;
/// From the load hook's target: the global list's low word.
pub const GLOBAL_LOW_OFFSET: u32 = 0x65;
/// The first alternative file's number.
pub const FIRST_ALT_FILE: u16 = 0x60;
/// The most an alternative file may hold (Lunar Magic's help).
pub const ALT_FILE_MAX: usize = 0x8000;
/// Where a level's load leaves its AN2 file (`ram::GFX_BUFFER`), as a
/// frame's word: the animated tile frames read it there.
pub const AN2_FRAMES: u16 = 0xAD00;
/// The most of the AN2 file the buffer at [`AN2_FRAMES`] holds.
pub const AN2_LEN: u16 = 0x1A00;
/// A level table entry for no list.
pub const NONE: u32 = 0x00_00FF;
/// Slots a list may have.
pub const SLOTS: usize = 32;

#[derive(Debug, Error)]
pub enum AnimationError {
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("the list ends before its {0}")]
    Truncated(&'static str),
    #[error("slot {slot:02X}: type {kind:02X} is not one Lunar Magic has")]
    UnknownType { slot: u8, kind: u8 },
    #[error("more than {SLOTS} slots")]
    TooManySlots,
    #[error(
        "the ROM keeps Lunar Magic 2.41's older lists, which Kobo does not read from a ROM; \
         import the levels from Lunar Magic's MWL exports, which convert them"
    )]
    OlderLayout,
}

/// A level's settings byte: which animations run in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings(pub u8);

impl Settings {
    /// The game's colour `$64` flashing is off.
    pub const NO_GAME_COLOURS: u8 = 0x80;
    /// The game's animated tiles are off.
    pub const NO_GAME_TILES: u8 = 0x40;
    /// The level's list is off.
    pub const NO_LEVEL: u8 = 0x20;
    /// The global list is off.
    pub const NO_GLOBAL: u8 = 0x10;

    /// What Lunar Magic's first save sets: everything on, but level
    /// `104`'s lists (the ending's Yoshi's House).
    pub fn default_for(level: u16) -> Self {
        Self(if level == 0x104 { 0x30 } else { 0 })
    }

    /// Whether `flag` (one of the constants) says something is on.
    pub fn on(self, flag: u8) -> bool {
        self.0 & flag == 0
    }
}

/// What a slot's type animates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `01`-`12`: 8x8 tiles, to VRAM.
    Tiles,
    /// `13`-`17`: colours.
    Colours,
    /// `18`-`1B`: colours rotated in the palette.
    Rotation,
}

impl Kind {
    pub fn of(kind: u8) -> Option<Self> {
        match kind {
            0x01..=0x12 => Some(Kind::Tiles),
            0x13..=0x17 => Some(Kind::Colours),
            0x18..=0x1B => Some(Kind::Rotation),
            _ => None,
        }
    }
}

/// Whether a trigger has a second set of frames, as Lunar Magic reads a
/// list: the game's conditions but the one-shots, the reserved ones, and
/// the custom triggers.
pub fn second_set(trigger: u8) -> bool {
    matches!(trigger, 0x01..=0x05 | 0x07 | 0x09..=0x0E | 0x20..=0x2F)
}

/// One slot of a list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    /// `01`-`1B` (`names::exanimation_type`).
    pub kind: u8,
    /// `00`-`4F` (`names::exanimation_trigger`).
    pub trigger: u8,
    /// Frames less one; for a rotation, how many times it waits less one.
    pub frames_less_one: u8,
    /// Tiles: the VRAM word address, bit 15 for sources in the alternative
    /// file. Colours: the first colour, then in the high byte the colours
    /// less one, bit 7 (bit 15 here) for the alternative file.
    pub dest: u16,
    /// A word per frame (twice as many with a second set; none for a
    /// rotation): a RAM address in bank `$7E`, an offset into the
    /// alternative file, or a colour.
    pub frames: Vec<u16>,
}

impl Slot {
    /// The words of frames its type and trigger give it.
    pub fn frame_words(kind: u8, trigger: u8, frames_less_one: u8) -> usize {
        if Kind::of(kind) == Some(Kind::Rotation) {
            return 0;
        }
        (frames_less_one as usize + 1) * if second_set(trigger) { 2 } else { 1 }
    }

    /// Its entry's length in bytes.
    pub fn entry_len(&self) -> usize {
        5 + 2 * self.frames.len()
    }

    fn to_bytes(&self, out: &mut Vec<u8>) {
        out.extend([self.kind, self.trigger, self.frames_less_one]);
        out.extend(self.dest.to_le_bytes());
        for f in &self.frames {
            out.extend(f.to_le_bytes());
        }
    }

    /// Colours: the first colour.
    pub fn colour(&self) -> u8 {
        self.dest as u8
    }

    /// Colours: how many.
    pub fn colours(&self) -> u16 {
        ((self.dest >> 8) & 0x7F) + 1
    }

    /// Whether the sources are in the alternative file.
    pub fn alternative(&self) -> bool {
        self.dest & 0x8000 != 0
    }

    /// Whether its frames are addresses (in bank `$7E`, or offsets into
    /// the alternative file) rather than colours: tiles, and types
    /// `13`-`15` with more than one colour. A single colour is the frame's
    /// word itself, and so is any colour of `16` and `17`.
    pub fn frames_are_addresses(&self) -> bool {
        match Kind::of(self.kind) {
            Some(Kind::Tiles) => true,
            Some(Kind::Colours) => matches!(self.kind, 0x13..=0x15) && self.colours() > 1,
            _ => false,
        }
    }

    /// Whether every frame is an address in the AN2 file's buffer, from
    /// [`AN2_FRAMES`] for [`AN2_LEN`] bytes (none, for a slot without
    /// frames or whose frames are colours or in the alternative file).
    pub fn frames_in_an2(&self) -> bool {
        self.frames_are_addresses()
            && !self.alternative()
            && !self.frames.is_empty()
            && self
                .frames
                .iter()
                .all(|&f| f.wrapping_sub(AN2_FRAMES) < AN2_LEN)
    }
}

/// A level's list, or the global one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct List {
    /// The slots it says it uses (the highest used plus one, as Lunar
    /// Magic writes it; a list may say more, or have none).
    pub count: u8,
    /// The alternative file, 0-3 for ExGFX `60`-`63`.
    pub alt_file: u8,
    /// The custom triggers that keep their state at the load; the others
    /// are cleared.
    pub custom_keep: u16,
    /// The custom triggers the load sets.
    pub custom_set: u16,
    /// The frames the load sets manual triggers to.
    pub manual: BTreeMap<u8, u8>,
    pub slots: BTreeMap<u8, Slot>,
}

impl Default for List {
    fn default() -> Self {
        Self {
            count: 0,
            alt_file: 0,
            custom_keep: 0xFFFF,
            custom_set: 0,
            manual: BTreeMap::new(),
            slots: BTreeMap::new(),
        }
    }
}

impl List {
    /// Reads a list from its bytes, returning it and its length. Every
    /// corpus list's length by this equals its RATS tag.
    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), AnimationError> {
        let get = |i: usize, what| bytes.get(i).copied().ok_or(AnimationError::Truncated(what));
        let word = |i: usize, what| {
            Ok::<u16, AnimationError>(u16::from_le_bytes([get(i, what)?, get(i + 1, what)?]))
        };
        let count = get(0, "header")?;
        if count as usize > SLOTS {
            return Err(AnimationError::TooManySlots);
        }
        let alt_file = get(1, "header")?;
        let custom_keep = word(2, "header")?;
        let custom_set = word(4, "header")?;
        let mask = word(6, "header")?;
        let mut manual = BTreeMap::new();
        let mut at = 8;
        for trigger in 0..16u8 {
            if mask & (1 << trigger) != 0 {
                manual.insert(trigger, get(at, "manual frames")?);
                at += 1;
            }
        }
        let base = at;
        let mut end = base + 2 * count as usize;
        let mut slots = BTreeMap::new();
        for slot in 0..count {
            let offset = word(base + 2 * slot as usize, "slot offsets")? as usize;
            if offset == 0 {
                continue;
            }
            let p = base + offset;
            let kind = get(p, "slot")?;
            if Kind::of(kind).is_none() {
                return Err(AnimationError::UnknownType { slot, kind });
            }
            let trigger = get(p + 1, "slot")?;
            let frames_less_one = get(p + 2, "slot")?;
            let dest = word(p + 3, "slot")?;
            let n = Slot::frame_words(kind, trigger, frames_less_one);
            let frames = (0..n)
                .map(|i| word(p + 5 + 2 * i, "frames"))
                .collect::<Result<Vec<_>, _>>()?;
            end = end.max(p + 5 + 2 * n);
            slots.insert(
                slot,
                Slot {
                    kind,
                    trigger,
                    frames_less_one,
                    dest,
                    frames,
                },
            );
        }
        Ok((
            Self {
                count,
                alt_file,
                custom_keep,
                custom_set,
                manual,
                slots,
            },
            end,
        ))
    }

    /// The list's bytes: the slots' entries in slot order, after the
    /// offsets, as Lunar Magic writes them.
    pub fn to_bytes(&self) -> Vec<u8> {
        let count = self.slots_used();
        let mut out = vec![count, self.alt_file];
        out.extend(self.custom_keep.to_le_bytes());
        out.extend(self.custom_set.to_le_bytes());
        let mask = self.manual.keys().fold(0u16, |m, &t| m | 1 << t);
        out.extend(mask.to_le_bytes());
        out.extend(self.manual.values());
        let mut offsets = Vec::new();
        let mut body = Vec::new();
        for slot in 0..count {
            match self.slots.get(&slot) {
                Some(s) => {
                    offsets.extend((2 * count as u16 + body.len() as u16).to_le_bytes());
                    s.to_bytes(&mut body);
                }
                None => offsets.extend([0, 0]),
            }
        }
        out.extend(offsets);
        out.extend(body);
        out
    }

    /// The slots the list says it uses: its count, or more if a slot is
    /// past it.
    pub fn slots_used(&self) -> u8 {
        let highest = self.slots.keys().next_back().map_or(0, |&s| s + 1);
        self.count.max(highest)
    }
}

/// Where a ROM keeps its ExAnimation, when it has Lunar Magic's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// 1.70 on: the level table's address at the load hook's target plus
    /// [`TABLE_OFFSET`], the global list's at [`GLOBAL_BANK_OFFSET`] and
    /// [`GLOBAL_LOW_OFFSET`].
    Current { target: SnesAddr, table: SnesAddr },
    /// No level table at [`TABLE_OFFSET`]: Lunar Magic 2.41's older lists
    /// (Kaizo Mario World 3), which Kobo does not read from a ROM. Where
    /// their table is was found in a way not recorded, and so was dropped
    /// (clean-room-audit.md); Lunar Magic's MWL export converts them.
    Older,
}

/// The ROM's layout, from the `JSL` at [`LOAD_HOOK`]; `None` without one.
pub fn layout(rom: &Rom) -> Result<Option<Layout>, RomError> {
    if rom.read_u8(LOAD_HOOK)? != 0x22 {
        return Ok(None);
    }
    let target = rom.read_ptr(LOAD_HOOK.add(1))?;
    let table = rom.read_u24(target.add(TABLE_OFFSET))?;
    if table >> 8 & 0xFF != 0 && rom.read(SnesAddr::new(table), 3 * 512).is_ok() {
        return Ok(Some(Layout::Current {
            target,
            table: SnesAddr::new(table),
        }));
    }
    Ok((table == 0).then_some(Layout::Older))
}

/// A list read from a ROM, and where it was.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Found {
    pub list: List,
    pub at: SnesAddr,
}

/// A level's list; `None` for none, and in the [`Layout::Older`], whose
/// lists Kobo does not read ([`read_global`] says so, once).
pub fn read_level(rom: &Rom, level: u16) -> Result<Option<Found>, AnimationError> {
    let Some(Layout::Current { table, .. }) = layout(rom)? else {
        return Ok(None);
    };
    let pointer = rom.read_u24(table.add(3 * level as u32))?;
    if pointer >> 8 & 0xFF == 0 || pointer & 0xFFFF == 0xFFFF {
        return Ok(None);
    }
    let at = SnesAddr::new(pointer);
    let bytes = rom.read_tail(at)?;
    Ok(Some(Found {
        list: List::parse(bytes)?.0,
        at,
    }))
}

/// The global list; `None` for none. A ROM in the [`Layout::Older`] is an
/// error, which tells an import that its lists were not taken.
pub fn read_global(rom: &Rom) -> Result<Option<Found>, AnimationError> {
    let target = match layout(rom)? {
        None => return Ok(None),
        Some(Layout::Current { target, .. }) => target,
        Some(Layout::Older) => return Err(AnimationError::OlderLayout),
    };
    let bank = rom.read_u16(target.add(GLOBAL_BANK_OFFSET))?;
    if bank == 0 {
        return Ok(None);
    }
    let low = rom.read_u16(target.add(GLOBAL_LOW_OFFSET))?;
    let at = SnesAddr::new((bank as u32) << 8 | low as u32);
    Ok(Some(Found {
        list: List::parse(rom.read_tail(at)?)?.0,
        at,
    }))
}

/// A level's settings byte, where the ROM has Lunar Magic's ExAnimation
/// (or any table there: Lunar Magic's first save writes it); `None` for
/// the vanilla ROM's `$FF`.
pub fn read_settings(rom: &Rom, level: u16) -> Result<Option<Settings>, RomError> {
    let byte = rom.read_u8(SETTINGS.add(level as u32))?;
    Ok((byte != 0xFF || layout(rom)?.is_some()).then_some(Settings(byte)))
}

/// The alternative files `60`-`63` the ROM has, each the RATS block its
/// pointer leads to.
pub fn read_alt_files(rom: &Rom) -> Result<Vec<(u16, Vec<u8>)>, RomError> {
    let mut out = Vec::new();
    if layout(rom)?.is_none() {
        return Ok(out);
    }
    for n in 0..4u16 {
        let pointer = rom.read_u24(ALT_FILES.add(3 * n as u32))?;
        if pointer == 0 || pointer == 0xFF_FFFF {
            continue;
        }
        let at = SnesAddr::new(pointer);
        let Ok(pc) = rom.pc(at) else { continue };
        let Some(len) = pc
            .as_usize()
            .checked_sub(8)
            .and_then(|tag| rats::tag_at(rom.data(), tag))
        else {
            continue;
        };
        out.push((FIRST_ALT_FILE + n, rom.read(at, len)?.to_vec()));
    }
    Ok(out)
}

/// Where the ROM's ExAnimation is: its tables, lists, and files, for an
/// import's report of what it read.
pub fn spans(rom: &Rom) -> Vec<(SnesAddr, usize)> {
    let mut out = Vec::new();
    let Ok(Some(layout)) = layout(rom) else {
        return out;
    };
    out.push((SETTINGS, 512));
    out.push((ALT_FILES, 16));
    let Layout::Current { table, .. } = layout else {
        return out;
    };
    out.push((table, 3 * 512));
    for level in 0..512u32 {
        let Ok(pointer) = rom.read_u24(table.add(3 * level)) else {
            continue;
        };
        if pointer >> 8 & 0xFF == 0 {
            continue;
        }
        let at = SnesAddr::new(pointer);
        let Ok(bytes) = rom.read_tail(at) else {
            continue;
        };
        if let Some(len) = List::parse(bytes).ok().map(|(_, n)| n) {
            out.push((at, len));
        }
    }
    if let Ok(Some(found)) = read_global(rom) {
        out.push((found.at, found.list.to_bytes().len()));
    }
    if let Ok(files) = read_alt_files(rom) {
        for (n, bytes) in files {
            if let Ok(pointer) = rom.read_u24(ALT_FILES.add(3 * (n - FIRST_ALT_FILE) as u32)) {
                out.push((SnesAddr::new(pointer), bytes.len()));
            }
        }
    }
    out
}

/// Why a build refuses a list, or `None`: triggers past `4F`, the precision
/// timer (`0F`) on a type that uploads frames (which Lunar Magic's code makes
/// upload its own work RAM), and frame counts past what the format holds.
pub fn refusal(list: &List) -> Option<String> {
    if list.alt_file > 3 {
        return Some(format!("alternative file {} is not 0-3", list.alt_file));
    }
    if list.slots_used() as usize > SLOTS {
        return Some("more than 32 slots".into());
    }
    for (&n, s) in &list.slots {
        let kind = Kind::of(s.kind);
        let at = format!("slot {n:02X}");
        if kind.is_none() {
            return Some(format!(
                "{at}: type {:02X} is not one Lunar Magic has",
                s.kind
            ));
        }
        if s.trigger > 0x4F {
            return Some(format!(
                "{at}: trigger {:02X} is not one Lunar Magic has",
                s.trigger
            ));
        }
        // The precision timer on a type that uploads from a frame: Lunar
        // Magic's code then uploads bytes of its own work RAM instead
        // (docs/lunar-magic-install.md, "ExAnimation").
        if s.trigger == 0x0F && !matches!(s.kind, 0x16..=0x1B) {
            return Some(format!(
                "{at}: trigger 0F on type {:02X} uploads Lunar Magic's own scratch, not the frames",
                s.kind
            ));
        }
        let n_words = Slot::frame_words(s.kind, s.trigger, s.frames_less_one);
        if s.frames.len() != n_words {
            return Some(format!(
                "{at}: {} frames where its type and trigger take {n_words}",
                s.frames.len()
            ));
        }
        if second_set(s.trigger) && s.frames_less_one > 0x7F {
            return Some(format!("{at}: more than 128 frames with a second set"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> List {
        let mut list = List {
            alt_file: 1,
            custom_keep: 0xFF00,
            custom_set: 0x0012,
            ..List::default()
        };
        list.manual.insert(0, 1);
        list.manual.insert(2, 2);
        list.slots.insert(
            0,
            Slot {
                kind: 0x04,
                trigger: 0x03,
                frames_less_one: 1,
                dest: 0x2000,
                frames: vec![0xAD00, 0xAD80, 0xAE00, 0xAE80],
            },
        );
        list.slots.insert(
            3,
            Slot {
                kind: 0x18,
                trigger: 0x00,
                frames_less_one: 1,
                dest: 0x0331,
                frames: vec![],
            },
        );
        list
    }

    #[test]
    fn lists_read_back() {
        let list = sample();
        let bytes = list.to_bytes();
        let (back, len) = List::parse(&bytes).unwrap();
        assert_eq!(len, bytes.len());
        assert_eq!(back, List { count: 4, ..list });
        assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn refusals() {
        let mut list = sample();
        assert_eq!(refusal(&list), None);
        list.slots.get_mut(&0).unwrap().trigger = 0x50;
        assert!(refusal(&list).unwrap().contains("trigger 50"));
        let mut list = sample();
        list.slots.get_mut(&0).unwrap().trigger = 0x0F;
        assert!(refusal(&list).unwrap().contains("scratch"));
        let mut list = sample();
        list.slots.get_mut(&0).unwrap().frames.pop();
        assert!(refusal(&list).unwrap().contains("frames"));
    }
}
