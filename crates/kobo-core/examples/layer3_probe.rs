//! What Lunar Magic's layer 3 settings do, found from memory effects only
//! (docs/lunar-magic-install.md, "Layer 3 settings").
//!
//! `play rom level frames path`: per frame of play along `path` (legs
//! `dx,dy@frames/...`, the player moved that much a frame), both layers'
//! positions, layer 3's RAM (`$22`/`$24`, `$13D5`, `$1403`, `$145E`-`$1460`,
//! `$40`, `$3E`, `$0D9D`/`$0D9E`, `$7FC01A`-`$7FC01F`), and the scroll and
//! screen registers the frame's NMI and IRQ left. `KOBO_LAYER3_WATCH=addr,len[;...]`
//! adds work RAM ranges, `KOBO_LAYER3_POKE=frame,addr,byte;...` (hex) sets bytes before
//! a frame (`KOBO_LAYER3_POKE_B`: in `compare`, for the second ROM only).
//!
//! `compare a b level frames path`: the frames on which the two ROMs leave
//! anything of that, or work RAM `$0000`-`$1FFF`, or layer 3's VRAM
//! (`$4000`-`$5FFF` words), different.
//!
//! `loads a b [levels]`: per level, what the load (and first frame) leaves
//! different in that same set.
//!
//! `sweep a b frames`: reads cases from standard input, one a line:
//! `level path setting...`, a setting `SLOT:n` (the slot's high nibble) or
//! `SLOT=word`; writes each case's list into both ROMs and reports the
//! first frame that differs, or nothing for a case that agrees.
//!
//! `cargo run --release --example layer3_probe -- play rom.smc 105 60 3,0`

#[path = "common/path.rs"]
mod path;

use kobo_core::Rom;
use kobo_core::expand;
use kobo_core::ram::{Ram, RamAddr};

fn level_arg(text: &str) -> u16 {
    u16::from_str_radix(text, 16).unwrap()
}

/// Moves the player along `path` (see the module docs) from `start`.
fn steer(legs: &str, frame: u32, ram: &mut Ram, start: (u16, u16)) {
    path::steer(legs, frame, ram, start, true);
}

/// What a frame left, for printing and comparing.
#[derive(Clone, PartialEq)]
struct Frame {
    line: String,
    wram: Vec<u8>,
    layer3_vram: Vec<u8>,
}

