//! Kobo's ExAnimation code against Lunar Magic's (docs/lunar-magic-install.md,
//! "ExAnimation"): the clean ROM, saved by Lunar Magic 3.70 with a level
//! that has ExAnimation (which installs Lunar Magic's code), and the same
//! with Kobo's `exanimation.asm` in its place, play seeded random level and
//! global lists of every type and trigger, with random settings, trigger
//! states at the load, and events; every frame's VRAM (but the player's
//! tiles), CGRAM, palette copies, and trigger RAM must be the same.
//!
//! Opt-in: `KOBO_LUNAR_MAGIC` is Lunar Magic 3.70's folder, run under Wine
//! through `tools/lunar-magic/lm`. Needs the vanilla ROM and Asar too.
//! `KOBO_EXANIM_SEED` and `KOBO_EXANIM_CASES` choose the scenarios;
//! `KOBO_EXANIM_KEEP=dir` writes the two ROMs there.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::Lcg;

use kobo_core::mwl::MwlFile;
use kobo_core::ram::{Ram, RamAddr};
use kobo_core::{Rom, SnesAddr, expand};

const LEVEL: u16 = 0x105;
/// The ExAnimation section of an MWL file.
const ANIMATION_SECTION: usize = 6;

/// `base` (the clean ROM, or it with SA-1 Pack) after Lunar Magic imports
/// level 105 with `animation` as its ExAnimation list: Lunar Magic's own
/// ExAnimation code installed.
fn lunar_magic_install(lm: &Path, clean: &Rom, base: &Rom, animation: &[u8]) -> Rom {
    let sa1 = if base.mapping().is_sa1() { "-sa1" } else { "" };
    let ws = common::lm::Workspace::new(lm, &format!("exanim{sa1}"), base, Some(clean), false);
    let level = format!("{LEVEL:X}");
    let mwl = ws.path().join("level.mwl");
    ws.run(&["-ExportLevel", "rom.smc", "level.mwl", &level]);
    let mut file = MwlFile::parse(&fs::read(&mwl).unwrap()).unwrap();
    let mut section = vec![0; 8];
    section.extend_from_slice(animation);
    file.sections[ANIMATION_SECTION] = section;
    fs::write(&mwl, file.to_bytes()).unwrap();
    ws.run(&["-ImportLevel", "rom.smc", "level.mwl", &level]);
    ws.rom()
}

fn target(rom: &Rom) -> u32 {
    assert_eq!(rom.read_u8(SnesAddr::new(0x0583AD)).unwrap(), 0x22);
    rom.read_u24(SnesAddr::new(0x0583AE)).unwrap()
}

/// `lm` with Kobo's ExAnimation code in place of Lunar Magic's, keeping the
/// level table, the global list, the alternative files, and the settings,
/// and with the
/// game's palette copy at `$00A5E1` back (tools/lunar-magic/with-kobo-exanim).
fn with_kobo(asar: &kobo_core::asar::Asar, clean: &Rom, lm: &Rom) -> Rom {
    let mut rom = Rom::from_headerless(lm.data().to_vec()).unwrap();
    let copy = clean.read(SnesAddr::new(0x00A5E1), 6).unwrap().to_vec();
    rom.write(SnesAddr::new(0x00A5E1), &copy).unwrap();
    // Nothing of Lunar Magic's for the patch's autoclean to free.
    rom.write(SnesAddr::new(0x0583AE), &[0xFF; 3]).unwrap();
    let mut rom = asar
        .patch(
            &rom,
            &kobo_core::install::patch(kobo_core::install::EXANIMATION),
        )
        .unwrap()
        .rom;
    let (from, to) = (target(lm), target(&rom));
    for (offset, len) in [(0xEA, 3), (0x5B, 2), (0x65, 2)] {
        let bytes = lm.read(SnesAddr::new(from + offset), len).unwrap().to_vec();
        rom.write(SnesAddr::new(to + offset), &bytes).unwrap();
    }
    for (at, len) in [(0x03BCC0, 16), (0x03FE00, 512)] {
        let bytes = lm.read(SnesAddr::new(at), len).unwrap().to_vec();
        rom.write(SnesAddr::new(at), &bytes).unwrap();
    }
    rom
}

