//! Which object drew each tile (`expand::ObjectMap`), watched as the
//! vanilla ROM's loader runs: an object owns what it draws, and every
//! tile a level's objects drew has an owner.

mod common;

use kobo_core::expand::{self, LoadedLevel, ObjectRef};
use kobo_core::level::objects::ObjectLayer;
use kobo_core::{Rom, level};

/// The tile the loader fills the grid with before any object.
const BLANK: u16 = 0x25;

fn object(layer: ObjectLayer, index: usize) -> ObjectRef {
    ObjectRef { layer, index }
}

#[test]
fn level_105s_objects_own_what_they_draw() {
    let Some(rom) = common::vanilla() else { return };
    let loaded = expand::expand_level(&rom, 0x105).unwrap();
    let (tiles, map) = (&loaded.tiles, &loaded.objects);
    let one = |index| object(ObjectLayer::One, index);
    // Object 10, ground ledge 14 at (60, 20), 12 tiles by 5.
    assert_eq!(map.bounds(tiles, one(10)), Some([60, 20, 12, 5]));
    assert_eq!(map.owner_at(tiles, ObjectLayer::One, 65, 22), Some(one(10)));
    // Object 0, the long ground ledge under everything, shows where
    // nothing was drawn over it.
    assert_eq!(map.owner_at(tiles, ObjectLayer::One, 2, 25), Some(one(0)));
    // A diagonal ledge (3A at (13, 18)) draws down to the left of its place.
    assert_eq!(map.bounds(tiles, one(2)), Some([11, 18, 9, 6]));
    // The sky is nobody's, and the level has no layer 2 objects.
    assert_eq!(map.owner_at(tiles, ObjectLayer::One, 2, 2), None);
    assert_eq!(map.owner_at(tiles, ObjectLayer::Two, 2, 2), None);
}

#[test]
fn layer_2_objects_are_watched_too() {
    let Some(rom) = common::vanilla() else { return };
    // Level 00E's layer 2 starts with object 35 at (0, 19).
    let loaded = expand::expand_level(&rom, 0x00E).unwrap();
    let first = object(ObjectLayer::Two, 0);
    assert_eq!(
        loaded.objects.bounds(&loaded.tiles, first),
        Some([0, 19, 30, 2])
    );
    assert_eq!(
        loaded
            .objects
            .owner_at(&loaded.tiles, ObjectLayer::Two, 3, 19),
        Some(first)
    );
}

/// Every tile of layer 1 the loader changed from the blank fill has an
/// owner, and is in its owner's footprint: what objects drew is all
/// accounted for. Levels without objects are boss arenas, whose few
/// tiles their own code draws.
fn check_owned(rom: &Rom, number: u16, loaded: &LoadedLevel) -> Result<(), String> {
    let objects = level::read_objects(rom, number).map_err(|e| e.to_string())?;
    if objects.layer1.objects.is_empty() {
        return Ok(());
    }
    let (tiles, map) = (&loaded.tiles, &loaded.objects);
    let (width, height) = tiles.size();
    for y in 0..height {
        for x in 0..width {
            if tiles.tile_at(x, y) == BLANK {
                continue;
            }
            let offset = tiles.offset(x, y);
            let owner = map
                .owner(offset)
                .ok_or_else(|| format!("{number:03X}: tile ({x}, {y}) has no owner"))?;
            if owner.index >= objects.layer1.objects.len() && owner.layer == ObjectLayer::One {
                return Err(format!("{number:03X}: ({x}, {y}) owned by {owner:?}"));
            }
            if !map.footprint(owner).contains(&offset) {
                return Err(format!(
                    "{number:03X}: ({x}, {y}) not in {owner:?}'s footprint"
                ));
            }
        }
    }
    Ok(())
}

#[test]
fn every_tile_objects_drew_has_an_owner() {
    let Some(rom) = common::vanilla() else { return };
    // A level of each kind: horizontal with a background (105), with
    // layer 2 objects (00E, 009), vertical (0E7, 0C2), and the other
    // background modes (004, 10E, 018). Every level with the slow checks.
    let levels: Vec<u16> = if common::full_render() {
        (0..0x200).collect()
    } else {
        vec![0x105, 0x00E, 0x009, 0x0E7, 0x0C2, 0x004, 0x10E, 0x018]
    };
    let failures: Vec<String> = levels
        .iter()
        .filter_map(|&number| {
            let loaded = expand::expand_level(&rom, number).ok()?;
            check_owned(&rom, number, &loaded).err()
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
