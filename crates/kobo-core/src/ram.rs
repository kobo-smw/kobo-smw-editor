//! The game's working memory, and where its variables live in it.
//!
//! A [`RamAddr`] names a variable by its address in the vanilla game
//! (`$7E0000`-`$7FFFFF`), the way the community's RAM maps do. A
//! [`RamMap`] turns it into the bus address the loaded ROM keeps that
//! variable at, and [`Ram`] holds the memory itself. This is the only
//! place that knows that mapping: SA-1 Pack moves most of the game's RAM
//! into the SA-1's I-RAM and BW-RAM and widens the sprite tables, so
//! nothing else may assume a variable sits in WRAM at its vanilla address.
//!
//! An SA-1 cartridge has no save RAM chip. It has BW-RAM, in banks
//! `$40`-`$4F` with an 8 KiB window on it at `$6000`-`$7FFF` of the
//! system banks, and the SA-1's 2 KiB of I-RAM at `$3000`-`$37FF`. The
//! SA-1 sees both as the S-CPU does, I-RAM again at `$0000`-`$07FF`, a
//! window of its own, a view of BW-RAM as 2- or 4-bit cells in banks
//! `$60`-`$6F`, and no work RAM at all.

use std::fmt;

use crate::addr::Mapping;
use crate::cpu::sa1::{Bitmap, Sa1};
use crate::rom::Rom;

pub const WRAM_LEN: usize = 0x2_0000;
pub const SRAM_LEN: usize = 0x8000;
pub const IRAM_LEN: usize = 0x800;
/// The most BW-RAM the SA-1 addresses; smaller chips mirror.
pub const BWRAM_LEN: usize = 0x4_0000;
/// Length of the BW-RAM window at `$6000`-`$7FFF`.
const BWRAM_BLOCK: usize = 0x2000;

/// A game variable, named by its vanilla address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct RamAddr(u32);

impl RamAddr {
    /// `addr` is a vanilla work RAM address, `$7E0000`-`$7FFFFF`.
    pub const fn new(addr: u32) -> Self {
        assert!(addr >= 0x7E_0000 && addr <= 0x7F_FFFF);
        Self(addr)
    }

    /// Parses a work RAM address, or `None` outside `$7E0000`-`$7FFFFF`.
    pub fn checked(addr: u32) -> Option<Self> {
        (0x7E_0000..=0x7F_FFFF)
            .contains(&addr)
            .then_some(Self(addr))
    }

    /// The vanilla address.
    pub const fn vanilla(self) -> u32 {
        self.0
    }
}

impl fmt::Display for RamAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:06X}", self.0)
    }
}

/// Where a ROM keeps the game's variables.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum RamMap {
    /// Everything in WRAM at its vanilla address. Lunar Magic and the
    /// usual tools leave this alone.
    #[default]
    Vanilla,
    /// SA-1 Pack: the SA-1 cannot reach work RAM, so everything its code
    /// touches is in I-RAM or BW-RAM, and the per-slot sprite tables are
    /// packed together with 22 slots each.
    Sa1Pack,
}

/// Length of a vanilla per-slot sprite table.
const VANILLA_SPRITE_SLOTS: u32 = 12;

