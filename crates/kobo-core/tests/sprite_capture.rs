//! Sprite graphics come from running the ROM's own sprite loader and
//! engine, so the checks here are about the capture behaving: every sprite
//! entry either draws something or is one of the game's invisible sprites,
//! objects land near their entries, and a spread of levels captures
//! without the CPU tripping.

mod common;

use kobo_core::video::UndrawnSprite;
use kobo_core::{expand, ram, sprites};

/// Sprites with no graphics of their own: the invisible warp hole and the
/// invisible mushroom.
const INVISIBLE: [u8; 2] = [0x8E, 0xC7];

/// Every sprite number that draws nothing anywhere in the vanilla game:
/// the message box trigger (`19`), a Magikoopa that has not appeared yet
/// (`1F`), the climbing net door, which is tiles until it turns (`54`),
/// the invisible solid block (`6D`), the bonus game (`82`), the layer 3
/// smasher (`89`), the side exit enabler (`8C`), the invisible ones above,
/// and the slotless sprites: shooters (`C9`-`CA`), generators (`CB`-`D9`),
/// and scroll commands (`E7`-`F5`).
fn draws_nothing(id: u8) -> bool {
    matches!(
        id,
        0x19 | 0x1F | 0x54 | 0x6D | 0x82 | 0x89 | 0x8C | 0x8E | 0xC7 | 0xC9..=0xD9 | 0xE7..=0xF5
    )
}

fn capture(
    rom: &kobo_core::Rom,
    level: u16,
) -> (sprites::SpriteList, kobo_core::video::SpriteScene) {
    let loaded = expand::expand_level(rom, level).unwrap();
    let list = sprites::read_sprites_at(rom, loaded.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(rom, &loaded, &list).unwrap();
    (list, scene)
}

#[test]
fn yoshis_island_1_sprites_all_draw_near_their_entries() {
    let Some(rom) = common::vanilla() else { return };
    let loaded = expand::expand_level(&rom, 0x105).unwrap();
    let list = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &loaded, &list).unwrap();
    assert!(
        scene.objects.len() >= 100,
        "{} objects",
        scene.objects.len()
    );
    for UndrawnSprite { x, y, id } in &scene.undrawn {
        assert!(
            INVISIBLE.contains(id),
            "sprite {id:02X} at ({x}, {y}) drew nothing"
        );
    }
    // Every visible entry has an object within a 48-pixel box of it.
    for e in &list.sprites {
        if INVISIBLE.contains(&e.id) {
            continue;
        }
        let (x, y) = e.tile_position(false);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        assert!(
            scene
                .objects
                .iter()
                .any(|o| (o.x - x).abs() <= 48 && (o.y - y).abs() <= 48),
            "sprite {:02X} at ({x}, {y}) has no object nearby",
            e.id
        );
    }
}

#[test]
fn vertical_level_sprites_draw_where_their_entries_are() {
    let Some(rom) = common::vanilla() else { return };
    let loaded = expand::expand_level(&rom, 0x0DB).unwrap();
    let list = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &loaded, &list).unwrap();
    assert!(scene.undrawn.is_empty(), "{:?}", scene.undrawn);
    for e in &list.sprites {
        let (x, y) = e.tile_position(true);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        assert!(
            scene
                .objects
                .iter()
                .any(|o| (o.x - x).abs() <= 48 && (o.y - y).abs() <= 48),
            "sprite {:02X} at ({x}, {y}) has no object nearby",
            e.id
        );
    }
}

/// Autoscroll commands drive the camera; the capture holds it still, so
/// the Buzzy Beetles and Swoopers of Vanilla Secret 2's layer 2 cave draw.
#[test]
fn sprites_draw_in_autoscrolling_levels() {
    let Some(rom) = common::vanilla() else { return };
    let (list, scene) = capture(&rom, 0x009);
    for e in list.sprites.iter().filter(|e| !draws_nothing(e.id)) {
        let (x, y) = e.tile_position(false);
        let (x, y) = (x as i32 * 16, y as i32 * 16);
        assert!(
            scene
                .objects
                .iter()
                .any(|o| (o.x - x).abs() <= 48 && (o.y - y).abs() <= 48),
            "sprite {:02X} at ({x}, {y}) has no object nearby",
            e.id
        );
    }
}

