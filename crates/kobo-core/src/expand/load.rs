//! Loading a level by running the ROM's own loader, phase by phase.

use super::machine::{Call, Interrupt, Machine};
use super::tiles::{
    GRID_LEN, LAYER2_TILEMAP_LEN, LevelTiles, SCREEN_COLS, SCREEN_LEN, SCREEN_ROWS,
};
use super::{ExpandError, LoadedLevel, boss, layer3, map16, player, routines};
use crate::level::{self, Layer2Kind, LevelMode};
use crate::operation::{Operation, Stage};
use crate::palette::Color15;
use crate::ram::{self, Ram};
use crate::rom::Rom;
use crate::video::{LevelScene, Screen, VideoMemory};

/// What the frame counter `$13` holds when a level's first frame runs
/// (see `load_level`).
pub const ENTRY_FRAME_COUNTER: u8 = 0x40;

/// Instruction limit for the reset code, which uploads the SPC engine.
const RESET_STEP_LIMIT: u64 = 200_000_000;

/// Runs the ROM's level loader and level preparation for `level`.
pub fn expand_level(rom: &Rom, level: u16) -> Result<LoadedLevel, ExpandError> {
    expand_level_traced(rom, level, false).map(|(t, _)| t)
}

/// A data read: (address of the reading instruction, address read). The
/// instruction is `None` once a ROM Lunar Magic saved has been loaded,
/// since it would show where its code is (clean room); the address read
/// is a memory effect.
pub type ReadTrace = Vec<(Option<u32>, u32)>;

/// Like [`expand_level`], optionally recording every data read the
/// loader made after reset.
pub fn expand_level_traced(
    rom: &Rom,
    level: u16,
    trace: bool,
) -> Result<(LoadedLevel, Option<ReadTrace>), ExpandError> {
    expand_controlled(rom, level, trace, None)
}

/// Loads with cancellation, progress, and a budget shared by all passes.
pub fn expand_level_with_control(
    rom: &Rom,
    level: u16,
    operation: &Operation,
) -> Result<LoadedLevel, ExpandError> {
    let level = expand_controlled(rom, level, false, Some(operation))?.0;
    operation.stage(Stage::Finished)?;
    Ok(level)
}

pub(crate) fn expand_controlled(
    rom: &Rom,
    level: u16,
    trace: bool,
    operation: Option<&Operation>,
) -> Result<(LoadedLevel, Option<ReadTrace>), ExpandError> {
    if let Some(op) = operation {
        op.stage(Stage::Boot)?;
    }
    let header = level::read_primary_header(rom, level)?;
    let mut machine = Machine::new(rom, level);
    machine.bus.operation = operation.cloned();
    boot(&mut machine)?;
    if trace {
        machine.cpu.trace_data_reads(true);
    }
    if let Some(op) = operation {
        op.stage(Stage::Loading)?;
    }
    let expanded = load_level(&mut machine)?;
    if let Some(op) = operation {
        op.stage(Stage::Preparing)?;
    }
    let [main, sub] = prepare_level(&mut machine)?;
    let trace = machine.cpu.trace_data_reads(false);

    let boss = boss::capture_boss_scene(&mut machine)?;
    let ram = &machine.bus.ram;
    let camera = [ram.u16(ram::LAYER1_X), ram.u16(ram::LAYER1_Y)];
    let layer2_position = [ram.u16(ram::LAYER2_X), ram.u16(ram::LAYER2_Y)];
    let screen = Screen {
        main,
        sub,
        color_math: ram.u8(ram::COLOR_MATH),
        math_select: ram.u8(ram::COLOR_MATH_SELECT),
        fixed_color: Color15(ram.u16(ram::BACKGROUND_COLOR)),
    };
    let mut diagnostics = Vec::new();
    let (layer3, player) = if boss.is_some() {
        // The arena's drawing pass already includes the player.
        (None, Vec::new())
    } else {
        (
            layer3::capture_layer3(&mut machine)?,
            player::capture_player(&mut machine, &mut diagnostics),
        )
    };
    let lunar_magic = rom.lunar_magic_version().is_some();
    let (bg_map16, bg_map16_at, layer2_screen_len) = match &expanded.layer2_tilemap {
        Some(planes) => {
            let (tiles, at, len) = map16::read_bg_map16(&mut machine, planes)?;
            (tiles, Some(at), len)
        }
        None => (Vec::new(), None, SCREEN_LEN),
    };
    let map16 = map16::lookup_map16(&mut machine, map16::routine_in_use(rom))?;
    let pipe_map16 = (!lunar_magic).then(|| map16::read_pipe_map16(&mut machine.bus));
    if let Some(op) = operation {
        op.check()?;
    }
    let bus = machine.bus;
    if !bus.unsupported.is_empty() {
        diagnostics.push(super::Diagnostic::Unsupported(bus.unsupported.clone()));
    }
    let tiles = LevelTiles {
        level,
        header,
        level_mode: LevelMode(bus.ram.u8(ram::LEVEL_MODE)),
        object_tileset: bus.ram.u8(ram::OBJECT_TILESET),
        vertical: expanded.vertical,
        screens: expanded.screens,
        rows: expanded.rows,
        low: bus.ram.bytes(ram::TILES_LOW, GRID_LEN),
        high: bus.ram.bytes(ram::TILES_HIGH, GRID_LEN),
        layer2_tilemap: expanded.layer2_tilemap,
        layer2_screen_len,
        map16,
        pipe_map16,
        bg_map16,
        bg_map16_at,
    };
    let level = LoadedLevel {
        tiles,
        video: VideoMemory {
            vram: bus.vram,
            vram_written: bus.vram_written,
            cgram: bus.cgram,
            bg_sc: bus.bg_sc,
            object_select: bus.object_select,
        },
        scene: LevelScene {
            screen,
            camera,
            layer2_position,
            layer3,
            player,
            boss,
        },
        ram: bus.ram,
        diagnostics,
    };
    Ok((level, trace))
}