/// SA-1 Pack's sprite tables (`more_sprites/sprite_tables.asm`): the
/// vanilla table and the bus address of the 22-slot one. Three direct
/// page tables moved out to make room, and two of those left behind
/// moved into the gaps.
const SA1_PACK_SPRITE_TABLES: [(u32, u32); 49] = [
    (0x7E_009E, 0x00_3200),
    (0x7E_00AA, 0x00_309E),
    (0x7E_00B6, 0x00_30B6),
    (0x7E_00C2, 0x00_30D8),
    (0x7E_00D8, 0x00_3216),
    (0x7E_00E4, 0x00_322C),
    (0x7E_14C8, 0x00_3242),
    (0x7E_14D4, 0x00_3258),
    (0x7E_14E0, 0x00_326E),
    (0x7E_14EC, 0x40_14C8),
    (0x7E_14F8, 0x40_14DE),
    (0x7E_1504, 0x40_14F4),
    (0x7E_1510, 0x40_150A),
    (0x7E_151C, 0x00_3284),
    (0x7E_1528, 0x00_329A),
    (0x7E_1534, 0x00_32B0),
    (0x7E_1540, 0x00_32C6),
    (0x7E_154C, 0x00_32DC),
    (0x7E_1558, 0x00_32F2),
    (0x7E_1564, 0x00_3308),
    (0x7E_1570, 0x00_331E),
    (0x7E_157C, 0x00_3334),
    (0x7E_1588, 0x00_334A),
    (0x7E_1594, 0x00_3360),
    (0x7E_15A0, 0x00_3376),
    (0x7E_15AC, 0x00_338C),
    (0x7E_15B8, 0x40_1520),
    (0x7E_15C4, 0x40_1536),
    (0x7E_15D0, 0x40_154C),
    (0x7E_15DC, 0x40_1562),
    (0x7E_15EA, 0x00_33A2),
    (0x7E_15F6, 0x00_33B8),
    (0x7E_1602, 0x00_33CE),
    (0x7E_160E, 0x00_33E4),
    (0x7E_161A, 0x40_1578),
    (0x7E_1626, 0x40_158E),
    (0x7E_1632, 0x40_15A4),
    (0x7E_163E, 0x00_33FA),
    (0x7E_164A, 0x40_15BA),
    (0x7E_1656, 0x40_15D0),
    (0x7E_1662, 0x40_15EA),
    (0x7E_166E, 0x40_1600),
    (0x7E_167A, 0x40_1616),
    (0x7E_1686, 0x40_162C),
    (0x7E_186C, 0x40_1642),
    (0x7E_187B, 0x00_3410),
    (0x7E_190F, 0x40_1658),
    (0x7E_1FD6, 0x40_166E),
    (0x7E_1FE2, 0x40_1FD6),
];

impl RamMap {
    /// The map a ROM uses. An SA-1 cartridge running SMW is SA-1 Pack:
    /// the game does not run on one without it.
    pub fn of(rom: &Rom) -> Self {
        match rom.mapping() {
            Mapping::LoRom => Self::Vanilla,
            Mapping::Sa1Rom | Mapping::BigSa1Rom => Self::Sa1Pack,
        }
    }

    /// Bus address of a variable. Tables are resolved by their first
    /// entry and indexed from there, since a map may widen them.
    pub fn resolve(self, addr: RamAddr) -> u32 {
        match self {
            Self::Vanilla => addr.0,
            Self::Sa1Pack => {
                let moved = SA1_PACK_SPRITE_TABLES
                    .iter()
                    .find(|(table, _)| (*table..table + VANILLA_SPRITE_SLOTS).contains(&addr.0));
                match (moved, addr.0) {
                    (Some((table, to)), a) => to + (a - table),
                    // The sprite loader's flags, 255 of them now.
                    (None, a @ 0x7E_1938..=0x7E_19B7) => 0x41_8A00 + (a - 0x7E_1938),
                    (None, a @ 0x7E_0000..=0x7E_00FF) => 0x00_3000 + (a & 0xFF),
                    (None, a @ 0x7E_0100..=0x7E_1FFF) => 0x40_0000 + (a & 0xFFFF),
                    (None, a @ 0x7E_C800..=0x7E_FFFF) => 0x40_0000 + (a & 0xFFFF),
                    (None, a @ 0x7F_C800..=0x7F_FFFF) => 0x41_0000 + (a & 0xFFFF),
                    // Wiggler segments.
                    (None, a @ 0x7F_9A7B..=0x7F_9C7A) => 0x41_8800 + (a - 0x7F_9A7B),
                    (None, a) => a,
                }
            }
        }
    }

    /// Sprite slots, the length of the per-slot sprite tables.
    pub fn sprite_slots(self) -> u32 {
        match self {
            Self::Vanilla => VANILLA_SPRITE_SLOTS,
            Self::Sa1Pack => 22,
        }
    }

    /// Length of the sprite loader's flag table at
    /// [`SPRITE_LOAD_STATUS`].
    pub fn sprite_load_flags(self) -> u32 {
        match self {
            Self::Vanilla => 0x80,
            Self::Sa1Pack => 0xFF,
        }
    }

    /// The direct page the game runs with: wherever `$7E0000` went.
    pub fn direct_page(self) -> u16 {
        self.resolve(RamAddr(0x7E_0000)) as u16
    }
}

/// Everything a routine can change apart from video memory: work RAM,
/// the cartridge's RAM, and on an SA-1 cartridge the SA-1 itself, which a
/// routine leaves changed as much as it does memory. Cloning it is a
/// snapshot of the game's state, and [`Clone::clone_from`] restores one.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Ram {
    map: RamMap,
    wram: Vec<u8>,
    /// Save RAM, or BW-RAM on an SA-1 cartridge.
    cart: Vec<u8>,
    /// Empty without an SA-1.
    iram: Vec<u8>,
    pub(crate) sa1: Option<Box<Sa1>>,
}

