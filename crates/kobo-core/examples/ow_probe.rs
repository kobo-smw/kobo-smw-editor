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
//!
//! The rest compare loads (`expand::load_overworld`): `check` the reader
//! against a ROM's own load, `build` and `passed` Kobo's build of a hack's
//! overworld against the hack (`passed` with every event passed), `write`
//! that build to a file, `pair` and `each` two ROMs with every event
//! passed or each alone (`SHOW=1` lists the bytes, `EVENT=n` takes one),
//! `end` and `ends` each event's end in play (`expand::end_event`):
//! two ROMs', or what it changes in one, and `beat` and `beats` levels
//! beaten (`expand::beat_level`): one's steps, or every level of two ROMs.
//! `enter` and `enter2` give the level each translevel enters, `name` the
//! level name's stripe image.

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

/// Each table the reader gives, against the load's RAM.
fn check(rom: &Rom) -> String {
    use kobo_core::overworld::Overworld;
    use kobo_core::ram::RamAddr;
    let read = match Overworld::read(rom) {
        Ok(read) => read,
        Err(e) => return format!("read: {e}"),
    };
    let loaded = match kobo_core::expand::load_overworld(rom) {
        Ok(loaded) => loaded,
        Err(e) => return format!("{:?} layout, load: {e}", read.layout),
    };
    let ram = |at: u32, len: usize| loaded.ram.bytes(RamAddr::new(at), len);
    let low: Vec<u8> = read.layer1.iter().map(|&t| t as u8).collect();
    let high: Vec<u8> = read.layer1.iter().map(|&t| (t >> 8) as u8).collect();
    let layer2: Vec<u8> = read.layer2.iter().flat_map(|w| w.to_le_bytes()).collect();
    let props = &read.events.properties;
    let mut out = format!("{:?} layout, {} events:", read.layout, read.events.count());
    for (what, ours, theirs) in [
        ("layer 1", low, ram(0x7E_C800, 0x800)),
        ("pages", high, ram(0x7F_C800, 0x800)),
        (
            "translevels",
            read.translevels.clone(),
            ram(0x7E_D000, 0x800),
        ),
        ("directions", read.directions.clone(), ram(0x7E_D800, 0x800)),
        ("layer 2", layer2, ram(0x7F_4000, 0x4000)),
        (
            "event properties",
            props.clone(),
            ram(0x7F_0000, props.len()),
        ),
    ] {
        let differ = ours.iter().zip(&theirs).filter(|(a, b)| a != b).count();
        out += &if differ == 0 {
            format!(" {what} ok;")
        } else {
            format!(" {what} {differ} differ;")
        };
    }
    out
}

/// A hack's overworld built by Kobo onto the clean ROM: read back against
/// the hack's, and the build's own load against the hack's tables.
fn build_check(clean: &Rom, hack: &Rom) -> String {
    use kobo_core::build::{self, Project};
    use kobo_core::overworld::Overworld;
    let theirs = match Overworld::read(hack) {
        Ok(o) => o.in_lunar_magic_shape(),
        Err(e) => return format!("read: {e}"),
    };
    let base = Overworld::read(clean).unwrap().in_lunar_magic_shape();
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(theirs.changes_from(&base)),
        ..Default::default()
    };
    let built = match build::build(clean, &project) {
        Ok(rom) => rom,
        Err(e) => return format!("build: {e}"),
    };
    let ours = match Overworld::read(&built) {
        Ok(o) => o,
        Err(e) => return format!("read back: {e}"),
    };
    let mut out = String::new();
    for (what, same) in [
        ("layer 1", ours.layer1 == theirs.layer1),
        ("translevels", ours.translevels == theirs.translevels),
        ("directions", ours.directions == theirs.directions),
        ("layer 2", ours.layer2 == theirs.layer2),
        ("names", ours.names == theirs.names),
        ("events", ours.event_list() == theirs.event_list()),
        ("crush", ours.events.crush == theirs.events.crush),
    ] {
        if !same {
            out += &format!(" {what} differs;");
        }
    }
    match kobo_core::expand::load_overworld(&built) {
        Ok(loaded) => {
            let d = kobo_core::overworld::differences(&ours, &loaded);
            if d.is_empty() {
                out += " the build loads as read";
            } else {
                out += &format!(" load: {}", d.join(", "));
            }
        }
        Err(e) => out += &format!(" load: {e}"),
    }
    out
}

