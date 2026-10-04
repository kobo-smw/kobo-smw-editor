//! A hack built by Kobo, played against the hack (or against another ROM
//! with the same levels), frame by frame: for checking that what a build
//! writes plays as the ROM it was imported from (docs/testing.md, "A hack
//! built by Kobo, played").
//!
//! `built_play hack.smc|patch.bps [option...]` imports the hack's changed
//! levels into a scratch folder, builds them onto the clean ROM (leaving out
//! each level a build refuses, as `tools/corpus-sweep` does), and plays each
//! level of interest in both ROMs along each path, comparing the RAM that
//! the camera, the layers' scroll settings and layer 3, the player, and
//! what he touches leave (the watched set below, or all work RAM with
//! `all`). It prints, per level and path, the first frame that differs and
//! the runs of addresses that do, with the first values of each (left: the
//! reference, right: the build).
//!
//! Options: `against=rom` plays the build against that ROM instead of the
//! hack (a hack moved into a ROM saved by Lunar Magic 3.70 with
//! `tools/lunar-magic/transfer`, to compare Kobo's code with 3.70's on the
//! same levels without the hack's own code); `levels=105,1C7` the levels to
//! play (default: those with Lunar Magic's added layer 2 scroll settings,
//! 8 to 11 or separate ones, and tide levels with layer 3's `advanced` or
//! `tides_act_as`); `paths=still,run,...` (default all of `still`, `run`,
//! `runleft`, `carry`, `far`, `up`, `down`); `frames=N` (600); `keep=dir`
//! keeps the project and the build there.
//!
//! Paths: `still` leaves the player alone; `run` and `runleft` hold the run
//! button and a direction, jumping for 20 frames of every 48; `carry`
//! carries the player right 3 pixels a frame at his start's height; `far`
//! 8 pixels a frame for 300 frames and back; `up` and `down` one pixel
//! right and two up or down a frame. The player is kept invulnerable.
//!
//! Clean room: the hack is a ROM Lunar Magic saved; this prints RAM values
//! only, and the stacks are left out as every dump leaves them out
//! (`kobo_core::clean_room`).

use kobo_core::build::{self, BuildError, Project};
use kobo_core::ram::{Ram, RamAddr, RamMap};
use kobo_core::{Rom, bps, clean_room, exgfx, expand, import};

/// What the levels' settings and the player decide, by vanilla address
/// ranges (`$1xxxx` for `$7Fxxxx`): the layers' positions and offsets,
/// colour math, the scroll directions, the screen counts, the player, the
/// face-left and entrance RAM, the screens' layers, the scroll and tide
/// settings, the camera, the scroll commands and Lunar Magic's moving
/// settings, layer 3's offsets, what is touched, the layers' movement, and
/// the tile planes.
const WATCHED: &[(u32, u32)] = &[
    (0x1A, 0x2A),
    (0x40, 0x41),
    (0x55, 0x57),
    (0x5B, 0x5F),
    (0x71, 0x79),
    (0x7A, 0x80),
    (0x94, 0x9C),
    (0xF9, 0xFA),
    (0x0BE7, 0x0BEE),
    (0x0D9D, 0x0D9F),
    (0x13CD, 0x13CE),
    (0x13D5, 0x13D6),
    (0x13E0, 0x13E1),
    (0x13EE, 0x13F2),
    (0x1400, 0x1405),
    (0x140D, 0x1415),
    (0x1417, 0x1419),
    (0x142A, 0x1430),
    (0x143E, 0x146C),
    (0x1471, 0x1472),
    (0x1693, 0x1694),
    (0x17BC, 0x17C0),
    (0xC800, 0x10000),
    (0x1C800, 0x20000),
];

const PATHS: [&str; 7] = ["still", "run", "runleft", "carry", "far", "up", "down"];

/// Every vanilla work RAM address's bus address on this map, `None` where
/// a dump leaves it out.
fn layout(map: RamMap) -> Vec<Option<u32>> {
    (0..0x20000u32)
        .map(|i| {
            let bus = map.resolve(RamAddr::new(0x7E_0000 + i));
            (!clean_room::withheld(map, bus)).then_some(bus)
        })
        .collect()
}

fn snapshot(ram: &Ram, layout: &[Option<u32>], watched: &[bool]) -> Vec<u8> {
    layout
        .iter()
        .zip(watched)
        .map(|(bus, &w)| match bus {
            Some(bus) if w => ram.read(*bus).unwrap_or(0),
            _ => 0,
        })
        .collect()
}

