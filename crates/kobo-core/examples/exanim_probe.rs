//! What ExAnimation does frame by frame, found from memory effects only
//! (docs/lunar-magic-install.md, "ExAnimation").
//!
//! `run rom level frames`: plays the level through the game loop and its NMI
//! and prints, per frame, the VRAM words and CGRAM colours the frame changed
//! and the ExAnimation RAM (`$7FC000`-`$7FC0FF`) that changed.
//!
//! `ab a b level frames`: plays the level in both ROMs and prints the frames
//! whose VRAM, CGRAM, or ExAnimation RAM differ.
//!
//! Chosen data, written into a copy of the ROM in memory (never saved):
//! `ANIM=hex` the level's ExAnimation data, `GLOBAL=hex` the global data
//! (`GLOBAL=none` for none), `SETTINGS=xx` the level's byte of `$03FE00`;
//! `SET=7E14AD=20@5-9;7FC0FC=0200@0` writes RAM before the frames given (one
//! byte per two hex digits, from the address up; `@n` from frame `n` on).
//! `FILE=hex` puts bytes at `$7EAD00` (the AN2 buffer) before the first frame.
//! `VWORDS=lo-hi` prints the VRAM words a frame changed there, with their
//! values (`VWORDS_ALL` every one of them); `ab` prints the ExAnimation RAM's
//! values in both ROMs.
//!
//! `cargo run --release --example exanim_probe -- run rom.smc 105 64`

use kobo_core::addr::SnesAddr;
use kobo_core::ram::{Ram, RamAddr};
use kobo_core::{Rom, expand};

