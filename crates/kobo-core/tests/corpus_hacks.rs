//! Tests written around one hack of the corpus (`common::hacks`), which
//! skip without it. They are a binary of their own: loading a ROM Lunar
//! Magic saved turns instruction-level output off for the rest of the
//! process (`kobo_core::clean_room`), which would otherwise depend on
//! which test ran first in a binary of vanilla tests.

mod common;

use kobo_core::expand::{self, Diagnostic, Pass};
use kobo_core::sprites;

/// Grand Poo World 2: an unused slot with no background table of its own
/// reads its definitions from bank 0 at the upload, as the game does; a
/// layer 2 objects level has no tilemap; a background level has its 32
/// rows and table.
#[test]
fn grand_poo_world_background_validation() {
    let Some((_, rom)) = common::corpus_hack(common::hacks::GRAND_POO_WORLD_2) else {
        return;
    };
    let unused = expand::expand_level(&rom, 0x09F).unwrap();
    assert!(unused.tiles.layer2_tilemap.is_some());
    let objects = expand::expand_level(&rom, 0x00E).unwrap();
    assert!(objects.tiles.layer2_tilemap.is_none());
    let background = expand::expand_level(&rom, 0x046).unwrap();
    assert_eq!(background.tiles.layer2_bg_rows(), 32);
    assert!(background.tiles.bg_map16.len() > 0x350);
}

/// Invictus level 136's per-level code stops every pass that runs the
/// level loop. The level still loads, without a player and with markers,
/// and says why.
#[test]
fn passes_the_cpu_gives_up_on_are_reported() {
    let Some((_, rom)) = common::corpus_hack(common::hacks::INVICTUS) else {
        return;
    };
    let loaded = expand::expand_level(&rom, 0x136).unwrap();
    let list = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &loaded, &list).unwrap();
    assert!(loaded.scene.player.is_empty());
    assert!(loaded.diagnostics.iter().any(|d| matches!(
        d,
        Diagnostic::Cpu {
            pass: Pass::Player,
            ..
        }
    )));
    assert!(!scene.undrawn.is_empty());
    assert!(scene.diagnostics.iter().any(|d| matches!(
        d,
        Diagnostic::Cpu {
            pass: Pass::Sprite { .. },
            ..
        }
    )));
}
