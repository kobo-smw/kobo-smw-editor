//! What a level's size setting does, as a ROM's own code has it: for
//! learning from memory effects what Lunar Magic's taller levels piece
//! does, and checking Kobo's against it (docs/lunar-magic-install.md,
//! "Taller levels").
//!
//! The level size table is found where Lunar Magic's layout keeps it,
//! `$240` bytes before the code the `JSL` at `$05DA8A` calls.
//!
//! `exlevel_probe sizes rom level [value...]` enters the level (through a
//! screen exit, `expand::enter_by_exit`) with each size byte (hex; default
//! every size with and without bit 7) and prints the work RAM that differs
//! from its entry with size 0. `KOBO_PROBE_FULL` runs the whole load
//! instead.
//!
//! `exlevel_probe entry a.sfc b.sfc level` prints the work RAM two ROMs'
//! entrance code leaves differently for the level (`KOBO_PROBE_FULL`: after
//! the whole load).
//!
//! `exlevel_probe ram rom level [size]` prints the taller levels' RAM after
//! the level's load.
//!
//! `exlevel_probe compare a.sfc b.sfc level [option...]` plays the level in
//! both ROMs and prints the frames where their work RAM (less the stack,
//! direct page scratch `$00`-`$0F`, and the ranges given with
//! `ignore=ADDR-END`) or, with `vram`, their video memory, differ, up to
//! 10. Options as `sprite_probe`'s: `path=X0:DX[:Y0:DY]` carries the
//! player (pixels, per frame), `frames=N`, `size=VV` writes the level's
//! size byte first, `ram:ADDR=VV` sets RAM on entry, `hold:ADDR=VV` on
//! every frame, `pad=HEX` holds controller bits (`$15`/`$16` as the NMI
//! reads them: `B Y sel start up down left right` in the high byte then `A
//! X L R`, held, and pressed anew every 16 frames), `free` leaves the player where the game puts it.
//! `runs=N` shows up to N differing runs of work RAM a frame (12; 0 lists
//! every differing frame's number instead), and
//! `words=` takes `$7F` addresses as `1xxxx`; `vram=LO-HI` compares only
//! those VRAM words. `lag=N` makes every Nth frame lag: an NMI comes
//! before the game loop has ended the frame (`expand::play_game_loop_lagging`).
//!
//! Clean room: a ROM Lunar Magic saved holds its code; this reads memory
//! only, and never with `KOBO_CPU_TRACE`.

use kobo_core::{Rom, SnesAddr, expand, ram};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches('$'), 16).unwrap()
}

fn read3(rom: &Rom, at: u32) -> u32 {
    rom.read_u24(SnesAddr::new(at)).unwrap()
}

/// Where the level size table is: `$240` bytes before the `$05DA8A` hook's
/// code, in Lunar Magic's layout and Kobo's.
fn size_table(rom: &Rom) -> u32 {
    assert_eq!(
        rom.read_u8(SnesAddr::new(0x05DA8A)).unwrap(),
        0x22,
        "no JSL at $05DA8A: the ROM has no taller levels"
    );
    read3(rom, 0x05DA8B) - 0x240
}

fn with_size(rom: &Rom, level: u16, size: u8) -> Rom {
    let mut rom = Rom::from_bytes(rom.data().to_vec()).unwrap();
    let at = size_table(&rom) + level as u32;
    rom.write_u8(SnesAddr::new(at), size).unwrap();
    rom
}

fn entry(rom: &Rom, level: u16) -> Vec<u8> {
    let ram = if std::env::var_os("KOBO_PROBE_FULL").is_some() {
        expand::expand_level(rom, level).unwrap().ram
    } else {
        let high = 0x04 | (level >> 8) as u8;
        expand::enter_by_exit(rom, level as u8, high, false, (level >> 8) as u8, |_| {}).unwrap()
    };
    let mut bytes = ram.bytes(ram::RamAddr::new(0x7E_0000), 0x2000);
    bytes[0x100..0x200].fill(0);
    bytes[..0x10].fill(0);
    bytes
}

fn runs(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            let start = i;
            while i < a.len() && (a[i] != b[i] || (i + 1 < a.len() && a[i + 1] != b[i + 1])) {
                i += 1;
            }
            out.push((start, i));
        } else {
            i += 1;
        }
    }
    out
}

