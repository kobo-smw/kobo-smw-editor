//! Map16 pages past 1 through a build: written in Lunar Magic's layout
//! with Kobo's code for it installed, read back by import as they were
//! written, and resolved by the ROM's own tilemap upload. Needs Asar's
//! library; the upload check needs the vanilla ROM too. That Lunar Magic
//! keeps a build's pages when it saves is `lunar_magic_save.rs`.

mod common;

use std::path::PathBuf;

use kobo_core::build::{self, Project};
use kobo_core::map16::pages;
use kobo_core::map16::{GameTables, Map16Tile, Tile8Ref};
use kobo_core::source::map16::{DEFAULT_ACTS, GamePage, GameTile, Map16Entry, Map16Page};
use kobo_core::{Rom, expand, import};

/// A tile made up from its number, to tell tiles apart.
fn entry(tile: u16) -> Map16Entry {
    let r = |i: u16| {
        Tile8Ref::new(
            tile.wrapping_add(i) & 0x3FF,
            (tile % 8) as u8,
            i == 3,
            i == 1,
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
        acts: [0x025, 0x130, 0x12F, 0x200][tile as usize % 4],
    }
}

/// Pages in five groups, among them two past `$40` and two whose
/// pointers are kept less one (`$20`-`$3F`, `$60`-`$7F`), with some
/// tiles each.
fn project() -> Project {
    let mut map16 = Vec::new();
    for page in [0x02u8, 0x13, 0x25, 0x45, 0x7F] {
        let first = page as u16 * 0x100;
        let mut tiles = Map16Page::default();
        for tile in [first, first + 1, first + 0x80, first + 0xFF] {
            tiles.tiles.insert(tile, entry(tile));
        }
        map16.push((page, tiles));
    }
    Project {
        root: PathBuf::from("."),
        manifest: Default::default(),
        levels: Vec::new(),
        map16,
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    }
}

fn check_read_back(rom: &Rom, project: &Project) {
    let (read, notes) = import::read_map16(rom).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let listed: Vec<u8> = project.map16.iter().map(|(p, _)| *p).collect();
    let back: Vec<u8> = read.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        back, listed,
        "the pages that read back are the ones written"
    );
    for ((_, written), (_, back)) in project.map16.iter().zip(&read) {
        for (&tile, entry) in &back.tiles {
            assert_eq!(*entry, written.tile(tile), "tile {tile:04X}");
        }
    }
    // A tile the page file does not list is empty.
    assert_eq!(pages::acts_like(rom, 0x0202).unwrap(), Some(DEFAULT_ACTS));
}

#[test]
fn pages_build_and_read_back() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    let project = project();
    let built = build::build_on(&base, &project, None).unwrap();
    assert!(pages::installed(&built));
    check_read_back(&built, &project);
    assert_eq!(
        build::build_on(&base, &project, None).unwrap().data(),
        built.data(),
        "same inputs, same output"
    );
}

