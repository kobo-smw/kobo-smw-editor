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
use super::machine::{Call, Machine};
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
    /// `BG1SC`-`BG4SC` and the layers' character bases (bytes), as the
    /// load left them: where in VRAM each layer's tilemap and characters
    /// are.
    pub bg_sc: [u8; 4],
    pub bg_character_base: [u16; 4],
    /// Frames from power-on.
    pub frames: u32,
}

/// The events passed (`OWEventsActivated`, a bit per event, `$78` of them),
/// which a new game clears and the player select copies from the save
/// buffer (`CopyFromSaveBuffer`) just before the load applies them, as a
/// hack's own file select may do otherwise.
const EVENTS_PASSED: RamAddr = RamAddr::new(0x7E_1F02);
const EVENT_BYTES: usize = 0x0F;

/// Loads the overworld as a new game does.
pub fn load_overworld(rom: &Rom) -> Result<LoadedOverworld, ExpandError> {
    load_overworld_passed(rom, &[0; EVENT_BYTES])
}

/// [`load_overworld`], with the events `passed` (a bit per event, the
/// highest first in each byte, as the game keeps them) held passed from
/// power-on, as a saved game's are: the load applies each one's changes.
pub fn load_overworld_passed(
    rom: &Rom,
    passed: &[u8; EVENT_BYTES],
) -> Result<LoadedOverworld, ExpandError> {
    let (machine, frames) = load(rom, passed)?;
    Ok(loaded(&machine, frames))
}

/// [`load_overworld`] with the players on `submap` (0 the main map), so
/// that the load puts up its graphics and palette: the overworld as a
/// player there sees it.
pub fn load_overworld_on(rom: &Rom, submap: u8) -> Result<LoadedOverworld, ExpandError> {
    load_overworld_on_passed(rom, submap, &[0; EVENT_BYTES])
}

/// [`load_overworld_on`] with the events `passed` held passed, as
/// [`load_overworld_passed`] has them.
pub fn load_overworld_on_passed(
    rom: &Rom,
    submap: u8,
    passed: &[u8; EVENT_BYTES],
) -> Result<LoadedOverworld, ExpandError> {
    let mut pins = vec![(PLAYER_SUBMAPS, submap), (PLAYER_SUBMAPS_2, submap)];
    if passed.iter().any(|&b| b != 0) {
        pins.extend(
            passed
                .iter()
                .enumerate()
                .map(|(i, &b)| (RamAddr::new(EVENTS_PASSED.vanilla() + i as u32), b)),
        );
    }
    let (machine, frames) = load_pinned(rom, &pins)?;
    Ok(loaded(&machine, frames))
}

/// Each player's submap (`OWPlayerSubmap`).
const PLAYER_SUBMAPS: RamAddr = RamAddr::new(0x7E_1F11);
const PLAYER_SUBMAPS_2: RamAddr = RamAddr::new(0x7E_1F12);

/// The game's last step of event `event` in play (`CODE_04E9EC`, the
/// event process's eighth state, which makes the event's further tiles
/// and marks it passed), on the overworld [`load_overworld_passed`]
/// loads: what it leaves.
pub fn end_event(
    rom: &Rom,
    passed: &[u8; EVENT_BYTES],
    event: u8,
) -> Result<LoadedOverworld, ExpandError> {
    let (mut machine, frames) = load(rom, passed)?;
    machine.bus.pinned.clear();
    machine.bus.ram.set_u8(OVERWORLD_EVENT, event);
    machine.bus.ram.set_u8(EVENT_PROCESS, 7);
    machine.call(Call::jsr(routines::END_EVENT))?;
    Ok(loaded(&machine, frames))
}

