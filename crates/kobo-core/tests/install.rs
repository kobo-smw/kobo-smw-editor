//! Kobo's clean-room code for Lunar Magic's layout (`kobo_core::install`),
//! checked by running it: the ROM's own loader, Map16 routine, and block
//! interaction on the vanilla ROM with the patches applied, and on it with
//! SA-1 Pack applied too when SA-1 Pack is configured. Needs Asar's
//! library and the vanilla ROM. That custom blocks run as under Lunar
//! Magic's code is checked by hand with `examples/contact_probe.rs`
//! (docs/testing.md).

mod common;

use kobo_core::map16::Map16Tile;
use kobo_core::rats::{Contents, FreeSpace};
use kobo_core::render::{self, RenderOptions, Sprites};
use kobo_core::{Rom, SnesAddr, expand, install, level};

/// Each base the patches go on, and the same with them: the vanilla ROM
/// expanded to 1 MiB, and with SA-1 Pack applied (if it is configured).
fn installs(clean: &Rom) -> Vec<(Rom, Rom)> {
    let Some(asar) = common::asar() else {
        return Vec::new();
    };
    let mut lorom = Rom::from_bytes(clean.data().to_vec()).unwrap();
    lorom.expand(0x10_0000).unwrap();
    std::iter::once(lorom)
        .chain(common::sa1_base(clean))
        .map(|base| {
            let rom = install::apply_lunar_magic(&asar, &base).unwrap();
            (base, rom)
        })
        .collect()
}

/// Where the log patches below keep their log: work RAM the game does not
/// use, or on an SA-1 ROM, whose SA-1 runs the actions, BW-RAM SA-1 Pack
/// does not use.
fn log_address(rom: &Rom) -> u32 {
    if rom.mapping().is_sa1() {
        0x41_B40F
    } else {
        0x7F_B40F
    }
}

/// A definition made up from its tile number, to tell tiles apart.
fn definition(tile: u16) -> [u8; 8] {
    let [lo, hi] = tile.to_le_bytes();
    [lo, hi, 0x11, 0x22, lo ^ 0xFF, hi, 0x33, 0x44]
}

#[test]
fn pages_0_and_1_are_the_games() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (base, rom) in installs(&clean) {
        let tiles: Vec<u16> = (0..0x200).collect();
        for level in [0x105, 0x0D3, 0x0C7] {
            assert_eq!(
                expand::resolve_map16(&rom, level, &tiles).unwrap(),
                expand::resolve_map16(&base, level, &tiles).unwrap(),
                "level {level:03X}"
            );
        }
        // With no page tables written, there are no tiles past page 1.
        let past = expand::resolve_map16(&rom, 0x105, &[0x200, 0x3FF, 0x7FFF]).unwrap();
        assert_eq!(past, [None, None, None]);
    }
}

#[test]
fn the_patches_find_variables_where_the_library_does() {
    use kobo_core::ram::{RamAddr, RamMap};
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    // Each variable as the patches write it, and its vanilla address.
    let names = [
        ("$000E|!dp", 0x7E_000E),
        ("$0100|!addr", 0x7E_0100),
        ("$010B|!addr", 0x7E_010B),
        ("$1925|!addr", 0x7E_1925),
        ("$1FFF|!addr", 0x7E_1FFF),
        ("!map16_low", 0x7E_C800),
        ("!map16_high", 0x7F_C800),
        ("!sprite_load_flags", 0x7E_1938),
        ("!9E", 0x7E_009E),
        ("!E4", 0x7E_00E4),
        ("!14E0", 0x7E_14E0),
        ("!D8", 0x7E_00D8),
        ("!14D4", 0x7E_14D4),
    ];
    let probe: String = names
        .iter()
        .map(|(name, _)| format!("print hex({name})\n"))
        .chain(["print hex(!load_flags)\n".to_string()])
        .collect();
    let probe = format!("incsrc \"memory.asm\"\n{probe}");
    for sa1 in [false, true] {
        let mut rom = Rom::from_bytes(clean.data().to_vec()).unwrap();
        if sa1 {
            rom.write_u8(SnesAddr::new(0x00FFD5), 0x23).unwrap();
        }
        let rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
        let map = RamMap::of(&rom);
        let prints = asar
            .patch(&rom, &install::patch(("probe.asm", &probe)))
            .unwrap()
            .output
            .prints;
        // Where the S-CPU's address leads: work RAM's first 8 KiB in bank
        // $00, or SA-1 Pack's I-RAM and BW-RAM window.
        let bus = |addr: u32| match (sa1, addr >> 16, addr & 0xFFFF) {
            (false, 0x00, a @ ..0x2000) => 0x7E_0000 | a,
            (true, 0x00, a @ 0x6000..0x8000) => 0x40_0000 + a - 0x6000,
            _ => addr,
        };
        for ((name, vanilla), print) in names.iter().zip(&prints) {
            let addr = u32::from_str_radix(print, 16).unwrap();
            assert_eq!(
                bus(addr),
                map.resolve(RamAddr::new(*vanilla)),
                "{name}, SA-1: {sa1}"
            );
        }
        let flags = u32::from_str_radix(&prints[names.len()], 16).unwrap();
        assert_eq!(flags, map.sprite_load_flags(), "SA-1: {sa1}");
    }
}