/// A slot's entry: type, trigger, frames less one, destination, frames.
fn slot(kind: u8, trigger: u8, frames: u8, dest: u16, list: &[u16]) -> Vec<u8> {
    let mut out = vec![kind, trigger, frames, dest as u8, (dest >> 8) as u8];
    for f in list {
        out.extend(f.to_le_bytes());
    }
    out
}

/// A list of `slots` (numbered), with its header.
fn list(
    slots: &[(u8, Vec<u8>)],
    count: u8,
    alt: u8,
    keep: u16,
    set: u16,
    manual: u16,
    frames: &[u8],
) -> Vec<u8> {
    let mut out = vec![count, alt];
    out.extend(keep.to_le_bytes());
    out.extend(set.to_le_bytes());
    out.extend(manual.to_le_bytes());
    out.extend(frames);
    let mut offsets = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for k in 0..count {
        match slots.iter().find(|(n, _)| *n == k) {
            Some((_, entry)) => {
                offsets.extend((2 * count as u16 + body.len() as u16).to_le_bytes());
                body.extend(entry);
            }
            None => offsets.extend([0, 0]),
        }
    }
    out.extend(offsets);
    out.extend(body);
    out
}

/// Triggers with a second set of frames, as Lunar Magic reads a list.
fn second_set(trigger: u8) -> bool {
    matches!(trigger, 0x01..=0x05 | 0x07 | 0x09..=0x0E | 0x20..=0x2F)
}

fn random_slot(rng: &mut Lcg) -> Vec<u8> {
    const TRIGGERS: [u8; 15] = [0, 0, 0, 0, 1, 2, 3, 4, 5, 7, 6, 8, 0x09, 0x10, 0x20];
    let trigger = |rng: &mut Lcg| {
        let t = rng.pick(&TRIGGERS);
        match t {
            // The help's "Do Not Use" (9-E); the precision timer (F) only
            // where builds take it, below.
            0x09 => 0x09 + rng.next(6) as u8,
            0x10 => 0x10 + rng.next(16) as u8,
            0x20 => [0x20 + rng.next(16) as u8, 0x30 + rng.next(32) as u8][rng.next(2) as usize],
            _ => t,
        }
    };
    match rng.next(3) {
        0 => {
            let kind = 1 + rng.next(0x12) as u8;
            let t = trigger(rng);
            let nf = rng.next(6) as u8;
            let mut dest = 0x2000 + rng.next(0x100) as u16 * 0x10;
            if rng.chance(20) {
                dest |= 0x8000;
            }
            let n = (nf as usize + 1) * if second_set(t) { 2 } else { 1 };
            let frames: Vec<u16> = (0..n)
                .map(|_| 0xAD00 + rng.next(0x60) as u16 * 0x20)
                .collect();
            slot(kind, t, nf, dest, &frames)
        }
        1 => {
            let kind = 0x13 + rng.next(5) as u8;
            let t = trigger(rng);
            let nf = rng.next(6) as u8;
            let count = rng.pick(&[1u16, 1, 1, 2, 3, 8, 16]);
            let first = 1 + rng.next(0xFF - count as u32) as u16;
            let alt = if rng.chance(15) { 0x80 } else { 0 };
            let n = (nf as usize + 1) * if second_set(t) { 2 } else { 1 };
            let frames: Vec<u16> = (0..n)
                .map(|_| {
                    if count == 1 {
                        rng.next(0x8000) as u16
                    } else {
                        0xAD00 + rng.next(0x400) as u16 * 2
                    }
                })
                .collect();
            slot(kind, t, nf, first | ((count - 1) | alt) << 8, &frames)
        }
        _ => {
            let kind = 0x18 + rng.next(4) as u8;
            // Every trigger, the manual (6), one-shot (8, 10-1F), and
            // once-only (30-4F) ones and 9-F too.
            let t = match rng.next(6) {
                0 => 0,
                1 => rng.next(9) as u8,
                2 => 0x09 + rng.next(7) as u8,
                3 => 0x10 + rng.next(16) as u8,
                4 => 0x20 + rng.next(16) as u8,
                _ => 0x30 + rng.next(32) as u8,
            };
            let count = 2 + rng.next(7) as u16;
            let first = 1 + rng.next(0xFF - count as u32) as u16;
            slot(kind, t, rng.next(4) as u8, first | (count - 1) << 8, &[])
        }
    }
}

