//! The vanilla round trip: every vanilla level imported as text and built
//! back into a ROM, with its layer data in the expanded ROM, reads back as
//! the same level and renders as vanilla does. The full picture check of
//! all 512 levels is `render_hashes` (docs/testing.md); this renders a
//! sample of level kinds.

mod common;

use common::temp::TempDir;
#[path = "common/object_cases.rs"]
mod object_cases;

use std::fs;
use std::path::{Path, PathBuf};

use kobo_core::bps;
use kobo_core::build::{self, Project};
use kobo_core::import;
use kobo_core::level::{self, tables};
use kobo_core::render::{self, RenderOptions};
use kobo_core::source::level::{Comments, Layer2, Level};
use kobo_core::{Rom, SnesAddr};

fn temp_dir(name: &str) -> TempDir {
    TempDir::unmade(name)
}

fn level_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir.join("levels"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn vanilla_imports_and_builds_back() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = temp_dir("import-all");
    let report = import::import_rom(&clean, &clean, &dir, true).unwrap();
    assert_eq!(report.levels.len(), 512);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert!(report.unmodelled.is_empty() && report.unread_blocks.is_empty());

    // Kobo's formatting is a fixed point.
    for file in level_files(&dir) {
        let text = fs::read_to_string(&file).unwrap();
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.to_toml(&comments), text, "{}", file.display());
    }

    let project = Project::load(&dir).unwrap();
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(
        build::build(&clean, &project).unwrap().data(),
        built.data(),
        "same inputs, same output"
    );
    assert!(built.internal_header().checksum_pair_valid());
    assert_eq!(built.internal_header().checksum, built.compute_checksum());

    for number in 0..level::LEVEL_COUNT {
        let (read, _) = import::read_level(&built, number).unwrap();
        let (vanilla, _) = import::read_level(&clean, number).unwrap();
        assert_eq!(read, vanilla, "level {number:03X}");
        let layer1 = level::layer1_ptr(&built, number).unwrap();
        assert!(layer1.bank() >= 0x10, "level {number:03X} at {layer1}");
    }
    // A build of the imported vanilla ROM imports as no changes at all.
    let again = temp_dir("reimport");
    let built_path = again.with_extension("sfc");
    built.save(&built_path).unwrap();
    let rebuilt = Rom::load(&built_path).unwrap();
    let report = import::import_rom(&rebuilt, &clean, &again, false).unwrap();
    assert!(report.levels.is_empty());
    // What the build changed is all level data and tables the import
    // reads, so nothing is left unaccounted for.
    assert_eq!(report.unmodelled, []);
    assert_eq!(report.unread_blocks, []);

    // Level kinds: horizontal with a background, vertical, layer 2
    // objects (horizontal and vertical), a boss arena, the title screen.
    for number in [0x105, 0x0D3, 0x01C, 0x0CB, 0x0E5, 0x1C7, 0x0C7] {
        for options in [
            RenderOptions::default(),
            RenderOptions {
                sprites: render::Sprites::Markers,
                player: false,
                hidden_layers: 0,
            },
        ] {
            let a = render::render_level(&clean, number, options).unwrap().image;
            let b = render::render_level(&built, number, options).unwrap().image;
            assert!(
                a.pixels == b.pixels,
                "level {number:03X} renders differently"
            );
        }
    }
    // Every level, with the checks of every level on.
    common::every_level_draws_the_same(&clean, &built, &[], "the vanilla import, built");
    let _ = fs::remove_file(&built_path);
}

#[test]
fn an_empty_project_builds_the_clean_rom() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        ..Default::default()
    };
    assert_eq!(build::build(&clean, &project).unwrap().data(), clean.data());
}

#[test]
fn edits_reach_the_rom() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    level.layer1.truncate(10);
    level.sprites.list[0].x += 1;
    let text = level.to_toml(&Comments::default());
    let (level, _) = Level::from_toml(&text).unwrap();
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level.clone())],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
    // The changed sprite list went to bank $07's unused space; the other
    // levels kept theirs.
    assert_eq!(
        level::sprite_ptr(&built, 0x105).unwrap(),
        SnesAddr::new(0x07A179)
    );
    assert_eq!(
        built.read_u16(tables::SPRITE_PTRS.add(2 * 0x106)).unwrap(),
        clean.read_u16(tables::SPRITE_PTRS.add(2 * 0x106)).unwrap()
    );
    assert_eq!(
        level::layer1_ptr(&built, 0x106).unwrap(),
        level::layer1_ptr(&clean, 0x106).unwrap()
    );
    assert!(built.read(SnesAddr::new(0x108000), 4).unwrap() == b"STAR");
}

/// An SA-1 image past 4 MiB (SA-1 Pack's 6 and 8 MiB patches, Asar's
/// `fullsa1rom`) takes Kobo's code for Lunar Magic's layout too: a level
/// with Lunar Magic's objects, a custom palette, and a taller size builds,
/// loads them, and reads back.
#[test]
fn lunar_magic_layout_builds_past_4_mib() {
    use kobo_core::level::objects::Object;
    use kobo_core::level::size::LevelSize;
    use kobo_core::palette::{Color15, CustomPalette, Palette};
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::sa1_base(&clean).is_none() {
        return;
    }
    let (mut level, _) = import::read_level(&common::base_as(&clean, true), 0x105).unwrap();
    level.layer1.push(Object::Lunar {
        number: 0x22,
        x: 1,
        y: 1,
        data: vec![0x01, 0xAB],
    });
    level.size = LevelSize {
        mode: 0x03,
        bottom_row: false,
        split: false,
    };
    let mut palette = Palette::default();
    for (i, color) in palette.colors.iter_mut().enumerate() {
        *color = Color15::from_rgb5(i as u8 % 32, (i / 8) as u8 % 32, 31 - i as u8 % 32);
    }
    level.palette = Some(CustomPalette {
        back_area: Color15::from_rgb5(3, 5, 7),
        palette: palette.clone(),
    });
    for size in [0x60_0000, 0x80_0000] {
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: kobo_core::source::project::Manifest {
                sa1: true,
                rom_size: Some(size),
                ..Default::default()
            },
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        let built = build::build(&clean, &project).unwrap();
        assert_eq!(built.len(), size);
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        let loaded = kobo_core::expand::expand_level(&built, 0x105).unwrap();
        let at = |x, y| loaded.tiles.tile_at(x, y);
        assert_eq!([at(1, 1), at(2, 1)], [0x0AB, 0x0AB], "{size:X}");
        assert_eq!(
            loaded.ram.u16(kobo_core::ram::RamAddr::new(0x7E_13D7)),
            LevelSize {
                mode: 0x03,
                bottom_row: false,
                split: false,
            }
            .height(),
            "{size:X}"
        );
        // The load uploads the palette, as the game then changes a few
        // colours.
        let cgram = &loaded.video.cgram;
        let same = (0..256)
            .filter(|&i| {
                u16::from_le_bytes([cgram[2 * i], cgram[2 * i + 1]]) == palette.colors[i].0
            })
            .count();
        assert!(
            same > 240,
            "{size:X}: {same} of 256 colours are the level's"
        );
    }
}