#[test]
fn vanilla_levels_draw_the_same() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (base, rom) in installs(&clean) {
        // The row and column uploads of both layers, horizontal and
        // vertical, a background, and layer 2 objects, all through the
        // routine.
        for level in [0x105, 0x0D3, 0x01C, 0x0CB, 0x0E5, 0x0C7] {
            let options = RenderOptions {
                sprites: Sprites::Markers,
                player: false,
            };
            let a = render::render_level(&base, level, options).unwrap().image;
            let b = render::render_level(&rom, level, options).unwrap().image;
            assert!(a.pixels == b.pixels, "level {level:03X}");
        }
        // Sprites that change tiles while they run, through the tile change
        // and tile generation code; boss arenas, whose floors the player
        // samples with high bytes past 1, which the acts-like chain must
        // keep solid.
        for level in [0x006, 0x0C3, 0x095, 0x1C7] {
            let options = RenderOptions::default();
            let a = render::render_level(&base, level, options).unwrap().image;
            let b = render::render_level(&rom, level, options).unwrap().image;
            assert!(a.pixels == b.pixels, "level {level:03X} with sprites");
        }
    }
}

#[test]
fn pages_past_1_come_from_the_page_tables() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (_, rom) in installs(&clean) {
        page_tables(rom);
    }
}

fn page_tables(mut rom: Rom) {
    let mut space = FreeSpace::scan(&rom);
    // Each group's table pointer and bank, where Lunar Magic's layout has
    // them, and whether the pointer is kept less one. A tile is at the
    // pointer plus its number times 8, kept to 16 bits.
    let groups: [(u16, u32, u32, bool); 5] = [
        (0x0200, 0x06F553, 0x06F557, false),
        (0x1000, 0x06F55C, 0x06F560, false),
        (0x2000, 0x06F567, 0x06F56B, true),
        (0x4000, 0x06F594, 0x06F598, false),
        (0x7000, 0x06F5B1, 0x06F5B5, true),
    ];
    let mut expected = Vec::new();
    for (first, pointer, bank, less_one) in groups {
        let tiles = [first, first + 1, first + 0x1FF];
        let len = (tiles[2] - first + 1) as usize * 8;
        let table = space.alloc(&mut rom, len, Contents::Data).unwrap();
        for &tile in &tiles {
            let at = table.add((tile - first) as u32 * 8);
            rom.write(at, &definition(tile)).unwrap();
            expected.push((tile, definition(tile)));
        }
        let index = first.wrapping_mul(8);
        let stored = table.offset().wrapping_sub(index + less_one as u16);
        rom.write_u16(SnesAddr::new(pointer), stored).unwrap();
        rom.write_u8(SnesAddr::new(bank), table.bank()).unwrap();
    }
    let tiles: Vec<u16> = expected.iter().map(|(t, _)| *t).collect();
    let resolved = expand::resolve_map16(&rom, 0x105, &tiles).unwrap();
    for ((tile, bytes), got) in expected.iter().zip(resolved) {
        assert_eq!(got, Some(Map16Tile::from_bytes(*bytes)), "tile {tile:04X}");
    }

    // Page 2 per object tileset: the table at the pointer plus $1000,
    // $800 bytes a tileset.
    let tileset = level::read_primary_header(&rom, 0x105)
        .unwrap()
        .object_tileset as usize;
    let table = space
        .alloc(&mut rom, 0x1000 + 0x800 * (tileset + 1), Contents::Data)
        .unwrap();
    let tile = 0x2A5;
    let at = table.add((0x1000 + 0x800 * tileset + 0xA5 * 8) as u32);
    rom.write(at, &definition(0x1234)).unwrap();
    rom.write_u16(SnesAddr::new(0x06F586), table.offset())
        .unwrap();
    rom.write_u8(SnesAddr::new(0x06F58A), table.bank()).unwrap();
    rom.write_u8(SnesAddr::new(0x06F547), 1).unwrap();
    let resolved = expand::resolve_map16(&rom, 0x105, &[tile, 0x3FF]).unwrap();
    assert_eq!(resolved[0], Some(Map16Tile::from_bytes(definition(0x1234))));
    // Page 3 still comes from the group's table.
    assert_eq!(resolved[1], Some(Map16Tile::from_bytes(definition(0x3FF))));
}

