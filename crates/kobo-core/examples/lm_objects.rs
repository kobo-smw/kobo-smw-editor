//! Cases for Lunar Magic's placed objects (`22`, `23`, `27`, `29`), for
//! learning from memory effects what its object code writes into a level's
//! grid and checking Kobo's against it (docs/lunar-magic-install.md).
//!
//! `lm_objects make in.mwl out.mwl group` replaces the layer 1 objects of
//! a level's MWL export with a group of 16 cases, each on every other
//! screen so it has the next one to cross into; import the result with
//! Lunar Magic into a copy of vanilla, and `lm_objects grid rom.sfc level
//! group` prints, per case, the tiles the level's load left on its two
//! screens, as `x,y=tile`, relative to the first. Groups 0 and 1 are for a
//! horizontal level (`105`), group 2 for a vertical one (`1CE`), whose
//! screens are 16 rows of two 16-column halves. `lm_objects hashes rom.sfc
//! level group` prints each case's grid as a SHA-1 instead, the lines of
//! `tests/fixtures/lunar_magic_objects.txt`. The cases are in
//! `tests/common/object_cases.rs`. `lm_objects add in.mwl
//! out.mwl "40 60 0B"` adds one of Lunar Magic's settings objects, as its
//! bytes, to the end of layer 1. `lm_objects exgfx in.mwl out.mwl slot
//! file` sets one of the level's graphics slots (0 = AN2 to 15 = LG1, as
//! the MWL file orders them) to a file number, in hex. `lm_objects exits
//! in.mwl out.mwl` replaces a level's screen exits with five in Lunar
//! Magic's format, on screens 0 to 4, destination `$20` plus the screen,
//! with flags `u`, `uh`, `us`, `uw`, and `uswh`. `lm_objects timer rom.sfc
//! level [frames]` prints the timer (`$0F31`-`$0F33`) and its status bar
//! copy (`$0F25`-`$0F27`) after the level loads, entered from the overworld
//! (sublevel count 0) and through a screen exit (1), each with the timer
//! first set to `9 9 9`, and after `frames` frames of play. `lm_objects
//! flags rom.sfc level addr...` reports how much of the level's grid
//! changes when 16 bytes from each bus address are set to `$FF` before the
//! load: where a ROM reads the conditional Direct Map16 flags from.

use kobo_core::level::objects::Object;
use kobo_core::mwl::MwlFile;
use kobo_core::{Rom, expand};

#[path = "../tests/common/object_cases.rs"]
mod object_cases;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("make") => make(&args[1], &args[2], args[3].parse().unwrap()),
        Some("add") => add(&args[1], &args[2], &args[3]),
        Some("exits") => exits(&args[1], &args[2]),
        Some("exgfx") => exgfx(
            &args[1],
            &args[2],
            args[3].parse().unwrap(),
            u16::from_str_radix(&args[4], 16).unwrap(),
        ),
        Some("timer") => timer(
            &args[1],
            u16::from_str_radix(&args[2], 16).unwrap(),
            args.get(3).map_or(0, |f| f.parse().unwrap()),
        ),
        Some("flags") => flags(
            &args[1],
            u16::from_str_radix(&args[2], 16).unwrap(),
            &args[3..],
        ),
        Some(command @ ("grid" | "hashes")) => grid(
            &args[1],
            u16::from_str_radix(&args[2], 16).unwrap(),
            args[3].parse().unwrap(),
            command == "hashes",
        ),
        _ => {
            eprintln!(
                "usage: lm_objects make in.mwl out.mwl group | grid|hashes rom.sfc level group"
            );
            std::process::exit(2)
        }
    }
}