#[test]
fn lunar_magic_objects_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::level::objects::{Object, ScreenExit};
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        // A 2x1 of tile $0AB, and a 2x2 block of Map16 from $1A0.
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
            data: vec![0x11, 0x41, 0xA0],
        });
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        let tiles = kobo_core::expand::expand_level(&built, 0x105)
            .unwrap()
            .tiles;
        let at = |x, y| tiles.tile_at(x, y);
        assert_eq!([at(1, 1), at(2, 1)], [0x0AB, 0x0AB]);
        assert_eq!(
            [at(4, 1), at(5, 1), at(4, 2), at(5, 2)],
            [0x1A0, 0x1A1, 0x1B0, 0x1B1]
        );

        // The music bypass, song $0A.
        let mut with_music = level.clone();
        with_music
            .layer1
            .push(Object::Unplaced(vec![0x40, 0x60, 0x0B]));
        let project = Project {
            levels: vec![(0x105, with_music)],
            ..project
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let ram = kobo_core::expand::expand_level(&built, 0x105).unwrap().ram;
        assert_eq!(ram.u8(kobo_core::ram::RamAddr::new(0x7E_0DDA)), 0x0A);

        // A music bypass alone still needs Kobo's code: without it the game
        // takes the object for one of its own and draws tiles.
        let (vanilla, _) = import::read_level(&base, 0x105).unwrap();
        let mut music_only = vanilla.clone();
        music_only
            .layer1
            .push(Object::Unplaced(vec![0x40, 0x60, 0x0B]));
        let project = Project {
            levels: vec![(0x105, music_only)],
            ..project
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        let loaded = kobo_core::expand::expand_level(&built, 0x105).unwrap();
        assert_eq!(loaded.ram.u8(kobo_core::ram::RamAddr::new(0x7E_0DDA)), 0x0A);
        let vanilla_tiles = kobo_core::expand::expand_level(&base, 0x105).unwrap().tiles;
        assert!(loaded.tiles.low == vanilla_tiles.low && loaded.tiles.high == vanilla_tiles.high);

        // Its graphics bypass builds, with Kobo's graphics code
        // (tests/graphics_build.rs).
        level.layer1.push(Object::Unplaced(vec![0x40, 0x40, 0x00]));
        let project = Project {
            levels: vec![(0x105, level.clone())],
            ..project
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert!(kobo_core::exgfx::has_exgfx(&built));
        // An exit in its format builds, with Kobo's exit hooks.
        level.layer1.pop();
        level.layer1.push(Object::ScreenExit(ScreenExit {
            screen: 1,
            flags: ScreenExit::LUNAR_MAGIC | ScreenExit::HIGH,
            destination: 0x06,
        }));
        let project = Project {
            levels: vec![(0x105, level.clone())],
            ..project
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert!(kobo_core::level::LevelFormat::of(&built).entrances);
        let written = kobo_core::level::read_objects(&built, 0x105).unwrap();
        assert_eq!(written.layer1.objects.last(), level.layer1.last());
    }
}

/// Every form of Lunar Magic's placed objects, in a horizontal and a
/// vertical level, built with Kobo's code gives the grid Lunar Magic's own
/// import of the same objects gave (`fixtures/lunar_magic_objects.txt`,
/// from `examples/lm_objects.rs`). A user object (`2D`), which draws
/// nothing without the user's code, leaves the rest of the level as it was.
#[test]
fn lunar_magic_object_forms_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::level::objects::Object;
        let fixture: std::collections::HashMap<&str, &str> =
            include_str!("fixtures/lunar_magic_objects.txt")
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .map(|l| {
                    let (hash, name) = l.split_once(' ').unwrap();
                    (name, hash)
                })
                .collect();
        let build_level = |number: u16, level: Level| {
            let project = Project {
                root: std::path::PathBuf::from("."),
                manifest: Default::default(),
                levels: vec![(number, level)],
                ..Default::default()
            };
            let built = common::build_as(&clean, &project, sa1).unwrap();
            kobo_core::expand::expand_level(&built, number)
                .unwrap()
                .tiles
        };
        let mut checked = 0;
        let mut wrong = Vec::new();
        for group in 0..3 {
            let number = if group == 2 { 0x1CE } else { 0x105 };
            let (mut level, _) = import::read_level(&base, number).unwrap();
            level.header.screens = object_cases::screens(group);
            level.layer1 = object_cases::objects(group);
            let tiles = build_level(number, level);
            for (name, cells) in object_cases::grids(&tiles, group) {
                checked += 1;
                if fixture.get(name) != Some(&object_cases::hash(&cells).as_str()) {
                    wrong.push(format!("{name}: {cells}"));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "grids unlike Lunar Magic's:\n{}",
            wrong.join("\n")
        );
        assert_eq!(checked, fixture.len());

        let (vanilla, _) = import::read_level(&base, 0x105).unwrap();
        let mut user = vanilla.clone();
        user.layer1.insert(
            0,
            Object::Lunar {
                number: 0x2D,
                x: 3,
                y: 5,
                data: vec![0x12, 0x34, 0x56],
            },
        );
        let (with, without) = (build_level(0x105, user), build_level(0x105, vanilla));
        assert!(with.low == without.low && with.high == without.high);
    }
}

#[test]
fn a_background_changed_in_place_is_not_the_clean_roms() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    // Level 105 points at one of Nintendo's backgrounds. Change one byte
    // of the stream so that it still decodes, to different tiles, and
    // keep the pointer: the level file can only name the clean ROM's.
    let original = level::read_background(&clean, 0x105).unwrap().unwrap();
    let hacked = (0..original.stream_len)
        .find_map(|offset| {
            let mut rom = Rom::from_bytes(clean.data().to_vec()).unwrap();
            let at = original.address.add(offset as u32);
            let byte = rom.read_u8(at).unwrap();
            rom.write_u8(at, byte ^ 0x01).unwrap();
            let changed = level::read_background(&rom, 0x105).ok()??;
            (changed.stream_len == original.stream_len
                && changed.data.len() == original.data.len()
                && changed.data != original.data)
                .then_some(rom)
        })
        .expect("a byte whose change alters the tiles");
    assert_eq!(
        import::read_level(&hacked, 0x105).unwrap().0,
        import::read_level(&clean, 0x105).unwrap().0,
        "the level file cannot tell the difference"
    );

    let dir = temp_dir("background-changed");
    let report = import::import_rom(&hacked, &clean, &dir, false).unwrap();
    assert!(report.levels.contains(&0x105), "{:?}", report.levels);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.starts_with("level 105:") && n.contains("background")),
        "{:?}",
        report.notes
    );
    let diffs = import::diff_levels(&hacked, &clean);
    assert!(
        diffs
            .iter()
            .any(|d| d.level == 0x105 && d.parts == ["background"]),
        "{diffs:?}"
    );
}