/// The actions Kobo's chain runs, logged by a routine in the help file's
/// slots: a sprite landing on tile `$200` and a shell hitting its side, in
/// a horizontal and in a vertical level, whose sprite block check reaches
/// the chain through one more call.
#[test]
fn sprite_actions_run_in_both_orientations() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    for (_, rom) in installs(&clean) {
        sprite_actions(&asar, &rom);
    }
}

fn sprite_actions(asar: &kobo_core::asar::Asar, rom: &Rom) {
    use kobo_core::ram::{self, RamAddr};
    // Each slot's first JSL logs its action's number, 3 above or below and
    // 4 the side, for a tile the chain looked up from $200 on ($03), as
    // the slots run for every tile. !log counts, the numbers follow.
    let count = log_address(rom);
    let log = format!(
        "incsrc \"memory.asm\"
!log = ${count:06X}
org $06F920 : JSL log_v
org $06F930 : JSL log_h
freecode
log_v: LDA #$03 : BRA log
log_h: LDA #$04
log: PHA : LDA $04 : CMP #$02 : PLA : BCC +
    PHX : PHA : LDA !log : TAX : PLA : STA !log+1,x
    INX : TXA : STA !log : PLX
+ RTL
"
    );
    let rom = asar
        .patch(rom, &install::patch(("log.asm", &log)))
        .unwrap()
        .rom;
    for level in [0x105, 0x1CE] {
        let vertical = level::read_primary_header(&rom, level)
            .unwrap()
            .level_mode
            .layer1_vertical();
        assert_eq!(vertical, level == 0x1CE);
        let start = expand::play_level(&rom, level, 0, |_, _| {}).unwrap();
        let (px, py) = (
            start.u16(ram::PLAYER_X) as usize,
            start.u16(ram::PLAYER_Y) as usize,
        );
        // Tile $200 three tiles right of the player, level with its head.
        let (tx, ty) = (px / 16 + 3, py / 16);
        let offset = if vertical {
            (ty / 16) * 0x200 + (tx / 16) * 0x100 + (ty % 16) * 16 + tx % 16
        } else {
            (tx / 16) * 0x1B0 + ty * 16 + tx % 16
        };
        let (bx, by) = ((tx * 16) as i32, (ty * 16) as i32);
        let sprites: [(u8, u8, i32, i32, i8, i8, u8); 2] = [
            // A Goomba dropped onto it, a kicked shell into its left side.
            (0x0F, 0x01, bx, by - 20, 0, 0x10, 3),
            (0x04, 0x0A, bx - 20, by, 0x30, 0, 4),
        ];
        for (number, status, x, y, x_speed, y_speed, action) in sprites {
            let ram = expand::play_level(&rom, level, 16, |frame, ram| {
                if frame == 0 {
                    ram.set_u8(RamAddr::new(0x7E_C800 + offset as u32), 0x00);
                    ram.set_u8(RamAddr::new(0x7F_C800 + offset as u32), 0x02);
                    ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                    ram.set_u16(ram::PLAYER_X, (bx - 0x60) as u16);
                    ram.set_u8_at(ram::SPRITE_NUMBER, 0, number);
                    ram.set_u8_at(ram::SPRITE_STATUS, 0, status);
                    ram.set_u8_at(ram::SPRITE_X_LOW, 0, x as u8);
                    ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (x >> 8) as u8);
                    ram.set_u8_at(ram::SPRITE_Y_LOW, 0, y as u8);
                    ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, (y >> 8) as u8);
                    ram.set_u8_at(RamAddr::new(0x7E_00B6), 0, x_speed as u8);
                    ram.set_u8_at(RamAddr::new(0x7E_00AA), 0, y_speed as u8);
                    ram.poke(count, 0);
                }
            })
            .unwrap();
            let logged: Vec<u8> = (0..ram.peek(count) as u32)
                .map(|i| ram.peek(count + 1 + i))
                .collect();
            assert!(
                logged.contains(&action),
                "level {level:03X}, sprite {number:02X}: {logged:?}"
            );
        }
    }
}

