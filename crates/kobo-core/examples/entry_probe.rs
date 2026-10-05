//! What each bit of a level's settings does to its entry, as a ROM's own
//! code has it (`expand::enter_by_exit`): for learning from memory effects
//! what Lunar Magic's entrance code does with its per-level tables, and
//! checking Kobo's against it.
//!
//! `entry_probe effects rom.sfc level` flips each bit of the level's eight
//! settings bytes (`$05F000`, `$05F200`, `$05F400`, `$05F600`, `$05DE00`,
//! `$06FA00`, `$06FC00`, `$06FE00`) in turn and prints the work RAM the
//! level's main entrance then leaves differently. `entry_probe compare
//! a.sfc b.sfc level [addr[-end][:mask]...]` runs every value of every byte
//! in both ROMs and prints where their entries differ, but for the bits
//! named (or only at `+addr[-end]`), from the bytes `KOBO_ENTRY_BASE`
//! names (`table=VV,...`, `SZ` the size table) where that is set. `entry_probe levels a.sfc b.sfc
//! [ignore...]` compares every level's (or secondary entrance's) entry as
//! the two ROMs have it. `entry_probe batch rom level word...` reads lines of
//! `table=value` settings (hex) and prints the 16-bit words of RAM named
//! that each leaves.

use kobo_core::{Rom, SnesAddr, expand, ram};

const TABLES: [(&str, u32); 8] = [
    ("05F000", 0x05F000),
    ("05F200", 0x05F200),
    ("05F400", 0x05F400),
    ("05F600", 0x05F600),
    ("05DE00", 0x05DE00),
    ("06FA00", 0x06FA00),
    ("06FC00", 0x06FC00),
    ("06FE00", 0x06FE00),
];

/// The work RAM an entry through the level's main entrance leaves, but the
/// stack and the scratch at `$00`-`$0F`.
/// With `KOBO_ENTRY_FULL` set, the RAM after the whole level load
/// instead, which is where the camera settles; with `KOBO_ENTRY_FRAMES`,
/// after that many frames of play.
fn entry(rom: &Rom, level: u16) -> Vec<u8> {
    let high = 0x04 | (level >> 8) as u8;
    let frames = std::env::var("KOBO_ENTRY_FRAMES").ok();
    let ram = if let Some(frames) = frames {
        expand::play_level(rom, level, frames.parse().unwrap(), |_, _| {}).unwrap()
    } else if std::env::var_os("KOBO_ENTRY_FULL").is_some() {
        expand::expand_level(rom, level).unwrap().ram
    } else {
        // KOBO_ENTRY_OW=<translevel>[,midway]: from the overworld, through
        // `$0109`, with the translevel's midway point passed or not.
        let overworld = std::env::var("KOBO_ENTRY_OW")
            .ok()
            .filter(|s| !s.is_empty());
        // KOBO_ENTRY_SECONDARY: `level` is a secondary entrance, reached by
        // an exit in Lunar Magic's format.
        let secondary = std::env::var_os("KOBO_ENTRY_SECONDARY").is_some();
        let high = if secondary { high | 0x02 } else { high };
        // KOBO_ENTRY_FLAGS=<hex>: bits to add to the exit's flags nibble (8,
        // w: to the midway entrance, or a water level).
        let high = high
            | std::env::var("KOBO_ENTRY_FLAGS")
                .map(|f| u8::from_str_radix(&f, 16).unwrap())
                .unwrap_or(0);
        expand::enter_by_exit(
            rom,
            level as u8,
            high,
            secondary,
            (level >> 8) as u8,
            |ram| {
                if let Some(ow) = &overworld {
                    let (translevel, midway) = ow.split_once(',').unwrap_or((ow, ""));
                    let translevel = u8::from_str_radix(translevel, 16).unwrap();
                    let at = |a| ram::RamAddr::new(0x7E_0000 | a);
                    ram.set_u8(ram::SUBLEVEL_COUNT, 0);
                    // The overworld names a level by its low byte, but
                    // CODE_05D8A2 takes $24 off one of $25 and up: those
                    // levels are named $24 higher, and past $DB have no
                    // name (the probe enters another level for them).
                    let low = level as u8;
                    let name = if low >= 0x25 {
                        low.wrapping_add(0x24)
                    } else {
                        low
                    };
                    ram.set_u8(at(0x0109), name);
                    ram.set_u8(at(0x13BF), translevel);
                    ram.set_u8(
                        at(0x1EA2 + translevel as u32),
                        if midway.is_empty() { 0 } else { 0x40 },
                    );
                }
            },
        )
        // A routine that fails (Lunar Magic's exit to the overworld, run
        // outside the game loop) leaves a RAM image of its own.
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            let mut ram = ram::Ram::new(ram::RamMap::Vanilla);
            ram.fill(ram::RamAddr::new(0x7E_0000), 0x2000, 0xEE);
            ram
        })
    };
    // The stack withheld (clean room), and the scratch bytes, which hold
    // whatever the last routine left.
    let mut bytes = kobo_core::clean_room::bytes(&ram, ram::RamAddr::new(0x7E_0000), 0x2000);
    bytes[..0x10].fill(0);
    bytes
}

