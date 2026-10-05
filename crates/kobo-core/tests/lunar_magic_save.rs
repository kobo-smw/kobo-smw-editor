//! The Lunar Magic check, automated: Lunar Magic 3.70 saves a copy of a
//! build (exporting a level and importing it back, as
//! `tools/lunar-magic/save-check` does), and everything the build wrote
//! must read back the same: the level, with its Lunar Magic objects,
//! palette, and entrance settings, and the Map16 pages, 0 and 1 and the
//! tilesets' own among them. Lunar Magic's full Map16 export of a build
//! must also show every tile and acts-like setting as the build wrote
//! them, which is what its Map16 editor reads. Each check runs twice,
//! with the bytes Kobo writes only because Lunar Magic checks them and
//! without, and requires what Lunar Magic does differently without them to
//! be what docs/lunar-magic-install.md records.
//!
//! Opt-in: `KOBO_LUNAR_MAGIC` is Lunar Magic 3.70's folder (the one with
//! `x64/Lunar Magic.exe`), run under Wine with `xvfb-run` through
//! `tools/lunar-magic/lm`. Needs the vanilla ROM and Asar too.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fixtures::sha1_hex;
use common::temp::TempDir;
use common::{Lcg, lm};

use kobo_core::build::{self, Project};
use kobo_core::entrance::{MidwayEntrance, SeparateMidway};
use kobo_core::level::objects::{Object, ScreenExit};
use kobo_core::map16::pages as map16_pages;
use kobo_core::map16::{GameTables, Map16Tile, TILESET_COUNT, Tile8Ref};
use kobo_core::palette::{Color15, CustomPalette, Palette};
use kobo_core::source::map16::{DEFAULT_ACTS, GamePage, GameTile, Map16Entry, Map16Page};
use kobo_core::tools::Tool;
use kobo_core::{Rom, SnesAddr, clean_room, exanimation, import};

const LEVEL: u16 = 0x105;

/// The sites of Yoshi's berry check that Kobo's `actslike.asm` takes, in
/// Lunar Magic's piece of bank `$06`, whose check is `$06F600`.
const BERRY_SITES: [u32; 4] = [0x01A24C, 0x01F589, 0x02BA9E, 0x02BAE8];

/// The sites of Lunar Magic's sprite loader group, which Kobo's
/// `sprites.asm` takes, and their lengths.
const SPRITE_LOADER_SITES: [(u32, usize); 19] = [
    (0x02A826, 8),
    (0x02A82E, 8),
    (0x02A846, 4),
    (0x02A8D8, 3),
    (0x02A968, 4),
    (0x02A9D7, 3),
    (0x02AB54, 5),
    (0x02ABD0, 5),
    (0x02ABF3, 1),
    (0x02AC64, 4),
    (0x02ACA4, 4),
    (0x02AF3D, 4),
    (0x02AFA7, 4),
    (0x01AC40, 10),
    (0x02D03A, 10),
    (0x02FED6, 10),
    (0x03B86C, 10),
    (0x01C08C, 8),
    (0x01C0E2, 1),
];

/// Has Lunar Magic save a copy of `rom` in a folder of its own, with the
/// clean ROM where its restore system wants it, and returns the copy.
///
/// Every build saved here is checked for the clean room on the way: a
/// build is never taken for a ROM Lunar Magic saved, and the copy is,
/// with its marker or without it (`kobo_core::clean_room`).
fn save(lunar_magic: &Path, clean: &Rom, rom: &Rom, name: &str) -> Rom {
    assert!(
        !clean_room::saved_by_lunar_magic(rom),
        "{name}: a Kobo build taken for a ROM Lunar Magic saved"
    );
    let ws = lm::Workspace::new(
        lunar_magic,
        &format!("save-{name}"),
        rom,
        Some(clean),
        false,
    );
    let level = format!("{LEVEL:X}");
    for command in ["-ExportLevel", "-ImportLevel"] {
        ws.run(&[command, "rom.smc", "level.mwl", &level]);
    }
    let saved = ws.rom();
    assert_ne!(saved.data(), rom.data(), "Lunar Magic saved nothing");
    assert!(clean_room::saved_by_lunar_magic(&saved));
    let mut unmarked = Rom::from_headerless(saved.data().to_vec()).unwrap();
    unmarked.write(clean_room::MARKER, &[0xFF; 64]).unwrap();
    assert!(
        clean_room::saved_by_lunar_magic(&unmarked),
        "{name}: a save without its marker not taken for one"
    );
    saved
}

/// A name for a save's or an export's folder, apart for an SA-1 build.
fn tag(name: &str, sa1: bool) -> String {
    if sa1 {
        format!("{name}-sa1")
    } else {
        name.to_string()
    }
}

/// A tile made up from its number, to tell tiles apart.
fn entry(tile: u16) -> Map16Entry {
    let r = |i: u16| {
        Tile8Ref::new(
            tile.wrapping_add(i) & 0x3FF,
            (tile % 8) as u8,
            false,
            false,
            false,
        )
    };
    Map16Entry {
        gfx: Map16Tile {
            top_left: r(0),
            bottom_left: r(1),
            top_right: r(2),
            bottom_right: r(3),
        },
        acts: [0x025, 0x130, 0x12F][tile as usize % 3],
    }
}

/// Level 105 with what step 2b builds in Lunar Magic's layout: objects
/// placing tiles of page 2, a palette of its own, a main entrance and a
/// separate midway entrance with Lunar Magic's settings, and a secondary
/// entrance using both of its further tables; and Map16 pages in three
/// groups.
fn project(clean: &Rom) -> Project {
    let (mut level, _) = import::read_level(clean, LEVEL).unwrap();
    level.layer1.push(Object::Lunar {
        number: 0x22,
        x: 1,
        y: 1,
        data: vec![0x01, 0xAB],
    });
    level.layer1.push(Object::Lunar {
        number: 0x27,
        x: 4,
        y: 1,
        data: vec![0x11, 0x42, 0x00],
    });
    // A time limit of 113, reset on every entry.
    level.layer1.push(Object::Unplaced(vec![0x41, 0x83, 0x81]));
    // A screen exit only Lunar Magic's format can say (to a secondary
    // entrance in bank 0, in water), beside the level's normal exit in the
    // game's format on screen 7, which a save turns secondary like this one
    // unless the build writes it in Lunar Magic's format too.
    level.layer1.push(Object::ScreenExit(ScreenExit {
        screen: 3,
        flags: ScreenExit::LUNAR_MAGIC | ScreenExit::SECONDARY | ScreenExit::WATER,
        destination: 0x20,
    }));
    // A changed sprite list, which goes in a RATS block of its own, its
    // bank in Lunar Magic's table: more sprites than the game's loader
    // reads, one of them starting with $FF (Y 31, extra bits 3, on screen
    // 17), so in the new sprite system's format; and the level's spawn
    // range and smart spawning, which Kobo's loader reads.
    level.sprites.list[0].x += 1;
    let first = level.sprites.list.len();
    for i in first..100 {
        level.sprites.list.push(kobo_core::source::level::Sprite {
            id: 0x0F,
            x: 16 * 17 + (i % 16) as u16,
            y: 4 + (i % 20) as u16,
            extra_bits: 0,
            extension: Vec::new(),
        });
    }
    level.sprites.list.push(kobo_core::source::level::Sprite {
        id: 0x0F,
        x: 16 * 17 + 5,
        y: 31,
        extra_bits: 3,
        extension: Vec::new(),
    });
    level.sprites.list.sort_by_key(|s| s.x / 16);
    level.settings.spawn_range = 2;
    level.settings.smart_spawn = true;
    let mut palette = Palette::default();
    for (i, color) in palette.colors.iter_mut().enumerate() {
        *color = Color15::from_rgb5(i as u8 % 32, (i / 8) as u8 % 32, 31 - i as u8 % 32);
    }
    level.palette = Some(CustomPalette {
        back_area: Color15::from_rgb5(3, 5, 7),
        palette,
    });
    level.animation = Some(animation_list(0x2000));
    level.animation_settings = Some(0x80);
    level.entrance.entrance_screen = 1;
    level.entrance.midway_screen = 2;
    level.settings.tile_position = Some((1, 1));
    level.settings.relative = Some(true);
    level.settings.face_left = true;
    // Lunar Magic's separate layer 2 scroll settings: horizontally moving
    // right 3 pixels a frame (the nibble 4 with H), vertically down 1.
    level.entrance.layer2_scroll = 4;
    level.settings.layer2_horizontal_high = true;
    level.settings.layer2_vertical_scroll = Some(0x12);
    level.settings.midway.separate = Some(SeparateMidway::Entrance(MidwayEntrance {
        slippery: false,
        water: false,
        action: 0,
        x: 17,
        y: 20,
        fg_position: 3,
        bg_position: 0,
        relative: None,
        face_left: false,
    }));
    let entrance = &mut level.entrances[0];
    (entrance.x, entrance.y) = (3, 8);
    entrance.settings.tile_position = Some((0, 2));
    entrance.settings.relative = Some(true);
    entrance.settings.face_left = true;
    entrance.settings.water = true;
    let mut map16 = Vec::new();
    for page in [0x02u8, 0x13, 0x45] {
        let first = page as u16 * 0x100;
        let mut tiles = Map16Page::default();
        for tile in [first, first + 1, first + 0x10, first + 0x11, first + 0xFF] {
            let mut entry = entry(tile);
            // Page 2 is per tileset: its graphics are in the tileset files.
            if page == 0x02 {
                entry.gfx = Map16Tile::default();
            }
            tiles.tiles.insert(tile, entry);
        }
        map16.push((page, tiles));
    }
    // Pages 0 and 1: a tile acting like another, one with graphics of its
    // own, and tiles the game keeps per tileset, for tileset 0 and the
    // tilesets sharing its table (7, level 105's, among them) and for
    // tileset 4; and page 2 per tileset, for tilesets 7 and 3.
    let game = GameTables::read(clean).unwrap();
    let specific = (0..0x200).find(|&t| game.is_specific(t)).unwrap();
    let common = (0x100..0x200).find(|&t| !game.is_specific(t)).unwrap();
    let mut page0 = GamePage::default();
    page0.tiles.insert(
        0x0F0,
        GameTile {
            acts: Some(0x025),
            gfx: None,
        },
    );
    let mut page1 = GamePage::default();
    page1.tiles.insert(
        common,
        GameTile {
            acts: Some(0x130),
            gfx: Some(entry(0x1111).gfx),
        },
    );
    let tileset = |tiles: &[(u16, u16)]| {
        let mut page = Map16Page::default();
        for &(tile, look) in tiles {
            page.tiles.insert(
                tile,
                Map16Entry {
                    acts: DEFAULT_ACTS,
                    ..entry(look)
                },
            );
        }
        page
    };
    Project {
        root: PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(LEVEL, level)],
        map16,
        map16_game: vec![(0x00, page0), (0x01, page1)],
        map16_tileset: vec![
            (0x0, tileset(&[(specific, 0x0100)])),
            (0x3, tileset(&[(0x2A0, 0x0303)])),
            (0x4, tileset(&[(specific, 0x0400)])),
            (
                0x7,
                tileset(&[(0x200, 0x0700), (0x242, 0x0742), (0x2FF, 0x07FF)]),
            ),
        ],
        map16_bg: Vec::new(),
        pipes: {
            // A vertical pipe's tiles in colour sets 0 and 3, and a diagonal
            // pipe's.
            let mut pipes = kobo_core::source::map16::Pipes::default();
            pipes.colours.insert((0, 0x133), entry(0x0133).gfx);
            pipes.colours.insert((3, 0x13A), entry(0x033A).gfx);
            pipes.diagonal.insert(0x1EC, entry(0x01EC).gfx);
            pipes
        },
        gfx: Vec::new(),
        exgfx: Vec::new(),
        animation_global: Some(animation_list(0x2400)),
        animation_files: vec![(0x60, (0..0x800).map(|i| i as u8).collect())],
        pixi_compiled: None,
    }
}

