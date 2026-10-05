//! The clean room (`docs/clean-room.md`): Kobo never shows
//! anyone Lunar Magic's code. This module is the one place that decides
//! when a process holds a ROM Lunar Magic saved, and what every output
//! Kobo has then leaves out.
//!
//! A ROM is recognised by its marker, or by code in one of the areas
//! Lunar Magic fills on every save whatever it finds there
//! ([`saved_by_lunar_magic`]). Once one has been loaded the process is
//! [`forbidden`] for good: a ROM's code can be in a trace kept before,
//! and a process may hold two. From then on:
//!
//! - `KOBO_CPU_TRACE` keeps and prints nothing (`cpu::Cpu`), and a
//!   `KOBO_RAM_WATCH` report gives the write alone, without the
//!   instruction that made it, or nothing at all for a push.
//! - A CPU error gives no address ([`CodeAddr`]), and an access to
//!   unmodelled hardware gives none for an instruction fetch.
//! - A data-read trace (`expand::expand_level_traced`) gives the
//!   addresses read without the instructions that read them, and a
//!   routine is called into (`expand::call_in_level`) only where the
//!   game's code or a hook starts ([`may_call`]), giving back its RAM and
//!   not the registers it left.
//! - `kobo asm` leaves out what a patch prints, and the text of Asar's
//!   errors and warnings, which can hold what a patch read of the ROM.
//! - A Map16 definition the ROM's own routine found is shown, as text or
//!   as a picture of the tile, only where the ROM's tables put that tile
//!   ([`Map16Shown`]): elsewhere the routine may have pointed into code.
//!
//! Whatever ROM is loaded, a RAM dump leaves out the stacks and SA-1
//! Pack's call pointers ([`withheld`]; dumps read through [`peek`] and
//! [`bytes`]): they hold return addresses and code addresses, and
//! nothing Kobo studies is kept there.
//!
//! The bus that runs a ROM's code (`cpu::smw_bus`) is the crate's own,
//! so the library offers no way to step a ROM's code outside these rules;
//! `cpu::Cpu` is public for the CPU tests' flat memory.
//!
//! The scripts under `tools/` follow the same rules (`tools/clean_room.py`,
//! and a check of their own in the Mesen scripts).

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::addr::SnesAddr;
use crate::map16::{self, GameTables};
use crate::ram::{Ram, RamAddr, RamMap};
use crate::rom::Rom;

/// Where Lunar Magic writes `Lunar Magic Version X.YZ` on every save.
pub const MARKER: SnesAddr = SnesAddr::new(0x0FF0A0);

/// Areas the vanilla game leaves `$FF` and Lunar Magic fills with its own
/// code on every save, whatever is there (`docs/lunar-magic-install.md`,
/// "What a save does with its install there"). Kobo's builds leave them
/// free, as SA-1 Pack and the pinned tools do, so code in either means
/// Lunar Magic has saved the ROM, with or without its marker. (The third
/// such area, `$0EF510`, holds Kobo's background entry.)
pub const SAVE_AREAS: [(SnesAddr, usize); 2] = [
    (SnesAddr::new(0x03BB00), 0x1F),
    (SnesAddr::new(0x03BCA0), 0x20),
];

/// Whether Lunar Magic has saved `rom`: its marker, or anything but
/// `$FF` in either of [`SAVE_AREAS`]. A ROM another tool wrote into those
/// areas counts too, which costs it only the trace.
pub fn saved_by_lunar_magic(rom: &Rom) -> bool {
    rom.lunar_magic_version().is_some()
        || SAVE_AREAS.iter().any(|&(addr, len)| {
            rom.read(addr, len)
                .is_ok_and(|bytes| bytes.iter().any(|&b| b != 0xFF))
        })
}

static FORBIDDEN: AtomicBool = AtomicBool::new(false);

/// Forbids the process if Lunar Magic has saved `rom`. Every [`Rom`] is
/// checked as it is made, and the bus checks the one it runs.
pub fn check(rom: &Rom) {
    if saved_by_lunar_magic(rom) {
        forbid();
    }
}

/// Turns every instruction-level output off for the rest of the process.
pub fn forbid() {
    if !FORBIDDEN.swap(true, Ordering::Relaxed) && std::env::var_os("KOBO_CPU_TRACE").is_some() {
        eprintln!("KOBO_CPU_TRACE ignored: a ROM Lunar Magic saved is loaded (clean room)");
    }
}