/// The first `len` bytes of GFX file `index` (`00` to `31`) as the ROM's
/// own code decompresses it, whatever routine a hack has put in the
/// game's place. This is what [`crate::gfx`]'s decoders are checked
/// against; a level's graphics always come this way.
pub fn decompress_gfx_file(rom: &Rom, index: u8, len: usize) -> Result<Vec<u8>, ExpandError> {
    let mut machine = Machine::new(rom, 0);
    let failed = |source| ExpandError::Gfx { index, source };
    machine
        .run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)
        .map_err(|error| match error {
            ExpandError::Cpu { source, .. } => failed(source),
            other => other,
        })?;
    machine
        .try_call(Call::jsl(routines::DECOMPRESS_GFX_FILE).index_y(index as u16))
        .map_err(failed)?;
    Ok(machine.bus.ram.bytes(ram::GFX_BUFFER, len))
}

/// Plays `level`: loads and prepares it as [`expand_level`] does, then
/// runs `frames` level frames (game mode `$14`, each with its NMI), calling
/// `each` with the frame number and the RAM before every one, to put the
/// player somewhere or hold a value. Returns the RAM after the last. For
/// checking what the game's code does in play, which a level load does
/// not run: block contact, tile changes, scrolling.
pub fn play_level(
    rom: &Rom,
    level: u16,
    frames: u32,
    each: impl FnMut(u32, &mut ram::Ram),
) -> Result<ram::Ram, ExpandError> {
    play_level_entered(rom, level, |_| {}, frames, each)
}

/// [`play_level`], with `entry` changing the RAM the level is entered
/// with first: [`expand_level`] enters a level as a screen exit on screen
/// 0 would (sublevel count 1), which some settings do not apply to.
pub fn play_level_entered(
    rom: &Rom,
    level: u16,
    entry: impl FnOnce(&mut ram::Ram),
    frames: u32,
    mut each: impl FnMut(u32, &mut ram::Ram),
) -> Result<ram::Ram, ExpandError> {
    let mut machine = Machine::new(rom, level);
    boot(&mut machine)?;
    load_level_with(&mut machine, entry)?;
    prepare_level(&mut machine)?;
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x14);
    for frame in 0..frames {
        each(frame, &mut machine.bus.ram);
        super::oam::draw_frame(&mut machine)
            .map_err(|source| ExpandError::Cpu { level, source })?;
    }
    Ok(machine.bus.ram)
}

/// What a frame of [`play_game_loop`] left: the RAM and the video memory.
pub struct PlayedFrame<'a> {
    pub ram: &'a ram::Ram,
    pub vram: &'a [u8],
    pub cgram: &'a [u8],
    pub bg_sc: [u8; 4],
    /// The scroll registers as last written (`BGnHOFS`, `BGnVOFS`).
    pub bg_scroll: [[u16; 2]; 4],
    /// `TM` and `TS` as last written.
    pub screen_layers: [u8; 2],
}

