//! `playtest::build`: a build that starts in a level at a tile. Needs the
//! vanilla ROM and Asar.

mod common;

use std::fs;
use std::sync::Arc;

use common::temp::TempDir;
use kobo_core::addr::SnesAddr;
use kobo_core::edit::Workspace;
use kobo_core::playtest::{self, Start};

#[test]
fn a_play_build_enters_the_level_at_the_tile() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else {
        return;
    };
    let dir = TempDir::new("playtest");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    // A horizontal level and a vertical one.
    for (level, x, y) in [(0x105u16, 70u16, 18u16), (0x0D3, 9, 40)] {
        let original = workspace.clean_level(level).unwrap();
        workspace.add_level(level, &original).unwrap();
        let start = Start {
            level,
            x,
            y,
            powerup: 1,
        };
        let rom = playtest::build(&workspace, &start, &asar).unwrap();
        // The title screen's load goes to Kobo's code.
        assert_eq!(rom.read_u8(SnesAddr::new(0x0096BE)).unwrap(), 0x22);
        // The entrance the ROM takes puts the player on the tile, as the
        // game's own loader enters by it.
        let id = workspace.free_entrance(level).unwrap();
        let (px, py) = kobo_core::expand::secondary_entry(&rom, level, id).unwrap();
        assert_eq!(
            (px / 16, py / 16),
            (x, y),
            "level {level:03X} entered at ({px}, {py})"
        );
        // The project is as it was.
        assert_eq!(workspace.level(level), Some(&original));
    }
}

#[test]
fn a_play_entrance_into_a_water_level_is_water() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    // 00A, Donut Secret 1, is water by its entrance's action, 7.
    let (level, _) = kobo_core::import::read_level(&clean, 0x00A).unwrap();
    assert_eq!(level.entrance.entrance_action, 7);
    let start = Start {
        level: 0x00A,
        x: 20,
        y: 20,
        powerup: 0,
    };
    let entrance = playtest::entrance(&level, &start, 0x50);
    assert!(entrance.settings.water);
    assert_eq!(entrance.action, 0, "not out of a pipe");
    let (level, _) = kobo_core::import::read_level(&clean, 0x105).unwrap();
    assert!(!playtest::entrance(&level, &start, 0x50).settings.water);
}

/// With UberASM Tool: a play build sets the translevel the overworld would
/// have entered by, a sublevel's its overworld level's, and with the Retry
/// System in the project's UberASM Tool folder, its respawn point to the
/// entrance, by the addresses its own `retry_config/ram.asm` gives.
#[test]
fn a_play_build_sets_the_translevel_and_the_retry_systems_respawn() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else {
        return;
    };
    if common::tool(kobo_core::tools::Tool::UberAsm).is_none() {
        return;
    }
    let dir = TempDir::new("playtest-retry");
    fs::write(
        dir.join("kobo.toml"),
        "format = 1\n\n[uberasm]\ndir = \"uberasm\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("uberasm/retry_config")).unwrap();
    fs::create_dir_all(dir.join("uberasm/library")).unwrap();
    fs::write(
        dir.join("uberasm/list.txt"),
        "verbose: off\nlevel:\noverworld:\ngamemode:\n\
         global: other/global_code.asm\nstatusbar: other/status_code.asm\n\
         macrolib: other/macro_library.asm\nfreeram: $7FAC80\n",
    )
    .unwrap();
    // As the Retry System's own file has them.
    fs::write(
        dir.join("uberasm/retry_config/ram.asm"),
        "includeonce\n!retry_freeram = $7FB400\nif read1($00FFD5) == $23\n    \
         !retry_freeram = $40A400\nendif\nmacro retry_ram(name,offset)\n    \
         !ram_<name> #= !retry_freeram+<offset>\n    \
         !retry_ram_<name> #= !ram_<name>\nendmacro\n\
         %retry_ram(timer,$00)\n%retry_ram(respawn,$03)\n",
    )
    .unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    // 105, translevel $29, and 1CB, the room its pipe leads to.
    for level in [0x105u16, 0x1CB] {
        let original = workspace.clean_level(level).unwrap();
        workspace.add_level(level, &original).unwrap();
    }
    let has =
        |rom: &kobo_core::rom::Rom, code: &[u8]| rom.data().windows(code.len()).any(|w| w == code);
    for level in [0x105u16, 0x1CB] {
        let start = Start {
            level,
            x: 5,
            y: 20,
            powerup: 0,
        };
        let rom = playtest::build(&workspace, &start, &asar).unwrap();
        // LDA #$29 : STA $13BF
        assert!(has(&rom, &[0xA9, 0x29, 0x8D, 0xBF, 0x13]), "{level:03X}");
        // LDA #id : STA.l $7FB403 : LDA #$02|bit 8 : STA.l $7FB404
        let id = workspace.free_entrance(level).unwrap();
        let respawn = [
            0xA9,
            id as u8,
            0x8F,
            0x03,
            0xB4,
            0x7F,
            0xA9,
            0x02 | (id >> 8) as u8,
            0x8F,
            0x04,
            0xB4,
            0x7F,
        ];
        assert!(has(&rom, &respawn), "{level:03X}");
    }
}
