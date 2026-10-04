//! Map16 definitions as the loaded level's tilemap uploads resolve them.

use std::collections::HashMap;

use super::ExpandError;
use super::machine::{Call, LOOKUP_STEP_LIMIT, Machine};
use super::routines;
use super::tiles::{GRID_LEN, LM_TALL_SCREEN_LEN, PIPE_TILE_COUNT, PIPE_VARIANTS, SCREEN_LEN};
use crate::addr::SnesAddr;
use crate::cpu::Bus;
use crate::cpu::smw_bus::SmwBus;
use crate::map16::{self, Map16Tile};
use crate::ram;
use crate::rom::Rom;

/// Opcode of `JSL`, which Lunar Magic's hooks replace vanilla code with.
const JSL: u8 = 0x22;

/// Where the game reads BG Map16 tile definitions from for the loaded
/// level, and the background's bytes per screen: the vanilla table and
/// `$1B0`, or whatever the routine Lunar Magic hooked into the layer 2
/// tilemap upload leaves in `$0A`-`$0C` and `$05`-`$06`. Runs after the
/// loader so the routine sees the level's Lunar Magic flags.
fn bg_map16_base(machine: &mut Machine) -> Result<(u32, usize), ExpandError> {
    if machine.bus.read(routines::BG_MAP16_BASE_HOOK) != JSL {
        return Ok((map16::tables::MAP16_BG_TILES.raw(), SCREEN_LEN));
    }
    let target = machine.bus.read_u24(routines::BG_MAP16_BASE_HOOK + 1);
    // The caller has a 16-bit accumulator and 8-bit index registers.
    machine.call(
        Call::jsl(target)
            .wide_accumulator()
            .limit(LOOKUP_STEP_LIMIT),
    )?;
    let ram = &machine.bus.ram;
    // A slot the hack never gave a background (Grand Poo World 2 `09F`,
    // an unused level) gets a null pointer, and the upload reads its
    // definitions from wherever that points, as the picture shows:
    // work RAM at bank 0. So does this.
    let base = ram.u24(ram::BG_MAP16_BASE);
    let len = ram.u16(ram::BG_SCREEN_LEN) as usize;
    if len != SCREEN_LEN && len != LM_TALL_SCREEN_LEN {
        return Err(ExpandError::BackgroundLayout {
            level: machine.level,
            len,
        });
    }
    Ok((base, len))
}

/// Background Map16 definitions, including any Lunar Magic indices above
/// the vanilla 0x200 tiles, where they were read from, and the
/// background's bytes per screen.
pub(super) fn read_bg_map16(
    machine: &mut Machine,
    planes: &(Vec<u8>, Vec<u8>),
) -> Result<(Vec<Map16Tile>, u32, usize), ExpandError> {
    let (base, screen_len) = bg_map16_base(machine)?;
    let tile_count = planes
        .0
        .iter()
        .zip(&planes.1)
        .take(2 * screen_len)
        .map(|(&lo, &hi)| lo as usize + ((hi as usize) << 8) + 1)
        .max()
        .unwrap_or(0)
        .max(map16::BG_TILE_COUNT);
    let tiles = (0..tile_count as u32)
        .map(|n| read_map16(&mut machine.bus, base.wrapping_add(8 * n)))
        .collect();
    Ok((tiles, base, screen_len))
}

/// Reads a Map16 definition from ROM.
fn read_map16(bus: &mut SmwBus, ptr: u32) -> Map16Tile {
    Map16Tile::from_bytes(std::array::from_fn(|i| {
        bus.read(ptr.wrapping_add(i as u32))
    }))
}