/// An ExAnimation list with a slot of each kind: tiles from RAM with the
/// ON/OFF trigger's second set, tiles from the alternative file, colours,
/// and a rotation; and triggers set at the load.
fn animation_list(vram: u16) -> exanimation::List {
    use exanimation::{List, Slot};
    let mut list = List {
        custom_keep: 0xFF00,
        custom_set: 0x0003,
        ..List::default()
    };
    list.manual.insert(2, 1);
    let slots = [
        (0, 0x04, 0x03, vram, vec![0xAD00, 0xAD80, 0xAE00, 0xAE80]),
        (3, 0x11, 0x00, vram | 0x8040, vec![0x0000, 0x0080, 0x0100]),
        (9, 0x14, 0x00, 0x0121, vec![0xB000, 0xB004]),
        (10, 0x18, 0x20, 0x0331, vec![]),
    ];
    for (n, kind, trigger, dest, frames) in slots {
        let frames_less_one = if kind == 0x18 {
            2
        } else {
            (frames.len()
                / if exanimation::second_set(trigger) {
                    2
                } else {
                    1
                }) as u8
                - 1
        };
        list.slots.insert(
            n,
            Slot {
                kind,
                trigger,
                frames_less_one,
                dest,
                frames,
            },
        );
    }
    list.count = 11;
    list
}

/// Whether the global ExAnimation list and file 60 read back from `saved`
/// as the project has them.
fn animation_kept(saved: &Rom, project: &Project) -> bool {
    let global = exanimation::read_global(saved).unwrap().map(|f| f.list);
    let files = exanimation::read_alt_files(saved).unwrap();
    global == project.animation_global && files == project.animation_files
}

/// The parts of the project's level that differ between two ROMs, and
/// whether every Map16 page the project lists reads back from `saved` as
/// it was written.
fn compare(built: &Rom, saved: &Rom, project: &Project) -> (Vec<String>, bool) {
    let parts = import::diff_levels(built, saved)
        .into_iter()
        .filter(|d| d.level == LEVEL)
        .flat_map(|d| d.parts)
        .collect();
    let (read, _) = import::read_map16(saved).unwrap();
    let pages = project.map16.iter().all(|(page, written)| {
        read.iter().any(|(p, back)| {
            p == page
                && back
                    .tiles
                    .iter()
                    .all(|(&tile, entry)| *entry == written.tile(tile))
        })
    });
    let clean = common::vanilla().unwrap();
    let game = import::read_map16_game(saved, &clean).unwrap();
    let game_pages = game.pages == project.map16_game && game.tilesets == project.map16_tileset;
    let pipes = import::read_pipes(saved, &clean).unwrap() == project.pipes;
    (parts, pages && game_pages && pipes)
}

/// A build with the bytes Kobo writes only because Lunar Magic checks them
/// put back to what a build without them has.
fn without_checked_bytes(built: &Rom) -> Rom {
    let mut without = Rom::from_headerless(built.data().to_vec()).unwrap();
    without.write(SnesAddr::new(0x03BD9C), &[0xFF; 4]).unwrap();
    for at in [0x05DC81, 0x0DE191, 0x0DE198, 0x0DE19F] {
        without.write(SnesAddr::new(at), &[0xFF; 3]).unwrap();
    }
    without.write(SnesAddr::new(0x06F5FC), &[0xFF; 2]).unwrap();
    without.write(SnesAddr::new(0x05DD7C), &[0xFF; 2]).unwrap();
    without.write_u8(SnesAddr::new(0x03FDFF), 0xFF).unwrap();
    without.fix_checksum().unwrap();
    without
}

/// Where Lunar Magic 3.70's full Map16 export disagrees with what the ROM
/// holds as Kobo reads it: `acts-like`, `pages 0-1 of tileset T`, `page 2
/// of tileset T`, or `page P`, each once. The file lists its
/// sections from `$70` as offset and size pairs: `$70` every tile's
/// definition, `$78` what tiles `0`-`7FFF` act like, `$90` page 2 of each
/// object tileset when it is per tileset (none otherwise), `$98` pages 0
/// and 1 of each object tileset, `$1000` bytes each (observed on its
/// exports of vanilla, Kaizo Kindergarten, and Kobo builds).
fn map16_export_mismatches(file: &[u8], rom: &Rom, project: &Project) -> Vec<String> {
    let section = |at: usize| {
        let word = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap()) as usize;
        &file[word(at)..word(at) + word(at + 4)]
    };
    let mut out: Vec<String> = Vec::new();
    let mut add = |kind: String| {
        if !out.contains(&kind) {
            out.push(kind);
        }
    };
    let acts = section(0x78);
    let differing: Vec<u16> = (0..0x4000u16)
        .filter(|&t| {
            let exported = u16::from_le_bytes([acts[2 * t as usize], acts[2 * t as usize + 1]]);
            map16_pages::acts_like(rom, t).unwrap() != Some(exported)
        })
        .collect();
    if !differing.is_empty() {
        add("acts-like".into());
    }
    let game = GameTables::read(rom).unwrap();
    let pages01 = section(0x98);
    for tileset in 0..TILESET_COUNT {
        for tile in 0..0x200u16 {
            let at = (tileset as usize * 0x200 + tile as usize) * 8;
            if pages01[at..at + 8] != *rom.read(game.address(tileset, tile), 8).unwrap() {
                add(format!("pages 0-1 of tileset {tileset:X}"));
            }
        }
    }
    // The pipes: the four colour sets in the game's order, set 1 being page
    // 1's own, and the diagonal pipes.
    let colours = section(0xA0);
    for set in 0..4u8 {
        for (i, tile) in kobo_core::map16::PIPE_COLOUR_TILES.enumerate() {
            let at = match set {
                1 => game.address(0, tile),
                _ => kobo_core::map16::pipe_address(Some(set), tile).unwrap(),
            };
            let n = (set as usize * 8 + i) * 8;
            if colours.get(n..n + 8) != Some(rom.read(at, 8).unwrap()) {
                add(format!("pipe colour set {set}"));
            }
        }
    }
    let diagonal = section(0xA8);
    for (i, tile) in kobo_core::map16::diagonal_pipe_tiles().enumerate() {
        let at = kobo_core::map16::pipe_address(None, tile).unwrap();
        if diagonal.get(i * 8..i * 8 + 8) != Some(rom.read(at, 8).unwrap()) {
            add("diagonal pipes".into());
        }
    }
    let page2 = section(0x90);
    if project.tileset_page2() {
        for tileset in 0..TILESET_COUNT {
            for tile in 0x200..0x300u16 {
                let at = (tileset as usize * 0x100 + tile as usize - 0x200) * 8;
                let def = map16_pages::tileset_page2_definition(rom, tileset, tile).unwrap();
                if page2.get(at..at + 8) != Some(rom.read(def, 8).unwrap()) {
                    add(format!("page 2 of tileset {tileset:X}"));
                }
            }
        }
    } else if !page2.is_empty() {
        add("page 2 per tileset, which the build does not have".into());
    }
    let all = section(0x70);
    for (page, _) in project.map16.iter().filter(|(p, _)| *p != 0x02) {
        let group = map16_pages::PAGE_GROUPS[*page as usize / 16];
        for tile in *page as u16 * 0x100..(*page as u16 + 1) * 0x100 {
            let def = group.definition(rom, tile).unwrap().unwrap();
            let at = tile as usize * 8;
            if all[at..at + 8] != *rom.read(def, 8).unwrap() {
                add(format!("page {page:02X}"));
            }
        }
    }
    out
}

