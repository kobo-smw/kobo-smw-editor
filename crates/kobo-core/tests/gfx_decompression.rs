//! The native LC_LZ2 and LC_LZ3 decoders against each ROM's own
//! decompression routine, run on the headless CPU: every GFX file in the
//! pointer tables must come out the same both ways. Runs on the vanilla
//! ROM and on any hacks in `KOBO_LM_ROMS`; a hack whose graphics Lunar
//! Magic stored as LC_LZ3 is what exercises that decoder. Hacks with a
//! hash in `fixtures/lunar_magic_gfx_export.txt` are also checked against
//! Lunar Magic's `-ExportGFX`, which covers `GFX32` and `GFX33` too.

mod common;

use kobo_core::gfx::{self, Compression, GFX_FILE_COUNT, GfxError};
use kobo_core::{Rom, expand};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

/// Files of the pointer tables, which is what `PrepareGraphicsFile` takes.
const TABLE_FILES: u8 = 0x32;

/// Checks every file, returning the ROM's format and what differed.
fn check_rom(rom: &Rom) -> Result<(Compression, Vec<String>), GfxError> {
    let reader = gfx::GfxReader::new(rom)?;
    let compression = reader.compression();
    let mut failures = Vec::new();
    for index in 0..TABLE_FILES {
        let file = match reader.read(index) {
            Ok(file) => file,
            Err(e) => {
                failures.push(e.to_string());
                continue;
            }
        };
        match expand::decompress_gfx_file(rom, index, file.data.len()) {
            Ok(want) if want == file.data => {}
            Ok(want) => {
                let at = (0..want.len()).find(|&i| want[i] != file.data[i]).unwrap();
                failures.push(format!(
                    "GFX{index:02X}: differs from the ROM's at byte {at:#x}"
                ));
            }
            Err(e) => failures.push(e.to_string()),
        }
    }
    Ok((compression, failures))
}

#[test]
fn vanilla_files_decode_as_the_rom_decodes_them() {
    let Some(rom) = common::vanilla() else { return };
    let (compression, failures) = check_rom(&rom).unwrap();
    assert_eq!(compression, Compression::Lz2);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn lunar_magic_files_decode_as_the_rom_decodes_them() {
    for (path, rom) in common::lunar_magic_roms() {
        match check_rom(&rom) {
            Ok((compression, failures)) => {
                eprintln!("{}: {compression}", path.display());
                assert!(
                    failures.is_empty(),
                    "{}:\n{}",
                    path.display(),
                    failures.join("\n")
                );
            }
            // Its files cannot be read by number, which is the point.
            Err(GfxError::Locked) => eprintln!("skipping {}: locked", path.display()),
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
}

/// Headerless ROM SHA-1 to the SHA-1 of its exported files, concatenated.
fn load_fixture() -> HashMap<String, String> {
    include_str!("fixtures/lunar_magic_gfx_export.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut p = l.split_whitespace();
            (p.next().unwrap().to_string(), p.next().unwrap().to_string())
        })
        .collect()
}

#[test]
fn lunar_magic_files_match_lunar_magic_export() {
    let fixture = load_fixture();
    for (path, rom) in common::lunar_magic_roms() {
        let Some(want) = fixture.get(&rom.sha1_hex()) else {
            eprintln!("skipping {}: no export hash in fixture", path.display());
            continue;
        };
        let mut hash = Sha1::new();
        let reader = gfx::GfxReader::new(&rom).unwrap();
        for index in 0..GFX_FILE_COUNT {
            hash.update(reader.read(index).unwrap().to_lm_export());
        }
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(&got, want, "{}", path.display());
    }
}