/// A long chain of acts-like entries resolves to its end, and only one
/// that loops falls back to cement: what a sprite's action is told, logged
/// from the slot.
#[test]
fn long_acts_like_chains_resolve_and_loops_do_not() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    for (_, rom) in installs(&clean) {
        acts_like_chains(&asar, &rom);
    }
}

fn acts_like_chains(asar: &kobo_core::asar::Asar, rom: &Rom) {
    use kobo_core::ram::{self, RamAddr};
    // The side and above-or-below slots log the tile reported, Y and
    // $1693, for a tile the chain looked up from $200 on ($03).
    let count = log_address(rom);
    let log = format!(
        "incsrc \"memory.asm\"
!log = ${count:06X}
org $06F920 : JSL log
org $06F930 : JSL log
freecode
log: LDA $04 : CMP #$02 : BCC +
    PHX : LDA !log : TAX : TYA : STA !log+1,x : LDA $1693|!addr : STA !log+2,x
    INX : INX : TXA : STA !log : PLX
+ RTL
"
    );
    let rom = asar
        .patch(rom, &install::patch(("log.asm", &log)))
        .unwrap()
        .rom;
    let table = rom.read_ptr(SnesAddr::new(0x06F624)).unwrap();
    let reported = |links: &[(u16, u16)]| {
        let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
        for &(tile, acts_like) in links {
            rom.write_u16(table.add(2 * tile as u32), acts_like)
                .unwrap();
        }
        let start = expand::play_level(&rom, 0x105, 0, |_, _| {}).unwrap();
        let (tx, ty) = (
            start.u16(ram::PLAYER_X) as usize / 16 + 8,
            start.u16(ram::PLAYER_Y) as usize / 16,
        );
        let offset = (tx / 16) * 0x1B0 + ty * 16 + tx % 16;
        let (bx, by) = ((tx * 16) as u16, (ty * 16) as u16);
        let ram = expand::play_level(&rom, 0x105, 16, |frame, ram| {
            if frame == 0 {
                ram.set_u8(RamAddr::new(0x7E_C800 + offset as u32), 0x00);
                ram.set_u8(RamAddr::new(0x7F_C800 + offset as u32), 0x02);
                ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                ram.set_u16(ram::PLAYER_X, bx - 0x60);
                ram.set_u8_at(ram::SPRITE_NUMBER, 0, 0x0F);
                ram.set_u8_at(ram::SPRITE_STATUS, 0, 0x01);
                ram.set_u8_at(ram::SPRITE_X_LOW, 0, bx as u8);
                ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (bx >> 8) as u8);
                ram.set_u8_at(ram::SPRITE_Y_LOW, 0, (by - 20) as u8);
                ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, ((by - 20) >> 8) as u8);
                ram.set_u8_at(RamAddr::new(0x7E_00AA), 0, 0x10);
                ram.poke(count, 0);
            }
        })
        .unwrap();
        let n = ram.peek(count) as u32;
        assert!(n >= 2, "no action ran");
        let at = |k| ram.peek(count + 1 + k) as u16;
        at(0) << 8 | at(1)
    };
    // $200 -> $201 -> ... -> $210 -> $025, seventeen lookups.
    let mut chain: Vec<(u16, u16)> = (0x200..0x210).map(|t| (t, t + 1)).collect();
    chain.push((0x210, 0x025));
    assert_eq!(reported(&chain), 0x025);
    // $200 -> $201 -> $200.
    assert_eq!(reported(&[(0x200, 0x201), (0x201, 0x200)]), 0x130);
}

/// In the Mode 7 boss battles the chain passes the tile through untouched,
/// as the game's floors there are sampled with high bytes past 1: what the
/// player's and sprites' contacts leave in `$1693` is vanilla's.
#[test]
fn boss_battles_pass_tiles_through() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (base, rom) in installs(&clean) {
        boss_battles(&base, &rom);
    }
}

fn boss_battles(base: &Rom, rom: &Rom) {
    use kobo_core::ram::RamAddr;
    for level in [0x095, 0x1C7] {
        let mut seen = [Vec::new(), Vec::new()];
        for (rom, seen) in [base, rom].into_iter().zip(&mut seen) {
            expand::play_level(rom, level, 60, |_, ram| {
                seen.push(ram.u8(RamAddr::new(0x7E_1693)));
            })
            .unwrap();
        }
        assert!(seen[0].iter().any(|&b| b != 0), "level {level:03X}");
        assert_eq!(seen[0], seen[1], "level {level:03X}");
    }
}

