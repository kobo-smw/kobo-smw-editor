//! Lunar Magic's taller levels, built with Kobo's code for them
//! (`asm/lunar-magic/exlevel.asm`): a level of each height places its
//! objects where its size says, through screen jumps with a vertical part,
//! leaves the RAM Lunar Magic's code leaves for that size
//! (docs/lunar-magic-install.md, "Taller levels"), and reads back from the
//! build as it was written. The corpus check reads every Lunar Magic ROM's
//! size table where Lunar Magic's layout has it.
//!
//! Needs the vanilla ROM and Asar; the corpus check `KOBO_LM_ROMS` and
//! `KOBO_MWL_DIR`.

mod common;

use kobo_core::build::{self, Project};
use kobo_core::level::LevelMode;
use kobo_core::level::objects::Object;
use kobo_core::level::size::{self, LevelSize};
use kobo_core::source::level::{Layer2, Level};
use kobo_core::{Rom, expand, import, ram};

fn project(levels: Vec<(u16, Level)>) -> Project {
    Project {
        root: std::path::PathBuf::from("."),
        levels,
        ..Default::default()
    }
}

/// Level 105 with a size, as many screens as it allows (at most 8), and a
/// cement block at the bottom left and the bottom right of each screen.
fn tall_level(clean: &Rom, size: LevelSize) -> Level {
    let (mut level, _) = import::read_level(clean, 0x105).unwrap();
    let screens = size.screens().min(8);
    level.size = size;
    level.header.screens = screens as u8;
    level.layer1 = (0..screens as u16)
        .flat_map(|screen| {
            let y = size.rows() as u16 - 1;
            [
                (screen * 16, y),
                (screen * 16 + 15, y),
                (screen * 16 + 3, 0),
            ]
        })
        .map(|(x, y)| Object::Standard {
            number: 0x0D,
            x,
            y,
            settings: 0x00,
        })
        .collect();
    level
}

#[test]
fn every_size_places_its_objects_and_sets_up_its_ram() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    for mode in 0..32u8 {
        for bottom_row in [false, true] {
            let size = LevelSize {
                mode,
                bottom_row,
                split: false,
            };
            let level = tall_level(&clean, size);
            // On the clean ROM, and as an SA-1 project.
            for (_, built) in common::builds(&clean, &project(vec![(0x105, level.clone())])) {
                let sa1 = built.mapping().is_sa1();
                let what = format!("size {mode:02X}, bottom row {bottom_row}, SA-1 {sa1}");
                check_size(&built, &level, size, &what);
            }
        }
    }
}

/// What a build with `level` at `size` as level 105 loads and leaves.
fn check_size(built: &Rom, level: &Level, size: LevelSize, what: &str) {
    assert_eq!(
        import::read_level(built, 0x105).unwrap().0.size,
        size,
        "{what}"
    );
    let loaded = expand::expand_level(built, 0x105).unwrap();
    let tiles = &loaded.tiles;
    for object in &level.layer1 {
        let Object::Standard { x, y, .. } = object else {
            unreachable!()
        };
        assert_eq!(
            tiles.tile_at(*x as usize, *y as usize),
            0x130,
            "{what}: the block at ({x}, {y})"
        );
    }
    let m = &loaded.ram;
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    if size.is_default() {
        // The game's own size needs no Lunar Magic layout: the build
        // is the game's, with no taller levels code.
        assert_eq!(m.u16(at(0x13D7)), 0, "{what}: $13D7");
        return;
    }
    // The tile grid's banks, which SA-1 Pack moves.
    let low = m.map().resolve(at(0xC800)) & 0xFF_0000;
    let high = m.map().resolve(ram::RamAddr::new(0x7F_C800)) & 0xFF_0000;
    let height = size.height();
    assert_eq!(m.u16(at(0x13D7)), height, "{what}: $13D7");
    assert_eq!(m.u16(at(0x1936)), height - 0x10, "{what}: $1936");
    assert_eq!(
        m.u8(at(0x0BF5)) & 0xDF,
        size.to_byte(false),
        "{what}: $0BF5"
    );
    for n in 0..32u32 {
        let address = 0xC800 + n * height as u32;
        let address = if address > 0xFFFF { 0 } else { address };
        assert_eq!(
            m.u24(at(0x0BF6 + 3 * n)),
            low | address,
            "{what}: screen pointer {n}"
        );
        assert_eq!(
            m.u24(at(0x0C56 + 3 * n)),
            high | address,
            "{what}: screen pointer {n}, high bytes"
        );
        assert_eq!(m.u8(at(0x0CB6 + n)), address as u8, "{what}: offset {n}");
        assert_eq!(
            m.u8(at(0x0CD6 + n)),
            (address >> 8) as u8,
            "{what}: offset {n}, high byte"
        );
    }
    // The scratch the code used is left as it was.
    assert_eq!(m.u8(at(0x0BDD)), 0, "{what}: the vertical part");
}

