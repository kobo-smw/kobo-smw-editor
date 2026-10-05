//! GFX decompression checked against Lunar Magic's `-ExportGFX` output.
//! The fixture holds hashes only; the exported bytes are Nintendo's.

mod common;

use common::fixtures::sha1_hex;
use kobo_core::gfx::{self, Bpp, GFX_FILE_COUNT, GfxFormat};

struct Expected {
    sha1: String,
    len: usize,
}

fn load_fixture() -> Vec<Option<Expected>> {
    let mut out: Vec<Option<Expected>> = (0..0x34).map(|_| None).collect();
    for record in common::fixtures::records(include_str!("fixtures/vanilla_gfx_lm_export.txt")) {
        let index = usize::from_str_radix(record[0].trim_start_matches("GFX"), 16).unwrap();
        out[index] = Some(Expected {
            sha1: record[1].to_string(),
            len: record[2].parse().unwrap(),
        });
    }
    out
}

#[test]
fn gfx_files_match_lunar_magic_export() {
    let Some(rom) = common::vanilla() else { return };
    let expected = load_fixture();
    let reader = gfx::GfxReader::new(&rom).unwrap();
    for index in 0..GFX_FILE_COUNT {
        let file = reader.read(index).unwrap();
        let export = file.to_lm_export();
        let want = expected[index as usize].as_ref().unwrap();
        let got = sha1_hex(&export);
        assert_eq!(export.len(), want.len, "GFX{index:02X} size");
        assert_eq!(got, want.sha1, "GFX{index:02X} contents");
    }
}

#[test]
fn gfx_file_shapes() {
    let Some(rom) = common::vanilla() else { return };
    let f = gfx::read_gfx_file(&rom, 0x00).unwrap();
    assert_eq!(f.bpp(), Some(Bpp::Three));
    assert_eq!(f.tile_count(), 128);
    assert_eq!(f.addr, kobo_core::SnesAddr::new(0x08D9F9));
    let f = gfx::read_gfx_file(&rom, 0x28).unwrap();
    assert_eq!(f.bpp(), Some(Bpp::Two));
    assert_eq!(f.tile_count(), 128);
    let f = gfx::read_gfx_file(&rom, 0x27).unwrap();
    assert_eq!(f.format, GfxFormat::Packed3);
    assert_eq!(f.tile_count(), 128);
    // Mario is stored at full depth; the animated tiles are widened on
    // their way into RAM.
    let f = gfx::read_gfx_file(&rom, 0x32).unwrap();
    assert_eq!(f.bpp(), Some(Bpp::Four));
    assert_eq!(f.tile_count(), 744);
    assert_eq!(f.addr, kobo_core::SnesAddr::new(0x088000));
    let f = gfx::read_gfx_file(&rom, 0x33).unwrap();
    assert_eq!(f.bpp(), Some(Bpp::Three));
    assert_eq!(f.tile_count(), 384);
    assert_eq!(f.addr, kobo_core::SnesAddr::new(0x08BFC0));
}
