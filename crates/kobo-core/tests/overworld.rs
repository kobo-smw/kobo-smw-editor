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
                        && back.opened == ours.opened;
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
            && ours.opened == theirs.opened;
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
