//! Layer 2 background tables against Lunar Magic's exports: the BG Map16
//! table a level resolves through hashes as Lunar Magic's export of it
//! does. (The tilemap each level uploads is checked against its buffer and
//! table in `tests/corpus_levels.rs`.)

mod common;

use kobo_core::{expand, map16};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

fn load_fixture() -> HashMap<String, (u16, String)> {
    include_str!("fixtures/lunar_magic_map16_bg_export.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut p = l.split_whitespace();
            let sha = p.next().unwrap().to_string();
            let level = u16::from_str_radix(p.next().unwrap(), 16).unwrap();
            (sha, (level, p.next().unwrap().to_string()))
        })
        .collect()
}

#[test]
fn lunar_magic_bg_map16_matches_export() {
    let fixture = load_fixture();
    for (path, rom) in common::lunar_magic_roms() {
        let Some((level, want)) = fixture.get(&rom.sha1_hex()) else {
            eprintln!("skipping {}: no export hash in fixture", path.display());
            continue;
        };
        let loaded = expand::expand_level(&rom, *level).unwrap();
        assert!(
            loaded.tiles.bg_map16.len() >= map16::BG_TILE_COUNT,
            "{} level {level:03X} has no BG table",
            path.display()
        );
        let bytes: Vec<u8> = loaded.tiles.bg_map16[..map16::BG_TILE_COUNT]
            .iter()
            .flat_map(|t| t.to_bytes())
            .collect();
        let got: String = Sha1::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(&got, want, "{}", path.display());
    }
}