/// Resolves the Map16 definition of every tile number present in the
/// loaded level's object grid, and of every other tile of the pages
/// those numbers are on and of the first [`FG_PAGES`] pages, the way the
/// level's tilemap upload does. Vanilla reads pages 0 and 1 through the
/// pointer table the loader built in RAM; Lunar Magic replaces that
/// lookup (`$058A65`) with a call to its foreground pointer routine for
/// every tile number, which is also the only way to reach pages 2 and up.
/// Background definitions stay separate: their tile numbers overlap
/// foreground pages 2 and 3. A tile the grid does not use and the
/// routine cannot resolve is left out rather than failing the level.
/// Returns the definitions and where each was read.
pub(super) fn lookup_map16(
    machine: &mut Machine,
    lunar_magic: bool,
) -> Result<(HashMap<u16, Map16Tile>, Sources), ExpandError> {
    let ram = &machine.bus.ram;
    let mut numbers: Vec<u16> = ram
        .bytes(ram::TILES_LOW, GRID_LEN)
        .into_iter()
        .zip(ram.bytes(ram::TILES_HIGH, GRID_LEN))
        .map(|(low, high)| u16::from_le_bytes([low, high]))
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    let mut pages: Vec<usize> = (0..FG_PAGES)
        .chain(numbers.iter().map(|&n| n as usize / PAGE_TILES))
        .collect();
    pages.sort_unstable();
    pages.dedup();
    let mut out = HashMap::with_capacity(pages.len() * PAGE_TILES);
    let mut sources = HashMap::with_capacity(pages.len() * PAGE_TILES);
    for n in numbers {
        if let Some((tile, at)) = lookup_one(machine, lunar_magic, n)? {
            out.insert(n, tile);
            sources.insert(n, at);
        }
    }
    for n in pages.into_iter().flat_map(|page| {
        let first = (page * PAGE_TILES) as u16;
        first..first + PAGE_TILES as u16
    }) {
        if out.contains_key(&n) {
            continue;
        }
        if let Ok(Some((tile, at))) = lookup_one(machine, lunar_magic, n) {
            out.insert(n, tile);
            sources.insert(n, at);
        }
    }
    Ok((out, sources))
}

/// Whether the first row upload calls the Map16 routine (`JSL $06F540` at
/// `$058A65`, over the game's `TAY : LDA Map16Pointers,Y`).
pub fn uploads_use_routine(rom: &Rom) -> bool {
    let jsl = [0x22, 0x40, 0xF5, 0x06];
    rom.read(SnesAddr::new(0x058A65), 4).is_ok_and(|b| b == jsl)
}

/// Whether the uploads go through the Map16 routine: in Lunar Magic's
/// ROMs, and in Kobo's builds that install it, which have no marker.
pub(super) fn routine_in_use(rom: &Rom) -> bool {
    rom.lunar_magic_version().is_some() || uploads_use_routine(rom)
}

/// Where each resolved foreground definition was read, by tile number.
pub(super) type Sources = HashMap<u16, u32>;

/// Tiles per Map16 page.
pub const PAGE_TILES: usize = 0x100;

/// Foreground pages resolved for every level, used or not: the vanilla
/// pages 0 and 1, and Lunar Magic's pages 2 and 3, which the numbers of
/// the background tiles overlap. Higher pages are resolved whole only
/// when the grid uses a tile on them.
pub const FG_PAGES: usize = 4;

/// The definition of one foreground tile number and where it was read,
/// or `None` where the ROM has no such tile (a vanilla ROM has pages 0
/// and 1 only).
pub(super) fn lookup_one(
    machine: &mut Machine,
    lunar_magic: bool,
    n: u16,
) -> Result<Option<(Map16Tile, u32)>, ExpandError> {
    Ok(match lookup_address(machine, lunar_magic, n)? {
        Some(ptr) => Some((read_map16(&mut machine.bus, ptr), ptr)),
        None => None,
    })
}

/// Where one foreground tile number's definition is, as [`lookup_one`]
/// finds it: a table's address, which the routine leaves in A and `$0C`.
pub(super) fn lookup_address(
    machine: &mut Machine,
    lunar_magic: bool,
    n: u16,
) -> Result<Option<u32>, ExpandError> {
    let ptr = if lunar_magic {
        machine.call(
            Call::jsl(routines::LM_MAP16_POINTER)
                .wide_accumulator()
                .wide_index()
                .accumulator(n.wrapping_mul(2))
                .limit(LOOKUP_STEP_LIMIT),
        )?;
        let bank = machine.bus.ram.u8(ram::LM_MAP16_BANK) as u32;
        let ptr = (bank << 16) | machine.cpu.a as u32;
        // A page whose table the ROM does not have: an empty table's
        // pointer leads outside the image.
        if n >= 0x200 && machine.bus.rom.read(SnesAddr::new(ptr), 8).is_err() {
            return Ok(None);
        }
        ptr
    } else if n < 0x200 {
        let pointer = machine.bus.ram.u16_at(ram::MAP16_POINTERS, n as u32);
        0x0D_0000 | pointer as u32
    } else {
        return Ok(None);
    };
    Ok(Some(ptr))
}

