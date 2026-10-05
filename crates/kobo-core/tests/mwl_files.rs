//! MWL files Lunar Magic exported, checked against the ROM they came
//! from. `KOBO_MWL_DIR` names a directory of directories, each holding a
//! ROM and the MWL files `tools/lunar-magic/export-mwl` wrote from it
//! (docs/testing.md). Every file must read and write back byte for byte,
//! decode, encode to a file that decodes the same, and agree with its
//! level in the ROM, apart from the rewrites Lunar Magic is known to make
//! on export (docs/lunar-magic.md), which are counted.

mod common;

use common::temp::TempDir;

use kobo_core::tiers::Tier;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use sha1::{Digest, Sha1};

use kobo_core::entrance::{self, EntranceSettings, LevelSettings};
use kobo_core::level::objects::{self, Layout, Object, ScreenExit};
use kobo_core::level::{self, BACKGROUND_TILES, Layer2Data as Pointer, Layer2Kind, LevelFormat};
use kobo_core::mwl::{self, Layer2Data, Mwl, MwlFile};
use kobo_core::{Rom, SnesAddr, palette, sprites};

/// Where vanilla's shared empty level keeps its data.
const EMPTY_LEVEL: SnesAddr = SnesAddr::new(0x068000);
/// The background Lunar Magic exports for levels whose layer 1 is there.
const EMPTY_LEVEL_BACKGROUND: SnesAddr = SnesAddr::new(0xFFDE54);
/// Lunar Magic's own screen exit format and the flags it says per exit.
const EXIT_LUNAR: u8 = 0x04;
const EXIT_SECONDARY: u8 = 0x02;

/// Screen exits as the game takes them, in Lunar Magic's format: in the
/// game's own the last exit's `s` holds for all of them and the high bit
/// of the destination is the level's.
fn resolve_exits(objects: &[Object], level: u16) -> Vec<Object> {
    let secondary = objects
        .iter()
        .rev()
        .find_map(|o| match o {
            Object::ScreenExit(e) => Some(e.flags & EXIT_SECONDARY != 0),
            _ => None,
        })
        .unwrap_or(false);
    objects
        .iter()
        .map(|o| match o {
            Object::ScreenExit(e) if e.flags & EXIT_LUNAR == 0 => Object::ScreenExit(ScreenExit {
                flags: e.flags & 0x08
                    | EXIT_LUNAR
                    | if secondary { EXIT_SECONDARY } else { 0 }
                    | (level >> 8) as u8,
                ..*e
            }),
            other => other.clone(),
        })
        .collect()
}

/// Screen exits apart, sorted: Lunar Magic moves them.
fn split_exits(objects: Vec<Object>) -> (Vec<String>, Vec<Object>) {
    let (exits, rest): (Vec<_>, Vec<_>) = objects
        .into_iter()
        .partition(|o| matches!(o, Object::ScreenExit(_)));
    let mut exits: Vec<String> = exits.iter().map(|e| format!("{e:?}")).collect();
    exits.sort();
    (exits, rest)
}

/// The level without the lengths of its encoded lists, which depend on
/// the encoder's choices.
fn without_lengths(mut mwl: Mwl) -> Mwl {
    mwl.layer1.data.len = 0;
    if let Layer2Data::Objects(data) = &mut mwl.layer2.data {
        data.len = 0;
    }
    mwl.sprites.list.len = 0;
    mwl
}

/// Rewrites seen, by kind.
type Counts = BTreeMap<&'static str, usize>;

struct RomInfo<'a> {
    rom: &'a Rom,
    format: LevelFormat,
    /// Lunar Magic's version, 0 for none.
    version: f32,
    /// Lunar Magic moved the secondary entrance tables away from the
    /// game's, which Kobo does not follow.
    relocated_entrances: bool,
    /// Where the ROM keeps Lunar Magic's entrance settings.
    layout: entrance::Layout,
}

