//! The overworld as `overworld::Overworld::read` reads it, against what the
//! ROM's own load leaves in RAM (`expand::load_overworld`): the vanilla
//! ROM, and every hack of the corpus whose new game reaches an overworld.

mod common;

use kobo_core::expand::{self, ExpandError};
use kobo_core::overworld::{self, Layout, Overworld};

#[test]
fn the_vanilla_overworld_reads_as_its_load_leaves_it() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let read = Overworld::read(&clean).unwrap();
    assert_eq!(read.layout, Layout::Game);
    assert_eq!(read.events.count(), 0x6F);
    let loaded = expand::load_overworld(&clean).unwrap();
    assert_eq!(overworld::differences(&read, &loaded), Vec::<String>::new());
    // Every name's text is the game's, from its tiles as from its parts.
    for t in 0..0x60u8 {
        let level = if t < 0x25 {
            u16::from(t)
        } else {
            0x101 + u16::from(t - 0x25)
        };
        assert_eq!(
            kobo_core::level::name_text(&read.names[usize::from(t)]),
            kobo_core::level::level_name(&clean, level),
            "translevel {t:02X}"
        );
    }
    // Level names read as the game composes them: translevel 28 is
    // "YOSHI'S HOUSE".
    assert_eq!(
        &read.names[0x28][..13],
        &[
            0x18, 0x0E, 0x12, 0x07, 0x08, 0x5D, 0x12, 0x1F, 0x07, 0x0E, 0x14, 0x12, 0x04
        ]
    );
}

/// The clean ROM's overworld, built by Kobo in Lunar Magic's layout, plays
/// as the game's own does: a saved game with every event passed loads the
/// same, and so does each event's end in play, where its further tiles are
/// made.
#[test]
fn the_vanilla_overworld_built_plays_as_the_games_own() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(overworld::Changes::default()),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let all = [0xFF; 0x0F];
    let (theirs, ours) = (
        expand::load_overworld_passed(&clean, &all).unwrap(),
        expand::load_overworld_passed(&built, &all).unwrap(),
    );
    assert_eq!(
        overworld::load_differences(&theirs, &ours),
        Vec::<String>::new()
    );
    let read = Overworld::read(&clean).unwrap();
    let none = [0; 0x0F];
    let mut ended = 0;
    for (event, extras) in read.events.extras.iter().enumerate() {
        if extras.is_empty() {
            continue;
        }
        ended += 1;
        let (theirs, ours) = (
            expand::end_event(&clean, &none, event as u8).unwrap(),
            expand::end_event(&built, &none, event as u8).unwrap(),
        );
        assert_eq!(
            overworld::load_differences(&theirs, &ours),
            Vec::<String>::new(),
            "event {event:#04x}"
        );
    }
    // The game's list: 44 entries, of 18 events.
    assert_eq!(read.events.extras.iter().flatten().count(), 44);
    assert_eq!(ended, 18);
}

/// Every level of the clean ROM's overworld beaten, by its normal exit and
/// its secret one, in the game and in Kobo's build of its overworld: the
/// same event plays out, frame by frame, and leaves the same overworld and
/// video memory.
#[test]
fn the_vanilla_overworld_built_plays_its_levels_events_as_the_games_own() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(overworld::Changes::default()),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let read = Overworld::read(&clean).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut beaten = Vec::new();
    for (i, &t) in read.translevels.iter().enumerate() {
        if t != 0 && seen.insert(t) {
            let place = overworld::layer1_place(i);
            beaten.extend([(place, 1), (place, 2)]);
        }
    }
    let none = [0; 0x0F];
    let theirs = expand::beat_levels(&clean, &none, &beaten, 0x200).unwrap();
    let ours = expand::beat_levels(&built, &none, &beaten, 0x200).unwrap();
    for ((place, exit), ((theirs, their_steps), (ours, our_steps))) in
        beaten.iter().zip(theirs.iter().zip(&ours))
    {
        let at = format!("{place:?}, exit {exit}");
        assert_eq!(their_steps, our_steps, "{at}");
        assert_eq!(
            overworld::load_differences(theirs, ours),
            Vec::<String>::new(),
            "{at}"
        );
        assert!(theirs.vram == ours.vram, "{at}: VRAM differs");
    }
    assert_eq!(seen.len(), 92);
}

