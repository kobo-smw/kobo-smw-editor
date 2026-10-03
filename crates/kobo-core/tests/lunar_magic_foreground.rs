//! Foreground pages 2-3 must resolve through Lunar Magic's foreground
//! table, even when the level also uses the same-numbered background pages.

mod common;

use kobo_core::expand;
use sha1::{Digest, Sha1};

#[test]
fn foreground_pages_two_and_three_match_lunar_magic_exports() {
    let fixture = include_str!("fixtures/lunar_magic_map16_fg_export.txt");
    for (path, rom) in common::lunar_magic_roms() {
        let sha = rom.sha1_hex();
        let Some(line) = fixture.lines().find(|line| line.starts_with(&sha)) else {
            eprintln!("skipping {}: no foreground export fixture", path.display());
            continue;
        };
        let fields: Vec<_> = line.split_whitespace().collect();
        let level = u16::from_str_radix(fields[1], 16).unwrap();
        let loaded = expand::expand_level(&rom, level).unwrap();
        let mut definitions = Vec::new();
        for number in fields[2].split(',') {
            let n = u16::from_str_radix(number, 16).unwrap();
            let tile = loaded.tiles.map16.get(&n).unwrap_or_else(|| {
                panic!("{} level {level:03X}: missing tile {n:03X}", path.display())
            });
            definitions.extend_from_slice(&tile.to_bytes());
        }
        let got = format!("{:x}", Sha1::digest(&definitions));
        assert_eq!(got, fields[3], "{} level {level:03X}", path.display());
    }
}
