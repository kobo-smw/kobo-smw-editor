//! Which sprites a ROM's own loader spawns, and where, as the camera moves
//! through a level (`expand::play_level_entered`): for learning, from memory
//! effects alone, what Lunar Magic's sprite loader does with a list, and
//! checking Kobo's against it.
//!
//! `sprite_probe play rom.sfc level [option...]` prints, for every frame in
//! which something changed, each occupied sprite slot (number, status,
//! position, load index), the load flags set, and the camera.
//! `sprite_probe spawns rom.sfc level [option...]` prints the frame each
//! entry first loaded, with the camera then. `sprite_probe compare a.sfc
//! b.sfc level [option...]` plays both and prints the frames where their
//! slots, load flags, camera, or the RAM named with `watch=` differ, up to
//! 20. The frame counter `$13` is counted on every frame, as the game loop
//! counts it. `KOBO_PROBE_SAVE=path` writes the ROM played;
//! `KOBO_PROBE_DUMP=frame:path` the work RAM at a frame.
//!
//! Options:
//! - `list=SPEC` (or `list=@file`): the level's sprite list, written into a
//!   copy of the ROM (expanded to 2 MiB, at `$3F8000` through the pointer at
//!   `$05EC00` and the bank at `$0EF100`, or in bank `$07` without that
//!   table): `new` for the new sprite system, `mem=N`, then sprites
//!   `id:x:y[:extra[:ext-bytes]]` (hex id, decimal tiles), or `raw:HEX` for
//!   bytes after the header as they are.
//! - `path=X0:DX[:Y0:DY]`: carries the player from pixel (X0, Y0) by (DX,
//!   DY) a frame (decimal), keeping it invulnerable; default `48:3`.
//!   `route=F:X:Y/F:X:Y/...` instead moves it in straight lines between
//!   points at frames F.
//! - `clear`: empties every sprite slot before each frame, so that nothing
//!   waits for a free slot.
//! - `frames=N` (default 600), `rom:ADDR=VV` (hex, a ROM byte written
//!   first), `ram:ADDR=VV` (a RAM byte set on entry), `hold:ADDR=VV` (set on
//!   every frame), `watch=ADDR[-END]` (RAM to print or compare).
//!
//! Clean room: a ROM Lunar Magic saved holds its code; this reads memory
//! only, and never with `KOBO_CPU_TRACE`.

use kobo_core::source::level::{Sprite, Sprites};
use kobo_core::sprites::{self, SpriteHeader};
use kobo_core::{Rom, SnesAddr, expand, level, ram};

const LIST_AT: u32 = 0x3F_8000;

#[derive(Default)]
struct Options {
    list: Option<String>,
    path: (i32, i32, Option<(i32, i32)>),
    route: Vec<(u32, i32, i32)>,
    frames: u32,
    rom: Vec<(u32, u8)>,
    ram: Vec<(u32, u8)>,
    hold: Vec<(u32, u8)>,
    watch: Vec<(u32, u32)>,
    clear: bool,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches('$'), 16).unwrap()
}

fn options(args: &[String]) -> Options {
    let mut o = Options {
        path: (48, 3, None),
        frames: 600,
        ..Default::default()
    };
    let poke = |s: &str| {
        let (a, v) = s.split_once('=').unwrap();
        (hex(a), hex(v) as u8)
    };
    for a in args {
        if let Some(v) = a.strip_prefix("list=@") {
            o.list = Some(std::fs::read_to_string(v).unwrap().trim().to_owned());
        } else if let Some(v) = a.strip_prefix("list=") {
            o.list = Some(v.to_owned());
        } else if let Some(v) = a.strip_prefix("route=") {
            o.route = v
                .split('/')
                .map(|p| {
                    let n: Vec<i32> = p.split(':').map(|x| x.parse().unwrap()).collect();
                    (n[0] as u32, n[1], n[2])
                })
                .collect();
        } else if let Some(v) = a.strip_prefix("path=") {
            let n: Vec<i32> = v.split(':').map(|x| x.parse().unwrap()).collect();
            o.path = (n[0], n[1], (n.len() == 4).then(|| (n[2], n[3])));
        } else if let Some(v) = a.strip_prefix("frames=") {
            o.frames = v.parse().unwrap();
        } else if let Some(v) = a.strip_prefix("rom:") {
            o.rom.push(poke(v));
        } else if let Some(v) = a.strip_prefix("ram:") {
            o.ram.push(poke(v));
        } else if let Some(v) = a.strip_prefix("hold:") {
            o.hold.push(poke(v));
        } else if a == "clear" {
            o.clear = true;
        } else if let Some(v) = a.strip_prefix("watch=") {
            let (s, e) = v.split_once('-').unwrap_or((v, v));
            o.watch.push((hex(s), hex(e)));
        } else {
            panic!("unknown option {a}");
        }
    }
    o
}

