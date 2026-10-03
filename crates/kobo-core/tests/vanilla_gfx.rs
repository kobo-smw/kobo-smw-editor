//! GFX decompression checked against Lunar Magic's `-ExportGFX` output.
//! The fixture holds hashes only; the exported bytes are Nintendo's.

mod common;

use kobo_core::gfx::{self, Bpp, GFX_FILE_COUNT, GfxFormat};
use sha1::{Digest, Sha1};

struct Expected {
    sha1: String,
    len: usize,
}

fn load_fixture() -> Vec<Option<Expected>> {
    let text = include_str!("fixtures/vanilla_gfx_lm_export.txt");
    let mut out: Vec<Option<Expected>> = (0..0x34).map(|_| None).collect();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap();
        let index = usize::from_str_radix(name.trim_start_matches("GFX"), 16).unwrap();
        out[index] = Some(Expected {
            sha1: parts.next().unwrap().to_string(),
            len: parts.next().unwrap().parse().unwrap(),
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
        let got: String = Sha1::digest(&export)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
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