/// Whether a ROM Lunar Magic saved has been loaded in this process.
pub fn forbidden() -> bool {
    FORBIDDEN.load(Ordering::Relaxed)
}

/// The address of an instruction, as an error reports it: shown unless
/// the process is [`forbidden`], when where a ROM's code stopped would
/// say where Lunar Magic's code is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CodeAddr(u32);

impl CodeAddr {
    pub fn new(bank: u8, offset: u16) -> Self {
        Self(((bank as u32) << 16) | offset as u32)
    }

    pub fn bank(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn offset(self) -> u16 {
        self.0 as u16
    }

    fn show(self, f: &mut fmt::Formatter<'_>, forbidden: bool) -> fmt::Result {
        if forbidden {
            write!(f, "(an address withheld: clean room)")
        } else {
            write!(f, "${:02X}:{:04X}", self.bank(), self.offset())
        }
    }
}

impl fmt::Display for CodeAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.show(f, forbidden())
    }
}

impl fmt::Debug for CodeAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.show(f, forbidden())
    }
}

/// Whether `addr` in `rom` is somewhere a routine may be called into, or
/// stopped at, once the process is [`forbidden`]: an instruction of the
/// vanilla game's, as its first two bytes say, or a hook, where a change
/// to the game's code starts (the byte before is still the game's).
/// Anywhere else may be the middle of Lunar Magic's code, and calls into
/// it one address at a time would read it instruction by instruction.
/// `addr` is taken to be where one of the game's instructions starts.
pub fn may_call(rom: &Rom, vanilla: &Rom, addr: SnesAddr) -> bool {
    let byte = |r: &Rom, a: u32| r.read(SnesAddr::new(a), 1).ok().map(|b| b[0]);
    let same = |a: u32| byte(rom, a).is_some_and(|b| Some(b) == byte(vanilla, a));
    // Vanilla's free space is `$FF`, and Lunar Magic writes its code there:
    // two `$FF` in a row are not the game's code.
    let code = |a: u32| byte(vanilla, a) != Some(0xFF) || byte(vanilla, a + 1) != Some(0xFF);
    let a = addr.raw();
    let intact = same(a) && same(a + 1) && code(a);
    let hook = a & 0xFFFF != 0x8000 && !same(a) && same(a - 1) && code(a - 1);
    intact || hook
}

/// Which of the Map16 definitions a level's load resolved an output may
/// show ([`crate::expand::LevelTiles::map16_sources`] and `bg_map16_at`
/// say where each was read). A definition is the 8 bytes wherever the
/// ROM's own Map16 routine points, and that is not always a table: in a
/// boss arena, a Lunar Magic ROM's routine points into code. Once the
/// process is [`forbidden`], a definition is shown only where the library,
/// reading the ROM's tables as data, has that tile's definition: the
/// game's tables for pages 0 and 1 (any object tileset's, and the pipes'),
/// and Lunar Magic's for the pages past them and for BG Map16, within a
/// RATS block. Pages in an older Lunar Magic's layout, which the library
/// finds only through the ROM's routine, are withheld with the rest.
pub struct Map16Shown<'a> {
    rom: &'a Rom,
    forbidden: bool,
    game: Option<GameTables>,
    blocks: Vec<std::ops::Range<usize>>,
}

impl<'a> Map16Shown<'a> {
    pub fn new(rom: &'a Rom) -> Self {
        Self::with(rom, forbidden())
    }