/// Yoshi's berry check, here a stunned baby Yoshi's, as Lunar Magic 3.70's
/// is observed to work (docs/lunar-magic-install.md, "Yoshi and berries"):
/// a custom tile acting like a berry is eaten as one, in a horizontal and a
/// vertical level and on layer 2 where the level has layer 2 interaction,
/// layer 1 first; a berry on layer 2 makes its mushroom where the berry is
/// on layer 1. Checked against Lunar Magic's code by hand with
/// `examples/contact_probe.rs berry`.
#[test]
fn berries_follow_the_chain_in_every_layout() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (_, rom) in installs(&clean) {
        berries(&rom);
    }
}

/// What a stunned baby Yoshi did with a tile it was held on.
#[derive(Debug)]
struct Berry {
    /// The berry it ate (`$18D6`) on the first frame it ate one, 0 if none.
    eaten: u8,
    /// The tile left where it was held.
    tile: u16,
    /// `$1693` at the end: the low byte of what the tile acts like.
    acts_like: u8,
    /// Where the mushroom a berry makes is from Yoshi, on layer 1.
    mushroom: Option<(i32, i32)>,
}

fn berries(rom: &Rom) {
    use kobo_core::ram::{self, RamAddr};
    // $245 acts like a berry, $246 like one through $245, $24A like cement.
    let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
    let table = rom.read_ptr(SnesAddr::new(0x06F624)).unwrap();
    for (tile, acts_like) in [(0x245, 0x045), (0x246, 0x245), (0x24A, 0x130)] {
        rom.write_u16(table.add(2 * tile), acts_like).unwrap();
    }
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    let byte = |table: u32, i: u32| rom.read_u8(SnesAddr::new(table + i)).unwrap() as u32;
    // Where the game keeps the tile at block (x, y) of a layer: its screen
    // offset tables, DATA_00BA60 and on.
    let cell = |layer2: bool, vertical: bool, x: u32, y: u32| {
        let (screen, high) = if vertical {
            (y / 16, x / 16)
        } else {
            (x / 16, y / 16)
        };
        let (low, high_table) = match (vertical, layer2) {
            (false, false) => (0x00BA60, 0x00BA9C),
            (false, true) => (0x00BA70, 0x00BAAC),
            (true, false) => (0x00BA80, 0x00BABC),
            (true, true) => (0x00BA8E, 0x00BACA),
        };
        let start = byte(high_table, screen) << 8 | byte(low, screen);
        0x7E_0000 + start + (y % 16) * 16 + x % 16 + (high << 8)
    };
    // Yoshi held where its centre is on (x, y) of the layer, from the frame
    // the tiles go in, by when the layers' offsets ($26, $28) are set;
    // `other` goes on layer 1 where layer 2's (x, y) is.
    let play = |level: u16, layer2: bool, x: i32, y: i32, tile: u16, other: Option<u16>| {
        let mode = level::read_primary_header(&rom, level).unwrap().level_mode;
        let vertical = |layer2| {
            if layer2 {
                mode.layer2() == level::Layer2Kind::VerticalObjects
            } else {
                mode.layer1_vertical()
            }
        };
        let here = cell(layer2, vertical(layer2), x as u32, y as u32);
        let mut eaten = 0;
        let place = 4;
        let ram = expand::play_level(&rom, level, 12, |frame, ram| {
            let offset = |a| ram.u16(at(a)) as i16 as i32;
            let (dx, dy) = if layer2 {
                (offset(0x26), offset(0x28))
            } else {
                (0, 0)
            };
            let (yx, yy) = ((x * 16 - dx) as u16, (y * 16 - dy) as u16);
            if frame == 0 {
                ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                ram.set_u8_at(ram::SPRITE_NUMBER, 0, 0x2D);
                ram.set_u8_at(ram::SPRITE_STATUS, 0, 0x09);
                // What InitSpriteTables loads: its tweaker bytes.
                for (table, values) in [
                    (0x1656, 0x07F26C),
                    (0x1662, 0x07F335),
                    (0x166E, 0x07F3FE),
                    (0x167A, 0x07F4C7),
                    (0x1686, 0x07F590),
                    (0x190F, 0x07F659),
                ] {
                    ram.set_u8_at(at(table), 0, byte(values, 0x2D) as u8);
                }
                // The player out of the way, at the level's left.
                ram.set_u16(ram::PLAYER_X, 0x20);
            }
            if frame == place {
                ram.set_u8(RamAddr::new(here), tile as u8);
                ram.set_u8(RamAddr::new(here + 0x1_0000), (tile >> 8) as u8);
                if let Some(other) = other {
                    let point = (x * 16 + 8 - dx) as u32 / 16;
                    let row = (y * 16 + 8 - dy) as u32 / 16;
                    let there = cell(false, vertical(false), point, row);
                    ram.set_u8(RamAddr::new(there), other as u8);
                    ram.set_u8(RamAddr::new(there + 0x1_0000), (other >> 8) as u8);
                }
                ram.set_u8(at(0x18D6), 0);
            }
            if frame > place && eaten == 0 {
                eaten = ram.u8(at(0x18D6));
            }
            ram.set_u8_at(ram::SPRITE_X_LOW, 0, yx as u8);
            ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (yx >> 8) as u8);
            ram.set_u8_at(ram::SPRITE_Y_LOW, 0, yy as u8);
            ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, (yy >> 8) as u8);
            ram.set_u16(ram::PLAYER_X, 0x20);
        })
        .unwrap();
        let position = |i| {
            let x = u16::from_le_bytes([
                ram.u8_at(ram::SPRITE_X_LOW, i),
                ram.u8_at(ram::SPRITE_X_HIGH, i),
            ]) as i32;
            let y = u16::from_le_bytes([
                ram.u8_at(ram::SPRITE_Y_LOW, i),
                ram.u8_at(ram::SPRITE_Y_HIGH, i),
            ]) as i32;
            (x, y)
        };
        let yoshi = position(0);
        let mushroom = (1..ram.map().sprite_slots())
            .find(|&i| {
                ram.u8_at(ram::SPRITE_STATUS, i) != 0 && ram.u8_at(ram::SPRITE_NUMBER, i) == 0x74
            })
            .map(|i| {
                let (x, y) = position(i);
                (x - yoshi.0, y - yoshi.1)
            });
        Berry {
            eaten,
            tile: u16::from_le_bytes([
                ram.u8(RamAddr::new(here)),
                ram.u8(RamAddr::new(here + 0x1_0000)),
            ]),
            acts_like: ram.u8(at(0x1693)),
            mushroom,
        }
    };
    let near = |m: Option<(i32, i32)>| m.is_some_and(|(x, y)| x.abs() < 0x20 && y.abs() < 0x20);
    // Level 105 is horizontal; F7 vertical; E7 vertical with layer 2
    // objects, which the player and sprites meet; D4 horizontal with layer 2
    // objects, whose layer 2 is $C0 above layer 1. Positions two screens
    // down or across tell the layouts apart.
    for (level, layer2, x, y) in [
        (0x105, false, 6, 20),
        (0x0F7, false, 20, 30),
        (0x0E7, true, 4, 20),
        (0x0D4, true, 6, 10),
    ] {
        let name = format!("level {level:03X}, layer {}", 1 + layer2 as u8);
        for tile in [0x045, 0x245, 0x246] {
            let berry = play(level, layer2, x, y, tile, None);
            assert_eq!(berry.eaten, 1, "{name}, tile {tile:03X}: {berry:?}");
            assert_eq!(berry.tile, 0x049, "{name}, tile {tile:03X}: {berry:?}");
            assert!(near(berry.mushroom), "{name}, tile {tile:03X}: {berry:?}");
        }
        let cement = play(level, layer2, x, y, 0x24A, None);
        assert_eq!(
            (cement.eaten, cement.tile, cement.acts_like),
            (0, 0x24A, 0x30),
            "{name}"
        );
    }
    // Layer 1 first: a berry on each where Yoshi is, layer 1's eaten.
    let both = play(0x0D4, true, 6, 10, 0x045, Some(0x046));
    assert_eq!(both.eaten, 2, "{both:?}");
    // Nothing meets layer 2 in level 105: a berry there stays.
    let layer2 = play(0x105, true, 6, 20, 0x245, None);
    assert_eq!((layer2.eaten, layer2.tile), (0, 0x245), "{layer2:?}");
}