#[test]
fn a_level_that_does_not_read_is_left_out_of_an_import() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    // Level 104's layer 1 moved to the last bytes of the ROM, its header
    // and then objects that run off the end with no terminator.
    let mut hacked = Rom::from_bytes(clean.data().to_vec()).unwrap();
    let header = clean
        .read(level::layer1_ptr(&clean, 0x104).unwrap(), 5)
        .unwrap()
        .to_vec();
    let at = SnesAddr::new(0x0FFFF0);
    hacked.write(at, &header).unwrap();
    hacked.write(at.add(5), &[0x01; 11]).unwrap();
    hacked
        .write_ptr(tables::LAYER1_PTRS.add(3 * 0x104), at)
        .unwrap();
    assert!(import::read_level(&hacked, 0x104).is_err());

    let dir = temp_dir("unreadable-level");
    let report = import::import_rom(&hacked, &clean, &dir, true).unwrap();
    assert!(!report.levels.contains(&0x104));
    assert_eq!(report.levels.len(), 511);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.starts_with("level 104: not imported")),
        "{:?}",
        report.notes
    );
}

#[test]
fn a_background_encoded_another_way_is_no_difference() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let original = level::read_background(&clean, 0x105).unwrap().unwrap();
    // Level 105's background, encoded by Kobo's compressor with one more
    // byte after the tiles the level uses, and written over Nintendo's
    // stream: more data, the same tiles. `diff` compares the tiles.
    let mut stream = kobo_core::compress::rle1::compress(&original.data).unwrap();
    stream.truncate(stream.len() - 2);
    stream.extend([0x00, 0xAA, 0xFF, 0xFF]);
    assert!(stream.len() <= original.stream_len, "the stream must fit");
    let mut rom = Rom::from_bytes(clean.data().to_vec()).unwrap();
    rom.write(original.address, &stream).unwrap();
    let encoded = level::read_background(&rom, 0x105).unwrap().unwrap();
    assert_ne!(encoded.data, original.data);
    assert_eq!(encoded.tiles(), original.tiles());
    assert!(
        !import::diff_levels(&rom, &clean)
            .iter()
            .any(|d| d.level == 0x105),
    );
}

#[test]
fn layer_2_must_match_the_level_mode() {
    use kobo_core::source::level::Layer2;
    let base = common::synthetic_base();
    let (level, _) = Level::from_toml(include_str!("fixtures/synthetic_level.toml")).unwrap();
    let build_with = |edit: &dyn Fn(&mut Level)| {
        let mut level = level.clone();
        edit(&mut level);
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level)],
            ..Default::default()
        };
        build::build_on(&base, &project, None).map(|_| ())
    };
    // Mode 0 loads a background: the fixture has one.
    build_with(&|_| {}).unwrap();
    let error = build_with(&|l| l.layer2 = Layer2::None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("loads a background on layer 2"), "{error}");
    let error = build_with(&|l| l.layer2 = Layer2::Objects(Vec::new()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("the file has objects"), "{error}");
    // Mode 1 loads layer 2 objects; mode 9, a boss arena, nothing.
    let error = build_with(&|l| l.header.level_mode = kobo_core::level::LevelMode(0x01))
        .unwrap_err()
        .to_string();
    assert!(error.contains("loads objects on layer 2"), "{error}");
    build_with(&|l| {
        l.header.level_mode = kobo_core::level::LevelMode(0x01);
        l.layer2 = Layer2::Objects(Vec::new());
    })
    .unwrap();
    build_with(&|l| {
        l.header.level_mode = kobo_core::level::LevelMode(0x09);
        l.layer2 = Layer2::None;
    })
    .unwrap();
}