fn hex_bytes(text: &str) -> Vec<u8> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (0..t.len() / 2)
        .map(|i| u8::from_str_radix(&t[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

/// Where Lunar Magic's ExAnimation keeps its pointers: the target of the
/// `JSL` at `$0583AD`, whose `+$EA` is the level table's address, and
/// `+$5B`/`+$65` the global data's bank (times `$100`) and low word.
fn hook_target(rom: &Rom) -> Option<u32> {
    (rom.read_u8(SnesAddr::new(0x0583AD)).ok()? == 0x22)
        .then(|| rom.read_u24(SnesAddr::new(0x0583AE)).unwrap())
}

fn prepare(mut rom: Rom, level: u16) -> Rom {
    let mut free = 0x20_8000u32;
    if ["ANIM", "GLOBAL", "PALETTE"]
        .iter()
        .any(|v| std::env::var_os(v).is_some())
        && rom.len() < 0x10_0000 * 2
    {
        rom.expand(0x20_0000).unwrap();
    }
    let mut place = |rom: &mut Rom, bytes: &[u8]| {
        let at = free;
        rom.write(SnesAddr::new(at), bytes).unwrap();
        free += (bytes.len() as u32 + 0xFF) & !0xFF;
        at
    };
    if let Ok(anim) = std::env::var("ANIM") {
        let t = hook_target(&rom).expect("no ExAnimation hook at $0583AD");
        let table = rom.read_u24(SnesAddr::new(t + 0xEA)).unwrap();
        let at = if anim == "none" {
            0
        } else {
            place(&mut rom, &hex_bytes(&anim))
        };
        rom.write_u24(SnesAddr::new(table + level as u32 * 3), at)
            .unwrap();
    }
    if let Ok(global) = std::env::var("GLOBAL") {
        let t = hook_target(&rom).expect("no ExAnimation hook at $0583AD");
        let at = if global == "none" {
            0
        } else {
            place(&mut rom, &hex_bytes(&global))
        };
        rom.write_u16(SnesAddr::new(t + 0x5B), (at >> 8) as u16 & 0xFF00)
            .unwrap();
        rom.write_u16(SnesAddr::new(t + 0x65), at as u16).unwrap();
    }
    // PALETTE=hex: a custom palette for the level ($0EF600: back area colour,
    // then 256 colours), for ROMs with Lunar Magic's.
    if let Ok(palette) = std::env::var("PALETTE") {
        let at = place(&mut rom, &hex_bytes(&palette));
        rom.write_u24(SnesAddr::new(0x0EF600 + level as u32 * 3), at)
            .unwrap();
    }
    if let Ok(s) = std::env::var("SETTINGS") {
        rom.write_u8(
            SnesAddr::new(0x03FE00 + level as u32),
            u8::from_str_radix(&s, 16).unwrap(),
        )
        .unwrap();
    }
    rom
}

struct Set {
    addr: u32,
    bytes: Vec<u8>,
    from: u32,
    to: u32,
}

fn sets() -> Vec<Set> {
    let Ok(spec) = std::env::var("SET") else {
        return Vec::new();
    };
    spec.split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let (lhs, when) = s.split_once('@').unwrap_or((s, "0"));
            let (a, v) = lhs.split_once('=').unwrap();
            let (from, to) = match when.split_once('-') {
                Some((x, y)) => (x.parse().unwrap(), y.parse().unwrap()),
                None => (when.parse().unwrap(), u32::MAX),
            };
            Set {
                addr: u32::from_str_radix(a, 16).unwrap(),
                bytes: hex_bytes(v),
                from,
                to,
            }
        })
        .collect()
}

fn apply(sets: &[Set], frame: u32, ram: &mut Ram) {
    for s in sets.iter().filter(|s| (s.from..=s.to).contains(&frame)) {
        for (i, b) in s.bytes.iter().enumerate() {
            ram.set_u8(RamAddr::new(s.addr + i as u32), *b);
        }
    }
    if frame == 0
        && let Ok(file) = std::env::var("FILE")
    {
        for (i, b) in hex_bytes(&file).iter().enumerate() {
            ram.set_u8(RamAddr::new(0x7E_AD00 + i as u32), *b);
        }
    }
}

/// A frame's state: VRAM, CGRAM, `$7FC000`-`$7FC0FF`, and `$14`.
#[derive(Clone)]
struct State {
    vram: Vec<u8>,
    cgram: Vec<u8>,
    exram: Vec<u8>,
    wram: Vec<u8>,
    frame: u8,
}

fn play(rom: &Rom, level: u16, frames: u32) -> (State, Vec<State>) {
    let sets = sets();
    let mut out = Vec::new();
    let mut first = None;
    let state = |played: &expand::PlayedFrame| State {
        vram: played.vram.to_vec(),
        cgram: played.cgram.to_vec(),
        exram: played.ram.bytes(RamAddr::new(0x7F_C000), 0x100),
        wram: kobo_core::clean_room::bytes(played.ram, RamAddr::new(0x7E_0000), 0x2000),
        frame: played.ram.u8(RamAddr::new(0x7E_0014)),
    };
    expand::play_game_loop_from(
        rom,
        level,
        frames,
        |ram| {
            // ENTRY=7FC0FC=0100;...: RAM before the level's load.
            if let Ok(spec) = std::env::var("ENTRY") {
                for item in spec.split(';').filter(|s| !s.is_empty()) {
                    let (a, v) = item.split_once('=').unwrap();
                    let a = u32::from_str_radix(a, 16).unwrap();
                    for (i, b) in hex_bytes(v).iter().enumerate() {
                        ram.set_u8(RamAddr::new(a + i as u32), *b);
                    }
                }
            }
        },
        |played| first = Some(state(&played)),
        |frame, ram| apply(&sets, frame, ram),
        |_, played| out.push(state(&played)),
    )
    .unwrap();
    (first.unwrap(), out)
}

fn runs(list: &[usize]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
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
    out.join(" ")
}

/// Changed words as runs of `vram <= values` where both count up by one,
/// so a pattern of word numbers at the source shows where each came from.
fn value_runs(words: &[usize], vram: &[u8]) -> String {
    let value = |w: usize| u16::from_le_bytes([vram[w * 2], vram[w * 2 + 1]]);
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let start = i;
        while i + 1 < words.len()
            && words[i + 1] == words[i] + 1
            && value(words[i + 1]) == value(words[i]).wrapping_add(1)
        {
            i += 1;
        }
        let (a, b) = (words[start], words[i]);
        out.push(if a == b {
            format!("{a:04X}={:04X}", value(a))
        } else {
            format!("{a:04X}-{b:04X}={:04X}-{:04X}", value(a), value(b))
        });
        i += 1;
    }
    out.join(" ")
}