/// The four position-dependent definitions of each vertical pipe tile,
/// from `MAP16AppTable`.
pub(super) fn read_pipe_map16(bus: &mut SmwBus) -> [[Map16Tile; PIPE_TILE_COUNT]; PIPE_VARIANTS] {
    std::array::from_fn(|variant| {
        let entry = routines::PIPE_POINTER_TABLE + 2 * variant as u32;
        let base = 0x0D_0000 | bus.read(entry) as u32 | ((bus.read(entry + 1) as u32) << 8);
        std::array::from_fn(|tile| read_map16(bus, base + 8 * tile as u32))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{Mapping, SnesAddr};
    use crate::rom::Rom;

    #[test]
    fn lunar_magic_foreground_pages_two_and_three_use_pointer_routine() {
        let mut bytes = vec![0; 0x10_0000];
        let mut put = |addr: u32, data: &[u8]| {
            let at = Mapping::LoRom
                .snes_to_pc(SnesAddr::new(addr))
                .unwrap()
                .as_usize();
            bytes[at..at + data.len()].copy_from_slice(data);
        };
        put(0x00_FFD5, &[0x20]);
        // Synthetic pointer routine: input A = tile * 2, return
        // $10:(tile * 8 + $8000), with the bank in direct page $0C.
        put(
            routines::LM_MAP16_POINTER,
            &[
                0x0A, 0x0A, // ASL : ASL
                0x18, 0x69, 0x00, 0x80, // CLC : ADC #$8000
                0x48, 0xA9, 0x10, 0x00, // PHA : LDA #$0010
                0x85, 0x0C, 0x68, 0x6B, // STA $0C : PLA : RTL
            ],
        );
        let page_two = Map16Tile::from_bytes([1, 0, 2, 0, 3, 0, 4, 0]);
        let page_three = Map16Tile::from_bytes([5, 0, 6, 0, 7, 0, 8, 0]);
        put(0x10_9000, &page_two.to_bytes());
        put(0x10_9800, &page_three.to_bytes());
        let rom = Rom::from_bytes(bytes).unwrap();
        let mut machine = Machine::new(&rom, 0x105);
        machine.bus.ram.set_u8_at(ram::TILES_HIGH, 0, 2);
        machine.bus.ram.set_u8_at(ram::TILES_HIGH, 1, 3);
        let (result, sources) = lookup_map16(&mut machine, true).unwrap();
        assert_eq!(result[&0x200], page_two);
        assert_eq!(result[&0x300], page_three);
        assert_eq!(sources[&0x200], 0x10_9000);
        // Every tile of the first four pages is resolved, used or not,
        // and so is the rest of a higher page the grid touches.
        machine.bus.ram.set_u8_at(ram::TILES_HIGH, 2, 0x05);
        let (result, _) = lookup_map16(&mut machine, true).unwrap();
        assert!((0..(FG_PAGES * PAGE_TILES) as u16).all(|n| result.contains_key(&n)));
        assert_eq!(result[&0x3FF], Map16Tile::from_bytes([0; 8]));
        assert!((0x0500..0x0600).all(|n| result.contains_key(&n)));
        assert!(!result.contains_key(&0x0400));
        // A page whose table the ROM does not have: the routine's address
        // for page $10 wraps below $8000, outside the image.
        machine.bus.ram.set_u8_at(ram::TILES_HIGH, 2, 0x10);
        let (result, _) = lookup_map16(&mut machine, true).unwrap();
        assert!(!result.contains_key(&0x1000));
    }
}