#[test]
fn lunar_magic_backgrounds_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::source::level::{BACKGROUND_ROWS, BackgroundTiles};
        let (level, _) = import::read_level(&base, 0x105).unwrap();
        // Tiles from the game's BG Map16, made up from their place.
        let tile = |i: usize| ((i * 7) % 0x200) as u16;
        for rows in [32, 27] {
            let high = |i: usize| {
                if rows == 27 {
                    0x100 | tile(i) & 0xFF
                } else {
                    tile(i)
                }
            };
            let tiles: Vec<u16> = (0..BACKGROUND_ROWS * 32)
                .map(|i| if i / 32 < rows { high(i) } else { 0 })
                .collect();
            let mut level = level.clone();
            level.layer2 = Layer2::Background(BackgroundTiles {
                table: 0,
                rows,
                tiles: tiles.clone(),
            });
            let project = Project {
                root: std::path::PathBuf::from("."),
                manifest: Default::default(),
                levels: vec![(0x105, level.clone())],
                ..Default::default()
            };
            let built = common::build_as(&clean, &project, sa1).unwrap();
            assert_eq!(
                import::read_level(&built, 0x105).unwrap().0,
                level,
                "{rows} rows"
            );
            let loaded = kobo_core::expand::expand_level(&built, 0x105)
                .unwrap()
                .tiles;
            let (low, high_plane) = loaded.layer2_tilemap.clone().unwrap();
            let screen = loaded.layer2_screen_len;
            assert_eq!(screen, if rows == 32 { 0x200 } else { 0x1B0 });
            for row in 0..rows {
                for half in 0..2 {
                    for col in 0..16 {
                        let at = half * screen + row * 16 + col;
                        let got = u16::from_le_bytes([low[at], high_plane[at]]);
                        assert_eq!(
                            got,
                            tiles[row * 32 + half * 16 + col],
                            "{rows} rows: {row},{half},{col}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn custom_palettes_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::palette::{Color15, CustomPalette, Palette};
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        let mut palette = Palette::default();
        for (i, color) in palette.colors.iter_mut().enumerate() {
            *color = Color15::from_rgb5(i as u8 % 32, (i / 8) as u8 % 32, 31 - i as u8 % 32);
        }
        let custom = CustomPalette {
            back_area: Color15::from_rgb5(3, 5, 7),
            palette,
        };
        level.palette = Some(custom.clone());
        let text = level.to_toml(&Comments::default());
        assert_eq!(Level::from_toml(&text).unwrap().0, level);
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        let loaded = kobo_core::expand::expand_level(&built, 0x105).unwrap();
        let cgram = &loaded.video.cgram;
        // The load uploads the palette, as the game then changes a few colours.
        let same = (0..256)
            .filter(|&i| {
                u16::from_le_bytes([cgram[2 * i], cgram[2 * i + 1]]) == custom.palette.colors[i].0
            })
            .count();
        assert!(same > 240, "{same} of 256 colours are the level's");
    }
}

#[test]
fn gfx_files_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    use kobo_core::gfx;
    let reader = gfx::GfxReader::new(&clean).unwrap();
    // A 3bpp file, the packed Mode 7 one, and one of the pair sharing a bank.
    let mut images = Vec::new();
    for index in [0x01u8, 0x27, 0x32] {
        let file = reader.read(index).unwrap();
        let mut tiles = file.tiles();
        for (i, tile) in tiles.iter_mut().enumerate() {
            tile.pixels[i % 8][(i / 8) % 8] = (i % file.colors()) as u8;
        }
        images.push((index, gfx::tiles_to_image(&tiles, file.colors()), tiles));
    }
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        gfx: images
            .iter()
            .map(|(i, image, _)| (*i, image.clone()))
            .collect(),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let back = gfx::GfxReader::new(&built).unwrap();
    for (index, _, tiles) in &images {
        assert_eq!(&back.read(*index).unwrap().tiles(), tiles, "GFX{index:02X}");
    }
    // GFX33 keeps its tiles, moved along with GFX32.
    assert_eq!(
        back.read(0x33).unwrap().data,
        reader.read(0x33).unwrap().data
    );
    let (read, notes) = import::read_gfx(&built, &clean).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(read.len(), 3);

    // The palette a file is viewed with is no input; its pixels are.
    let keys = build::stage_keys(&clean, &project).unwrap();
    let mut recoloured = project.clone();
    recoloured.gfx[0].1.palette[1] = [0x12, 0x34, 0x56];
    assert_eq!(build::stage_keys(&clean, &recoloured).unwrap(), keys);
    let mut edited = project.clone();
    edited.gfx[0].1.pixels[0] ^= 1;
    assert_ne!(build::stage_keys(&clean, &edited).unwrap(), keys);
}

#[test]
fn a_long_exit_leads_to_an_entrance_past_1ff() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::level::objects::{Object, ScreenExit};
    use kobo_core::ram::RamAddr;
    let byte = |ram: &kobo_core::ram::Ram, a: u32| ram.u8(RamAddr::new(0x7E_0000 | a));
    // What entering an entrance leaves: the level, its action, the
    // player's and the layers' positions.
    let entered = |ram: &kobo_core::ram::Ram| -> Vec<u8> {
        [
            0x0E, 0x0F, 0x192A, 0x94, 0x95, 0x96, 0x97, 0x1C, 0x1D, 0x20, 0x21,
        ]
        .iter()
        .map(|&a| byte(ram, a))
        .collect()
    };
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        // Level 105's entrance renumbered 320, and level 106 given a long
        // exit to it on screen 0, as riff2 has them (lunar-magic.md).
        let (mut l105, _) = import::read_level(&base, 0x105).unwrap();
        let old = l105.entrances[0];
        let mut entrance = old;
        entrance.id = 0x320;
        l105.entrances = vec![entrance];
        let (mut l106, _) = import::read_level(&base, 0x106).unwrap();
        l106.layer1.push(Object::ScreenExit(ScreenExit::lunar_magic(
            0,
            ScreenExit::SECONDARY,
            0x320,
        )));
        let project = Project {
            levels: vec![(0x105, l105.clone()), (0x106, l106.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, l105);
        assert_eq!(import::read_level(&built, 0x106).unwrap().0, l106);
        // The exit, as the object leaves it, enters 105 as the entrance did
        // under its old number on the base ROM.
        let ram = kobo_core::expand::enter_by_exit(&built, 0x20, 0x17, false, 0, |_| {}).unwrap();
        let before = kobo_core::expand::enter_by_exit(
            &base,
            old.id as u8,
            (old.id >> 8) as u8,
            true,
            1,
            |_| {},
        )
        .unwrap();
        assert_eq!(entered(&ram), entered(&before), "sa1 {sa1}");
        assert_eq!(byte(&ram, 0x0E), 0x05);
        // Loading level 106 leaves the long exit where the exit code reads it.
        let loaded = kobo_core::expand::expand_level(&built, 0x106).unwrap();
        assert_eq!(loaded.ram.u8(RamAddr::new(0x7E_19B8)), 0x20);
        assert_eq!(loaded.ram.u8(RamAddr::new(0x7E_19D8)), 0x17);
    }
}

#[test]
fn entrances_in_lunar_magic_format_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        // Level 105 given entrance 0CB, whose number says bank 0: only Lunar
        // Magic's format, with the exit hook installed, can lead it there.
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        let mut entrance = level.entrances[0];
        entrance.id = 0x0CB;
        level.entrances = vec![entrance];
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        // A secondary exit to 0CB, in either format, reaches 105.
        for high in [0x06u8, 0x02] {
            let ram = kobo_core::expand::enter_by_exit(&built, 0xCB, high, high == 0x02, 0, |_| {})
                .unwrap();
            let level = ram.u16(kobo_core::ram::RamAddr::new(0x7E_000E));
            assert_eq!(level, 0x105, "flags {high:X}");
        }
        // A normal exit with `h` reaches level 105 from bank 0's submap, which
        // the game's format would send to 005.
        let ram = kobo_core::expand::enter_by_exit(&built, 0x05, 0x05, false, 0, |_| {}).unwrap();
        assert_eq!(ram.u16(kobo_core::ram::RamAddr::new(0x7E_000E)), 0x105);
        // `w` on a secondary exit makes the entrance a water level.
        let ram = kobo_core::expand::enter_by_exit(&built, 0xCB, 0x0E, false, 0, |_| {}).unwrap();
        assert_eq!(ram.u16(kobo_core::ram::RamAddr::new(0x7E_000E)), 0x105);
        let action = ram.u8(kobo_core::ram::RamAddr::new(0x7E_192A));
        assert_eq!(action & 0x40, 0x40, "entrance type {action:02X}");
    }
}

