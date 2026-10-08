//! Lunar Magic reads a Kobo build's overworld: its overworld transfer
//! (`-TransferOverworld`, its editor's "Copy Overworld to Another ROM")
//! from a build into a clean ROM leaves the overworld the build has, as
//! Kobo reads it. Run twice, with the byte Kobo writes only because Lunar
//! Magic checks it (`$04D818`) and without, when Lunar Magic must lose
//! layer 1's pages and nothing else, as docs/lunar-magic-install.md
//! records.
//!
//! Opt-in: the Lunar Magic tier, with the vanilla ROM and Asar.

mod common;

use common::lm;
use kobo_core::build::{self, Project};
use kobo_core::overworld::{EventBlock, Overworld, Start, layer1_index, layer2_index};
use kobo_core::{Rom, SnesAddr};

/// The byte Lunar Magic reads layer 1's pages only with, and the game's.
const PAGES_CHECK: SnesAddr = SnesAddr::new(0x04_D818);
const GAME_BYTE: u8 = 0x05;

/// The overworld Lunar Magic leaves when it copies `rom`'s into a clean ROM.
fn transferred(lunar_magic: &std::path::Path, clean: &Rom, rom: &Rom, name: &str) -> Overworld {
    let ws = lm::Workspace::new(lunar_magic, name, clean, Some(clean), false);
    std::fs::write(ws.path().join("source.smc"), rom.data()).unwrap();
    ws.run(&["-TransferOverworld", "rom.smc", "source.smc"]);
    Overworld::read(&ws.rom()).unwrap().in_lunar_magic_shape()
}

#[test]
fn lunar_magic_reads_a_built_overworld() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    // Some of each table: layer 1 tiles of page 1, a name, a level's
    // event, an event block, and a start.
    let base = Overworld::read(&clean).unwrap().in_lunar_magic_shape();
    let mut ours = base.clone();
    for x in 2..6 {
        ours.layer1[layer1_index(0, x, 2)] = 0x150 + u16::from(x);
    }
    ours.names[0x28] = kobo_core::source::overworld::parse_name("n", "KOBO'S HOUSE").unwrap();
    ours.level_events[0x29] = 0x30;
    let mut event = ours.event_list()[6].clone();
    event.blocks.push(EventBlock {
        place: (layer2_index(1, 10, 10) * 2) as u16,
        tiles: vec![0x1C58; 4],
    });
    ours.set_event(6, event).unwrap();
    ours.start[0] = Start {
        submap: 0,
        x: 12 * 16 + 8,
        y: 3 * 16 + 8,
    };
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(ours.changes_from(&base)),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    let theirs = transferred(&lunar_magic, &clean, &built, "overworld");
    let read = Overworld::read(&built).unwrap();
    assert!(theirs.layer1 == read.layer1, "layer 1");
    assert!(theirs.translevels == read.translevels, "translevels");
    assert!(theirs.layer2 == read.layer2, "layer 2");
    assert!(theirs.names == read.names, "names");
    assert!(theirs.event_list() == read.event_list(), "events");
    assert_eq!(theirs.start, read.start);
    assert_eq!(theirs.level_events, read.level_events);

    // Without the check byte, Lunar Magic takes layer 1 for one page.
    let mut unchecked = Rom::from_bytes(built.data().to_vec()).unwrap();
    unchecked.write(PAGES_CHECK, &[GAME_BYTE]).unwrap();
    let theirs = transferred(&lunar_magic, &clean, &unchecked, "overworld-unchecked");
    let low: Vec<u16> = read.layer1.iter().map(|&t| t & 0xFF).collect();
    assert!(theirs.layer1 == low, "layer 1 without its pages");
    assert!(theirs.layer1 != read.layer1);
    assert!(theirs.names == read.names, "names, without the check");
}