/// The loader is called until the column is exhausted (it stops after a
/// scroll sprite, and a stack of Boos outnumbers the free slots), sprites
/// on one spot stay together (the flying platform draws its Hammer Bro),
/// the camera follows a line-guided chainsaw to where its initialisation
/// sends it, and a Podoboo is waited for until it leaves the lava.
#[test]
fn awkward_sprites_draw() {
    let Some(rom) = common::vanilla() else { return };
    for (level, id) in [
        (0x1CF, 0x26),
        (0x11D, 0x37),
        (0x1C0, 0x9B),
        (0x1C0, 0x9C),
        (0x00F, 0x65),
        (0x00F, 0x7B),
        (0x101, 0x33),
        (0x105, 0xDB),
    ] {
        let (list, scene) = capture(&rom, level);
        assert!(list.sprites.iter().any(|e| e.id == id));
        assert!(
            !scene.undrawn.iter().any(|undrawn| undrawn.id == id),
            "level {level:03X}: sprite {id:02X} drew nothing"
        );
    }
}

/// The castle candle flames are cluster sprites the `E6` entry spawns,
/// positioned on layer 2.
#[test]
fn a_podoboo_is_drawn_with_the_tiles_its_pass_uploaded() {
    let Some(rom) = common::vanilla() else { return };
    // The Podoboo redirects its object to tile `06` and has the NMI copy
    // its frame there, so its characters are not in the level's VRAM.
    let loaded = expand::expand_level(&rom, 0x01A).unwrap();
    let list = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()).unwrap();
    let scene = expand::capture_sprites(&rom, &loaded, &list).unwrap();
    let podoboos = list.sprites.iter().filter(|e| e.id == 0x33).count();
    assert_eq!(scene.dynamic.len(), podoboos);
    for capture in &scene.dynamic {
        assert!(capture.objects.iter().all(|o| o.tile & 0xEE == 0x06));
        let tile_06 = 0xC000 + 0x06 * 32;
        assert_eq!(capture.characters[0].address, tile_06);
        let vram = &loaded.video.vram;
        for character in &capture.characters {
            let at = character.address as usize;
            assert_ne!(character.data, vram[at..at + 32]);
            assert!(character.data.iter().any(|&b| b != 0));
        }
        assert_eq!(capture.patched(vram).len(), vram.len());
    }
    // Nothing else in the level needs tiles of its own.
    assert!(scene.objects.len() > 20);
}

#[test]
fn sprites_get_the_slots_their_neighbours_leave_them() {
    let Some(rom) = common::vanilla() else { return };
    // The four birds on the roof of Yoshi's House (`8A`) take their colour
    // from their slot: captured one at a time in an empty table they
    // would all get the same slot, and all be yellow.
    let (_, scene) = capture(&rom, 0x104);
    let mut palettes: Vec<u8> = (scene.objects.iter())
        .filter(|o| (0x40..0x80).contains(&o.x) && (0xD0..0xF0).contains(&o.y))
        .map(|o| o.attr >> 1 & 7)
        .collect();
    palettes.sort_unstable();
    palettes.dedup();
    assert_eq!(palettes.len(), 4, "bird palettes {palettes:?}");
}

#[test]
fn candle_flames_ride_on_layer_2() {
    let Some(rom) = common::vanilla() else { return };
    let (_, scene) = capture(&rom, 0x101);
    assert_eq!(scene.layer2_objects.len(), 4);
    assert!(!scene.undrawn.iter().any(|undrawn| undrawn.id == 0xE6));
    let (_, scene) = capture(&rom, 0x105);
    assert!(scene.layer2_objects.is_empty());
}