impl fmt::Debug for Ram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ram").field("map", &self.map).finish()
    }
}

impl Ram {
    /// Zeroed memory laid out by `map`.
    pub fn new(map: RamMap) -> Self {
        let sa1 = map == RamMap::Sa1Pack;
        Self {
            map,
            wram: vec![0; WRAM_LEN],
            cart: vec![0; if sa1 { BWRAM_LEN } else { SRAM_LEN }],
            iram: vec![0; if sa1 { IRAM_LEN } else { 0 }],
            sa1: sa1.then(Box::default),
        }
    }

    pub fn map(&self) -> RamMap {
        self.map
    }

    /// Reads a bus address as the S-CPU sees it, or `None` if it is not
    /// RAM: work RAM in banks `$7E`-`$7F` with its low 8 KiB mirrored in
    /// the system banks, and save RAM in banks `$70`-`$7D` or the SA-1's
    /// memories. Every CPU access comes through here or [`Ram::write`],
    /// so the decode is kept to one bank match.
    #[inline(always)]
    pub fn read(&self, addr: u32) -> Option<u8> {
        let off = addr as u16;
        match (addr >> 16) as u8 {
            0x7E | 0x7F => Some(self.wram[(addr - 0x7E_0000) as usize]),
            0x00..=0x3F | 0x80..=0xBF if off < 0x2000 => Some(self.wram[off as usize]),
            _ => match &self.sa1 {
                None => match (addr >> 16) as u8 {
                    0x70..=0x7D if off < 0x8000 => Some(self.cart[off as usize % SRAM_LEN]),
                    _ => None,
                },
                Some(sa1) => self.sa1_read(addr, sa1.bwram_window[0], false),
            },
        }
    }

    /// Writes a bus address as the S-CPU sees it; false if it is not RAM.
    #[inline(always)]
    pub fn write(&mut self, addr: u32, value: u8) -> bool {
        let off = addr as u16;
        let cell = match (addr >> 16) as u8 {
            0x7E | 0x7F => &mut self.wram[(addr - 0x7E_0000) as usize],
            0x00..=0x3F | 0x80..=0xBF if off < 0x2000 => &mut self.wram[off as usize],
            _ => match &self.sa1 {
                None => match (addr >> 16) as u8 {
                    0x70..=0x7D if off < 0x8000 => &mut self.cart[off as usize % SRAM_LEN],
                    _ => return false,
                },
                Some(sa1) => return self.sa1_write(addr, value, sa1.bwram_window[0], false),
            },
        };
        *cell = value;
        true
    }

    /// Reads a bus address as the SA-1 sees it.
    pub fn read_sa1(&self, addr: u32) -> Option<u8> {
        let window = self.sa1.as_ref()?.bwram_window[1];
        self.sa1_read(addr, window, true)
    }

    /// Writes a bus address as the SA-1 sees it.
    pub fn write_sa1(&mut self, addr: u32, value: u8) -> bool {
        match &self.sa1 {
            Some(sa1) => self.sa1_write(addr, value, sa1.bwram_window[1], true),
            None => false,
        }
    }

    /// Where a bus address lands in the SA-1's memories. `window` is the
    /// register that places `$6000`-`$7FFF` for the processor asking;
    /// only the SA-1's can select the bitmap view, and only the SA-1 has
    /// I-RAM in its first page and the bitmap banks.
    fn sa1_cell(&self, addr: u32, window: u8, from_sa1: bool) -> Option<Cell> {
        let off = addr as usize & 0xFFFF;
        match (addr >> 16) as u8 {
            0x00..=0x3F | 0x80..=0xBF => match off {
                0x0000..=0x07FF if from_sa1 => Some(Cell::Iram(off)),
                0x3000..=0x37FF => Some(Cell::Iram(off - 0x3000)),
                0x6000..=0x7FFF => {
                    let at = (window & 0x7F) as usize * BWRAM_BLOCK + (off - 0x6000);
                    Some(if from_sa1 && window & 0x80 != 0 {
                        Cell::Bitmap(at)
                    } else {
                        Cell::Bwram(at % BWRAM_LEN)
                    })
                }
                _ => None,
            },
            0x40..=0x4F => Some(Cell::Bwram(addr as usize % BWRAM_LEN)),
            0x60..=0x6F if from_sa1 => Some(Cell::Bitmap(addr as usize & 0xF_FFFF)),
            _ => None,
        }
    }