/// A level beaten in play: on the overworld [`load_overworld_passed`]
/// loads, the player is put on the level tile at `place` (submap, x and y
/// in 16x16 tiles) as though they had left its level by `exit` (1 the
/// normal exit, 2 the secret one: `OWLevelExitMode`), and the game goes
/// back to the overworld (game mode `$0B`) and runs `frames` frames of
/// it, in which it plays the exit's event. What it leaves, and the
/// overworld's process and its event process (`OverworldProcess`,
/// `OverworldEventProcess`) of each frame.
pub fn beat_level(
    rom: &Rom,
    passed: &[u8; EVENT_BYTES],
    place: (u8, u8, u8),
    exit: u8,
    frames: u32,
) -> Result<Beaten, ExpandError> {
    Ok(beat_levels(rom, passed, &[(place, exit)], frames)?.remove(0))
}

/// What a level beaten leaves, and the overworld's and its event
/// process's steps, frame by frame.
pub type Beaten = (LoadedOverworld, Vec<(u8, u8)>);

/// [`beat_level`] for each place and exit, from one load.
pub fn beat_levels(
    rom: &Rom,
    passed: &[u8; EVENT_BYTES],
    beaten: &[((u8, u8, u8), u8)],
    frames: u32,
) -> Result<Vec<Beaten>, ExpandError> {
    let (mut loaded, loaded_at) = load(rom, passed)?;
    loaded.bus.pinned.clear();
    beaten
        .iter()
        .map(|&(place, exit)| {
            let mut machine = loaded.clone();
            beat(&mut machine, rom, place, exit, frames)
                .map(|steps| (self::loaded(&machine, loaded_at + frames), steps))
        })
        .collect()
}

fn beat(
    machine: &mut Machine,
    rom: &Rom,
    place: (u8, u8, u8),
    exit: u8,
    frames: u32,
) -> Result<Vec<(u8, u8)>, ExpandError> {
    let (submap, x, y) = place;
    let ram = &mut machine.bus.ram;
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    ram.set_u8(at(0x0DD6), 0);
    ram.set_u8(at(0x1F11), submap);
    for (word, value) in [
        (0x1F17, u16::from(x) * 16),
        (0x1F19, u16::from(y) * 16),
        (0x1F1F, u16::from(x)),
        (0x1F21, u16::from(y)),
    ] {
        ram.set_u8(at(word), value as u8);
        ram.set_u8(at(word + 1), (value >> 8) as u8);
    }
    ram.set_u8(at(0x0DD5), exit);
    // What the level and its goal leave: the level passed (`MidwayFlag`),
    // the event to make (`CreditsScreenNumber`), and which, the level's
    // (`OverworldEvent`, from `DATA_05D608` by translevel, as the level
    // load takes it; the secret exit's is the next).
    ram.set_u8(at(0x13CE), 1);
    ram.set_u8(at(0x1DE9), 1);
    let index = u32::from(submap != 0) * 0x400
        + u32::from(y >> 4) * 0x200
        + u32::from(x >> 4) * 0x100
        + u32::from(y & 15) * 16
        + u32::from(x & 15);
    let translevel = ram.u8(RamAddr::new(0x7E_D000 + index));
    ram.set_u8(at(0x13BF), translevel);
    let event = rom
        .read_u8(LEVEL_EVENTS.add(u32::from(translevel)))
        .map_err(|_| ExpandError::Overworld {
            mode: 0x0E,
            frames: 0,
        })?;
    let ram = &mut machine.bus.ram;
    ram.set_u8(at(0x1DEA), event);
    ram.set_u8(ram::GAME_MODE, 0x0B);
    let mut steps = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        run_game_mode(machine, 0)?;
        machine.bus.pad = 0;
        machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
        vertical_blank(machine, 0)?;
        let ram = &machine.bus.ram;
        steps.push((ram.u8(OVERWORLD_PROCESS), ram.u8(EVENT_PROCESS)));
    }
    Ok(steps)
}