#[test]
fn layer_2_objects_start_where_the_size_splits_the_screens() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // Level 1A: layer 2 objects (mode 02).
    let (mut level, _) = import::read_level(&clean, 0x1A).unwrap();
    assert!(matches!(level.layer2, Layer2::Objects(_)));
    for mode in [0x01, 0x03, 0x0A, 0x16, 0x19] {
        let size = LevelSize {
            mode,
            bottom_row: false,
            split: false,
        };
        let split = size.layer2_screen();
        level.size = size;
        level.header.screens = split.clamp(1, 8) as u8;
        level.layer1 = Vec::new();
        level.layer2 = Layer2::Objects(vec![Object::Standard {
            number: 0x0D,
            x: 2,
            y: 3,
            settings: 0x00,
        }]);
        let built = build::build(&clean, &project(vec![(0x1A, level.clone())])).unwrap();
        let loaded = expand::expand_level(&built, 0x1A).unwrap();
        let m = &loaded.ram;
        let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
        let first = 0xC800 + split as u32 * size.height() as u32;
        let first = if first > 0xFFFF { 0 } else { first };
        assert_eq!(
            m.u24(at(0x0BF6 + 3 * 16)),
            0x7E_0000 | first,
            "size {mode:02X}: layer 2's first screen"
        );
        assert_eq!(
            loaded.tiles.layer2_object_tile(2, 3),
            Some(0x130),
            "size {mode:02X}: the layer 2 block"
        );
    }
}

#[test]
fn a_size_a_level_cannot_take_is_refused() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let size = LevelSize {
        mode: 0x19,
        bottom_row: false,
        split: false,
    };
    // A screen count past the room the height leaves builds, as Lunar
    // Magic's code loads it; an object past that room does not.
    let mut level = tall_level(&clean, size);
    level.header.screens = 5;
    assert!(build::build(&clean, &project(vec![(0x105, level.clone())])).is_ok());
    level
        .layer1
        .push(kobo_core::level::objects::Object::Standard {
            number: 0x0D,
            x: 16 * size.screens() as u16,
            y: 2,
            settings: 0,
        });
    let error = build::build(&clean, &project(vec![(0x105, level)])).unwrap_err();
    assert!(error.to_string().contains("leave room for"), "{error}");
    // Layer 2 objects in a size of one screen, where Lunar Magic's layout
    // starts layer 2's at screen 0, over layer 1's.
    let (mut level, _) = import::read_level(&clean, 0x1A).unwrap();
    level.size = LevelSize {
        mode: 0x1C,
        bottom_row: false,
        split: false,
    };
    level.header.screens = 1;
    assert!(build::build(&clean, &project(vec![(0x1A, level)])).is_err());
    // A vertical level.
    let (mut level, _) = import::read_level(&clean, 0x1CE).unwrap();
    level.size = size;
    assert_eq!(level.header.level_mode, LevelMode(0x08));
    assert!(build::build(&clean, &project(vec![(0x1CE, level)])).is_err());
}

#[test]
fn every_lunar_magic_rom_keeps_its_size_table_where_the_layout_has_it() {
    let Some(mwl_dir) = std::env::var_os("KOBO_MWL_DIR").map(std::path::PathBuf::from) else {
        eprintln!("skipping: KOBO_MWL_DIR is not set");
        return;
    };
    let mut checked = 0;
    for (path, rom) in common::lunar_magic_roms() {
        let Some(table) = size::table(&rom) else {
            continue;
        };
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let dir = mwl_dir.join(&name);
        if !dir.is_dir() {
            continue;
        }
        for level in 0..0x200u16 {
            let file = dir.join(format!("level {level:03X}.mwl"));
            let Ok(bytes) = std::fs::read(&file) else {
                continue;
            };
            let mwl = kobo_core::mwl::MwlFile::parse(&bytes).unwrap();
            let exported = mwl.section(kobo_core::mwl::Section::LevelInfo).unwrap()[16];
            let byte = rom.read_u8(table.add(level as u32)).unwrap();
            // The export writes T from the level; a level never saved keeps
            // 0 in the ROM.
            assert_eq!(
                byte & 0x7F,
                exported & 0x7F,
                "{name} level {level:03X}: size byte"
            );
            if byte & 0x1F != 0 {
                assert_eq!(byte, exported, "{name} level {level:03X}: T");
            }
            checked += 1;
        }
    }
    eprintln!("{checked} levels' size bytes checked");
}

/// Choc Island 2's rooms (`asm/lunar-magic/choc-island.asm`): entered
/// through translevel `$24`, level `0CD` loads one of the game's rooms,
/// whose 16-bit pointers take the banks of `0CD`'s own data. A build moves
/// that data, at the game's height and at a size of its own, and the room
/// still loads with the game's banks, as on the clean ROM.
#[test]
fn choc_island_2s_rooms_keep_their_banks_when_their_levels_move() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let pointers = |rom: &Rom| {
        let ram = expand::play_level_entered(
            rom,
            0x0CD,
            |ram| ram.set_u8(ram::RamAddr::new(0x7E_13BF), 0x24),
            1,
            |_, _| {},
        )
        .unwrap();
        // Layer 1's, layer 2's, and the sprites' pointers.
        [0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0xCE, 0xCF, 0xD0]
            .map(|a| ram.u8(ram::RamAddr::new(0x7E_0000 + a)))
    };
    let expected = pointers(&clean);
    assert_eq!([expected[2], expected[5], expected[8]], [0x06, 0x0C, 0x07]);
    for rows in [27, 28] {
        let (mut level, _) = import::read_level(&clean, 0x0CD).unwrap();
        level.size = LevelSize::from_byte(LevelSize::with_rows(rows).unwrap());
        for (_, built) in common::builds(&clean, &project(vec![(0x0CD, level)])) {
            let moved = kobo_core::level::layer1_ptr(&built, 0x0CD).unwrap();
            assert_ne!(moved.bank(), 0x06, "{rows} rows: level 0CD moved");
            assert_eq!(pointers(&built), expected, "{rows} rows");
        }
    }
}