/// The clean ROM's overworld and Kobo's build of it draw the same, the
/// main map and each submap.
#[test]
fn the_vanilla_overworld_built_draws_as_the_games_own() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(overworld::Changes::default()),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    for submap in 0..=6 {
        let theirs = kobo_core::render::render_overworld(&clean, submap).unwrap();
        let ours = kobo_core::render::render_overworld(&built, submap).unwrap();
        assert!(theirs.pixels == ours.pixels, "submap {submap}");
        // Not a blank picture: the map has more than a few colours.
        let colours: std::collections::BTreeSet<_> = theirs.pixels.iter().collect();
        assert!(
            colours.len() > 16,
            "submap {submap}: {} colours",
            colours.len()
        );
    }
}

/// Every star and pipe tile of the clean ROM's overworld warps to the same
/// place in Kobo's build of it as in the game.
/// The title screen's layer 3, rewritten as a stripe image of Kobo's in a
/// build, shows as the game's.
#[test]
fn the_vanilla_title_screen_built_shows_as_the_games_own() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let base = Overworld::read(&clean).unwrap().in_lunar_magic_shape();
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(base.changes_from(&base)),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let ours = expand::load_title(&built).unwrap();
    let theirs = expand::load_title(&clean).unwrap();
    let layer3 = 0xA000..0xA800;
    assert!(
        ours.vram[layer3.clone()] == theirs.vram[layer3],
        "the title's layer 3"
    );
}

#[test]
fn the_vanilla_overworld_built_warps_as_the_games_own() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(overworld::Changes::default()),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let read = Overworld::read(&clean).unwrap();
    let mut places = Vec::new();
    for (i, &tile) in read.layer1.iter().enumerate() {
        if matches!(tile & 0xFF, 0x5B | 0x5F | 0x81 | 0x82) {
            let (map, x, y) = overworld::layer1_place(i);
            for submap in if map == 0 { 0..=0 } else { 1..=6 } {
                places.push((submap, x, y));
            }
        }
    }
    let theirs = expand::warp(&clean, &places).unwrap();
    let ours = expand::warp(&built, &places).unwrap();
    assert_eq!(theirs, ours);
    assert!(theirs.iter().filter(|w| w.is_some()).count() >= 18);
}

/// A translevel's settings in Lunar Magic's layout, in Kobo's build: its
/// save prompt flag (bit 4) brings up the save prompt once it is passed,
/// and its no-entry flag (bit 5) with passed keeps the player out, as Lunar
/// Magic-saved ROMs do; the game's own lets the player in whatever the bits.
#[test]
fn a_built_overworld_keeps_a_level_s_settings() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    // Yoshi's Island 1 (translevel 29, at 3, 8 of the submaps' map).
    let place = (1, 3, 8);
    let mut changes = overworld::Changes::default();
    changes.level_flags.insert(0x29, overworld::FLAG_SAVE);
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(changes),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // The overworld waits in process 5 at the end, for the prompt.
    let waits = |rom: &kobo_core::Rom| {
        let (_, steps) = expand::beat_level(rom, &[0; 0x0F], place, 1, 0x200).unwrap();
        steps.iter().rev().take(0x40).all(|s| s.0 == 5)
    };
    assert!(waits(&built));
    assert!(!waits(&clean));
    let settings = [0x00, 0x80, 0x20, 0xA0];
    assert_eq!(
        expand::enters(&built, place, &settings).unwrap(),
        [true, true, true, false]
    );
    assert_eq!(
        expand::enters(&clean, place, &settings).unwrap(),
        [true, true, true, true]
    );
}

