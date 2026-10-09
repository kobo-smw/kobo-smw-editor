//! Compares what every level's load resolves in two ROMs, graphics and
//! palettes aside: the layer 1 grid, the background tilemap, and the
//! foreground and BG Map16 definitions the level's tiles use. For checking
//! a build of an imported hack against the hack before Kobo carries its
//! graphics.
//!
//! `cargo run --release --example tiles_diff -- a.sfc b.sfc [level...]`

#[path = "../tests/common/render_hashes.rs"]
mod render_hashes;

use kobo_core::{Rom, expand};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = Rom::load(&args[0]).unwrap();
    let b = Rom::load(&args[1]).unwrap();
    let levels: Vec<u16> = if args.len() > 2 {
        args[2..]
            .iter()
            .map(|l| u16::from_str_radix(l, 16).unwrap())
            .collect()
    } else {
        (0..0x200).collect()
    };
    // Each level on whichever core is free, the lines printed in order.
    let outcomes = render_hashes::on_every_core(&levels, |&level| compare(&a, &b, level));
    let mut same = 0;
    for outcome in outcomes {
        match outcome {
            None => same += 1,
            Some(line) => println!("{line}"),
        }
    }
    println!("{same} levels the same");
}

/// What differs in `level`'s load between the two ROMs, as a line, or
/// `None` when nothing does.
fn compare(a: &Rom, b: &Rom, level: u16) -> Option<String> {
    let (Ok(x), Ok(y)) = (
        expand::expand_level(a, level),
        expand::expand_level(b, level),
    ) else {
        return Some(format!("{level:03X}: fails to load in one"));
    };
    let (x, y) = (&x.tiles, &y.tiles);
    let mut parts = Vec::new();
    if x.low != y.low || x.high != y.high {
        parts.push("layer 1");
    }
    if x.layer2_tilemap != y.layer2_tilemap || x.layer2_screen_len != y.layer2_screen_len {
        parts.push("background tilemap");
    }
    let mut used = x
        .low
        .iter()
        .zip(&x.high)
        .map(|(&l, &h)| u16::from_le_bytes([l, h]));
    if used.any(|t| x.map16.get(&t) != y.map16.get(&t)) {
        parts.push("Map16");
    }
    if let Some((low, high)) = &x.layer2_tilemap {
        let mut bg = low
            .iter()
            .zip(high)
            .map(|(&l, &h)| u16::from_le_bytes([l, h]) as usize);
        if bg.any(|t| x.bg_map16.get(t) != y.bg_map16.get(t)) {
            parts.push("BG Map16");
        }
    }
    (!parts.is_empty()).then(|| format!("{level:03X}: {}", parts.join(", ")))
}