    fn bitmap(&self) -> Bitmap {
        self.sa1.as_ref().map(|sa1| sa1.bitmap).unwrap_or_default()
    }

    fn sa1_read(&self, addr: u32, window: u8, from_sa1: bool) -> Option<u8> {
        Some(match self.sa1_cell(addr, window, from_sa1)? {
            Cell::Iram(at) => self.iram[at],
            Cell::Bwram(at) => self.cart[at],
            Cell::Bitmap(at) => {
                let (byte, shift, mask) = self.bitmap().cell(at);
                (self.cart[byte % BWRAM_LEN] >> shift) & mask
            }
        })
    }

    fn sa1_write(&mut self, addr: u32, value: u8, window: u8, from_sa1: bool) -> bool {
        match self.sa1_cell(addr, window, from_sa1) {
            Some(Cell::Iram(at)) => self.iram[at] = value,
            Some(Cell::Bwram(at)) => self.cart[at] = value,
            Some(Cell::Bitmap(at)) => {
                let (byte, shift, mask) = self.bitmap().cell(at);
                let cell = &mut self.cart[byte % BWRAM_LEN];
                *cell = (*cell & !(mask << shift)) | ((value & mask) << shift);
            }
            None => return false,
        }
        true
    }

    /// Reads RAM at a bus address the ROM's own code names (a table a
    /// patch added, say). Panics if the address is not RAM.
    pub fn peek(&self, addr: u32) -> u8 {
        self.read(addr)
            .unwrap_or_else(|| panic!("${addr:06X} is not RAM"))
    }

    /// Writes RAM at a bus address. Panics if the address is not RAM.
    pub fn poke(&mut self, addr: u32, value: u8) {
        assert!(self.write(addr, value), "${addr:06X} is not RAM");
    }

    pub fn u8(&self, addr: RamAddr) -> u8 {
        self.peek(self.map.resolve(addr))
    }

    /// Entry `index` of the byte table starting at `table`.
    pub fn u8_at(&self, table: RamAddr, index: u32) -> u8 {
        self.peek(self.map.resolve(table) + index)
    }

    pub fn u16(&self, addr: RamAddr) -> u16 {
        u16::from_le_bytes([self.u8_at(addr, 0), self.u8_at(addr, 1)])
    }

    /// Entry `index` of the table of 16-bit words starting at `table`.
    pub fn u16_at(&self, table: RamAddr, index: u32) -> u16 {
        u16::from_le_bytes([
            self.u8_at(table, 2 * index),
            self.u8_at(table, 2 * index + 1),
        ])
    }

    pub fn u24(&self, addr: RamAddr) -> u32 {
        u32::from_le_bytes([
            self.u8_at(addr, 0),
            self.u8_at(addr, 1),
            self.u8_at(addr, 2),
            0,
        ])
    }

    /// `len` bytes starting at `addr`.
    pub fn bytes(&self, addr: RamAddr, len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| self.u8_at(addr, i)).collect()
    }

    pub fn set_u8(&mut self, addr: RamAddr, value: u8) {
        self.poke(self.map.resolve(addr), value);
    }

    pub fn set_u8_at(&mut self, table: RamAddr, index: u32, value: u8) {
        self.poke(self.map.resolve(table) + index, value);
    }

    pub fn set_u16(&mut self, addr: RamAddr, value: u16) {
        let [low, high] = value.to_le_bytes();
        self.set_u8_at(addr, 0, low);
        self.set_u8_at(addr, 1, high);
    }

    /// Sets `len` bytes starting at `addr` to `value`.
    pub fn fill(&mut self, addr: RamAddr, len: u32, value: u8) {
        for i in 0..len {
            self.set_u8_at(addr, i, value);
        }
    }
}

/// A byte of the SA-1's memories, or a cell of the bitmap view of BW-RAM.
enum Cell {
    Iram(usize),
    Bwram(usize),
    Bitmap(usize),
}

const fn ram(addr: u32) -> RamAddr {
    RamAddr::new(addr)
}

