//! Lunar Magic's backgrounds as the corpus has them: per level with a
//! background, its flags (`$0EF310`), the BG Map16 table the level's upload
//! read from (as the ROM's own code leaves it), the background's bytes per
//! screen, the highest tile number its tilemap uses, and the ROM's BG Map16
//! pointers at `$0EFD50`. Memory effects only (docs/lunar-magic-install.md).
//!
//! `cargo run --release --example bg_survey -- rom.smc...`

use kobo_core::level::tables;
use kobo_core::{Rom, SnesAddr, expand};

fn main() {
    for path in std::env::args().skip(1) {
        let rom = Rom::load(&path).unwrap();
        let pointers: Vec<String> = (0..16)
            .map(|i| {
                format!(
                    "{:06X}",
                    rom.read_u24(SnesAddr::new(0x0EFD50 + 3 * i)).unwrap_or(0)
                )
            })
            .collect();
        println!("{path}\n  0EFD50: {}", pointers.join(" "));
        for level in 0..0x200u16 {
            let Ok(loaded) = expand::expand_level(&rom, level) else {
                continue;
            };
            let tiles = &loaded.tiles;
            let (Some((low, high)), Some(at)) = (&tiles.layer2_tilemap, tiles.bg_map16_at) else {
                continue;
            };
            let max = low
                .iter()
                .zip(high)
                .map(|(&l, &h)| u16::from_le_bytes([l, h]))
                .max()
                .unwrap_or(0);
            let flags = rom
                .read_u8(tables::LEVEL_FLAGS.add(level as u32))
                .unwrap_or(0);
            println!(
                "  {level:03X} flags {flags:02X} table {at:06X} screen {:03X} max tile {max:04X}",
                tiles.layer2_screen_len
            );
        }
    }
}