/// Lunar Magic's added layer 2 scroll settings, as its camera was observed
/// to run them (docs/lunar-magic-install.md, "Layer 2 scroll settings"):
/// layer 2 against layer 1, frame by frame, with the player carried right
/// through level 105. Checked against Lunar Magic's camera by hand with
/// `examples/exlevel_probe.rs compare`.
#[test]
fn layer2_scroll_settings_move_layer_2() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (_, rom) in installs(&clean) {
        layer2_scroll(&rom);
    }
}

fn layer2_scroll(rom: &Rom) {
    use kobo_core::ram::RamAddr;
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    // Each frame's ($1A, $1E, $1C, $20, $1417) with the level's $05F000
    // high nibble and $06FA00 byte set.
    let play = |nibble: u8, fa: u8| {
        let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
        let header = SnesAddr::new(0x05F000 + 0x105);
        let byte = rom.read_u8(header).unwrap();
        rom.write_u8(header, byte & 0x0F | nibble << 4).unwrap();
        rom.write_u8(SnesAddr::new(0x06FA00 + 0x105), fa).unwrap();
        let mut frames = Vec::new();
        expand::play_level(&rom, 0x105, 120, |frame, ram| {
            ram.set_u16(at(0x94), (48 + 3 * frame) as u16);
            let word = |a| ram.u16(at(a)) as i32;
            frames.push([word(0x1A), word(0x1E), word(0x1C), word(0x20), word(0x1417)]);
        })
        .unwrap();
        frames
    };
    // Settings 8 and on are vertical 1/4 and on, with horizontal 1/2.
    for [x, x2, y, y2, offset] in play(8, 0x20) {
        assert_eq!((x2, y2), (x >> 1, (y >> 2) + offset), "setting 8");
    }
    // Separate settings: horizontal 1.2 times (8) and 1/32 (3).
    let fast = play(8, 0xA1);
    assert!(fast.iter().any(|f| f[0] > 0x40), "layer 1 did not move");
    for [x, x2, ..] in fast {
        assert_eq!(x2, x * 6 / 5, "horizontal 8");
    }
    for [x, x2, ..] in play(3, 0xA1) {
        assert_eq!(x2, x >> 5, "horizontal 3");
    }
    // Moving settings: horizontal $12 (H and nibble 2) a pixel a frame
    // ahead of layer 1, vertical $16 a quarter of a pixel a frame up.
    let moving = play(2, 0xF6);
    let ahead: Vec<i32> = moving.iter().map(|f| f[1] - f[0]).collect();
    assert!(
        ahead.windows(2).all(|w| w[1] == w[0] + 1),
        "horizontal $12: {ahead:?}"
    );
    let up: Vec<i32> = moving.iter().map(|f| f[3] - f[2]).collect();
    assert!(
        up.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] - 1),
        "vertical $16"
    );
    // A quarter of a pixel a frame, give or take where the fraction starts.
    let moved = up[0] - up[up.len() - 1];
    assert!(
        (moved - (up.len() as i32 - 1) / 4).abs() <= 1,
        "vertical $16: {up:?}"
    );
    // $1C on follows layer 1.
    for [x, x2, ..] in play(0xC, 0xE1) {
        assert_eq!(x2, x, "horizontal $1C");
    }
}

