//! Kobo's ROM-side code where no level pass reaches it: the overworld,
//! run through the routines that build it. Needs Asar's library and the
//! vanilla ROM; skips without them.

use super::machine::{Call, Machine};
use crate::asar::Asar;
use crate::rom::{Rom, RomIdentity};

fn vanilla() -> Option<Rom> {
    let rom = Rom::load(crate::config::vanilla_rom_path().ok()?).ok()?;
    (rom.identify() == RomIdentity::VanillaUsa).then_some(rom)
}

/// VRAM after the overworld's layer 1 is built: `CODE_04DC09` sets the
/// overworld's tileset, and `CODE_04D6E9` builds the tilemap through the
/// Map16 uploads Kobo's routine hooks.
fn overworld_vram(rom: &Rom) -> Vec<u8> {
    let mut machine = Machine::new(rom, 0);
    machine.call(Call::jsl(0x04DC09)).unwrap();
    machine.call(Call::jsl(0x04D6E9)).unwrap();
    machine.bus.vram.clone()
}

#[test]
fn the_overworld_draws_as_vanilla_does() {
    let (Some(clean), Ok(asar)) = (vanilla(), Asar::offline()) else {
        eprintln!("skipping: no vanilla ROM or Asar configured");
        return;
    };
    let mut rom = Rom::from_headerless(clean.data().to_vec()).unwrap();
    rom.expand(0x10_0000).unwrap();
    let rom = crate::install::apply_lunar_magic(&asar, &rom).unwrap();
    let (a, b) = (overworld_vram(&clean), overworld_vram(&rom));
    assert!(a.iter().any(|&x| x != 0), "the overworld drew nothing");
    let differ = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    assert_eq!(differ, 0, "VRAM bytes that differ");
}