/// Plays `level` as [`play_level`] does, but each frame through the game
/// loop itself (`JSR RunGameMode` at `$008072`, which patches reroute, up
/// to `STZ $10` after it), then its NMI and IRQs. `each` runs before a
/// frame and may change RAM; `after` sees RAM and VRAM once they have run. For
/// checking code that hooks the game loop or the NMI's uploads, such as a
/// VRAM patch's tilemap streaming.
pub fn play_game_loop(
    rom: &Rom,
    level: u16,
    frames: u32,
    each: impl FnMut(u32, &mut ram::Ram),
    after: impl FnMut(u32, PlayedFrame),
) -> Result<(), ExpandError> {
    play_game_loop_from(rom, level, frames, |_| {}, |_| {}, each, after)
}

/// [`play_game_loop`], with `entry` changing the RAM the level's load
/// starts from (as [`play_level_entered`]'s does), and `start` seeing the
/// level as its preparation left it, before the first frame.
pub fn play_game_loop_from(
    rom: &Rom,
    level: u16,
    frames: u32,
    entry: impl FnOnce(&mut ram::Ram),
    start: impl FnOnce(PlayedFrame),
    each: impl FnMut(u32, &mut ram::Ram),
    after: impl FnMut(u32, PlayedFrame),
) -> Result<(), ExpandError> {
    play_frames(rom, level, frames, entry, start, |_| false, each, after)
}

/// [`play_game_loop`], with the frames `lag` names lagging: the game
/// loop's frame takes longer than a frame, so an NMI comes while it is
/// still running (`$10` set, the game's lag path) before the one that
/// ends it. `after` sees each vertical blank, so a lagging frame twice.
/// For checking code that hooks the NMI's lag path.
pub fn play_game_loop_lagging(
    rom: &Rom,
    level: u16,
    frames: u32,
    lag: impl FnMut(u32) -> bool,
    each: impl FnMut(u32, &mut ram::Ram),
    after: impl FnMut(u32, PlayedFrame),
) -> Result<(), ExpandError> {
    play_frames(rom, level, frames, |_| {}, |_| {}, lag, each, after)
}

#[allow(clippy::too_many_arguments)]
fn play_frames(
    rom: &Rom,
    level: u16,
    frames: u32,
    entry: impl FnOnce(&mut ram::Ram),
    start: impl FnOnce(PlayedFrame),
    mut lag: impl FnMut(u32) -> bool,
    mut each: impl FnMut(u32, &mut ram::Ram),
    mut after: impl FnMut(u32, PlayedFrame),
) -> Result<(), ExpandError> {
    let mut machine = Machine::new(rom, level);
    boot(&mut machine)?;
    load_level_with(&mut machine, entry)?;
    prepare_level(&mut machine)?;
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x14);
    start(played(&machine));
    for frame in 0..frames {
        each(frame, &mut machine.bus.ram);
        run_game_mode(&mut machine, level)?;
        if lag(frame) {
            // A vertical blank that comes before the loop's STZ $10: what
            // the screen shows while the frame lags.
            vertical_blank(&mut machine, level)?;
            after(frame, played(&machine));
        }
        machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
        vertical_blank(&mut machine, level)?;
        after(frame, played(&machine));
    }
    Ok(())
}

/// The game loop's frame, up to where it waits for the vertical blank.
fn run_game_mode(machine: &mut Machine, level: u16) -> Result<(), ExpandError> {
    const RUN_GAME_MODE: u32 = 0x00_8072;
    const AFTER_GAME_MODE: u32 = 0x00_8075;
    machine.bus.ram.set_u8(ram::LAG_FLAG, 1);
    // Stopped where the loop goes on, or not: the frame is done either way.
    machine
        .try_call_to(Call::jsr(RUN_GAME_MODE), Some(AFTER_GAME_MODE))
        .map_err(|source| ExpandError::Cpu { level, source })?;
    Ok(())
}

/// What [`play_game_loop`]'s callers see of the machine.
fn played<'a>(machine: &'a Machine) -> PlayedFrame<'a> {
    PlayedFrame {
        ram: &machine.bus.ram,
        vram: &machine.bus.vram,
        cgram: &machine.bus.cgram,
        bg_sc: machine.bus.bg_sc,
        bg_scroll: machine.bus.bg_scroll,
        screen_layers: machine.bus.screen_layers,
    }
}