#[test]
fn cached_builds_equal_clean_ones() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    let mut project = project();
    let dir = std::env::temp_dir().join(format!("kobo-map16-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = build::Cache::new(dir.clone());
    let clean = build::build_on(&base, &project, None).unwrap();
    assert_eq!(
        build::build_on(&base, &project, Some(&cache))
            .unwrap()
            .data(),
        clean.data()
    );
    // From the snapshots, and after a page changes.
    assert_eq!(
        build::build_on(&base, &project, Some(&cache))
            .unwrap()
            .data(),
        clean.data()
    );
    project.map16[1].1.tiles.insert(0x1377, entry(0x1234));
    let changed = build::build_on(&base, &project, None).unwrap();
    assert_ne!(changed.data(), clean.data());
    assert_eq!(
        build::build_on(&base, &project, Some(&cache))
            .unwrap()
            .data(),
        changed.data()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tilemap_upload_finds_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let project = project();
    // On the clean ROM, and as an SA-1 project.
    for (base, built) in common::builds(&clean, &project) {
        check_read_back(&built, &project);
        let tiles: Vec<u16> = project
            .map16
            .iter()
            .flat_map(|(_, p)| p.tiles.keys().copied())
            .collect();
        let resolved = expand::resolve_map16(&built, 0x105, &tiles).unwrap();
        for (tile, got) in tiles.iter().zip(resolved) {
            assert_eq!(got, Some(entry(*tile).gfx), "tile {tile:04X}");
        }
        // The vanilla levels are untouched by the install.
        let options = kobo_core::render::RenderOptions::default();
        for level in [0x105, 0x0C7] {
            let a = kobo_core::render::render_level(&base, level, options).unwrap();
            let b = kobo_core::render::render_level(&built, level, options).unwrap();
            assert!(a.image.pixels == b.image.pixels, "level {level:03X}");
        }
    }
}

#[test]
fn bg_pages_build_and_read_back() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    // One tile of the game's page 0 changed, a page of the first table
    // past the game's two, and one of table 3.
    let mut changed = Map16Page::default();
    let at = kobo_core::map16::tables::MAP16_BG_TILES.add(0x12 * 8);
    let theirs = Map16Tile::from_bytes(base.read(at, 8).unwrap().try_into().unwrap());
    let gfx = Map16Tile {
        top_left: Tile8Ref(theirs.top_left.0 ^ 0x0155),
        ..theirs
    };
    changed.tiles.insert(
        0x012,
        Map16Entry {
            gfx,
            ..Map16Entry::default()
        },
    );
    let mut map16_bg = vec![(0x00u8, changed)];
    for page in [0x04u8, 0x32] {
        let first = page as u16 * 0x100;
        let mut tiles = Map16Page::default();
        for tile in [first, first + 0x7F, first + 0xFF] {
            tiles.tiles.insert(
                tile,
                Map16Entry {
                    acts: DEFAULT_ACTS,
                    ..entry(tile)
                },
            );
        }
        map16_bg.push((page, tiles));
    }
    let project = Project {
        root: PathBuf::from("."),
        manifest: Default::default(),
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg,
        gfx: Vec::new(),
        ..Default::default()
    };
    let built = build::build_on(&base, &project, None).unwrap();
    let (read, _) = import::read_map16_bg(&built, &base).unwrap();
    // The game's pages keep the base's tiles where the project leaves them
    // out, and read back as only the tile changed.
    assert_eq!(read.len(), 3);
    for ((page, written), (read_page, back)) in project.map16_bg.iter().zip(&read) {
        assert_eq!(page, read_page);
        assert_eq!(back, written, "page {page:02X}");
    }
    assert!(pages::bg_table(&built, 3).unwrap().is_some());
    assert!(pages::bg_table(&built, 1).unwrap().is_none());
}

/// Pages 0 and 1 and the tilesets' own tiles, changed: a tile acting like
/// another, a common tile's graphics, a tile the game keeps per tileset in
/// two tables (tilesets 0, 7, and C share one; 4, 5, and D another), and
/// page 2 per tileset for tilesets 3 and 7.
fn game_project(clean: &Rom) -> (Project, u16, u16) {
    let game = GameTables::read(clean).unwrap();
    let common = (0x100..0x200).find(|&t| !game.is_specific(t)).unwrap();
    let specific = (0..0x200).find(|&t| game.is_specific(t)).unwrap();
    let mut page0 = GamePage::default();
    page0.tiles.insert(
        0x025,
        GameTile {
            acts: Some(0x130),
            gfx: None,
        },
    );
    let mut page1 = GamePage::default();
    page1.tiles.insert(
        common,
        GameTile {
            acts: None,
            gfx: Some(entry(0x1234).gfx),
        },
    );
    let tileset_entry = |tile: u16| Map16Entry {
        acts: DEFAULT_ACTS,
        ..entry(tile)
    };
    let mut tilesets = Vec::new();
    for (tileset, tiles) in [
        (0x0u8, vec![(specific, 0x0101u16)]),
        (0x3, vec![(0x2A0, 0x0303)]),
        (0x4, vec![(specific, 0x0404)]),
        (0x7, vec![(0x2A0, 0x0707), (0x2FF, 0x0777)]),
    ] {
        let mut page = Map16Page::default();
        for (tile, look) in tiles {
            page.tiles.insert(tile, tileset_entry(look));
        }
        tilesets.push((tileset, page));
    }
    let project = Project {
        map16_game: vec![(0x00, page0), (0x01, page1)],
        map16_tileset: tilesets,
        ..Default::default()
    };
    (project, common, specific)
}

#[test]
fn game_pages_build_and_read_back() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let (project, common, specific) = game_project(&clean);
    assert!(project.tileset_page2());
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(pages::acts_like(&built, 0x025).unwrap(), Some(0x130));
    assert_eq!(pages::acts_like(&built, 0x026).unwrap(), Some(0x026));
    let back = import::read_map16_game(&built, &clean).unwrap();
    assert!(back.notes.is_empty(), "{:?}", back.notes);
    assert_eq!(back.pages, project.map16_game);
    assert_eq!(back.tilesets, project.map16_tileset);
    // Nothing of pages 0 and 1 changed, nothing read.
    let vanilla = import::read_map16_game(&clean, &clean).unwrap();
    assert_eq!(vanilla, import::GameMap16::default());

    // What the game's uploads resolve, by level: 103 is tileset 0, 105
    // tileset 7 (sharing 0's table), 107 tileset 5 (sharing 4's), 101
    // tileset 1, 1D tileset 3.
    let tiles = [specific, common, 0x2A0, 0x2FF];
    let resolve = |level: u16| expand::resolve_map16(&built, level, &tiles).unwrap();
    let vanilla_of = |level: u16| expand::resolve_map16(&clean, level, &[specific]).unwrap()[0];
    let looks = |tile: u16| Some(entry(tile).gfx);
    let empty = Some(Map16Tile::default());
    assert_eq!(resolve(0x103), [looks(0x0101), looks(0x1234), empty, empty]);
    assert_eq!(
        resolve(0x105),
        [looks(0x0101), looks(0x1234), looks(0x0707), looks(0x0777)]
    );
    assert_eq!(resolve(0x107), [looks(0x0404), looks(0x1234), empty, empty]);
    assert_eq!(
        resolve(0x101)[..3],
        [vanilla_of(0x101), looks(0x1234), empty]
    );
    assert_eq!(resolve(0x1D)[2], looks(0x0303));
}

