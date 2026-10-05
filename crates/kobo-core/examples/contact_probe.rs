//! Which of a block's actions fire as the player meets it in different
//! ways, observed by running the ROM (`expand::play_level`): for learning,
//! from memory effects alone, when Lunar Magic's acts-like chain runs each
//! custom block action, and for checking Kobo's against it.
//!
//! `contact_probe place in.mwl out.mwl x y` adds tile `$200` at (x, y) to an
//! MWL file (Lunar Magic's object `27`). `contact_probe run rom.sfc [x y]`
//! plays level `105` with the player put next to the tile at (x, y),
//! default (6, 20), which it writes into the level's grid first, in each
//! scenario, for small and big Mario, and prints
//! the actions the probe block (`tools/lunar-magic/block-probe/probe.asm`)
//! logged, with where the game sampled, relative to the player.
//! `contact_probe stand rom.sfc level x y tile...` drops the player onto
//! each tile in turn, written into the grid at (x, y) of a horizontal level
//! (or of a vertical level's first screen), and prints whether they landed
//! and the low byte of the tile the game was told it acts like (`$1693`),
//! and, on a probe ROM, the actions the probe block logged. An argument
//! `addr=value` (hex) instead of a tile writes that RAM byte on every frame.
//! `contact_probe berry rom.sfc x y tile...` puts each tile in level
//! `105`'s grid at (x, y) with a stunned baby Yoshi (sprite `2D`, which
//! checks the tile at its centre for a berry, as Yoshi's tongue does) on
//! it, and prints the berry type the game recorded (`$18D6`), the sprites
//! it made and where, the tile left there, `$1693` and `$1933`, and the
//! five bytes from `$7FB40F` (where
//! `tools/lunar-magic/block-probe/tongue-slot.asm`'s routine in the Yoshi's
//! tongue slot logs); with `addr=value`, as above. `level=n` plays another
//! level (vertical ones too), `layer=2` puts the tile in layer 2, with
//! Yoshi kept where layer 2 has it at (x, y), and `other=tile` puts a tile
//! in the other layer where Yoshi's centre is. `KOBO_BERRY_FRAMES` sets how
//! many frames it plays (16), `KOBO_BERRY_PLACE` on which frame the tiles
//! go in (0; the layers' offsets are set from frame 1), and `KOBO_BERRY_DUMP=prefix` writes each
//! run's work RAM (`$7E0000`-`$7FFFFF`) to `prefix-TILE.bin`.
//! `contact_probe tongue rom.sfc tile...` enters level `105` (or `level=n`) with Yoshi
//! (`$0DC1` set), writes the tile over a block ahead of the player, and
//! presses Y, printing the same (`KOBO_TONGUE_MOUNT` and
//! `KOBO_TONGUE_FRAMES` set the frames, `KOBO_TONGUE_TRACE` prints each).

use kobo_core::level::objects::Object;
use kobo_core::mwl::MwlFile;
use kobo_core::{Rom, SnesAddr, expand, ram};