fn random_list(rng: &mut Lcg) -> Vec<u8> {
    let count = rng.pick(&[1u8, 2, 4, 8, 9, 16, 24, 32]);
    let mut slots: Vec<(u8, Vec<u8>)> = Vec::new();
    for k in 0..count {
        if k == count - 1 || rng.chance(60) {
            slots.push((k, random_slot(rng)));
        }
    }
    let any = rng.next(0x10000) as u16;
    let keep = rng.pick(&[0xFFFF, 0xFFFF, 0, any]);
    let set = rng.next(0x10000) as u16;
    let manual = if rng.chance(30) {
        rng.next(0x10000) as u16
    } else {
        0
    };
    let frames: Vec<u8> = (0..manual.count_ones())
        .map(|_| {
            let any = rng.next(256) as u8;
            rng.pick(&[0, 1, 2, 3, 5, 0xFF, any])
        })
        .collect();
    list(&slots, count, rng.next(4) as u8, keep, set, manual, &frames)
}

/// A RAM write before a frame.
struct Event {
    frame: u32,
    addr: u32,
    bytes: Vec<u8>,
}

fn random_events(rng: &mut Lcg, frames: u32) -> Vec<Event> {
    let mut out = Vec::new();
    for _ in 0..rng.next(8) {
        let frame = rng.next(frames);
        let (addr, bytes): (u32, Vec<u8>) = match rng.next(9) {
            0 => (0x7E14AD, vec![rng.pick(&[0, 0x40, 0xFF])]),
            1 => (0x7E14AE, vec![rng.pick(&[0, 0x40])]),
            2 => (0x7E14AF, vec![rng.next(2) as u8]),
            3 => (0x7E1490, vec![rng.pick(&[0, 0x40])]),
            4 => (0x7E0F31, vec![rng.next(2) as u8, 9, 9]),
            5 => (0x7E1420, vec![rng.pick(&[0, 4, 5, 6])]),
            6 => (0x7FC0FC, vec![rng.next(256) as u8, rng.next(256) as u8]),
            7 => (
                0x7FC070,
                (0..16)
                    .map(|_| rng.pick(&[0, 1, 2, 3, 4, 7, 0xFF]))
                    .collect(),
            ),
            _ => (0x7FC0F8, (0..4).map(|_| rng.next(256) as u8).collect()),
        };
        out.push(Event { frame, addr, bytes });
    }
    // The game stopped ($9D) or paused ($13D4) for a while.
    for _ in 0..rng.next(3) {
        let from = rng.next(frames);
        let to = from + 1 + rng.next(20);
        let addr = rng.pick(&[0x7E009D, 0x7E13D4]);
        for frame in from..=to {
            out.push(Event {
                frame,
                addr,
                bytes: vec![1],
            });
        }
        out.push(Event {
            frame: to + 1,
            addr,
            bytes: vec![0],
        });
    }
    if rng.chance(10) {
        out.push(Event {
            frame: rng.next(frames),
            addr: 0x7E1493,
            bytes: vec![0xFF],
        });
    }
    out
}

struct Scenario {
    level: Option<Vec<u8>>,
    global: Option<Vec<u8>>,
    settings: u8,
    entry: Vec<Event>,
    events: Vec<Event>,
}

