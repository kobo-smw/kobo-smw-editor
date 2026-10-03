//! Tests that need the real vanilla ROM. They skip when none is configured
//! so CI without ROM data still passes.

mod common;

use common::vanilla;
use kobo_core::{Mapping, PcAddr, RomIdentity, SnesAddr, rom::VANILLA_USA_SHA1};

#[test]
fn identifies_vanilla_usa() {
    let Some(rom) = vanilla() else { return };
    assert_eq!(rom.identify(), RomIdentity::VanillaUsa);
    assert_eq!(rom.sha1(), VANILLA_USA_SHA1);
    assert_eq!(rom.len(), 0x80000);
    assert_eq!(rom.mapping(), Mapping::LoRom);
}

#[test]
fn vanilla_internal_header() {
    let Some(rom) = vanilla() else { return };
    let h = rom.internal_header();
    assert_eq!(h.title, "SUPER MARIOWORLD");
    assert_eq!(h.map_mode, 0x20);
    assert_eq!(h.cartridge_type, 0x02);
    assert_eq!(h.rom_size().unwrap(), 0x80000);
    assert_eq!(h.sram_size().unwrap(), 0x800);
    assert_eq!(h.region, 0x01);
    assert!(h.checksum_pair_valid());
    assert_eq!(h.checksum, 0xA0DA);
    assert_eq!(rom.compute_checksum(), h.checksum);
}

#[test]
fn vanilla_level_pointer_tables() {
    let Some(rom) = vanilla() else { return };
    // Layer 1 and layer 2 tables hold 3-byte pointers. The sprite table holds
    // 2-byte pointers into bank $07. A layer 2 pointer with bank $FF marks a
    // background tilemap; the game substitutes bank $0C for it.
    let layer1 = SnesAddr::new(0x05E000);
    let layer2 = SnesAddr::new(0x05E600);
    let sprites = SnesAddr::new(0x05EC00);
    assert_eq!(rom.pc(layer1), Ok(PcAddr::new(0x02E000)));
    // Yoshi's Island 1.
    assert_eq!(
        rom.read_ptr(layer1.add(0x105 * 3)).unwrap(),
        SnesAddr::new(0x0688DD)
    );
    for level in 0..0x200u32 {
        let l1 = rom.read_ptr(layer1.add(level * 3)).unwrap();
        assert!(
            rom.read_u8(l1).is_ok(),
            "level {level:03X} layer 1 pointer {l1}"
        );

        let mut l2 = rom.read_ptr(layer2.add(level * 3)).unwrap();
        if l2.bank() == 0xFF {
            l2 = SnesAddr::from_bank_offset(0x0C, l2.offset());
        }
        assert!(
            rom.read_u8(l2).is_ok(),
            "level {level:03X} layer 2 pointer {l2}"
        );

        let sp = SnesAddr::from_bank_offset(0x07, rom.read_u16(sprites.add(level * 2)).unwrap());
        assert!(
            rom.read_u8(sp).is_ok(),
            "level {level:03X} sprite pointer {sp}"
        );
    }
}