/// `$0100`: the game mode.
pub const GAME_MODE: RamAddr = ram(0x7E_0100);
/// `$010B`: the level loaded, which Lunar Magic's level number code (and
/// Kobo's, `level.asm`) keeps.
pub const LEVEL_NUMBER: RamAddr = ram(0x7E_010B);
/// `$0101`-`$0108`: the GFX files currently in VRAM. `$FF` forces uploads.
/// Where the game decompresses a GFX file to before uploading it.
pub const GFX_BUFFER: RamAddr = ram(0x7E_AD00);
pub const LOADED_GFX_FILES: RamAddr = ram(0x7E_0101);
/// Where the level's graphics list is, 3 bytes, which Lunar Magic's code
/// (and Kobo's, `graphics.asm`) leaves for every level when the ROM has
/// lists (docs/lunar-magic-install.md).
pub const LM_GRAPHICS_LIST: RamAddr = ram(0x7F_C006);
pub const LOADED_GFX_FILES_LEN: u32 = 8;
/// `$10`: zero once the game loop has finished a frame. The NMI handler
/// sets it, and skips its uploads if the loop had not got that far.
pub const LAG_FLAG: RamAddr = ram(0x7E_0010);
/// `$13`: the frame counter, including paused frames.
pub const TRUE_FRAME: RamAddr = ram(0x7E_0013);

/// `$141A`: non-zero while inside a level, so the header pointer
/// routine takes the screen-exit path instead of the overworld one.
pub const SUBLEVEL_COUNT: RamAddr = ram(0x7E_141A);
/// `$19B8`: screen exit table, level number low byte per screen.
pub const EXIT_TABLE_LOW: RamAddr = ram(0x7E_19B8);
/// `$19D8`: screen exit table, flags per screen. Vanilla stores the
/// exit's water bit here and never reads it back. In a Lunar Magic ROM an
/// entry with bit 2 set is in its exit format, the screen exit's flags
/// `0000wush` (the community's level format page; UberASM Tool's and
/// GPS's teleports write it): bit 0 is the level number's high byte, bit 1
/// selects a secondary exit, and bit 3 reaches `$192A` (`exit_probe`).
pub const EXIT_TABLE_HIGH: RamAddr = ram(0x7E_19D8);
/// `$1F11`: the player's submap, which vanilla turns into the level
/// number's high byte.
pub const OW_PLAYER_SUBMAP: RamAddr = ram(0x7E_1F11);
/// Whether the exit being taken leads to a secondary entrance
/// (`UseSecondaryExit`).
pub const USE_SECONDARY_EXIT: RamAddr = ram(0x7E_1B93);
pub const LAST_SCREEN_HORIZ: RamAddr = ram(0x7E_005E);
pub const SCREEN_MODE: RamAddr = ram(0x7E_005B);
pub const LEVEL_MODE: RamAddr = ram(0x7E_1925);
pub const SCREENS: RamAddr = ram(0x7E_005D);
/// `$13D7`: the level height in pixels. Vanilla leaves it zero;
/// Lunar Magic 3's loader hook (`JSL` at `$05D9A1`) stores the
/// height of the level's horizontal level mode here.
pub const LEVEL_HEIGHT: RamAddr = ram(0x7E_13D7);
/// `$1931`: the object tileset, as the loader stored it.
pub const OBJECT_TILESET: RamAddr = ram(0x7E_1931);
/// The Map16 tile grid: low bytes and high bytes, `0x3800` each.
pub const TILES_LOW: RamAddr = ram(0x7E_C800);
pub const TILES_HIGH: RamAddr = ram(0x7F_C800);
pub const LAYER2_TILEMAP_LOW: RamAddr = ram(0x7E_B900);
pub const LAYER2_TILEMAP_HIGH: RamAddr = ram(0x7E_BD00);
pub const BACKGROUND_COLOR: RamAddr = ram(0x7E_0701);
/// 0x200 two-byte pointers into bank `$0D`, built by the level loader.
pub const MAP16_POINTERS: RamAddr = ram(0x7E_0FBE);
/// Direct page `$0C`: the bank the `$06F540` hook's target leaves there.
pub const LM_MAP16_BANK: RamAddr = ram(0x7E_000C);
/// Direct page `$0A`-`$0C`: the BG Map16 table pointer the initial
/// layer 2 tilemap upload reads tile definitions through.
pub const BG_MAP16_BASE: RamAddr = ram(0x7E_000A);
/// Direct page `$05`-`$06`: bytes per screen of the background buffer,
/// as set by Lunar Magic's hook (vanilla hard-codes `$1B0`).
pub const BG_SCREEN_LEN: RamAddr = ram(0x7E_0005);
/// Direct page `$CE`-`$D0`: the level's sprite data pointer.
pub const SPRITE_DATA_PTR: RamAddr = ram(0x7E_00CE);

