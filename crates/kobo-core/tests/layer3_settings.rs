//! Lunar Magic's layer 3 settings (docs/lunar-magic-install.md, "Layer 3
//! settings"): Kobo's code for them (`asm/lunar-magic/layer3.asm`) against
//! a build's expectations on the vanilla ROM, and against Lunar Magic's own
//! code in every corpus hack that has it (`KOBO_LM_ROMS`).

mod common;

use kobo_core::addr::SnesAddr;
use kobo_core::build::{self, Project};
use kobo_core::exgfx::{self, GraphicsList, Layer3Settings, slot};
use kobo_core::ram::{Ram, RamAddr};
use kobo_core::source::project::Manifest;
use kobo_core::{Rom, expand, import, install};

fn project(levels: Vec<(u16, kobo_core::source::level::Level)>) -> Project {
    Project {
        root: std::path::PathBuf::from("."),
        manifest: Manifest::default(),
        levels,
        ..Default::default()
    }
}

/// Moves the player along a path of legs `dx,dy@frames`, from `start`,
/// invulnerable, with vertical scrolling on when a leg moves vertically.
fn steer(path: &str, frame: u32, ram: &mut Ram, start: (u16, u16)) {
    let (mut x, mut y) = (start.0 as i32, start.1 as i32);
    let mut left = frame as i32;
    let mut vertical = false;
    for leg in path.split('/') {
        let (step, n) = match leg.split_once('@') {
            Some((s, n)) => (s, n.parse::<i32>().unwrap()),
            None => (leg, i32::MAX),
        };
        let (dx, dy) = step
            .split_once(',')
            .map(|(a, b)| (a.parse::<i32>().unwrap(), b.parse::<i32>().unwrap()))
            .unwrap();
        vertical |= dy != 0;
        let k = left.min(n);
        x += dx * k;
        y += dy * k;
        left -= k;
        if left == 0 {
            break;
        }
    }
    for (a, v) in [(0x94, x), (0x96, y), (0xD1, x), (0xD3, y)] {
        ram.set_u16(RamAddr::new(0x7E_0000 + a), v.max(0) as u16);
    }
    ram.set_u8(RamAddr::new(0x7E_007B), 0);
    ram.set_u8(RamAddr::new(0x7E_007D), 0);
    ram.set_u8(RamAddr::new(0x7E_1497), 0x7F);
    if vertical {
        for a in [0x1412, 0x1404, 0x13F1] {
            ram.set_u8(RamAddr::new(0x7E_0000 + a), 1);
        }
    }
}

/// What layer 3's settings decide, per frame of play: its RAM, the
/// registers the frame left, and its tilemap.
#[derive(PartialEq, Debug)]
struct Frame {
    ram: Vec<u8>,
    registers: ([u16; 2], [u8; 2]),
    tilemap: Vec<u8>,
}

/// What the settings decide that the game or a patch sees: layer 3's
/// position, colour math, the screens, the scroll type, what a sprite
/// beyond the edges touches, whether the player is in water, and the tiles
/// a tide fills (the first screen's rows 16 and 17). The code's own state (`$1458`-`$1460`,
/// `$146A`-`$146D`, `$1B78`-`$1B7B`, `$7FC01A`-`$7FC01C`) is left out here:
/// older versions of Lunar Magic, whose code most corpus hacks carry, keep
/// it otherwise than 3.70 does (docs/lunar-magic-install.md).
const WATCHED: [(u32, usize); 8] = [
    (0x7E_0022, 4),  // layer 3's position
    (0x7E_0040, 1),  // CGADSUB
    (0x7E_0075, 1),  // the player in water
    (0x7E_0D9D, 2),  // TM, TS
    (0x7E_13D5, 1),  // the scroll type
    (0x7E_1693, 1),  // what a sprite touches
    (0x7E_E400, 32), // a tide's tiles
    (0x7F_E400, 32),
];

