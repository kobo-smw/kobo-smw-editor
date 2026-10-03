//! Layer 2 background tilemaps: the buffer the loader captures and the BG
//! Map16 table it resolves through must reproduce the tilemap the game
//! itself uploaded to VRAM during level preparation. This catches both a
//! stale or clobbered `$7EB900` capture and a BG table read from the wrong
//! place, on the vanilla ROM and on any Lunar Magic hacks in `KOBO_LM_ROMS`.

mod common;

use kobo_core::expand::{self, LoadedLevel, SCREEN_COLS};
use kobo_core::{Rom, map16, ram};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

/// Where an 8x8 tilemap entry lives in VRAM for a `BGnSC` value, wrapping
/// like the hardware does: `(sx, sy)` sub-screens of 32x32 words.
fn vram_offset(bg_sc: u8, col8: usize, row8: usize) -> usize {
    let base = ((bg_sc >> 2) as usize) << 11;
    let wide = (bg_sc & 1) as usize + 1;
    let tall = ((bg_sc >> 1) & 1) as usize + 1;
    let col8 = col8 % (32 * wide);
    let row8 = row8 % (32 * tall);
    base + ((row8 / 32) * wide + col8 / 32) * 0x800 + (row8 % 32) * 64 + (col8 % 32) * 2
}

/// The background rows the game uploads. A 64-tall tilemap takes the whole
/// two-screen background. Lunar Magic's 32-tall tilemap takes the 16 rows
/// from one above the layer 2 position, `shift` rows further down (as VRAM
/// shows after a load). In that format, rows 27-31 of a 27-row background
/// hold what lies past it and are not checked.
fn uploaded_rows(loaded: &LoadedLevel, shift: isize) -> std::ops::Range<isize> {
    let rows = loaded.tiles.layer2_bg_rows() as isize;
    if loaded.video.bg_sc[1] & 0x02 != 0 {
        return 0..rows;
    }
    let y = loaded.ram.u16(ram::LAYER2_Y) as isize;
    let first = y / 16 - 1 + shift;
    first..first + 16
}

/// For each VRAM word of the layer 2 tilemap the game uploaded, the word
/// the captured background and BG Map16 table say it should hold.
fn candidates(loaded: &LoadedLevel, shift: isize) -> HashMap<usize, Vec<[u8; 2]>> {
    let mut out: HashMap<usize, Vec<[u8; 2]>> = HashMap::new();
    for world_y in uploaded_rows(loaded, shift) {
        let y = world_y.rem_euclid(32) as usize;
        if y >= loaded.tiles.layer2_bg_rows() {
            continue;
        }
        for screen in 0..2 {
            for x in 0..SCREEN_COLS {
                let n = loaded.tiles.layer2_bg_tile(screen, x, y).unwrap();
                let def = loaded.tiles.bg_map16[n as usize - 0x200].to_bytes();
                let col8 = 2 * (screen * SCREEN_COLS + x);
                let row8 = (2 * world_y).rem_euclid(64) as usize;
                // Definition order is TL, BL, TR, BR.
                for (q, (dx, dy)) in [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().enumerate() {
                    let at = vram_offset(loaded.video.bg_sc[1], col8 + dx, row8 + dy);
                    out.entry(at)
                        .or_default()
                        .push([def[2 * q], def[2 * q + 1]]);
                }
            }
        }
    }
    out
}

/// (words checked, words that were never uploaded or differ). The upload
/// comes before the level loop's first camera update, which settles layer
/// 2 by a few pixels in some levels (Super Hark Bros 2 level `135` goes
/// from `$C0` to `$BD`), so the rows may be those of a position one row
/// either side of where the layer ends up.
fn check_level(loaded: &LoadedLevel) -> (usize, Vec<String>) {
    let check = |shift| {
        let mut checked = 0;
        let mut bad = Vec::new();
        let mut candidates: Vec<_> = candidates(loaded, shift).into_iter().collect();
        candidates.sort();
        for (at, want) in candidates {
            checked += 1;
            let written = loaded.video.vram_written[at] && loaded.video.vram_written[at + 1];
            let got = &loaded.video.vram[at..at + 2];
            if !written || !want.iter().any(|w| got == w) {
                let want: Vec<String> = want
                    .iter()
                    .map(|w| format!("{:04X}", u16::from_le_bytes(*w)))
                    .collect();
                bad.push(format!(
                    "VRAM ${:04X}: {} {:04X}, expected {}",
                    at / 2,
                    if written { "holds" } else { "never written," },
                    u16::from_le_bytes([got[0], got[1]]),
                    want.join("|")
                ));
            }
        }
        (checked, bad)
    };
    [0, -1, 1]
        .into_iter()
        .map(check)
        .min_by_key(|(_, bad)| bad.len())
        .unwrap()
}

/// Checks every level whose layer 2 is a background tilemap. Vertical
/// levels (mode `$0A`) keep layer 2 horizontal and upload the same
/// two-screen-wide, 27-row tilemap, so they need no special handling.
/// Returns (levels checked, failure descriptions).
fn check_rom(rom: &Rom) -> (usize, Vec<String>) {
    let mut checked = 0;
    let mut failures = Vec::new();
    let gpw2 = rom.sha1_hex() == "390583d5faa0cc02e0c4f414f7638228661b2dc9";
    for level in 0..0x200u16 {
        if gpw2 && level == 0x09F {
            // An unused slot with no background table: its definitions
            // come from bank 0 work RAM as it stood at the upload, which
            // the loaded level's RAM no longer is.
            continue;
        }
        let loaded = match expand::expand_level(rom, level) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("level {level:03X}: {e}"));
                continue;
            }
        };
        if !loaded.tiles.shows_background() {
            continue;
        }
        checked += 1;
        let (words, bad) = check_level(&loaded);
        // At least 960 words, including when scrolling through the five
        // rows outside a 27-row buffer. Both background screens count.
        if words < 30 * 32 || !bad.is_empty() {
            failures.push(format!(
                "level {level:03X} (mode {}, BG2SC ${:02X}): {} of {words} tilemap words missing or different\n  {}",
                loaded.tiles.level_mode,
                loaded.video.bg_sc[1],
                bad.len(),
                bad.iter().take(24).cloned().collect::<Vec<_>>().join("\n  ")
            ));
        }
    }
    (checked, failures)
}

#[test]
fn vanilla_backgrounds_match_uploaded_tilemap() {
    let Some(rom) = common::vanilla() else { return };
    let (checked, failures) = check_rom(&rom);
    eprintln!("checked {checked} vanilla levels");
    assert!(checked > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn lunar_magic_backgrounds_match_uploaded_tilemap() {
    for (path, rom) in common::lunar_magic_roms() {
        let (checked, failures) = check_rom(&rom);
        eprintln!("checked {checked} levels in {}", path.display());
        assert!(checked > 0, "{}: no background levels", path.display());
        assert!(
            failures.is_empty(),
            "{}:\n{}",
            path.display(),
            failures.join("\n")
        );
    }
}

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
