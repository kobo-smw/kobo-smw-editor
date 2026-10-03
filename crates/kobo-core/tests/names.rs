//! The names in `kobo_core::names` and `LevelMode::name` must say what the
//! vanilla ROM's own tables do: which tilesets share an object set, which
//! objects have no routine, and what each level mode's per-mode tables
//! set up.

mod common;

use kobo_core::level::LevelMode;
use kobo_core::names::{self, ObjectSet, UNUSED};
use kobo_core::{Rom, SnesAddr};

/// `JSL ExecutePtrLong`, which the long pointer tables follow.
const JSL: u8 = 0x22;
/// The object tileset dispatch (`CODE_0DA415`) and the extended object
/// dispatch (`CODE_0DA106`), each a `JSL` after 5 bytes.
const TILESET_DISPATCH: u32 = 0x0DA415;
const EXTENDED_DISPATCH: u32 = 0x0DA106;
/// Each object set's routine: `SEP`, `LDX`, `DEX`, `TXA`, then the `JSL`.
const SET_TABLE_OFFSET: u32 = 10;
/// The per-level-mode tables in `LoadLevel`.
const VERTICAL_TABLE: u32 = 0x058417;
const MAIN_SCREEN_TABLE: u32 = 0x058437;
const COLOUR_MATH_TABLE: u32 = 0x058477;
const SPECIAL_LEVEL_TABLE: u32 = 0x058497;

fn u8_at(rom: &Rom, addr: u32) -> u8 {
    rom.read_u8(SnesAddr::new(addr)).unwrap()
}

/// The long pointers of a table that follows a `JSL` at `jsl`.
fn table_after(rom: &Rom, jsl: u32, len: u32) -> Vec<u32> {
    assert_eq!(u8_at(rom, jsl), JSL, "no JSL at ${jsl:06X}");
    (0..len)
        .map(|i| rom.read_u24(SnesAddr::new(jsl + 4 + 3 * i)).unwrap())
        .collect()
}

#[test]
fn object_sets_follow_the_game_dispatch() {
    let Some(rom) = common::vanilla() else {
        return;
    };
    let by_tileset = table_after(&rom, TILESET_DISPATCH + 5, 15);
    for a in 0..15u8 {
        for b in 0..15u8 {
            let same_routine = by_tileset[a as usize] == by_tileset[b as usize];
            let same_set = ObjectSet::of_tileset(a) == ObjectSet::of_tileset(b);
            assert_eq!(same_routine, same_set, "tilesets {a:X} and {b:X}");
        }
    }
    for tileset in 0..15u8 {
        let set = table_after(
            &rom,
            by_tileset[tileset as usize] + SET_TABLE_OFFSET - 4,
            0x3F,
        );
        let routine = |n: u8| set[n as usize - 1];
        // Object 18, the water surface, is the routine unused objects fall
        // into.
        for n in 0x01..=0x3F {
            let name = names::standard_object(n, tileset).unwrap();
            let fallthrough = n >= 0x22 && routine(n) == routine(0x18);
            assert_eq!(
                name == UNUSED,
                fallthrough,
                "tileset {tileset:X} object {n:02X}: {name}"
            );
        }
    }
}

#[test]
fn unused_extended_objects_have_no_routine() {
    let Some(rom) = common::vanilla() else {
        return;
    };
    let table = table_after(&rom, EXTENDED_DISPATCH + 5, 0x100);
    let door = table[0x47];
    for n in 0x00..=0xFFu8 {
        let name = names::extended_object(n);
        let routine = table[n as usize];
        // 98-FF share the door routine and read past its tile table.
        let none = routine == 0 || (n > 0x48 && routine == door);
        // Lunar Magic gives 02 and 03 a meaning.
        let lunar_magic = matches!(n, 0x02 | 0x03);
        assert_eq!(
            name == UNUSED,
            none && !lunar_magic,
            "extended object {n:02X}: {name}"
        );
    }
}

#[test]
fn level_mode_names_follow_the_per_mode_tables() {
    let Some(rom) = common::vanilla() else {
        return;
    };
    for mode in 0..0x20u8 {
        let name = LevelMode(mode).name().unwrap();
        let vertical = u8_at(&rom, VERTICAL_TABLE + mode as u32);
        let main_screen = u8_at(&rom, MAIN_SCREEN_TABLE + mode as u32);
        let colour_math = u8_at(&rom, COLOUR_MATH_TABLE + mode as u32);
        let special = u8_at(&rom, SPECIAL_LEVEL_TABLE + mode as u32);
        let at = format!("mode {mode:02X}: {name}");
        assert_eq!(name.starts_with("Vertical"), vertical & 0x01 != 0, "{at}");
        // Bit 7 of `$5B` lets the player interact with layer 2.
        assert_eq!(name.contains("solid"), vertical & 0x80 != 0, "{at}");
        if name.contains("layer 2") && vertical & 0x02 != 0 {
            let horizontal_layer1 = !name.starts_with("Vertical");
            assert!(!horizontal_layer1 || name.contains("vertical"), "{at}");
        }
        assert_eq!(name.starts_with("Boss"), special != 0, "{at}");
        assert_eq!(
            name.contains("layer 3 in front"),
            main_screen == 0x04,
            "{at}"
        );
        assert_eq!(
            name.contains("translucent layer 1"),
            main_screen == 0x01,
            "{at}"
        );
        assert_eq!(
            name.contains("translucent solid layer 2"),
            main_screen == 0x02,
            "{at}"
        );
        assert_eq!(name.contains("dark"), colour_math == 0x70, "{at}");
        assert_eq!(name.contains("spotlight"), colour_math == 0xFF, "{at}");
    }
}
