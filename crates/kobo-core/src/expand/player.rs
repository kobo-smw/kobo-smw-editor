//! The player as the game draws him at the level's entrance.

use super::diagnostics::{Diagnostic, Pass};
use super::load_flags::LoadFlags;
use super::machine::Machine;
use super::oam;
use crate::cpu::CpuError;
use crate::cpu::smw_bus::SmwBus;
use crate::ram;
use crate::video::SpriteObject;

/// Most frames the player pass runs waiting for an entrance action to end.
const PLAYER_FRAMES: usize = 128;
/// The OAM slots `DrawMarioAndYoshi` (`CODE_01EA70`) fills, at `$0300`-
/// `$031F`: the player and, when he rides, Yoshi. Everything else the game
/// draws in a frame (cluster sprites such as the castle candle flames and
/// the ghost house Boo ceilings the loader spawned) uses other slots.
const PLAYER_OAM_SLOTS: std::ops::Range<usize> = 64..72;

/// The player at the entrance: one level frame after another from the
/// prepared state, with every sprite slot cleared and every level sprite
/// marked as already loaded so nothing else spawns, until his entrance
/// action (`$71`) has finished. Each frame runs the ROM's whole NMI,
/// uploading player graphics and animated tiles and palettes, including
/// the dragon coin's flashing colour. The bus keeps those character and
/// palette uploads; RAM and the tilemaps are restored to the loader's
/// ([`Tilemaps`]). Only his own OAM slots are read: the cluster
/// sprites the loader spawned are still drawn in this pass, and the
/// sprite passes already capture them. They are found in the image as
/// drawn ([`oam::Frame`]), since SA-1 Pack moves every object at the end
/// of the frame. The objects come back in level coordinates from the
/// camera the pass ended with.
///
/// The level loop runs the hack's per-level code, which can be broken
/// (Invictus level 136's stops the pass). The level itself
/// has loaded by then; it goes without a player, and says so in
/// `diagnostics`.
pub(super) fn capture_player(
    machine: &mut Machine,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<SpriteObject> {
    let saved = machine.bus.ram.clone();
    let tilemaps = Tilemaps::take(&machine.bus);
    let objects = match enter(machine) {
        Ok(objects) => objects,
        Err(error) => {
            diagnostics.push(Diagnostic::Cpu {
                pass: Pass::Player,
                error,
            });
            Vec::new()
        }
    };
    machine.bus.ram = saved;
    tilemaps.restore(&mut machine.bus);
    objects
}

/// The three background tilemaps as the loader left them in VRAM. The
/// entrance frames run the level's own per-frame code, which can move a
/// layer and have the game upload tilemap rows for the new position
/// (Akogare2 level `111` pans layer 2 upward from its second dozen
/// frames, and the rows follow it). The picture is
/// drawn at the positions the loader's RAM holds, so the tilemaps have to
/// be the loader's as well; only the character and palette uploads the
/// frames make are kept.
struct Tilemaps {
    /// (first VRAM byte, bytes, whether each was written) per layer.
    areas: Vec<(usize, Vec<u8>, Vec<bool>)>,
}

impl Tilemaps {
    fn take(bus: &SmwBus) -> Self {
        let len = bus.vram.len();
        let areas = bus.bg_sc[..3]
            .iter()
            .map(|&sc| {
                let base = ((sc >> 2) as usize) << 11;
                let size = 0x800 * (1 + (sc & 1) as usize) * (1 + ((sc >> 1) & 1) as usize);
                let bytes = (0..size).map(|i| bus.vram[(base + i) % len]).collect();
                let written = (0..size)
                    .map(|i| bus.vram_written[(base + i) % len])
                    .collect();
                (base, bytes, written)
            })
            .collect();
        Self { areas }
    }

    fn restore(&self, bus: &mut SmwBus) {
        let len = bus.vram.len();
        for (base, bytes, written) in &self.areas {
            for (i, (&byte, &was)) in bytes.iter().zip(written).enumerate() {
                bus.vram[(base + i) % len] = byte;
                bus.vram_written[(base + i) % len] = was;
            }
        }
    }
}

fn enter(machine: &mut Machine) -> Result<Vec<SpriteObject>, CpuError> {
    let load_flags = LoadFlags::detect(&mut machine.bus);
    let ram = &mut machine.bus.ram;
    ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
    load_flags.fill(ram, 1);
    ram.set_u8(ram::SPRITE_GENERATOR, 0);
    ram.set_u8(ram::GAME_MODE, 0x14);
    let mut frame = oam::draw_frame(machine)?;
    for _ in 1..PLAYER_FRAMES {
        if machine.bus.ram.u8(ram::PLAYER_ANIMATION) == 0 {
            break;
        }
        frame = oam::draw_frame(machine)?;
    }
    let (image, first) = frame.uploaded_from(PLAYER_OAM_SLOTS);
    let ram = &machine.bus.ram;
    let camera = (
        ram.u16(ram::LAYER1_X) as i16 as i32,
        ram.u16(ram::LAYER1_Y) as i16 as i32,
    );
    let sizes = oam::object_sizes(machine.bus.object_select);
    Ok(oam::screen_objects(&image, first, sizes)
        .into_iter()
        .map(|object| object.translated(camera.0, camera.1))
        .collect())
}