/// Lunar Magic's option to turn the event path fade off, in Kobo's build:
/// each step of an event's layer 2 path takes ceil($40 / speed) + 1 frames,
/// as in Lunar Magic-saved ROMs, and the speed reads back from the build.
#[test]
fn a_built_overworld_reveals_paths_at_its_speed() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let changes = overworld::Changes {
        reveal_speed: Some(6),
        ..Default::default()
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(changes),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(expand::reveal_speed(&built, &clean).unwrap(), Some(6));
    assert_eq!(expand::reveal_speed(&clean, &clean).unwrap(), None);
    // A level whose event has layer 2 blocks.
    let read = Overworld::read(&clean).unwrap();
    let (index, _) = read
        .translevels
        .iter()
        .enumerate()
        .find(|&(_, &t)| {
            t != 0 && {
                let e = usize::from(read.level_events[usize::from(t)]);
                e < read.events.count() && !read.events.blocks(e).is_empty()
            }
        })
        .unwrap();
    let place = overworld::layer1_place(index);
    // The lengths of the runs of the event process's step 4.
    let steps = |rom: &kobo_core::Rom| {
        let (_, steps) = expand::beat_level(rom, &[0; 0x0F], place, 1, 0x300).unwrap();
        let mut runs: Vec<(u8, u8, usize)> = Vec::new();
        for (p, e) in steps {
            match runs.last_mut() {
                Some((q, f, n)) if (*q, *f) == (p, e) => *n += 1,
                _ => runs.push((p, e, 1)),
            }
        }
        runs.into_iter()
            .filter(|&(p, e, _)| (p, e) == (1, 4))
            .map(|(_, _, n)| n)
            .collect::<Vec<_>>()
    };
    let ours = steps(&built);
    assert!(!ours.is_empty());
    assert!(ours.iter().all(|&n| n == 12), "{ours:?}");
    assert!(steps(&clean).iter().all(|&n| n == 5));
}

/// The level a translevel enters, in Kobo's build of the clean ROM's
/// overworld: in Lunar Magic's layout by the translevel (`$1xx` from `$25`
/// on), whichever map it is on, as Lunar Magic-saved ROMs' loads take it;
/// through the overworld override (`$0109`), by the submap, as the game's.
#[test]
fn a_built_overworld_enters_levels_by_translevel() {
    use kobo_core::build::{self, Project};
    use kobo_core::ram::{self, RamAddr};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(overworld::Changes::default()),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    // The level entered from translevel `t` on `submap`, from the player's
    // place or through the override.
    let enter = |rom: &kobo_core::Rom, t: u8, submap: u8, by_override: bool| {
        let ram = expand::enter_by_exit(rom, 0, 0, false, submap, |ram| {
            ram.set_u8(ram::SUBLEVEL_COUNT, 0);
            ram.set_u8(at(0x0DD6), 0);
            ram.set_u8(at(0x1F1F), 5);
            ram.set_u8(at(0x1F20), 0);
            ram.set_u8(at(0x1F21), 6);
            ram.set_u8(at(0x1F22), 0);
            let place = 5 | 6 << 4 | if submap != 0 { 0x400 } else { 0 };
            ram.set_u8(RamAddr::new(0x7E_D000 + place), t);
            ram.set_u8(at(0x0109), if by_override { t } else { 0 });
        })
        .unwrap();
        u16::from(ram.u8(at(0x0F))) << 8 | u16::from(ram.u8(at(0x0E)))
    };
    for (t, submap, game, layout) in [
        (0x05, 0, 0x005, 0x005),
        (0x05, 1, 0x105, 0x005),
        (0x30, 0, 0x00C, 0x10C),
        (0x30, 1, 0x10C, 0x10C),
    ] {
        assert_eq!(enter(&clean, t, submap, false), game, "{t:02X} on {submap}");
        assert_eq!(
            enter(&built, t, submap, false),
            layout,
            "{t:02X} on {submap}"
        );
        assert_eq!(
            enter(&built, t, submap, true),
            game,
            "{t:02X} on {submap}, $0109"
        );
    }
}

