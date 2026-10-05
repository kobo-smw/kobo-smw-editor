//! The clean room's switch for the whole process (`kobo_core::clean_room`),
//! alone in its binary so that nothing loaded before it: a process that has
//! only loaded clean ROMs keeps instruction-level output, and loses it for
//! good once a ROM Lunar Magic saved is made. No ROM needed.
//!
//! Every other test binary may load such a ROM in any test, which switches
//! the rest of its tests' traces off; run a vanilla test alone (`cargo test
//! --test <binary> <name>`) to trace it.

mod common;

use kobo_core::cpu::Cpu;
use kobo_core::{Rom, clean_room};

#[test]
fn traces_stay_on_until_a_rom_lunar_magic_saved_is_loaded() {
    let clean = common::synthetic_base();
    assert!(!clean_room::saved_by_lunar_magic(&clean));
    assert!(
        !clean_room::forbidden(),
        "a clean ROM switched the trace off"
    );
    let mut cpu = Cpu::new();
    cpu.keep_history(8);
    assert!(cpu.keeps_history());

    // Lunar Magic fills an area the game leaves $FF on every save.
    let (area, len) = clean_room::SAVE_AREAS[0];
    let mut saved = clean.data().to_vec();
    let at = clean.pc(area).unwrap().as_usize();
    saved[at..at + len].fill(0x00);
    let saved = Rom::from_headerless(saved).unwrap();
    assert!(clean_room::saved_by_lunar_magic(&saved));
    assert!(clean_room::forbidden(), "making the ROM switches it off");

    let mut cpu = Cpu::new();
    cpu.keep_history(8);
    assert!(!cpu.keeps_history(), "a trace in a forbidden process");
    // And for good: a clean ROM made after does not switch it back on.
    let _ = Rom::from_headerless(clean.data().to_vec()).unwrap();
    assert!(clean_room::forbidden());
}
