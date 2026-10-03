//! Reading the objects a frame drew out of the game's OAM image.

use super::machine::{Call, Interrupt, Machine};
use super::routines;
use crate::cpu::CpuError;
use crate::cpu::smw_bus::{self, SmwBus};
use crate::ram::{self, Ram};
use crate::video::SpriteObject;

/// Screen size the sprite engine draws within.
pub(super) const SCREEN_W: i32 = 256;
pub(super) const SCREEN_H: i32 = 224;
/// The Y position the game parks unused OAM objects at.
pub(super) const HIDDEN_Y: u8 = 0xF0;
/// Objects in OAM, and the bytes of the image: four per object, then the
/// size and X-high bits packed four objects to a byte.
pub(super) const OAM_OBJECTS: usize = 128;
pub(super) const OAM_LEN: usize = smw_bus::OAM_LEN;

/// Small and large object dimensions for an `OBSEL` value.
pub fn object_sizes(object_select: u8) -> [(i32, i32); 2] {
    [
        [(8, 8), (16, 16)],
        [(8, 8), (32, 32)],
        [(8, 8), (64, 64)],
        [(16, 16), (32, 32)],
        [(16, 16), (64, 64)],
        [(32, 32), (64, 64)],
        [(16, 32), (32, 64)],
        [(16, 32), (32, 32)],
    ][(object_select >> 5) as usize]
}

/// An OAM image and the object the PPU draws in front of the others.
pub(super) type OamImage = (Vec<u8>, usize);

/// Object memory as the ROM's NMI upload left it. The ROM's own upload decides
/// which object comes first: vanilla turns priority rotation on and
/// starts from `$3F`, SA-1 Pack leaves it off.
pub(super) fn uploaded(bus: &SmwBus) -> OamImage {
    (bus.oam.clone(), bus.first_object())
}

/// A frame of the level loop, as the game drew it and as the PPU got it.
pub(super) struct Frame {
    /// The game's OAM image when the frame got to the end-of-frame OAM
    /// routine. That is where to look for whatever draws to fixed objects
    /// (the player, the candle flames): SA-1 Pack's MaxTile rebuilds the
    /// whole image there, in priority order. A frame that never gets
    /// there (a message box is up, or a patch has rerouted the end of the
    /// frame) gives the image it left.
    pub drawn: Vec<u8>,
    pub uploaded: OamImage,
}

impl Frame {
    /// The uploaded image with every object hidden but those the game
    /// drew at `slots`, which are found again by what they are: the PPU's
    /// order of them is the uploaded image's, wherever the end of the
    /// frame moved them to.
    pub fn uploaded_from(&self, slots: std::ops::Range<usize>) -> OamImage {
        let wanted: Vec<_> = slots.map(|slot| object_bytes(&self.drawn, slot)).collect();
        let (mut image, first) = self.uploaded.clone();
        for slot in 0..OAM_OBJECTS {
            if !wanted.contains(&object_bytes(&image, slot)) {
                image[slot * 4 + 1] = HIDDEN_Y;
            }
        }
        (image, first)
    }
}

/// An object's position, tile, and attributes, and its size and X-high
/// bits.
fn object_bytes(image: &[u8], slot: usize) -> (&[u8], u8) {
    let high = image[OAM_OBJECTS * 4 + slot / 4] >> (2 * (slot % 4));
    (&image[slot * 4..slot * 4 + 4], high & 3)
}

/// Runs one frame of the level loop and then the ROM's whole NMI handler:
/// OAM, player and dynamic sprite graphics, and animated tiles and palettes.
pub(super) fn draw_frame(machine: &mut Machine) -> Result<Frame, CpuError> {
    let frame = Call::jsr(routines::DRAW_LEVEL_FRAME);
    let drawn = if machine.try_call_to(frame, Some(routines::CONSOLIDATE_OAM))? {
        machine.bus.ram.bytes(ram::OAM, OAM_LEN)
    } else {
        let drawn = read_unpacked_oam(&machine.bus.ram);
        machine.finish_call(frame)?;
        drawn
    };
    machine.bus.ram.set_u8(ram::LAG_FLAG, 0);
    machine.try_interrupt(Interrupt::Nmi)?;
    Ok(Frame {
        drawn,
        uploaded: uploaded(&machine.bus),
    })
}

