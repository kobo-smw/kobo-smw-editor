//! The LC_LZ2 compressor on the vanilla GFX files: every file is
//! decompressed, compressed again, and read back, by the native decoder
//! and, written over the original in a copy of the ROM, by the game's own
//! routine on the headless CPU. The compressor's parse is optimal, so no
//! file may come out larger than Nintendo's.

mod common;

use kobo_core::compress::lz2;
use kobo_core::gfx::{self, GFX_FILE_COUNT};
use kobo_core::ram::RamAddr;
use kobo_core::{Rom, expand};

/// Files of the pointer tables, which is what `PrepareGraphicsFile` takes.
const TABLE_FILES: u8 = 0x32;

#[test]
fn vanilla_gfx_files_recompress_no_larger() {
    let Some(rom) = common::vanilla() else { return };
    let reader = gfx::GfxReader::new(&rom).unwrap();
    let mut bytes = rom.data().to_vec();
    let mut files = Vec::new();
    let (mut vanilla, mut kobo) = (0, 0);
    for index in 0..GFX_FILE_COUNT {
        let file = reader.read(index).unwrap();
        let packed = lz2::compress(&file.data).unwrap();
        let back = lz2::decompress(&packed).unwrap();
        assert!(back.data == file.data, "GFX{index:02X}: round trip differs");
        assert!(
            packed.len() <= file.compressed_len,
            "GFX{index:02X}: {} bytes, vanilla {}",
            packed.len(),
            file.compressed_len
        );
        vanilla += file.compressed_len;
        kobo += packed.len();
        let at = rom.pc(file.addr).unwrap().as_usize();
        bytes[at..at + packed.len()].copy_from_slice(&packed);
        files.push(file.data);
    }
    eprintln!(
        "{GFX_FILE_COUNT} GFX files: {kobo} bytes recompressed, {vanilla} in the vanilla ROM ({:.2}%)",
        100.0 * kobo as f64 / vanilla as f64
    );

    let recompressed = Rom::from_bytes(bytes).unwrap();
    for index in 0..TABLE_FILES {
        let want = &files[index as usize];
        let got = expand::decompress_gfx_file(&recompressed, index, want.len()).unwrap();
        assert!(
            &got == want,
            "GFX{index:02X}: the game reads it differently"
        );
    }
    // `GFX32` and `GFX33` have no table entry; loading a level puts them
    // at `$7E2000` to `$7EAD00` (`CODE_00B888`), and the level's files
    // and Mario's tiles in VRAM.
    let want = expand::expand_level(&rom, 0x105).unwrap();
    let got = expand::expand_level(&recompressed, 0x105).unwrap();
    let buffers = |level: &expand::LoadedLevel| level.ram.bytes(RamAddr::new(0x7E_2000), 0x8D00);
    assert!(buffers(&want)[..files[0x32].len()] == files[0x32]);
    assert!(buffers(&got) == buffers(&want), "GFX32 or GFX33 differs");
    assert!(got.video.vram == want.video.vram, "VRAM differs");
}