const ACTIONS: [&str; 12] = [
    "below",
    "above",
    "side",
    "sprite v",
    "sprite h",
    "cape",
    "fireball",
    "top corner",
    "body",
    "head",
    "wall feet",
    "wall body",
];
const LOG: u32 = 0x7F_B40F;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("place") => place(
            &args[1],
            &args[2],
            args[3].parse().unwrap(),
            args[4].parse().unwrap(),
        ),
        Some("run") => {
            let at = |i: usize, default| args.get(i).map_or(default, |s| s.parse().unwrap());
            run(&args[1], at(2, 6), at(3, 20))
        }
        Some("stand") => {
            let n = |i: usize| u16::from_str_radix(&args[i], 16).unwrap();
            let (pokes, tiles): (Vec<_>, Vec<_>) = args[5..].iter().partition(|a| a.contains('='));
            let tiles: Vec<u16> = tiles
                .iter()
                .map(|t| u16::from_str_radix(t, 16).unwrap())
                .collect();
            let pokes: Vec<(u32, u8)> = pokes
                .iter()
                .map(|p| {
                    let (a, v) = p.split_once('=').unwrap();
                    (
                        u32::from_str_radix(a, 16).unwrap(),
                        u8::from_str_radix(v, 16).unwrap(),
                    )
                })
                .collect();
            let (x, y) = (args[3].parse().unwrap(), args[4].parse().unwrap());
            stand(&args[1], n(2), x, y, &tiles, &pokes)
        }
        Some("tongue") => {
            let level = args[2..]
                .iter()
                .find_map(|a| a.strip_prefix("level="))
                .map_or(0x105, |v| u16::from_str_radix(v, 16).unwrap());
            let (pokes, tiles): (Vec<_>, Vec<_>) = args[2..]
                .iter()
                .filter(|a| !a.starts_with("level="))
                .partition(|a| a.contains('='));
            let tiles: Vec<u16> = tiles
                .iter()
                .map(|t| u16::from_str_radix(t, 16).unwrap())
                .collect();
            let pokes: Vec<(u32, u8)> = pokes
                .iter()
                .map(|p| {
                    let (a, v) = p.split_once('=').unwrap();
                    (
                        u32::from_str_radix(a, 16).unwrap(),
                        u8::from_str_radix(v, 16).unwrap(),
                    )
                })
                .collect();
            tongue(&args[1], level, &tiles, &pokes)
        }
        Some("berry") => {
            let option = |name: &str| {
                args[4..]
                    .iter()
                    .find_map(|a| a.strip_prefix(name)?.strip_prefix('='))
                    .map(|v| u16::from_str_radix(v, 16).unwrap())
            };
            let level = option("level").unwrap_or(0x105);
            let layer2 = option("layer") == Some(2);
            let other = option("other");
            let (pokes, tiles): (Vec<_>, Vec<_>) = args[4..]
                .iter()
                .filter(|a| {
                    !a.starts_with("level=") && !a.starts_with("layer=") && !a.starts_with("other=")
                })
                .partition(|a| a.contains('='));
            let tiles: Vec<u16> = tiles
                .iter()
                .map(|t| u16::from_str_radix(t, 16).unwrap())
                .collect();
            let pokes: Vec<(u32, u8)> = pokes
                .iter()
                .map(|p| {
                    let (a, v) = p.split_once('=').unwrap();
                    (
                        u32::from_str_radix(a, 16).unwrap(),
                        u8::from_str_radix(v, 16).unwrap(),
                    )
                })
                .collect();
            let (x, y) = (args[2].parse().unwrap(), args[3].parse().unwrap());
            berry(&args[1], level, layer2, other, x, y, &tiles, &pokes)
        }
        _ => {
            eprintln!(
                "usage: contact_probe place in.mwl out.mwl x y | run rom.sfc [x y] \
             | stand rom.sfc level x y tile... | berry rom.sfc x y tile... [level=n] [layer=2] [other=tile] | tongue rom.sfc tile... [level=n]"
            );
            std::process::exit(2)
        }
    }
}

fn place(input: &str, output: &str, x: u16, y: u16) {
    let bytes = std::fs::read(input).unwrap();
    let mut mwl = MwlFile::parse(&bytes).unwrap().decode(None).unwrap();
    // One tile: HHHHWWWW, 00BBBBBB, bbbbbbbb.
    mwl.layer1.data.objects.push(Object::Lunar {
        number: 0x27,
        x,
        y,
        data: vec![0x00, 0x02, 0x00],
    });
    std::fs::write(output, mwl.to_file(None).unwrap().to_bytes()).unwrap();
}

