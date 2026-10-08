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
                        && back.events.reveal == ours.events.reveal;
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