/// `$1A`/`$1C`: layer 1 position, and `$1462`/`$1464`: the position
/// the camera update copies from at the start of each frame.
pub const LAYER1_X: RamAddr = ram(0x7E_001A);
pub const LAYER1_Y: RamAddr = ram(0x7E_001C);
/// `$1E`/`$20`: layer 2 position. Game mode `$11` copies all eight
/// bytes of `$1A`-`$21` to `$1462`-`$1469` after resolving the level
/// header, seeding the camera update.
pub const LAYER2_X: RamAddr = ram(0x7E_001E);
pub const LAYER2_Y: RamAddr = ram(0x7E_0020);
pub const LAYER_POSITIONS_LEN: u32 = 8;
pub const NEXT_LAYER1_X: RamAddr = ram(0x7E_1462);
pub const NEXT_LAYER1_Y: RamAddr = ram(0x7E_1464);
/// `$22`/`$24`: layer 3 position, as the IRQ handler writes it to
/// `BG3HOFS`/`BG3VOFS` below the status bar.
pub const LAYER3_X: RamAddr = ram(0x7E_0022);
pub const LAYER3_Y: RamAddr = ram(0x7E_0024);
/// `$17BD`/`$17BC`: how far layer 1 moved this frame, as the camera
/// update leaves it for the layer scroll routine.
pub const LAYER1_DX: RamAddr = ram(0x7E_17BD);
pub const LAYER1_DY: RamAddr = ram(0x7E_17BC);
/// `$55`: layer 1 scroll direction, which the sprite loader turns into
/// an offset from the camera to the column it loads (1: none).
pub const LAYER1_SCROLL_DIR: RamAddr = ram(0x7E_0055);
/// `$1411`/`$1412`: horizontal and vertical camera scroll settings;
/// zero freezes the camera.
pub const HORIZ_SCROLL_SETTING: RamAddr = ram(0x7E_1411);
pub const VERT_SCROLL_SETTING: RamAddr = ram(0x7E_1412);
/// `$1404`: vertical scrolling at will: the camera follows the player up
/// and down without waiting for him to land. Game mode `$11` sets it for
/// the first camera update, which clears it again once the camera is
/// where it wants to be.
pub const SCROLL_AT_WILL: RamAddr = ram(0x7E_1404);
/// `$143E`/`$143F`: the layer 1 and layer 2 scroll commands a scroll
/// sprite (`E7`-`F5`) installed; zero when the level has none.
/// Autoscroll commands drive the camera every frame.
pub const LAYER1_SCROLL_CMD: RamAddr = ram(0x7E_143E);
pub const LAYER2_SCROLL_CMD: RamAddr = ram(0x7E_143F);

/// `$94`/`$96`: the player's position for the next frame.
pub const PLAYER_X: RamAddr = ram(0x7E_0094);
pub const PLAYER_Y: RamAddr = ram(0x7E_0096);
/// `$7B`/`$7D`: the player's speed.
pub const PLAYER_X_SPEED: RamAddr = ram(0x7E_007B);
pub const PLAYER_Y_SPEED: RamAddr = ram(0x7E_007D);
/// `$71`: the player's animation state; non-zero while an entrance
/// action (pipe, cannon pipe, door) is still playing.
pub const PLAYER_ANIMATION: RamAddr = ram(0x7E_0071);
/// `$185C`: non-zero skips the player's interaction with tiles.
pub const PLAYER_NO_TILE_INTERACTION: RamAddr = ram(0x7E_185C);
/// `$9D`: sprite lock, which also pauses layer 3 autoscroll.
pub const SPRITE_LOCK: RamAddr = ram(0x7E_009D);