fn random_scenario(rng: &mut Lcg, frames: u32) -> Scenario {
    let level = rng.chance(85).then(|| random_list(rng));
    let global = rng.chance(50).then(|| random_list(rng));
    let settings = rng.pick(&[0, 0, 0, 0, 0, 0, 0x80, 0x40, 0x20, 0x10, 0x30, 0xF0, 0xC0]);
    let mut entry = Vec::new();
    if rng.chance(50) {
        entry.push(Event {
            frame: 0,
            addr: 0x7FC070,
            bytes: (0..16).map(|_| rng.pick(&[0, 1, 2, 5, 0xFF])).collect(),
        });
        entry.push(Event {
            frame: 0,
            addr: 0x7FC0F8,
            bytes: (0..6).map(|_| rng.next(256) as u8).collect(),
        });
    }
    let events = random_events(rng, frames);
    Scenario {
        level,
        global,
        settings,
        entry,
        events,
    }
}

/// `rom` with the scenario's lists at `$208000` and on, through the
/// pointers Lunar Magic's layout keeps at the load hook's target.
fn with_lists(rom: &Rom, s: &Scenario) -> Rom {
    let mut rom = Rom::from_headerless(rom.data().to_vec()).unwrap();
    rom.expand(0x20_0000).unwrap();
    let t = target(&rom);
    let table = rom.read_u24(SnesAddr::new(t + 0xEA)).unwrap();
    let level_at = 0x20_8000u32;
    let global_at = 0x21_8000u32;
    let pointer = match &s.level {
        Some(list) => {
            rom.write(SnesAddr::new(level_at), list).unwrap();
            level_at
        }
        None => 0x0000FF,
    };
    rom.write_u24(SnesAddr::new(table + LEVEL as u32 * 3), pointer)
        .unwrap();
    let (bank, low) = match &s.global {
        Some(list) => {
            rom.write(SnesAddr::new(global_at), list).unwrap();
            (((global_at >> 16) as u16) << 8, global_at as u16)
        }
        None => (0, 0),
    };
    rom.write_u16(SnesAddr::new(t + 0x5B), bank).unwrap();
    rom.write_u16(SnesAddr::new(t + 0x65), low).unwrap();
    rom.write_u8(SnesAddr::new(0x03FE00 + LEVEL as u32), s.settings)
        .unwrap();
    rom
}

/// What is compared after each frame (and before the first).
fn state(played: &expand::PlayedFrame) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, pair) in played.vram.chunks(2).enumerate() {
        // The player's tiles: see the doc comment of the module.
        if !(0x6000..0x6200).contains(&i) && !(0x67F0..0x6800).contains(&i) {
            out.extend(pair);
        }
    }
    out.extend(played.cgram);
    let ram = played.ram;
    out.extend(ram.bytes(RamAddr::new(0x7E_0701), 0x3F4));
    out.push(ram.u8(RamAddr::new(0x7F_C004)));
    out.push(ram.u8(RamAddr::new(0x7F_C019)));
    out.extend(ram.bytes(RamAddr::new(0x7F_C070), 0x50));
    out.extend(ram.bytes(RamAddr::new(0x7F_C0F8), 6));
    out
}