/// A frame's NMI and IRQs (the status bar's split, which sets layer 3's
/// scroll and colour math for the rest of the screen), as the game loop
/// left `$10`.
fn vertical_blank(machine: &mut Machine, level: u16) -> Result<(), ExpandError> {
    machine
        .try_interrupt(Interrupt::Nmi)
        .map_err(|source| ExpandError::Cpu { level, source })?;
    let mut irqs = 0;
    while machine.bus.interrupt_enable & 0x20 != 0 && irqs < 4 {
        machine
            .try_interrupt(Interrupt::TimerIrq)
            .map_err(|source| ExpandError::Cpu { level, source })?;
        irqs += 1;
    }
    Ok(())
}

/// The registers [`call_in_level`] enters a routine with, and those it
/// returns: the 8- or 16-bit sizes are the `M` and `X` bits of `p`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Registers {
    pub a: u16,
    pub x: u16,
    pub y: u16,
    pub p: u8,
    pub db: u8,
}

/// Loads and prepares `level` as [`play_level`] does, lets `setup` change
/// the RAM, then calls the routine at `addr` (with `JSL` when `long`, else
/// `JSR` from its own bank) with `regs`, and returns the RAM and registers
/// it left. For learning what one routine of the game, or a hook in it,
/// does with chosen inputs, from memory effects alone. Once a ROM Lunar
/// Magic saved has been loaded (clean room) the registers are `None`,
/// since what its code leaves in them is not a memory effect, and `addr`
/// must be the game's code or a hook's first byte, as `vanilla` tells
/// ([`crate::clean_room::may_call`]).
pub fn call_in_level(
    rom: &Rom,
    vanilla: &Rom,
    level: u16,
    setup: impl FnOnce(&mut ram::Ram),
    addr: u32,
    long: bool,
    regs: Registers,
) -> Result<(ram::Ram, Option<Registers>), ExpandError> {
    call_after_frames(
        rom,
        vanilla,
        level,
        0,
        |_, _| {},
        setup,
        addr,
        long,
        regs,
        false,
    )
    .map(|called| (called.ram, called.registers))
}

/// What [`call_after_frames`] leaves: the RAM and registers as the routine
/// returned them (registers `None` under the clean room, as
/// [`call_in_level`]'s), and VRAM after the vertical blank that follows,
/// which uploads what the routine queued.
pub struct Called {
    pub ram: ram::Ram,
    pub registers: Option<Registers>,
    pub vram: Vec<u8>,
}

/// [`call_in_level`] after `frames` frames of the game loop, as
/// [`play_game_loop`] plays them, `each` changing the RAM before each; and
/// VRAM after the next vertical blank, with one more frame of the game loop
/// before it (`then_frame`, for what a patch does with what a frame left
/// once the frame ends, at `$008072`).
#[allow(clippy::too_many_arguments)]
pub fn call_after_frames(
    rom: &Rom,
    vanilla: &Rom,
    level: u16,
    frames: u32,
    mut each: impl FnMut(u32, &mut ram::Ram),
    setup: impl FnOnce(&mut ram::Ram),
    addr: u32,
    long: bool,
    regs: Registers,
    then_frame: bool,
) -> Result<Called, ExpandError> {
    const M: u8 = 0x20;
    const X: u8 = 0x10;
    let mut machine = Machine::new(rom, level);
    if crate::clean_room::forbidden()
        && !crate::clean_room::may_call(rom, vanilla, crate::addr::SnesAddr::new(addr))
    {
        return Err(ExpandError::Cpu {
            level,
            source: crate::cpu::CpuError::Forbidden,
        });
    }
    boot(&mut machine)?;
    load_level(&mut machine)?;
    prepare_level(&mut machine)?;
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x14);
    for frame in 0..frames {
        each(frame, &mut machine.bus.ram);
        run_game_mode(&mut machine, level)?;
        machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
        vertical_blank(&mut machine, level)?;
    }
    setup(&mut machine.bus.ram);
    let mut call = if long {
        Call::jsl(addr)
    } else {
        Call::jsr(addr)
    };
    if regs.p & M == 0 {
        call = call.wide_accumulator();
    }
    if regs.p & X == 0 {
        call = call.wide_index();
    }
    call = call
        .data_bank(regs.db)
        .accumulator(regs.a)
        .index_x(regs.x)
        .index_y(regs.y);
    machine.call(call)?;
    let ram = machine.bus.ram.clone();
    let cpu = &machine.cpu;
    let out = (!crate::clean_room::forbidden()).then_some(Registers {
        a: cpu.a,
        x: cpu.x,
        y: cpu.y,
        p: cpu.p,
        db: cpu.db,
    });
    if then_frame {
        run_game_mode(&mut machine, level)?;
    }
    machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
    vertical_blank(&mut machine, level)?;
    Ok(Called {
        ram,
        registers: out,
        vram: machine.bus.vram.to_vec(),
    })
}