/// The address of Lunar Magic's separate midway settings table `n` (0 to
/// 3), one 512-byte table after another (`kobo_core::entrance::Layout`).
fn midway_table(rom: &Rom, n: u32) -> u32 {
    kobo_core::entrance::Layout::of(rom)
        .midway
        .expect("no midway tables")
        .raw()
        + n * 0x200
}

/// The level size table (`SZ`), where Lunar Magic's layout has it.
fn size_table(rom: &Rom) -> u32 {
    kobo_core::level::size::table(rom)
        .expect("no level size table")
        .raw()
}

/// The tables a run varies, as (name, address in `a`, address in `b`):
/// the level's eight settings bytes; with `KOBO_ENTRY_MIDWAY`, the midway
/// tables as well, each ROM's own; with `KOBO_ENTRY_SIZE`, the level size
/// table too; with `KOBO_ENTRY_SECONDARY`, a secondary entrance's tables
/// instead, Lunar Magic's two included.
fn table_list(a: &Rom, b: &Rom) -> Vec<(&'static str, u32, u32)> {
    if std::env::var_os("KOBO_ENTRY_SECONDARY").is_some() {
        return vec![
            ("05FA00", 0x05FA00, 0x05FA00),
            ("05FC00", 0x05FC00, 0x05FC00),
            ("05FE00", 0x05FE00, 0x05FE00),
            ("S5", read3(a, 0x05DC86), read3(b, 0x05DC86)),
            ("S6", read3(a, 0x05DC8B), read3(b, 0x05DC8B)),
        ];
    }
    let mut tables: Vec<_> = TABLES.iter().map(|&(n, t)| (n, t, t)).collect();
    if std::env::var_os("KOBO_ENTRY_MIDWAY").is_some() {
        for (i, name) in ["M1", "M2", "M3", "M4"].into_iter().enumerate() {
            tables.push((name, midway_table(a, i as u32), midway_table(b, i as u32)));
        }
    }
    if std::env::var_os("KOBO_ENTRY_SIZE").is_some() {
        tables.push(("SZ", size_table(a), size_table(b)));
    }
    tables
}

/// A table named in a `table=value` argument: an address in hex, or `M1`
/// to `M4` for the midway tables.
fn table_address(rom: &Rom, name: &str) -> u32 {
    match name.strip_prefix('M') {
        Some(n) => midway_table(rom, n.parse::<u32>().unwrap() - 1),
        None => match name {
            "S5" => read3(rom, 0x05DC86),
            "S6" => read3(rom, 0x05DC8B),
            "SZ" => size_table(rom),
            _ => u32::from_str_radix(name, 16).unwrap(),
        },
    }
}

fn read3(rom: &Rom, a: u32) -> u32 {
    let b = |i| rom.read_u8(SnesAddr::new(a + i)).unwrap() as u32;
    b(0) | b(1) << 8 | b(2) << 16
}

fn with(rom: &Rom, table: u32, level: u16, value: u8) -> Rom {
    let mut rom = Rom::from_bytes(rom.data().to_vec()).unwrap();
    rom.write_u8(SnesAddr::new(table + level as u32), value)
        .unwrap();
    rom
}

fn diff(a: &[u8], b: &[u8]) -> String {
    let parts: Vec<String> = (0..a.len())
        .filter(|&i| a[i] != b[i])
        .map(|i| format!("{i:04X}:{:02X}>{:02X}", a[i], b[i]))
        .collect();
    parts.join(" ")
}

/// Bits to leave out of a comparison, per address, from arguments
/// `addr[-end][:mask]` (hex; the mask defaults to every bit).
fn ignored(args: &[String]) -> Vec<(usize, u8)> {
    let mut ignore = Vec::new();
    // `+addr[-end]` arguments name the only bytes to compare instead.
    let only: Vec<(usize, usize)> = args
        .iter()
        .filter_map(|a| a.strip_prefix('+'))
        .map(|range| {
            let (start, end) = range.split_once('-').unwrap_or((range, range));
            let parse = |s| usize::from_str_radix(s, 16).unwrap();
            (parse(start), parse(end))
        })
        .collect();
    if !only.is_empty() {
        for address in 0..0x2000 {
            if !only
                .iter()
                .any(|&(start, end)| (start..=end).contains(&address))
            {
                ignore.push((address, 0xFF));
            }
        }
    }
    for arg in args.iter().filter(|a| !a.starts_with('+')) {
        let (range, mask) = arg.split_once(':').unwrap_or((arg, "FF"));
        let (start, end) = range.split_once('-').unwrap_or((range, range));
        let mask = u8::from_str_radix(mask, 16).unwrap();
        let end = usize::from_str_radix(end, 16).unwrap();
        for address in usize::from_str_radix(start, 16).unwrap()..=end {
            ignore.push((address, mask));
        }
    }
    ignore
}