#[test]
fn a_level_with_an_exit_in_lunar_magic_format_has_all_its_exits_so() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::level::objects::{Object, ScreenExit};
    let exits = |objects: &[Object]| -> Vec<ScreenExit> {
        objects
            .iter()
            .filter_map(|o| match o {
                Object::ScreenExit(e) => Some(*e),
                _ => None,
            })
            .collect()
    };
    // Level 105's exit in the game's format, and one only Lunar Magic's
    // can say, which a save would otherwise copy `s` from (lunar-magic.md).
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    assert_eq!(exits(&level.layer1).len(), 1);
    level.layer1.push(Object::ScreenExit(ScreenExit {
        screen: 3,
        flags: ScreenExit::LUNAR_MAGIC | ScreenExit::SECONDARY | ScreenExit::WATER,
        destination: 0x20,
    }));
    let project = Project {
        levels: vec![(0x105, level.clone())],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let written = exits(&level::read_objects(&built, 0x105).unwrap().layer1.objects);
    assert_eq!(written.len(), 2);
    assert!(
        written
            .iter()
            .all(|e| e.flags & ScreenExit::LUNAR_MAGIC != 0),
        "{written:?}"
    );
    // The import puts the one it can back in the game's format.
    assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);

    // Without one in Lunar Magic's format, the level's stay in the game's.
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    let project = Project {
        levels: vec![(0x105, level)],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let written = exits(&level::read_objects(&built, 0x105).unwrap().layer1.objects);
    assert_eq!(written[0].flags & ScreenExit::LUNAR_MAGIC, 0);
}

#[test]
fn lunar_magic_entrance_settings_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::entrance::{MidwayEntrance, SeparateMidway};
        use kobo_core::ram::RamAddr;
        // Level 105's main entrance at tile (9, 22) of screen 1 with the
        // layers relative to the player, 10 rows above it; a midway entrance
        // of its own at tile (17, 20) of screen 2; and its secondary entrance
        // made a water level at tile (3, 40).
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        level.entrance.entrance_screen = 1;
        level.entrance.midway_screen = 2;
        level.entrance.entrance_x = 1;
        level.entrance.entrance_y = 6;
        (level.entrance.fg_position, level.entrance.bg_position) = (1, 2);
        level.settings.tile_position = Some((1, 1));
        level.settings.relative = Some(true);
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
        entrance.settings.water = true;
        let id = entrance.id;
        let text = level.to_toml(&Comments::default());
        assert_eq!(Level::from_toml(&text).unwrap().0, level);
        let project = Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        // What Lunar Magic checks before it keeps the two further tables
        // (docs/lunar-magic-install.md): the marker, and the vanilla tables'
        // pointers where its code keeps them.
        let read = |at: u32, len: usize| built.read(kobo_core::SnesAddr::new(at), len).unwrap();
        assert_eq!(read(0x03BD9C, 4), b"LM\x10\x01");
        for (at, table) in [
            (0x05DC81, 0x05FE00u32),
            (0x0DE191, 0x05F800),
            (0x0DE198, 0x05FA00),
            (0x0DE19F, 0x05FC00),
        ] {
            assert_eq!(read(at, 3), &table.to_le_bytes()[..3], "${at:06X}");
        }
        let word = |ram: &kobo_core::ram::Ram, a: u32| ram.u16(RamAddr::new(0x7E_0000 | a));
        let main = kobo_core::expand::enter_by_exit(&built, 0x05, 0x05, false, 1, |_| {}).unwrap();
        assert_eq!((word(&main, 0x94), word(&main, 0x96)), (0x190, 0x160));
        assert_eq!(word(&main, 0x1C), 0xC0);
        // From the overworld with the midway point passed.
        let midway = kobo_core::expand::enter_by_exit(&built, 0x05, 0x05, false, 1, |ram| {
            ram.set_u8(kobo_core::ram::SUBLEVEL_COUNT, 0);
            ram.set_u8(RamAddr::new(0x7E_0109), 0x05);
            ram.set_u8(RamAddr::new(0x7E_13BF), 0x10);
            ram.set_u8(RamAddr::new(0x7E_1EB2), 0x40);
        })
        .unwrap();
        assert_eq!((word(&midway, 0x94), word(&midway, 0x96)), (0x210, 0x140));
        // Kobo's $05D9E3 routine sets the midway entrance up whole, as Lunar
        // Magic's does: its save keeps that hook and puts its own code at
        // $05DA17, so the result must not rest on Kobo's there. With the
        // game's instructions back at $05DA17, the midway entrance is the same.
        let mut without = kobo_core::Rom::from_headerless(built.data().to_vec()).unwrap();
        without
            .write(
                kobo_core::SnesAddr::new(0x05DA17),
                base.read(kobo_core::SnesAddr::new(0x05DA17), 5).unwrap(),
            )
            .unwrap();
        let alone = kobo_core::expand::enter_by_exit(&without, 0x05, 0x05, false, 1, |ram| {
            ram.set_u8(kobo_core::ram::SUBLEVEL_COUNT, 0);
            ram.set_u8(RamAddr::new(0x7E_0109), 0x05);
            ram.set_u8(RamAddr::new(0x7E_13BF), 0x10);
            ram.set_u8(RamAddr::new(0x7E_1EB2), 0x40);
        })
        .unwrap();
        for at in [0x94, 0x96, 0x1C, 0x20] {
            assert_eq!(word(&alone, at), word(&midway, at), "${at:02X}");
        }
        for at in [0x13CD, 0x192A] {
            let byte = |ram: &kobo_core::ram::Ram| ram.u8(RamAddr::new(0x7E_0000 | at));
            assert_eq!(byte(&alone), byte(&midway), "${at:04X}");
        }
        let secondary = kobo_core::expand::enter_by_exit(
            &built,
            id as u8,
            0x06 | (id >> 8) as u8,
            true,
            0,
            |_| {},
        )
        .unwrap();
        assert_eq!(word(&secondary, 0x96), 0x280);
        assert_eq!(secondary.u8(RamAddr::new(0x7E_192A)) & 0x40, 0x40);
    }
}

/// An entrance's face left (`L`) turns the player left, and a slanted pipe
/// then shoots him left, as Lunar Magic's code at `$009708` and `$00D2B2`
/// does (docs/lunar-magic-install.md); without it he faces and flies right.
#[test]
fn face_left_turns_the_player_and_the_slanted_pipe() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        for (action, face_left) in [(0, false), (0, true), (6, false), (6, true)] {
            let (mut level, _) = import::read_level(&base, 0x105).unwrap();
            level.entrance.entrance_action = action;
            level.settings.face_left = face_left;
            let project = Project {
                root: std::path::PathBuf::from("."),
                manifest: Default::default(),
                levels: vec![(0x105, level)],
                ..Default::default()
            };
            let built = common::build_as(&clean, &project, sa1).unwrap();
            let byte = |ram: &kobo_core::ram::Ram, at: u32| {
                ram.u8(kobo_core::ram::RamAddr::new(0x7E_0000 | at))
            };
            let mut first = None;
            kobo_core::expand::play_level(&built, 0x105, 1, |_, ram| {
                first = Some((byte(ram, 0x76), byte(ram, 0x7B)));
            })
            .unwrap();
            let (direction, speed) = first.unwrap();
            let case = format!("action {action}, face left {face_left}, SA-1 {sa1}");
            assert_eq!(direction, if face_left { 0 } else { 1 }, "{case}");
            if action == 6 {
                assert_eq!(speed, if face_left { 0xC0 } else { 0x40 }, "{case}");
            }
        }
    }
}

