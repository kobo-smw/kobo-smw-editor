//! Where screen exits lead, as a ROM's own entrance code resolves them
//! (`expand::enter_by_exit`), for learning from memory effects what Lunar
//! Magic's exit and entrance hooks do and checking Kobo's against them.
//!
//! `exit_probe rom.sfc [low...] [addr=value]` prints, for each destination low byte
//! (default `20` and `C8`), each exit flags nibble (`$19D8`), the game's
//! secondary flag, and the player's submap: the level loaded (`$0E`-`$0F`),
//! the entrance setting (`$192A`), and the player's position.

use kobo_core::{Rom, expand, ram};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = Rom::load(&args[0]).unwrap();
    // `addr=value` (hex) sets a 16-bit word of RAM first.
    let poke = args[1..].iter().find_map(|a| {
        let (addr, value) = a.split_once('=')?;
        Some((
            u32::from_str_radix(addr, 16).ok()?,
            u16::from_str_radix(value, 16).ok()?,
        ))
    });
    let lows: Vec<u8> = args[1..]
        .iter()
        .filter(|a| !a.contains('='))
        .map(|l| u8::from_str_radix(l, 16).unwrap())
        .collect();
    let lows = if lows.is_empty() {
        vec![0x20, 0xC8]
    } else {
        lows
    };
    let byte = |ram: &ram::Ram, a: u32| ram.u8(ram::RamAddr::new(0x7E_0000 | a));
    let word = |ram: &ram::Ram, a: u32| byte(ram, a) as u16 | (byte(ram, a + 1) as u16) << 8;
    for low in lows {
        for high in [0x0u8, 0x1, 0x4, 0x5, 0x6, 0x7, 0xC, 0xD, 0xE, 0xF] {
            for secondary in [false, true] {
                for submap in [0u8, 1] {
                    let ram = expand::enter_by_exit(&rom, low, high, secondary, submap, |ram| {
                        if let Some((addr, value)) = poke {
                            ram.set_u16(ram::RamAddr::new(0x7E_0000 | addr), value);
                        }
                    })
                    .unwrap();
                    println!(
                        "exit {low:02X} flags {high:X} {} submap {submap}: level {:04X} entrance {:02X} player {:04X},{:04X} layers {:04X},{:04X}",
                        if secondary { "sec" } else { "   " },
                        word(&ram, 0x0E),
                        byte(&ram, 0x192A),
                        word(&ram, 0x94),
                        word(&ram, 0x96),
                        word(&ram, 0x1C),
                        word(&ram, 0x20),
                    );
                }
            }
        }
    }
}
