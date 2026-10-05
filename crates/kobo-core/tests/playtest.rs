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
        // The title screen's code goes to Kobo's.
        assert_eq!(rom.read_u8(SnesAddr::new(0x009C64)).unwrap(), 0x5C);
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
