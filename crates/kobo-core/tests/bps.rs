//! BPS patches against the vanilla ROM, and round trips of the hacks in
//! `KOBO_LM_ROMS` through a patch made from it.

mod common;

use kobo_core::bps::{self, SourceForm};
use kobo_core::rom::COPIER_HEADER_LEN;

/// Vanilla edited the way a hack edits it: bytes changed in place, a bank's
/// worth moved, data repeated, and the image expanded with zero-filled space.
fn modified(vanilla: &[u8]) -> Vec<u8> {
    let mut rom = vanilla.to_vec();
    rom[0x7FC0..0x7FD0].copy_from_slice(b"KOBO BPS TEST   ");
    rom[0x2E000..0x2E400].fill(0x25);
    rom.copy_within(0x40000..0x48000, 0x10000);
    rom.resize(0x10_0000, 0);
    rom.copy_within(0x68000..0x70000, 0x80000);
    rom[0x80100..0x80180].copy_from_slice(&vanilla[0x7000..0x7080]);
    rom[0x0F_FFF0..].fill(0xFF);
    rom
}

#[test]
fn round_trips_a_modified_vanilla_image() {
    let Some(vanilla) = common::vanilla() else {
        return;
    };
    let target = modified(vanilla.data());
    let patch = bps::create(vanilla.data(), &target);
    assert!(patch.len() < 0x400, "patch is {} bytes", patch.len());
    assert_eq!(bps::apply(&patch, vanilla.data()).unwrap(), target);
    let patched = bps::apply_to_rom(&patch, &vanilla).unwrap();
    assert_eq!(patched.source, SourceForm::Headerless);
    assert_eq!(patched.data, target);
}

#[test]
fn applies_a_patch_made_against_a_headered_image() {
    let Some(vanilla) = common::vanilla() else {
        return;
    };
    let header = [0u8; COPIER_HEADER_LEN];
    let target = modified(vanilla.data());
    let patch = bps::create(
        &[&header[..], vanilla.data()].concat(),
        &[&header[..], &target].concat(),
    );
    let patched = bps::apply_to_rom(&patch, &vanilla).unwrap();
    assert_eq!(patched.source, SourceForm::ZeroHeader);
    assert_eq!(patched.data, target);

    // The header of the usual headered dump: 64 units of 8 KiB.
    let mut header = [0u8; COPIER_HEADER_LEN];
    header[0] = 0x40;
    let patch = bps::create(
        &[&header[..], vanilla.data()].concat(),
        &[&header[..], &target].concat(),
    );
    let patched = bps::apply_to_rom(&patch, &vanilla).unwrap();
    let want = match vanilla.copier_header() {
        Some(own) if own == header => SourceForm::OwnHeader,
        _ => SourceForm::SizeHeader,
    };
    assert_eq!(patched.source, want);
    assert_eq!(patched.data, target);
}

#[test]
fn lunar_magic_hacks_round_trip_through_a_patch() {
    let Some(vanilla) = common::vanilla() else {
        return;
    };
    for (path, rom) in common::lunar_magic_roms() {
        let patch = bps::create(vanilla.data(), rom.data());
        let back = bps::apply(&patch, vanilla.data())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(back == rom.data(), "{}: round trip differs", path.display());
        eprintln!(
            "{}: {} bytes for {} KiB",
            path.display(),
            patch.len(),
            rom.len() / 1024
        );
    }
}