fn run(path: &str, tile_x: i32, tile_y: i32) {
    let rom = Rom::load(path).unwrap();
    let (bx, by) = (tile_x * 16, tile_y * 16);
    // Tile $200 in the grid (horizontal, 27 rows a screen), as Lunar
    // Magic's object puts it and a ROM without that object code cannot.
    let index = (tile_x / 16 * 0x1B0 + tile_y * 16 + tile_x % 16) as u32;
    let place = |ram: &mut ram::Ram| {
        ram.set_u8(ram::RamAddr::new(0x7E_C800 + index), 0x00);
        ram.set_u8(ram::RamAddr::new(0x7F_C800 + index), 0x02);
    };
    // Name, and the player's position and speeds on the first frame.
    let scenarios: [(&str, i32, i32, i8, i8); 12] = [
        ("fall onto", bx, by - 40, 0, 0x30),
        ("jump into from below", bx, by + 20, 0, -0x50),
        ("run into left side", bx - 20, by - 12, 0x30, 0),
        ("run into right side", bx + 20, by - 12, -0x30, 0),
        ("one foot on, right", bx + 8, by - 34, 0, 0x10),
        ("one foot on, left", bx - 8, by - 34, 0, 0x10),
        ("walk off right", bx + 2, by - 32, 0x18, 0),
        ("walk off left", bx - 2, by - 32, -0x18, 0),
        ("stand on right edge", bx + 13, by - 32, 0, 0x10),
        ("stand on left edge", bx - 13, by - 32, 0, 0x10),
        ("overlap", bx, by - 16, 0, 0),
        ("overlap low", bx, by - 8, 0, 0),
    ];
    for (powerup, size) in [(0u8, "small"), (1, "big")] {
        for (name, x, y, x_speed, y_speed) in scenarios {
            let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
                if frame == 0 {
                    place(ram);
                    ram.set_u8(ram::RamAddr::new(0x7E_0019), powerup);
                    ram.set_u16(ram::PLAYER_X, x as u16);
                    ram.set_u16(ram::PLAYER_Y, y as u16);
                    ram.set_u8(ram::PLAYER_X_SPEED, x_speed as u8);
                    ram.set_u8(ram::PLAYER_Y_SPEED, y_speed as u8);
                    ram.set_u8(ram::RamAddr::new(LOG), 0);
                }
            })
            .unwrap();
            println!("{size:5} {name:22} {}", log(&ram).join(" "));
        }
    }
    // Sprites: slot 0 alone, the player out of the way. Number, status
    // (1 to start, $0A a kicked shell), position, speeds.
    let sprites: [(&str, u8, u8, i32, i32, i8, i8); 4] = [
        ("goomba dropped on", 0x0F, 0x01, bx, by - 20, 0, 0x10),
        ("shell into left side", 0x04, 0x0A, bx - 20, by, 0x30, 0),
        ("shell into right side", 0x04, 0x0A, bx + 20, by, -0x30, 0),
        ("shell up into", 0x04, 0x0A, bx, by + 20, 0, -0x40),
    ];
    for (name, number, status, x, y, x_speed, y_speed) in sprites {
        let ram = expand::play_level(&rom, 0x105, 16, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                ram.set_u16(ram::PLAYER_X, (bx + 0x80) as u16);
                ram.set_u8_at(ram::SPRITE_NUMBER, 0, number);
                ram.set_u8_at(ram::SPRITE_STATUS, 0, status);
                ram.set_u8_at(ram::SPRITE_X_LOW, 0, x as u8);
                ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (x >> 8) as u8);
                ram.set_u8_at(ram::SPRITE_Y_LOW, 0, y as u8);
                ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, (y >> 8) as u8);
                ram.set_u8_at(ram::RamAddr::new(0x7E_00B6), 0, x_speed as u8);
                ram.set_u8_at(ram::RamAddr::new(0x7E_00AA), 0, y_speed as u8);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
        })
        .unwrap();
        println!("sprite {name:22} {}", log(&ram).join(" "));
    }
    // The cape, spinning beside the block, and a fireball thrown into it
    // (extended sprite 5, in one of the player's two slots).
    for (name, side) in [
        ("cape spin, left of it", -12i32),
        ("cape spin, right of it", 12),
    ] {
        let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.set_u8(ram::RamAddr::new(0x7E_0019), 2);
                ram.set_u16(ram::PLAYER_X, (bx + side) as u16);
                ram.set_u16(ram::PLAYER_Y, (by - 16) as u16);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
            ram.set_u8(ram::RamAddr::new(0x7E_14A6), 0x12);
        })
        .unwrap();
        println!("cape   {name:22} {}", log(&ram).join(" "));
    }
    for (name, x, y, speed, fall) in [
        ("fireball from the left", bx - 24, by + 4, 0x30i8, 0i8),
        ("fireball from the right", bx + 24, by + 4, -0x30, 0),
        ("fireball in it", bx + 4, by + 4, 0x30, 0),
        ("fireball onto it", bx + 4, by - 12, 0x10, 0x30),
    ] {
        let ram = expand::play_level(&rom, 0x105, 12, |frame, ram| {
            if frame == 0 {
                place(ram);
                ram.set_u16(ram::PLAYER_X, (bx + 0x80) as u16);
                ram.set_u8(ram::RamAddr::new(0x7E_170B + 8), 5);
                ram.set_u8(ram::RamAddr::new(0x7E_171F + 8), x as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1733 + 8), (x >> 8) as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1715 + 8), y as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1729 + 8), (y >> 8) as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_1747 + 8), speed as u8);
                ram.set_u8(ram::RamAddr::new(0x7E_173D + 8), fall as u8);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
        })
        .unwrap();
        println!("fire   {name:22} {}", log(&ram).join(" "));
    }
}