#[test]
fn lunar_magic_reads_the_map16_a_build_writes() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let project = project(&base);
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let file = common::lm::export_map16(&lunar_magic, &built, &tag("with", sa1));
        let mismatches = map16_export_mismatches(&file, &built, &project);
        assert!(mismatches.is_empty(), "{mismatches:?}");

        // Without the marker at $06F5FC, Lunar Magic reads what tiles act like,
        // each tileset's page 2, and the pages past $0F from elsewhere.
        let file = common::lm::export_map16(
            &lunar_magic,
            &without_checked_bytes(&built),
            &tag("without", sa1),
        );
        let mismatches = map16_export_mismatches(&file, &built, &project);
        let mut expected = vec!["acts-like".to_string()];
        expected.extend((0..TILESET_COUNT).map(|t| format!("page 2 of tileset {t:X}")));
        expected.extend(["page 13".to_string(), "page 45".to_string()]);
        assert_eq!(mismatches, expected, "without the marker");
    }
}

#[test]
fn a_build_survives_a_lunar_magic_save() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let project = project(&base);
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(
            import::read_level(&built, LEVEL).unwrap().0,
            project.levels[0].1
        );
        let sprites = kobo_core::level::sprite_ptr(&built, LEVEL).unwrap();
        assert!(sprites.bank() >= 0x10, "the sprite list is at {sprites}");

        let saved = save(&lunar_magic, &clean, &built, &tag("with", sa1));
        let (parts, pages) = compare(&built, &saved, &project);
        assert!(parts.is_empty(), "level {LEVEL:X} after a save: {parts:?}");
        assert!(pages, "Map16 pages after a save");
        assert!(animation_kept(&saved, &project), "ExAnimation after a save");
        // The save keeps Kobo's sprite loader, whose check is the JSL at
        // $02AF3D, at every site of the group (lunar-magic-install.md).
        for (at, len) in SPRITE_LOADER_SITES {
            let at = SnesAddr::new(at);
            assert_eq!(
                saved.read(at, len).unwrap(),
                built.read(at, len).unwrap(),
                "the sprite loader's site {at} after a save"
            );
        }
        // And Kobo's berry check: with the gate set, a save does not put
        // Lunar Magic's hooks for Yoshi's tongue in its place.
        for at in BERRY_SITES {
            let at = SnesAddr::new(at);
            assert_eq!(
                saved.read(at, 4).unwrap(),
                built.read(at, 4).unwrap(),
                "the berry check's site {at} after a save"
            );
        }

        // And Kobo's camera, which runs the separate layer 2 scroll
        // settings: with "LM" at $05DD7C the save leaves it in place.
        let camera = SnesAddr::new(0x00F79D);
        assert_eq!(
            saved.read(camera, 4).unwrap(),
            built.read(camera, 4).unwrap(),
            "the camera's layer 2 site after a save"
        );
        // And the entrance's face left, at game mode $11's $009708 and the
        // slanted pipe's $00D2B2: the level is entered facing left.
        for (at, len) in [(0x009708, 4), (0x00D2B2, 2)] {
            let at = SnesAddr::new(at);
            assert_eq!(
                saved.read(at, len).unwrap(),
                built.read(at, len).unwrap(),
                "face left's site {at} after a save"
            );
        }
        let mut facing = None;
        kobo_core::expand::play_level(&saved, LEVEL, 1, |_, ram| {
            facing = Some(ram.u8(kobo_core::ram::RamAddr::new(0x7E_0076)));
        })
        .unwrap();
        assert_eq!(facing, Some(0), "facing left after a save");

        // Without the checked bytes (lunar-magic-install.md, "Bytes Kobo
        // writes because Lunar Magic checks them"), the save loses the
        // secondary entrance's further tables, the separate layer 2 scroll
        // settings, and the level's ExAnimation settings (set to what a
        // first save gives every level), and nothing else.
        let without = without_checked_bytes(&built);
        let saved = save(&lunar_magic, &clean, &without, &tag("without", sa1));
        let (parts, pages) = compare(&built, &saved, &project);
        assert_eq!(
            parts,
            ["settings", "entrances", "animation"],
            "level {LEVEL:X} after a save without the checked bytes"
        );
        let (level, _) = import::read_level(&saved, LEVEL).unwrap();
        assert_eq!(level.animation, project.levels[0].1.animation);
        assert_eq!(level.animation_settings, None);
        assert!(pages, "Map16 pages after a save without the checked bytes");
        assert!(
            animation_kept(&saved, &project),
            "ExAnimation without the checked bytes"
        );

        // Without the JSL at $00A390, a save of the level installs Lunar
        // Magic's ExAnimation over Kobo's, with tables of its own: the global
        // list and the level's are lost.
        let mut no_hook = Rom::from_headerless(built.data().to_vec()).unwrap();
        no_hook.write_u8(exanimation::CHECK, 0x5C).unwrap();
        no_hook.fix_checksum().unwrap();
        let saved = save(&lunar_magic, &clean, &no_hook, &tag("no-hook", sa1));
        let (parts, _) = compare(&built, &saved, &project);
        assert_eq!(parts, ["animation"], "without the JSL at $00A390");
        assert!(!animation_kept(&saved, &project));
    }
}

/// Lunar Magic's `-ExportGFX` of a copy of `rom`: each file's SHA-1, by
/// name.
fn export_gfx(lunar_magic: &Path, rom: &Rom, name: &str) -> Vec<(String, String)> {
    let ws = lm::Workspace::new(lunar_magic, &format!("gfx-{name}"), rom, None, true);
    ws.run(&["-ExportGFX", "rom.smc"]);
    let mut files: Vec<(String, String)> = fs::read_dir(ws.path().join("Graphics"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, sha1_hex(&fs::read(&path).unwrap()))
        })
        .collect();
    files.sort();
    files
}

/// An SA-1 build with its GFX in LC_LZ3 (`[rom] lz3`, which needs SA-1
/// Pack): Lunar Magic reads every file as from the same build in LC_LZ2
/// (which is vanilla's but for `GFX17`, whose export Lunar Magic 3.70
/// changes from 3.21's in the fixtures, LC_LZ2 or not), and a
/// save keeps the ROM's setting for LC_LZ3. Without `$0FFFFF` set it takes
/// the files for LC_LZ2, and a save records LC_LZ2 over the setting.
#[test]
fn lunar_magic_reads_an_lz3_build() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    if common::tool(Tool::Sa1Pack).is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let project = |lz3| Project {
        manifest: kobo_core::source::project::Manifest {
            sa1: true,
            lz3,
            ..Default::default()
        },
        ..Default::default()
    };
    let built = build::build(&clean, &project(true)).unwrap();
    let lz2 = build::build(&clean, &project(false)).unwrap();
    let vanilla = export_gfx(&lunar_magic, &lz2, "lz2");
    assert_eq!(export_gfx(&lunar_magic, &built, "with"), vanilla);
    let setting = kobo_core::gfx::COMPRESSION_SETTING;
    let saved = save(&lunar_magic, &clean, &built, "lz3-with");
    assert_eq!(saved.read_u8(setting).unwrap(), 0x02);

    let mut without = Rom::from_headerless(built.data().to_vec()).unwrap();
    without
        .write_u8(kobo_core::gfx::SETTINGS_PRESENT, 0xFF)
        .unwrap();
    without.fix_checksum().unwrap();
    assert_ne!(export_gfx(&lunar_magic, &without, "without"), vanilla);
    let saved = save(&lunar_magic, &clean, &without, "lz3-without");
    assert_eq!(saved.read_u8(setting).unwrap(), 0x00);
}

/// `rom` with `sprites` as `level`'s list, at `$3F8000`, and the level's
/// `tTT` byte set.
fn with_sprites(rom: &Rom, level: u16, list: &[u8], ttt: u8) -> Rom {
    let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
    rom.expand(0x20_0000).unwrap();
    let at = SnesAddr::new(0x3F_8000);
    rom.write(at, list).unwrap();
    let tables = |a: u32| SnesAddr::new(a + level as u32);
    rom.write_u16(SnesAddr::new(0x05EC00 + 2 * level as u32), 0x8000)
        .unwrap();
    rom.write_u8(tables(0x0EF100), 0x3F).unwrap();
    rom.write_u8(tables(0x05DE00), ttt).unwrap();
    rom.fix_checksum().unwrap();
    rom
}

/// The sprite slots and load flags after each frame of `level`, with the
/// player carried along `route` (frame, x, y) and kept alive, the frame
/// counter the game loop keeps running, vertical scrolling at will, and,
/// with `clear`, every slot emptied before each frame.
fn sprite_frames(rom: &Rom, level: u16, route: &[(u32, i32, i32)], clear: bool) -> Vec<Vec<u8>> {
    use kobo_core::ram::RamAddr;
    let frames = route.last().unwrap().0;
    let mut out = Vec::new();
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    kobo_core::expand::play_level(rom, level, frames, |frame, ram| {
        let (slots, flags) = (ram.map().sprite_slots(), ram.map().sprite_load_flags());
        let mut state = Vec::new();
        for table in [0x9E, 0x14C8, 0xE4, 0x14E0, 0xD8, 0x14D4, 0x161A, 0x187B] {
            state.extend((0..slots).map(|i| ram.u8_at(at(table), i)));
        }
        state.extend(ram.bytes(at(0x1938), flags as usize));
        // The range that keeps sprites, which PIXI's SubOffScreen reads,
        // and tTT.
        state.extend(ram.bytes(at(0x0BF0), 5));
        out.push(state);
        let w = route.windows(2).find(|w| frame < w[1].0).unwrap();
        let t = (frame - w[0].0) as i32;
        let n = (w[1].0 - w[0].0) as i32;
        let x = (w[0].1 + (w[1].1 - w[0].1) * t / n).max(0) as u16;
        let y = (w[0].2 + (w[1].2 - w[0].2) * t / n).max(0) as u16;
        ram.set_u8(at(0x94), x as u8);
        ram.set_u8(at(0x95), (x >> 8) as u8);
        ram.set_u8(at(0x96), y as u8);
        ram.set_u8(at(0x97), (y >> 8) as u8);
        ram.set_u8(at(0x1497), 0x7F);
        let true_frame = ram.u8(at(0x13));
        ram.set_u8(at(0x13), true_frame.wrapping_add(1));
        for (a, v) in [(0x1412, 1), (0x13F1, 1), (0x1404, 1)] {
            ram.set_u8(at(a), v);
        }
        if clear {
            for i in 0..slots {
                ram.set_u8_at(at(0x14C8), i, 0);
            }
        }
    })
    .unwrap();
    out
}