fn steer(path: &str, frame: u32, m: &mut Ram, start: (u16, u16)) {
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    let pad = |m: &mut Ram, held: u16, pressed: u16| {
        m.set_u8(at(0x15), (held >> 8) as u8);
        m.set_u8(at(0x17), held as u8);
        m.set_u8(at(0x16), (pressed >> 8) as u8);
        m.set_u8(at(0x18), pressed as u8);
    };
    let carry = |m: &mut Ram, x: i32, y: i32| {
        m.set_u16(at(0x94), x.max(0) as u16);
        m.set_u16(at(0x96), y.max(0) as u16);
        m.set_u8(at(0x7B), 0);
        m.set_u8(at(0x7D), 0);
    };
    m.set_u8(at(0x1497), 0x7F);
    let (x0, y0) = (start.0 as i32, start.1 as i32);
    let f = frame as i32;
    match path {
        "still" => {}
        "run" | "runleft" => {
            let direction = if path == "run" { 0x0100 } else { 0x0200 };
            let jump = if f % 48 < 20 { 0x8000 } else { 0 };
            let pressed = if f % 48 == 0 { 0x8000 } else { 0 };
            pad(m, direction | 0x4000 | jump, pressed);
        }
        "carry" => carry(m, x0 + 3 * f, y0),
        "far" => carry(m, x0 + 8 * if f < 300 { f } else { 600 - f }, y0),
        "up" => carry(m, x0 + f, y0 - 2 * f),
        "down" => carry(m, x0 + f, y0 + 2 * f),
        _ => panic!("unknown path {path}"),
    }
}

/// Every frame's watched RAM up to the first that leaves the level (game
/// mode `$14`, the level after its death or exit being another's), and the
/// error that stopped the level, if one did.
fn play(
    rom: &Rom,
    level: u16,
    frames: u32,
    path: &str,
    all: bool,
) -> (Vec<Vec<u8>>, Option<String>) {
    let layout = layout(RamMap::of(rom));
    let watched: Vec<bool> = (0..0x20000u32)
        .map(|i| all || WATCHED.iter().any(|&(s, e)| (s..e).contains(&i)))
        .collect();
    let mut out = Vec::new();
    let mut start = None;
    let mut left = false;
    let result = expand::play_game_loop(
        rom,
        level,
        frames,
        |frame, m| {
            let s = *start.get_or_insert((
                m.u16(RamAddr::new(0x7E_0094)),
                m.u16(RamAddr::new(0x7E_0096)),
            ));
            steer(path, frame, m, s);
        },
        |_, played| {
            left |= played.ram.u8(kobo_core::ram::GAME_MODE) != 0x14;
            if !left {
                out.push(snapshot(played.ram, &layout, &watched));
            }
        },
    );
    (out, result.err().map(|e| e.to_string()))
}

/// A run of differing addresses: its first and last, the first frame any
/// differs on, and the first values of each (the reference's, the build's).
type Run = (usize, usize, usize, Vec<(u8, u8)>);

fn compare(a: &Rom, b: &Rom, level: u16, frames: u32, path: &str, all: bool) -> String {
    let (fa, ea) = play(a, level, frames, path, all);
    let (fb, eb) = play(b, level, frames, path, all);
    let mut first: std::collections::BTreeMap<usize, (usize, u8, u8)> = Default::default();
    for (frame, (x, y)) in fa.iter().zip(&fb).enumerate() {
        for i in 0..x.len() {
            if x[i] != y[i] {
                first.entry(i).or_insert((frame, x[i], y[i]));
            }
        }
    }
    let mut runs: Vec<Run> = Vec::new();
    for (&i, &(frame, x, y)) in &first {
        match runs.last_mut() {
            Some(run) if run.1 + 1 == i => {
                run.1 = i;
                run.2 = run.2.min(frame);
                if run.3.len() < 4 {
                    run.3.push((x, y));
                }
            }
            _ => runs.push((i, i, frame, vec![(x, y)])),
        }
    }
    let mut stopped = match (&ea, &eb) {
        (None, None) => String::new(),
        _ => format!(
            " (stopped: {} / {})",
            ea.as_deref().unwrap_or("-"),
            eb.as_deref().unwrap_or("-")
        ),
    };
    if fa.len() != fb.len() {
        stopped += &format!(" (in the level for {} / {} frames)", fa.len(), fb.len());
    }
    if runs.is_empty() {
        return format!("same on {} frames{stopped}", fa.len().min(fb.len()));
    }
    let first_frame = runs.iter().map(|r| r.2).min().unwrap();
    let shown: Vec<String> = runs
        .iter()
        .take(12)
        .map(|(s, e, frame, values)| {
            let name = |i: usize| {
                if i >= 0x10000 {
                    format!("$7F{:04X}", i & 0xFFFF)
                } else {
                    format!("${i:04X}")
                }
            };
            let range = if s == e {
                name(*s)
            } else {
                format!("{}-{:04X}", name(*s), e & 0xFFFF)
            };
            format!("{range}@{frame} {values:02X?}")
        })
        .collect();
    format!(
        "from frame {first_frame}: {}{}{stopped}",
        shown.join(", "),
        if runs.len() > 12 { ", ..." } else { "" }
    )
}