/// Kobo's VRAM patch (`vram.asm`): layer 1's tilemap at `$3000`, 64x32,
/// holds every cell in view as the camera moves, in a horizontal and a
/// vertical level, going both ways. Each cell's four words are its Map16
/// definition, for the tile the level's grid holds there at that frame.
/// That the tilemaps match Lunar Magic's own patch frame by frame is
/// checked by hand (docs/testing.md, `gfx_probe scroll`).
#[test]
fn the_vram_patch_keeps_layer_1_in_view() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    for (_, rom) in installs(&clean) {
        vram_patch(&rom);
    }
}

fn vram_patch(rom: &Rom) {
    use kobo_core::ram;
    // (level, pixels a frame right and down, then back)
    for (level, dx, dy) in [(0x105u16, 5i32, -2i32), (0x1CE, 1, -4), (0x01A, 4, -3)] {
        let loaded = expand::expand_level(rom, level).unwrap();
        assert_eq!(loaded.video.bg_sc[0], 0x31, "level {level:03X}");
        let tiles = &loaded.tiles;
        let mut start = None;
        let mut checked = 0;
        expand::play_game_loop(
            rom,
            level,
            120,
            |frame, ram| {
                let (x0, y0) =
                    *start.get_or_insert((ram.u16(ram::PLAYER_X), ram.u16(ram::PLAYER_Y)));
                let k = if frame < 60 {
                    frame as i32
                } else {
                    120 - frame as i32
                };
                ram.set_u16(ram::PLAYER_X, (x0 as i32 + dx * k).max(0) as u16);
                ram.set_u16(ram::PLAYER_Y, (y0 as i32 + dy * k).max(0) as u16);
                ram.set_u16(ram::RamAddr::new(0x7E_00D1), ram.u16(ram::PLAYER_X));
                ram.set_u16(ram::RamAddr::new(0x7E_00D3), ram.u16(ram::PLAYER_Y));
                ram.set_u8(ram::RamAddr::new(0x7E_1412), 1);
                ram.set_u8(ram::RamAddr::new(0x7E_1404), 1);
                ram.set_u8(ram::RamAddr::new(0x7E_13F1), 1);
            },
            |frame, played| {
                let x = played.ram.u16(ram::LAYER1_X) as usize;
                let y = played.ram.u16(ram::LAYER1_Y) as usize;
                let (cx, cy) = (x / 16, (y + 1) / 16);
                let (width, height) = tiles.size();
                for row in cy..(cy + 15).min(height) {
                    for col in cx..(cx + 17).min(width) {
                        let i = tiles.offset(col, row) as u32;
                        let n = played.ram.u8_at(ram::TILES_LOW, i) as u16
                            | (played.ram.u8_at(ram::TILES_HIGH, i) as u16) << 8;
                        // The pipes' definitions depend on where they are.
                        if expand::PIPE_TILES.contains(&n) {
                            continue;
                        }
                        let Some(def) = tiles.map16.get(&n) else {
                            continue;
                        };
                        for (k, word) in def.refs().iter().enumerate() {
                            let (tc, tr) = ((col * 2 + k / 2) % 64, (row * 2 + k % 2) % 32);
                            let at = 0x3000 + (tc / 32) * 0x400 + tr * 32 + tc % 32;
                            let got =
                                u16::from_le_bytes([played.vram[at * 2], played.vram[at * 2 + 1]]);
                            assert_eq!(
                                got, word.0,
                                "level {level:03X} frame {frame} camera ({x:X}, {y:X}) cell \
                                 ({col}, {row}) tile {n:X}"
                            );
                            checked += 1;
                        }
                    }
                }
            },
        )
        .unwrap();
        assert!(
            checked > 10_000,
            "level {level:03X}: {checked} words checked"
        );
    }
}

