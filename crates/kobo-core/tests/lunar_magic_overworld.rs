//! Lunar Magic reads a Kobo build's overworld: its overworld transfer
//! (`-TransferOverworld`, its editor's "Copy Overworld to Another ROM")
//! from a build into a clean ROM leaves the overworld the build has, as
//! Kobo reads it. Run again without the byte Kobo writes only because
//! Lunar Magic checks it (`$04D818`), when Lunar Magic must lose layer 1's
//! pages and nothing else, without the bank of layer 1's 16x16 tiles
//! where Lunar Magic reads it (`$058B22`), and without the `JSL` at
//! `$00A4E3` it takes the overworld's ExAnimation as installed by, as
//! docs/lunar-magic-install.md records.
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
/// Where Lunar Magic reads the bank of layer 1's 16x16 tiles: the second
/// scroll upload's `LDY #$05` operand, `$05` in the game.
const TILES_BANK: SnesAddr = SnesAddr::new(0x05_8B22);
/// What Lunar Magic takes the overworld's ExAnimation as installed by: a
/// `JSL` here.
const ANIMATION_CHECK: SnesAddr = SnesAddr::new(0x00_A4E3);

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
    // Some of each table: layer 1 tiles of page 1, with 16x16 tiles of
    // their own, a name, a level's event, an event block, and a start.
    let base = Overworld::read(&clean).unwrap().in_lunar_magic_shape();
    let mut ours = base.clone();
    ours.tiles.resize(0x156, [0; 4]);
    for x in 2..6 {
        let tile = 0x150 + u16::from(x);
        ours.layer1[layer1_index(0, x, 2)] = tile;
        ours.tiles[usize::from(tile)] = [0x0CA0 + tile; 4];
    }
    ours.names[0x28] = kobo_core::source::overworld::parse_name("n", "KOBO'S HOUSE").unwrap();
    ours.level_events[0x29] = 0x30;
    let mut event = ours.event_list()[6].clone();
    event.blocks.push(EventBlock {
        place: (layer2_index(1, 10, 10) * 2) as u16,
        tiles: vec![0x1C58; 4],
    });
    ours.set_event(6, event).unwrap();
    // ExAnimation: a list on submap 1, a global one, and a setting.
    let mut list = kobo_core::exanimation::List {
        count: 1,
        ..Default::default()
    };
    list.slots.insert(
        0,
        kobo_core::exanimation::Slot {
            kind: 0x02,
            trigger: 0x00,
            frames_less_one: 1,
            dest: 0x0400,
            frames: vec![0xAD00, 0xAD40],
        },
    );
    let mut animation = kobo_core::exanimation::OverworldAnimation::default();
    animation.settings[2] = 0x40;
    animation.submaps[1] = Some(list.clone());
    animation.global = Some(list);
    ours.animation = Some(animation);
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
    assert!(theirs.tiles == read.tiles, "16x16 tiles");
    assert!(theirs.translevels == read.translevels, "translevels");
    assert!(theirs.layer2 == read.layer2, "layer 2");
    assert!(theirs.names == read.names, "names");
    assert!(theirs.event_list() == read.event_list(), "events");
    assert_eq!(theirs.start, read.start);
    assert_eq!(theirs.level_events, read.level_events);
    assert_eq!(theirs.animation, read.animation);

    // Without the check byte, Lunar Magic takes layer 1 for one page.
    let mut unchecked = Rom::from_bytes(built.data().to_vec()).unwrap();
    unchecked.write(PAGES_CHECK, &[GAME_BYTE]).unwrap();
    let theirs = transferred(&lunar_magic, &clean, &unchecked, "overworld-unchecked");
    let low: Vec<u16> = read.layer1.iter().map(|&t| t & 0xFF).collect();
    assert!(theirs.layer1 == low, "layer 1 without its pages");
    assert!(theirs.layer1 != read.layer1);
    assert!(theirs.names == read.names, "names, without the check");

    // Lunar Magic takes the 16x16 tiles' bank from the second scroll
    // upload's operand alone: with the game's there, it reads them from
    // bank $05.
    let mut unpointed = Rom::from_bytes(built.data().to_vec()).unwrap();
    unpointed.write(TILES_BANK, &[GAME_BYTE]).unwrap();
    let theirs = transferred(&lunar_magic, &clean, &unpointed, "overworld-unpointed");
    assert!(theirs.tiles != read.tiles, "16x16 tiles without their bank");
    assert!(
        theirs.layer1 == read.layer1,
        "layer 1, without the tiles' bank"
    );

    // Lunar Magic takes the overworld's ExAnimation as installed by the
    // JSL at $00A4E3 (Kobo's NMI hook): with the game's REP there, the
    // transfer leaves none, and the rest as it was.
    let mut unhooked = Rom::from_bytes(built.data().to_vec()).unwrap();
    unhooked.write(ANIMATION_CHECK, &[0xC2]).unwrap();
    let theirs = transferred(&lunar_magic, &clean, &unhooked, "overworld-unhooked");
    assert_eq!(theirs.animation, None, "ExAnimation without its check");
    assert!(
        theirs.layer1 == read.layer1,
        "layer 1, without the ExAnimation check"
    );
}
