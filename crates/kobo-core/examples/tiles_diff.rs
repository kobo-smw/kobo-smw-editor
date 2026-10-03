//! Compares what every level's load resolves in two ROMs, graphics and
//! palettes aside: the layer 1 grid, the background tilemap, and the
//! foreground and BG Map16 definitions the level's tiles use. For checking
//! a build of an imported hack against the hack before Kobo carries its
//! graphics.
//!
//! `cargo run --release --example tiles_diff -- a.sfc b.sfc [level...]`

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
    let mut same = 0;
    for level in levels {
        let (Ok(x), Ok(y)) = (
            expand::expand_level(&a, level),
            expand::expand_level(&b, level),
        ) else {
            println!("{level:03X}: fails to load in one");
            continue;
        };
        let (x, y) = (&x.tiles, &y.tiles);
        let mut parts = Vec::new();
        if x.low != y.low || x.high != y.high {
            parts.push("layer 1");
        }
        if x.layer2_tilemap != y.layer2_tilemap || x.layer2_screen_len != y.layer2_screen_len {
            parts.push("background tilemap");
        }
        let used = x
            .low
            .iter()
            .zip(&x.high)
            .map(|(&l, &h)| u16::from_le_bytes([l, h]));
        if used.clone().any(|t| x.map16.get(&t) != y.map16.get(&t)) {
            parts.push("Map16");
        }
        if let Some((low, high)) = &x.layer2_tilemap {
            let bg = low
                .iter()
                .zip(high)
                .map(|(&l, &h)| u16::from_le_bytes([l, h]) as usize);
            if bg.clone().any(|t| x.bg_map16.get(t) != y.bg_map16.get(t)) {
                parts.push("BG Map16");
            }
        }
        if parts.is_empty() {
            same += 1;
        } else {
            println!("{level:03X}: {}", parts.join(", "));
        }
    }
    println!("{same} levels the same");
}