#[test]
fn cached_builds_equal_clean_ones() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let cache_dir = temp_dir("cache");
    let cache = build::Cache::new(cache_dir.to_path_buf());
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    let mut project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level)],
        ..Default::default()
    };
    let uncached = build::build(&clean, &project).unwrap();
    let cold = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    let warm = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    assert_eq!(cold.data(), uncached.data());
    assert_eq!(warm.data(), uncached.data());
    assert_eq!(
        fs::read_dir(&cache_dir).unwrap().count(),
        build::Stage::ALL.len()
    );

    // An edit reruns the level stage from the cached base.
    project.levels[0].1.layer1.pop();
    let edited = build::build_cached(&clean, &project, Some(&cache)).unwrap();
    assert_eq!(
        edited.data(),
        build::build(&clean, &project).unwrap().data()
    );
    assert_eq!(
        fs::read_dir(&cache_dir).unwrap().count(),
        build::Stage::ALL.len() + 1
    );
}

/// A build needs no ROM data to check that its output is the same on every
/// platform: this one runs on a synthetic base in CI.
#[test]
fn a_synthetic_build_is_the_same_everywhere() {
    let base = common::synthetic_base();
    let text = include_str!("fixtures/synthetic_level.toml");
    let (level, comments) = Level::from_toml(text).unwrap();
    assert_eq!(level.to_toml(&comments), text);
    let project = Project {
        root: std::path::PathBuf::from("."),
        manifest: Default::default(),
        levels: vec![(0x105, level.clone()), (0x0C7, level)],
        ..Default::default()
    };
    let built = build::build_on(&base, &project, None).unwrap();
    assert_eq!(
        import::read_level(&built, 0x105).unwrap().0,
        project.levels[0].1
    );
    assert_eq!(
        built.sha1_hex(),
        "5f308245ae892ca5e8b2778540c220a03cf6b847",
        "the synthetic build's output changed"
    );
    // Distributed as a patch (`kobo build --bps`), it gives the build back.
    let patch = bps::create(base.data(), built.data());
    assert_eq!(bps::apply(&patch, base.data()).unwrap(), built.data());
}

#[test]
fn secondary_entrances_are_checked_and_written() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let entrances_of = |rom: &Rom, n| import::read_level(rom, n).unwrap().0.entrances;
    let level = |n| import::read_level(&clean, n).unwrap().0;
    let build_with = |levels: Vec<(u16, Level)>| {
        let project = Project {
            root: PathBuf::from("."),
            manifest: Default::default(),
            levels,
            ..Default::default()
        };
        build::build(&clean, &project).map_err(|e| e.to_string())
    };
    // Vanilla level 105 has secondary entrances leading to it; others to
    // 106 and 1xx levels.
    let (mut l105, l106) = (level(0x105), level(0x106));
    let theirs = entrances_of(&clean, 0x106);
    assert!(!l105.entrances.is_empty() && !theirs.is_empty());

    // A level's list is all of its entrances: an empty one clears them.
    let mut cleared = l105.clone();
    cleared.entrances.clear();
    let built = build_with(vec![(0x105, cleared)]).unwrap();
    assert!(entrances_of(&built, 0x105).is_empty());
    assert_eq!(entrances_of(&built, 0x106), theirs);

    // Two levels cannot share one, nor take one the base ROM gives a level
    // the project does not define.
    let mut shared = l106.clone();
    shared.entrances.push(l105.entrances[0]);
    let error = build_with(vec![(0x105, l105.clone()), (0x106, shared)]).unwrap_err();
    assert!(error.contains("is also level 105's"), "{error}");
    let mut taken = l105.clone();
    taken.entrances.push(theirs[0]);
    let error = build_with(vec![(0x105, taken)]).unwrap_err();
    assert!(error.contains("leads to level 106"), "{error}");

    // An entrance numbered in the other bank builds with Kobo's exit hooks,
    // which take the destination's bit 8 from the entrance.
    let unused = (0..0x100u16)
        .find(|id| {
            (0..0x200u16).all(|n| !entrances_of(&clean, n).iter().any(|e| e.id == *id))
                && level::read_entrances(&clean).unwrap()[*id as usize] == Default::default()
        })
        .unwrap();
    let mut entrance = l105.entrances[0];
    entrance.id = unused;
    l105.entrances.push(entrance);
    let built = build_with(vec![(0x105, l105.clone())]).unwrap();
    assert!(entrances_of(&built, 0x105).iter().any(|e| e.id == unused));
    // One past 1FF, which a long exit names, in tables the build moves to
    // hold the last entrance in use (entrances.asm), all six of them, as
    // Lunar Magic's save sizes them; one past 1FFF is refused.
    l105.entrances.last_mut().unwrap().id = 0x320;
    let built = build_with(vec![(0x105, l105.clone())]).unwrap();
    assert_eq!(level::entrance_count(&built), 0x321);
    let block = |at| {
        let pc = built.pc(at).unwrap().as_usize();
        kobo_core::rats::tag_at(built.data(), pc - 8)
    };
    let extra = kobo_core::entrance::Layout::of(&built).extra.unwrap();
    for table in level::entrance_tables(&built).into_iter().chain(extra) {
        assert_eq!(block(table), Some(0x321), "the table at {table}");
    }
    assert!(entrances_of(&built, 0x105).iter().any(|e| e.id == 0x320));
    assert_eq!(entrances_of(&built, 0x106), theirs);
    // The game's are copied, and the unused ones past 1FF are left clear.
    let tables = level::read_entrances(&built).unwrap();
    assert_eq!(tables.len(), 0x321);
    let clean_tables = level::read_entrances(&clean).unwrap();
    for id in 0..0x200 {
        if !entrances_of(&clean, 0x105)
            .iter()
            .any(|e| e.id == id as u16)
        {
            assert_eq!(
                tables[id].0[..3],
                clean_tables[id].0[..3],
                "entrance {id:03X}"
            );
        }
    }
    assert!(
        tables[0x200..0x320]
            .iter()
            .all(|e| *e == Default::default())
    );
    // An exit to an entrance past the last one defined sizes them too.
    let mut named = l105.clone();
    named
        .layer1
        .push(kobo_core::level::objects::Object::ScreenExit(
            kobo_core::level::objects::ScreenExit::lunar_magic(
                1,
                kobo_core::level::objects::ScreenExit::SECONDARY,
                0x4AB,
            ),
        ));
    let built = build_with(vec![(0x105, named)]).unwrap();
    assert_eq!(level::entrance_count(&built), 0x4AC);
    l105.entrances.last_mut().unwrap().id = 0x2000;
    let error = build_with(vec![(0x105, l105)]).unwrap_err();
    assert!(error.contains("past the 8192"), "{error}");
}