fn changed_words(a: &[u8], b: &[u8]) -> Vec<usize> {
    (0..a.len() / 2)
        .filter(|w| a[w * 2..w * 2 + 2] != b[w * 2..w * 2 + 2])
        .collect()
}

fn run(path: &str, level: u16, frames: u32) {
    let rom = prepare(Rom::load(path).unwrap(), level);
    let (first, states) = play(&rom, level, frames);
    // COLOURS=lo-hi: CGRAM colours before the first frame.
    if let Ok(r) = std::env::var("COLOURS") {
        let (lo, hi) = r.split_once('-').unwrap();
        let c: Vec<String> = (usize::from_str_radix(lo, 16).unwrap()
            ..=usize::from_str_radix(hi, 16).unwrap())
            .map(|c| {
                format!(
                    "{c:02X}:{:04X}",
                    u16::from_le_bytes([first.cgram[c * 2], first.cgram[c * 2 + 1]])
                )
            })
            .collect();
        println!("cgram before frame 0: {}", c.join(" "));
    }
    // DUMP=lo-hi: work RAM ($0000-$1FFF) before the first frame.
    if let Ok(r) = std::env::var("DUMP") {
        let (lo, hi) = r.split_once('-').unwrap();
        let (lo, hi) = (
            usize::from_str_radix(lo, 16).unwrap(),
            usize::from_str_radix(hi, 16).unwrap(),
        );
        let b: Vec<String> = first.wram[lo..=hi]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        println!("wram {lo:04X}: {}", b.join(" "));
    }
    let initial: Vec<String> = (0..0x100)
        .filter(|&i| first.exram[i] != 0)
        .map(|i| format!("{:02X}={:02X}", i, first.exram[i]))
        .collect();
    println!("7FC0xx before frame 0: {}", initial.join(" "));
    let mut prev = first;
    for (f, s) in states.iter().enumerate() {
        let vram = changed_words(&prev.vram, &s.vram);
        let colours: Vec<String> = changed_words(&prev.cgram, &s.cgram)
            .iter()
            .map(|&c| {
                format!(
                    "{c:02X}:{:04X}",
                    u16::from_le_bytes([s.cgram[c * 2], s.cgram[c * 2 + 1]])
                )
            })
            .collect();
        let ex: Vec<String> = (0..0x100)
            .filter(|&i| prev.exram[i] != s.exram[i])
            .map(|i| format!("{:02X}={:02X}", i, s.exram[i]))
            .collect();
        let mut line = format!("frame {f:3} $14={:02X}", s.frame);
        if !vram.is_empty() {
            line += &format!(" | vram {}", runs(&vram));
            if std::env::var_os("WORDS").is_some() {
                line += &format!(" [{}]", value_runs(&vram, &s.vram));
            }
        }
        if !colours.is_empty() {
            line += &format!(" | cgram {}", colours.join(" "));
        }
        if !ex.is_empty() {
            line += &format!(" | 7FC0 {}", ex.join(" "));
        }
        // VWORDS=lo-hi: the values of the VRAM words the frame changed there
        // (with VWORDS_ALL, all of them).
        if let Ok(r) = std::env::var("VWORDS") {
            let (lo, hi) = r.split_once('-').unwrap();
            let (lo, hi) = (
                usize::from_str_radix(lo, 16).unwrap(),
                usize::from_str_radix(hi, 16).unwrap(),
            );
            let w: Vec<String> = (lo..=hi)
                .filter(|&i| {
                    std::env::var_os("VWORDS_ALL").is_some()
                        || prev.vram[2 * i..2 * i + 2] != s.vram[2 * i..2 * i + 2]
                })
                .map(|i| format!("{i:04X}={:02X}{:02X}", s.vram[2 * i + 1], s.vram[2 * i]))
                .collect();
            if !w.is_empty() {
                line += &format!(" | words {}", w.join(" "));
            }
        }
        // WATCH=lo-hi: work RAM ($0000-$1FFF) bytes the frame changed there.
        if let Ok(r) = std::env::var("WATCH") {
            let (lo, hi) = r.split_once('-').unwrap();
            let (lo, hi) = (
                usize::from_str_radix(lo, 16).unwrap(),
                usize::from_str_radix(hi, 16).unwrap(),
            );
            let w: Vec<String> = (lo..=hi)
                .filter(|&i| prev.wram[i] != s.wram[i])
                .map(|i| format!("{i:04X}={:02X}", s.wram[i]))
                .collect();
            if !w.is_empty() {
                line += &format!(" | wram {}", w.join(" "));
            }
        }
        println!("{line}");
        prev = s.clone();
    }
}