/// `$0200`-`$03FF`: the OAM image, four bytes per object, followed at
/// `$0400`-`$041F` by the size and X-high bits packed four objects to a
/// byte. `$3F` is the OAM address the first object was written at.
pub const OAM: RamAddr = ram(0x7E_0200);
pub const OAM_ADDRESS: RamAddr = ram(0x7E_003F);
/// `$0420`-`$049F`: the size and X-high bits one object to a byte, as
/// the drawing routines write them. The end of each frame packs them
/// into the OAM image.
pub const OAM_SIZES: RamAddr = ram(0x7E_0420);
/// `$14C8`: sprite slot status (0 = free), one byte per slot.
pub const SPRITE_STATUS: RamAddr = ram(0x7E_14C8);
/// `$9E`, `$E4`/`$14E0`, `$D8`/`$14D4`: sprite number and position
/// tables, one byte per slot.
pub const SPRITE_NUMBER: RamAddr = ram(0x7E_009E);
pub const SPRITE_X_LOW: RamAddr = ram(0x7E_00E4);
pub const SPRITE_X_HIGH: RamAddr = ram(0x7E_14E0);
pub const SPRITE_Y_LOW: RamAddr = ram(0x7E_00D8);
pub const SPRITE_Y_HIGH: RamAddr = ram(0x7E_14D4);
/// `$1938`: the per-entry "already loaded" flags the level sprite
/// loader keeps, [`RamMap::sprite_load_flags`] of them.
pub const SPRITE_LOAD_STATUS: RamAddr = ram(0x7E_1938);
/// `$18B9`: the active sprite generator, which keeps spawning sprites
/// after its own sprite slot is cleared.
pub const SPRITE_GENERATOR: RamAddr = ram(0x7E_18B9);
/// `$1892`: cluster sprite numbers (0 = free), `$1E16`/`$1E02`: the
/// low bytes of their positions. 20 slots.
pub const CLUSTER_NUMBER: RamAddr = ram(0x7E_1892);
pub const CLUSTER_X_LOW: RamAddr = ram(0x7E_1E16);
pub const CLUSTER_Y_LOW: RamAddr = ram(0x7E_1E02);
pub const CLUSTER_SLOTS: u32 = 20;

