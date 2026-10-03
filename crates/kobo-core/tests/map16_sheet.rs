//! The foreground Map16 a loaded vanilla level resolves is the vanilla
//! table of its tileset, so the sheet a level shows and the sheet its
//! tileset shows agree.

mod common;

use kobo_core::expand;
use kobo_core::map16;

#[test]
fn loaded_foreground_map16_matches_vanilla_table() {
    let Some(rom) = common::vanilla() else {
        return;
    };
    // Tileset 7 (level 105) has the diagonal pipe override, tileset 1 (1D4) does not.
    for level in [0x105, 0x1D4, 0x0C5] {
        let loaded = expand::expand_level(&rom, level).unwrap();
        let table = map16::vanilla_map16(&rom, loaded.tiles.object_tileset, true).unwrap();
        let sheet = loaded.tiles.foreground_map16();
        assert_eq!(sheet.len(), map16::FG_TILE_COUNT, "level {level:03X}");
        for (n, (got, want)) in sheet.iter().zip(&table.tiles).enumerate() {
            if expand::PIPE_TILES.contains(&(n as u16)) {
                continue;
            }
            assert_eq!(got.as_ref(), Some(want), "level {level:03X} tile {n:03X}");
        }
        assert_eq!(loaded.tiles.bg_map16, table.tiles[map16::FG_TILE_COUNT..]);
    }
}