/// Where two states first differ, by what [`state`] keeps there.
fn describe(a: &[u8], b: &[u8]) -> String {
    let vram_words: Vec<u32> = (0..0x8000u32)
        .filter(|i| !(0x6000..0x6200).contains(i) && !(0x67F0..0x6800).contains(i))
        .collect();
    let vram = 2 * vram_words.len();
    let place = |i: usize| -> String {
        if i < vram {
            format!("VRAM word ${:04X}", vram_words[i / 2])
        } else if i < vram + 0x200 {
            format!("CGRAM byte ${:03X}", i - vram)
        } else {
            let r = i - vram - 0x200;
            match r {
                0..0x3F4 => format!("${:06X}", 0x7E_0701 + r),
                0x3F4 => "$7FC004".into(),
                0x3F5 => "$7FC019".into(),
                0x3F6..0x446 => format!("${:06X}", 0x7F_C070 + r - 0x3F6),
                _ => format!("${:06X}", 0x7F_C0F8 + r - 0x446),
            }
        }
    };
    (0..a.len())
        .filter(|&i| a[i] != b[i])
        .take(6)
        .map(|i| format!("{} {:02X}>{:02X}", place(i), a[i], b[i]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn write(ram: &mut Ram, event: &Event) {
    for (i, b) in event.bytes.iter().enumerate() {
        ram.set_u8(RamAddr::new(event.addr + i as u32), *b);
    }
}

fn play(rom: &Rom, s: &Scenario, frames: u32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut before = None;
    expand::play_game_loop_from(
        rom,
        LEVEL,
        frames,
        |ram| s.entry.iter().for_each(|e| write(ram, e)),
        |played| before = Some(state(&played)),
        |frame, ram| {
            if frame == 0 {
                // Frames to show: each 16-bit word its own number.
                for i in 0..0xD00u32 {
                    ram.set_u16(RamAddr::new(0x7E_AD00 + 2 * i), 0x1000 + i as u16);
                }
            }
            s.events
                .iter()
                .filter(|e| e.frame == frame)
                .for_each(|e| write(ram, e));
        },
        |_, played| out.push(state(&played)),
    )
    .unwrap();
    out.insert(0, before.unwrap());
    out
}

#[test]
fn kobos_exanimation_runs_as_lunar_magics() {
    let Some(lunar_magic) = common::lunar_magic() else {
        return;
    };
    let Some(clean) = common::vanilla() else {
        return;
    };
    let Some(asar) = common::asar() else { return };
    let install = list(
        &[(0, slot(0x04, 0, 1, 0x2000, &[0xAD00, 0xAD80]))],
        1,
        0,
        0xFFFF,
        0,
        0,
        &[],
    );
    // On the clean ROM, and with SA-1 Pack applied.
    for sa1 in common::sa1_variants(&clean) {
        let base = common::base_as(&clean, sa1);
        let lm = lunar_magic_install(&lunar_magic, &clean, &base, &install);
        let kobo = with_kobo(&asar, &base, &lm);
        // KOBO_EXANIM_KEEP=dir keeps the two ROMs, for examples/exanim_probe.rs.
        if let Some(dir) = std::env::var_os("KOBO_EXANIM_KEEP") {
            let dir = PathBuf::from(dir);
            let sa1 = if sa1 { "-sa1" } else { "" };
            fs::write(dir.join(format!("lm{sa1}.sfc")), lm.data()).unwrap();
            fs::write(dir.join(format!("kobo{sa1}.sfc")), kobo.data()).unwrap();
        }
        let seed = common::env_number("KOBO_EXANIM_SEED", 1);
        let count = common::env_number("KOBO_EXANIM_CASES", 16);
        let frames = 96;
        let mut rng = Lcg(seed);
        for case in 0..count {
            let s = random_scenario(&mut rng, frames);
            let a = play(&with_lists(&lm, &s), &s, frames);
            let b = play(&with_lists(&kobo, &s), &s, frames);
            let first = (0..a.len()).find(|&f| a[f] != b[f]);
            assert_eq!(
                first,
                None,
                "case {case} (seed {seed}, SA-1 {sa1}): frame {first:?} differs (0 is before \
                 the first): {}; level list {}, global list {}, settings {:02X}, \
                 events {}",
                first.map_or(String::new(), |f| describe(&a[f], &b[f])),
                s.level.as_deref().map_or("none".into(), hex),
                s.global.as_deref().map_or("none".into(), hex),
                s.settings,
                s.entry
                    .iter()
                    .chain(&s.events)
                    .map(|e| format!("{:06X}={}@{}", e.addr, hex(&e.bytes), e.frame))
                    .collect::<Vec<_>>()
                    .join(";"),
            );
        }
    }
}
