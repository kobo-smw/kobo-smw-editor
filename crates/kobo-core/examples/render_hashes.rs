//! Prints a SHA-1 of every level's rendered picture, with sprites drawn
//! and again as markers without the player, and a hash of what the passes
//! reported (`tests/common/render_hashes.rs` has the format): diff the
//! output of two builds to check that a change to `expand` or `render`
//! left every picture byte-identical. Levels that fail to render print
//! their error instead.
//!
//! `cargo run --release --example render_hashes -- rom.smc [level...] > before.txt`
//!
//! Levels (hex) after the ROM limit it to those; the default is all 512.
//! `--tiles` adds a hash of what each level's load resolves, graphics aside
//! (what `tiles_diff` compares), from the same load as the pictures.
//! `tests/fixtures/render_hashes/` holds the vanilla ROM's and the SA-1
//! reference ROM's (`cargo xtask baseline`).

#[path = "../tests/common/render_hashes.rs"]
mod render_hashes;

use kobo_core::Rom;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let tiles = args.iter().any(|a| a == "--tiles");
    args.retain(|a| a != "--tiles");
    let mut args = args.into_iter();
    let path = args
        .next()
        .expect("usage: render_hashes <rom> [--tiles] [level...]");
    let rom = Rom::load(&path).expect("ROM must load");
    let mut levels: Vec<u16> = args
        .map(|level| u16::from_str_radix(&level, 16).expect("levels must be hex"))
        .collect();
    if levels.is_empty() {
        levels = (0..0x200).collect();
    }
    for line in render_hashes::lines_with(&rom, &levels, tiles) {
        println!("{line}");
    }
}