/// Where the game keeps the tile at block (x, y) of a layer, as its
/// block lookups find it (`DATA_00BA60` and the tables after it): a
/// horizontal layer is screens of 16 columns side by side, a vertical one
/// screens of 16 rows, two to a row.
fn cell(rom: &Rom, layer2: bool, vertical: bool, x: u32, y: u32) -> u32 {
    let table = |at: u32, i: u32| rom.read_u8(SnesAddr::new(at + i)).unwrap() as u32;
    let within = (y & 0x0F) << 4 | (x & 0x0F);
    let (screen, high) = if vertical {
        (y >> 4, x >> 4)
    } else {
        (x >> 4, y >> 4)
    };
    let (low_table, high_table) = match (vertical, layer2) {
        (false, false) => (0x00BA60, 0x00BA9C),
        (false, true) => (0x00BA70, 0x00BAAC),
        (true, false) => (0x00BA80, 0x00BABC),
        (true, true) => (0x00BA8E, 0x00BACA),
    };
    let start = table(high_table, screen) << 8 | table(low_table, screen);
    0x7E_0000 | (start + within + (high << 8))
}

#[allow(clippy::too_many_arguments)]
fn berry(
    path: &str,
    level: u16,
    layer2: bool,
    other: Option<u16>,
    tile_x: i32,
    tile_y: i32,
    tiles: &[u16],
    pokes: &[(u32, u8)],
) {
    let rom = Rom::load(path).unwrap();
    let mode = kobo_core::level::read_primary_header(&rom, level)
        .unwrap()
        .level_mode;
    let vertical = |layer2| {
        if layer2 {
            mode.layer2() == kobo_core::level::Layer2Kind::VerticalObjects
        } else {
            mode.layer1_vertical()
        }
    };
    let low = cell(&rom, layer2, vertical(layer2), tile_x as u32, tile_y as u32);
    let high = low + 0x1_0000;
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    // Yoshi where its centre, moved by layer 2's offset from layer 1
    // (`$26`, `$28`) when the tile is on layer 2, is in the tile.
    let hold = |ram: &mut ram::Ram| {
        let (dx, dy) = if layer2 {
            (ram.u16(at(0x26)) as i32, ram.u16(at(0x28)) as i32)
        } else {
            (0, 0)
        };
        let x = (tile_x * 16 - dx) as u16;
        let y = (tile_y * 16 - dy) as u16;
        ram.set_u8_at(ram::SPRITE_X_LOW, 0, x as u8);
        ram.set_u8_at(ram::SPRITE_X_HIGH, 0, (x >> 8) as u8);
        ram.set_u8_at(ram::SPRITE_Y_LOW, 0, y as u8);
        ram.set_u8_at(ram::SPRITE_Y_HIGH, 0, (y >> 8) as u8);
    };
    let frames: u32 = std::env::var("KOBO_BERRY_FRAMES").map_or(16, |v| v.parse().unwrap());
    let place: u32 = std::env::var("KOBO_BERRY_PLACE").map_or(0, |v| v.parse().unwrap());
    for &tile in tiles {
        let ram = expand::play_level(&rom, level, frames, |frame, ram| {
            if frame == place {
                let [low_byte, high_byte] = tile.to_le_bytes();
                ram.set_u8(ram::RamAddr::new(low), low_byte);
                ram.set_u8(ram::RamAddr::new(high), high_byte);
                if let Some(other) = other {
                    // The other layer's tile where Yoshi checks: layer 1
                    // where layer 2's (x, y) is, or the other way round.
                    let sign = if layer2 { -1 } else { 1 };
                    let dx = ram.u16(at(0x26)) as i16 as i32 * sign;
                    let dy = ram.u16(at(0x28)) as i16 as i32 * sign;
                    let x = ((tile_x * 16 + 8 + dx) >> 4) as u32;
                    let y = ((tile_y * 16 + 8 + dy) >> 4) as u32;
                    let at = cell(&rom, !layer2, vertical(!layer2), x, y);
                    let [low_byte, high_byte] = other.to_le_bytes();
                    ram.set_u8(ram::RamAddr::new(at), low_byte);
                    ram.set_u8(ram::RamAddr::new(at + 0x1_0000), high_byte);
                }
            }
            if frame == 0 {
                ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
                ram.set_u16(ram::PLAYER_X, (tile_x * 16 + 0x60) as u16);
                ram.set_u8_at(ram::SPRITE_NUMBER, 0, 0x2D);
                // Stunned, which is where it checks for berries.
                ram.set_u8_at(ram::SPRITE_STATUS, 0, 0x09);
                ram.set_u8(at(0x18D6), 0);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
                // What InitSpriteTables would load: its tweaker bytes,
                // from the game's tables (Sprite1656Vals and on).
                for (table, values) in [
                    (0x1656, 0x07F26C),
                    (0x1662, 0x07F335),
                    (0x166E, 0x07F3FE),
                    (0x167A, 0x07F4C7),
                    (0x1686, 0x07F590),
                    (0x190F, 0x07F659),
                ] {
                    let v = rom.read_u8(SnesAddr::new(values + 0x2D)).unwrap();
                    ram.set_u8(at(table), v);
                }
                let palette = rom.read_u8(SnesAddr::new(0x07F3FE + 0x2D)).unwrap();
                ram.set_u8(at(0x15F6), palette & 0x0F);
            }
            for &(addr, value) in pokes {
                ram.set_u8(ram::RamAddr::new(addr), value);
            }
            // Held in place over the tile, stunned.
            hold(ram);
        })
        .unwrap();
        let sprites: Vec<String> = (0..12)
            .filter(|&i| ram.u8_at(ram::SPRITE_STATUS, i) != 0)
            .map(|i| {
                let x = u16::from_le_bytes([
                    ram.u8_at(ram::SPRITE_X_LOW, i),
                    ram.u8_at(ram::SPRITE_X_HIGH, i),
                ]);
                let y = u16::from_le_bytes([
                    ram.u8_at(ram::SPRITE_Y_LOW, i),
                    ram.u8_at(ram::SPRITE_Y_HIGH, i),
                ]);
                format!(
                    "{:02X}:{:02X}@{x:04X},{y:04X}",
                    ram.u8_at(ram::SPRITE_NUMBER, i),
                    ram.u8_at(ram::SPRITE_STATUS, i)
                )
            })
            .collect();
        let left = u16::from_le_bytes([
            ram.u8(ram::RamAddr::new(low)),
            ram.u8(ram::RamAddr::new(high)),
        ]);
        println!(
            "{tile:04X}: berry {:02X}, sprites {}, tile now {left:04X}, $1693 = {:02X}, $1933 = {:02X}, log {}",
            ram.u8(at(0x18D6)),
            sprites.join(" "),
            ram.u8(at(0x1693)),
            ram.u8(at(0x1933)),
            ram.bytes(ram::RamAddr::new(LOG), 5)
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Ok(path) = std::env::var("KOBO_BERRY_DUMP") {
            let bytes = kobo_core::clean_room::bytes(&ram, ram::RamAddr::new(0x7E_0000), 0x2_0000);
            std::fs::write(format!("{path}-{tile:04X}.bin"), bytes).unwrap();
        }
    }
}

fn stand(path: &str, level: u16, tile_x: i32, tile_y: i32, tiles: &[u16], pokes: &[(u32, u8)]) {
    let rom = Rom::load(path).unwrap();
    let vertical = kobo_core::level::read_primary_header(&rom, level)
        .unwrap()
        .level_mode
        .layer1_vertical();
    let index = if vertical {
        (tile_y * 16 + tile_x) as u32
    } else {
        (tile_x / 16 * 0x1B0 + tile_y * 16 + tile_x % 16) as u32
    };
    for &tile in tiles {
        let ram = expand::play_level(&rom, level, 16, |frame, ram| {
            if frame == 0 {
                let [low, high] = tile.to_le_bytes();
                ram.set_u8(ram::RamAddr::new(0x7E_C800 + index), low);
                ram.set_u8(ram::RamAddr::new(0x7F_C800 + index), high);
                ram.set_u16(ram::PLAYER_X, (tile_x * 16) as u16);
                ram.set_u16(ram::PLAYER_Y, (tile_y * 16 - 40) as u16);
                ram.set_u8(ram::PLAYER_Y_SPEED, 0x30);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
            }
            for &(addr, value) in pokes {
                ram.set_u8(ram::RamAddr::new(addr), value);
            }
        })
        .unwrap();
        let on_ground = ram.u8(ram::RamAddr::new(0x7E_13EF)) != 0;
        let y = ram.u16(ram::PLAYER_Y) as i32 - (tile_y * 16 - 32);
        println!(
            "{tile:04X}: {} (y {y:+}), $1693 = {:02X} {}",
            if on_ground { "landed" } else { "in the air" },
            ram.u8(ram::RamAddr::new(0x7E_1693)),
            log(&ram).join(" ")
        );
    }
}

/// The probe block's log: each action once, with where the game sampled
/// relative to the player (or, for a sprite, where it sampled).
fn log(ram: &ram::Ram) -> Vec<String> {
    let mut seen = Vec::new();
    for entry in 0..ram.u8(ram::RamAddr::new(LOG)) as u32 {
        let byte = |k: u32| ram.u8(ram::RamAddr::new(LOG + 1 + 16 * entry + k)) as i32;
        let word = |k: u32| byte(k) | byte(k + 1) << 8;
        let (touch_x, touch_y) = (word(3), word(1));
        let (player_x, player_y) = (word(5), word(7));
        let text = format!(
            "{}@({:+},{:+})",
            ACTIONS.get(byte(0) as usize).unwrap_or(&"(unknown)"),
            touch_x - player_x,
            touch_y - player_y
        );
        if !seen.contains(&text) {
            seen.push(text);
        }
    }
    seen
}

/// Yoshi's tongue into a tile: `level` entered with Yoshi (`$0DC1`
/// set, so the player starts on him); at frame `mount`, `tile` is written
/// over the four columns from one ahead of the player and the three rows
/// from the player's, and the player presses Y. The RAM after `frames`
/// frames, and the cells written.
fn tongue_ram(
    rom: &Rom,
    level: u16,
    tile: u16,
    mount: u32,
    frames: u32,
    pokes: &[(u32, u8)],
) -> (ram::Ram, Vec<u32>) {
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    let vertical = kobo_core::level::read_primary_header(rom, level)
        .unwrap()
        .level_mode
        .layer1_vertical();
    let cell = |x: u32, y: u32| cell(rom, false, vertical, x, y) - 0x7E_C800;
    let mut cells = Vec::new();
    let ram = expand::play_level_entered(
        rom,
        level,
        |ram| ram.set_u8(at(0x0DC1), 1),
        frames,
        |frame, ram| {
            if std::env::var_os("KOBO_TONGUE_TRACE").is_some() {
                eprintln!(
                    "frame {frame}: player {} {} riding {:02X} sprites {:?} tongue {:02X}/{:02X} {:02X} berry {:02X}",
                    ram.u16(ram::PLAYER_X),
                    ram.u16(ram::PLAYER_Y),
                    ram.u8(at(0x187A)),
                    (0..12)
                        .filter(|&i| ram.u8_at(ram::SPRITE_STATUS, i) != 0)
                        .map(|i| ram.u8_at(ram::SPRITE_NUMBER, i))
                        .collect::<Vec<_>>(),
                    ram.u8(at(0x1594)),
                    ram.u8(at(0x151C)),
                    ram.u8(at(0x160E)),
                    ram.u8(at(0x18D6)),
                );
            }
            if frame == mount {
                let x = ram.u16(ram::PLAYER_X) as u32 / 16;
                let y = (ram.u16(ram::PLAYER_Y) as u32 + 16) / 16;
                for cx in x + 1..=x + 4 {
                    for cy in y..=y + 2 {
                        let i = cell(cx, cy);
                        let [low, high] = tile.to_le_bytes();
                        ram.set_u8(ram::RamAddr::new(0x7E_C800 + i), low);
                        ram.set_u8(ram::RamAddr::new(0x7F_C800 + i), high);
                        cells.push(i);
                    }
                }
                ram.set_u8(at(0x18D6), 0);
                ram.set_u8(ram::RamAddr::new(LOG), 0);
                // Y, newly pressed.
                ram.set_u8(at(0x16), 0x40);
                ram.set_u8(at(0x15), 0x40);
            }
            for &(addr, value) in pokes {
                ram.set_u8(ram::RamAddr::new(addr), value);
            }
        },
    )
    .unwrap();
    (ram, cells)
}

fn tongue(path: &str, level: u16, tiles: &[u16], pokes: &[(u32, u8)]) {
    let rom = Rom::load(path).unwrap();
    let at = |a: u32| ram::RamAddr::new(0x7E_0000 | a);
    let mount: u32 = std::env::var("KOBO_TONGUE_MOUNT").map_or(8, |v| v.parse().unwrap());
    let frames: u32 = std::env::var("KOBO_TONGUE_FRAMES").map_or(40, |v| v.parse().unwrap());
    for &tile in tiles {
        let (ram, cells) = tongue_ram(&rom, level, tile, mount, frames, pokes);
        let left: Vec<String> = cells
            .iter()
            .map(|&i| {
                format!(
                    "{:02X}{:02X}",
                    ram.u8(ram::RamAddr::new(0x7F_C800 + i)),
                    ram.u8(ram::RamAddr::new(0x7E_C800 + i))
                )
            })
            .collect();
        let sprites: Vec<String> = (0..12)
            .filter(|&i| ram.u8_at(ram::SPRITE_STATUS, i) != 0)
            .map(|i| {
                format!(
                    "{:02X}:{:02X}",
                    ram.u8_at(ram::SPRITE_NUMBER, i),
                    ram.u8_at(ram::SPRITE_STATUS, i)
                )
            })
            .collect();
        println!(
            "{tile:04X}: riding {:02X}, berry {:02X}, sprites {}, tiles {}, $1693 = {:02X}, log {}",
            ram.u8(at(0x187A)),
            ram.u8(at(0x18D6)),
            sprites.join(" "),
            left.join(" "),
            ram.u8(at(0x1693)),
            ram.bytes(ram::RamAddr::new(LOG), 5)
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Ok(path) = std::env::var("KOBO_BERRY_DUMP") {
            let bytes = kobo_core::clean_room::bytes(&ram, ram::RamAddr::new(0x7E_0000), 0x2_0000);
            std::fs::write(format!("{path}-tongue-{tile:04X}.bin"), bytes).unwrap();
        }
    }
}
