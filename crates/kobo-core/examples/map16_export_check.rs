//! Where a ROM's Map16 pages past 1, as the ROM's own routine resolves them
//! (`expand::resolve_map16`), agree with Lunar Magic's `-ExportAllMap16` of
//! the same ROM: for each page, how many of its 256 tiles have the same
//! definition, and how many the export has that are not empty.
//!
//! `cargo run --release --example map16_export_check -- rom.smc all.map16 [level]`
//!
//! With `KOBO_CHECK_IMPORT=1` it compares what `import::read_map16` takes
//! from the ROM instead: per page, the tiles whose definition or acts-like
//! setting differs from the export's, Lunar Magic's empty tile (`$1004` four
//! times) counting as Kobo's (zeros).
//!
//! The export lists its sections from `$70` as offset and size pairs: `$70`
//! every tile's definition, 8 bytes each (docs/lunar-magic.md). Clean room:
//! this compares data only.

use kobo_core::{Rom, expand};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = Rom::load(&args[0]).unwrap();
    let file = std::fs::read(&args[1]).unwrap();
    let level = args
        .get(2)
        .map_or(0x105, |l| u16::from_str_radix(l, 16).unwrap());
    let word = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap()) as usize;
    let all = &file[word(0x70)..word(0x70) + word(0x74)];
    if std::env::var_os("KOBO_CHECK_IMPORT").is_some() {
        check_import(&rom, &file);
        return;
    }
    let tiles: Vec<u16> = (0x200..0x8000u16).collect();
    let resolved = expand::resolve_map16(&rom, level, &tiles).unwrap();
    // KOBO_PROBE_DUMP=file: the resolved definitions of tiles 200-7FFF.
    if let Some(path) = std::env::var_os("KOBO_PROBE_DUMP") {
        let bytes: Vec<u8> = resolved
            .iter()
            .flat_map(|t| t.map_or([0; 8], |t| t.to_bytes()))
            .collect();
        std::fs::write(path, bytes).unwrap();
    }
    let empty = [0u8; 8];
    for page in 2..0x80u16 {
        let (mut same, mut used, mut resolved_used) = (0, 0, 0);
        for tile in page * 0x100..(page + 1) * 0x100 {
            let at = tile as usize * 8;
            let Some(exported) = all.get(at..at + 8) else {
                continue;
            };
            let ours = resolved[tile as usize - 0x200].map(|t| t.to_bytes());
            if ours.as_ref().map(|b| &b[..]) == Some(exported) {
                same += 1;
            }
            if exported != empty {
                used += 1;
            }
            if ours.is_some_and(|b| b != empty) {
                resolved_used += 1;
            }
        }
        if used > 0 || resolved_used > 0 {
            println!(
                "page {page:02X}: {same:3} of 256 the same; the export has {used:3} not empty, \
                 the ROM's routine {resolved_used:3}"
            );
        }
    }
}

fn check_import(rom: &Rom, file: &[u8]) {
    use kobo_core::source::map16::DEFAULT_ACTS;
    let word = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap()) as usize;
    let all = &file[word(0x70)..word(0x70) + word(0x74)];
    let acts = &file[word(0x78)..word(0x78) + word(0x7C)];
    let (pages, notes) = kobo_core::import::read_map16(rom).unwrap();
    for note in &notes {
        println!("note: {note}");
    }
    let lm_empty = [0x04, 0x10, 0x04, 0x10, 0x04, 0x10, 0x04, 0x10];
    let mut bad = 0;
    for page in 2..0x80u16 {
        let imported = pages
            .iter()
            .find(|(p, _)| *p as u16 == page)
            .map(|(_, t)| t);
        let (mut gfx, mut act) = (0, 0);
        for tile in page * 0x100..(page + 1) * 0x100 {
            let entry = imported.and_then(|t| t.tiles.get(&tile));
            let ours = entry.map_or([0; 8], |e| e.gfx.to_bytes());
            let theirs: [u8; 8] = all[tile as usize * 8..tile as usize * 8 + 8]
                .try_into()
                .unwrap();
            let empty = |b: &[u8; 8]| *b == [0; 8] || *b == lm_empty;
            if ours != theirs && !(empty(&ours) && empty(&theirs)) {
                gfx += 1;
            }
            let ours_acts = entry.map_or(DEFAULT_ACTS, |e| e.acts);
            if let Some(b) = acts.get(tile as usize * 2..tile as usize * 2 + 2)
                && u16::from_le_bytes([b[0], b[1]]) != ours_acts
            {
                act += 1;
            }
        }
        if gfx + act > 0 {
            bad += 1;
            println!(
                "page {page:02X}: {gfx} definitions and {act} acts-like settings differ \
                 (imported: {})",
                imported.is_some()
            );
        }
    }
    println!("{bad} pages differ");
}