fn check_level(info: &RomInfo, file: &MwlFile, mwl: &Mwl, name: &str, counts: &mut Counts) {
    let (rom, level) = (info.rom, mwl.info.level);
    let mut count = |kind| *counts.entry(kind).or_default() += 1;
    let objects = level::read_objects(rom, level).unwrap();
    let byte = |table: SnesAddr| rom.read_u8(table.add(level as u32)).unwrap();

    // Layer 1: the header byte for byte, the objects as the game takes
    // them, screen exits in any order. Lunar Magic rewrites objects 3C to
    // 3F in tileset 4, and one level's vertical scroll.
    let layer1_ptr = level::layer1_ptr(rom, level).unwrap();
    assert_eq!(mwl.layer1.header.source(), Some(layer1_ptr), "{name}");
    let (header, rom_header) = (mwl.layer1.data.header, objects.layer1.header);
    if header != rom_header {
        let scroll_only =
            (0..4).all(|i| header[i] == rom_header[i]) && (header[4] ^ rom_header[4]) & !0x30 == 0;
        assert!(
            scroll_only && level == 0xC5,
            "{name}: primary header {rom_header:02X?} -> {header:02X?}"
        );
        count("level 0C5's vertical scroll");
    }
    let (our_exits, ours) = split_exits(resolve_exits(&objects.layer1.objects, level));
    let (their_exits, theirs) = split_exits(mwl.layer1.data.objects.clone());
    assert_eq!(our_exits, their_exits, "{name}: screen exits");
    if ours != theirs {
        let other = |o: &&Object| {
            !matches!(
                o,
                Object::Standard {
                    number: 0x3C..=0x3F,
                    ..
                }
            )
        };
        assert!(
            objects.header().object_tileset == 4
                && ours.iter().filter(other).eq(theirs.iter().filter(other)),
            "{name}: layer 1 objects"
        );
        count("objects 3C-3F rewritten in tileset 4");
    }

    // Layer 2, as the ROM's pointer and the file's flags say.
    let pointer = level::layer2_ptr(rom, level).unwrap();
    let mode = mwl.layer1.primary_header().level_mode;
    match (&mwl.layer2.data, pointer) {
        (Layer2Data::Objects(data), Pointer::Objects(addr)) => {
            let layout = if mode.layer2() == Layer2Kind::VerticalObjects {
                Layout::Vertical
            } else {
                Layout::Horizontal
            };
            let ours = objects::decode(rom.read_tail(addr).unwrap(), layout, info.format.jumps);
            assert_eq!(ours.unwrap().objects, data.objects, "{name}: layer 2");
        }
        (Layer2Data::Background(tiles), pointer) => {
            let raw = match pointer {
                Pointer::Objects(addr) => addr,
                Pointer::Tilemap(addr) => SnesAddr::from_bank_offset(0xFF, addr.offset()),
            };
            let source = mwl.layer2.header.source();
            if source != Some(raw) {
                assert!(
                    layer1_ptr == EMPTY_LEVEL && source == Some(EMPTY_LEVEL_BACKGROUND),
                    "{name}: background from {source:?}, pointer {raw}"
                );
                count("empty level's background replaced");
            } else {
                let bg = level::read_background_at(rom, level, pointer).unwrap();
                // Past the end of a short stream, a tile is whatever Lunar
                // Magic's buffer held: one that padding the stream changes.
                let padded = |fill| {
                    let mut bg = bg.clone();
                    bg.data.resize(4 * BACKGROUND_TILES, fill);
                    bg.tiles()
                };
                let (zeros, ones) = (padded(0x00), padded(0xFF));
                let ours = bg.tiles();
                let differ: Vec<usize> = (0..ours.len()).filter(|&i| ours[i] != tiles[i]).collect();
                assert!(
                    differ.iter().all(|&i| zeros[i] != ones[i]),
                    "{name}: background tiles {differ:?}"
                );
                if !differ.is_empty() {
                    count("background stream short");
                }
            }
        }
        (_, pointer) => panic!("{name}: layer 2 is objects in the file, {pointer:?} in the ROM"),
    }

    // Sprites, with the ROM's PIXI extension bytes.
    let list = sprites::read_sprites(rom, level).unwrap();
    assert_eq!(list.header, mwl.sprites.list.header, "{name}");
    assert_eq!(list.sprites, mwl.sprites.list.sprites, "{name}: sprites");
    let sprite_ptr = level::sprite_ptr(rom, level).unwrap();
    assert_eq!(mwl.sprites.header.source(), Some(sprite_ptr), "{name}");
    let stored = rom.read(sprite_ptr, list.len).unwrap();
    assert_eq!(&file.sections[3][8..], stored, "{name}: sprite bytes");

    // Header bytes. Before 3.00 Lunar Magic had a layer 2 scroll setting
    // 8, which it exports as 3, and a Y bit in `$05DE00`, which it moves
    // to `$06FC00`.
    let secondary = level::read_secondary_header(rom, level).unwrap();
    if secondary != mwl.info.secondary {
        let rewritten = level::SecondaryHeader {
            layer2_scroll: 3,
            ..secondary
        };
        assert!(
            info.version < 3.0 && secondary.layer2_scroll == 8 && rewritten == mwl.info.secondary,
            "{name}: secondary header"
        );
        count("layer 2 scroll 8 exported as 3");
    }
    if info.format.lunar_magic {
        let rom_byte = byte(mwl::tables::SECONDARY_HEADER_5);
        let byte5 = mwl.info.secondary_lm[0];
        if info.version < 3.0 && !mode.layer1_vertical() {
            assert_eq!(rom_byte & !0x10, byte5, "{name}: $05DE00");
            assert_eq!((rom_byte >> 4) & 1, mwl.info.lm3[0] & 1, "{name}: Y bit");
            if rom_byte & 0x10 != 0 {
                count("$05DE00 Y bit moved to $06FC00");
            }
        } else if info.version < 3.0 {
            assert_eq!(rom_byte, byte5, "{name}: $05DE00");
        } else {
            assert_eq!(rom_byte, byte5, "{name}: $05DE00");
            for (at, table) in [
                (0, mwl::tables::SECONDARY_HEADER_FC),
                (1, mwl::tables::SECONDARY_HEADER_FE),
            ] {
                assert_eq!(byte(table), mwl.info.lm3[at], "{name}: {table}");
            }
        }
        if info.version >= 3.4 {
            let rom_byte = byte(mwl::tables::SECONDARY_HEADER_FA);
            assert_eq!(rom_byte, mwl.info.lm3[3], "{name}: $06FA00");
        }
        // Lunar Magic 1.62 had no animation settings: the table is $FF.
        let settings = byte(mwl::tables::ANIMATION_SETTINGS);
        if info.version >= 2.0 {
            assert_eq!(settings, mwl.animation.settings(), "{name}: $03FE00");
        }
    }
    assert_eq!(mwl.info.secondary_lm[1..], [0, 0], "{name}");
    assert_eq!(mwl.info.midway[4], 0, "{name}");
    assert!(mwl.info.rest.iter().all(|&b| b == 0), "{name}");

    // The palette: the custom one, or the one the header selects, apart
    // from what Lunar Magic shows for the player (row 8) and layer 3
    // (rows 0 and 1, colours 8 to 15), which the game sets elsewhere.
    let custom = palette::lm_level_palette(rom, level).unwrap();
    assert_eq!(mwl.layer1.custom_palette(), custom.is_some(), "{name}");
    match custom {
        Some(c) => {
            assert_eq!(c.palette, mwl.palette.colors, "{name}: palette");
            assert_eq!(c.back_area, mwl.palette.back_area, "{name}: back area");
        }
        None => {
            let select = objects.header().palette_select();
            let ours = palette::vanilla_level_palette(rom, select).unwrap();
            let theirs = &mwl.palette.colors;
            for i in 0..256 {
                let (row, col) = (i / 16, i % 16);
                if row == 8 || (row < 2 && col >= 8) || ours.colors[i].0 == 0 {
                    continue;
                }
                assert_eq!(ours.colors[i], theirs.colors[i], "{name}: colour {i}");
            }
            let back = palette::vanilla_back_area_color(rom, select.back_area).unwrap();
            assert_eq!(back, mwl.palette.back_area, "{name}: back area");
        }
    }

    // The level's ExAnimation list, as Kobo reads it from the ROM and from
    // the file. Lunar Magic 2.41's older lists Kobo does not read from a ROM
    // (exanimation::Layout::Older); the export converts them.
    let ours = kobo_core::exanimation::read_level(rom, level)
        .unwrap_or_else(|e| panic!("{name}: ExAnimation: {e}"))
        .map(|found| found.list);
    let theirs = match mwl.animation.data.as_slice() {
        [] => None,
        data => Some(kobo_core::exanimation::List::parse(data).unwrap().0),
    };
    let older = matches!(
        kobo_core::exanimation::layout(rom),
        Ok(Some(kobo_core::exanimation::Layout::Older))
    );
    if !older {
        assert_eq!(ours, theirs, "{name}: ExAnimation");
    }
    if let Some(list) = &theirs {
        assert_eq!(
            list.to_bytes(),
            mwl.animation.data,
            "{name}: ExAnimation bytes"
        );
    }

    // Lunar Magic's per-level and midway settings, as Kobo reads them from
    // the ROM and from the file.
    let vertical = mode.layer1_vertical();
    let ours = entrance::read_level_settings(rom, &info.layout, level, vertical).unwrap();
    let [m1, m2, m3, m4, _] = mwl.info.midway;
    let [fc, fe, _, fa] = mwl.info.lm3;
    let theirs =
        LevelSettings::from_bytes([mwl.info.secondary_lm[0], fa, fc, fe], [m1, m2, m3, m4]);
    assert_eq!(ours, theirs, "{name}: Lunar Magic's settings");

    // Secondary entrances: each as the game's tables have it, and all of
    // those in use that lead here. Lunar Magic sets bit 3 of `$05FE00` to
    // bit 8 of the destination; the game takes it from the entrance's
    // number. Levels 000 and 100 get none of the unused entries that
    // point at them.
    // Where Lunar Magic moved the tables, the first 512 entrances are
    // compared through its pointers; a level's full list is not.
    {
        let t = level::entrance_tables(rom);
        let entrance = |table: usize, id: u16| rom.read_u8(t[table].add(id as u32)).unwrap();
        for e in &mwl.entrances.entries {
            if e.id >= 0x200 {
                assert!(info.relocated_entrances, "{name}: entrance {:03X}", e.id);
                continue;
            }
            let high = ((level >> 8) as u8) << 3;
            let mut last = entrance(3, e.id) | high;
            if info.version < 3.0 && !mode.layer1_vertical() {
                // `IPYXDAAA` before 3.00: the Y bit moves to the first of
                // Lunar Magic 3's tables, as the main entrance's does, if
                // `P` says to use it. Vertical levels keep it, where 3.00
                // stopped swapping X and Y.
                let y = (last >> 5) & (last >> 6) & 1;
                assert_eq!(y, e.lm[0] & 1, "{name}: entrance {:03X}", e.id);
                if last & 0x20 != 0 {
                    count("entrance Y bit moved");
                }
                last &= !0x20;
            }
            let ours = [entrance(1, e.id), entrance(2, e.id), last];
            assert_eq!(ours, e.tables, "{name}: entrance {:03X}", e.id);
            assert_eq!(
                entrance(0, e.id),
                level as u8,
                "{name}: entrance {:03X}",
                e.id
            );
            assert_eq!(e.unused, 0, "{name}");
            // Lunar Magic's settings, as Kobo reads them from the ROM (in
            // Lunar Magic 3's layout, whatever the version) and from the
            // file. A fresh install's extra tables stop short of the last
            // two entrances, where the file has the bytes after them.
            if (e.id as usize) < entrance::tables::ENTRANCE_EXTRA_LEN {
                let ours = entrance::read_entrance_settings(
                    rom,
                    &info.layout,
                    e.id,
                    entrance(3, e.id),
                    mode.layer1_vertical(),
                )
                .unwrap();
                let theirs = EntranceSettings::from_bytes(e.tables, e.lm);
                assert_eq!(ours, theirs, "{name}: entrance {:03X} settings", e.id);
            }
        }
        if level & 0xFF != 0 && !info.relocated_entrances {
            let leads_here = |id: u16| {
                let high = if info.format.lunar_magic {
                    (entrance(3, id) >> 3) as u16 & 1
                } else {
                    id >> 8
                };
                let used = (0..4).any(|table| entrance(table, id) != 0);
                used && entrance(0, id) == level as u8 && high == level >> 8
            };
            let ours: Vec<u16> = (0..0x200).filter(|&id| leads_here(id)).collect();
            let mut theirs: Vec<u16> = mwl.entrances.entries.iter().map(|e| e.id).collect();
            theirs.sort();
            assert_eq!(ours, theirs, "{name}: entrances");
        }
    }

    // ExAnimation, as stored where the header says. Lunar Magic 2.41
    // stored an older format, which it converts.
    match mwl.animation.source() {
        Some(src) => {
            let data = &mwl.animation.data;
            if rom.read(src, data.len()).unwrap() != &data[..] {
                assert!(info.version < 2.43, "{name}: ExAnimation");
                count("ExAnimation converted");
            }
        }
        _ => assert!(mwl.animation.data.is_empty(), "{name}"),
    }
    // What Lunar Magic writes for the ExGFX of a ROM it has not saved.
    if !info.format.lunar_magic {
        let mut files = [0x7F; 16];
        files[8] = 0xFFFF;
        files[12..].copy_from_slice(&[0x2B, 0x2A, 0x29, 0x28]);
        assert_eq!(mwl.exgfx.0, files, "{name}: ExGFX");
    }
    count("levels");
}