#[test]
fn lunar_magic_overworlds_read_as_their_loads_leave_them() {
    let failures = common::failures::Failures::new(
        "overworld::lunar_magic_overworlds_read_as_their_loads_leave_them",
    );
    for (path, rom) in common::lunar_magic_roms() {
        failures.checked(&path, &rom);
        let read = match Overworld::read(&rom) {
            Ok(read) => read,
            Err(e) => {
                failures.fail(&rom, None, format!("read: {e}"));
                continue;
            }
        };
        // As changes against the clean ROM's, and back.
        if let Some(clean) = common::vanilla() {
            let clean = Overworld::read(&clean).unwrap().in_lunar_magic_shape();
            let ours = read.clone().in_lunar_magic_shape();
            match clean.clone().with(&ours.changes_from(&clean)) {
                Ok(back) => {
                    let same = back.layer1 == ours.layer1
                        && back.translevels == ours.translevels
                        && back.directions == ours.directions
                        && back.layer2 == ours.layer2
                        && back.names == ours.names
                        && back.event_list() == ours.event_list()
                        && back.events.crush == ours.events.crush
                        && back.events.reveal == ours.events.reveal
                        && back.start == ours.start
                        && back.level_flags == ours.level_flags
                        && back.level_events == ours.level_events
                        && back.tables == ours.tables;
                    if !same {
                        failures.fail(
                            &rom,
                            None,
                            "changes against the clean ROM's do not give it back",
                        );
                    }
                }
                Err(e) => failures.fail(&rom, None, format!("changes: {e}")),
            }
        }
        match expand::load_overworld(&rom) {
            Ok(loaded) => {
                for d in overworld::differences(&read, &loaded) {
                    failures.fail(&rom, None, d);
                }
            }
            // A hack whose new game goes into a level has no overworld to
            // compare with.
            Err(ExpandError::Overworld { mode: 0x14, .. }) => {}
            Err(e) => failures.fail(&rom, None, format!("load: {e}")),
        }
    }
    failures.finish();
}

/// Each hack's overworld, as changes against the clean ROM's, built by Kobo
/// in Lunar Magic's layout: the build reads back as the hack's, Kobo's
/// code for the load leaves what it reads, and a saved game with every
/// event passed loads as the hack's own does.
#[test]
fn lunar_magic_overworlds_build_and_load_as_the_hacks_have_them() {
    use kobo_core::build::{self, Project};

    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    let base = Overworld::read(&clean).unwrap().in_lunar_magic_shape();
    let failures = common::failures::Failures::new(
        "overworld::lunar_magic_overworlds_build_and_load_as_the_hacks_have_them",
    );
    for (path, rom) in common::lunar_magic_roms() {
        failures.checked(&path, &rom);
        let theirs = match Overworld::read(&rom) {
            Ok(read) => read.in_lunar_magic_shape(),
            Err(e) => {
                failures.fail(&rom, None, format!("read: {e}"));
                continue;
            }
        };
        let project = Project {
            root: std::path::PathBuf::from("."),
            overworld: Some(theirs.changes_from(&base)),
            ..Default::default()
        };
        let built = match build::build(&clean, &project) {
            Ok(built) => built,
            Err(e) => {
                failures.fail(&rom, None, format!("build: {e}"));
                continue;
            }
        };
        let ours = Overworld::read(&built).unwrap();
        let same = ours.layer1 == theirs.layer1
            && ours.translevels == theirs.translevels
            && ours.directions == theirs.directions
            && ours.layer2 == theirs.layer2
            && ours.names == theirs.names
            && ours.event_list() == theirs.event_list()
            && ours.events.crush == theirs.events.crush
            && ours.events.reveal == theirs.events.reveal
            && ours.start == theirs.start
            && ours.level_flags == theirs.level_flags
            && ours.level_events == theirs.level_events
            && ours.tables == theirs.tables
            // A hack with fewer 16x16 tiles than the game's has the
            // clean ROM's after its own.
            && ours.tiles.get(..theirs.tiles.len()) == Some(&theirs.tiles[..])
            && ours.palettes == theirs.palettes
            && ours.border == theirs.border
            && ours.title == theirs.title
            && ours.options == theirs.options
            // An install with no list and no setting is none.
            && ours.animation.as_ref().filter(|a| !a.is_empty())
                == theirs.animation.as_ref().filter(|a| !a.is_empty());
        if !same {
            failures.fail(&rom, None, "the build reads back otherwise");
        }
        match expand::load_overworld(&built) {
            Ok(loaded) => {
                for d in overworld::differences(&ours, &loaded) {
                    failures.fail(&rom, None, format!("the build's load: {d}"));
                }
            }
            Err(e) => failures.fail(&rom, None, format!("the build's load: {e}")),
        }
        // A hack whose own new game does not reach the overworld is the
        // other test's to report.
        let all = [0xFF; 0x0F];
        if let Ok(theirs) = expand::load_overworld_passed(&rom, &all) {
            match expand::load_overworld_passed(&built, &all) {
                Ok(ours) => {
                    for d in overworld::load_differences(&theirs, &ours) {
                        failures.fail(&rom, None, format!("every event passed: {d}"));
                    }
                }
                Err(e) => failures.fail(&rom, None, format!("every event passed: {e}")),
            }
        }
    }
    failures.finish();
}