fn show(bytes: &[u8]) -> String {
    let text: Vec<String> = bytes.iter().take(24).map(|b| format!("{b:02x}")).collect();
    let more = if bytes.len() > 24 { " ..." } else { "" };
    format!("{}{more}", text.join(" "))
}

fn sizes(rom: &Rom, level: u16, values: &[u8]) {
    let base = entry(&with_size(rom, level, 0), level);
    for &v in values {
        let got = entry(&with_size(rom, level, v), level);
        let diff = runs(&base, &got);
        println!("size {v:02X}: {} runs", diff.len());
        for (s, e) in diff {
            println!(
                "  ${s:04X}+{}  {}  ->  {}",
                e - s,
                show(&base[s..e]),
                show(&got[s..e])
            );
        }
    }
}

fn ram_after(rom: &Rom, level: u16) {
    let m = expand::expand_level(rom, level).unwrap().ram;
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    let w24 = |a: u32| m.u24(at(a));
    println!(
        "$5B={:02X} $13D7={:04X} $1936={:04X} $0BE7={:02X} $13CD={:02X}",
        m.u8(at(0x5B)),
        m.u16(at(0x13D7)),
        m.u16(at(0x1936)),
        m.u8(at(0x0BE7)),
        m.u8(at(0x13CD))
    );
    let line = |f: &dyn Fn(u32) -> String| (0..32).map(f).collect::<Vec<_>>().join(" ");
    println!("low  {}", line(&|i| format!("{:06X}", w24(0x0BF6 + 3 * i))));
    println!("high {}", line(&|i| format!("{:06X}", w24(0x0C56 + 3 * i))));
    println!(
        "offs {}",
        line(&|i| format!("{:02X}{:02X}", m.u8(at(0x0CD6 + i)), m.u8(at(0x0CB6 + i))))
    );
}

struct Options {
    path: Option<(i32, i32, i32, i32)>,
    frames: u32,
    /// `runs=N`: how many differing work RAM runs `compare` shows a frame.
    runs: usize,
    size: Option<u8>,
    ram: Vec<(u32, u8)>,
    hold: Vec<(u32, u8)>,
    pad: u16,
    ignore: Vec<(usize, usize)>,
    vram: bool,
    /// `vram=LO-HI`: only those VRAM words.
    vram_words: Option<(usize, usize)>,
    words: Vec<u32>,
    /// `lag=N`: every Nth frame lags (an NMI before the loop's frame ends).
    lag: u32,
}

fn options(args: &[String]) -> Options {
    let mut o = Options {
        path: Some((48, 3, -1, 0)),
        frames: 300,
        runs: 12,
        size: None,
        ram: vec![],
        hold: vec![],
        pad: 0,
        ignore: vec![(0x100, 0x200), (0x00, 0x10)],
        vram: false,
        vram_words: None,
        words: vec![],
        lag: 0,
    };
    let poke = |s: &str| {
        let (a, v) = s.split_once('=').unwrap();
        (hex(a), hex(v) as u8)
    };
    for a in args {
        if let Some(v) = a.strip_prefix("path=") {
            let n: Vec<i32> = v.split(':').map(|x| x.parse().unwrap()).collect();
            o.path = Some(if n.len() >= 4 {
                (n[0], n[1], n[2], n[3])
            } else {
                (n[0], n[1], -1, 0)
            });
        } else if a == "free" {
            o.path = None;
        } else if a == "vram" {
            o.vram = true;
        } else if let Some(v) = a.strip_prefix("vram=") {
            let (lo, hi) = v.split_once('-').unwrap();
            o.vram = true;
            o.vram_words = Some((hex(lo) as usize, hex(hi) as usize));
        } else if let Some(v) = a.strip_prefix("frames=") {
            o.frames = v.parse().unwrap();
        } else if let Some(v) = a.strip_prefix("size=") {
            o.size = Some(hex(v) as u8);
        } else if let Some(v) = a.strip_prefix("pad=") {
            o.pad = hex(v) as u16;
        } else if let Some(v) = a.strip_prefix("ram:") {
            o.ram.push(poke(v));
        } else if let Some(v) = a.strip_prefix("hold:") {
            o.hold.push(poke(v));
        } else if let Some(v) = a.strip_prefix("runs=") {
            o.runs = v.parse().unwrap();
        } else if let Some(v) = a.strip_prefix("lag=") {
            o.lag = v.parse().unwrap();
        } else if let Some(v) = a.strip_prefix("words=") {
            o.words = v.split(',').map(hex).collect();
        } else if let Some(v) = a.strip_prefix("ignore=") {
            let (s, e) = v.split_once('-').unwrap_or((v, v));
            o.ignore.push((hex(s) as usize, hex(e) as usize + 1));
        } else {
            panic!("unknown option {a}");
        }
    }
    o
}

