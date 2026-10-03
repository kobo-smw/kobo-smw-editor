//! Sprite lists must parse to exactly the bytes Lunar Magic stored. Lunar
//! Magic wraps relocated sprite data in a RATS block whose tag precedes the
//! header byte, so the block size is an independent statement of where the
//! list ends: a parser that stops early or overruns disagrees with it.
//! Runs on the vanilla ROM (no RATS tags, so only requiring clean parses)
//! and on every hack in `KOBO_LM_ROMS`. Levels with Lunar Magic 3's
//! expanded heights must also keep every sprite inside the level, which
//! ties the Y position jumps to the height the loader reported.

mod common;

use kobo_core::expand::{self, ExpandError};
use kobo_core::{Rom, sprites};

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

/// (levels parsed, levels whose length a RATS tag confirmed).
fn check_rom(name: &str, rom: &Rom) -> (usize, usize) {
    let mut parsed = 0;
    let mut confirmed = 0;
    for level in 0..0x200u16 {
        let loaded = match expand::expand_level(rom, level) {
            Ok(t) => t,
            // A level the hack itself cannot load (34_idol's object
            // pre-scan runs over bank 0 in nine of its slots) has no
            // pointer to check.
            Err(ExpandError::Cpu { .. }) => {
                eprintln!("{name} level {level:03X}: the ROM cannot load it, skipped");
                continue;
            }
            Err(e) => panic!("{name} level {level:03X}: {e}"),
        };
        let start = loaded.sprite_data_ptr();
        let list = sprites::read_sprites_at(rom, start)
            .unwrap_or_else(|e| panic!("{name} level {level:03X}: {e}"));
        parsed += 1;
        let (w, h) = loaded.tiles.size();
        // Lunar Magic 3 sets the screen count from its own per-level table
        // rather than the header; a level it never saved can come out as
        // `$FF` (Grand Poo World 2's 109), which `size()` bounds.
        assert!(
            loaded.tiles.rows * loaded.tiles.screens * 16 <= expand::GRID_LEN
                || loaded.tiles.vertical
                || loaded.tiles.screens == 0xFF,
            "{name} level {level:03X}: {} screens of {} rows overflow the planes",
            loaded.tiles.screens,
            loaded.tiles.rows
        );
        // Lunar Magic lets sprites sit beyond the last screen, so only Y
        // is checked: it is the coordinate the Y position jumps extend.
        if !loaded.tiles.vertical && loaded.tiles.rows != expand::SCREEN_ROWS {
            for s in &list.sprites {
                let (x, y) = s.tile_position(false);
                assert!(
                    y < h,
                    "{name} level {level:03X} ({} rows): sprite {:02X} at ({x}, {y}) lies below {w}x{h}",
                    loaded.tiles.rows,
                    s.id
                );
            }
        }
        if let Some(size) = rats_size(rom, start) {
            assert_eq!(
                list.len, size,
                "{name} level {level:03X}: parsed {} bytes at {start} but the RATS block holds {size}",
                list.len
            );
            confirmed += 1;
        }
    }
    (parsed, confirmed)
}

#[test]
fn sprite_lists_match_their_rats_blocks() {
    let Some(vanilla) = common::vanilla() else {
        return;
    };
    let (parsed, _) = check_rom("vanilla", &vanilla);
    assert_eq!(parsed, 0x200);
    for (path, rom) in common::lunar_magic_roms() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let (parsed, confirmed) = check_rom(&name, &rom);
        assert!(confirmed > 0, "{name}: no RATS-wrapped sprite lists found");
        eprintln!("{name}: {parsed} lists parsed, {confirmed} confirmed by RATS tags");
    }
}