/// ROM hashes and the hash of the MWL files exported from each.
fn load_fixture() -> HashMap<String, String> {
    include_str!("fixtures/lunar_magic_mwl_export.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut p = l.split_whitespace();
            (p.next().unwrap().to_string(), p.next().unwrap().to_string())
        })
        .collect()
}

/// The directories of `KOBO_MWL_DIR` that hold a ROM and MWL files.
fn export_dirs() -> Option<Vec<(PathBuf, PathBuf)>> {
    let root = common::tier_path(Tier::Mwl)?;
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .expect("the MWL exports must be a directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .filter_map(|dir| {
            let rom = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|e| e == "smc" || e == "sfc"))?;
            Some((dir, rom))
        })
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "the MWL exports hold no exported ROM");
    Some(dirs)
}

#[test]
fn exported_levels_match_their_rom() {
    let Some(dirs) = export_dirs() else { return };
    let fixture = load_fixture();
    for (dir, rom_path) in dirs {
        let rom = Rom::load(&rom_path).unwrap();
        let sizes = sprites::pixi_size_table(&rom).unwrap();
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "mwl"))
            .collect();
        paths.sort();
        let mut hash = Sha1::new();
        for path in &paths {
            hash.update(std::fs::read(path).unwrap());
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let dir_name = dir.file_name().unwrap().to_string_lossy().into_owned();
        match fixture.get(&rom.sha1_hex()) {
            Some(want) => assert_eq!(&digest, want, "{dir_name}: MWL files"),
            None => eprintln!(
                "{dir_name}: not in the fixture: {} {digest}",
                rom.sha1_hex()
            ),
        }
        let levels: Vec<(String, MwlFile, Mwl)> = paths
            .iter()
            .map(|path| {
                let name = path.display().to_string();
                let bytes = std::fs::read(path).unwrap();
                let file = MwlFile::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(file.to_bytes(), bytes, "{name}: container");
                let mwl = file.decode(sizes).unwrap_or_else(|e| panic!("{name}: {e}"));
                let again = mwl
                    .to_file(sizes)
                    .and_then(|f| f.decode(sizes))
                    .unwrap_or_else(|e| panic!("{name}: encoded: {e}"));
                assert!(
                    without_lengths(again) == without_lengths(mwl.clone()),
                    "{name}: encoded"
                );
                // Files are named after their level: `level 105.mwl`.
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                let number = stem.rsplit(' ').next().unwrap();
                assert_eq!(
                    u16::from_str_radix(number, 16),
                    Ok(mwl.info.level),
                    "{name}"
                );
                (name, file, mwl)
            })
            .collect();
        let info = RomInfo {
            rom: &rom,
            format: LevelFormat::of(&rom),
            version: rom
                .lunar_magic_version()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            relocated_entrances: levels
                .iter()
                .any(|(_, _, m)| m.entrances.entries.iter().any(|e| e.id >= 0x200)),
            layout: entrance::Layout::of(&rom),
        };
        let mut counts = Counts::new();
        for (name, file, mwl) in &levels {
            check_level(&info, file, mwl, name, &mut counts);
        }
        eprintln!("{dir_name}: {counts:?}");
    }
}

