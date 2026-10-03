//! Lunar Magic's graphics formats through a build: 4bpp GFX files, ExGFX
//! files, per-level graphics lists, and the older lists objects `24` and
//! `25` name, loaded by Kobo's code for them (`asm/lunar-magic/graphics.asm`)
//! and read back by import.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use kobo_core::build::{self, BuildError, Project};
use kobo_core::exgfx::{self, GraphicsList, slot};
use kobo_core::expand;
use kobo_core::gfx::{self, Bpp, Tile8};
use kobo_core::import;
use kobo_core::level::objects::Object;
use kobo_core::ram;
use kobo_core::render::{self, RenderOptions};
use kobo_core::source::project::Manifest;

/// `count` tiles no GFX file has: each tile's pixels count up from its
/// number, below `colors`.
fn marked_tiles(count: usize, colors: usize, seed: usize) -> Vec<Tile8> {
    (0..count)
        .map(|i| {
            let mut t = Tile8::default();
            for (y, row) in t.pixels.iter_mut().enumerate() {
                for (x, px) in row.iter_mut().enumerate() {
                    *px = ((i * 7 + y * 3 + x + seed) % colors) as u8;
                }
            }
            t
        })
        .collect()
}

fn encode(bpp: Bpp, tiles: &[Tile8]) -> Vec<u8> {
    gfx::GfxFormat::Planar(bpp).encode(tiles)
}

fn project(manifest: Manifest) -> Project {
    Project {
        root: PathBuf::from("."),
        manifest,
        ..Default::default()
    }
}

/// VRAM bytes of `words` words from word address `at`.
fn vram(level: &expand::LoadedLevel, at: usize, words: usize) -> &[u8] {
    &level.video.vram[at * 2..(at + words) * 2]
}

#[test]
fn a_4bpp_build_draws_levels_as_the_game_does() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let project = project(Manifest {
        four_bpp: true,
        ..Manifest::default()
    });
    // On the clean ROM, and as an SA-1 project.
    for (base, built) in common::builds(&clean, &project) {
        assert!(exgfx::is_4bpp(&built) && exgfx::has_exgfx(&built));
        // Every file the game keeps as 3bpp is stored as Lunar Magic 3.70
        // stores it, and read back as no change.
        let reader = gfx::GfxReader::new(&built).unwrap();
        let vanilla = gfx::GfxReader::new(&base).unwrap();
        for index in 0..gfx::GFX_FILE_COUNT {
            let ours = vanilla.read(index).unwrap();
            let theirs = reader.read(index).unwrap();
            if exgfx::converts(ours.format) {
                assert_eq!(theirs.data, exgfx::stored_4bpp(&ours), "GFX{index:02X}");
            } else {
                assert_eq!(theirs.data, ours.data, "GFX{index:02X}");
            }
        }
        let (files, notes) = import::read_gfx(&built, &base).unwrap();
        assert!(files.is_empty() && notes.is_empty(), "{notes:?}");
        // Levels of several tilesets, a boss arena, the berry file (17), and
        // the special world's file 01 swap aside.
        for level in [0x105, 0x106, 0x1CE, 0x0C7, 0x10E, 0x01A, 0x113] {
            let a = render::render_level(&base, level, RenderOptions::default()).unwrap();
            let b = render::render_level(&built, level, RenderOptions::default()).unwrap();
            assert!(a.image == b.image, "level {level:03X} draws differently");
        }
    }
}

