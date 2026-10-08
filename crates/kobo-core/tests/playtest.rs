//! `playtest::build`: a build that starts in a level at a tile. Needs the
//! vanilla ROM and Asar.

mod common;

use std::fs;
use std::sync::Arc;

use common::temp::TempDir;
use kobo_core::addr::SnesAddr;
use kobo_core::edit::Workspace;
use kobo_core::playtest::{self, Settings, Start};
use kobo_core::rom::Rom;

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
            at: Some((x, y)),
            settings: Settings {
                powerup: 1,
                ..Settings::default()
            },
        };
        let rom = playtest::build(&workspace, &start, &asar).unwrap();
        // The title screen's load goes to Kobo's code, and the overworld's
        // once it has cleared the level's RAM (Clear_1A_13D3, at $00A093),
        // item memory among it, so that collected coins are back.
        assert_eq!(rom.read_u8(SnesAddr::new(0x0096BE)).unwrap(), 0x22);
        assert_eq!(rom.read_u8(SnesAddr::new(0x00A093)).unwrap(), 0x20);
        assert_eq!(rom.read_u8(SnesAddr::new(0x00A096)).unwrap(), 0x5C);
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
    let entrance = playtest::entrance(&level, (20, 20), 0x50);
    assert!(entrance.settings.water);
    assert_eq!(entrance.action, 0, "not out of a pipe");
    let (level, _) = kobo_core::import::read_level(&clean, 0x105).unwrap();
    assert!(!playtest::entrance(&level, (20, 20), 0x50).settings.water);
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
            at: Some((5, 20)),
            settings: Settings::default(),
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

/// The game a play build starts, as its title screen hook leaves RAM: the
/// settings' power-up, switch palaces, and ON/OFF switch, the translevel,
/// and a screen exit from every screen to the play's entrance, or, from
/// the level's start, to the level itself (its main entrance, as the level
/// has it: 0E9's brings the player out of a pipe, which an entrance at the
/// tile its load leaves him on would put him inside).
#[test]
fn a_play_build_starts_the_game_as_its_settings_say() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else {
        return;
    };
    use kobo_core::expand::{self, Registers};
    use kobo_core::ram::RamAddr;
    let dir = TempDir::new("playtest-settings");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(
        &dir,
        Arc::new(Rom::from_headerless(clean.data().to_vec()).unwrap()),
    )
    .unwrap()
    .without_cache();
    let original = workspace.clean_level(0x0E9).unwrap();
    workspace.add_level(0x0E9, &original).unwrap();
    let settings = Settings {
        powerup: 2,
        switches: 0b1010,
        off: true,
    };
    let id = workspace.free_entrance(0x0E9).unwrap();
    for (at, low, high, secondary) in [
        (Some((8u16, 20u16)), id as u8, 0x06 | (id >> 8) as u8, 1u8),
        (None, 0xE9, 0x04, 0),
    ] {
        let start = Start {
            level: 0x0E9,
            at,
            settings,
        };
        let rom = playtest::build(&workspace, &start, &asar).unwrap();
        let hook = rom.read(SnesAddr::new(0x0096BF), 3).unwrap();
        let routine = u32::from(hook[0]) | u32::from(hook[1]) << 8 | u32::from(hook[2]) << 16;
        let (ram, _) = expand::call_in_level(
            &rom,
            &clean,
            0x0E9,
            |_| {},
            routine,
            true,
            Registers {
                p: 0x30,
                ..Default::default()
            },
        )
        .unwrap();
        let what = format!("from {at:?}");
        let at = |a: u32| ram.u8(RamAddr::new(0x7E_0000 | a));
        assert_eq!(at(0x19), 2, "{what}");
        assert_eq!(
            [at(0x1F27), at(0x1F28), at(0x1F29), at(0x1F2A)],
            [0, 1, 0, 1],
            "{what}"
        );
        assert_eq!(at(0x14AF), 1, "{what}");
        // Into a sublevel, as a screen exit goes.
        assert_eq!(at(0x141A), 1, "{what}");
        assert_eq!((at(0x19B8), at(0x19D8)), (low, high), "{what}");
        assert_eq!((at(0x19B8 + 31), at(0x19D8 + 31)), (low, high), "{what}");
        assert_eq!(at(0x1B93), secondary, "{what}");
    }
}