/// Where a screen exit leads, as the ROM's own entrance code resolves it:
/// after booting, a level whose screen 0 has the exit `low` (its
/// destination's low byte) and `high` (what the exit object leaves in
/// `$19D8`: bit 0 the destination's bit 8, and in Lunar Magic's format bit
/// 1 secondary and bit 2 the format itself), with `secondary` in `$1B93`
/// as the game's format sets it and the player on submap `submap`, and
/// whatever else `set` puts in RAM last (the level being left, say, or a
/// sublevel count of 0 and `$0109` for an entry from the overworld), runs
/// `CODE_05D796` and returns the RAM it leaves: the level in `$0E`-`$0F`,
/// an entrance's settings, the player's and layers' positions. For
/// learning what an entrance hook does, and checking Kobo's against it.
pub fn enter_by_exit(
    rom: &Rom,
    low: u8,
    high: u8,
    secondary: bool,
    submap: u8,
    set: impl FnOnce(&mut ram::Ram),
) -> Result<ram::Ram, ExpandError> {
    let mut machine = Machine::new(rom, 0);
    boot(&mut machine)?;
    let ram = &mut machine.bus.ram;
    ram.set_u8(ram::SUBLEVEL_COUNT, 1);
    ram.set_u8(ram::EXIT_TABLE_LOW, low);
    ram.set_u8(ram::EXIT_TABLE_HIGH, high);
    ram.set_u8(ram::RamAddr::new(0x7E_1B93), secondary as u8);
    ram.set_u8(ram::OW_PLAYER_SUBMAP, submap);
    ram.set_u8(ram::GAME_MODE, 0x11);
    set(ram);
    machine.call(Call::jsl(routines::LOAD_HEADER_POINTERS))?;
    Ok(machine.bus.ram)
}

/// The definitions of Map16 tiles as the ROM's own code finds them once
/// `level` has loaded: through the Map16 routine where the uploads call it
/// (Lunar Magic's layout), else from the game's pointer table, which holds
/// pages 0 and 1 only (`None` past them).
pub fn resolve_map16(
    rom: &Rom,
    level: u16,
    tiles: &[u16],
) -> Result<Vec<Option<crate::map16::Map16Tile>>, ExpandError> {
    let mut machine = Machine::new(rom, level);
    boot(&mut machine)?;
    load_level(&mut machine)?;
    let routine = map16::routine_in_use(rom);
    tiles
        .iter()
        .map(|&n| map16::lookup_one(&mut machine, routine, n))
        .collect()
}

/// Brings the machine to where the game is when a level load starts.
/// The reset code runs up to the main loop: it builds the RAM-resident
/// OAM reset routine, uploads the SPC engine (against a stub that echoes
/// the handshake), and clears memory; patches hooked into it run as well.
/// Then come the layer 3 tiles, which the game uploads once on the
/// "Nintendo Presents" screen and which survive every level load.
fn boot(machine: &mut Machine) -> Result<(), ExpandError> {
    machine.run_from_reset(routines::GAME_LOOP, RESET_STEP_LIMIT)?;
    machine.call(Call::jsr(routines::CLEAR_LAYER3))?;
    machine.call(Call::jsr(routines::UPLOAD_LAYER3_GFX))
}

/// What has to be read right after `LoadLevel`, before level preparation
/// overwrites it.
struct Expanded {
    screens: usize,
    vertical: bool,
    rows: usize,
    layer2_tilemap: Option<(Vec<u8>, Vec<u8>)>,
}

/// Game mode `$11`: resolves the level's header pointers, places the
/// player and the camera at the entrance, and expands the level's
/// objects into the tile grid.
fn load_level(machine: &mut Machine) -> Result<Expanded, ExpandError> {
    load_level_with(machine, |_| {})
}