#[test]
fn exgfx_files_and_lists_build() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // ExGFX 80 for SP1 and 100 for FG1 (4bpp), 101 for LG1 (2bpp), and 102
    // as the animated tiles' file (bytes).
    let fg1 = encode(Bpp::Four, &marked_tiles(128, 16, 1));
    let sp1 = encode(Bpp::Four, &marked_tiles(128, 16, 2));
    let lg1 = encode(Bpp::Two, &marked_tiles(128, 4, 3));
    let an2: Vec<u8> = (0..0x1800u32).map(|i| (i * 13 % 251) as u8).collect();
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    let mut list = GraphicsList::DEFAULT;
    list.0[slot::AN2] = exgfx::BYPASS | exgfx::LAYER3_FILES | 0x102;
    list.0[slot::FG1] = 0x100;
    list.0[slot::SP1] = 0x80;
    list.0[slot::LG1] = 0x101;
    for (s, file) in [
        (slot::FG2, 0x01),
        (slot::BG1, 0x13),
        (slot::FG3, 0x02),
        (slot::SP2, 0x01),
        (slot::SP3, 0x13),
        (slot::SP4, 0x02),
    ] {
        list.0[s] = file;
    }
    level.graphics = Some(list);
    let mut p = project(Manifest::default());
    p.levels = vec![(0x105, level.clone())];
    p.exgfx = vec![
        (0x80, sp1.clone()),
        (0x100, fg1.clone()),
        (0x101, lg1.clone()),
        (0x102, an2.clone()),
    ];
    // On the clean ROM, and as an SA-1 project.
    for (base, built) in common::builds(&clean, &p) {
        let loaded = expand::expand_level(&built, 0x105).unwrap();
        // FG1's first 64 tiles (the animated ones come later), SP1 past the
        // player's tiles and short of its last, which the game writes, and LG1.
        assert!(vram(&loaded, 0x0000, 0x400) == &fg1[..0x800], "FG1");
        assert!(vram(&loaded, 0x6400, 0x3F0) == &sp1[0x800..0xFE0], "SP1");
        assert!(vram(&loaded, 0x4000, 0x400) == &lg1[..], "LG1");
        // The list's other files where they go: FG2 is GFX01 (past its first
        // tiles, which the level's animations write).
        let gfx01 = exgfx::stored_4bpp(&gfx::read_gfx_file(&base, 0x01).unwrap());
        assert!(vram(&loaded, 0x0A00, 0x200) == &gfx01[0x400..0x800], "FG2");
        // AN2 is left in the buffer, where the animated tiles read it.
        assert!(loaded.ram.bytes(ram::GFX_BUFFER, 0x1800) == an2, "AN2");
        // Where the list is, for code that reads its settings later.
        let entry = loaded.ram.u24(ram::LM_GRAPHICS_LIST);
        assert_eq!(
            entry,
            built.read_u24(exgfx::LIST_POINTER).unwrap() + 0x105 * 32
        );

        // Import reads it all back.
        assert_eq!(import::read_level(&built, 0x105).unwrap().0, level);
        let (files, notes) = import::read_exgfx_files(&built).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let files: BTreeMap<u16, import::ExGfxImport> = files.into_iter().collect();
        assert_eq!(files.len(), 4);
        let image = |bytes: &[u8], bpp: Bpp| {
            import::ExGfxImport::Image(
                gfx::tiles_to_image(&gfx::decode_tiles(bpp, bytes), bpp.colors()),
                bpp.bits(),
            )
        };
        assert!(files[&0x100] == image(&fg1, Bpp::Four), "ExGFX100");
        assert!(files[&0x101] == image(&lg1, Bpp::Two), "ExGFX101");
        assert!(
            files[&0x102] == import::ExGfxImport::Bytes(an2.clone()),
            "ExGFX102"
        );
    }

    // An ExGFX file is an input of the graphics stage.
    let keys = build::stage_keys(&clean, &p).unwrap();
    let mut edited = p.clone();
    edited.exgfx[1].1[0] ^= 1;
    assert_ne!(build::stage_keys(&clean, &edited).unwrap(), keys);
}

#[test]
fn objects_24_and_25_name_the_older_lists() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let fg = encode(Bpp::Four, &marked_tiles(128, 16, 4));
    let an2: Vec<u8> = (0..0x1000u32).map(|i| (i * 7 % 253) as u8).collect();
    let (mut level, _) = import::read_level(&clean, 0x106).unwrap();
    // Object 24 with no sprite list and FG list 3 (its third byte, 4), and
    // object 25 with file 81 (82) for the animated tiles.
    level.layer1.push(Object::Unplaced(vec![0x40, 0x40, 0x04]));
    level.layer1.push(Object::Unplaced(vec![0x40, 0x50, 0x82]));
    let mut p = project(Manifest {
        bypass_lists: BTreeMap::from([(3, [0x80, 0x01, 0x13, 0x02])]),
        ..Manifest::default()
    });
    p.levels = vec![(0x106, level.clone())];
    p.exgfx = vec![(0x80, fg.clone()), (0x81, an2.clone())];
    // On the clean ROM, and as an SA-1 project.
    for (base, built) in common::builds(&clean, &p) {
        let loaded = expand::expand_level(&built, 0x106).unwrap();
        assert!(vram(&loaded, 0x0000, 0x400) == &fg[..0x800], "FG1");
        let gfx13 = exgfx::stored_4bpp(&gfx::read_gfx_file(&base, 0x13).unwrap());
        assert!(vram(&loaded, 0x1000, 0x800) == &gfx13[..], "BG1");
        assert!(loaded.ram.bytes(ram::GFX_BUFFER, 0x1000) == an2, "AN2");
        assert_eq!(import::read_level(&built, 0x106).unwrap().0, level);
        assert_eq!(
            exgfx::read_old_lists(&built).unwrap(),
            [(3, [0x80, 0x01, 0x13, 0x02])]
        );
    }

    // In layer 2, which Kobo's code does not read, they are refused.
    let mut on_layer2 = p.clone();
    let (mut level, _) = import::read_level(&clean, 0x1CE).unwrap();
    if let kobo_core::source::level::Layer2::Objects(list) = &mut level.layer2 {
        list.push(Object::Unplaced(vec![0x40, 0x40, 0x04]));
    } else {
        panic!("level 1CE has layer 2 objects");
    }
    on_layer2.levels = vec![(0x1CE, level)];
    let error = build::build(&clean, &on_layer2).unwrap_err().to_string();
    assert!(error.contains("layer 1 only"), "{error}");
}

