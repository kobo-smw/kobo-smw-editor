//! The sprite loader's per-entry "already loaded" flags.

use crate::cpu::Bus;
use crate::cpu::smw_bus::SmwBus;
use crate::ram::{self, Ram};

/// Where the ROM's sprite loader keeps its per-entry "already loaded"
/// flags: `$1938` (128 entries, or wherever the RAM map has them) in
/// vanilla, or `$7FAF00` (256 entries) on a LoROM cartridge when PIXI's
/// 255-sprite option has put its `JML` at `$02A856`, PIXI's marker set
/// (PIXI's `asm/main.asm`; on SA-1 its flags are where the RAM map has
/// them). Lunar Magic has no such patch: its help says other programs
/// declare one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct LoadFlags {
    /// Bus address of the first flag.
    base: u32,
    count: u32,
}

impl LoadFlags {
    pub fn detect(bus: &mut SmwBus) -> Self {
        const FLAG_CHECK: u32 = 0x02_A856;
        const JML: u8 = 0x5C;
        const PIXI_FLAGS: u32 = 0x7F_AF00;
        let marker = crate::sprites::PIXI_SIZE_TABLE_MARKER.raw();
        if bus.ram.map() == ram::RamMap::Vanilla
            && bus.read(FLAG_CHECK) == JML
            && bus.read(marker) == crate::sprites::PIXI_MARKER_VALUE
        {
            return Self {
                base: PIXI_FLAGS,
                count: 0x100,
            };
        }
        Self {
            base: bus.ram.map().resolve(ram::SPRITE_LOAD_STATUS),
            count: bus.ram.map().sprite_load_flags(),
        }
    }

    pub fn fill(self, ram: &mut Ram, value: u8) {
        for i in 0..self.count {
            ram.poke(self.base + i, value);
        }
    }

    pub fn read(self, ram: &Ram) -> Vec<u8> {
        (0..self.count).map(|i| ram.peek(self.base + i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{Mapping, SnesAddr};
    use crate::rom::Rom;

    #[test]
    fn load_flags_follow_pixis_255_sprite_option() {
        let mut bytes = vec![0; 0x10_0000];
        let pc = |addr: u32| {
            Mapping::LoRom
                .snes_to_pc(SnesAddr::new(addr))
                .unwrap()
                .as_usize()
        };
        let vanilla = LoadFlags {
            base: 0x7E_1938,
            count: 0x80,
        };
        bytes[0x7FD5] = 0x20; // LoROM
        // Vanilla: `LDA $1938,X`.
        bytes[pc(0x02_A856)..][..3].copy_from_slice(&[0xBD, 0x38, 0x19]);
        let rom = Rom::from_bytes(bytes.clone()).unwrap();
        assert_eq!(LoadFlags::detect(&mut SmwBus::new(&rom)), vanilla);
        // PIXI's `JML`, but no PIXI marker: some other patch.
        bytes[pc(0x02_A856)..][..4].copy_from_slice(&[0x5C, 0x00, 0x80, 0x12]);
        let rom = Rom::from_bytes(bytes.clone()).unwrap();
        assert_eq!(LoadFlags::detect(&mut SmwBus::new(&rom)), vanilla);
        // And with it.
        bytes[pc(0x0E_F30F)] = 0x42;
        let rom = Rom::from_bytes(bytes).unwrap();
        assert_eq!(
            LoadFlags::detect(&mut SmwBus::new(&rom)),
            LoadFlags {
                base: 0x7F_AF00,
                count: 0x100
            }
        );
    }
}