fn masked(mut bytes: Vec<u8>, ignore: &[(usize, u8)]) -> Vec<u8> {
    for &(address, mask) in ignore {
        bytes[address] &= !mask;
    }
    bytes
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("effects") => {
            let rom = Rom::load(&args[1]).unwrap();
            let level = u16::from_str_radix(&args[2], 16).unwrap();
            let base = entry(&rom, level);
            for (name, table, _) in table_list(&rom, &rom) {
                let value = rom.read_u8(SnesAddr::new(table + level as u32)).unwrap();
                for bit in (0..8).rev() {
                    let flipped = value ^ (1 << bit);
                    let after = entry(&with(&rom, table, level, flipped), level);
                    println!(
                        "{name} {value:02X}>{flipped:02X} bit {bit}: {}",
                        diff(&base, &after)
                    );
                }
            }
        }
        Some("compare") => {
            let mut a = Rom::load(&args[1]).unwrap();
            let mut b = Rom::load(&args[2]).unwrap();
            let level = u16::from_str_radix(&args[3], 16).unwrap();
            // KOBO_ENTRY_BASE=table=VV,...: the level's bytes in those tables
            // (an address in hex, or SZ for the size table), in both, first.
            if let Ok(list) = std::env::var("KOBO_ENTRY_BASE") {
                for item in list.split(',') {
                    let (table, v) = item.split_once('=').unwrap();
                    let v = u8::from_str_radix(v, 16).unwrap();
                    let (ta, tb) = match table {
                        "SZ" => (size_table(&a), size_table(&b)),
                        t => {
                            let t = u32::from_str_radix(t, 16).unwrap();
                            (t, t)
                        }
                    };
                    a = with(&a, ta, level, v);
                    b = with(&b, tb, level, v);
                }
            }
            let ignore = ignored(&args[4..]);
            let mut same = 0;
            let tables = table_list(&a, &b);
            for &(name, table_a, table_b) in &tables {
                for value in 0..=255u8 {
                    let x = masked(entry(&with(&a, table_a, level, value), level), &ignore);
                    let y = masked(entry(&with(&b, table_b, level, value), level), &ignore);
                    if x == y {
                        same += 1;
                    } else {
                        println!("{name}={value:02X}: {}", diff(&x, &y));
                    }
                }
            }
            println!("{same} of {} the same", 256 * tables.len());
        }
        Some("levels") => {
            // Every level's entry as it stands, in both ROMs.
            let a = Rom::load(&args[1]).unwrap();
            let b = Rom::load(&args[2]).unwrap();
            let ignore = ignored(&args[3..]);
            let mut same = 0;
            for level in 0..0x200u16 {
                let x = masked(entry(&a, level), &ignore);
                let y = masked(entry(&b, level), &ignore);
                if x == y {
                    same += 1;
                } else {
                    println!("{level:03X}: {}", diff(&x, &y));
                }
            }
            println!("{same} of 512 the same");
        }
        Some("batch") => {
            let rom = Rom::load(&args[1]).unwrap();
            let level = u16::from_str_radix(&args[2], 16).unwrap();
            let words: Vec<usize> = args[3..]
                .iter()
                .map(|a| usize::from_str_radix(a, 16).unwrap())
                .collect();
            for line in std::io::stdin().lines() {
                let line = line.unwrap();
                let mut trial = Rom::from_bytes(rom.data().to_vec()).unwrap();
                for poke in line.split_whitespace() {
                    let (table, value) = poke.split_once('=').unwrap();
                    let table = table_address(&trial, table);
                    trial = with(&trial, table, level, u8::from_str_radix(value, 16).unwrap());
                }
                let ram = entry(&trial, level);
                let values: Vec<String> = words
                    .iter()
                    .map(|&a| format!("{:04X}", ram[a] as u16 | (ram[a + 1] as u16) << 8))
                    .collect();
                println!("{line}: {}", values.join(" "));
            }
        }
        _ => {
            eprintln!(
                "usage: entry_probe effects rom level | compare a b level [ignore...] | batch rom level word... < cases | levels a b [ignore...] (see the source)"
            );
            std::process::exit(2)
        }
    }
}
