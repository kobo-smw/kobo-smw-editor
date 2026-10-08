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
    let mut changes = theirs.changes_from(&base);
    changes.reveal_speed =
        kobo_core::expand::reveal_speed(hack, clean).map_err(|e| e.to_string())?;
    let project = Project {
        root: std::path::PathBuf::from("."),
        overworld: Some(changes),
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
                ("level flags", a.level_flags == b.level_flags),
                ("level events", a.level_events == b.level_events),
                ("tables", a.tables == b.tables),
                ("16x16 tiles", a.tiles == b.tiles),
                (
                    "ExAnimation",
                    a.animation.as_ref().filter(|x| !x.is_empty())
                        == b.animation.as_ref().filter(|x| !x.is_empty()),
                ),
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
        Some("warps") if args.len() == 3 => {
            // `warps a.smc b.smc`: every star and pipe tile of a's (layer 1
            // tiles 5B, 5F, 81, 82, 59...), warped from in both ROMs.
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            let read = kobo_core::overworld::Overworld::read(&a).unwrap();
            let mut places = Vec::new();
            for (i, &tile) in read.layer1.iter().enumerate() {
                if matches!(tile & 0xFF, 0x5B | 0x5F | 0x82 | 0x81 | 0x59 | 0x58) {
                    let (map, x, y) = kobo_core::overworld::layer1_place(i);
                    for submap in if map == 0 { 0..=0 } else { 1..=6 } {
                        places.push((submap, x, y));
                    }
                }
            }
            let x = kobo_core::expand::warp(&a, &places).unwrap();
            let y = kobo_core::expand::warp(&b, &places).unwrap();
            let mut same = 0;
            for ((place, p), q) in places.iter().zip(&x).zip(&y) {
                if p == q {
                    same += 1;
                } else {
                    println!("{place:?}: {p:x?} {q:x?}");
                }
            }
            let found = x.iter().filter(|w| w.is_some()).count();
            println!("{same} of {} the same ({found} warps)", places.len());
        }
        Some("prompts") if args.len() == 2 => {
            // `prompts rom.smc`: each level beaten by its normal exit, and
            // whether the overworld then waits in process 5 (a prompt),
            // with its translevel's byte of the table at $05DDA0 and its
            // layer 1 tile.
            let a = Rom::load(&args[1]).unwrap();
            let read = kobo_core::overworld::Overworld::read(&a).unwrap();
            let mut seen = std::collections::BTreeSet::new();
            let mut beaten = Vec::new();
            for (i, &t) in read.translevels.iter().enumerate() {
                if t != 0 && seen.insert(t) {
                    beaten.push((
                        (kobo_core::overworld::layer1_place(i), 1u8),
                        t,
                        read.layer1[i],
                    ));
                }
            }
            let list: Vec<_> = beaten.iter().map(|b| b.0).collect();
            let x = kobo_core::expand::beat_levels(&a, &[0; 0x0F], &list, 0x200).unwrap();
            for ((_, t, tile), (_, steps)) in beaten.iter().zip(&x) {
                let waits = steps.iter().rev().take(0x40).all(|s| s.0 == 5);
                let flags = a
                    .read_u8(kobo_core::SnesAddr::new(0x05_DDA0 + u32::from(*t)))
                    .unwrap();
                println!(
                    "{t:02X} tile {tile:03X} flags {flags:08b} {}",
                    if waits { "PROMPT" } else { "" }
                );
            }
        }
        Some("enters") if args.len() >= 5 => {
            // `enters rom.smc submap x y`: whether the level tile there is
            // entered with each of several settings bytes.
            let rom = Rom::load(&args[1]).unwrap();
            let n = |i: usize| u8::from_str_radix(&args[i], 16).unwrap();
            let settings = [0x00u8, 0x10, 0x20, 0x40, 0x80, 0xA0, 0xB0, 0x60, 0xE0, 0x30];
            let got = kobo_core::expand::enters(&rom, (n(2), n(3), n(4)), &settings).unwrap();
            for (s, e) in settings.iter().zip(got) {
                print!(" {s:02X}:{}", if e { "in" } else { "no" });
            }
            println!();
        }
        Some("speed") if args.len() >= 3 => {
            // `speed clean.smc rom...`: each ROM's path reveal speed.
            let clean = Rom::load(&args[1]).unwrap();
            for path in &args[2..] {
                let rom = Rom::load(path).unwrap();
                println!(
                    "{path}: {:?}",
                    kobo_core::expand::reveal_speed(&rom, &clean)
                );
            }
        }
        Some("lists") if args.len() == 2 => {
            // `lists rom.smc`: for each submap, where $7FC006 points after
            // the overworld's load, as a list number from read3($0FF7FF).
            use kobo_core::ram::RamAddr;
            let rom = Rom::load(&args[1]).unwrap();
            let base = rom.read_u24(kobo_core::SnesAddr::new(0x0F_F7FF)).unwrap();
            for submap in 0..=6u8 {
                let l = kobo_core::expand::load_overworld_on(&rom, submap).unwrap();
                let p = u32::from(l.ram.u8(RamAddr::new(0x7F_C006)))
                    | u32::from(l.ram.u8(RamAddr::new(0x7F_C007))) << 8
                    | u32::from(l.ram.u8(RamAddr::new(0x7F_C008))) << 16;
                let list = p.wrapping_sub(base) as i64 / 32;
                println!("submap {submap}: $7FC006 = {p:06X} (list {list:X}, base {base:06X})");
            }
        }
        Some("sublists") if args.len() == 2 => {
            let rom = Rom::load(&args[1]).unwrap();
            println!("has exgfx {}", kobo_core::exgfx::has_exgfx(&rom));
            for n in 0x200..0x207 {
                println!("{n:X}: {:x?}", kobo_core::exgfx::read_list(&rom, n));
            }
        }
        Some("cgram") if args.len() == 4 => {
            // `cgram rom.smc submap out.bin`: CGRAM after the load on a submap.
            let rom = Rom::load(&args[1]).unwrap();
            let l = kobo_core::expand::load_overworld_on(&rom, args[2].parse().unwrap()).unwrap();
            std::fs::write(&args[3], &l.cgram).unwrap();
        }
        Some("anims") if args.len() == 3 => {
            // `anims base.smc rom.smc`: the RATS blocks `rom` has that
            // `base` has not which read as an ExAnimation list exactly their
            // length: slots and types only, never the bytes, since a block
            // may be Lunar Magic's code.
            let base = Rom::load(&args[1]).unwrap();
            let rom = Rom::load(&args[2]).unwrap();
            let theirs: std::collections::HashSet<Vec<u8>> = rats::blocks(&base)
                .iter()
                .map(|b| base.read(b.start, b.len).unwrap().to_vec())
                .collect();
            for block in rats::blocks(&rom) {
                let bytes = rom.read(block.start, block.len).unwrap();
                if theirs.contains(bytes) {
                    continue;
                }
                if let Ok((list, len)) = kobo_core::exanimation::List::parse(bytes)
                    && len == block.len
                {
                    let kinds: Vec<String> = list
                        .slots
                        .iter()
                        .map(|(n, s)| format!("{n}:{:02X}/{:02X}", s.kind, s.trigger))
                        .collect();
                    println!(
                        "{} {:#x} bytes: {} slots, alt file {}, {}",
                        block.start,
                        block.len,
                        list.count,
                        list.alt_file,
                        kinds.join(" ")
                    );
                }
            }
        }
        Some("frames") if args.len() >= 5 => {
            // `frames a.smc b.smc submap frames [ram]`: the overworld on a
            // submap played on in both ROMs, frame by frame: where VRAM
            // (by $400 words) and CGRAM differ, and with `ram`, RAM.
            let submap: u8 = args[3].parse().unwrap();
            let frames: u32 = args[4].parse().unwrap();
            let play = |p: &str| {
                kobo_core::expand::play_overworld_on(&Rom::load(p).unwrap(), submap, frames)
                    .unwrap()
            };
            let (a, b) = (play(&args[1]), play(&args[2]));
            for (n, (x, y)) in a.iter().zip(&b).enumerate() {
                let mut line = Vec::new();
                for (k, (p, q)) in x.vram.chunks(0x800).zip(y.vram.chunks(0x800)).enumerate() {
                    let differ = p.iter().zip(q).filter(|(c, d)| c != d).count();
                    if differ > 0 {
                        line.push(format!("VRAM ${:04X} {differ}", k * 0x400));
                    }
                }
                if std::env::var_os("DETAIL").is_some() {
                    let mut runs: Vec<(usize, usize)> = Vec::new();
                    for w in 0..x.vram.len() / 2 {
                        if x.vram[2 * w..2 * w + 2] != y.vram[2 * w..2 * w + 2] {
                            match runs.last_mut() {
                                Some(r) if r.1 + 1 == w => r.1 = w,
                                _ => runs.push((w, w)),
                            }
                        }
                    }
                    let text: Vec<String> = runs
                        .iter()
                        .map(|(a, b)| format!("{a:04X}-{b:04X}"))
                        .collect();
                    line.push(format!("words {}", text.join(" ")));
                }
                let cg: Vec<usize> = (0..x.cgram.len() / 2)
                    .filter(|&i| x.cgram[2 * i..2 * i + 2] != y.cgram[2 * i..2 * i + 2])
                    .collect();
                if !cg.is_empty() {
                    line.push(format!("colours {cg:02X?}"));
                }
                if args.get(5).is_some_and(|s| s == "ram") {
                    let (p, q) = (
                        x.ram
                            .bytes(kobo_core::ram::RamAddr::new(0x7E_0000), 0x2_0000),
                        y.ram
                            .bytes(kobo_core::ram::RamAddr::new(0x7E_0000), 0x2_0000),
                    );
                    // The stack left out: on a ROM Lunar Magic saved it
                    // holds where its code is.
                    let at: Vec<String> = (0..p.len())
                        .filter(|&i| p[i] != q[i] && !(0x100..0x200).contains(&i))
                        .map(|i| format!("{:06X}", 0x7E_0000 + i))
                        .collect();
                    if !at.is_empty() {
                        line.push(format!("RAM {} {:?}", at.len(), &at[..at.len().min(64)]));
                    }
                }
                if !line.is_empty() {
                    println!("frame {n}: {}", line.join("; "));
                }
            }
        }
        Some("vfind") if args.len() == 4 => {
            // `vfind rom.smc submap word`: where VRAM's 32 bytes from word
            // (hex) are in the overworld's RAM, as the load leaves it.
            let submap: u8 = args[2].parse().unwrap();
            let w = usize::from_str_radix(&args[3], 16).unwrap();
            let l = kobo_core::expand::load_overworld_on(&Rom::load(&args[1]).unwrap(), submap)
                .unwrap();
            let needle = &l.vram[2 * w..2 * w + 32];
            let ram = l
                .ram
                .bytes(kobo_core::ram::RamAddr::new(0x7E_0000), 0x2_0000);
            let hits: Vec<String> = ram
                .windows(32)
                .enumerate()
                .filter(|(_, x)| *x == needle)
                .map(|(i, _)| format!("{:06X}", 0x7E_0000 + i))
                .collect();
            println!("VRAM {w:04X}: {:02X?}... in RAM at {hits:?}", &needle[..8]);
            let mut planes = Vec::new();
            for i in (0..ram.len() - 16).step_by(1) {
                if ram[i..i + 16] == needle[..16] {
                    planes.push(format!("{:06X}", 0x7E_0000 + i));
                }
            }
            println!(
                "  its first 16 bytes at {:?}",
                &planes[..planes.len().min(8)]
            );
        }
        Some("ram") if args.len() == 7 => {
            // `ram rom.smc submap frames from len`: RAM from `from` (hex),
            // `len` bytes, after each frame of the overworld played on.
            let submap: u8 = args[2].parse().unwrap();
            let frames: u32 = args[3].parse().unwrap();
            let from = u32::from_str_radix(&args[4], 16).unwrap();
            let len: usize = args[5].parse().unwrap();
            assert!(!(0x100..0x200).contains(&(from & 0xFFFF)), "not the stack");
            let played =
                kobo_core::expand::play_overworld_on(&Rom::load(&args[1]).unwrap(), submap, frames)
                    .unwrap();
            for (n, f) in played.iter().enumerate() {
                let b = f.ram.bytes(kobo_core::ram::RamAddr::new(from), len);
                let hex: Vec<String> = b.iter().map(|x| format!("{x:02X}")).collect();
                println!(
                    "{n:3} frame {} $14={:02X} {}",
                    f.frames,
                    f.ram.u8(kobo_core::ram::RamAddr::new(0x7E_0014)),
                    hex.join(" ")
                );
            }
        }
        Some("owanim") if args.len() == 2 => {
            // `owanim rom.smc`: the overworld's ExAnimation as read: each
            // submap's settings and slots, and the global list's.
            match kobo_core::exanimation::read_overworld(&Rom::load(&args[1]).unwrap()) {
                Ok(Some(a)) => {
                    let lists: Vec<String> = a
                        .submaps
                        .iter()
                        .map(|l| l.as_ref().map_or("-".into(), |l| l.slots.len().to_string()))
                        .collect();
                    let mut triggers: Vec<u8> = a
                        .submaps
                        .iter()
                        .chain(std::iter::once(&a.global))
                        .flatten()
                        .flat_map(|l| l.slots.values().map(|s| s.trigger))
                        .collect();
                    triggers.sort();
                    triggers.dedup();
                    println!("triggers {triggers:02X?}");
                    println!(
                        "settings {:02X?}, submaps' slots {}, global {}",
                        a.settings,
                        lists.join(" "),
                        a.global
                            .as_ref()
                            .map_or("-".into(), |l| l.slots.len().to_string())
                    );
                }
                Ok(None) => println!("none installed"),
                Err(e) => println!("{e}"),
            }
        }
        Some("vram") if args.len() == 4 => {
            // `vram a.smc b.smc submap`: where in VRAM the two ROMs' loads
            // on a submap differ, by $400 words, with the layers' tilemap
            // and character bases, and CGRAM.
            let submap: u8 = args[3].parse().unwrap();
            let load = |p: &str| {
                kobo_core::expand::load_overworld_on(&Rom::load(p).unwrap(), submap).unwrap()
            };
            let (a, b) = (load(&args[1]), load(&args[2]));
            println!(
                "BGnSC {:02X?} {:02X?}, characters {:04X?} {:04X?}",
                a.bg_sc, b.bg_sc, a.bg_character_base, b.bg_character_base
            );
            for (n, (x, y)) in a.vram.chunks(0x800).zip(b.vram.chunks(0x800)).enumerate() {
                let differ = x.iter().zip(y).filter(|(p, q)| p != q).count();
                if differ > 0 {
                    println!("  VRAM word ${:04X}: {differ} bytes differ", n * 0x400);
                }
            }
            let differ = a.cgram.iter().zip(&b.cgram).filter(|(p, q)| p != q).count();
            println!("  CGRAM: {differ} bytes differ");
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