/// [`load_level`], with `set` changing the RAM the entry starts from (a
/// sublevel count of 0 for an entry from the overworld, say) before the
/// entrance code runs.
fn load_level_with(
    machine: &mut Machine,
    set: impl FnOnce(&mut ram::Ram),
) -> Result<Expanded, ExpandError> {
    // Enter the level the way a screen exit on screen 0 would. The
    // overworld path cannot express every level number through `$0109`,
    // loads the "No Yoshi" entrance intro room for castle and ghost house
    // tilesets, and is rerouted by some Lunar Magic versions. The high
    // byte is given in both the vanilla form (the player's submap) and
    // Lunar Magic's exit table form.
    let [lo, hi] = machine.level.to_le_bytes();
    let ram = &mut machine.bus.ram;
    ram.set_u8(ram::SUBLEVEL_COUNT, 1);
    ram.set_u8(ram::EXIT_TABLE_LOW, lo);
    ram.set_u8(ram::EXIT_TABLE_HIGH, 0x04 | hi);
    ram.set_u8(ram::OW_PLAYER_SUBMAP, hi);
    // The frame counter has counted every vertical blank since power-on
    // by the time a player enters a level, so its value there is any at
    // all. Straight from reset it is zero, which is the one value that
    // fires the rarest of the game's periodic events on the level's first
    // frame: Lakitu's cloud throws a Spiny when the counter's low seven
    // bits are clear (`$01E98D`), and vanilla test level `132` gained two
    // Spinies that no emulator entry shows. Bit 6 alone moves that event
    // 192 frames off, further than any pass runs, and leaves every shorter
    // period (`$3F` down to `$01`, animation phases among them) as it was.
    ram.set_u8(ram::TRUE_FRAME, ENTRY_FRAME_COUNTER);
    set(ram);
    // Run each phase with the game mode the real machine would be in,
    // and the vertical blank between them: game mode `$10` ends by
    // setting `$11`, so the console runs the NMI once with the mode at
    // `$11` before the frame that loads the level.
    ram.set_u8(ram::GAME_MODE, 0x11);
    end_frame(machine)?;
    machine.call(Call::jsl(routines::LOAD_HEADER_POINTERS))?;
    // Game mode $11 seeds the camera update's previous positions from the
    // entrance, sets the player up, and places the layers for the entry
    // camera before it loads anything: `LOAD_LEVEL_DATA` ends by spawning
    // the sprites around that camera. The screen count is still the
    // maximum then, and vertical scrolling at will is on, which lets the
    // update bring the camera to the player at once where the header's
    // position would not show him (vertical level `12A` starts 192
    // pixels higher for it).
    let ram = &mut machine.bus.ram;
    for i in 0..ram::LAYER_POSITIONS_LEN {
        let value = ram.u8_at(ram::LAYER1_X, i);
        ram.set_u8_at(ram::NEXT_LAYER1_X, i, value);
    }
    machine.call(Call::jsr(routines::INIT_LEVEL_RAM))?;
    // The ROM's own bytes for the screen count's maximum and layer 2's
    // first position, run in place: Lunar Magic's code there sets up more
    // of the entry.
    run_in_place(
        machine,
        routines::GM11_MAX_SCREENS,
        routines::GM11_LAYER2_END,
    )?;
    machine.bus.ram.set_u8(ram::SCROLL_AT_WILL, 1);
    machine.call(Call::jsl(routines::UPDATE_CAMERA))?;
    machine.call(Call::jsl(routines::LOAD_LEVEL_DATA))?;
    let ram = &machine.bus.ram;
    let vertical = ram.u8(ram::SCREEN_MODE) & 0x01 != 0;
    let expanded = Expanded {
        // Boss preparation reuses the screen-count byte (level $1C7 ends
        // with $FF). Read the length while it still describes the grid.
        screens: ram.u8(ram::SCREENS) as usize,
        vertical,
        rows: level_rows(ram, vertical),
        // Level preparation decompresses GFX files into `$7EAD00`, and
        // Lunar Magic's 4bpp files overrun the vanilla 3bpp buffer into
        // the background at `$7EB900`. The game has uploaded the tilemap
        // to VRAM by then, so it does not care; we do.
        layer2_tilemap: (LevelMode(ram.u8(ram::LEVEL_MODE)).layer2() == Layer2Kind::Background)
            .then(|| {
                (
                    ram.bytes(ram::LAYER2_TILEMAP_LOW, LAYER2_TILEMAP_LEN),
                    ram.bytes(ram::LAYER2_TILEMAP_HIGH, LAYER2_TILEMAP_LEN),
                )
            }),
    };
    // Game mode `$11` ends by moving on to `$12`; then comes the
    // vertical blank that ends the loading frame.
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x12);
    end_frame(machine)?;
    Ok(expanded)
}