/// Where `$1693` is in a frame's [`WATCHED`] bytes.
const TOUCHED: usize = 9;

fn play(rom: &Rom, level: u16, frames: u32, path: &str) -> Result<Vec<Frame>, String> {
    let mut start = None;
    let mut out = Vec::new();
    expand::play_game_loop(
        rom,
        level,
        frames,
        |frame, ram| {
            let s = *start.get_or_insert((
                ram.u16(RamAddr::new(0x7E_0094)),
                ram.u16(RamAddr::new(0x7E_0096)),
            ));
            steer(path, frame, ram, s);
        },
        |_, played| {
            let mut ram = Vec::new();
            for (a, n) in WATCHED {
                ram.extend(played.ram.bytes(RamAddr::new(a), n));
            }
            out.push(Frame {
                ram,
                registers: (played.bg_scroll[2], played.screen_layers),
                tilemap: played.vram[0xA000..0xC000].to_vec(),
            });
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(out)
}

/// The first frame on which two ROMs differ, if any; `untouched` leaves out
/// what a sprite touches (`$1693`).
fn first_difference(
    a: &Rom,
    b: &Rom,
    level: u16,
    frames: u32,
    path: &str,
    untouched: bool,
) -> Option<String> {
    let play = |rom| {
        play(rom, level, frames, path).map(|mut frames| {
            if untouched {
                for f in &mut frames {
                    f.ram[TOUCHED] = 0;
                }
            }
            frames
        })
    };
    match (play(a), play(b)) {
        (Ok(fa), Ok(fb)) => fa
            .iter()
            .zip(&fb)
            .position(|(x, y)| x != y)
            .map(|f| format!("frame {f}: {:?} / {:?}", fa[f].ram, fb[f].ram)),
        // A level the game's own code cannot run in either is not this
        // test's business.
        (Err(_), Err(_)) => None,
        (x, y) => Some(format!("{:?} / {:?}", x.err(), y.err())),
    }
}

/// A build of level 105 with the settings moves layer 3 as they say.
#[test]
fn a_build_moves_layer3_by_its_settings() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let (mut level, _) = import::read_level(&clean, 0x105).unwrap();
    let mut list = GraphicsList::DEFAULT;
    list.set_layer3(&Layer3Settings {
        advanced: true,
        horizontal: 0x02, // half of layer 1
        vertical: 0x08,   // autoscroll, a pixel a frame
        x: 1,             // 4 tiles
        y: -2,
        cgadsub: true,
        subscreen: true,
        sprites_air: true,
        ..Layer3Settings::default()
    })
    .unwrap();
    level.graphics = Some(list);
    // On the clean ROM, and as an SA-1 project.
    for (_, built) in common::builds(&clean, &project(vec![(0x105, level)])) {
        assert_eq!(built.read_u8(exgfx::LAYER3_CHECK).unwrap(), exgfx::JSL);
        let mut seen = 0;
        let mut start = None;
        expand::play_game_loop(
            &built,
            0x105,
            120,
            |frame, ram| {
                let s = *start.get_or_insert((
                    ram.u16(RamAddr::new(0x7E_0094)),
                    ram.u16(RamAddr::new(0x7E_0096)),
                ));
                steer("3,0", frame, ram, s);
            },
            |frame, played| {
                let r = played.ram;
                let w = |a| r.u16(RamAddr::new(a));
                assert_eq!(w(0x7E_0022), (w(0x7E_001A) >> 1) + 0x40, "frame {frame}");
                // The camera stays put vertically: one pixel a frame from -32.
                assert_eq!(w(0x7E_0024), (frame as u16 + 1).wrapping_sub(0x20));
                assert_eq!(r.u8(RamAddr::new(0x7E_0040)) & 4, 4);
                assert_eq!(played.screen_layers[0] & 4, 0);
                assert_eq!(played.screen_layers[1] & 4, 4);
                seen += 1;
            },
        )
        .unwrap();
        assert_eq!(seen, 120);
    }

    // Refused: AN2's bit 12; advanced settings in a vertical level with a
    // tide.
    for (number, settings, size, mode, why) in [
        (
            0x105,
            Layer3Settings {
                unknown: true,
                ..Layer3Settings::default()
            },
            0,
            None,
            "layer 3 settings",
        ),
        (
            0x127,
            Layer3Settings {
                advanced: true,
                ..Layer3Settings::default()
            },
            0,
            Some(0x0A),
            "layer 3 settings",
        ),
    ] {
        let (mut level, _) = import::read_level(&clean, number).unwrap();
        level.size = kobo_core::level::size::LevelSize::from_byte(size);
        if let Some(mode) = mode {
            level.header.level_mode = kobo_core::level::LevelMode(mode);
            level.layer1.clear();
            level.sprites.list.clear();
        }
        let mut list = GraphicsList::DEFAULT;
        list.set_layer3(&settings).unwrap();
        level.graphics = Some(list);
        let e = build::build(&clean, &project(vec![(number, level)])).unwrap_err();
        assert!(e.to_string().contains(why), "{e}");
    }
}

/// A build of tide level 127 with what tides act like fills the tide's
/// rows with those tiles, low and high bytes, and with advanced settings
/// builds too.
#[test]
fn a_build_fills_a_tide_with_what_it_acts_like() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    // AAAA: the top row's tile and the tile under it.
    for (aaaa, top, under) in [
        (0, 0x000, 0x000),
        (1, 0x004, 0x005),
        (3, 0x159, 0x005),
        (4, 0x130, 0x130),
        (0xF, 0x20A, 0x20A),
    ] {
        let (mut level, _) = import::read_level(&clean, 0x127).unwrap();
        let mut list = GraphicsList::DEFAULT;
        list.set_layer3(&Layer3Settings {
            advanced: aaaa == 4,
            tides_act_as: aaaa,
            ..Layer3Settings::default()
        })
        .unwrap();
        level.graphics = Some(list);
        for (_, built) in common::builds(&clean, &project(vec![(0x127, level)])) {
            let loaded = expand::expand_level(&built, 0x127).unwrap();
            let tile = |at: u32| {
                let low = loaded.ram.u8(RamAddr::new(0x7E_0000 + at)) as u16;
                let high = loaded.ram.u8(RamAddr::new(0x7F_0000 + at)) as u16;
                high << 8 | low
            };
            // Each of layer 2's 16 screens, at rows 16 and 26.
            for screen in 0..16 {
                let row16 = 0xE400 + screen * 0x1B0;
                assert_eq!(tile(row16), top, "{aaaa:X}: screen {screen}");
                assert_eq!(tile(row16 + 15), top, "{aaaa:X}: screen {screen}");
                assert_eq!(tile(row16 + 16), under, "{aaaa:X}: screen {screen}");
                assert_eq!(tile(row16 + 0xAF), under, "{aaaa:X}: screen {screen}");
            }
        }
    }
}

