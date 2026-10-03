//! Lists the sprite numbers that draw nothing when a ROM's levels are
//! captured, with how many entries and which levels: a quick way to see
//! whether a change to `expand::capture_sprites` lost (or gained) sprites
//! in a hack. Levels that fail to load are skipped; capture errors go to
//! standard error.
//!
//! `cargo run --release --example sprite_census -- rom.smc`

use std::collections::BTreeMap;

use kobo_core::{Rom, expand, sprites};

fn main() {
    let path = std::env::args().nth(1).expect("usage: sprite_census <rom>");
    let rom = Rom::load(&path).expect("ROM must load");
    let mut undrawn: BTreeMap<u8, Vec<u16>> = BTreeMap::new();
    for level in 0..0x200u16 {
        let Ok(loaded) = expand::expand_level(&rom, level) else {
            continue;
        };
        let Ok(list) = sprites::read_sprites_at(&rom, loaded.sprite_data_ptr()) else {
            continue;
        };
        match expand::capture_sprites(&rom, &loaded, &list) {
            Ok(scene) => {
                for sprite in scene.undrawn {
                    undrawn.entry(sprite.id).or_default().push(level);
                }
            }
            Err(e) => eprintln!("{level:03X}: {e}"),
        }
    }
    for (id, levels) in undrawn {
        let entries = levels.len();
        let mut levels = levels;
        levels.dedup();
        println!("{id:02X}: {entries} entries in levels {levels:03X?}");
    }
}