/// Runs the ROM's code from `from` to the instruction at `to`, inside a
/// routine of the game's whose other steps the loader calls one by one:
/// with a game-mode frame's registers (8-bit, data bank `$00`), entered as
/// if by `JSR` and left open where it reaches `to`. Code that returns
/// before it gets there has run whole.
fn run_in_place(machine: &mut Machine, from: u32, to: u32) -> Result<(), ExpandError> {
    machine
        .try_call_to(Call::jsr(from), Some(to))
        .map(|_| ())
        .map_err(|source| machine.error(source))
}

/// The vertical blank at the end of a game-mode frame: the ROM's whole
/// NMI handler, as the console runs it between two passes of the game
/// loop, with the lag flag clear so that it does its uploads. Code a
/// hack hooks into the handler runs here with the game mode the frame
/// left, and some needs it (QLDC 2021 `70_DPBOX` loads wrong without).
fn end_frame(machine: &mut Machine) -> Result<(), ExpandError> {
    machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
    machine.interrupt(Interrupt::Nmi)
}

/// All of game mode `$12`: this is what draws boss arenas, sets up layer
/// 3, and uploads GFX and palettes. Then the camera update the level loop
/// starts with.
fn prepare_level(machine: &mut Machine) -> Result<[u8; 2], ExpandError> {
    let ram = &mut machine.bus.ram;
    ram.fill(ram::LOADED_GFX_FILES, ram::LOADED_GFX_FILES_LEN, 0xFF);
    machine.call(Call::jsr(routines::DECOMPRESS_PLAYER_GFX))?;
    // `GM12PrepLevel` ($00A59C) through the game loop's call of the game
    // mode, as the game runs it, so that a patch hooking the loop there
    // sees the frame end (Lunar Magic's VRAM patch puts the frame's changed
    // tiles in place then).
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x12);
    let level = machine.level;
    run_game_mode(machine, level)?;
    // The level's own screen designation is what preparation set
    // (`ScreenSettings`); a Mode 7 arena's handlers change the layers
    // between the status bar and the playfield, and a hack's handlers
    // may write a band's designation of their own (Super Hark Bros 2
    // puts layer 3 on the main screen there, which a frame later is
    // otherwise again). So read it before the frame's vertical blank.
    let screen_layers = machine.bus.screen_layers;
    // The vertical blank that ends the preparation frame.
    end_frame(machine)?;
    // The level loop updates the camera before anything is shown, which
    // is what settles the layer 2 position: code a hack runs during
    // preparation may have moved it (Super Hark Bros 2 level `00A` leaves
    // it at `$5D`; the update derives `$C0` from the camera again, which
    // is where the game had uploaded the background for).
    machine.call(Call::jsl(routines::UPDATE_CAMERA))?;
    Ok(screen_layers)
}

/// Rows per screen of the loaded level. Lunar Magic 3's expanded level
/// format stores a horizontal level's height in `$13D7`; vanilla and
/// older Lunar Magic ROMs leave it zero. Anything that does not describe
/// whole rows fitting the planes is treated as the vanilla 27.
fn level_rows(ram: &Ram, vertical: bool) -> usize {
    if vertical {
        return 16;
    }
    let height = ram.u16(ram::LEVEL_HEIGHT) as usize;
    match height / 16 {
        rows if height.is_multiple_of(16) && rows > 0 && rows * SCREEN_COLS <= GRID_LEN => rows,
        _ => SCREEN_ROWS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ram::RamMap;

    #[test]
    fn level_rows_come_from_the_reported_height() {
        let mut ram = Ram::new(RamMap::Vanilla);
        assert_eq!(level_rows(&ram, false), SCREEN_ROWS);
        assert_eq!(level_rows(&ram, true), 16);
        ram.set_u16(ram::LEVEL_HEIGHT, 0x0280);
        assert_eq!(level_rows(&ram, false), 40);
        ram.set_u16(ram::LEVEL_HEIGHT, 0x0288); // not whole rows
        assert_eq!(level_rows(&ram, false), SCREEN_ROWS);
    }
}