/// Every frame's work RAM (and VRAM) as the level plays.
fn play(rom: &Rom, level: u16, o: &Options) -> Vec<(Vec<u8>, Vec<u8>)> {
    let rom = match o.size {
        Some(v) => with_size(rom, level, v),
        None => Rom::from_bytes(rom.data().to_vec()).unwrap(),
    };
    let mut frames = Vec::new();
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    let lag = o.lag;
    let result = expand::play_game_loop_lagging(
        &rom,
        level,
        o.frames,
        |frame| lag != 0 && frame % lag == lag - 1,
        |frame, m| {
            if frame == 0 {
                for &(a, v) in &o.ram {
                    m.set_u8(at(a), v);
                }
            }
            for &(a, v) in &o.hold {
                m.set_u8(at(a), v);
            }
            if o.pad != 0 {
                // Held, and pressed anew every 16 frames.
                let pressed = frame % 16 == 0;
                m.set_u8(at(0x15), (o.pad >> 8) as u8);
                m.set_u8(at(0x17), o.pad as u8);
                m.set_u8(at(0x16), if pressed { (o.pad >> 8) as u8 } else { 0 });
                m.set_u8(at(0x18), if pressed { o.pad as u8 } else { 0 });
            }
            if let Some((x0, dx, y0, dy)) = o.path {
                let x = x0 + dx * frame as i32;
                m.set_u16(at(0x94), x as u16);
                if y0 >= 0 {
                    let y = y0 + dy * frame as i32;
                    m.set_u16(at(0x96), y as u16);
                    m.set_u8(at(0x7D), 0);
                }
                m.set_u8(at(0x1497), 0x7F); // invulnerable
            }
        },
        |_, played| {
            let mut bytes = played.ram.bytes(ram::RamAddr::new(0x7E_0000), 0x20000);
            for &(s, e) in &o.ignore {
                bytes[s..e].fill(0);
            }
            let video = if o.vram {
                let mut v = played.vram.to_vec();
                if let Some((lo, hi)) = o.vram_words {
                    v[..lo * 2].fill(0);
                    v[hi * 2..].fill(0);
                }
                v
            } else {
                Vec::new()
            };
            frames.push((bytes, video));
        },
    );
    if let Err(error) = result {
        eprintln!("stopped: {error}");
    }
    frames
}

/// `call rom level addr jsl|jsr [a=HEX x= y= p= db=] [size=VV] [ram:ADDR=VV...]
/// [words=ADDR,...]`: the registers and words the routine leaves. On a
/// ROM Lunar Magic saved, only the words, and only for a routine of the
/// game's or a hook (clean room).
fn call(args: &[String]) {
    let mut rom = Rom::load(&args[0]).unwrap();
    let level = hex(&args[1]) as u16;
    let addr = hex(&args[2]);
    let long = args[3] == "jsl";
    let mut regs = expand::Registers {
        p: 0x30,
        ..Default::default()
    };
    let mut pokes = Vec::new();
    let mut words = Vec::new();
    for a in &args[4..] {
        let (k, v) = a.split_once('=').unwrap();
        match k {
            "a" => regs.a = hex(v) as u16,
            "x" => regs.x = hex(v) as u16,
            "y" => regs.y = hex(v) as u16,
            "p" => regs.p = hex(v) as u8,
            "db" => regs.db = hex(v) as u8,
            "size" => rom = with_size(&rom, level, hex(v) as u8),
            "words" => words = v.split(',').map(hex).collect(),
            _ => {
                let at = hex(k.strip_prefix("ram:").unwrap());
                pokes.push((at, hex(v) as u8));
            }
        }
    }
    let vanilla = Rom::load(kobo_core::config::vanilla_rom_path().unwrap()).unwrap();
    let result = expand::call_in_level(
        &rom,
        &vanilla,
        level,
        |m| {
            for &(a, v) in &pokes {
                m.set_u8(ram::RamAddr::new(0x7E_0000 | a), v);
            }
        },
        addr,
        long,
        regs,
    );
    match result {
        Ok((m, r)) => {
            let words: Vec<String> = words
                .iter()
                .map(|&a| {
                    let w = kobo_core::clean_room::bytes(&m, ram::RamAddr::new(0x7E_0000 | a), 2);
                    format!("${a:04X}={:04X}", u16::from_le_bytes([w[0], w[1]]))
                })
                .collect();
            // No registers on a ROM Lunar Magic saved (clean room).
            let regs = r.map_or("registers withheld (clean room)".into(), |r| {
                format!("a={:04X} x={:04X} y={:04X} p={:02X}", r.a, r.x, r.y, r.p)
            });
            println!("{regs} {}", words.join(" "));
        }
        Err(e) => println!("failed: {e}"),
    }
}

