//! Vanilla Map16 assembly checked against Lunar Magic's `-ExportAllMap16`
//! output. The fixture holds hashes only.

mod common;

use kobo_core::map16::{self, BG_TILE_COUNT, FG_TILE_COUNT, TILESET_COUNT};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

fn load_fixture() -> HashMap<String, String> {
    include_str!("fixtures/vanilla_map16_lm_export.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut p = l.split_whitespace();
            (p.next().unwrap().to_string(), p.next().unwrap().to_string())
        })
        .collect()
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn fg_tiles_match_lunar_magic_export() {
    let Some(rom) = common::vanilla() else { return };
    let expected = load_fixture();
    for tileset in 0..TILESET_COUNT {
        // Lunar Magic exports the tileset data as stored, without the
        // diagonal pipe override the game applies at load time.
        let table = map16::vanilla_map16(&rom, tileset, false).unwrap();
        assert_eq!(table.tiles.len(), FG_TILE_COUNT + BG_TILE_COUNT);
        let got = sha1_hex(&table.bytes(0..FG_TILE_COUNT));
        assert_eq!(
            got,
            expected[&format!("fg{tileset:X}")],
            "tileset {tileset:X}"
        );
    }
}

#[test]
fn bg_tiles_match_lunar_magic_export() {
    let Some(rom) = common::vanilla() else { return };
    let expected = load_fixture();
    let table = map16::vanilla_map16(&rom, 0, true).unwrap();
    let got = sha1_hex(&table.bytes(FG_TILE_COUNT..FG_TILE_COUNT + BG_TILE_COUNT));
    assert_eq!(got, expected["bg"]);
}

#[test]
fn pipe_override_changes_eight_tiles() {
    let Some(rom) = common::vanilla() else { return };
    for tileset in 0..TILESET_COUNT {
        let plain = map16::vanilla_map16(&rom, tileset, false).unwrap();
        let patched = map16::vanilla_map16(&rom, tileset, true).unwrap();
        let changed: Vec<usize> = (0..FG_TILE_COUNT)
            .filter(|&t| plain.tiles[t] != patched.tiles[t])
            .collect();
        if tileset == 0 || tileset == 7 {
            assert_eq!(
                changed,
                [0x1C4, 0x1C5, 0x1C6, 0x1C7, 0x1EC, 0x1ED, 0x1EE, 0x1EF]
            );
        } else {
            assert!(changed.is_empty(), "tileset {tileset:X}");
        }
    }
}
