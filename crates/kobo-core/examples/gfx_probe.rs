//! Where a level's graphics end up in VRAM, found from memory effects only
//! (docs/lunar-magic-install.md, "Graphics").
//!
//! `vram rom level`: the tilemap and character registers the load left,
//! and which VRAM ranges were written.
//!
//! `map a.smc b.smc level`: each run of 32-byte tiles in `b`'s VRAM after
//! the load, by where the same bytes are in `a`'s, so a layout that moved
//! things shows as runs with another source.
//!
//! `cargo run --release --example gfx_probe -- vram rom.smc 105`

#[path = "common/path.rs"]
mod path;

use kobo_core::{Rom, expand};

fn load(path: &str, level: u16) -> expand::LoadedLevel {
    let rom = Rom::load(path).unwrap();
    expand::expand_level(&rom, level).unwrap()
}

fn level_arg(text: &str) -> u16 {
    u16::from_str_radix(text, 16).unwrap()
}

fn written_runs(written: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < written.len() {
        if written[i] {
            let start = i;
            while i < written.len() && written[i] {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }
    runs
}

fn vram(path: &str, level: u16) {
    let loaded = load(path, level);
    let v = &loaded.video;
    let sc: Vec<String> = v.bg_sc.iter().map(|b| format!("{b:02X}")).collect();
    println!("BGnSC {} OBSEL {:02X}", sc.join(" "), v.object_select);
    for (start, end) in written_runs(&v.vram_written) {
        println!(
            "written words {:04X}-{:04X} ({:#x} bytes)",
            start / 2,
            (end - 1) / 2,
            end - start
        );
    }
}

fn map(a: &str, b: &str, level: u16) {
    let (va, vb) = (load(a, level).video.vram, load(b, level).video.vram);
    let tile = |v: &[u8], i: usize| v[i * 32..i * 32 + 32].to_vec();
    let mut index = std::collections::HashMap::new();
    for i in (0..va.len() / 32).rev() {
        index.insert(tile(&va, i), i);
    }
    let blank = vec![0u8; 32];
    // Runs of tiles of b at consecutive places in a.
    let mut i = 0;
    let n = vb.len() / 32;
    while i < n {
        let t = tile(&vb, i);
        let from = if t == blank {
            None
        } else {
            index.get(&t).copied()
        };
        let mut j = i + 1;
        while j < n {
            let u = tile(&vb, j);
            let f = if u == blank {
                None
            } else {
                index.get(&u).copied()
            };
            let expect = from.map(|s| s + (j - i));
            if f != expect
                && !(u == blank && from.is_some() && tile(&va, from.unwrap() + j - i) == blank)
            {
                break;
            }
            j += 1;
        }
        let words = |t: usize| t * 16;
        match from {
            Some(s) => println!(
                "b {:04X}-{:04X} = a {:04X}-{:04X}",
                words(i),
                words(j) - 1,
                words(s),
                words(s + j - i) - 1
            ),
            None if t == blank => println!("b {:04X}-{:04X} blank", words(i), words(j) - 1),
            None => println!("b {:04X}-{:04X} not in a", words(i), words(j) - 1),
        }
        i = j;
    }
}

/// A path for the player: `right` walks the camera right 3 pixels a
/// frame, `down` moves the player down a vertical level or a horizontal
/// one with vertical scrolling on, `diag` both.
fn steer(mode: &str, frame: u32, ram: &mut kobo_core::ram::Ram, start: (u16, u16)) {
    use kobo_core::ram::RamAddr;
    let named = match mode {
        "right" => "3,0",
        "left" => "-3,0",
        "down" => "0,3",
        "up" => "0,-3",
        "diag" => "3,2",
        other => other,
    };
    path::steer(named, frame, ram, start, false);
    // KOBO_GFX_POKE_TILE=dx,dy,tile (hex): on the first frame, a tile at the
    // player's starting block plus (dx, dy) blocks of a horizontal level (a
    // coin the player then collects, say), for the tile change uploads.
    if frame == 0
        && let Ok(spec) = std::env::var("KOBO_GFX_POKE_TILE")
    {
        let v: Vec<i32> = spec
            .split(',')
            .map(|t| i32::from_str_radix(t, 16).unwrap())
            .collect();
        let bx = start.0 as i32 / 16 + v[0];
        let by = start.1 as i32 / 16 + v[1];
        let offset = if ram.u8(RamAddr::new(0x7E_005B)) & 1 != 0 {
            (by / 16) * 0x200 + (bx / 16) * 0x100 + (by % 16) * 16 + bx % 16
        } else {
            (bx / 16) * 0x1B0 + by * 16 + bx % 16
        };
        ram.set_u8(RamAddr::new(0x7E_C800 + offset as u32), v[2] as u8);
        ram.set_u8(RamAddr::new(0x7F_C800 + offset as u32), (v[2] >> 8) as u8);
    }
}

/// One frame of [`play`]: the camera, VRAM, and work RAM `$0000`-`$1FFF`.
type Played = (u16, u16, Vec<u8>, Vec<u8>);

/// Plays `level` in each ROM along the same path and prints, per frame,
/// the camera and the VRAM words that differ between the two.
fn play(paths: &[String], level: u16, frames: u32, mode: &str) {
    use kobo_core::ram::RamAddr;
    let mut runs: Vec<Vec<Played>> = Vec::new();
    for path in paths {
        let rom = Rom::load(path).unwrap();
        let mut start = None;
        let mut out = Vec::new();
        expand::play_game_loop(
            &rom,
            level,
            frames,
            |frame, ram| {
                let s = *start.get_or_insert((
                    ram.u16(RamAddr::new(0x7E_0094)),
                    ram.u16(RamAddr::new(0x7E_0096)),
                ));
                steer(mode, frame, ram, s);
            },
            |_, played| {
                out.push((
                    played.ram.u16(RamAddr::new(0x7E_001A)),
                    played.ram.u16(RamAddr::new(0x7E_001C)),
                    played.vram.to_vec(),
                    kobo_core::clean_room::bytes(played.ram, RamAddr::new(0x7E_0000), 0x2000),
                ));
            },
        )
        .unwrap();
        runs.push(out);
    }
    for f in 0..frames as usize {
        let (x, y, ref va, ref ra) = runs[0][f];
        let mut line = format!("frame {f:3} camera {x:04X},{y:04X}");
        for run in &runs[1..] {
            let (x2, y2, ref vb, ref rb) = run[f];
            if (x2, y2) != (x, y) {
                line += &format!(" | other camera {x2:04X},{y2:04X}");
            }
            // KOBO_GFX_TILEMAPS=1: layer 1's and 2's tilemaps only.
            let range = if std::env::var_os("KOBO_GFX_TILEMAPS").is_some() {
                0x3000..0x4000
            } else {
                0..va.len() / 2
            };
            let words: Vec<usize> = range
                .filter(|w| va[w * 2..w * 2 + 2] != vb[w * 2..w * 2 + 2])
                .collect();
            let ram: Vec<usize> = (0..ra.len()).filter(|&i| ra[i] != rb[i]).collect();
            line += &format!(" | vram words differ: {}", summarize(&words));
            line += &format!(" | ram differs: {}", summarize(&ram));
        }
        println!("{line}");
    }
}

fn summarize(list: &[usize]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() && (out.len() < 12 || std::env::var_os("KOBO_GFX_FULL").is_some()) {
        let start = list[i];
        while i + 1 < list.len() && list[i + 1] == list[i] + 1 {
            i += 1;
        }
        out.push(if list[i] == start {
            format!("{start:04X}")
        } else {
            format!("{start:04X}-{:04X}", list[i])
        });
        i += 1;
    }
    format!("{} ({} total)", out.join(" "), list.len())
}

/// Prints, per frame, the camera and which tilemap words the frame
/// changed in one ROM, as rows and columns of each layer's map.
fn watch(path: &str, level: u16, frames: u32, mode: &str) {
    use kobo_core::ram::RamAddr;
    let rom = Rom::load(path).unwrap();
    let mut start = None;
    let mut prev: Option<Vec<u8>> = None;
    let mut first_ram: Option<Vec<u8>> = None;
    let mut ever = std::collections::BTreeSet::new();
    expand::play_game_loop(
        &rom,
        level,
        frames,
        |frame, ram| {
            let s = *start.get_or_insert((
                ram.u16(RamAddr::new(0x7E_0094)),
                ram.u16(RamAddr::new(0x7E_0096)),
            ));
            steer(mode, frame, ram, s);
        },
        |frame, played| {
            let x = played.ram.u16(RamAddr::new(0x7E_001A));
            let y = played.ram.u16(RamAddr::new(0x7E_001C));
            let x2 = played.ram.u16(RamAddr::new(0x7E_001E));
            let y2 = played.ram.u16(RamAddr::new(0x7E_0020));
            let mut line = format!(
                "frame {frame:3} L1 {x:04X},{y:04X} L2 {x2:04X},{y2:04X} sc {:02X} {:02X} player {:04X},{:04X} mode {:02X} anim {:02X} 9D {:02X} 1411 {:02X}",
                played.bg_sc[0],
                played.bg_sc[1],
                played.ram.u16(RamAddr::new(0x7E_0094)),
                played.ram.u16(RamAddr::new(0x7E_0096)),
                played.ram.u8(RamAddr::new(0x7E_0100)),
                played.ram.u8(RamAddr::new(0x7E_0071)),
                played.ram.u8(RamAddr::new(0x7E_009D)),
                played.ram.u8(RamAddr::new(0x7E_1411)),
            );
            if let Some(p) = &prev {
                let changed: Vec<usize> = (0..0x8000)
                    .filter(|w| p[w * 2..w * 2 + 2] != played.vram[w * 2..w * 2 + 2])
                    .collect();
                for (name, sc) in [("L1", played.bg_sc[0]), ("L2", played.bg_sc[1])] {
                    line += &format!(" {name}[{}]", map_changes(&changed, sc));
                }
            }
            println!("{line}");
            prev = Some(played.vram.to_vec());
            if std::env::var("KOBO_GFX_DUMP_FRAME").ok().and_then(|f| f.parse::<u32>().ok()) == Some(frame) {
                std::fs::write(std::env::var("KOBO_GFX_DUMP_TO").unwrap(), played.vram).unwrap();
            }
            // Which bytes of the given WRAM range ever change (KOBO_GFX_WATCH_RAM=7F8183,504).
            if let Ok(spec) = std::env::var("KOBO_GFX_WATCH_RAM") {
                let (a, n) = spec.split_once(',').unwrap();
                let (a, n) = (u32::from_str_radix(a, 16).unwrap(), n.parse::<usize>().unwrap());
                let now = kobo_core::clean_room::bytes(played.ram, RamAddr::new(a), n);
                let first = first_ram.get_or_insert(now.clone());
                for (i, (x, y)) in now.iter().zip(first.iter()).enumerate() {
                    if x != y {
                        ever.insert(i);
                    }
                }
                if frame + 1 == frames {
                    let list: Vec<usize> = ever.iter().map(|i| i + a as usize).collect();
                    println!("ram that changed: {}", summarize(&list));
                }
            }
        },
    )
    .unwrap();
}

/// Changed words of the tilemap `sc` (a `BGnSC` value) as `col:rows`.
fn map_changes(changed: &[usize], sc: u8) -> String {
    let base = (sc as usize & 0xFC) << 8;
    let (wide, tall) = (sc & 1 != 0, sc & 2 != 0);
    let len = 0x400 * (1 + wide as usize) * (1 + tall as usize);
    let mut cells = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for &w in changed.iter().filter(|&&w| w >= base && w < base + len) {
        let o = w - base;
        let quadrant = o / 0x400;
        let (qx, qy) = if wide {
            (quadrant % 2, quadrant / 2)
        } else {
            (0, quadrant)
        };
        let col = qx * 32 + o % 32;
        let row = qy * 32 + (o % 0x400) / 32;
        cells.entry(col).or_default().push(row);
    }
    let mut out = Vec::new();
    for (col, rows) in cells {
        out.push(format!("{col}:{}", summarize_dec(&rows)));
    }
    out.join(" ")
}

fn summarize_dec(list: &[usize]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        let start = list[i];
        while i + 1 < list.len() && list[i + 1] == list[i] + 1 {
            i += 1;
        }
        out.push(if list[i] == start {
            format!("{start}")
        } else {
            format!("{start}-{}", list[i])
        });
        i += 1;
    }
    out.join(",")
}

/// Per frame of a path: both layers' cameras, and the tilemap words of the
/// cells in view (64x32 maps at `$3000`/`$3800`: columns cx..cx+16, rows
/// cy..cy+14 of each layer).
type Visible = ((u16, u16, u16, u16), Vec<u16>);

fn visible_frames(
    rom: &Rom,
    rom_name: &str,
    level: u16,
    frames: u32,
    path: &str,
) -> Option<Vec<Visible>> {
    use kobo_core::ram::RamAddr;
    let mut start = None;
    let mut out = Vec::new();
    expand::play_game_loop(
        rom,
        level,
        frames,
        |frame, ram| {
            let s = *start.get_or_insert((
                ram.u16(RamAddr::new(0x7E_0094)),
                ram.u16(RamAddr::new(0x7E_0096)),
            ));
            steer(path, frame, ram, s);
        },
        |frame, played| {
            // KOBO_PROBE_DUMP=dir KOBO_PROBE_FRAME=n: that frame's VRAM, as
            // `<rom file name>.vram` in dir.
            if let (Some(dir), Some(n)) = (
                std::env::var_os("KOBO_PROBE_DUMP"),
                std::env::var("KOBO_PROBE_FRAME").ok(),
            ) && n.parse() == Ok(frame)
            {
                let name = format!("{}.vram", rom_name);
                std::fs::write(std::path::Path::new(&dir).join(name), played.vram).unwrap();
            }
            let r = |a| played.ram.u16(RamAddr::new(a));
            let cams = (r(0x7E_001A), r(0x7E_001C), r(0x7E_001E), r(0x7E_0020));
            let mut cells = Vec::new();
            for (base, x, y) in [(0x3000usize, cams.0, cams.1), (0x3800, cams.2, cams.3)] {
                let (cx, cy) = ((x >> 4) as usize, (y >> 4) as usize);
                for row in cy..cy + 15 {
                    for col in cx..cx + 17 {
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let tc = (col * 2 + dx) % 64;
                            let tr = (row * 2 + dy) % 32;
                            let w = base + (tc / 32) * 0x400 + tr * 32 + tc % 32;
                            cells.push(u16::from_le_bytes([
                                played.vram[w * 2],
                                played.vram[w * 2 + 1],
                            ]));
                        }
                    }
                }
            }
            out.push((cams, cells));
        },
    )
    .ok()?;
    Some(out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("vram") => vram(&args[1], level_arg(&args[2])),
        Some("map") => map(&args[1], &args[2], level_arg(&args[3])),
        Some("play") => {
            // play level frames mode rom...
            play(
                &args[4..],
                level_arg(&args[1]),
                args[2].parse().unwrap(),
                &args[3],
            )
        }
        Some("watch") => watch(
            &args[1],
            level_arg(&args[2]),
            args[3].parse().unwrap(),
            &args[4],
        ),
        Some("loads") => {
            // loads a b [range words, default 3000-4000]: every level whose
            // VRAM after the load differs between the two ROMs in that range.
            let (a, b) = (Rom::load(&args[1]).unwrap(), Rom::load(&args[2]).unwrap());
            let (lo, hi) = args
                .get(3)
                .and_then(|r| r.split_once('-'))
                .map(|(x, y)| {
                    (
                        usize::from_str_radix(x, 16).unwrap(),
                        usize::from_str_radix(y, 16).unwrap(),
                    )
                })
                .unwrap_or((0x3000, 0x4000));
            let mut same = 0;
            for level in 0..0x200u16 {
                let (Ok(la), Ok(lb)) = (
                    expand::expand_level(&a, level),
                    expand::expand_level(&b, level),
                ) else {
                    println!("{level:03X}: failed to load");
                    continue;
                };
                let words: Vec<usize> = (lo..hi)
                    .filter(|w| la.video.vram[w * 2..w * 2 + 2] != lb.video.vram[w * 2..w * 2 + 2])
                    .collect();
                if words.is_empty() && la.video.bg_sc == lb.video.bg_sc {
                    same += 1;
                } else {
                    println!("{level:03X}: {}", summarize(&words));
                }
            }
            println!("{same} levels the same");
        }
        Some("scroll") => {
            // scroll a b frames path [levels...]: plays each level along the
            // path in both ROMs and reports visible tilemap cells that
            // differ in two frames running.
            let (a, b) = (Rom::load(&args[1]).unwrap(), Rom::load(&args[2]).unwrap());
            let frames: u32 = args[3].parse().unwrap();
            let path = args[4].clone();
            let levels: Vec<u16> = if args.len() > 5 {
                args[5..].iter().map(|l| level_arg(l)).collect()
            } else {
                (0..0x200).collect()
            };
            let mut clean = 0;
            for level in levels {
                let va = visible_frames(&a, "a", level, frames, &path);
                let vb = visible_frames(&b, "b", level, frames, &path);
                let (Some(va), Some(vb)) = (va, vb) else {
                    println!("{level:03X}: failed");
                    continue;
                };
                let mut report = None;
                let mut last_bad = false;
                for f in 0..va.len().min(vb.len()) {
                    let (ca, cb) = (&va[f], &vb[f]);
                    let bad = ca.0 == cb.0 && ca.1 != cb.1;
                    if bad && last_bad && report.is_none() {
                        let n = ca.1.iter().zip(&cb.1).filter(|(x, y)| x != y).count();
                        report = Some(format!(
                            "{level:03X}: frame {f} camera {:?}: {n} visible cells differ",
                            ca.0
                        ));
                    }
                    last_bad = bad;
                }
                match report {
                    Some(r) => println!("{r}"),
                    None => clean += 1,
                }
            }
            println!("{clean} levels without a lasting difference");
        }
        Some("bypass") => {
            // Lunar Magic's settings objects 24 and 25 in every level.
            for path in &args[1..] {
                let rom = Rom::load(path).unwrap();
                let mut count = std::collections::BTreeMap::<String, usize>::new();
                for level in 0..0x200u16 {
                    let Ok(objects) = kobo_core::level::read_objects(&rom, level) else {
                        continue;
                    };
                    for o in &objects.layer1.objects {
                        if let kobo_core::level::objects::Object::Unplaced(bytes) = o
                            && matches!(o.lunar_number(), Some(0x24 | 0x25))
                        {
                            let hex: Vec<String> =
                                bytes.iter().map(|b| format!("{b:02X}")).collect();
                            *count.entry(hex.join(" ")).or_default() += 1;
                            if std::env::var_os("KOBO_GFX_LEVELS").is_some() {
                                println!("  {level:03X}: {}", hex.join(" "));
                            }
                        }
                    }
                }
                println!("{path}");
                for (k, n) in count {
                    println!("  {k}: {n}");
                }
            }
        }
        Some("dump") => {
            let loaded = load(&args[1], level_arg(&args[2]));
            std::fs::write(&args[3], &loaded.video.vram).unwrap();
        }
        _ => {
            eprintln!(
                "usage: gfx_probe vram|dump|map|watch|play|bypass|loads|scroll ... (see the source)"
            );
            std::process::exit(2)
        }
    }
}