/// Kobo's sprite loader against Lunar Magic's: the clean ROM saved by Lunar
/// Magic, and the same with Kobo's loader in place of Lunar Magic's, play
/// seeded random sprite lists (both formats, up to 128 sprites, the
/// game's special loaders among them) in horizontal and vertical levels,
/// with every spawn range and smart spawning, along random camera paths;
/// every frame's sprite slots, load indexes, and load flags must be the
/// same (docs/lunar-magic-install.md, "Sprites").
#[test]
fn kobos_sprite_loader_spawns_as_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    loaders_spawn_alike(&asar, &lunar_magic, &clean, &clean, "loader");
}

/// The same on the clean ROM with SA-1 Pack applied, where both loaders
/// work with SA-1 Pack's own changes to the game's (docs/sa1.md).
#[test]
fn kobos_sprite_loader_spawns_as_lunar_magics_on_sa1() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let Some(base) = common::sa1_base(&clean) else {
        return;
    };
    loaders_spawn_alike(&asar, &lunar_magic, &clean, &base, "loader-sa1");
}

/// `base` saved by Lunar Magic, against the same with its sprite loader
/// group put back to `base`'s bytes and Kobo's applied. `name` keeps the
/// save's folder apart from other tests'.
fn loaders_spawn_alike(
    asar: &kobo_core::asar::Asar,
    lunar_magic: &Path,
    clean: &Rom,
    base: &Rom,
    name: &str,
) {
    let lm = save(lunar_magic, clean, base, name);
    let kobo = common::swap::swap(asar, common::swap::Piece::Sprites, &lm, base).unwrap();
    use kobo_core::source::level::{Sprite, Sprites};
    const IDS: [u8; 18] = [
        0x0D, 0x0F, 0x04, 0x05, 0x1C, 0x3E, 0x2F, 0x7B, 0xC9, 0xE0, 0xDE, 0xE2, 0x26, 0x10, 0x1D,
        0x09, 0xD5, 0x3F,
    ];
    let seed = common::env_number("KOBO_LOADER_SEED", 1);
    let count = common::env_number("KOBO_LOADER_CASES", 24);
    let mut rng = Lcg(seed);
    let mut ran = 0;
    for case in 0..count {
        let (level, vertical) = [
            (0x105, false),
            (0x106, false),
            (0x004, false),
            (0x1CE, true),
        ][rng.next(4) as usize];
        let new = rng.next(10) < 6;
        let n = if rng.next(10) < 8 {
            1 + rng.next(40)
        } else {
            85 + rng.next(44)
        };
        let mut list: Vec<Sprite> = (0..n)
            .map(|_| {
                let (x, y) = if vertical {
                    (rng.next(32) as u16, rng.next(80) as u16)
                } else {
                    (
                        rng.next(61) as u16,
                        rng.next(if new { 128 } else { 32 }) as u16,
                    )
                };
                Sprite {
                    id: IDS[rng.next(IDS.len() as u32) as usize],
                    x,
                    y,
                    extra_bits: [0, 0, 0, 1, 2, 3][rng.next(6) as usize],
                    extension: Vec::new(),
                }
            })
            .collect();
        list.sort_by_key(|s| if vertical { s.y / 16 } else { s.x / 16 });
        let header = base
            .read_u8(kobo_core::level::sprite_ptr(base, level).unwrap())
            .unwrap();
        let sprites = Sprites {
            memory: [0, 4, 8, 10, 14][rng.next(5) as usize],
            buoyancy: header & 0x80 != 0,
            buoyancy_no_layer2: header & 0x40 != 0,
            list,
        };
        let new = new || sprites.needs_new_system(vertical);
        let (header, entries) = sprites.to_entries(vertical, new);
        // A list the format cannot hold (too many sprites to a screen) is
        // left out; the cases that ran are counted below.
        let Ok(bytes) = kobo_core::sprites::encode(header, &entries, None) else {
            continue;
        };
        ran += 1;
        let ttt = rng.next(8) as u8;
        let mut route = vec![(0, 48, 352)];
        let (mut frame, mut x, mut y) = (0u32, 48i32, 352i32);
        while frame < 400 {
            frame += 20 + rng.next(130);
            x = (x + rng.next(1100) as i32 - 400).max(0);
            y = (y + rng.next(600) as i32 - 300).clamp(0, 420);
            route.push((frame, x, y));
        }
        let clear = rng.next(2) == 0;
        let a = sprite_frames(&with_sprites(&lm, level, &bytes, ttt), level, &route, clear);
        let b = sprite_frames(
            &with_sprites(&kobo, level, &bytes, ttt),
            level,
            &route,
            clear,
        );
        let first = (0..a.len()).find(|&f| a[f] != b[f]);
        assert_eq!(
            first, None,
            "case {case} (seed {seed}): level {level:03X}, tTT {ttt}, {n} sprites"
        );
    }
    eprintln!("{ran} of {count} cases ran; the rest could not be encoded");
    assert!(
        ran > 0 || count == 0,
        "no case of seed {seed} could be encoded"
    );
}

/// Kobo's palette fade at a level's end (`palette.asm`) leaves the same
/// palette as Lunar Magic's at every step of the fade: the clean ROM saved
/// by Lunar Magic, against the same with Kobo's patch over it, the fade
/// (`CODE_00AF35`) called with every value of its timer on a palette of
/// distinct colours and a fade copy of `$1555`s, so that a colour the fade
/// leaves, sets, or blends each shows.
#[test]
fn kobos_end_fade_is_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let bases = std::iter::once(copy).chain(common::sa1_base(&clean));
    for (base, name) in bases.zip(["fade", "fade-sa1"]) {
        let lm = save(&lunar_magic, &clean, &base, name);
        // Lunar Magic's palette hook is cleared first, so that the patch's
        // autoclean frees nothing of Lunar Magic's.
        let mut kobo = Rom::from_headerless(lm.data().to_vec()).unwrap();
        kobo.write(SnesAddr::new(0x00A5BF), &[0xFF; 4]).unwrap();
        let piece = *kobo_core::install::LUNAR_MAGIC
            .iter()
            .find(|(name, _)| *name == "palette.asm")
            .unwrap();
        let kobo = asar
            .patch(&kobo, &kobo_core::install::patch(piece))
            .unwrap()
            .rom;
        let fade = |rom: &Rom, timer: u8| {
            use kobo_core::expand::{self, Registers};
            use kobo_core::ram::{self, RamAddr};
            let at = |a: u32| RamAddr::new(0x7E_0000 | a);
            // `$00AF35` is the game's routine in the base the save started
            // from, which is where a call may enter (clean room).
            let (ram, _) = expand::call_in_level(
                rom,
                &base,
                0x105,
                |ram| {
                    ram.set_u8(ram::TRUE_FRAME, 0);
                    ram.set_u8(at(0x1495), timer);
                    for i in 0..0x100 {
                        let colour = (i * 0x1234 + 0x0421) as u16 & 0x7FFF;
                        ram.set_u16(at(0x0703 + 2 * i), colour);
                    }
                    for i in 0..0xF8 {
                        ram.set_u16(at(0x0905 + 2 * i), 0x1555);
                    }
                    ram.set_u16(at(0x0701), 0x7FFF);
                    ram.set_u16(at(0x0903), 0);
                },
                0x00_AF35,
                false,
                Registers {
                    p: 0x30,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut words: Vec<u16> = (0..0xF8).map(|i| ram.u16(at(0x0905 + 2 * i))).collect();
            words.push(ram.u16(at(0x0701)));
            words
        };
        for timer in (0..0x40).step_by(2) {
            assert_eq!(
                fade(&kobo, timer),
                fade(&lm, timer),
                "{name}, timer {timer:02X}"
            );
        }
    }
}

/// Kobo's code at the sites Lunar Magic's save leaves alone with `"LM"` at
/// `$05DD7C` (`entrance.asm`: the layer 2 collision at `$00E966`, a
/// vertical level's camera at `$00F77B`, game mode `$11`'s `$009708`)
/// plays as Lunar Magic's: the clean ROM saved by Lunar Magic, against the
/// same with Kobo's exit and entrance patches over it, through levels with
/// layer 2 interaction, a tide, and vertical scrolling, the player run by
/// the controller or carried, with the words those touch compared on every
/// frame.
#[test]
fn kobos_layer2_interaction_and_camera_are_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let bases = std::iter::once(copy).chain(common::sa1_base(&clean));
    for (base, name) in bases.zip(["camera", "camera-sa1"]) {
        let lm = save(&lunar_magic, &clean, &base, name);
        let kobo = with_kobos_entrances(&asar, &lm);
        // A level, the controller, and the player carried from X at DX
        // pixels a frame.
        type Run = (u16, u16, Option<(i32, i32)>);
        let runs: [Run; 8] = [
            (0x1CF, 0xC100, None),
            (0x0D4, 0x4100, None),
            (0x1E2, 0xC100, None),
            (0x0BE, 0x4100, None),
            (0x1CE, 0x8000, None),
            (0x0F7, 0, Some((0, 3))),
            (0x12A, 0, Some((480, -5))),
            (0x0C2, 0, Some((200, 1))),
        ];
        const WORDS: [u32; 26] = [
            0x1A, 0x1C, 0x1E, 0x20, 0x26, 0x28, 0x55, 0x72, 0x76, 0x7B, 0x7D, 0x94, 0x96, 0xF9,
            0x13CD, 0x142A, 0x1693, 0x17BC, 0x17BE, 0x0BE7, 0x0BE8, 0x0BEA, 0x0BEC, 0x0D89, 0x0D93,
            0x0DA0,
        ];
        let frames = |rom: &Rom, (level, pad, carry): Run| {
            use kobo_core::ram::RamAddr;
            let at = |a: u32| RamAddr::new(0x7E_0000 | a);
            let mut out = Vec::new();
            kobo_core::expand::play_game_loop(
                rom,
                level,
                300,
                |frame, ram| {
                    let pressed = frame % 16 == 0;
                    ram.set_u8(at(0x15), (pad >> 8) as u8);
                    ram.set_u8(at(0x17), pad as u8);
                    ram.set_u8(at(0x16), if pressed { (pad >> 8) as u8 } else { 0 });
                    ram.set_u8(at(0x18), if pressed { pad as u8 } else { 0 });
                    if let Some((x, dx)) = carry {
                        ram.set_u16(at(0x94), (x + dx * frame as i32) as u16);
                        ram.set_u8(at(0x1497), 0x7F);
                    }
                },
                |_, played| {
                    out.push(WORDS.map(|a| played.ram.u16(at(a))));
                },
            )
            .unwrap();
            out
        };
        for run in runs {
            let (a, b) = (frames(&lm, run), frames(&kobo, run));
            let first = (0..a.len()).find(|&f| a[f] != b[f]);
            assert_eq!(first, None, "{name}: level {:03X}", run.0);
        }
    }
}