/// In tide level 127, layer 3's vertical position stays as Lunar Magic's
/// code keeps it where it moves as a tide does (with layer 1, setting 1,
/// or by an autoscroll): not below 0, and from `$108` on within
/// `$108`-`$117`; a fraction of layer 1 leaves it as it is.
#[test]
fn a_tide_keeps_layer3_within_lunar_magics_bounds() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    for (vertical, y, bounded) in [
        (0x01, -12, true),
        (0x01, 30, true),
        (0x09, 20, true),
        (0x02, -12, false),
    ] {
        let (mut level, _) = import::read_level(&clean, 0x127).unwrap();
        let mut list = GraphicsList::DEFAULT;
        list.set_layer3(&Layer3Settings {
            advanced: true,
            vertical,
            y,
            ..Layer3Settings::default()
        })
        .unwrap();
        level.graphics = Some(list);
        for (_, built) in common::builds(&clean, &project(vec![(0x127, level)])) {
            let frames = play(&built, 0x127, 240, "0,3@120/0,-3@120").unwrap();
            let ys: Vec<i16> = frames
                .iter()
                .map(|f| i16::from_le_bytes([f.ram[2], f.ram[3]]))
                .collect();
            let inside = ys.iter().all(|&y| (0..0x118).contains(&y));
            assert_eq!(inside, bounded, "{vertical:02X} {y}: {ys:?}");
        }
    }
}