/// `$3E`: `BGMODE` mirror; `$40`: `CGADSUB` mirror; `$44`: `CGWSEL`
/// mirror; `$0D9D`/`$0D9E`: main and sub screen designation mirrors.
pub const BG_MODE: RamAddr = ram(0x7E_003E);
pub const COLOR_MATH: RamAddr = ram(0x7E_0040);
pub const COLOR_MATH_SELECT: RamAddr = ram(0x7E_0044);
pub const MAIN_SCREEN: RamAddr = ram(0x7E_0D9D);
pub const SUB_SCREEN: RamAddr = ram(0x7E_0D9E);
/// `$0D9B`: which NMI and IRQ code runs. Bit 7 marks a Mode 7 boss
/// arena, bit 6 one that uploads boss tiles, bit 0 one without the
/// ceiling and floor IRQs.
pub const IRQ_NMI_COMMAND: RamAddr = ram(0x7E_0D9B);
/// `$04A0`: window 1's left and right edges per scanline, as the HDMA
/// feeds them to the PPU.
pub const WINDOW_TABLE: RamAddr = ram(0x7E_04A0);
/// `$41`-`$43`: `W12SEL`, `W34SEL`, and `WOBJSEL` mirrors.
pub const WINDOW_SELECT: RamAddr = ram(0x7E_0041);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_resolve_through_the_map() {
        let mut ram = Ram::new(RamMap::Vanilla);
        ram.set_u16(LAYER1_X, 0x1234);
        assert_eq!(ram.peek(0x7E_001A), 0x34);
        assert_eq!(ram.peek(0x00_001B), 0x12); // low WRAM mirror
        assert_eq!(ram.u16(LAYER1_X), 0x1234);
        ram.set_u8_at(SPRITE_STATUS, 11, 8);
        assert_eq!(ram.u8(RamAddr::new(0x7E_14D3)), 8);
        ram.fill(TILES_HIGH, 3, 0xAB);
        assert_eq!(ram.bytes(TILES_HIGH, 4), [0xAB, 0xAB, 0xAB, 0]);
        assert_eq!(ram.u24(TILES_HIGH), 0xAB_ABAB);
    }

    #[test]
    fn only_ram_answers_as_ram() {
        let mut ram = Ram::new(RamMap::Vanilla);
        assert_eq!(ram.read(0x00_8000), None);
        assert_eq!(ram.read(0x00_2100), None);
        assert!(!ram.write(0x05_8000, 1));
        assert!(ram.write(0x70_0010, 7));
        assert_eq!(ram.read(0x71_0010), Some(7)); // one SRAM chip, mirrored
        assert_eq!(RamAddr::checked(0x80_0000), None);
        assert_eq!(RamAddr::checked(0x7F_FFFF).unwrap().to_string(), "$7FFFFF");
    }

    #[test]
    fn sa1_pack_moves_what_the_sa1_touches() {
        let map = RamMap::Sa1Pack;
        let at = |addr| map.resolve(RamAddr::new(addr));
        assert_eq!(at(0x7E_0013), 0x00_3013); // direct page: I-RAM
        assert_eq!(map.direct_page(), 0x3000);
        assert_eq!(at(0x7E_0100), 0x40_0100); // the rest of low RAM: BW-RAM
        assert_eq!(at(0x7E_1FFF), 0x40_1FFF);
        assert_eq!(at(0x7E_C800), 0x40_C800); // the tile grid
        assert_eq!(at(0x7F_C800), 0x41_C800);
        assert_eq!(at(0x7F_9A7B), 0x41_8800); // Wiggler segments
        assert_eq!(at(0x7E_1938), 0x41_8A00); // sprite load flags
        assert_eq!(at(0x7E_2000), 0x7E_2000); // the S-CPU's own stays put
        assert_eq!(at(0x7F_0000), 0x7F_0000);
        // Sprite tables are packed together, 22 slots each, and those
        // left on the direct page moved up into the gaps.
        assert_eq!(map.sprite_slots(), 22);
        assert_eq!(at(0x7E_009E), 0x00_3200);
        assert_eq!(at(0x7E_14C8), 0x00_3242);
        assert_eq!(at(0x7E_14D3), 0x00_324D);
        assert_eq!(at(0x7E_00AA), 0x00_309E);
        assert_eq!(at(0x7E_00C2), 0x00_30D8);
        assert_eq!(at(0x7E_14EC), 0x40_14C8);
        assert_eq!(at(0x7E_00CE), 0x00_30CE); // not a table: the data pointer
        let mut ram = Ram::new(map);
        ram.set_u8_at(SPRITE_STATUS, 21, 8);
        assert_eq!(ram.peek(0x00_3242 + 21), 8);
        assert_eq!(ram.peek(0x00_3258), 0); // and not into the next table
        // Every table has room for its 22 slots before the next begins.
        let mut tables: Vec<u32> = SA1_PACK_SPRITE_TABLES.iter().map(|t| t.1).collect();
        tables.sort_unstable();
        assert!(tables.windows(2).all(|w| w[1] - w[0] >= 22), "{tables:X?}");
    }

    #[test]
    fn an_sa1_cartridge_has_its_own_memories() {
        let mut ram = Ram::new(RamMap::Sa1Pack);
        assert!(ram.write(0x00_3000, 1));
        assert_eq!(ram.read(0x80_3000), Some(1));
        assert_eq!(ram.read_sa1(0x00_0000), Some(1)); // I-RAM, mirrored down
        assert_eq!(ram.read(0x00_0000), Some(0)); // where the S-CPU has work RAM
        assert_eq!(ram.read(0x00_3800), None);
        assert_eq!(ram.read_sa1(0x7E_0000), None); // the SA-1 has no work RAM
        assert_eq!(ram.read(0x70_0000), None); // and the cartridge no save RAM

        // BW-RAM, and each processor's window on it.
        assert!(ram.write(0x41_2005, 7));
        assert_eq!(ram.read(0x45_2005), Some(7)); // mirrored through `$4F`
        assert_eq!(ram.read(0x00_6005), Some(0));
        ram.sa1.as_mut().unwrap().bwram_window = [9, 0]; // block 9 is `$412000`
        assert_eq!(ram.read(0x00_6005), Some(7));
        assert_eq!(ram.read_sa1(0x00_6005), Some(0));

        // The SA-1 alone can address BW-RAM in cells.
        assert!(ram.write_sa1(0x40_0000, 0xA5));
        assert_eq!(ram.read_sa1(0x60_0000), Some(0x5));
        assert_eq!(ram.read_sa1(0x60_0001), Some(0xA));
        assert!(ram.write_sa1(0x60_0001, 0xFC)); // only four bits land
        assert_eq!(ram.read_sa1(0x40_0000), Some(0xC5));
        assert_eq!(ram.read(0x60_0000), None);
        ram.sa1.as_mut().unwrap().bitmap = Bitmap::TwoBits;
        assert_eq!(ram.read_sa1(0x60_0003), Some(0x3)); // `$C5`: 01 01 00 11
        ram.sa1.as_mut().unwrap().bwram_window[1] = 0x80; // the window, in cells
        assert_eq!(ram.read_sa1(0x00_6003), Some(0x3));
    }

    #[test]
    fn a_clone_is_a_snapshot() {
        let mut ram = Ram::new(RamMap::Vanilla);
        ram.set_u8(GAME_MODE, 0x11);
        let saved = ram.clone();
        ram.set_u8(GAME_MODE, 0x14);
        ram.clone_from(&saved);
        assert_eq!(ram.u8(GAME_MODE), 0x11);
    }
}