#[test]
fn what_a_graphics_build_refuses() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    let refused = |change: fn(&mut GraphicsList), exgfx: Vec<(u16, Vec<u8>)>| {
        let mut level = level.clone();
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::AN2] |= exgfx::BYPASS;
        change(&mut list);
        level.graphics = Some(list);
        let mut p = project(Manifest::default());
        p.levels = vec![(0x105, level)];
        p.exgfx = exgfx;
        match build::build(&clean, &p) {
            Err(e @ BuildError::Level { .. }) => e.to_string(),
            other => panic!("{other:?}"),
        }
    };
    // AN2's bit 12, whose effect was not found.
    let e = refused(|l| l.0[slot::AN2] |= exgfx::UNKNOWN_BIT, Vec::new());
    assert!(e.contains("bit 12"), "{e}");
    // Layer 3's tilemap of size 3.
    let e = refused(
        |l| {
            l.0[slot::AN2] |= exgfx::LAYER3_TILEMAP;
            l.0[slot::LT3] = 0x7000 | 0x28;
        },
        Vec::new(),
    );
    assert!(e.contains("size 3"), "{e}");
    // One past the GFX files, and one too large. A file the project lacks
    // builds, and its slot loads nothing, as a slot naming a file the ROM
    // lacks does for Lunar Magic's code: the same as 7F.
    let vram = |file: u16| {
        let mut level = level.clone();
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::AN2] |= exgfx::BYPASS;
        list.0[slot::FG1] = file;
        level.graphics = Some(list);
        let mut p = project(Manifest::default());
        p.levels = vec![(0x105, level)];
        let built = build::build(&clean, &p).unwrap();
        kobo_core::expand::expand_level(&built, 0x105)
            .unwrap()
            .video
            .vram
    };
    assert!(vram(0x100) == vram(0x7F), "a missing file loads as 7F");
    let e = refused(|l| l.0[slot::FG1] = 0x40, Vec::new());
    assert!(e.contains("not a GFX file"), "{e}");
    let e = refused(|l| l.0[slot::FG1] = 0x100, vec![(0x100, vec![0; 0x1001])]);
    assert!(e.contains("where the slot takes"), "{e}");
}

/// ExGFX files as imported, as the bytes a build writes.
fn exgfx_bytes(files: &[(u16, import::ExGfxImport)]) -> Vec<(u16, Vec<u8>)> {
    files
        .iter()
        .map(|(n, file)| {
            let data = match file {
                import::ExGfxImport::Bytes(bytes) => bytes.clone(),
                import::ExGfxImport::Image(image, bits) => {
                    let bpp = if *bits == 2 { Bpp::Two } else { Bpp::Four };
                    let count = image.height as usize / 8 * 16;
                    encode(
                        bpp,
                        &gfx::image_to_tiles(image, count, bpp.colors()).unwrap(),
                    )
                }
            };
            (*n, data)
        })
        .collect()
}