/// Tide level 127 at sizes of its own fills the rows Lunar Magic's load
/// does (docs/lunar-magic-install.md, "Layer 3 settings"): rows 16 down of
/// the screens from layer 2's first (the split's with `T`, else 16), the
/// first always and then each that ends within `$3700` bytes of the
/// planes, with water or what the tide acts like.
#[test]
fn a_tide_in_a_taller_level_fills_the_rows_lunar_magic_does() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::level::size::LevelSize;
    for (mode, split, aaaa) in [
        (0x03, false, 0),
        (0x0A, true, 1),
        (0x0C, false, 4),
        (0x18, true, 0),
    ] {
        let (mut level, _) = import::read_level(&clean, 0x127).unwrap();
        level.size = LevelSize {
            mode,
            bottom_row: false,
            split,
        };
        level.header.screens = 1;
        level.layer1.clear();
        if aaaa != 0 {
            let mut list = GraphicsList::DEFAULT;
            list.set_layer3(&Layer3Settings {
                tides_act_as: aaaa,
                ..Layer3Settings::default()
            })
            .unwrap();
            level.graphics = Some(list);
        }
        let (top, under) = match aaaa {
            0 => (0x000, 0x000),
            1 => (0x004, 0x005),
            _ => (0x130, 0x130),
        };
        let h = level.size.height() as u32;
        let total = kobo_core::level::size::SIZES[mode as usize].1 as u32;
        let first = if split {
            level.size.layer2_screen() as u32
        } else {
            16
        };
        let filled = |k: u32| k >= first && k < total && (k == first || (k + 1) * h <= 0x3700);
        for (_, built) in common::builds(&clean, &project(vec![(0x127, level.clone())])) {
            let loaded = expand::expand_level(&built, 0x127).unwrap();
            let tile = |at: u32| {
                let low = loaded.ram.u8(RamAddr::new(0x7E_C800 + at)) as u16;
                let high = loaded.ram.u8(RamAddr::new(0x7F_C800 + at)) as u16;
                high << 8 | low
            };
            for k in first.saturating_sub(1)..total {
                let row16 = k * h + 0x100;
                let (want_top, want_under) = if filled(k) {
                    (top, under)
                } else {
                    (0x025, 0x025)
                };
                assert_eq!(tile(row16), want_top, "size {mode:02X}: screen {k} row 16");
                assert_eq!(
                    tile(row16 + 16),
                    want_under,
                    "size {mode:02X}: screen {k} row 17"
                );
                assert_eq!(
                    tile(k * h + h - 1),
                    want_under,
                    "size {mode:02X}: screen {k}, last row"
                );
            }
        }
    }
}

/// A ROM with Lunar Magic's layer 3 code, with Kobo's in its place: the
/// hook sites put back to the clean ROM's bytes (with SA-1 Pack's, for an
/// SA-1 ROM), then `layer3.asm`; `None` where Asar's free space search would
/// write into a tagged block of the hack's (docs/toolchain.md, "Asar 1.91
/// RATS boundary limitation"), which a build, writing onto the clean ROM,
/// never meets.
fn with_kobos(hack: &Rom, clean: &Rom, asar: &kobo_core::asar::Asar) -> Option<Rom> {
    let mut rom = Rom::from_bytes(hack.data().to_vec()).unwrap();
    for (a, b) in [
        (0x00A01F, 0x00A024),
        (0x00A153, 0x00A156),
        (0x0194B6, 0x0194BA),
        (0x05C40C, 0x05C410),
    ] {
        let len = b - a + 1;
        let bytes = clean.read(SnesAddr::new(a), len as usize).unwrap().to_vec();
        rom.write(SnesAddr::new(a), &bytes).unwrap();
    }
    match install::apply_layer3(asar, &rom) {
        Ok(rom) => Some(rom),
        Err(kobo_core::asar::AsarError::Damaged { .. }) => None,
        Err(e) => panic!("layer3.asm: {e}"),
    }
}