/// The list's bytes, header first.
fn list_bytes(spec: &str, level_header: u8, vertical: bool) -> Vec<u8> {
    let mut new = false;
    let mut memory = level_header & 0x1F;
    let mut list = Vec::new();
    let mut raw = None;
    for item in spec.split(',').filter(|s| !s.is_empty()) {
        if item == "new" {
            new = true;
        } else if let Some(m) = item.strip_prefix("mem=") {
            memory = m.parse().unwrap();
        } else if let Some(r) = item.strip_prefix("raw:") {
            raw = Some(
                (0..r.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&r[i..i + 2], 16).unwrap())
                    .collect::<Vec<u8>>(),
            );
        } else {
            let f: Vec<&str> = item.split(':').collect();
            list.push(Sprite {
                id: hex(f[0]) as u8,
                x: f[1].parse().unwrap(),
                y: f[2].parse().unwrap(),
                extra_bits: f.get(3).map_or(0, |e| e.parse().unwrap()),
                extension: f.get(4).map_or(Vec::new(), |e| {
                    (0..e.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&e[i..i + 2], 16).unwrap())
                        .collect()
                }),
            });
        }
    }
    let header = SpriteHeader {
        memory,
        buoyancy: level_header & 0x80 != 0,
        buoyancy_no_layer2: level_header & 0x40 != 0,
        new_sprite_system: new,
    };
    if let Some(mut raw) = raw {
        raw.insert(0, header.to_byte());
        return raw;
    }
    let sprites = Sprites {
        memory,
        buoyancy: header.buoyancy,
        buoyancy_no_layer2: header.buoyancy_no_layer2,
        list,
    };
    let (header, entries) = sprites.to_entries(vertical, new);
    // Extension bytes go as given, whatever the ROM's size table says.
    let mut out = sprites::encode(
        header,
        &entries
            .iter()
            .map(|e| sprites::SpriteEntry {
                extension: Vec::new(),
                ..e.clone()
            })
            .collect::<Vec<_>>(),
        None,
    )
    .unwrap();
    if entries.iter().any(|e| !e.extension.is_empty()) {
        // Re-encode by hand to put the bytes in.
        let mut with = vec![out[0]];
        let mut at = 1;
        for e in &entries {
            while out[at] == 0xFF && new && out[at + 1] != 0xFF {
                with.extend(&out[at..at + 2]);
                at += 2;
            }
            if out[at] == 0xFF && new {
                with.push(0xFF);
                at += 1;
            }
            with.extend(&out[at..at + 3]);
            with.extend(&e.extension);
            at += 3;
        }
        with.extend(&out[at..]);
        out = with;
    }
    out
}