/// What two plays of an overworld leave differently, frame by frame: the
/// first frame that differs and where, or `None`. WRAM is compared by bus
/// address but for the stack, the scratch at `$00`-`$0F` (which Lunar
/// Magic's code leaves otherwise), and what each implementation of its
/// ExAnimation keeps for itself (`$7FC000`-`$7FC01F`, its queue at
/// `$7FC0C0`-`$7FC0F7`), where the ROM's RAM map puts them and in WRAM.
fn first_difference(
    a: &[expand::LoadedOverworld],
    b: &[expand::LoadedOverworld],
) -> Option<String> {
    use kobo_core::ram::RamAddr;
    for (n, (x, y)) in a.iter().zip(b).enumerate() {
        if x.vram != y.vram {
            let w = (0..x.vram.len() / 2)
                .find(|&w| x.vram[2 * w..2 * w + 2] != y.vram[2 * w..2 * w + 2])
                .unwrap();
            return Some(format!("frame {n}: VRAM word ${w:04X}"));
        }
        if x.cgram != y.cgram {
            return Some(format!("frame {n}: CGRAM"));
        }
        let map = x.ram.map();
        let skip: Vec<std::ops::Range<u32>> = [
            (0x7E_0000, 0x10),
            (0x7E_0100, 0x100),
            (0x7F_C000, 0x20),
            (0x7F_C0C0, 0x38),
        ]
        .into_iter()
        .flat_map(|(at, len)| {
            let start = map.resolve(RamAddr::new(at));
            [start..start + len, at..at + len]
        })
        .collect();
        for addr in 0x7E_0000..0x80_0000u32 {
            if skip.iter().any(|r| r.contains(&addr)) {
                continue;
            }
            if x.ram.read(addr) != y.ram.read(addr) {
                return Some(format!("frame {n}: RAM ${addr:06X}"));
            }
        }
    }
    None
}

/// Kobo's code for the overworld's ExAnimation, swapped into each corpus
/// hack that has Lunar Magic's in its place and pointed at its tables
/// (`common::swap`), plays every submap's lists for 64 frames as Lunar
/// Magic's does (docs/lunar-magic-install.md, "The overworld").
#[test]
fn kobo_overworld_animation_plays_as_lunar_magics() {
    use common::swap::{Piece, swap};
    use kobo_core::SnesAddr;

    let Some(asar) = common::asar() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let failures = common::failures::Failures::new(
        "overworld::kobo_overworld_animation_plays_as_lunar_magics",
    );
    for (path, rom) in common::lunar_magic_roms() {
        if rom.read_u8(SnesAddr::new(0x04_8086)).ok() != Some(0x22) {
            continue;
        }
        failures.checked(&path, &rom);
        let ours = match swap(&asar, Piece::OverworldAnimation, &rom, &clean) {
            Ok(mut ours) => {
                // Lunar Magic's option to merge FG1-2 into SP3-4 moves the
                // game's animated tiles with them; Kobo's code sends them
                // where the game's LDY #$0750 says, which is how a build
                // that carries the option will move them.
                let merged =
                    expand::load_overworld(&rom).is_ok_and(|l| l.bg_character_base[0] == 0xE000);
                if merged {
                    ours.write(SnesAddr::new(0x00_A4EB), &[0x50, 0x77]).unwrap();
                }
                ours
            }
            Err(e) => {
                failures.fail(&rom, None, format!("swap: {e}"));
                continue;
            }
        };
        for submap in 0..7 {
            let theirs = expand::play_overworld_on(&rom, submap, 64);
            let kobo = expand::play_overworld_on(&ours, submap, 64);
            match (theirs, kobo) {
                (Ok(a), Ok(b)) => {
                    if let Some(d) = first_difference(&a, &b) {
                        failures.fail(&rom, None, format!("submap {submap}: {d}"));
                    }
                }
                (Err(e), _) => failures.fail(&rom, None, format!("submap {submap}, the hack: {e}")),
                (_, Err(e)) => failures.fail(&rom, None, format!("submap {submap}, Kobo's: {e}")),
            }
        }
    }
    failures.finish();
}
