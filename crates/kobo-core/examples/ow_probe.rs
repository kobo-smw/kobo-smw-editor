//! What Lunar Magic's overworld layout holds, found from data only
//! (docs/lunar-magic-install.md, "The overworld"): never its code.
//!
//! `blocks a.smc b.smc [wram.bin]`: the RATS blocks `b` has that `a` has
//! not, each with what it decompresses to (LC_LZ2), and where its bytes,
//! raw or decompressed, appear in an overworld RAM dump of `b`
//! (`tools/oracle/dump_overworld.sh`): a block that is the layer 2
//! tilemap's tile numbers is matched against `$7F4000`'s even bytes, and
//! so on.
//!
//! `cargo run --release --example ow_probe -- blocks base.smc self.smc d-self/overworld.wram.bin`

use kobo_core::compress::lz2;
use kobo_core::{Rom, rats};

/// Where `needle` (its first 32 bytes, or all if shorter) first appears in
/// `hay`, as an offset.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let n = &needle[..needle.len().min(32)];
    if n.len() < 8 || n.iter().all(|&b| b == n[0]) {
        return None;
    }
    hay.windows(n.len()).position(|w| w == n)
}

/// The layer 2 tilemap's tile numbers and properties: every other byte of
/// `$7F4000`-`$7F7FFF`.
fn layer2(wram: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let map = &wram[0x1_4000..0x1_8000];
    (
        map.iter().step_by(2).copied().collect(),
        map.iter().skip(1).step_by(2).copied().collect(),
    )
}

fn blocks(a: &Rom, b: &Rom, wram: Option<&[u8]>) {
    let old: Vec<(u32, Vec<u8>)> = rats::blocks(a)
        .iter()
        .map(|bl| (bl.start.raw(), a.read(bl.start, bl.len).unwrap().to_vec()))
        .collect();
    for block in rats::blocks(b) {
        let bytes = b.read(block.start, block.len).unwrap();
        if old
            .iter()
            .any(|(at, data)| *at == block.start.raw() && data == bytes)
        {
            continue;
        }
        let mut line = format!("${:06X} {:#06x} bytes", block.start.raw(), block.len);
        let unpacked = lz2::decompress(bytes).ok();
        if let Some(u) = &unpacked {
            line += &format!(", LZ2 {:#x} -> {:#x}", u.consumed, u.data.len());
        }
        if let Some(wram) = wram {
            let (numbers, props) = layer2(wram);
            let mut seen = Vec::new();
            for (what, data) in [
                ("raw", bytes.to_vec()),
                ("lz2", unpacked.map(|u| u.data).unwrap_or_default()),
            ] {
                if data.is_empty() {
                    continue;
                }
                if let Some(at) = find(wram, &data) {
                    seen.push(format!("{what} at ${:06X}", 0x7E_0000 + at));
                }
                if let Some(at) = find(&numbers, &data) {
                    seen.push(format!("{what} = layer 2 tile numbers from {at:#x}"));
                }
                if let Some(at) = find(&props, &data) {
                    seen.push(format!("{what} = layer 2 properties from {at:#x}"));
                }
            }
            if !seen.is_empty() {
                line += &format!(": {}", seen.join("; "));
            }
        }
        println!("{line}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("blocks") if args.len() >= 3 => {
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            let wram = args.get(3).map(|p| std::fs::read(p).unwrap());
            blocks(&a, &b, wram.as_deref());
        }
        _ => eprintln!("usage: ow_probe blocks a.smc b.smc [wram.bin]"),
    }
}
