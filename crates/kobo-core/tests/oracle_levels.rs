//! Compares the headless loader's tile grid against dumps taken from a
//! real emulator (see `tools/oracle/`). Runs only when `KOBO_ORACLE_DIR`
//! points at a directory of `level_XXX.l1lo.bin` / `.l1hi.bin` files.
//! The dumps are of the vanilla ROM unless `KOBO_ORACLE_ROM` names the
//! ROM they were made from (the SA-1 reference ROM, say).

mod common;

use kobo_core::expand;
use std::fs;
use std::path::PathBuf;

fn oracle_dir() -> Option<PathBuf> {
    match std::env::var_os("KOBO_ORACLE_DIR") {
        Some(d) => Some(PathBuf::from(d)),
        None => {
            eprintln!("skipping: KOBO_ORACLE_DIR is not set");
            None
        }
    }
}

#[test]
fn tile_grids_match_emulator_dumps() {
    let Some(rom) = common::oracle_rom() else {
        return;
    };
    let Some(dir) = oracle_dir() else { return };
    let mut checked = 0;
    let mut failures = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.expect("oracle directory must be readable"))
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".l1lo.bin"))
        .collect();
    entries.sort();
    for lo_path in entries {
        let name = lo_path.file_name().unwrap().to_string_lossy().to_string();
        let level = u16::from_str_radix(&name[6..9], 16).unwrap();
        let hi_path = dir.join(format!("level_{level:03X}.l1hi.bin"));
        let want_lo = fs::read(&lo_path).unwrap();
        let want_hi = fs::read(&hi_path).unwrap();
        match expand::expand_level(&rom, level) {
            Ok(loaded) => {
                let bad = (0..expand::GRID_LEN)
                    .filter(|&i| {
                        loaded.tiles.low[i] != want_lo[i] || loaded.tiles.high[i] != want_hi[i]
                    })
                    .count();
                if bad != 0 {
                    failures.push(format!("level {level:03X}: {bad} bytes differ"));
                }
            }
            Err(e) => failures.push(format!("level {level:03X}: {e}")),
        }
        checked += 1;
    }
    assert!(checked > 0, "configured oracle checked no levels");
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The sprites the level load spawned around the entrance, slot by slot:
/// which slot a sprite gets decides how some of them look, and SA-1 Pack
/// hands out its 22 differently from vanilla's 12.
#[test]
fn sprite_slots_match_emulator_dumps() {
    use kobo_core::ram;
    /// Pixels a sprite may have moved by the time of the dump.
    const MOVED: i32 = 4;
    let Some(rom) = common::oracle_rom() else {
        return;
    };
    let Some(dir) = oracle_dir() else { return };
    let tables = [
        ram::SPRITE_NUMBER,
        ram::SPRITE_STATUS,
        ram::SPRITE_X_LOW,
        ram::SPRITE_X_HIGH,
        ram::SPRITE_Y_LOW,
        ram::SPRITE_Y_HIGH,
    ];
    let mut failures = Vec::new();
    let mut checked = 0;
    for level in 0..0x200u16 {
        let path = dir.join(format!("level_{level:03X}.sprites.bin"));
        let want = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        let loaded = expand::expand_level(&rom, level).unwrap();
        let slots = loaded.ram.map().sprite_slots();
        assert_eq!(want.len(), slots as usize * tables.len());
        checked += 1;
        for (slot, want) in (0..slots).zip(want.as_chunks::<6>().0) {
            let got = tables.map(|table| loaded.ram.u8_at(table, slot));
            // A free slot keeps whatever was last in it. The dump comes a
            // frame or so into the level, when a sprite may have moved,
            // and one spawned out of range may have been erased or, erased
            // during preparation, have been spawned again in its slot. So
            // it is the sprite and the place that are compared, not
            // whether it is there at this moment.
            let free = got[1] == 0 && want[1] == 0;
            let position = |s: &[u8; 6]| {
                let at = |low: usize| u16::from_le_bytes([s[low], s[low + 1]]) as i32;
                (at(2), at(4))
            };
            let (x, y) = position(&got);
            let (want_x, want_y) = position(want);
            let near = (x - want_x).abs() <= MOVED && (y - want_y).abs() <= MOVED;
            if !free && (got[0] != want[0] || !near) {
                failures.push(format!(
                    "level {level:03X} slot {slot}: {got:02X?}, emulator {want:02X?}"
                ));
            }
        }
    }
    assert!(checked > 0, "configured oracle checked no levels");
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Capture with KOBO_ORACLE_VIDEO=1 after the title-screen loader guard.
/// Compare stable arena graphics, excluding animated characters and tilemaps.
#[test]
fn boss_graphics_match_emulator_dumps() {
    let Some(rom) = common::vanilla() else { return };
    let Some(dir) = std::env::var_os("KOBO_BOSS_ORACLE_DIR").map(PathBuf::from) else {
        eprintln!("skipping: KOBO_BOSS_ORACLE_DIR is not set");
        return;
    };
    for level in [0x096, 0x0CC, 0x0D9, 0x1C7] {
        let want = fs::read(dir.join(format!("level_{level:03X}.vram.bin"))).unwrap();
        assert_eq!(want.len(), 0x10000);
        let loaded = expand::expand_level(&rom, level).unwrap();
        for (name, addresses) in [
            (
                "Mode 7 characters",
                (1..0x8000).step_by(2).collect::<Vec<_>>(),
            ),
            ("layer 3 GFX", (0x8000..0xA000).collect()),
            ("arena tilemap", (0xB000..0xC000).collect()),
            ("SP3 characters", (0xE000..0xF000).collect()),
        ] {
            for address in addresses {
                assert_eq!(
                    loaded.video.vram[address], want[address],
                    "level {level:03X} {name} at {address:04X}"
                );
            }
        }
    }
}

/// The layer 3 tilemap the game uploaded during level preparation, below
/// the status bar rows the NMI handler rewrites every frame.
#[test]
fn layer3_tilemaps_match_emulator_dumps() {
    let Some(rom) = common::oracle_rom() else {
        return;
    };
    let Some(dir) = oracle_dir() else { return };
    let mut failures = Vec::new();
    let mut checked = 0;
    for level in 0..0x200u16 {
        let path = dir.join(format!("level_{level:03X}.vram.bin"));
        let want = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("{}: {e}", path.display()),
        };
        let loaded = expand::expand_level(&rom, level).unwrap();
        if loaded.scene.boss.is_some() {
            continue;
        }
        checked += 1;
        let region = 0xA140..0xC000;
        let bad = region
            .clone()
            .filter(|&i| loaded.video.vram[i] != want[i])
            .count();
        if bad != 0 {
            let first = region
                .clone()
                .find(|&i| loaded.video.vram[i] != want[i])
                .unwrap();
            failures.push(format!(
                "level {level:03X}: {bad} bytes differ, first at {first:04X}: {:02X} vs {:02X}",
                loaded.video.vram[first], want[first]
            ));
        }
    }
    assert!(checked > 0, "configured oracle checked no levels");
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
