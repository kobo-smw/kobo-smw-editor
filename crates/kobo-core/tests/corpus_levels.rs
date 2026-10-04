//! Every level of the vanilla ROM and of each hack in `KOBO_LM_ROMS`,
//! loaded once and checked two ways, on every core:
//!
//! - Layer 2 background tilemaps: the buffer the loader captures and the
//!   BG Map16 table it resolves through must reproduce the tilemap the game
//!   itself uploaded to VRAM during level preparation. This catches both a
//!   stale or clobbered `$7EB900` capture and a BG table read from the
//!   wrong place.
//! - Sprite lists must parse to exactly the bytes Lunar Magic stored. Lunar
//!   Magic wraps relocated sprite data in a RATS block whose tag precedes
//!   the header byte, so the block size is an independent statement of
//!   where the list ends: a parser that stops early or overruns disagrees
//!   with it. Levels with Lunar Magic 3's expanded heights must also keep
//!   every sprite inside the level, which ties the Y position jumps to the
//!   height the loader reported.
//!
//! The two were separate tests, each loading every level of the corpus on
//! one core; together they took most of a full run.

mod common;

use kobo_core::expand::{self, LoadedLevel, SCREEN_COLS};
use kobo_core::{Rom, ram, sprites};
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
fn check_background(loaded: &LoadedLevel) -> (usize, Vec<String>) {
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

/// The size of the RATS block whose data starts at `data`, if a valid tag
/// immediately precedes it.
fn rats_size(rom: &Rom, data: kobo_core::addr::SnesAddr) -> Option<usize> {
    let off = rom
        .mapping()
        .snes_to_pc(data)
        .ok()?
        .as_usize()
        .checked_sub(8)?;
    let tag = rom.data().get(off..off + 8)?;
    if &tag[..4] != b"STAR" {
        return None;
    }
    let size = u16::from_le_bytes([tag[4], tag[5]]);
    let complement = u16::from_le_bytes([tag[6], tag[7]]);
    (size == !complement).then_some(size as usize + 1)
}

/// What one level gave: whether its background was checked, whether its
/// sprite list parsed and whether a RATS tag confirmed its length, and the
/// failures.
#[derive(Default)]
struct LevelResult {
    background: bool,
    parsed: bool,
    confirmed: bool,
    failures: Vec<String>,
}

fn check_level(rom: &Rom, gpw2: bool, level: u16) -> LevelResult {
    let mut out = LevelResult::default();
    let loaded = match expand::expand_level(rom, level) {
        Ok(t) => t,
        Err(e) => {
            // A level the hack itself cannot load (34_idol's object
            // pre-scan runs over bank 0 in nine of its slots, a known
            // failure; docs/testing.md).
            out.failures.push(format!("level {level:03X}: {e}"));
            return out;
        }
    };
    // An unused slot of Grand Poo World 2 with no background table: its
    // definitions come from bank 0 work RAM as it stood at the upload,
    // which the loaded level's RAM no longer is.
    if loaded.tiles.shows_background() && !(gpw2 && level == 0x09F) {
        out.background = true;
        let (words, bad) = check_background(&loaded);
        // At least 960 words, including when scrolling through the five
        // rows outside a 27-row buffer. Both background screens count.
        if words < 30 * 32 || !bad.is_empty() {
            out.failures.push(format!(
                "level {level:03X} (mode {}, BG2SC ${:02X}): {} of {words} tilemap words missing or different\n  {}",
                loaded.tiles.level_mode,
                loaded.video.bg_sc[1],
                bad.len(),
                bad.iter().take(24).cloned().collect::<Vec<_>>().join("\n  ")
            ));
        }
    }
    let start = loaded.sprite_data_ptr();
    let list = match sprites::read_sprites_at(rom, start) {
        Ok(list) => list,
        Err(e) => {
            out.failures
                .push(format!("level {level:03X}: sprites: {e}"));
            return out;
        }
    };
    out.parsed = true;
    let (w, h) = loaded.tiles.size();
    // Lunar Magic 3 sets the screen count from its own per-level table
    // rather than the header; a level it never saved can come out as
    // `$FF` (Grand Poo World 2's 109), which `size()` bounds.
    if !(loaded.tiles.rows * loaded.tiles.screens * 16 <= expand::GRID_LEN
        || loaded.tiles.vertical
        || loaded.tiles.screens == 0xFF)
    {
        out.failures.push(format!(
            "level {level:03X}: {} screens of {} rows overflow the planes",
            loaded.tiles.screens, loaded.tiles.rows
        ));
    }
    // Lunar Magic lets sprites sit beyond the last screen, so only Y is
    // checked: it is the coordinate the Y position jumps extend.
    if !loaded.tiles.vertical && loaded.tiles.rows != expand::SCREEN_ROWS {
        for s in &list.sprites {
            let (x, y) = s.tile_position(false);
            if y >= h {
                out.failures.push(format!(
                    "level {level:03X} ({} rows): sprite {:02X} at ({x}, {y}) lies below {w}x{h}",
                    loaded.tiles.rows, s.id
                ));
            }
        }
    }
    if let Some(size) = rats_size(rom, start) {
        if list.len != size {
            out.failures.push(format!(
                "level {level:03X}: parsed {} bytes of sprites at {start} but the RATS block holds {size}",
                list.len
            ));
        }
        out.confirmed = true;
    }
    out
}

/// Every level of `rom`: (backgrounds checked, sprite lists parsed, lists
/// a RATS tag confirmed, failures).
fn check_rom(rom: &Rom) -> (usize, usize, usize, Vec<String>) {
    let gpw2 = rom.sha1_hex() == "390583d5faa0cc02e0c4f414f7638228661b2dc9";
    let levels: Vec<u16> = (0..0x200).collect();
    let results = common::par_map(&levels, |&level| check_level(rom, gpw2, level));
    let count = |f: fn(&LevelResult) -> bool| results.iter().filter(|r| f(r)).count();
    let (background, parsed, confirmed) = (
        count(|r| r.background),
        count(|r| r.parsed),
        count(|r| r.confirmed),
    );
    let failures = results.into_iter().flat_map(|r| r.failures).collect();
    (background, parsed, confirmed, failures)
}

#[test]
fn vanilla_levels_load_as_the_game_has_them() {
    let Some(rom) = common::vanilla() else { return };
    let (background, parsed, _, failures) = check_rom(&rom);
    eprintln!("vanilla: {background} backgrounds checked, {parsed} sprite lists parsed");
    assert!(background > 0);
    assert_eq!(parsed, 0x200);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every hack is checked whatever an earlier one gave, and the failures
/// are reported together.
#[test]
fn lunar_magic_levels_load_as_the_hacks_have_them() {
    let mut failures = Vec::new();
    for (path, rom) in common::lunar_magic_roms() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (background, parsed, confirmed, mut found) = check_rom(&rom);
        eprintln!(
            "{name}: {background} backgrounds checked, {parsed} sprite lists parsed, \
             {confirmed} confirmed by RATS tags"
        );
        if background == 0 {
            found.push("no background levels".into());
        }
        if confirmed == 0 {
            found.push("no RATS-wrapped sprite lists found".into());
        }
        failures.extend(found.into_iter().map(|f| format!("{name}: {f}")));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
