//! Map16 tables against Lunar Magic's exports (`-ExportAllMap16`, hashes
//! in `fixtures/`): the BG Map16 table a level resolves through, and
//! foreground pages 2 and 3, which must resolve through Lunar Magic's
//! foreground table even when the level also uses the same-numbered
//! background pages. (The tilemap each level uploads is checked against
//! its buffer and table in `tests/corpus_levels.rs`.)

mod common;

use common::failures::Failures;
use common::fixtures::{records, sha1_hex};
use kobo_core::tiers::Tier;
use kobo_core::{expand, map16};

#[test]
fn lunar_magic_bg_map16_matches_export() {
    let fixture = records(include_str!("fixtures/lunar_magic_map16_bg_export.txt"));
    let failures = Failures::new("lunar_magic_map16_exports::lunar_magic_bg_map16_matches_export");
    let mut checked = 0;
    for (path, rom) in common::lunar_magic_roms() {
        let Some(record) = fixture.iter().find(|r| r[0] == rom.sha1_hex()) else {
            continue;
        };
        failures.checked(&path, &rom);
        checked += 1;
        let level = u16::from_str_radix(record[1], 16).unwrap();
        let loaded = match expand::expand_level(&rom, level) {
            Ok(loaded) => loaded,
            Err(e) => {
                failures.fail(&rom, Some(level), e.to_string());
                continue;
            }
        };
        if loaded.tiles.bg_map16.len() < map16::BG_TILE_COUNT {
            failures.fail(&rom, Some(level), "no BG table");
            continue;
        }
        let bytes: Vec<u8> = loaded.tiles.bg_map16[..map16::BG_TILE_COUNT]
            .iter()
            .flat_map(|t| t.to_bytes())
            .collect();
        let got = sha1_hex(&bytes);
        if got != record[2] {
            failures.fail(
                &rom,
                Some(level),
                format!("BG Map16 hashes {got}, export {}", record[2]),
            );
        }
    }
    common::none_checked(
        Tier::LmRoms,
        checked,
        "no hack of the corpus has a BG export hash",
    );
    failures.finish();
}

#[test]
fn foreground_pages_two_and_three_match_lunar_magic_exports() {
    let fixture = records(include_str!("fixtures/lunar_magic_map16_fg_export.txt"));
    let failures = Failures::new(
        "lunar_magic_map16_exports::foreground_pages_two_and_three_match_lunar_magic_exports",
    );
    let mut checked = 0;
    for (path, rom) in common::lunar_magic_roms() {
        let Some(record) = fixture.iter().find(|r| r[0] == rom.sha1_hex()) else {
            continue;
        };
        failures.checked(&path, &rom);
        checked += 1;
        let level = u16::from_str_radix(record[1], 16).unwrap();
        let loaded = match expand::expand_level(&rom, level) {
            Ok(loaded) => loaded,
            Err(e) => {
                failures.fail(&rom, Some(level), e.to_string());
                continue;
            }
        };
        let mut definitions = Vec::new();
        for number in record[2].split(',') {
            let n = u16::from_str_radix(number, 16).unwrap();
            match loaded.tiles.map16.get(&n) {
                Some(tile) => definitions.extend_from_slice(&tile.to_bytes()),
                None => failures.fail(&rom, Some(level), format!("missing tile {n:03X}")),
            }
        }
        let got = sha1_hex(&definitions);
        if got != record[3] {
            failures.fail(
                &rom,
                Some(level),
                format!("pages 2-3 hash {got}, export {}", record[3]),
            );
        }
    }
    common::none_checked(
        Tier::LmRoms,
        checked,
        "no hack of the corpus has a foreground export hash",
    );
    failures.finish();
}