#[test]
fn game_pages_are_refused_where_the_rom_cannot_hold_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let (project, _, specific) = game_project(&clean);
    let refused = |project: &Project, what: &str| {
        let error = build::build(&clean, project).unwrap_err().to_string();
        assert!(error.contains(what), "{error}");
    };
    // A tileset's own tile in page 0's file.
    let mut p = project.clone();
    p.map16_game[0].1.tiles.insert(
        specific,
        GameTile {
            acts: None,
            gfx: Some(Map16Tile::default()),
        },
    );
    refused(&p, "tileset files");
    // What it acts like is not per tileset, and may be there.
    let mut p = project.clone();
    p.map16_game[0].1.tiles.insert(
        specific,
        GameTile {
            acts: Some(0x25),
            gfx: None,
        },
    );
    if common::asar().is_some() {
        build::build(&clean, &p).unwrap();
    }
    // A common tile in a tileset file.
    let mut p = project.clone();
    p.map16_tileset[1]
        .1
        .tiles
        .insert(0x025, Map16Entry::default());
    refused(&p, "same in every tileset");
    // Tilesets 0 and 7 share their table.
    let mut p = project.clone();
    p.map16_tileset[3]
        .1
        .tiles
        .insert(specific, Map16Entry::default());
    refused(&p, "0 and 7 do");
    // Page 2's graphics in its own file while it is per tileset.
    let mut p = project;
    let mut page2 = Map16Page::default();
    page2.tiles.insert(0x200, entry(0x200));
    p.map16.push((0x02, page2));
    refused(&p, "per tileset");
}

/// Every Lunar Magic hack of `KOBO_LM_ROMS`: its pages 0 and 1, imported
/// and built onto the clean ROM, give every tileset's tiles and what each
/// acts like as the hack has them, and import from the build as they did
/// from the hack. SA-1 hacks are left out: their base is not the clean ROM.
#[test]
fn the_corpus_pages_0_and_1_build_as_the_hacks_have_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let game = GameTables::read(&clean).unwrap();
    let mut failures = Vec::new();
    for (path, hack) in common::lunar_magic_roms() {
        if hack.mapping().is_sa1() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let read = import::read_map16_game(&hack, &clean).unwrap();
        let project = Project {
            map16_game: read.pages.clone(),
            map16_tileset: read.tilesets.clone(),
            ..Default::default()
        };
        let built = build::build(&clean, &project).unwrap();
        let mut differ = 0;
        for tileset in 0..kobo_core::map16::TILESET_COUNT {
            for tile in 0..0x200 {
                let at = game.address(tileset, tile);
                if built.read(at, 8).unwrap() != hack.read(at, 8).unwrap() {
                    differ += 1;
                }
            }
        }
        let acts = |rom: &Rom, tile: u16| {
            if pages::installed(rom) {
                pages::acts_like(rom, tile).unwrap().unwrap_or(tile)
            } else {
                tile
            }
        };
        let acts_differ = (0..0x200)
            .filter(|&t| acts(&built, t) != acts(&hack, t))
            .count();
        let again = import::read_map16_game(&built, &clean).unwrap();
        let same = again.pages == read.pages && again.tilesets == read.tilesets;
        let changed: usize = read.pages.iter().map(|(_, p)| p.tiles.len()).sum::<usize>()
            + read
                .tilesets
                .iter()
                .map(|(_, p)| p.tiles.len())
                .sum::<usize>();
        eprintln!(
            "{name}: {changed} tiles changed, {} notes; definitions differing {differ}, acts {acts_differ}, re-import same {same}",
            read.notes.len()
        );
        if differ > 0 || acts_differ > 0 || !same {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}