/// Every corpus hack with ExGFX (`KOBO_LM_ROMS`): its ExGFX files, older
/// lists, and the levels' lists a build takes (files it has and that fit
/// their slots, layer 3 settings it does not refuse), imported and built onto vanilla
/// levels, read back as the hack has them.
#[test]
fn the_corpus_graphics_build_as_the_hacks_have_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let mut checked = 0;
    for (path, hack) in common::lunar_magic_roms() {
        if !exgfx::has_exgfx(&hack) || gfx::is_locked(&hack) {
            continue;
        }
        let name = path.display();
        let (files, _) = import::read_exgfx_files(&hack).unwrap();
        let exgfx = exgfx_bytes(&files);
        let limit = |s: usize| match s {
            slot::AN2 => 0x1A00,
            slot::LT3 => 0x2000,
            _ => 0x1000,
        };
        // The clean ROM's levels: the game's height.
        let builds = |list: &GraphicsList, tide: bool, vertical: bool| {
            let l3 = list.layer3();
            !l3.unknown
                && !(tide && l3.advanced && vertical)
                && !(list.layer3_tilemap() && list.tilemap_settings() & 3 == 3)
                && list.files().iter().all(|&(s, f)| match f {
                    0x00..0x32 => true,
                    0x32..0x80 => false,
                    _ => exgfx
                        .iter()
                        .any(|(n, data)| *n == f && data.len() <= limit(s)),
                })
        };
        let mut levels = Vec::new();
        for number in 0..0x200 {
            let Some(list) = exgfx::read_list(&hack, number)
                .unwrap()
                .filter(|l| l.is_used())
            else {
                continue;
            };
            let (mut level, _) = import::read_level(&clean, number).unwrap();
            let tide = exgfx::has_tide(&clean, level.header.object_tileset, level.entrance.layer3)
                .unwrap();
            if builds(&list, tide, level.header.level_mode.layer1_vertical()) {
                level.graphics = Some(list);
                levels.push((number, level));
            }
        }
        let mut p = project(Manifest {
            rom_size: Some(hack.len().max(0x20_0000)),
            four_bpp: true,
            bypass_lists: exgfx::read_old_lists(&hack).unwrap().into_iter().collect(),
            ..Manifest::default()
        });
        p.levels = levels;
        p.exgfx = exgfx;
        let built = build::build(&clean, &p).unwrap_or_else(|e| panic!("{name}: {e}"));
        // As bytes: which lists the build took decides PNG or bytes.
        let (again, _) = import::read_exgfx_files(&built).unwrap();
        assert!(exgfx_bytes(&again) == p.exgfx, "{name}: ExGFX files");
        assert_eq!(
            exgfx::read_old_lists(&built).unwrap(),
            exgfx::read_old_lists(&hack).unwrap(),
            "{name}: older lists"
        );
        for (number, level) in &p.levels {
            assert_eq!(
                exgfx::read_list(&built, *number).unwrap(),
                level.graphics,
                "{name}: level {number:03X}'s list"
            );
        }
        eprintln!(
            "{name}: {} ExGFX files, {} lists",
            files.len(),
            p.levels.len()
        );
        checked += 1;
    }
    eprintln!("{checked} hacks with ExGFX");
}

/// A `$2000`-byte layer 3 tilemap runs past the buffer over
/// `$7EBD00`-`$7ECCFF`; the loader puts those bytes aside in VRAM at
/// `$4000`-`$47FF` and brings them back, leaving the copy there when no layer 3
/// files load after (docs/lunar-magic-install.md, "Layer 3 in the lists").
#[test]
fn a_full_layer3_tilemap_leaves_the_ram_after_the_buffer_as_it_was() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let tilemap: Vec<u8> = (0..0x2000u32).map(|i| (i * 29 % 253) as u8 | 1).collect();
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    let build = |with_tilemap: bool| {
        let mut level = level.clone();
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::AN2] |= exgfx::BYPASS;
        if with_tilemap {
            list.0[slot::AN2] |= exgfx::LAYER3_TILEMAP;
            list.0[slot::LT3] = 0x0100; // size 0, under the status bar
        }
        level.graphics = Some(list);
        let mut p = project(Manifest::default());
        p.levels = vec![(0x105, level)];
        p.exgfx = vec![(0x100, tilemap.clone())];
        common::builds(&clean, &p)
    };
    let after = ram::RamAddr::new(0x7E_BD00);
    for ((_, without), (_, with)) in build(false).into_iter().zip(build(true)) {
        let a = expand::expand_level(&without, 0x105).unwrap();
        let b = expand::expand_level(&with, 0x105).unwrap();
        let kept = b.ram.bytes(after, 0x1000);
        assert!(kept == a.ram.bytes(after, 0x1000), "RAM after the buffer");
        assert!(vram(&b, 0x4000, 0x800) == &kept[..], "the copy in VRAM");
        assert!(vram(&b, 0x50A0, 0xF60) == &tilemap[0x140..], "the tilemap");
    }
}