/// The levels Lunar Magic's added layer 2 scroll settings or a tide's
/// layer 3 settings play a part in.
fn levels_of_interest(project: &Project, hack: &Rom) -> Vec<u16> {
    project
        .levels
        .iter()
        .filter(|(number, level)| {
            let scroll = (8..12).contains(&level.entrance.layer2_scroll)
                || level.settings.layer2_vertical_scroll.is_some();
            let tide = level.graphics.as_ref().is_some_and(|list| {
                let s = list.layer3();
                (s.advanced || s.tides_act_as != 0)
                    && (exgfx::has_tide(hack, level.header.object_tileset, level.entrance.layer3)
                        .unwrap_or(false)
                        || expand::expand_level(hack, *number)
                            .is_ok_and(|l| l.ram.u8(RamAddr::new(0x7E_1403)) != 0))
            });
            scroll || tide
        })
        .map(|(number, _)| *number)
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!(
            "usage: built_play hack.smc|patch.bps [against=rom] [levels=..] [paths=..] [frames=N] [keep=dir] [all]"
        );
        std::process::exit(2);
    };
    let clean = Rom::load(kobo_core::config::vanilla_rom_path().unwrap()).unwrap();
    let hack = if path.to_ascii_lowercase().ends_with(".bps") {
        let patch = std::fs::read(path).unwrap();
        Rom::from_bytes(bps::apply_to_rom(&patch, &clean).unwrap().data).unwrap()
    } else {
        Rom::load(path).unwrap()
    };
    let mut against = None;
    let mut levels: Option<Vec<u16>> = None;
    let mut paths: Vec<String> = PATHS.iter().map(|p| p.to_string()).collect();
    let mut frames = 600;
    let mut keep = None;
    let mut all = false;
    for a in &args[1..] {
        match a.split_once('=') {
            Some(("against", v)) => against = Some(Rom::load(v).unwrap()),
            Some(("levels", v)) => {
                levels = Some(
                    v.split(',')
                        .map(|l| u16::from_str_radix(l, 16).unwrap())
                        .collect(),
                )
            }
            Some(("paths", v)) => paths = v.split(',').map(str::to_string).collect(),
            Some(("frames", v)) => frames = v.parse().unwrap(),
            Some(("keep", v)) => keep = Some(std::path::PathBuf::from(v)),
            None if a == "all" => all = true,
            _ => panic!("unknown option {a}"),
        }
    }
    let dir = keep.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("kobo-built-play-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&dir);
    let options = import::Options {
        all: false,
        pixi: None,
    };
    import::import_rom_with(&hack, &clean, &dir, &options).unwrap();
    let mut project = Project::load(&dir).unwrap();
    let built = loop {
        match build::build(&clean, &project) {
            Ok(rom) => break rom,
            Err(BuildError::Level { level, message }) => {
                println!("left out: level {level:03X}: {message}");
                project.levels.retain(|(n, _)| *n != level);
            }
            Err(e) => panic!("build: {e}"),
        }
    };
    built.save(dir.join("build.sfc")).unwrap();
    let levels = levels.unwrap_or_else(|| levels_of_interest(&project, &hack));
    let reference = against.as_ref().unwrap_or(&hack);
    for level in levels {
        if !project.levels.iter().any(|(n, _)| *n == level) {
            println!("{level:03X}: not built");
            continue;
        }
        for path in &paths {
            println!(
                "{level:03X} {path}: {}",
                compare(reference, &built, level, frames, path, all)
            );
        }
    }
}