/// A midway entrance that redirects to another level's, an exit that leads
/// to the midway entrance (`w` without `s`), and what a build refuses: a
/// redirect loop, and Lunar Magic settings for entrances its tables do not
/// hold.
#[test]
fn midway_redirects_and_what_is_refused() {
    use kobo_core::entrance::{MidwayEntrance, SeparateMidway};
    use kobo_core::level::objects::{Object, ScreenExit};
    use kobo_core::ram::RamAddr;
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let level = |n| import::read_level(&base, n).unwrap().0;
        let project = |levels: Vec<(u16, Level)>| Project {
            root: std::path::PathBuf::from("."),
            manifest: Default::default(),
            levels,
            ..Default::default()
        };
        let mut l105 = level(0x105);
        l105.entrance.midway_screen = 2;
        l105.settings.midway.separate = Some(SeparateMidway::Entrance(MidwayEntrance {
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
        let mut l104 = level(0x104);
        l104.settings.midway.separate = Some(SeparateMidway::Redirect(0x105));
        // Level 106's exit on screen 0 leads to level 105's midway entrance.
        let mut l106 = level(0x106);
        l106.layer1.push(Object::ScreenExit(ScreenExit {
            screen: 0,
            flags: ScreenExit::LUNAR_MAGIC | ScreenExit::WATER | ScreenExit::HIGH,
            destination: 0x05,
        }));
        let built = common::build_as(
            &clean,
            &project(vec![
                (0x104, l104.clone()),
                (0x105, l105.clone()),
                (0x106, l106),
            ]),
            sa1,
        )
        .unwrap();
        let word = |ram: &kobo_core::ram::Ram, a: u32| ram.u16(RamAddr::new(0x7E_0000 | a));
        let from_overworld = |n: u8| {
            kobo_core::expand::enter_by_exit(&built, n, 0x05, false, 1, |ram| {
                ram.set_u8(kobo_core::ram::SUBLEVEL_COUNT, 0);
                ram.set_u8(RamAddr::new(0x7E_0109), n);
                ram.set_u8(RamAddr::new(0x7E_13BF), 0x10);
                ram.set_u8(RamAddr::new(0x7E_1EB2), 0x40);
            })
            .unwrap()
        };
        let midway = from_overworld(0x05);
        assert_eq!((word(&midway, 0x94), word(&midway, 0x96)), (0x210, 0x140));
        let redirected = from_overworld(0x04);
        assert_eq!(word(&redirected, 0x0E), 0x105);
        assert_eq!(
            (word(&redirected, 0x94), word(&redirected, 0x96)),
            (0x210, 0x140)
        );
        let by_exit =
            kobo_core::expand::enter_by_exit(&built, 0x05, 0x0D, false, 1, |_| {}).unwrap();
        assert_eq!((word(&by_exit, 0x94), word(&by_exit, 0x96)), (0x210, 0x140));

        // 104 -> 105 -> 104 loops.
        let mut looping = l105.clone();
        looping.settings.midway.separate = Some(SeparateMidway::Redirect(0x104));
        let error = common::build_as(&clean, &project(vec![(0x104, l104), (0x105, looping)]), sa1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("loop"), "{error}");

        // Entrance 1FE's Lunar Magic settings have no place in its tables.
        let mut high = level(0x105);
        let entrance = high
            .entrances
            .first_mut()
            .expect("105 has a secondary entrance");
        entrance.id = 0x1FE;
        entrance.settings.water = true;
        let error = common::build_as(&clean, &project(vec![(0x105, high)]), sa1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("do not fit"), "{error}");
    }
}

/// Lunar Magic's time limit bypass (`28`): the timer each form leaves after
/// level `105` loads, entered from the overworld and through a screen exit
/// with the timer at 999 before, as a Lunar Magic 3.70 ROM with the object
/// imported (or written into its level data) leaves it, digits hundreds
/// first, and the status bar's copy (`examples/lm_objects.rs timer`,
/// 2026-09-27).
#[test]
fn the_time_limit_bypass_sets_the_timer_as_lunar_magic_does() {
    use kobo_core::level::objects::Object;
    use kobo_core::ram::{self, RamAddr};
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let at = |a: u32| RamAddr::new(0x7E_0000 | a);
        // Object bytes; then the timer and its status bar copy from the
        // overworld and through an exit (None: left at 999).
        type Digits = Option<([u8; 3], [u8; 3])>;
        let cases: [([u8; 3], Digits, Digits); 7] = [
            (
                [0x41, 0x83, 0x81],
                Some(([1, 1, 3], [1, 1, 3])),
                Some(([1, 1, 3], [1, 1, 3])),
            ),
            ([0x41, 0x83, 0x01], Some(([1, 1, 3], [1, 1, 3])), None),
            (
                [0x40, 0x80, 0x80],
                Some(([0, 0, 0], [0xFC, 0xFC, 0])),
                Some(([0, 0, 0], [0xFC, 0xFC, 0])),
            ),
            (
                [0x4F, 0x8F, 0x8F],
                Some(([15; 3], [15; 3])),
                Some(([15; 3], [15; 3])),
            ),
            ([0x40, 0x80, 0x00], Some(([0, 0, 0], [0xFC, 0xFC, 0])), None),
            (
                [0x41, 0x83, 0xF1],
                Some(([1, 1, 3], [1, 1, 3])),
                Some(([1, 1, 3], [1, 1, 3])),
            ),
            ([0x41, 0x83, 0x7F], Some(([15, 1, 3], [15, 1, 3])), None),
        ];
        let (vanilla, _) = import::read_level(&base, 0x105).unwrap();
        for (bytes, overworld, exit) in cases {
            let mut level = vanilla.clone();
            level.layer1.push(Object::Unplaced(bytes.to_vec()));
            let project = Project {
                levels: vec![(0x105, level)],
                ..Default::default()
            };
            let built = common::build_as(&clean, &project, sa1).unwrap();
            for (count, expected) in [(0u8, overworld), (1, exit)] {
                let ram = kobo_core::expand::play_level_entered(
                    &built,
                    0x105,
                    |ram| {
                        ram.set_u8(ram::SUBLEVEL_COUNT, count);
                        ram.set_u8(at(0x0109), 0x05);
                        for i in 0..3 {
                            ram.set_u8(at(0x0F31 + i), 9);
                            ram.set_u8(at(0x0F25 + i), 9);
                        }
                    },
                    0,
                    |_, _| {},
                )
                .unwrap();
                let read = |a: u32| [0, 1, 2].map(|i| ram.u8(at(a + i)));
                let (timer, status) = expected.unwrap_or(([9; 3], [9; 3]));
                assert_eq!(
                    (read(0x0F31), read(0x0F25)),
                    (timer, status),
                    "{bytes:02X?}, sublevel count {count}"
                );
            }
        }
    }
}

/// The sprite slots after `frames` frames of play: each slot's number,
/// status, and position, and the per-entry load flags.
fn sprite_slots(rom: &Rom, level: u16, frames: u32) -> Vec<Vec<u8>> {
    use kobo_core::ram;
    let played = kobo_core::expand::play_level(rom, level, frames, |frame, ram| {
        // Carry the player right, so the camera follows and the loader
        // meets the list's sprites.
        let x = 0x30 + 3 * frame as u16;
        ram.set_u8(ram::RamAddr::new(0x7E_0094), x as u8);
        ram.set_u8(ram::RamAddr::new(0x7E_0095), (x >> 8) as u8);
        ram.set_u8(ram::RamAddr::new(0x7E_0096), 0x40);
    })
    .unwrap();
    let table = |t| (0..12).map(|i| played.u8_at(t, i)).collect::<Vec<u8>>();
    vec![
        table(ram::SPRITE_NUMBER),
        table(ram::SPRITE_STATUS),
        table(ram::SPRITE_X_LOW),
        table(ram::SPRITE_X_HIGH),
        table(ram::SPRITE_Y_LOW),
        table(ram::SPRITE_Y_HIGH),
        played.bytes(ram::SPRITE_LOAD_STATUS, 0x80),
    ]
}

#[test]
fn sprite_lists_past_bank_07_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::source::level::Sprite;
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        level.sprites.list[0].x += 1;
        let alone = Project {
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        assert!(!alone.installs_lunar_magic(&base));
        // 24 more levels of 60 sprites each: 4,392 bytes of lists, which bank
        // $07's first-fit placement cannot take.
        let mut levels = vec![(0x105, level.clone())];
        for number in 0x110..0x128u16 {
            let (mut other, _) = import::read_level(&base, number).unwrap();
            other.sprites.list = (0..60)
                .map(|i| Sprite {
                    id: 0x0D,
                    x: 20 + i * 3,
                    y: 5 + i % 20,
                    extra_bits: 0,
                    extension: Vec::new(),
                })
                .collect();
            levels.push((number, other));
        }
        let many = Project {
            levels,
            ..Default::default()
        };
        assert!(!many.lunar_magic_layout() && many.installs_lunar_magic(&base));
        let built = common::build_as(&clean, &many, sa1).unwrap();
        for (number, level) in &many.levels {
            assert_eq!(&import::read_level(&built, *number).unwrap().0, level);
            let at = level::sprite_ptr(&built, *number).unwrap();
            assert!(at.bank() >= 0x10, "level {number:03X}'s list at {at}");
        }
        // Levels the project does not define keep bank $07.
        assert_eq!(
            level::sprite_ptr(&built, 0x106).unwrap(),
            level::sprite_ptr(&base, 0x106).unwrap()
        );
        // The game reads a list from its bank: level 105 plays the same from
        // a RATS block as from bank $07.
        let in_bank_07 = common::build_as(&clean, &alone, sa1).unwrap();
        assert_eq!(level::sprite_ptr(&in_bank_07, 0x105).unwrap().bank(), 0x07);
        let slots = sprite_slots(&built, 0x105, 900);
        assert!(
            slots[6].iter().filter(|&&f| f != 0).count() > 5,
            "{slots:?}"
        );
        assert_eq!(slots, sprite_slots(&in_bank_07, 0x105, 900));
    }
}

