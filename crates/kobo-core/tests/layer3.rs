//! Layer 3 capture: the scroll behaviour `expand` measures by running the
//! ROM's layer scroll routine must agree with what the vanilla game does
//! for each layer 3 setting and tileset (`Layer3TilemapSettings` at
//! `$009F88`, applied by `CODE_009FB8`), and the position must be where
//! that routine put it.

mod common;

use kobo_core::addr::SnesAddr;
use kobo_core::{expand, level, ram};

/// Per-level layer 3 setting: bits 7-6 of the secondary header byte at
/// `$05F200`.
const LAYER3_SETTINGS: u32 = 0x05_F200;
/// Three bytes per object tileset, indexed by the setting minus one:
/// 1 and 2 are tides, `$80`/`$C0` fixed backgrounds, `$81` scrolling ones.
const TILESET_TABLE: u32 = 0x00_9F88;

#[test]
fn vanilla_scroll_rates_and_positions_follow_the_tileset_table() {
    let Some(rom) = common::vanilla() else { return };
    let mut failures = Vec::new();
    let mut checked = 0;
    for lv in 0..level::LEVEL_COUNT {
        let header = level::read_primary_header(&rom, lv).unwrap();
        let loaded = match expand::expand_level(&rom, lv) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("level {lv:03X}: {e}"));
                continue;
            }
        };
        if loaded.scene.boss.is_some() {
            assert!(
                loaded.scene.layer3.is_none(),
                "level {lv:03X}: boss arena with layer 3"
            );
            continue;
        }
        let setting = rom
            .read_u8(SnesAddr::new(LAYER3_SETTINGS + lv as u32))
            .unwrap()
            >> 6;
        let tileset = header.object_tileset as u32;
        let kind = match setting {
            0 => 0,
            s => rom
                .read_u8(SnesAddr::new(TILESET_TABLE + tileset * 3 + s as u32 - 1))
                .unwrap(),
        };
        // (rate, y position) per kind; the crusher ($80) is moved by its
        // sprite during the entrance, so only its rate is fixed.
        let (rate, y) = match kind {
            0 | 0xC0 => ([0, 0], Some(0xD0)),
            0x80 => ([0, 0], None),
            1 => ([16, 0], Some(0x70)),
            2 => ([16, 0], Some(0x40)),
            0x81 if tileset == 1 || tileset == 3 => ([8, 0], Some(0xC0)),
            0x81 => ([16, 16], None),
            other => panic!("level {lv:03X}: unknown layer 3 kind {other:02X}"),
        };
        let Some(layer3) = loaded.scene.layer3 else {
            // Level modes without layer 3 on the main screen.
            if layer3_on_main(&loaded) {
                failures.push(format!("level {lv:03X}: layer 3 missing"));
            }
            continue;
        };
        checked += 1;
        if layer3.scroll_per_16 != rate {
            failures.push(format!(
                "level {lv:03X}: kind {kind:02X} scrolls {:?}, want {rate:?}",
                layer3.scroll_per_16
            ));
        }
        let want_y = match (kind, y) {
            (0x81, None) => layer3.camera[1],
            (_, Some(y)) => y,
            _ => continue,
        };
        if layer3.position[1] != want_y {
            failures.push(format!(
                "level {lv:03X}: kind {kind:02X} at y {}, want {want_y}",
                layer3.position[1]
            ));
        }
        assert_eq!(layer3.tilemap, 0x53, "level {lv:03X}: BG3SC");
        assert_eq!(layer3.character_base, 0x8000, "level {lv:03X}: BG34NBA");
        assert_eq!(
            layer3.high_priority_in_front(),
            header.layer3_priority,
            "level {lv:03X}: priority bit"
        );
    }
    eprintln!("checked {checked} levels");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `$0D9D` (main screen designation) bit 2 after loading.
fn layer3_on_main(loaded: &expand::LoadedLevel) -> bool {
    loaded.ram.u8(ram::MAIN_SCREEN) & 0x04 != 0
}