fn prepare(path: &str, level: u16, o: &Options) -> Rom {
    let mut rom = Rom::load(path).unwrap();
    if rom.len() < 0x20_0000 {
        rom.expand(0x20_0000).unwrap();
    }
    for &(a, v) in &o.rom {
        rom.write_u8(SnesAddr::new(a), v).unwrap();
    }
    if let Some(spec) = &o.list {
        let at = level::sprite_ptr(&rom, level).unwrap();
        let header = rom.read_u8(at).unwrap();
        let data = level::read_objects(&rom, level).unwrap();
        let vertical = data.header().level_mode.layer1_vertical();
        let bytes = list_bytes(spec, header, vertical);
        // Without Lunar Magic's bank table, in bank $07's unused space.
        let list_at = if level::LevelFormat::of(&rom).lunar_magic {
            LIST_AT
        } else {
            0x07_E76F
        };
        rom.write(SnesAddr::new(list_at), &bytes).unwrap();
        rom.write_u16(
            level::tables::SPRITE_PTRS.add(2 * level as u32),
            (list_at & 0xFFFF) as u16,
        )
        .unwrap();
        if list_at == LIST_AT {
            rom.write_u8(
                level::tables::SPRITE_BANKS.add(level as u32),
                (LIST_AT >> 16) as u8,
            )
            .unwrap();
        }
    }
    rom.fix_checksum().unwrap();
    if let Some(path) = std::env::var_os("KOBO_PROBE_SAVE") {
        rom.save(path).unwrap();
    }
    rom
}

/// What one frame leaves: a line per slot in use, then the load flags and
/// watched RAM.
fn frame_state(ram: &ram::Ram, o: &Options) -> Vec<String> {
    let slot = |t: u32, i: u32| ram.u8_at(ram::RamAddr::new(0x7E_0000 | t), i);
    let mut out = Vec::new();
    for i in 0..12 {
        let status = slot(0x14C8, i);
        if status == 0 {
            continue;
        }
        let x = slot(0xE4, i) as u16 | (slot(0x14E0, i) as u16) << 8;
        let y = slot(0xD8, i) as u16 | (slot(0x14D4, i) as u16) << 8;
        out.push(format!(
            "slot {i:2}: {:02X} st {status:02X} at {x:04X},{y:04X} idx {:02X}",
            slot(0x9E, i),
            slot(0x161A, i),
        ));
    }
    let flags: Vec<String> = (0..0x80)
        .filter(|&i| ram.peek(0x7E_1938 + i) != 0)
        .map(|i| format!("{i:02X}"))
        .collect();
    out.push(format!("flags {}", flags.join(" ")));
    let word = |a: u32| ram.peek(a) as u16 | (ram.peek(a + 1) as u16) << 8;
    out.push(format!(
        "cam {:04X},{:04X}",
        word(0x7E_001A),
        word(0x7E_001C)
    ));
    let lm: Vec<String> = (0..0x100)
        .filter(|&i| ram.peek(0x7F_AF00 + i) != 0)
        .map(|i| format!("{i:02X}"))
        .collect();
    if !lm.is_empty() {
        out.push(format!("7FAF00 {}", lm.join(" ")));
    }
    for &(s, e) in &o.watch {
        let bytes: Vec<String> = (s..=e)
            .map(|a| format!("{:02X}", kobo_core::clean_room::peek(ram, a)))
            .collect();
        out.push(format!("{s:06X}: {}", bytes.join(" ")));
    }
    out
}

/// Where a route puts the player at `frame`: straight lines between its
/// points, `frame:x:y` each, and the last point after it.
fn route_at(route: &[(u32, i32, i32)], frame: u32) -> Option<(u16, u16)> {
    let last = route.last()?;
    let (x, y) = match route.windows(2).find(|w| frame < w[1].0) {
        Some(w) if frame >= w[0].0 => {
            let t = (frame - w[0].0) as i32;
            let n = (w[1].0 - w[0].0) as i32;
            (
                w[0].1 + (w[1].1 - w[0].1) * t / n,
                w[0].2 + (w[1].2 - w[0].2) * t / n,
            )
        }
        Some(w) => (w[0].1, w[0].2),
        None => (last.1, last.2),
    };
    Some((x.max(0) as u16, y.max(0) as u16))
}