/// Kobo's ExAnimation code with no lists leaves the game's own animated
/// tiles and colour $64 flashing as they were, frame by frame.
#[test]
fn exanimation_without_lists_keeps_the_games_animation() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    for (_, rom) in installs(&clean) {
        game_animation(&asar, &rom);
    }
}

fn game_animation(asar: &kobo_core::asar::Asar, rom: &Rom) {
    let with = asar
        .patch(rom, &install::patch(install::EXANIMATION))
        .unwrap()
        .rom;
    let frames = |rom: &Rom, level: u16| {
        let mut out = Vec::new();
        expand::play_game_loop(
            rom,
            level,
            40,
            |_, _| {},
            |_, played| out.push((played.vram.to_vec(), played.cgram.to_vec())),
        )
        .unwrap();
        out
    };
    for level in [0x105, 0x0C7, 0x106, 0x0E5] {
        let (a, b) = (frames(rom, level), frames(&with, level));
        let first = (0..a.len()).find(|&f| a[f] != b[f]);
        assert_eq!(first, None, "level {level:03X}");
    }
}

/// `entrances.asm` moves all six entrance tables to blocks of the count it
/// is given, copying the game's entrances and Lunar Magic's further
/// tables' settings, and refuses a count outside `$201`-`$2000`.
#[test]
fn entrance_tables_take_the_count_they_are_given() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    for (_, rom) in installs(&clean) {
        let before = level::read_entrances(&rom).unwrap();
        let layout = kobo_core::entrance::Layout::of(&rom);
        let extra = |rom: &Rom, layout: &kobo_core::entrance::Layout| -> Vec<Vec<u8>> {
            layout
                .extra
                .unwrap()
                .iter()
                .map(|&t| rom.read(t, 0x1FE).unwrap().to_vec())
                .collect()
        };
        for count in [0x201u16, 0x321, 0x2000] {
            let moved = install::apply_entrances(&asar, &rom, count).unwrap();
            assert_eq!(level::entrance_count(&moved), count);
            let moved_layout = kobo_core::entrance::Layout::of(&moved);
            for table in level::entrance_tables(&moved)
                .into_iter()
                .chain(moved_layout.extra.unwrap())
            {
                let pc = moved.pc(table).unwrap().as_usize();
                let len = kobo_core::rats::tag_at(moved.data(), pc - 8);
                assert_eq!(len, Some(count as usize), "the table at {table}");
            }
            let after = level::read_entrances(&moved).unwrap();
            assert_eq!(after[..0x200], before[..]);
            assert!(after[0x200..].iter().all(|e| *e == Default::default()));
            assert_eq!(extra(&moved, &moved_layout), extra(&rom, &layout));
        }
        for count in [0x200u16, 0x2001] {
            assert!(
                install::apply_entrances(&asar, &rom, count).is_err(),
                "a count of {count:X}"
            );
        }
    }
}