fn ab(a: &str, b: &str, level: u16, frames: u32) {
    let ra = prepare(Rom::load(a).unwrap(), level);
    let rb = prepare(Rom::load(b).unwrap(), level);
    let (fa, sa) = play(&ra, level, frames);
    let (fb, sb) = play(&rb, level, frames);
    // EXRAM=lo-hi,...: the bytes of $7FC000-$7FC0FF compared.
    let ex_range: Vec<usize> = match std::env::var("EXRAM") {
        Ok(r) => r
            .split(',')
            .flat_map(|r| {
                let (x, y) = r.split_once('-').unwrap();
                usize::from_str_radix(x, 16).unwrap()..=usize::from_str_radix(y, 16).unwrap()
            })
            .collect(),
        Err(_) => (0x70..0x100).collect(),
    };
    // VRAM_SKIP=lo-hi,...: word ranges left out (the player's tiles, say).
    let skip: Vec<(usize, usize)> = std::env::var("VRAM_SKIP")
        .map(|s| {
            s.split(',')
                .map(|r| {
                    let (lo, hi) = r.split_once('-').unwrap();
                    (
                        usize::from_str_radix(lo, 16).unwrap(),
                        usize::from_str_radix(hi, 16).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let report = |f: &str, x: &State, y: &State| {
        let vram: Vec<usize> = changed_words(&x.vram, &y.vram)
            .into_iter()
            .filter(|w| !skip.iter().any(|&(lo, hi)| (lo..=hi).contains(w)))
            .collect();
        let cgram = changed_words(&x.cgram, &y.cgram);
        let ex: Vec<usize> = ex_range
            .iter()
            .copied()
            .filter(|&i| x.exram[i] != y.exram[i])
            .collect();
        // AB_WRAM=lo-hi: work RAM $0000-$1FFF compared as well, in that range.
        let wram: Vec<usize> = match std::env::var("AB_WRAM") {
            Ok(r) => {
                let (lo, hi) = r.split_once('-').unwrap();
                (usize::from_str_radix(lo, 16).unwrap()..=usize::from_str_radix(hi, 16).unwrap())
                    .filter(|&i| x.wram[i] != y.wram[i])
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        if vram.is_empty() && cgram.is_empty() && ex.is_empty() && wram.is_empty() {
            return false;
        }
        // The ExAnimation RAM's values, as the first ROM and the second.
        let values: Vec<String> = ex
            .iter()
            .take(8)
            .map(|&i| format!("{i:02X}:{:02X}>{:02X}", x.exram[i], y.exram[i]))
            .collect();
        println!(
            "{f}: vram {} | cgram {} | 7FC0xx {} ({}) | wram {}",
            runs(&vram),
            runs(&cgram),
            runs(&ex),
            values.join(" "),
            runs(&wram)
        );
        true
    };
    let mut bad = report("before", &fa, &fb) as usize;
    for f in 0..sa.len().min(sb.len()) {
        bad += report(&format!("frame {f}"), &sa[f], &sb[f]) as usize;
    }
    println!("{bad} frames differ");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let level = |s: &str| u16::from_str_radix(s, 16).unwrap();
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1], level(&args[2]), args[3].parse().unwrap()),
        Some("ab") => ab(
            &args[1],
            &args[2],
            level(&args[3]),
            args[4].parse().unwrap(),
        ),
        _ => eprintln!("exanim_probe run rom level frames | ab a b level frames"),
    }
}