fn play(path: &str, level: u16, o: &Options) -> Vec<Vec<String>> {
    let rom = prepare(path, level, o);
    let mut frames = Vec::new();
    let (x0, dx, y) = o.path;
    let mut last = None;
    expand::play_level_entered(
        &rom,
        level,
        |ram| {
            for &(a, v) in &o.ram {
                ram.poke(a, v);
            }
        },
        o.frames,
        |frame, ram| {
            if let Some(prev) = last.take() {
                frames.push(prev);
            }
            last = Some(frame_state(ram, o));
            if let Ok(spec) = std::env::var("KOBO_PROBE_DUMP")
                && let Some((n, path)) = spec.split_once(':')
                && n.parse() == Ok(frame)
            {
                let wram =
                    kobo_core::clean_room::bytes(ram, ram::RamAddr::new(0x7E_0000), 0x2_0000);
                std::fs::write(path, wram).unwrap();
            }
            let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
            let x = (x0 + dx * frame as i32).max(0) as u16;
            ram.set_u8(at(0x94), x as u8);
            ram.set_u8(at(0x95), (x >> 8) as u8);
            if let Some((x, y)) = route_at(&o.route, frame) {
                ram.set_u8(at(0x94), x as u8);
                ram.set_u8(at(0x95), (x >> 8) as u8);
                ram.set_u8(at(0x96), y as u8);
                ram.set_u8(at(0x97), (y >> 8) as u8);
            } else if let Some((y0, dy)) = y {
                let y = (y0 + dy * frame as i32).max(0) as u16;
                ram.set_u8(at(0x96), y as u8);
                ram.set_u8(at(0x97), (y >> 8) as u8);
            }
            ram.set_u8(at(0x1497), 0x7F);
            // The game loop counts frames in $13, which play_level leaves
            // alone; the loader and the despawn checks alternate on it.
            let true_frame = ram.u8(at(0x13));
            ram.set_u8(at(0x13), true_frame.wrapping_add(1));
            for &(a, v) in &o.hold {
                ram.poke(a, v);
            }
            if o.clear {
                for i in 0..12 {
                    ram.poke(0x7E_14C8 + i, 0);
                }
            }
        },
    )
    .unwrap();
    if let Some(prev) = last {
        frames.push(prev);
    }
    frames
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("play") => {
            let level = hex(&args[2]) as u16;
            let o = options(&args[3..]);
            let frames = play(&args[1], level, &o);
            let mut prev: Option<&Vec<String>> = None;
            for (n, state) in frames.iter().enumerate() {
                if prev != Some(state) {
                    println!("frame {n}");
                    for line in state {
                        println!("  {line}");
                    }
                }
                prev = Some(state);
            }
        }
        Some("spawns") => {
            // The frame each list index was first loaded, with the camera
            // then.
            let level = hex(&args[2]) as u16;
            let o = options(&args[3..]);
            let frames = play(&args[1], level, &o);
            let mut seen = std::collections::BTreeMap::new();
            for (n, state) in frames.iter().enumerate() {
                let cam = state.iter().find(|l| l.starts_with("cam ")).unwrap();
                let flags = state.iter().find(|l| l.starts_with("flags")).unwrap();
                for f in flags.split(' ').skip(1).filter(|f| !f.is_empty()) {
                    seen.entry(f.to_owned())
                        .or_insert_with(|| format!("frame {n} {cam}"));
                }
            }
            for (index, when) in seen {
                println!("{index}: {when}");
            }
        }
        Some("compare") => {
            let level = hex(&args[3]) as u16;
            let o = options(&args[4..]);
            let a = play(&args[1], level, &o);
            let b = play(&args[2], level, &o);
            let mut shown = 0;
            for (n, (x, y)) in a.iter().zip(&b).enumerate() {
                if x != y {
                    println!("frame {n}");
                    for line in x.iter().filter(|l| !y.contains(l)) {
                        println!("  a {line}");
                    }
                    for line in y.iter().filter(|l| !x.contains(l)) {
                        println!("  b {line}");
                    }
                    shown += 1;
                    if shown == 20 {
                        break;
                    }
                }
            }
            if shown == 0 {
                println!("the same over {} frames", a.len());
            }
        }
        _ => eprintln!("usage: sprite_probe play|compare ... (see the source)"),
    }
}