/// A list only Lunar Magic's loader reads: more sprites than the game's
/// loader takes, one starting with `$FF` (so in the new sprite system's
/// format), builds with Kobo's loader, reads back, and loads its last
/// sprites in play.
#[test]
fn lists_for_lunar_magics_loader_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // On the clean ROM, and as an SA-1 project.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        use kobo_core::source::level::Sprite;
        let (mut level, _) = import::read_level(&base, 0x105).unwrap();
        let sprite = |x: u16, y: u16, extra_bits: u8| Sprite {
            id: 0x0F,
            x,
            y,
            extra_bits,
            extension: Vec::new(),
        };
        level.sprites.list = (0..120).map(|i| sprite(18 + i, 8 + i % 12, 0)).collect();
        // Y 31, extra bits 3, on screen 17: a first byte of $FF.
        level.sprites.list[100] = sprite(16 * 17 + 6, 31, 3);
        level.sprites.list.sort_by_key(|s| s.x / 16);
        assert!(level.sprites.needs_new_system(false));
        let project = Project {
            levels: vec![(0x105, level.clone())],
            ..Default::default()
        };
        assert!(project.lunar_magic_layout());
        let built = common::build_as(&clean, &project, sa1).unwrap();
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        let list = kobo_core::sprites::read_sprites(&built, 0x105).unwrap();
        assert!(list.header.new_sprite_system && list.len > 0x100);
        // Kobo's loader reads past the 84 sprites the game's reads.
        let played = kobo_core::expand::play_level(&built, 0x105, 900, |frame, ram| {
            use kobo_core::ram::RamAddr;
            let x = 0x30 + 3 * frame as u16;
            ram.set_u8(RamAddr::new(0x7E_0094), x as u8);
            ram.set_u8(RamAddr::new(0x7E_0095), (x >> 8) as u8);
            ram.set_u8(RamAddr::new(0x7E_0096), 0x40);
            ram.set_u8(RamAddr::new(0x7E_1497), 0x7F);
            for slot in 0..12 {
                ram.set_u8(RamAddr::new(0x7E_14C8 + slot), 0);
            }
        })
        .unwrap();
        let flags = played.bytes(kobo_core::ram::SPRITE_LOAD_STATUS, 0x80);
        let loaded: Vec<usize> = (0..0x80).filter(|&i| flags[i] != 0).collect();
        assert!(loaded.iter().any(|&i| i > 84), "{loaded:?}");

        // What the loaders cannot read is refused.
        let refused = |level: Level, what: &str| {
            let project = Project {
                levels: vec![(0x105, level)],
                ..Default::default()
            };
            let error = common::build_as(&clean, &project, sa1)
                .unwrap_err()
                .to_string();
            assert!(error.contains(what), "{error}");
        };
        let mut unsorted = level.clone();
        unsorted.sprites.list.swap(0, 90);
        refused(unsorted, "screen order");
        let mut many = level.clone();
        many.sprites.list = (0..129).map(|i| sprite(18 + i, 8, 0)).collect();
        refused(many, "128");
        let (mut vertical, _) = import::read_level(&base, 0x1CE).unwrap();
        vertical.sprites.list.push(sprite(40, 70, 0));
        vertical.sprites.list.sort_by_key(|s| s.y / 16);
        let project = Project {
            levels: vec![(0x1CE, vertical)],
            ..Default::default()
        };
        let error = common::build_as(&clean, &project, sa1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("32 columns"), "{error}");
    }
}