/// Invictus level 136's per-level code stops every pass that runs the
/// level loop. The level still loads, without a player and with markers,
/// and says why.
#[test]
fn passes_the_cpu_gives_up_on_are_reported() {
    use expand::Pass;
    if let Some((_, rom)) = common::corpus_hack(common::hacks::INVICTUS) {
        let loaded = expand::expand_level(&rom, 0x136).unwrap();
        let (_, scene) = capture(&rom, 0x136);
        assert!(loaded.scene.player.is_empty());
        assert!(loaded.diagnostics.iter().any(|d| matches!(
            d,
            expand::Diagnostic::Cpu {
                pass: Pass::Player,
                ..
            }
        )));
        assert!(!scene.undrawn.is_empty());
        assert!(scene.diagnostics.iter().any(|d| matches!(
            d,
            expand::Diagnostic::Cpu {
                pass: Pass::Sprite { .. },
                ..
            }
        )));
    }
    let Some(rom) = common::vanilla() else { return };
    let loaded = expand::expand_level(&rom, 0x105).unwrap();
    let (_, scene) = capture(&rom, 0x105);
    assert!(loaded.diagnostics.is_empty() && scene.diagnostics.is_empty());
}

#[test]
fn a_spread_of_vanilla_levels_captures_cleanly() {
    let Some(rom) = common::vanilla() else { return };
    let mut captured = 0;
    for level in (0..0x200u16).step_by(7) {
        let loaded = expand::expand_level(&rom, level).unwrap();
        let list = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()).unwrap();
        let scene = expand::capture_sprites(&rom, &loaded, &list)
            .unwrap_or_else(|e| panic!("level {level:03X}: {e}"));
        for UndrawnSprite { x, y, id } in &scene.undrawn {
            assert!(
                draws_nothing(*id),
                "level {level:03X}: sprite {id:02X} at ({x}, {y}) drew nothing"
            );
        }
        captured += scene.objects.len();
    }
    assert!(captured > 0);
}

/// The player is captured by his own drawing pass: standing at the
/// entrance in Yoshi's Island 1, shot out of the cannon pipe that level
/// 0D0 starts with, and left to the arena's own pass in boss rooms.
#[test]
fn player_is_drawn_at_the_entrance() {
    let Some(rom) = common::vanilla() else { return };
    let loaded = expand::expand_level(&rom, 0x105).unwrap();
    let player = |loaded: &expand::LoadedLevel| {
        (
            i32::from(loaded.ram.u16(ram::PLAYER_X)),
            i32::from(loaded.ram.u16(ram::PLAYER_Y)),
        )
    };
    let (x, y) = player(&loaded);
    assert_eq!((x, y), (0x10, 0x160));
    assert!(!loaded.scene.player.is_empty());
    for o in &loaded.scene.player {
        assert!(
            (o.x - x).abs() <= 16 && (o.y - y).abs() <= 32,
            "object at ({}, {}) is not the player at ({x}, {y})",
            o.x,
            o.y
        );
    }
    // Mario's palette (row 8, colours 6-F) is uploaded with his tiles.
    assert_ne!(loaded.video.palette().get(8, 8).0, 0);

    // The cannon pipe fires him up and to the right of where he entered,
    // and the pass follows him until the launch ends.
    let loaded = expand::expand_level(&rom, 0x0D0).unwrap();
    assert_eq!(
        loaded.ram.u8(ram::PLAYER_ANIMATION),
        7,
        "cannon pipe entrance"
    );
    let (x, y) = player(&loaded);
    let (w, h) = loaded.tiles.size();
    assert!(!loaded.scene.player.is_empty());
    for o in &loaded.scene.player {
        assert!(
            o.x > x && o.y < y && o.x < w as i32 * 16 && o.y >= 0 && o.y < h as i32 * 16,
            "object at ({}, {}) after launching from ({x}, {y})",
            o.x,
            o.y
        );
    }

    let loaded = expand::expand_level(&rom, 0x1C7).unwrap();
    assert!(loaded.scene.player.is_empty());
}