/// Layer 2's vertical offset from layer 1 (`$1417`) as the entrance leaves
/// it, which the game's `CODE_00A796` sets by its own rates after the
/// entrance code: Kobo's entrance and camera against Lunar Magic's, in
/// level 105 of the clean ROM saved by Lunar Magic, with every layer 2
/// scroll nibble and every separate vertical setting (with and without
/// `H`), the layers placed relative to the player or not, played 60 frames
/// with the player carried up and right (docs/lunar-magic-install.md,
/// "Layer 2 scroll settings").
#[test]
fn kobos_layer2_offset_at_the_entrance_is_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let lm = save(&lunar_magic, &clean, &copy, "offset");
    let kobo = with_kobos_entrances(&asar, &lm);
    use kobo_core::ram::RamAddr;
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    // The level's byte in each table: the nibble's (and the Y position's),
    // the separate settings, and RL-ooooo (R: relative, bb $1A).
    let table = |t: u32| SnesAddr::new(t + LEVEL as u32);
    let low = clean.read_u8(table(0x05F000)).unwrap() & 0x0F;
    let mut cases = Vec::new();
    for relative in [0x9A, 0x1A] {
        for nibble in 0..16u8 {
            cases.push((nibble << 4 | low, 0x00, relative));
        }
        for setting in 0..0x20u8 {
            cases.push((low, 0x80 | setting, relative));
            cases.push((low, 0xC0 | setting, relative));
        }
    }
    const WORDS: [u32; 12] = [
        0x1A, 0x1C, 0x1E, 0x20, 0x28, 0x142A, 0x1413, 0x1417, 0x1443, 0x1445, 0x144A, 0x144C,
    ];
    let frames = |rom: &Rom, (nibble, separate, relative): (u8, u8, u8)| {
        let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
        rom.write_u8(table(0x05F000), nibble).unwrap();
        rom.write_u8(table(0x06FA00), separate).unwrap();
        rom.write_u8(table(0x06FC00), 0x00).unwrap();
        rom.write_u8(table(0x06FE00), relative).unwrap();
        let mut out = Vec::new();
        let mut start = None;
        kobo_core::expand::play_game_loop(
            &rom,
            LEVEL,
            60,
            |frame, ram| {
                let (x, y) = *start.get_or_insert((ram.u16(at(0x94)), ram.u16(at(0x96))));
                ram.set_u16(at(0x94), x + 2 * frame as u16);
                ram.set_u16(at(0x96), y.saturating_sub(2 * frame as u16));
                ram.set_u8(at(0x7D), 0);
                ram.set_u8(at(0x1497), 0x7F);
                ram.set_u8(at(0x1404), 1);
            },
            |_, played| out.push(WORDS.map(|a| played.ram.u16(at(a)))),
        )
        .unwrap();
        out
    };
    let mut failures = Vec::new();
    for case in cases {
        let (a, b) = (frames(&lm, case), frames(&kobo, case));
        if let Some(f) = (0..a.len()).find(|&f| a[f] != b[f]) {
            failures.push(format!(
                "{case:02X?}: frame {f}: {:04X?} / {:04X?}",
                a[f], b[f]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The camera's first place and its vertical scrolling turned off, which
/// the corpus play comparison found (docs/lunar-magic-install.md, "The
/// sites a save keeps with the marker"): Kobo's entrance code against Lunar
/// Magic's, in the clean ROM saved by Lunar Magic. Level 105 entered on
/// each of its last screens at each X, standing; and with vertical
/// scrolling off (setting 2, the player not flying) in level 105 at a
/// taller size and vertical level `0F7`, the player carried up and down;
/// layer 1's and 2's positions every frame.
#[test]
fn kobos_first_camera_and_scrolling_off_are_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let lm = save(&lunar_magic, &clean, &copy, "first-camera");
    let kobo = with_kobos_entrances(&asar, &lm);
    use kobo_core::ram::RamAddr;
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    type Edit = Vec<(u32, u8)>;
    // A level, ROM bytes to set, whether vertical scrolling is held off,
    // and the player's carry (pixels a frame up, then down).
    let frames = |rom: &Rom, level: u16, edits: &Edit, off: bool| {
        let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
        for &(addr, value) in edits {
            rom.write_u8(SnesAddr::new(addr), value).unwrap();
        }
        let mut out = Vec::new();
        let mut start = None;
        kobo_core::expand::play_game_loop(
            &rom,
            level,
            if off { 120 } else { 20 },
            |frame, ram| {
                ram.set_u8(at(0x1497), 0x7F);
                if off {
                    let (x, y) = *start.get_or_insert((ram.u16(at(0x94)), ram.u16(at(0x96))));
                    let k = if frame < 60 { frame } else { 120 - frame } as u16;
                    ram.set_u16(at(0x94), x);
                    ram.set_u16(at(0x96), y.saturating_sub(3 * k));
                    ram.set_u8(at(0x7D), 0);
                    ram.set_u8(at(0x1412), 2);
                    ram.set_u8(at(0x13F1), 0);
                }
            },
            |_, played| out.push([0x1A, 0x1C, 0x1E, 0x20, 0x5E].map(|a| played.ram.u16(at(a)))),
        )
        .unwrap();
        out
    };
    let mut cases: Vec<(u16, Edit, bool)> = Vec::new();
    // Level 105's main entrance: screen (the fourth table's low five bits)
    // and X (the second's low three).
    let table = |t: u32, level: u16| t + level as u32;
    let fourth = clean
        .read_u8(SnesAddr::new(table(0x05F600, LEVEL)))
        .unwrap();
    let second = clean
        .read_u8(SnesAddr::new(table(0x05F200, LEVEL)))
        .unwrap();
    for screen in [0x10, 0x12, 0x13] {
        for x in 0..8 {
            cases.push((
                LEVEL,
                vec![
                    (table(0x05F600, LEVEL), fourth & 0xE0 | screen),
                    (table(0x05F200, LEVEL), second & 0xF8 | x),
                ],
                false,
            ));
        }
    }
    // Level 105 at sizes of its own (the size table, $240 bytes before the
    // code the JSL at $05DA8A calls), and vertical level 0F7.
    let sizes = lm.read_u24(SnesAddr::new(0x05DA8B)).unwrap() - 0x240;
    for size in [0x00, 0x05, 0x0F, 0x1B] {
        cases.push((LEVEL, vec![(sizes + LEVEL as u32, size)], true));
    }
    cases.push((0x0F7, vec![], true));
    // With the game's bytes at $00F871 (LDY #$04 : BRA) the camera moves
    // otherwise in some of the cases with scrolling off.
    let mut game = Rom::from_headerless(kobo.data().to_vec()).unwrap();
    let site = SnesAddr::new(0x00F871);
    game.write(site, clean.read(site, 4).unwrap()).unwrap();
    let mut failures = Vec::new();
    let mut game_differs = false;
    for (level, edits, off) in cases {
        let (a, b) = (
            frames(&lm, level, &edits, off),
            frames(&kobo, level, &edits, off),
        );
        if let Some(f) = (0..a.len()).find(|&f| a[f] != b[f]) {
            failures.push(format!(
                "{level:03X} {edits:X?}: frame {f}: {:04X?} / {:04X?}",
                a[f], b[f]
            ));
        }
        game_differs |= off && frames(&game, level, &edits, off) != a;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(game_differs, "the game's $00F871 plays as Lunar Magic's");
}

/// `lm`, saved by Lunar Magic, with Kobo's exit and entrance patches over
/// it. The operands the patches autoclean are cleared first, so that they
/// free nothing of Lunar Magic's.
fn with_kobos_entrances(asar: &kobo_core::asar::Asar, lm: &Rom) -> Rom {
    common::swap::swap(asar, common::swap::Piece::Entrances, lm, lm).unwrap()
}

/// A screen exit in the game's format, Yoshi's wings, and the bonus game
/// lead where Lunar Magic's code takes them, bit 8 from the translevel
/// (docs/lunar-magic-install.md, "Entrances, exits, and midway points"): the
/// clean ROM saved by Lunar Magic, against the same with Kobo's exit and
/// entrance patches over it, each entered from every third translevel on
/// three submaps, and the special ones with flags of Lunar Magic's format
/// on the exit's screen.
#[test]
fn kobos_special_exits_are_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let bases = std::iter::once(copy).chain(common::sa1_base(&clean));
    for (base, name) in bases.zip(["special", "special-sa1"]) {
        let lm = save(&lunar_magic, &clean, &base, name);
        let kobo = with_kobos_entrances(&asar, &lm);
        use kobo_core::ram::RamAddr;
        let at = |a: u32| RamAddr::new(0x7E_0000 | a);
        let level = |rom: &Rom, flag: u32, translevel: u8, submap: u8, high: u8| {
            let ram = kobo_core::expand::enter_by_exit(rom, 0x20, high, false, submap, |ram| {
                if flag != 0 {
                    ram.set_u8(at(flag), 1);
                }
                ram.set_u8(at(0x13BF), translevel);
            })
            .unwrap();
            (ram.u8(at(0x0E)), ram.u8(at(0x0F)), ram.u8(at(0x192A)))
        };
        let mut seen = std::collections::BTreeSet::new();
        for flag in [0, 0x1B95, 0x1425] {
            for translevel in (0..=0xFFu8).step_by(3).chain([0x24, 0x25]) {
                let highs = if flag == 0 {
                    [0x00, 0x01, 0x00]
                } else {
                    [0x00, 0x05, 0x07]
                };
                for (submap, high) in [0, 1, 3].into_iter().zip(highs) {
                    let expected = level(&lm, flag, translevel, submap, high);
                    seen.insert(expected.1);
                    assert_eq!(
                        level(&kobo, flag, translevel, submap, high),
                        expected,
                        "{name}: ${flag:04X}, translevel {translevel:02X}, submap {submap}, flags {high:X}"
                    );
                }
            }
        }
        assert_eq!(seen.len(), 2, "{name}: both banks of levels");
    }
}

/// Kobo's VRAM patch (`vram.asm`) shows a lagging frame as Lunar Magic's
/// does: the clean ROM saved by Lunar Magic, against the same with the
/// patch's sites put back to the clean ROM's bytes and Kobo's patch
/// applied (as `examples/swap.rs vram` does), every other frame
/// lagging, with layer 1's and 2's scroll registers and tilemaps compared at
/// every vertical blank but the row above the level's top, which Lunar
/// Magic's patch fills and Kobo's leaves (docs/lunar-magic-install.md,
/// "Graphics"). Then without lag, with the camera skipping columns: the
/// player put at x 0 or far right on the first frame or a later one, then
/// carried right, and once with the ground shaking. And which tiles changed
/// in play each writes, around the camera.
#[test]
fn kobos_vram_patch_lags_as_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let copy = Rom::from_headerless(clean.data().to_vec()).unwrap();
    let bases = std::iter::once(copy).chain(common::sa1_base(&clean));
    for (base, name) in bases.zip(["lag", "lag-sa1"]) {
        let lm = save(&lunar_magic, &clean, &base, name);
        let mut kobo = Rom::from_headerless(lm.data().to_vec()).unwrap();
        for (start, end) in [
            (0x008072, 0x008074),
            (0x0081E2, 0x0081E5),
            (0x008209, 0x00820C),
            (0x0085D2, 0x0085DE),
            (0x00A5A2, 0x00A5A5),
            (0x00F6E4, 0x00F6E7),
            (0x0580A9, 0x0580AC),
            (0x0586F7, 0x0586FA),
            (0x00C116, 0x00C123),
        ] {
            let len = (end - start + 1) as usize;
            let bytes = base.read(SnesAddr::new(start), len).unwrap().to_vec();
            kobo.write(SnesAddr::new(start), &bytes).unwrap();
        }
        kobo.write(SnesAddr::new(0x0580BF), &[0xFF; 4]).unwrap();
        let piece = *kobo_core::install::LUNAR_MAGIC
            .iter()
            .find(|(name, _)| *name == "vram.asm")
            .unwrap();
        let kobo = asar
            .patch(&kobo, &kobo_core::install::patch(piece))
            .unwrap()
            .rom;
        // Every other frame lagging, the player running; or none lagging, the
        // player put at `x` on frame `from` and carried right, and the ground
        // shaking from then on with `shake`.
        let blanks = |rom: &Rom, level: u16, carried: Option<(u16, u32, bool)>| {
            use kobo_core::ram::RamAddr;
            let at = |a: u32| RamAddr::new(0x7E_0000 | a);
            let mut out = Vec::new();
            kobo_core::expand::play_game_loop_lagging(
                rom,
                level,
                200,
                |frame| carried.is_none() && frame % 2 == 1,
                |frame, ram| match carried {
                    None => {
                        ram.set_u8(at(0x15), 0xC1);
                        ram.set_u8(at(0x16), if frame % 16 == 0 { 0xC1 } else { 0 });
                    }
                    Some((x, from, shake)) => {
                        if frame >= from {
                            ram.set_u16(at(0x94), x + 3 * (frame - from) as u16);
                            ram.set_u8(at(0x1497), 0x7F);
                        }
                        if shake && frame == from {
                            ram.set_u8(at(0x1887), 0x40);
                        }
                    }
                },
                |_, played| {
                    // Tile rows 30 and 31 of each 32x32 part: the level's
                    // row -1.
                    let tilemaps: Vec<u8> = (0x3000..0x4000usize)
                        .filter(|w| (w & 0x3FF) >> 5 < 30)
                        .flat_map(|w| [played.vram[w * 2], played.vram[w * 2 + 1]])
                        .collect();
                    out.push((played.bg_scroll[..2].to_vec(), tilemaps));
                },
            )
            .unwrap();
            out
        };
        for level in [0x105, 0x1CF, 0x0F7, 0x0E7, 0x0C2] {
            let (a, b) = (blanks(&lm, level, None), blanks(&kobo, level, None));
            let first = (0..a.len()).find(|&i| a[i] != b[i]);
            assert_eq!(first, None, "{name}: level {level:03X}");
        }
        for (level, x, from, shake) in [
            (0x1E2, 0, 0, false),
            (0x1E2, 0x380, 0, false),
            (0x105, 0x600, 0, false),
            (0x1E2, 0, 20, false),
            (0x105, 0x600, 20, false),
            // Back to x $F8 on frame 1, then right: the camera turns within
            // a cell, and the column on its right edge is built again.
            (0x0ED, 0xF8, 1, false),
            // The ground shaking takes the camera a row up and back, which
            // builds the rows it shows.
            (0x105, 0x600, 0, true),
        ] {
            let a = blanks(&lm, level, Some((x, from, shake)));
            let b = blanks(&kobo, level, Some((x, from, shake)));
            let first = (0..a.len()).find(|&i| a[i] != b[i]);
            assert_eq!(
                first, None,
                "{name}: level {level:03X}, x {x:X} on frame {from}"
            );
        }
        // A tile changed in play shows in the rows the frame's builds keep
        // (cy to cy+14) and, on a layer that scrolls horizontally, the game's
        // window of columns (8 before the camera's to 23 after it): a tile
        // put either side of each edge after 20 frames, with the player held
        // where it entered, and whether its place in the tilemap changed
        // once the next frame has ended.
        let shows = |rom: &Rom, level: u16, x0: u16, dx: i32, dy: i32| {
            use kobo_core::expand::{self, Registers};
            use kobo_core::ram::RamAddr;
            let at = |a: u32| RamAddr::new(0x7E_0000 | a);
            let word = |tile: u8| {
                let slot = std::cell::Cell::new(0usize);
                let called = expand::call_after_frames(
                    rom,
                    &base,
                    level,
                    20,
                    |_, ram| {
                        ram.set_u16(at(0x94), x0);
                        ram.set_u8(at(0x1497), 0x7F);
                    },
                    |ram| {
                        let x = (ram.u16(at(0x1A)) as i32 / 16 + dx) * 16;
                        let y = ((ram.u16(at(0x1C)) as i32 + 1) / 16 + dy) * 16;
                        slot.set(
                            0x3000
                                | ((x as usize & 0x100) << 2)
                                | ((y as usize & 0xF0) << 2)
                                | ((x as usize & 0xF0) >> 3),
                        );
                        ram.set_u8(at(0x9C), tile);
                        ram.set_u16(at(0x9A), x as u16);
                        ram.set_u16(at(0x98), y as u16);
                    },
                    0x00_BEB0,
                    true,
                    Registers {
                        p: 0x30,
                        ..Default::default()
                    },
                    true,
                )
                .unwrap();
                let w = slot.get();
                [called.vram[2 * w], called.vram[2 * w + 1]]
            };
            // Tile 0 changes nothing; 5 (a used block) is in none of these.
            word(0x00) != word(0x05)
        };
        for (level, x0, dx, dy) in [
            (0x0ED, 0x4E0, -9, 4),
            (0x0ED, 0x4E0, -8, 4),
            (0x0ED, 0x4E0, 23, 4),
            (0x0ED, 0x4E0, 24, 4),
            (0x0ED, 0x4E0, 8, -1),
            (0x0ED, 0x4E0, 8, 0),
            (0x0C2, 0x80, 4, -1),
            (0x0C2, 0x80, 4, 0),
            (0x0C2, 0x80, 4, 14),
            (0x0C2, 0x80, 4, 15),
        ] {
            assert_eq!(
                shows(&lm, level, x0, dx, dy),
                shows(&kobo, level, x0, dx, dy),
                "{name}: level {level:03X}, a tile at {dx},{dy} from the camera"
            );
        }
    }
}

/// Level 105 with a graphics list of ExGFX files, in a 4bpp build.
fn graphics_project(clean: &Rom) -> Project {
    use kobo_core::exgfx::{self, GraphicsList, slot};
    let (mut level, _) = import::read_level(clean, LEVEL).unwrap();
    let mut list = GraphicsList::DEFAULT;
    list.0[slot::AN2] = exgfx::BYPASS | exgfx::LAYER3_FILES | exgfx::NO_FILE;
    list.0[slot::FG1] = 0x100;
    list.0[slot::SP1] = 0x80;
    list.0[slot::LG1] = 0x101;
    // A save writes `$EFFF` for SP4's `$FFFF` in a list with `G` set
    // (docs/lunar-magic-install.md); a file there stays.
    list.0[slot::SP4] = 0x02;
    level.graphics = Some(list);
    let file = |seed: u8, len: usize| -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
            .collect()
    };
    Project {
        root: PathBuf::from("."),
        levels: vec![(LEVEL, level)],
        exgfx: vec![
            (0x80, file(1, 0x1000)),
            (0x100, file(2, 0x1000)),
            (0x101, file(3, 0x800)),
        ],
        manifest: kobo_core::source::project::Manifest {
            four_bpp: true,
            bypass_lists: [(2, [0x80, 0x01, 0x13, 0x02])].into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// What of a graphics build differs after a save: the level's parts, and
/// `exgfx`, `gfx`, and `bypass lists` for the project-wide files.
fn graphics_differences(built: &Rom, saved: &Rom) -> Vec<String> {
    use kobo_core::exgfx;
    let mut out: Vec<String> = import::diff_levels(built, saved)
        .into_iter()
        .filter(|d| d.level == LEVEL)
        .flat_map(|d| d.parts)
        .collect();
    if import::read_exgfx_files(built).unwrap() != import::read_exgfx_files(saved).unwrap() {
        out.push("exgfx".into());
    }
    let clean = common::vanilla().unwrap();
    if import::read_gfx(built, &clean).unwrap() != import::read_gfx(saved, &clean).unwrap() {
        out.push("gfx".into());
    }
    if exgfx::read_old_lists(built).unwrap() != exgfx::read_old_lists(saved).unwrap() {
        out.push("bypass lists".into());
    }
    out
}

#[test]
fn a_graphics_build_survives_a_lunar_magic_save() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let project = graphics_project(&base);
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let saved = save(&lunar_magic, &clean, &built, &tag("graphics", sa1));
        let differences = graphics_differences(&built, &saved);
        assert!(
            differences.is_empty(),
            "after a save: {differences:?}: {:04X?} then {:04X?}",
            kobo_core::exgfx::read_list(&built, LEVEL).unwrap(),
            kobo_core::exgfx::read_list(&saved, LEVEL).unwrap()
        );

        // Each byte Lunar Magic checks put back to the clean ROM's, and what
        // the save then does (docs/lunar-magic-install.md, "Graphics").
        let mut found: Vec<(u32, Vec<String>)> = Vec::new();
        for (at, len) in [
            (0x00AAD8u32, 1usize),
            (0x00AA47, 1),
            (0x0583B8, 1),
            (0x0FF15C, 2),
        ] {
            let mut without = Rom::from_headerless(built.data().to_vec()).unwrap();
            let bytes = base.read(SnesAddr::new(at), len).unwrap().to_vec();
            without.write(SnesAddr::new(at), &bytes).unwrap();
            without.fix_checksum().unwrap();
            let mut saved = save(
                &lunar_magic,
                &clean,
                &without,
                &tag(&format!("graphics-{at:06X}"), sa1),
            );
            // Put back, so that Kobo reads the tables whatever the save did.
            let bytes = built.read(SnesAddr::new(at), len).unwrap().to_vec();
            saved.write(SnesAddr::new(at), &bytes).unwrap();
            found.push((at, graphics_differences(&built, &saved)));
        }
        // Without the marker the save installs Lunar Magic's own ExGFX code
        // over the tables and rewrites the lists; the other three matter to
        // its editor and exports, not to a save.
        let expected: Vec<(u32, Vec<String>)> = vec![
            (0x00AAD8, vec![]),
            (0x00AA47, vec![]),
            (0x0583B8, vec![]),
            (0x0FF15C, vec!["graphics".into(), "exgfx".into()]),
        ];
        assert_eq!(found, expected, "without each checked byte");
    }
}

/// A build with layer 3 settings in level 105's list (Kobo's layer 3 code
/// installed, its `JSL` at `$00A01F` the byte Lunar Magic checks): a save
/// keeps the list and Kobo's hooks, and plays the level's layer 3 the same.
/// Without the byte it installs its own layer 3 code over Kobo's hooks,
/// and the list still comes back.
#[test]
fn a_layer3_build_survives_a_lunar_magic_save() {
    use kobo_core::exgfx::{self, GraphicsList, Layer3Settings};
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let (mut level, _) = import::read_level(&base, LEVEL).unwrap();
        let mut list = GraphicsList::DEFAULT;
        list.set_layer3(&Layer3Settings {
            advanced: true,
            horizontal: 0x03,
            vertical: 0x06,
            x: 2,
            y: 5,
            subscreen: true,
            sync_fix: true,
            sprites_air: true,
            ..Layer3Settings::default()
        })
        .unwrap();
        level.graphics = Some(list);
        let project = Project {
            root: PathBuf::from("."),
            levels: vec![(LEVEL, level)],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let hooks = [(0x00A01Fu32, 6usize), (0x0194B6, 5), (0x05C40C, 5)];
        let at = |rom: &Rom, (a, n): (u32, usize)| rom.read(SnesAddr::new(a), n).unwrap().to_vec();
        let layer3 = |rom: &Rom| {
            use kobo_core::ram::RamAddr;
            let mut out = Vec::new();
            let mut start = None;
            kobo_core::expand::play_game_loop(
                rom,
                LEVEL,
                90,
                |frame, ram| {
                    let (x, y) = *start.get_or_insert((
                        ram.u16(RamAddr::new(0x7E_0094)),
                        ram.u16(RamAddr::new(0x7E_0096)),
                    ));
                    let x = x + 3 * frame as u16;
                    for (a, v) in [(0x94, x), (0x96, y), (0xD1, x), (0xD3, y)] {
                        ram.set_u16(RamAddr::new(0x7E_0000 | a), v);
                    }
                },
                |_, played| {
                    out.push((
                        played.ram.bytes(RamAddr::new(0x7E_0022), 4),
                        played.screen_layers,
                    ))
                },
            )
            .unwrap();
            out
        };
        let saved = save(&lunar_magic, &clean, &built, &tag("layer3", sa1));
        assert_eq!(
            exgfx::read_list(&saved, LEVEL).unwrap(),
            exgfx::read_list(&built, LEVEL).unwrap()
        );
        for hook in hooks {
            assert_eq!(at(&saved, hook), at(&built, hook), "{:06X}", hook.0);
        }
        assert_eq!(layer3(&saved), layer3(&built));

        let mut without = Rom::from_headerless(built.data().to_vec()).unwrap();
        let byte = base.read_u8(exgfx::LAYER3_CHECK).unwrap();
        without.write_u8(exgfx::LAYER3_CHECK, byte).unwrap();
        without.fix_checksum().unwrap();
        let saved = save(&lunar_magic, &clean, &without, &tag("layer3-check", sa1));
        assert_eq!(
            exgfx::read_list(&saved, LEVEL).unwrap(),
            exgfx::read_list(&built, LEVEL).unwrap()
        );
        assert_eq!(saved.read_u8(exgfx::LAYER3_CHECK).unwrap(), exgfx::JSL);
        for hook in hooks {
            assert_ne!(at(&saved, hook), at(&built, hook), "{:06X}", hook.0);
        }
    }
}

/// Whether crossing level `0EB`'s goal tape, placed for the secret exit
/// (extra bits 1), sets the secret exit flag (`$141C`): the player
/// walks right from the entrance through the game loop.
fn secret_goal_tape(rom: &Rom) -> bool {
    use kobo_core::ram::RamAddr;
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    let mut start = None;
    let mut secret = false;
    kobo_core::expand::play_game_loop(
        rom,
        0x0EB,
        260,
        |frame, ram| {
            let (x, y) = *start.get_or_insert((ram.u16(at(0x94)), ram.u16(at(0x96))));
            let x = x + frame as u16;
            for (addr, value) in [(0x94, x), (0x96, y), (0xD1, x), (0xD3, y)] {
                ram.set_u16(at(addr), value);
            }
            ram.set_u8(at(0x7B), 0);
            ram.set_u8(at(0x7D), 0);
        },
        |_, played| secret |= played.ram.u8(at(0x141C)) != 0,
    )
    .unwrap();
    secret
}

/// With PIXI too: a build with a PIXI sprite placed in level 105, saved by
/// Lunar Magic. The level, with the sprite's extension bytes, comes back,
/// and PIXI's code stays where PIXI put it, the sites it hooks and its
/// tables (docs/toolchain.md): a goal tape placed for the secret exit
/// still gives it, which it did not when a save installed Lunar Magic's
/// sprite group over PIXI's goal tape hook at `$01C089`.
#[test]
fn a_pixi_build_survives_a_lunar_magic_save() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() || common::tool(kobo_core::tools::Tool::Pixi).is_none() {
        return;
    }
    let dir = TempDir::new("lm-pixi");
    for (file, text) in [
        ("pixi/list.txt", "00 test.cfg\n"),
        (
            "pixi/sprites/test.cfg",
            "01\n36\n00 00 00 00 00 00\n00 00\ntest.asm\n2:0\n",
        ),
        (
            "pixi/sprites/test.asm",
            "print \"INIT \",pc\n    RTL\nprint \"MAIN \",pc\n    LDA #$42\n    STA $0DBF\n    RTL\n",
        ),
    ] {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let (mut level, _) = import::read_level(&base, LEVEL).unwrap();
        // In the list's order, which a save keeps by position.
        let at = level.sprites.list.iter().position(|s| s.x > 20).unwrap();
        level.sprites.list.insert(
            at,
            kobo_core::source::level::Sprite {
                id: 0x00,
                x: 20,
                y: 20,
                extra_bits: 2,
                extension: vec![0x12, 0x34],
            },
        );
        let project = Project {
            root: dir.to_path_buf(),
            manifest: kobo_core::source::project::Manifest {
                pixi: Some(PathBuf::from("pixi")),
                ..Default::default()
            },
            levels: vec![(LEVEL, level)],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let saved = save(&lunar_magic, &clean, &built, &tag("pixi", sa1));
        let parts: Vec<String> = import::diff_levels(&built, &saved)
            .into_iter()
            .filter(|d| d.level == LEVEL)
            .flat_map(|d| d.parts)
            .collect();
        assert!(parts.is_empty(), "level {LEVEL:X} after a save: {parts:?}");
        let sites: Vec<String> = [
            (0x01C089u32, 8usize),
            (0x02A856, 4),
            (0x02A8BB, 4),
            (0x02A936, 4),
            (0x02ABF2, 4),
            (0x02FAE9, 4),
            (0x02FFE2, 30),
            (0x05D8B9, 4),
            (0x0EF30C, 4),
            (0x0FFFE0, 1),
        ]
        .into_iter()
        .filter(|&(at, len)| {
            built.read(SnesAddr::new(at), len).unwrap()
                != saved.read(SnesAddr::new(at), len).unwrap()
        })
        .map(|(at, _)| format!("${at:06X}"))
        .collect();
        assert!(sites.is_empty(), "PIXI's sites after a save: {sites:?}");
        assert!(secret_goal_tape(&base), "the clean ROM's secret goal tape");
        assert!(secret_goal_tape(&built), "the build's secret goal tape");
        assert!(
            secret_goal_tape(&saved),
            "the secret goal tape after a save"
        );
    }
}

const TALL: u16 = 0x106;
/// Level 106 taller (not 105, which the save imports again, with Lunar
/// Magic setting its screen count), with blocks past its 32nd row (placed through screen
/// jumps with a vertical part), and the camera showing its last row whole.
fn taller_project(clean: &Rom) -> Project {
    use kobo_core::level::size::LevelSize;
    let (mut level, _) = import::read_level(clean, TALL).unwrap();
    level.size = LevelSize {
        mode: 0x0D,
        bottom_row: true,
        split: false,
    };
    level.header.screens = 4;
    level.layer1 = [(3, 40), (20, 55), (40, 10), (60, 55)]
        .into_iter()
        .map(|(x, y)| Object::Standard {
            number: 0x0D,
            x,
            y,
            settings: 0x00,
        })
        .collect();
    Project {
        root: PathBuf::from("."),
        levels: vec![(TALL, level)],
        ..Default::default()
    }
}

#[test]
fn a_taller_level_build_survives_a_lunar_magic_save() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let project = taller_project(&base);
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(
            import::read_level(&built, TALL).unwrap().0,
            project.levels[0].1
        );
        let parts = |saved: &Rom| -> Vec<String> {
            import::diff_levels(&built, saved)
                .into_iter()
                .filter(|d| d.level == TALL)
                .flat_map(|d| d.parts)
                .collect()
        };
        // The save keeps the size table, and Kobo's code at the piece's
        // sites (docs/lunar-magic-install.md, "Taller levels").
        let saved = save(&lunar_magic, &clean, &built, &tag("taller", sa1));
        assert!(
            parts(&saved).is_empty(),
            "after a save: {:?}",
            parts(&saved)
        );
        for (at, len) in [
            (0x05D9A1u32, 4usize),
            (0x05DA8A, 4),
            (0x00BDA8, 0x100),
            (0x00F478, 3),
            (0x0DA963, 4),
            (0x00F70D, 4),
            (0x00A2AF, 4),
        ] {
            let at = SnesAddr::new(at);
            assert_eq!(
                saved.read(at, len).unwrap(),
                built.read(at, len).unwrap(),
                "the taller levels' site {at} after a save"
            );
        }
        // Without the JSL at $05DA8A, Lunar Magic's check for the piece, the
        // save installs its own over the sites, with a new table: the level
        // loses its size.
        let mut without = Rom::from_headerless(built.data().to_vec()).unwrap();
        let bytes = base.read(SnesAddr::new(0x05DA8A), 4).unwrap().to_vec();
        without.write(SnesAddr::new(0x05DA8A), &bytes).unwrap();
        without.fix_checksum().unwrap();
        let saved = save(&lunar_magic, &clean, &without, &tag("taller-without", sa1));
        assert_eq!(parts(&saved), ["size"], "after a save without the JSL");
        let table = kobo_core::level::size::table;
        assert!(table(&saved).is_some() && table(&saved) != table(&built));
    }
}