/// Where a warp takes a player on the warp tile at `place` (submap, x and
/// y in 16x16 tiles) of the overworld [`load_overworld`] loads: the game's
/// lookup (`CODE_048509`, the warp's index in `StarWarpIndex`) and its
/// move (`CODE_04853B`), as a star or pipe runs them. `None` where no warp
/// is; else the submap and the place in pixels the player is put at.
pub fn warp(
    rom: &Rom,
    places: &[(u8, u8, u8)],
) -> Result<Vec<Option<(u8, u16, u16)>>, ExpandError> {
    let (mut loaded, _) = load(rom, &[0; EVENT_BYTES])?;
    loaded.bus.pinned.clear();
    let at = |a: u32| RamAddr::new(0x7E_0000 | a);
    let word = |ram: &Ram, a: u32| u16::from(ram.u8(at(a))) | u16::from(ram.u8(at(a + 1))) << 8;
    places
        .iter()
        .map(|&(submap, x, y)| {
            let mut machine = loaded.clone();
            let ram = &mut machine.bus.ram;
            ram.set_u8(at(0x0DD6), 0);
            ram.set_u8(at(0x0DB3), 0);
            ram.set_u8(at(0x1F11), submap);
            for (a, value) in [
                (0x1F17, u16::from(x) * 16),
                (0x1F19, u16::from(y) * 16),
                (0x1F1F, u16::from(x)),
                (0x1F21, u16::from(y)),
            ] {
                ram.set_u8(at(a), value as u8);
                ram.set_u8(at(a + 1), (value >> 8) as u8);
            }
            machine.call(Call::jsr(routines::WARP_LOOKUP).data_bank(0x04))?;
            if machine.bus.ram.u8(at(0x1DF6)) >= 0x80 {
                return Ok(None);
            }
            machine.call(Call::jsl(routines::WARP_MOVE).data_bank(0x04))?;
            let ram = &machine.bus.ram;
            Ok(Some((
                ram.u8(at(0x13C3)),
                word(ram, 0x1F17),
                word(ram, 0x1F19),
            )))
        })
        .collect()
}

/// Each translevel's event (`DATA_05D608`), which the level load gives the
/// overworld.
const LEVEL_EVENTS: crate::addr::SnesAddr = crate::addr::SnesAddr::new(0x05_D608);

/// The overworld's state (`OverworldProcess`).
const OVERWORLD_PROCESS: RamAddr = RamAddr::new(0x7E_13D9);

/// The event an event process is making (`OverworldEvent`), and the
/// process's state (`OverworldEventProcess`).
const OVERWORLD_EVENT: RamAddr = RamAddr::new(0x7E_1DEA);
const EVENT_PROCESS: RamAddr = RamAddr::new(0x7E_1B86);

fn loaded(machine: &Machine, frames: u32) -> LoadedOverworld {
    let bus = &machine.bus;
    LoadedOverworld {
        ram: bus.ram.clone(),
        vram: bus.vram.clone(),
        cgram: bus.cgram.clone(),
        bg_sc: bus.bg_sc,
        bg_character_base: bus.bg_character_base,
        frames,
    }
}

/// The machine at the first frame of game mode `$0E`, and the frames to it.
fn load<'r>(rom: &'r Rom, passed: &[u8; EVENT_BYTES]) -> Result<(Machine<'r>, u32), ExpandError> {
    let pins: Vec<(RamAddr, u8)> = if passed.iter().any(|&b| b != 0) {
        passed
            .iter()
            .enumerate()
            .map(|(i, &b)| (RamAddr::new(EVENTS_PASSED.vanilla() + i as u32), b))
            .collect()
    } else {
        Vec::new()
    };
    load_pinned(rom, &pins)
}

/// [`load`], with `pins` held from power-on (`SmwBus::pinned`).
fn load_pinned<'r>(
    rom: &'r Rom,
    pins: &[(RamAddr, u8)],
) -> Result<(Machine<'r>, u32), ExpandError> {
    let mut machine = Machine::new(rom, 0);
    machine.bus.pinned = pins.to_vec();
    for &(at, b) in pins {
        machine.bus.ram.set_u8(at, b);
    }
    machine.run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)?;
    let mut started = false;
    for frame in 0..FRAME_LIMIT {
        let mode = machine.bus.ram.u8(ram::GAME_MODE);
        if mode == 0x0E {
            return Ok((machine, frame));
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
