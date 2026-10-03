//! Layer 3's position, and a measurement of how it follows the camera.

use super::ExpandError;
use super::machine::{Call, Machine};
use super::routines;
use crate::ram::{self, Ram};
use crate::video::Layer3;

/// Layer 3 as level preparation left it, plus how it follows the camera.
/// The latter is measured rather than decoded: the ROM's per-frame layer
/// scroll routine runs three times from the prepared state, with the
/// camera where it is and then moved 16 pixels along each axis, the way
/// the camera update hands it a frame's movement. Tides, the tileset
/// backgrounds' half-speed parallax, autoscrolling fish, sprite-driven
/// layers, and custom scroll code hooked into the routine all come out
/// of the same measurement. `None` when layer 3 is not shown on either
/// screen in Mode 1.
pub(super) fn capture_layer3(machine: &mut Machine) -> Result<Option<Layer3>, ExpandError> {
    let ram = &machine.bus.ram;
    let bg_mode = ram.u8(ram::BG_MODE);
    let shown = machine.bus.screen_layers[0] | machine.bus.screen_layers[1];
    if bg_mode & 0x07 != 1 || shown & 0x04 == 0 {
        return Ok(None);
    }
    let layer3_position = |ram: &Ram| [ram.u16(ram::LAYER3_X), ram.u16(ram::LAYER3_Y)];
    let position = layer3_position(ram);
    let camera = [ram.u16(ram::LAYER1_X), ram.u16(ram::LAYER1_Y)];
    let saved = ram.clone();
    let mut scrolled = |delta: [u16; 2]| {
        let ram = &mut machine.bus.ram;
        ram.clone_from(&saved);
        // The entrance locks sprites, which also pauses autoscroll.
        ram.set_u8(ram::SPRITE_LOCK, 0);
        let axes = [
            (ram::LAYER1_X, ram::NEXT_LAYER1_X, ram::LAYER1_DX),
            (ram::LAYER1_Y, ram::NEXT_LAYER1_Y, ram::LAYER1_DY),
        ];
        for (axis, (now, next, moved)) in axes.into_iter().enumerate() {
            let at = camera[axis].wrapping_add(delta[axis]);
            ram.set_u16(now, at);
            ram.set_u16(next, at);
            ram.set_u8(moved, delta[axis] as u8);
        }
        machine.call(Call::jsl(routines::SCROLL_LAYERS))?;
        Ok::<_, ExpandError>(layer3_position(&machine.bus.ram))
    };
    let still = scrolled([0, 0])?;
    let moved_x = scrolled([16, 0])?;
    let moved_y = scrolled([0, 16])?;
    machine.bus.ram = saved;
    Ok(Some(Layer3 {
        position,
        camera,
        scroll_per_16: [
            moved_x[0].wrapping_sub(still[0]) as i16 as i32,
            moved_y[1].wrapping_sub(still[1]) as i16 as i32,
        ],
        tilemap: machine.bus.bg_sc[2],
        character_base: machine.bus.bg_character_base[2],
        bg_mode,
    }))
}