/// Every vanilla level imported from its MWL export into one project and
/// built reads back as vanilla's, but for what Lunar Magic changes on
/// export (docs/lunar-magic.md): the background of levels sharing the
/// empty level, level `0C5`'s vertical scroll, the tileset 4 objects
/// and exits of eleven levels, and the entrances of levels `000` and `100`.
#[test]
fn vanilla_exports_import_and_build() {
    let Some(dirs) = export_dirs() else { return };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some((dir, _)) = dirs.into_iter().find(|(_, rom)| {
        kobo_core::Rom::load(rom).is_ok_and(|r| r.identify() == kobo_core::RomIdentity::VanillaUsa)
    }) else {
        common::skip(Tier::Mwl, "the MWL exports hold no vanilla export");
        return;
    };
    let project_dir = TempDir::unmade("mwl-import");
    for level in 0..0x200u16 {
        let file = dir.join(format!("level {level:03X}.mwl"));
        let bytes = std::fs::read(&file).unwrap();
        kobo_core::import::import_mwl(&bytes, &clean, &project_dir, None).unwrap();
    }
    let project = kobo_core::build::Project::load(&project_dir).unwrap();
    assert_eq!(project.levels.len(), 512);
    let built = kobo_core::build::build(&clean, &project).unwrap();
    let empty = kobo_core::SnesAddr::new(0x068000);
    for diff in kobo_core::import::diff_levels(&built, &clean) {
        let n = diff.level;
        let parts: Vec<&str> = diff.parts.iter().map(String::as_str).collect();
        let expected = match parts[..] {
            ["layer2"] | ["layer2", "background"] => {
                kobo_core::level::layer1_ptr(&clean, n).unwrap() == empty
            }
            ["header"] => n == 0x0C5,
            // A vanilla ROM has no ExAnimation settings; the export gives
            // every level 0, which is not what a build gives level 104
            // (its lists off, as Lunar Magic's first save sets them).
            ["animation"] => n == 0x104,
            // Which never-used entrances Lunar Magic exports with levels
            // 000 and 100 is not known (docs/lunar-magic.md).
            // Both are among the eleven whose layer 1 is rewritten.
            ["entrances"] | ["layer1", "entrances"] => n & 0xFF == 0,
            ["layer1"] => [
                0x000, 0x0BD, 0x0DA, 0x0E6, 0x0F4, 0x0FD, 0x100, 0x1BB, 0x1BC, 0x1E4, 0x1F7,
            ]
            .contains(&n),
            _ => false,
        };
        assert!(expected, "level {n:03X}: {parts:?}");
    }
    // `diff_levels` puts exits in one format, so the bytes are checked
    // here: the game's format, as the clean ROM has them.
    let exits = |rom: &kobo_core::Rom, n: u16| {
        let data = kobo_core::level::read_objects(rom, n).unwrap();
        let mut exits: Vec<_> = data
            .layer1
            .objects
            .into_iter()
            .filter_map(|o| match o {
                kobo_core::level::objects::Object::ScreenExit(exit) => Some(exit),
                _ => None,
            })
            .collect();
        exits.sort_by_key(|e| e.screen);
        exits
    };
    for n in 0..0x200u16 {
        assert_eq!(exits(&built, n), exits(&clean, n), "level {n:03X}'s exits");
    }
}