/// Every corpus hack with Lunar Magic's layer 3 code from 3.10 on: with
/// Kobo's in its place, each level whose list has settings plays the same as the hack's,
/// layer 3's RAM, registers, and tilemap alike, along two paths. Levels
/// whose settings a build refuses are left out.
#[test]
fn kobos_layer3_code_plays_as_lunar_magics() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else {
        return;
    };
    let mut hacks = 0;
    let mut failures = Vec::new();
    let mut sa1 = None;
    for (path, hack) in common::lunar_magic_roms() {
        // Lunar Magic 3.0x and 2.x keep a position that scrolls below 0
        // within 11 bits and read an empty slot's settings otherwise; Kobo's
        // code does as 3.70's (and 3.1x-3.5x's) does.
        let version = hack
            .lunar_magic_version()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        if hack.read_u8(exgfx::LAYER3_CHECK).ok() != Some(exgfx::JSL)
            || version < 3.1
            || kobo_core::gfx::is_locked(&hack)
        {
            continue;
        }
        // An SA-1 hack's hook sites go back to SA-1 Pack's bytes.
        let base = if hack.mapping().is_sa1() {
            match sa1.get_or_insert_with(|| common::sa1_base(&clean)) {
                Some(base) => base,
                None => continue,
            }
        } else {
            &clean
        };
        let Some(kobo) = with_kobos(&hack, base, &asar) else {
            eprintln!(
                "{}: Kobo's code does not fit beside the hack's",
                path.display()
            );
            continue;
        };
        let mut levels = 0;
        for number in 0..0x200u16 {
            let Some(list) = exgfx::read_list(&hack, number).unwrap() else {
                continue;
            };
            if !list.has_layer3() && !(list.layer3_tilemap() && list.0[slot::AN2] != exgfx::EMPTY) {
                continue;
            }
            let s = list.layer3();
            let Ok((level, _)) = import::read_level(&hack, number) else {
                continue;
            };
            // By the game's table, or as the level loads ($1403): a hack may
            // set a tide its own way.
            let tide = exgfx::has_tide(&hack, level.header.object_tileset, level.entrance.layer3)
                .unwrap()
                || expand::expand_level(&hack, number)
                    .is_ok_and(|l| l.ram.u8(RamAddr::new(0x7E_1403)) != 0);
            if s.unknown || (tide && s.advanced && level.header.level_mode.layer1_vertical()) {
                continue;
            }
            let (frames, paths): (u32, &[&str]) = if s.advanced {
                (180, &["3,-1", "6,-5@60/-7,6"])
            } else {
                (30, &["3,-1"])
            };
            // In a tide level, what a sprite touches comes from Lunar Magic's
            // code at $00E966 as well, which stays in the hack and reads
            // Lunar Magic's layer 3 state, not Kobo's; a build has Kobo's at
            // both (entrance.asm), and plays as the hack there (QLDC 2021
            // `06_Friday`'s level 106, docs/lunar-magic-install.md).
            for p in paths {
                if let Some(d) = first_difference(&hack, &kobo, number, frames, p, tide) {
                    failures.push(format!("{}: {number:03X} {p}: {d}", path.display()));
                }
            }
            levels += 1;
        }
        eprintln!("{}: {levels} levels", path.display());
        hacks += 1;
    }
    eprintln!("{hacks} hacks with Lunar Magic's layer 3 code");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