fn make(input: &str, output: &str, group_index: usize) {
    let bytes = std::fs::read(input).unwrap();
    let mut mwl = MwlFile::parse(&bytes).unwrap().decode(None).unwrap();
    let data = &mut mwl.layer1.data;
    // 32 screens, or 12 of a vertical level's two halves.
    data.header[0] = data.header[0] & 0xE0 | (object_cases::screens(group_index) - 1);
    data.objects = object_cases::objects(group_index);
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn add(input: &str, output: &str, bytes: &str) {
    let mut mwl = MwlFile::parse(&std::fs::read(input).unwrap())
        .unwrap()
        .decode(None)
        .unwrap();
    let bytes = bytes
        .split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect();
    mwl.layer1.data.objects.push(Object::Unplaced(bytes));
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn exits(input: &str, output: &str) {
    use kobo_core::level::objects::ScreenExit;
    let mut mwl = MwlFile::parse(&std::fs::read(input).unwrap())
        .unwrap()
        .decode(None)
        .unwrap();
    let objects = &mut mwl.layer1.data.objects;
    objects.retain(|o| !matches!(o, Object::ScreenExit(_)));
    for (screen, flags) in [0x4u8, 0x5, 0x6, 0xC, 0xF].into_iter().enumerate() {
        objects.push(Object::ScreenExit(ScreenExit {
            screen: screen as u8,
            flags,
            destination: 0x20 + screen as u8,
        }));
    }
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn exgfx(input: &str, output: &str, slot: usize, file: u16) {
    let mut mwl = MwlFile::parse(&std::fs::read(input).unwrap())
        .unwrap()
        .decode(None)
        .unwrap();
    mwl.exgfx.0[slot] = file;
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn timer(path: &str, level: u16, frames: u32) {
    use kobo_core::ram::{self, RamAddr};
    let rom = Rom::load(path).unwrap();
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    for (entry, count) in [("overworld", 0u8), ("exit", 1)] {
        let ram = expand::play_level_entered(
            &rom,
            level,
            |ram| {
                ram.set_u8(ram::SUBLEVEL_COUNT, count);
                if count == 0 {
                    // Named as entry_probe names it from the overworld.
                    let low = level as u8;
                    let name = if low >= 0x25 {
                        low.wrapping_add(0x24)
                    } else {
                        low
                    };
                    ram.set_u8(at(0x0109), name);
                }
                for i in 0..3 {
                    ram.set_u8(at(0x0F31 + i), 9);
                    ram.set_u8(at(0x0F25 + i), 9);
                }
            },
            frames,
            |_, _| {},
        )
        .unwrap();
        let bytes = |a: u32| {
            (0..3)
                .map(|i| format!("{:02X}", ram.u8(at(a + i))))
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!(
            "{entry:9} timer {} status bar {} frames {:02X}",
            bytes(0x0F31),
            bytes(0x0F25),
            ram.u8(at(0x0F30))
        );
    }
}

/// The tile grid a load leaves with nothing set before it, and with each
/// of `addrs` (bus addresses, as the S-CPU sees them) and the 15 bytes
/// after it set to `$FF` first: where a ROM's load reads the conditional
/// Direct Map16 flags from, by which of them changes the grid.
fn flags(path: &str, level: u16, addrs: &[String]) {
    use kobo_core::ram::RamAddr;
    let rom = Rom::load(path).unwrap();
    let grid = |set: Option<u32>| {
        let ram = expand::play_level_entered(
            &rom,
            level,
            |ram| {
                for a in set.into_iter().flat_map(|a| a..a + 16) {
                    ram.poke(a, 0xFF);
                }
            },
            0,
            |_, _| {},
        )
        .unwrap();
        let mut cells = ram.bytes(RamAddr::new(0x7E_C800), 0x3800);
        cells.extend(ram.bytes(RamAddr::new(0x7F_C800), 0x3800));
        cells
    };
    let none = grid(None);
    for addr in addrs {
        let a = u32::from_str_radix(addr.trim_start_matches('$'), 16).unwrap();
        let cells = grid(Some(a));
        let changed = none.iter().zip(&cells).filter(|(x, y)| x != y).count();
        println!("${a:06X}: {changed} grid bytes change");
    }
}

fn grid(path: &str, level: u16, group_index: usize, hashes: bool) {
    let rom = Rom::load(path).unwrap();
    let loaded = expand::expand_level(&rom, level).unwrap();
    for (name, cells) in object_cases::grids(&loaded.tiles, group_index) {
        if hashes {
            println!("{} {name}", object_cases::hash(&cells));
        } else {
            println!("{name}: {cells}");
        }
    }
}