/// The 16-bit words named with `words=` on every frame they change.
fn show_words(rom: &Rom, level: u16, o: &Options) {
    let mut last = String::new();
    for (frame, (ram, _)) in play(rom, level, o).iter().enumerate() {
        let line: Vec<String> = o
            .words
            .iter()
            .map(|&a| {
                let a = (a & 0x1FFFF) as usize;
                let name = if a > 0xFFFF {
                    format!("$7F{:04X}", a & 0xFFFF)
                } else {
                    format!("${a:04X}")
                };
                format!("{name}={:04X}", ram[a] as u16 | (ram[a + 1] as u16) << 8)
            })
            .collect();
        let line = line.join(" ");
        if line != last {
            println!("frame {frame}: {line}");
            last = line;
        }
    }
}

fn compare(a: &Rom, b: &Rom, level: u16, o: &Options) {
    let (fa, fb) = (play(a, level, o), play(b, level, o));
    let mut shown = 0;
    for (frame, ((ra, va), (rb, vb))) in fa.iter().zip(&fb).enumerate() {
        let wram = runs(ra, rb);
        let vram = runs(va, vb);
        if wram.is_empty() && vram.is_empty() {
            continue;
        }
        if o.runs == 0 {
            // `runs=0`: every differing frame's number, one line.
            print!("{frame} ");
            shown += 1;
            continue;
        }
        println!("frame {frame}:");
        for (s, e) in wram.iter().take(o.runs) {
            println!(
                "  wram ${:06X}+{}  {}  ->  {}",
                0x7E_0000 + s,
                e - s,
                show(&ra[*s..*e]),
                show(&rb[*s..*e])
            );
        }
        for (s, e) in vram.iter().take(6) {
            println!(
                "  vram word ${:04X}+{}  {}  ->  {}",
                s / 2,
                e - s,
                show(&va[*s..*e]),
                show(&vb[*s..*e])
            );
        }
        shown += 1;
        if shown == 10 {
            break;
        }
    }
    if fa.len() != fb.len() {
        println!("played {} and {} frames", fa.len(), fb.len());
    }
    if shown == 0 {
        println!("same on all {} frames", fa.len().min(fb.len()));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = |i: usize| Rom::load(&args[i]).unwrap();
    match args.first().map(String::as_str) {
        Some("sizes") => {
            let values: Vec<u8> = if args.len() > 3 {
                args[3..].iter().map(|v| hex(v) as u8).collect()
            } else {
                (0..32).flat_map(|m| [m, m | 0x80]).collect()
            };
            sizes(&rom(1), hex(&args[2]) as u16, &values);
        }
        Some("ram") => {
            let mut r = rom(1);
            let level = hex(&args[2]) as u16;
            if let Some(v) = args.get(3) {
                r = with_size(&r, level, hex(v) as u8);
            }
            ram_after(&r, level);
        }
        Some("entry") => {
            let level = hex(&args[3]) as u16;
            let (a, b) = (entry(&rom(1), level), entry(&rom(2), level));
            for (s, e) in runs(&a, &b) {
                println!(
                    "  ${s:04X}+{}  {}  ->  {}",
                    e - s,
                    show(&a[s..e]),
                    show(&b[s..e])
                );
            }
        }
        Some("call") => call(&args[1..]),
        Some("show") => {
            let o = options(&args[3..]);
            show_words(&rom(1), hex(&args[2]) as u16, &o);
        }
        Some("compare") => {
            let o = options(&args[4..]);
            compare(&rom(1), &rom(2), hex(&args[3]) as u16, &o);
        }
        _ => eprintln!("usage: exlevel_probe sizes|ram|compare ..."),
    }
}