/// Level 105 with its secondary entrance numbered 320, past the game's
/// tables, and a long screen exit to it on screen 4: a build moves the
/// entrance tables to hold 321, the last entrance in use (entrances.asm),
/// and Lunar Magic's save must read them, and keep the entrance and the
/// exit as Kobo wrote them.
#[test]
fn entrances_past_1ff_survive_a_lunar_magic_save() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let (mut level, _) = import::read_level(&base, LEVEL).unwrap();
        level.entrances[0].id = 0x320;
        level.entrances[0].settings.water = true;
        level
            .layer1
            .push(Object::ScreenExit(ScreenExit::lunar_magic(
                4,
                ScreenExit::SECONDARY,
                0x320,
            )));
        let project = Project {
            levels: vec![(LEVEL, level.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, LEVEL).unwrap().0, level);
        let saved = save(&lunar_magic, &clean, &built, &tag("entrances", sa1));
        let parts: Vec<String> = import::diff_levels(&built, &saved)
            .into_iter()
            .filter(|d| d.level == LEVEL)
            .flat_map(|d| d.parts)
            .collect();
        assert!(parts.is_empty(), "level {LEVEL:X} after a save: {parts:?}");
        // The save sizes the tables as the build does, to the last
        // entrance in use (lunar-magic-install.md), and keeps every entrance.
        assert_eq!(kobo_core::level::entrance_count(&built), 0x321);
        assert_eq!(kobo_core::level::entrance_count(&saved), 0x321);
        let ours = kobo_core::level::read_entrances(&built).unwrap();
        let theirs = kobo_core::level::read_entrances(&saved).unwrap();
        let differ: Vec<usize> = (0..ours.len()).filter(|&i| ours[i] != theirs[i]).collect();
        assert!(differ.is_empty(), "entrances after a save: {differ:03X?}");
        // And the exit leads where it did.
        let entered = |rom: &Rom| {
            let ram = kobo_core::expand::enter_by_exit(rom, 0x20, 0x17, false, 0, |_| {}).unwrap();
            [0x0E, 0x0F, 0x192A, 0x94, 0x95, 0x96, 0x97, 0x1C, 0x20]
                .map(|a| ram.u8(kobo_core::ram::RamAddr::new(0x7E_0000 | a)))
        };
        assert_eq!(entered(&saved), entered(&built));
        assert_eq!(entered(&built)[..2], [0x05, 0x01]);
    }
}