/// Kobo's build of a hack's overworld onto the clean ROM.
fn build_overworld(clean: &Rom, hack: &Rom) -> Result<Rom, String> {
    use kobo_core::build::{self, Project};
    use kobo_core::overworld::Overworld;
    let theirs = Overworld::read(hack)
        .map_err(|e| format!("read: {e}"))?
        .in_lunar_magic_shape();
    let base = Overworld::read(clean).unwrap().in_lunar_magic_shape();
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(theirs.changes_from(&base)),
        ..Default::default()
    };
    build::build(clean, &project).map_err(|e| format!("build: {e}"))
}

/// A hack's own load and Kobo's build of its overworld, every event passed.
fn passed_check(clean: &Rom, hack: &Rom) -> String {
    match build_overworld(clean, hack) {
        Ok(built) => compare_passed(hack, &built),
        Err(e) => e,
    }
}

/// Two ROMs' loads with every event passed: the tables that differ.
fn compare_passed(hack: &Rom, built: &Rom) -> String {
    compare_with(hack, built, &[0xFF; 0x0F])
}

/// Two ROMs' loads with the events `passed`: the tables that differ.
fn compare_with(hack: &Rom, built: &Rom, all: &[u8; 0x0F]) -> String {
    use kobo_core::ram::RamAddr;
    let (a, b) = match (
        kobo_core::expand::load_overworld_passed(hack, all),
        kobo_core::expand::load_overworld_passed(built, all),
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return format!("the hack's load: {e}"),
        (_, Err(e)) => return format!("the build's load: {e}"),
    };
    let mut out = String::new();
    for (what, at, len) in [
        ("layer 1", 0x7E_C800, 0x800),
        ("pages", 0x7F_C800, 0x800),
        ("translevels", 0x7E_D000, 0x800),
        ("directions", 0x7E_D800, 0x800),
        ("layer 2", 0x7F_4000, 0x4000),
    ] {
        let (x, y) = (
            a.ram.bytes(RamAddr::new(at), len),
            b.ram.bytes(RamAddr::new(at), len),
        );
        let differ: Vec<usize> = (0..len).filter(|&i| x[i] != y[i]).collect();
        if !differ.is_empty() {
            out += &format!(
                " {what}: {} differ (first at +{:#x});",
                differ.len(),
                differ[0]
            );
            if std::env::var_os("SHOW").is_some() {
                for &i in &differ {
                    out += &format!("\n  +{i:#06x}: {:02x} {:02x}", x[i], y[i]);
                }
            }
        }
    }
    // The rest of what the library compares: the level name.
    for d in kobo_core::overworld::load_differences(&a, &b) {
        if d.contains("name") {
            out += &format!(" {d};");
        }
    }
    let vram = (0..a.vram.len())
        .filter(|&i| a.vram[i] != b.vram[i])
        .count();
    if out.is_empty() {
        format!("the same with every event passed (VRAM {vram} bytes differ)")
    } else {
        out
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
        Some("load") if args.len() >= 3 => {
            // `load rom.smc dir`: the overworld as Kobo's machine loads it,
            // as dump_overworld.sh writes it.
            let rom = Rom::load(&args[1]).unwrap();
            let loaded = kobo_core::expand::load_overworld(&rom).unwrap();
            let dir = std::path::Path::new(&args[2]);
            std::fs::create_dir_all(dir).unwrap();
            let mut wram = loaded
                .ram
                .bytes(kobo_core::ram::RamAddr::new(0x7E_0000), 0x2_0000);
            wram[0x110..0x200].fill(0);
            std::fs::write(dir.join("overworld.wram.bin"), wram).unwrap();
            std::fs::write(dir.join("overworld.vram.bin"), &loaded.vram).unwrap();
            std::fs::write(dir.join("overworld.cgram.bin"), &loaded.cgram).unwrap();
            println!("loaded at frame {}", loaded.frames);
        }
        Some("check") if args.len() >= 2 => {
            // `check rom...`: the overworld as Overworld::read reads it,
            // against what the ROM's own load leaves in RAM.
            for path in &args[1..] {
                let rom = Rom::load(path).unwrap();
                let name = std::path::Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy();
                println!("{name}: {}", check(&rom));
            }
        }
        Some("build") if args.len() >= 3 => {
            // `build clean.smc hack.smc...`: each hack's overworld built
            // onto the clean ROM by Kobo, read back, and loaded.
            let clean = Rom::load(&args[1]).unwrap();
            for path in &args[2..] {
                let rom = Rom::load(path).unwrap();
                let name = std::path::Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy();
                println!("{name}: {}", build_check(&clean, &rom));
            }
        }
        Some("passed") if args.len() >= 3 => {
            // `passed clean.smc hack.smc...`: each hack's own load and
            // Kobo's build of its overworld, with every event passed.
            let clean = Rom::load(&args[1]).unwrap();
            for path in &args[2..] {
                let rom = Rom::load(path).unwrap();
                let name = std::path::Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy();
                println!("{name}: {}", passed_check(&clean, &rom));
            }
        }
        Some("each") if args.len() == 3 => {
            // `each a.smc b.smc`: the events that, passed alone, load
            // differently.
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            for e in 0..0x78 {
                let mut passed = [0; 0x0F];
                passed[e / 8] = 0x80 >> (e % 8);
                let out = compare_with(&a, &b, &passed);
                if std::env::var("EVENT").is_ok_and(|v| usize::from_str_radix(&v, 16).unwrap() != e)
                {
                    continue;
                }
                if !out.starts_with("the same") {
                    println!("event {e:#04x}:{out}");
                }
            }
        }
        Some("write") if args.len() == 4 => {
            // `write clean.smc hack.smc out.sfc`: Kobo's build of the
            // hack's overworld.
            let clean = Rom::load(&args[1]).unwrap();
            let hack = Rom::load(&args[2]).unwrap();
            let built = build_overworld(&clean, &hack).unwrap();
            std::fs::write(&args[3], built.data()).unwrap();
        }
        Some("end") if args.len() == 3 => {
            // `end a.smc b.smc`: each event's last step in play, on the
            // overworld with no event passed, in both ROMs.
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            for e in 0..0x78u8 {
                let none = [0; 0x0F];
                match (
                    kobo_core::expand::end_event(&a, &none, e),
                    kobo_core::expand::end_event(&b, &none, e),
                ) {
                    (Ok(x), Ok(y)) => {
                        let d = kobo_core::overworld::load_differences(&x, &y);
                        if !d.is_empty() {
                            println!("event {e:#04x}: {}", d.join(", "));
                        }
                    }
                    (x, y) => println!("event {e:#04x}: {:?} {:?}", x.err(), y.err()),
                }
            }
        }
        Some("ends") if args.len() == 2 => {
            // `ends rom.smc`: what each event's last step changes.
            let a = Rom::load(&args[1]).unwrap();
            let none = [0; 0x0F];
            let base = kobo_core::expand::load_overworld(&a).unwrap();
            for e in 0..0x78u8 {
                let x = kobo_core::expand::end_event(&a, &none, e).unwrap();
                let d = kobo_core::overworld::load_differences(&base, &x);
                if !d.is_empty() {
                    println!("event {e:#04x}: {}", d.join(", "));
                }
            }
        }
        Some("name") if args.len() >= 2 => {
            // `name rom...`: the stripe image the level name went out in,
            // with no event passed and with all.
            use kobo_core::ram::RamAddr;
            for path in &args[1..] {
                let rom = Rom::load(path).unwrap();
                for passed in [[0; 0x0F], [0xFF; 0x0F]] {
                    match kobo_core::expand::load_overworld_passed(&rom, &passed) {
                        Ok(l) => {
                            let b = l.ram.bytes(RamAddr::new(0x7F_837B), 0x34);
                            let hex: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
                            println!("{path}: {}", hex.join(" "));
                        }
                        Err(e) => println!("{path}: {e}"),
                    }
                }
            }
        }
        Some("enter") if args.len() >= 2 => {
            // `enter rom...`: the level number the level load takes for
            // each translevel entered through $0109, from the main map and
            // from a submap ($0E-$0F after CODE_05D796).
            use kobo_core::ram::RamAddr;
            for path in &args[1..] {
                let rom = Rom::load(path).unwrap();
                let mut line = String::new();
                for t in [0x01u8, 0x24, 0x25, 0x30, 0x5F] {
                    for submap in [0u8, 1] {
                        let ram =
                            kobo_core::expand::enter_by_exit(&rom, 0, 0, false, submap, |ram| {
                                ram.set_u8(kobo_core::ram::SUBLEVEL_COUNT, 0);
                                ram.set_u8(RamAddr::new(0x7E_0109), t);
                                if let Ok(n) = std::env::var("TRANSLEVEL") {
                                    let n = u8::from_str_radix(&n, 16).unwrap();
                                    ram.set_u8(RamAddr::new(0x7E_13BF), n);
                                }
                            })
                            .unwrap();
                        let level = u16::from(ram.u8(RamAddr::new(0x7E_000F))) << 8
                            | u16::from(ram.u8(RamAddr::new(0x7E_000E)));
                        line += &format!(" {t:02X}/{submap}:{level:03X}");
                    }
                }
                println!("{path}:{line}");
            }
        }
        Some("enter2") if args.len() >= 2 => {
            // `enter2 rom...`: as `enter`, from the player's place (x 5,
            // y 6), with each bit of the place's direction byte set.
            use kobo_core::ram::RamAddr;
            let at = |a: u32| RamAddr::new(0x7E_0000 | a);
            for path in &args[1..] {
                let rom = Rom::load(path).unwrap();
                let mut line = String::new();
                for t in [0x05u8, 0x30] {
                    for submap in [0u8, 1] {
                        for dir in [0u8, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80] {
                            let ram = kobo_core::expand::enter_by_exit(
                                &rom,
                                0,
                                0,
                                false,
                                submap,
                                |ram| {
                                    ram.set_u8(kobo_core::ram::SUBLEVEL_COUNT, 0);
                                    ram.set_u8(at(0x0109), 0);
                                    ram.set_u8(at(0x0DD6), 0);
                                    ram.set_u8(at(0x1F1F), 5);
                                    ram.set_u8(at(0x1F20), 0);
                                    ram.set_u8(at(0x1F21), 6);
                                    ram.set_u8(at(0x1F22), 0);
                                    let place = 5 | 6 << 4 | if submap != 0 { 0x400 } else { 0 };
                                    ram.set_u8(RamAddr::new(0x7E_D000 + place), t);
                                    ram.set_u8(RamAddr::new(0x7E_D800 + place), dir);
                                },
                            )
                            .unwrap();
                            let level =
                                u16::from(ram.u8(at(0x0F))) << 8 | u16::from(ram.u8(at(0x0E)));
                            line += &format!(" {t:02X}/{submap}/{dir:02X}:{level:03X}");
                        }
                    }
                }
                println!("{path}:{line}");
            }
        }
        Some("beat") if args.len() >= 5 => {
            // `beat rom.smc submap x y [exit] [frames]`: the overworld
            // process frame by frame after the level tile there is beaten.
            let rom = Rom::load(&args[1]).unwrap();
            let n = |i: usize, d: u32| {
                args.get(i)
                    .map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap())
                    .unwrap_or(d)
            };
            let place = (n(2, 0) as u8, n(3, 0) as u8, n(4, 0) as u8);
            let (loaded, processes) =
                kobo_core::expand::beat_level(&rom, &[0; 0x0F], place, n(5, 1) as u8, n(6, 0x200))
                    .unwrap();
            let mut runs: Vec<((u8, u8), usize)> = Vec::new();
            for p in processes {
                match runs.last_mut() {
                    Some((q, c)) if *q == p => *c += 1,
                    _ => runs.push((p, 1)),
                }
            }
            println!("{runs:x?}");
            let _ = loaded;
        }
        Some("beats") if args.len() >= 3 => {
            // `beats a.smc b.smc [exit]`: every level tile of a's beaten,
            // in both ROMs: what differs after its event.
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            let exit = args.get(3).map(|e| e.parse().unwrap()).unwrap_or(1u8);
            let read = kobo_core::overworld::Overworld::read(&a).unwrap();
            let mut seen = std::collections::BTreeSet::new();
            let mut beaten = Vec::new();
            let mut levels = Vec::new();
            for (i, &t) in read.translevels.iter().enumerate() {
                if t != 0 && seen.insert(t) {
                    beaten.push((kobo_core::overworld::layer1_place(i), exit));
                    levels.push(t);
                }
            }
            let none = [0; 0x0F];
            let x = kobo_core::expand::beat_levels(&a, &none, &beaten, 0x200).unwrap();
            let y = kobo_core::expand::beat_levels(&b, &none, &beaten, 0x200).unwrap();
            for (((place, _), t), ((x1, xs), (y1, ys))) in
                beaten.iter().zip(&levels).zip(x.iter().zip(&y))
            {
                let mut d = kobo_core::overworld::load_differences(x1, y1);
                let vram = (0..x1.vram.len())
                    .filter(|&i| x1.vram[i] != y1.vram[i])
                    .count();
                if vram > 0 {
                    d.push(format!("VRAM: {vram} bytes differ"));
                }
                if xs != ys {
                    d.push("the steps differ".into());
                }
                if !d.is_empty() {
                    println!("{t:02X} at {place:?}: {}", d.join(", "));
                }
            }
        }
        Some("png") if args.len() == 4 => {
            // `png rom.smc submap out.png`: the overworld as a player on
            // that submap sees it.
            let rom = Rom::load(&args[1]).unwrap();
            let submap = args[2].parse().unwrap();
            let img = kobo_core::render::render_overworld(&rom, submap).unwrap();
            img.write_png(&args[3]).unwrap();
        }
        Some("same") if args.len() == 3 => {
            // `same a.smc b.smc`: the two ROMs' overworlds, as the reader
            // reads them, table by table.
            use kobo_core::overworld::Overworld;
            let read = |p: &str| {
                Overworld::read(&Rom::load(p).unwrap())
                    .unwrap()
                    .in_lunar_magic_shape()
            };
            let (a, b) = (read(&args[1]), read(&args[2]));
            let mut differ = Vec::new();
            for (what, same) in [
                ("layer 1", a.layer1 == b.layer1),
                ("translevels", a.translevels == b.translevels),
                ("directions", a.directions == b.directions),
                ("layer 2", a.layer2 == b.layer2),
                ("names", a.names == b.names),
                ("events", a.event_list() == b.event_list()),
                ("crush", a.events.crush == b.events.crush),
                ("reveal", a.events.reveal == b.events.reveal),
                ("start", a.start == b.start),
                ("opened", a.opened == b.opened),
                ("level events", a.level_events == b.level_events),
                ("tables", a.tables == b.tables),
            ] {
                if !same {
                    differ.push(what);
                }
            }
            if std::env::var_os("SHOW").is_some() {
                for (i, (x, y)) in a.layer1.iter().zip(&b.layer1).enumerate() {
                    if x != y {
                        println!(
                            "  layer 1 {:?}: {x:03X} {y:03X}",
                            kobo_core::overworld::layer1_place(i)
                        );
                    }
                }
                for (i, (x, y)) in a.translevels.iter().zip(&b.translevels).enumerate() {
                    if x != y {
                        println!(
                            "  translevel {:?}: {x:02X} {y:02X}",
                            kobo_core::overworld::layer1_place(i)
                        );
                    }
                }
                for (i, (x, y)) in a.events.crush.iter().zip(&b.events.crush).enumerate() {
                    if x != y {
                        println!("  crush {i}: {x:?} {y:?}");
                    }
                }
            }
            if differ.is_empty() {
                println!("the same overworld");
            } else {
                println!("differ: {}", differ.join(", "));
            }
        }
        Some("pair") if args.len() == 3 => {
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            println!("{}", compare_passed(&a, &b));
        }
        _ => eprintln!(
            "usage: ow_probe blocks a.smc b.smc [wram.bin] | load rom.smc dir | check rom... \
             | build clean.smc hack... | passed clean.smc hack... | write clean.smc hack.smc out.sfc \
             | pair a.smc b.smc | each a.smc b.smc | end a.smc b.smc | ends rom.smc"
        ),
    }
}
