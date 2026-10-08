//! The overworld as the ROM's own code loads it: from power-on through the
//! title screen and a new game on the file select, with the overworld
//! override (`$0109`) held at 0 so the game goes there rather than to the
//! intro level, until game mode `$0E` (the overworld running). What it
//! leaves is what an overworld reader must agree with, whatever code a
//! hack has put in the game's place (docs/lunar-magic-install.md, "The
//! overworld"); `tools/oracle/dump_overworld.lua` does the same in an
//! emulator. Nothing of the code that runs is traced or kept.

use super::ExpandError;
use super::load::{RESET_STEP_LIMIT, run_game_mode, vertical_blank};
use super::machine::Machine;
use super::routines;
use crate::ram::{self, Ram, RamAddr};
use crate::rom::Rom;

/// Controller 1's Start, in `$4219`.
const START: u16 = 0x1000;
/// Controller 1's A, in `$4218`.
const A: u16 = 0x0080;
/// The overworld override, which a new game sets to the intro level.
const OVERWORLD_OVERRIDE: RamAddr = RamAddr::new(0x7E_0109);
/// Frames the title screen, the file select, and the load may take.
const FRAME_LIMIT: u32 = 3000;

/// What the overworld's load left: work RAM and video memory on the first
/// frame of game mode `$0E`.
pub struct LoadedOverworld {
    pub ram: Ram,
    pub vram: Vec<u8>,
    pub cgram: Vec<u8>,
    /// Frames from power-on.
    pub frames: u32,
}

/// Loads the overworld as a new game does.
pub fn load_overworld(rom: &Rom) -> Result<LoadedOverworld, ExpandError> {
    let mut machine = Machine::new(rom, 0);
    machine.run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)?;
    let mut started = false;
    for frame in 0..FRAME_LIMIT {
        let mode = machine.bus.ram.u8(ram::GAME_MODE);
        if mode == 0x0E {
            let bus = &machine.bus;
            return Ok(LoadedOverworld {
                ram: bus.ram.clone(),
                vram: bus.vram.clone(),
                cgram: bus.cgram.clone(),
                frames: frame,
            });
        }
        started |= mode >= 0x0A;
        if started && mode < 0x0E {
            machine.bus.ram.set_u8(OVERWORLD_OVERRIDE, 0);
        }
        run_game_mode(&mut machine, 0)?;
        // A press lasts one frame in eight, so the game sees each.
        let press = match mode {
            0x07 => START,
            0x08..=0x0A => A,
            _ => 0,
        };
        machine.bus.pad = if frame % 8 == 0 { press } else { 0 };
        machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
        vertical_blank(&mut machine, 0)?;
    }
    Err(ExpandError::Overworld {
        mode: machine.bus.ram.u8(ram::GAME_MODE),
        frames: FRAME_LIMIT,
    })
}