/// The two checked bytes of the background and level number pieces
/// (lunar-magic-install.md, "Bytes Kobo writes because Lunar Magic checks
/// them"): a build whose level has a background in Lunar Magic's layout,
/// saved with the `JML` at `$0EF519` and the level number hook's code at
/// `$0EF550`, and without each.
#[test]
fn the_background_and_level_number_checks() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::source::level::{BackgroundTiles, Layer2};
    let (mut level, _) = import::read_level(&clean, LEVEL).unwrap();
    let tiles = (0..32 * 32).map(|i| (i as u16 * 7) % 0x200).collect();
    level.layer2 = Layer2::Background(BackgroundTiles {
        table: 0,
        rows: 32,
        tiles,
    });
    let project = Project {
        root: PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(LEVEL, level)],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let parts = |rom: &Rom| -> Vec<String> {
        import::diff_levels(&built, rom)
            .into_iter()
            .filter(|d| d.level == LEVEL)
            .flat_map(|d| d.parts)
            .collect()
    };
    let area = |rom: &Rom| rom.read(SnesAddr::new(0x0EF550), 0x1C).unwrap().to_vec();
    // With both, the save keeps the level's background and Kobo's code.
    let saved = save(&lunar_magic, &clean, &built, "bg-with");
    assert!(parts(&saved).is_empty(), "{:?}", parts(&saved));
    assert_eq!(area(&saved), area(&built));
    // Without the JML at $0EF519 it loses the level's background.
    let mut no_jml = Rom::from_headerless(built.data().to_vec()).unwrap();
    no_jml.write_u8(SnesAddr::new(0x0EF519), 0xFF).unwrap();
    no_jml.fix_checksum().unwrap();
    let saved = save(&lunar_magic, &clean, &no_jml, "bg-no-jml");
    assert_eq!(parts(&saved), ["layer2", "background"]);
    // Without code at $0EF550 it puts its own there, the hook kept, and
    // the level reads the same.
    let mut no_level = Rom::from_headerless(built.data().to_vec()).unwrap();
    no_level
        .write(SnesAddr::new(0x0EF550), &[0xFF; 0x1C])
        .unwrap();
    no_level.fix_checksum().unwrap();
    let saved = save(&lunar_magic, &clean, &no_level, "bg-no-level");
    assert!(parts(&saved).is_empty(), "{:?}", parts(&saved));
    let hook = SnesAddr::new(0x05D8E2);
    assert_eq!(saved.read(hook, 4).unwrap(), built.read(hook, 4).unwrap());
    assert!(area(&saved).iter().any(|&b| b != 0xFF));
    assert_ne!(area(&saved), area(&built));
}