    fn with(rom: &'a Rom, forbidden: bool) -> Self {
        let blocks = if forbidden {
            crate::rats::blocks(rom)
                .into_iter()
                .filter_map(|b| {
                    rom.pc(b.start)
                        .ok()
                        .map(|pc| pc.as_usize()..pc.as_usize() + b.len)
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            rom,
            forbidden,
            game: forbidden.then(|| GameTables::read(rom).ok()).flatten(),
            blocks,
        }
    }

    /// Whether foreground tile `tile`'s definition, read at bus address
    /// `at`, may be shown.
    pub fn foreground(&self, tile: u16, at: u32) -> bool {
        if !self.forbidden {
            return true;
        }
        let at = SnesAddr::new(at);
        if (tile as usize) < map16::FG_TILE_COUNT {
            let game = self.game.as_ref().is_some_and(|game| {
                (0..map16::TILESET_COUNT).any(|tileset| game.address(tileset, tile) == at)
            });
            let pipes = [None, Some(0), Some(2), Some(3)]
                .into_iter()
                .any(|set| map16::pipe_address(set, tile) == Some(at));
            return game || pipes;
        }
        let page = (tile / 0x100) as u8;
        let in_group = map16::pages::PAGE_GROUPS.iter().any(|group| {
            group.pages().contains(&page)
                && group.definition(self.rom, tile).ok().flatten() == Some(at)
        });
        let per_tileset = page == 2
            && map16::pages::tileset_page2(self.rom).unwrap_or(false)
            && (0..map16::TILESET_COUNT).any(|tileset| {
                map16::pages::tileset_page2_definition(self.rom, tileset, tile).ok() == Some(at)
            });
        (in_group || per_tileset) && self.in_block(at)
    }

    /// Whether background tile `tile`'s definition may be shown, the
    /// level's definitions starting at bus address `base`.
    pub fn background(&self, tile: u16, base: u32) -> bool {
        if !self.forbidden {
            return true;
        }
        let at = SnesAddr::new(base.wrapping_add(8 * tile as u32));
        if base == map16::tables::MAP16_BG_TILES.raw() {
            return (tile as usize) < map16::BG_TILE_COUNT;
        }
        let table = (0..16).any(|t| {
            map16::pages::bg_table(self.rom, t).ok().flatten() == Some(SnesAddr::new(base))
        });
        table && self.in_block(at)
    }

    /// Whether the 8 bytes at `at` are within one RATS block.
    fn in_block(&self, at: SnesAddr) -> bool {
        self.rom.pc(at).is_ok_and(|pc| {
            let pc = pc.as_usize();
            self.blocks.iter().any(|b| b.start <= pc && pc + 8 <= b.end)
        })
    }
}

/// Whether a dump of RAM leaves out the byte at bus address `addr` (as the
/// S-CPU sees it): the S-CPU's stack page in work RAM, and with SA-1 Pack
/// the SA-1's in I-RAM and the pointers of the two processors' calls to
/// each other (`docs/sa1.md`).
/// `$0100`-`$010F` stay: the game keeps variables there, under the stack.
pub fn withheld(map: RamMap, addr: u32) -> bool {
    let (bank, offset) = ((addr >> 16) as u8, addr as u16);
    let system = matches!(bank, 0x00..=0x3F | 0x80..=0xBF);
    let wram_stack = (0x0110..0x0200).contains(&offset) && (bank == 0x7E || system);
    let sa1 = map == RamMap::Sa1Pack
        && system
        && ((0x3700..0x3800).contains(&offset) || (0x3180..0x3186).contains(&offset));
    wram_stack || sa1
}

/// The byte at bus address `addr`, as a dump shows it: zero where
/// [`withheld`]. Panics if the address is not RAM.
pub fn peek(ram: &Ram, addr: u32) -> u8 {
    if withheld(ram.map(), addr) {
        0
    } else {
        ram.peek(addr)
    }
}

/// `len` bytes from `addr` in the vanilla layout ([`Ram::bytes`]), as a
/// dump shows them: zero where the address they resolve to is
/// [`withheld`].
pub fn bytes(ram: &Ram, addr: RamAddr, len: usize) -> Vec<u8> {
    let map = ram.map();
    (addr.vanilla()..addr.vanilla() + len as u32)
        .map(|a| {
            let a = RamAddr::new(a);
            if withheld(map, map.resolve(a)) {
                0
            } else {
                ram.u8(a)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with(edit: impl FnOnce(&mut [u8])) -> Rom {
        let mut data = vec![0xFFu8; 0x8_0000];
        data[0x7FC0 + 0x15] = 0x20;
        edit(&mut data);
        Rom::from_bytes(data).unwrap()
    }

    #[test]
    fn a_save_is_known_by_its_marker_or_its_code() {
        assert!(!saved_by_lunar_magic(&rom_with(|_| {})));
        let marked =
            rom_with(|d| d[0x7_F0A0..0x7_F0B8].copy_from_slice(b"Lunar Magic Version 3.70"));
        assert!(saved_by_lunar_magic(&marked));
        // `$03BCBF`, the last byte of the last area, at file offset `$1BCBF`.
        assert!(saved_by_lunar_magic(&rom_with(|d| d[0x1_BCBF] = 0)));
        // `$0EF510`, where a Kobo build has its background entry.
        assert!(!saved_by_lunar_magic(&rom_with(|d| d[0x7_7510] = 0)));
        // `$03BB1F`, just past the first.
        assert!(!saved_by_lunar_magic(&rom_with(|d| d[0x1_BB1F] = 0)));
    }

    #[test]
    fn a_code_address_is_withheld_when_forbidden() {
        let at = CodeAddr::new(0x10, 0x8123);
        struct Shown(CodeAddr, bool);
        impl fmt::Display for Shown {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.show(f, self.1)
            }
        }
        assert_eq!(Shown(at, false).to_string(), "$10:8123");
        assert!(!Shown(at, true).to_string().contains("8123"));
    }

    #[test]
    fn calls_go_to_the_games_code_or_a_hook() {
        let vanilla = rom_with(|d| d[0x100..0x110].copy_from_slice(&[0xA9; 16]));
        let hooked = rom_with(|d| {
            d[0x100..0x110].copy_from_slice(&[0xA9; 16]);
            d[0x108..0x10C].copy_from_slice(&[0x22, 0x00, 0x80, 0x10]);
        });
        let at = |a: u32| SnesAddr::new(0x00_8000 + a);
        assert!(may_call(&hooked, &vanilla, at(0x100)));
        assert!(may_call(&hooked, &vanilla, at(0x108)));
        assert!(!may_call(&hooked, &vanilla, at(0x109)));
        assert!(!may_call(&hooked, &vanilla, at(0x10A)));
        assert!(!may_call(&hooked, &vanilla, at(0x107)));
        assert!(may_call(&hooked, &vanilla, at(0x106)));
        // In vanilla's free space, where Lunar Magic puts its code.
        let filled = rom_with(|d| d[0x200..0x204].copy_from_slice(&[0x22, 0x00, 0x80, 0x10]));
        assert!(!may_call(&filled, &vanilla, at(0x200)));
        assert!(!may_call(&filled, &vanilla, at(0x204)));
    }

    #[test]
    fn map16_definitions_are_shown_only_from_the_tables() {
        // 1.25 MiB of `$FF`: every tile of pages 0 and 1 in the common
        // table, and group `00`'s table at `$10:8008`, tile `200` first,
        // in a RATS block of 512 tiles.
        let mut data = vec![0xFFu8; 0x14_0000];
        data[0x7FC0 + 0x15] = 0x20;
        data[0x3_7553..0x3_7555].copy_from_slice(&0x7008u16.to_le_bytes());
        data[0x3_7557] = 0x10;
        data[0x8_0000..0x8_0008].copy_from_slice(b"STAR\xFF\x0F\x00\xF0");
        let rom = Rom::from_bytes(data).unwrap();
        let open = Map16Shown::with(&rom, false);
        assert!(open.foreground(0x10, 0x00_8000));
        assert!(open.background(0x10, 0x00_8000));
        let shown = Map16Shown::with(&rom, true);
        assert!(shown.foreground(0x10, 0x0D_8080));
        assert!(!shown.foreground(0x10, 0x0D_8088));
        assert!(!shown.foreground(0x10, 0x00_8000));
        assert!(shown.foreground(0x200, 0x10_8008));
        assert!(!shown.foreground(0x200, 0x10_8010));
        assert!(shown.foreground(0x3FF, 0x10_8FF8 + 8));
        // Tile `400` is where the table says, but past the block's end.
        assert!(!shown.foreground(0x400, 0x10_9008));
        assert!(shown.background(0x1FF, 0x0D_9100));
        assert!(!shown.background(0x200, 0x0D_9100));
        // No BG Map16 hook: no table but the game's.
        assert!(!shown.background(0, 0x10_8008));
    }

    #[test]
    fn dumps_leave_out_the_stacks() {
        assert!(!withheld(RamMap::Vanilla, 0x7E_0100));
        assert!(withheld(RamMap::Vanilla, 0x7E_01FF));
        assert!(withheld(RamMap::Vanilla, 0x00_0110));
        assert!(!withheld(RamMap::Vanilla, 0x7F_01FF));
        assert!(!withheld(RamMap::Vanilla, 0x00_3700));
        assert!(withheld(RamMap::Sa1Pack, 0x00_3700));
        assert!(withheld(RamMap::Sa1Pack, 0x00_3182));
        assert!(!withheld(RamMap::Sa1Pack, 0x00_3186));
        assert!(!withheld(RamMap::Sa1Pack, 0x40_0110));
        let mut ram = Ram::new(RamMap::Vanilla);
        ram.fill(RamAddr::new(0x7E_0100), 0x100, 1);
        assert_eq!(bytes(&ram, RamAddr::new(0x7E_010E), 4), [1, 1, 0, 0]);
        assert_eq!(peek(&ram, 0x00_010F), 1);
        assert_eq!(peek(&ram, 0x00_0110), 0);
    }
}
