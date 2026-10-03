//! The video-mode bands and object artwork of a Mode 7 boss arena.

use super::ExpandError;
use super::machine::{Call, Interrupt, Machine};
use super::oam::{self, SCREEN_H};
use super::routines;
use crate::cpu::smw_bus::SmwBus;
use crate::ram;
use crate::video::{Band, BossScene, Layer1Registers, Window};

/// Bands of a picture: the status bar, the arena, and its floor.
const MAX_BANDS: usize = 3;

fn layer1(bus: &SmwBus) -> Layer1Registers {
    Layer1Registers {
        mode: bus.bg_mode,
        tilemap: bus.bg_sc[0],
        character_base: bus.bg_character_base[0],
        scroll: bus.bg_scroll[0],
        mode7: bus.mode7,
    }
}

/// Runs a frame of the arena and the interrupts that follow it. The ROM
/// chooses its tilemap, graphics base, Mode 7 transform, and IRQ
/// scanlines: each handler leaves the registers of the band below its
/// line. The drawing pass populates the sprite-based arena artwork; its
/// RAM changes are isolated from the captured collision grid. `None`
/// for a level that is not an arena.
pub(super) fn capture_boss_scene(machine: &mut Machine) -> Result<Option<BossScene>, ExpandError> {
    let command = machine.bus.ram.u8(ram::IRQ_NMI_COMMAND);
    if command & 0x80 == 0 {
        return Ok(None);
    }
    let saved = machine.bus.ram.clone();
    machine.bus.ram.set_u8(ram::GAME_MODE, 0x14);
    machine.call(Call::jsr(routines::DRAW_LEVEL_FRAME))?;
    // The frame's vertical blank: the handler uploads the player's and
    // the boss's tiles and OAM, and sets the top band's registers.
    machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
    machine.interrupt(Interrupt::Nmi)?;
    let (oam, first_object) = oam::uploaded(&machine.bus);
    let object_select = machine.bus.object_select;
    let objects = oam::screen_objects(&oam, first_object, oam::object_sizes(object_select));
    let mut bands = vec![Band {
        start: 0,
        layer: layer1(&machine.bus),
    }];
    // The handlers arm the next IRQ themselves: the ceiling line from
    // the NMI, the floor line from the ceiling's IRQ, none from Bowser's.
    while machine.bus.interrupt_enable & 0x20 != 0 && bands.len() < MAX_BANDS {
        let start = machine.bus.irq_scanline as usize;
        machine.interrupt(Interrupt::TimerIrq)?;
        bands.push(Band {
            start,
            layer: layer1(&machine.bus),
        });
    }
    let ram = &machine.bus.ram;
    let window = Window {
        rows: ram
            .bytes(ram::WINDOW_TABLE, SCREEN_H as usize * 2)
            .as_chunks::<2>()
            .0
            .to_vec(),
        window2: machine.bus.window2,
        masks: machine.bus.window_masks,
        select: std::array::from_fn(|i| ram.u8_at(ram::WINDOW_SELECT, i as u32)),
        logic: machine.bus.window_logic,
    };
    machine.bus.ram = saved;
    Ok(Some(BossScene {
        bands,
        window,
        objects,
        object_select,
    }))
}