fn watched() -> Vec<(u32, usize)> {
    std::env::var("KOBO_LAYER3_WATCH")
        .ok()
        .map(|spec| {
            spec.split(';')
                .map(|r| {
                    let (a, n) = r.split_once(',').unwrap();
                    (u32::from_str_radix(a, 16).unwrap(), n.parse().unwrap())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn frames(rom: &Rom, level: u16, n: u32, path: &str) -> Result<Vec<Frame>, String> {
    frames_poked(rom, level, n, path, std::env::var("KOBO_LAYER3_POKE").ok())
}

fn frames_poked(
    rom: &Rom,
    level: u16,
    n: u32,
    path: &str,
    poke: Option<String>,
) -> Result<Vec<Frame>, String> {
    let mut start = None;
    let mut out = Vec::new();
    let watch = watched();
    expand::play_game_loop(
        rom,
        level,
        n,
        |frame, ram| {
            let s = *start.get_or_insert((
                ram.u16(RamAddr::new(0x7E_0094)),
                ram.u16(RamAddr::new(0x7E_0096)),
            ));
            steer(path, frame, ram, s);
            // KOBO_LAYER3_POKE=frame,addr,byte;...: RAM set before that frame.
            if let Some(spec) = &poke {
                for p in spec.split(';') {
                    let v: Vec<u32> = p
                        .split(',')
                        .map(|t| u32::from_str_radix(t, 16).unwrap())
                        .collect();
                    if v[0] == frame {
                        ram.set_u8(RamAddr::new(v[1]), v[2] as u8);
                    }
                }
            }
        },
        |_, played| {
            let r = played.ram;
            let b = |a: u32| r.u8(RamAddr::new(a));
            let w = |a: u32| r.u16(RamAddr::new(a));
            let mut line = format!(
                "L1 {:04X},{:04X} L2 {:04X},{:04X} L3 {:04X},{:04X} 13D5 {:02X} 1403 {:02X} 145E {:02X} {:02X} {:02X} 40 {:02X} 3E {:02X} TM/TS {:02X}/{:02X} 7FC01A {} | reg BG3 {:04X},{:04X} TM/TS {:02X}/{:02X} player {:04X},{:04X} 75 {:02X} 85 {:02X}",
                w(0x7E_001A),
                w(0x7E_001C),
                w(0x7E_001E),
                w(0x7E_0020),
                w(0x7E_0022),
                w(0x7E_0024),
                b(0x7E_13D5),
                b(0x7E_1403),
                b(0x7E_145E),
                b(0x7E_145F),
                b(0x7E_1460),
                b(0x7E_0040),
                b(0x7E_003E),
                b(0x7E_0D9D),
                b(0x7E_0D9E),
                hex(&r.bytes(RamAddr::new(0x7F_C01A), 6)),
                played.bg_scroll[2][0],
                played.bg_scroll[2][1],
                played.screen_layers[0],
                played.screen_layers[1],
                w(0x7E_0094),
                w(0x7E_0096),
                b(0x7E_0075),
                b(0x7E_0085),
            );
            for &(a, len) in &watch {
                let bytes = kobo_core::clean_room::bytes(r, RamAddr::new(a), len);
                line += &format!(" {a:06X}: {}", hex(&bytes));
            }
            out.push(Frame {
                line,
                wram: kobo_core::clean_room::bytes(r, RamAddr::new(0x7E_0000), 0x2000),
                layer3_vram: played.vram[0x8000..0xC000].to_vec(),
            });
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn runs(list: &[usize]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() && out.len() < 16 {
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

/// What differs between two frames, or `None`.
fn differs(a: &Frame, b: &Frame) -> Option<String> {
    // KOBO_LAYER3_LINE_ONLY=1: only layer 3's RAM and registers, for ROMs that differ
    // elsewhere (a hack against a build of it).
    if std::env::var_os("KOBO_LAYER3_LINE_ONLY").is_some() {
        return (a.line != b.line).then(|| format!("\n  a {}\n  b {}", a.line, b.line));
    }
    let ram: Vec<usize> = (0..a.wram.len())
        .filter(|&i| a.wram[i] != b.wram[i] && !(0x100..0x200).contains(&i))
        .collect();
    let vram: Vec<usize> = (0..a.layer3_vram.len() / 2)
        .filter(|&i| a.layer3_vram[i * 2..i * 2 + 2] != b.layer3_vram[i * 2..i * 2 + 2])
        .map(|i| i + 0x4000)
        .collect();
    if a.line == b.line && ram.is_empty() && vram.is_empty() {
        return None;
    }
    let mut out = String::new();
    if a.line != b.line {
        out += &format!("\n  a {}\n  b {}", a.line, b.line);
    }
    if !ram.is_empty() {
        out += &format!("\n  ram {}", runs(&ram));
    }
    if !vram.is_empty() {
        out += &format!("\n  vram {}", runs(&vram));
    }
    Some(out)
}

const SLOTS: [&str; 16] = [
    "AN2", "LT3", "BG3", "BG2", "FG3", "BG1", "FG2", "FG1", "SP4", "SP3", "SP2", "SP1", "LG4",
    "LG3", "LG2", "LG1",
];

/// `rom` with `level`'s list changed by `settings`.
fn with_settings(rom: &Rom, level: u16, settings: &[&str]) -> Rom {
    use kobo_core::addr::SnesAddr;
    let mut out = Rom::from_bytes(rom.data().to_vec()).unwrap();
    let list = rom.read_u24(SnesAddr::new(0x0F_F7FF)).unwrap() + level as u32 * 32;
    for s in settings {
        let (slot, value, whole) = match s.split_once('=') {
            Some((k, v)) => (k, v, true),
            None => {
                let (k, v) = s.split_once(':').unwrap();
                (k, v, false)
            }
        };
        let i = SLOTS.iter().position(|n| n == &slot).unwrap() as u32;
        let at = SnesAddr::new(list + i * 2);
        let v = u16::from_str_radix(value, 16).unwrap();
        let word = if whole {
            v
        } else {
            (out.read_u16(at).unwrap() & 0x0FFF) | v << 12
        };
        out.write_u16(at, word).unwrap();
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("play") => {
            let rom = Rom::load(&args[1]).unwrap();
            let n = args[3].parse().unwrap();
            let path = args.get(4).map(String::as_str).unwrap_or("0,0");
            for (f, frame) in frames(&rom, level_arg(&args[2]), n, path)
                .unwrap()
                .iter()
                .enumerate()
            {
                println!("{f:3} {}", frame.line);
            }
        }
        Some("compare") => {
            let (a, b) = (Rom::load(&args[1]).unwrap(), Rom::load(&args[2]).unwrap());
            let level = level_arg(&args[3]);
            let n = args[4].parse().unwrap();
            let path = args.get(5).map(String::as_str).unwrap_or("0,0");
            // POKE_B: pokes for the second ROM only.
            let fb = match std::env::var("KOBO_LAYER3_POKE_B") {
                Ok(p) => frames_poked(&b, level, n, path, Some(p)),
                Err(_) => frames(&b, level, n, path),
            };
            let (fa, fb) = (frames(&a, level, n, path), fb);
            let (fa, fb) = match (fa, fb) {
                (Ok(x), Ok(y)) => (x, y),
                (x, y) => {
                    println!("failed: {:?} {:?}", x.err(), y.err());
                    return;
                }
            };
            let mut shown = 0;
            for f in 0..fa.len().min(fb.len()) {
                if let Some(d) = differs(&fa[f], &fb[f]) {
                    println!("frame {f}:{d}");
                    shown += 1;
                    if shown >= 8 && std::env::var_os("KOBO_LAYER3_ALL").is_none() {
                        break;
                    }
                }
            }
            if shown == 0 {
                println!("same over {} frames", fa.len());
            }
        }
        Some("loads") => {
            // loads a b frames path [levels...]: the levels whose frames differ.
            let (a, b) = (Rom::load(&args[1]).unwrap(), Rom::load(&args[2]).unwrap());
            let n: u32 = args[3].parse().unwrap();
            let path = args[4].clone();
            let levels: Vec<u16> = if args.len() > 5 {
                args[5..].iter().map(|l| level_arg(l)).collect()
            } else {
                (0..0x200).collect()
            };
            let mut same = 0;
            for level in levels {
                match (frames(&a, level, n, &path), frames(&b, level, n, &path)) {
                    (Ok(fa), Ok(fb)) => {
                        let first = (0..fa.len().min(fb.len()))
                            .find_map(|f| differs(&fa[f], &fb[f]).map(|d| (f, d)));
                        match first {
                            None => same += 1,
                            Some((f, d)) => println!("{level:03X}: frame {f}:{d}"),
                        }
                    }
                    (x, y) => println!(
                        "{level:03X}: failed {:?} {:?}",
                        x.err().unwrap_or_default(),
                        y.err().unwrap_or_default()
                    ),
                }
            }
            println!("{same} levels the same");
        }
        Some("sweep") => {
            use std::io::BufRead;
            let (a, b) = (Rom::load(&args[1]).unwrap(), Rom::load(&args[2]).unwrap());
            let n: u32 = args[3].parse().unwrap();
            let (mut cases, mut same) = (0, 0);
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.len() < 2 || words[0].starts_with('#') {
                    continue;
                }
                let level = level_arg(words[0]);
                let path = words[1];
                let (ra, rb) = (
                    with_settings(&a, level, &words[2..]),
                    with_settings(&b, level, &words[2..]),
                );
                cases += 1;
                match (frames(&ra, level, n, path), frames(&rb, level, n, path)) {
                    (Ok(fa), Ok(fb)) => {
                        let first = (0..fa.len().min(fb.len()))
                            .find_map(|f| differs(&fa[f], &fb[f]).map(|d| (f, d)));
                        match first {
                            None => same += 1,
                            Some((f, d)) => println!("{line}: frame {f}:{d}"),
                        }
                    }
                    (x, y) => println!(
                        "{line}: failed {:?} {:?}",
                        x.err().unwrap_or_default(),
                        y.err().unwrap_or_default()
                    ),
                }
            }
            println!("{same} of {cases} cases the same");
        }
        _ => {
            eprintln!("usage: layer3_probe play|compare|loads|sweep ... (see the source)");
            std::process::exit(2)
        }
    }
}