/// The OAM image of a frame that has drawn but not yet reached
/// [`routines::CONSOLIDATE_OAM`]: the size bits are packed here from the
/// table the drawing routines wrote them to.
fn read_unpacked_oam(ram: &Ram) -> Vec<u8> {
    let mut image = ram.bytes(ram::OAM, OAM_OBJECTS * 4);
    let sizes = ram.bytes(ram::OAM_SIZES, OAM_OBJECTS);
    image.extend(sizes.chunks(4).map(|four| {
        four.iter()
            .enumerate()
            .fold(0, |packed, (i, size)| packed | (size & 3) << (2 * i))
    }));
    image
}

/// Visible objects in an OAM image, front to back from object `first`,
/// in screen coordinates. Y `$F0` is the game's hidden marker; objects
/// entirely off the screen are dropped, and those wrapped past its bottom
/// are read as negative.
pub(super) fn screen_objects(
    oam: &[u8],
    first: usize,
    sizes: [(i32, i32); 2],
) -> Vec<SpriteObject> {
    let mut out = Vec::new();
    for offset in 0..OAM_OBJECTS {
        let object = (first + offset) % OAM_OBJECTS;
        let (bytes, high) = object_bytes(oam, object);
        let large = high & 2 != 0;
        let (width, height) = sizes[large as usize];
        let x = bytes[0] as i32 - if high & 1 != 0 { 256 } else { 0 };
        let y = bytes[1];
        if y == HIDDEN_Y {
            continue;
        }
        let y = if y as i32 >= SCREEN_H {
            y as i32 - 256
        } else {
            y as i32
        };
        if x + width <= 0 || x >= SCREEN_W || y + height <= 0 || y >= SCREEN_H {
            continue;
        }
        out.push(SpriteObject {
            x,
            y,
            tile: bytes[2],
            attr: bytes[3],
            large,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oam_with(objects: &[(usize, u8, u8, u8, u8, u8)]) -> Vec<u8> {
        let mut oam = vec![0u8; OAM_LEN];
        for i in 0..OAM_OBJECTS {
            oam[i * 4 + 1] = HIDDEN_Y;
        }
        for &(i, x, y, tile, attr, high) in objects {
            oam[i * 4..i * 4 + 4].copy_from_slice(&[x, y, tile, attr]);
            oam[OAM_OBJECTS * 4 + i / 4] |= high << (2 * (i % 4));
        }
        oam
    }

    #[test]
    fn hidden_and_offscreen_objects_are_dropped() {
        let sizes = object_sizes(0x60); // 16x16 and 32x32
        let oam = oam_with(&[
            (0, 10, 20, 0x40, 0x30, 2),   // large, visible
            (1, 10, 0xF0, 0x40, 0x30, 2), // hidden marker
            (2, 0xF8, 30, 0x41, 0x00, 1), // x = -8, small: 8 px visible
            (3, 0xF0, 30, 0x41, 0x00, 1), // x = -16, small: gone
            (4, 0, 0xF8, 0x42, 0x00, 2),  // y = -8, large: visible
            (5, 0, 0xE0, 0x42, 0x00, 0),  // y = 224: below the screen
        ]);
        let got: Vec<_> = screen_objects(&oam, 0, sizes)
            .iter()
            .map(|o| (o.x, o.y, o.tile, o.attr, o.large))
            .collect();
        assert_eq!(
            got,
            [
                (10, 20, 0x40, 0x30, true),
                (-8, 30, 0x41, 0x00, false),
                (0, -8, 0x42, 0x00, true),
            ]
        );
    }

    #[test]
    fn unpacked_sizes_pack_four_to_a_byte() {
        use crate::ram::RamMap;
        for map in [RamMap::Vanilla, RamMap::Sa1Pack] {
            let mut ram = Ram::new(map);
            ram.set_u8_at(ram::OAM, 65 * 4 + 2, 0x42);
            ram.set_u8_at(ram::OAM_SIZES, 65, 2);
            ram.set_u8_at(ram::OAM_SIZES, 67, 0xFF); // only two bits count
            let image = read_unpacked_oam(&ram);
            assert_eq!(image.len(), OAM_LEN);
            assert_eq!(image[65 * 4 + 2], 0x42);
            assert_eq!(image[OAM_OBJECTS * 4 + 16], 0b1100_1000);
        }
    }

    #[test]
    fn objects_start_from_the_first_written_one() {
        let sizes = object_sizes(0x03); // 8x8 and 16x16, what SMW uses
        assert_eq!(sizes, [(8, 8), (16, 16)]);
        let oam = oam_with(&[(0, 1, 1, 1, 0, 0), (100, 2, 2, 2, 0, 0)]);
        let got: Vec<u8> = screen_objects(&oam, 100, sizes)
            .iter()
            .map(|o| o.tile)
            .collect();
        assert_eq!(got, [2, 1]);
    }
}