/// A background in Lunar Magic's layout imports from an MWL file as it
/// does from the ROM the file came from, but for Lunar Magic's older
/// format (`C` without `F`), which the export folds into table 0 and a
/// ROM import reads with the tiles' high byte as the table.
#[test]
fn backgrounds_import_from_mwl_as_from_the_rom() {
    use kobo_core::source::level::Layer2;
    let Some(dirs) = export_dirs() else { return };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let mut compared = 0;
    for (dir, rom) in dirs {
        let Ok(rom) = kobo_core::Rom::load(&rom) else {
            continue;
        };
        for level in 0..0x200u16 {
            let Ok((from_rom, _)) = kobo_core::import::read_level(&rom, level) else {
                continue;
            };
            let Layer2::Background(expected) = &from_rom.layer2 else {
                continue;
            };
            let bytes = std::fs::read(dir.join(format!("level {level:03X}.mwl"))).unwrap();
            let Ok(mwl) = MwlFile::parse(&bytes).unwrap().decode(None) else {
                continue;
            };
            let (from_mwl, _) = kobo_core::import::level_from_mwl(&mwl, level, &clean).unwrap();
            let Layer2::Background(got) = &from_mwl.layer2 else {
                panic!("{}: level {level:03X} has no background", dir.display());
            };
            let older = mwl.layer2.header.0[0] == 0x06
                && got.table == 0
                && expected.table != 0
                && got.tiles == expected.tiles;
            assert!(
                got == expected || older,
                "{}: level {level:03X}",
                dir.display()
            );
            compared += 1;
        }
    }
    eprintln!("{compared} backgrounds compared");
}
