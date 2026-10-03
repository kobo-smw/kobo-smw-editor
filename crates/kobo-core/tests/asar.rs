//! Asar through its shared library, on synthetic images: writes, free
//! space, output, errors, expansion, repeatability, and the guard against
//! damage to existing RATS blocks. Skips when no library is configured
//! (`KOBO_ASAR_LIB` or `tools.asar`).

mod common;

use std::path::PathBuf;

use kobo_core::asar::{self, AsarError, Patch};
use kobo_core::rats::{self, Damage, RatsBlock};
use kobo_core::{Mapping, Rom, SnesAddr};

const KIB: usize = 0x400;
const MIB: usize = 0x10_0000;

/// A LoROM image of `len` bytes: `$FF` up to `$108000`, then free.
fn image(len: usize) -> Rom {
    image_mapped(len, 0x20)
}

fn image_mapped(len: usize, map_mode: u8) -> Rom {
    let mut data = vec![0xFF; len];
    if len > 512 * KIB {
        data[512 * KIB..].fill(0);
    }
    data[0x7FD5] = map_mode;
    Rom::from_bytes(data).unwrap()
}

/// Where in-memory patches pretend to be. Nothing is written there.
fn virtual_dir() -> PathBuf {
    std::env::temp_dir().join("kobo-asar-virtual")
}

fn source(text: &str) -> Patch {
    Patch::source(virtual_dir().join("main.asm"), text)
}

fn read(rom: &Rom, addr: SnesAddr, len: usize) -> Vec<u8> {
    rom.read(addr, len).unwrap().to_vec()
}

fn tag_before(rom: &Rom, addr: SnesAddr) -> Option<usize> {
    rats::tag_at(rom.data(), rom.pc(addr).unwrap().as_usize() - rats::TAG_LEN)
}

#[test]
fn the_library_is_the_pinned_release() {
    let Some(asar) = common::asar() else { return };
    assert_eq!(asar.version(), asar::PINNED, "{}", asar.path().display());
}

#[test]
fn writes_bytes_and_free_space() {
    let Some(asar) = common::asar() else { return };
    let rom = image(MIB);
    let patch = source(
        "lorom
org $008000
    autoclean JML Code
freecode
Code:
    LDA.l Table
    RTL
freedata cleaned
Table:
    db $01, $02, $03
print \"code at $\", hex(Code)
print \"done\"
",
    );
    let patched = asar.patch(&rom, &patch).unwrap();
    let out = &patched.rom;
    let output = &patched.output;
    let code = output.label("Code").unwrap();
    let table = output.label("Table").unwrap();
    assert_eq!(code, SnesAddr::new(0x908008));
    assert_eq!(
        read(out, SnesAddr::new(0x008000), 4),
        [0x5C, 0x08, 0x80, 0x90]
    );
    let [lo, hi, bank, _] = table.raw().to_le_bytes();
    assert_eq!(read(out, code, 5), [0xAF, lo, hi, bank, 0x6B]);
    assert_eq!(read(out, table, 3), [1, 2, 3]);
    assert_eq!(tag_before(out, code), Some(5));
    assert_eq!(tag_before(out, table), Some(3));
    assert_eq!(output.prints, ["code at $908008", "done"]);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    // The checksum is Kobo's to fix, and the size stays.
    assert_eq!(read(out, SnesAddr::new(0x00FFDC), 4), [0xFF; 4]);
    assert_eq!(out.len(), MIB);
    assert_eq!(out.mapping(), Mapping::LoRom);
    // What Asar wrote is reported, and the input is left alone.
    assert!(output.written.iter().any(|w| w.contains(&0)));
    assert_eq!(rom.data(), image(MIB).data());
}

#[test]
fn errors_and_warnings_come_back() {
    let Some(asar) = common::asar() else { return };
    let rom = image(MIB);
    let warned = asar
        .patch(
            &rom,
            &source("lorom\norg $008000\nwarn \"careful\"\ndb $EA\n"),
        )
        .unwrap();
    let [warning] = &warned.output.warnings[..] else {
        panic!("{:?}", warned.output.warnings)
    };
    assert!(warning.raw.contains("careful"), "{warning:?}");
    assert_eq!(warning.line, Some(3));
    assert_eq!(warned.rom.read_u8(SnesAddr::new(0x008000)).unwrap(), 0xEA);

    let failed = asar.patch(
        &rom,
        &source("lorom\norg $008000\nprint \"before\"\nJML Nowhere\nerror \"boom\"\n"),
    );
    let Err(AsarError::Failed { errors, .. }) = failed else {
        panic!("{failed:?}")
    };
    assert!(
        errors.iter().any(|e| e.raw.contains("Nowhere")),
        "{errors:?}"
    );
    assert!(errors.iter().any(|e| e.raw.contains("boom")), "{errors:?}");
    let boom = errors.iter().find(|e| e.raw.contains("boom")).unwrap();
    assert!(
        boom.file.as_deref().unwrap().ends_with("main.asm"),
        "{boom:?}"
    );
    assert_eq!(boom.line, Some(5));
    let message = AsarError::Failed {
        errors: errors.clone(),
        warnings: Vec::new(),
        prints: Vec::new(),
    }
    .to_string();
    assert!(message.contains("boom"), "{message}");

    let missing = asar.patch(&rom, &Patch::new(virtual_dir().join("missing.asm")));
    assert!(
        matches!(missing, Err(AsarError::Failed { .. })),
        "{missing:?}"
    );
}

#[test]
fn a_full_image_expands() {
    let Some(asar) = common::asar() else { return };
    let rom = image(512 * KIB);
    let patched = asar
        .patch(&rom, &source("lorom\nfreecode\nCode:\n    RTL\n"))
        .unwrap();
    assert_eq!(patched.rom.len(), MIB);
    assert_eq!(patched.rom.mapping(), Mapping::LoRom);
    assert_eq!(patched.rom.internal_header().rom_size_code, 0x0A);
    let code = patched.output.label("Code").unwrap();
    assert_eq!(code, SnesAddr::new(0x908008));
    assert_eq!(read(&patched.rom, code, 1), [0x6B]);
}

#[test]
fn an_image_too_long_for_asars_buffer_is_refused_before_the_call() {
    let Some(asar) = common::asar() else { return };
    // Asar copies the image into a 16 MiB buffer of its own without
    // checking the length it is given; one past Kobo's buffer never
    // reaches it.
    let rom = image(asar::BUFFER_LEN + 32 * KIB);
    let result = asar.patch(&rom, &source("lorom\norg $008000\ndb $42\n"));
    assert!(
        matches!(result, Err(AsarError::InputTooLarge { len }) if len == rom.len()),
        "{result:?}"
    );
}

#[test]
fn a_grown_image_is_read_headerless() {
    let Some(asar) = common::asar() else { return };
    // Writing 512 bytes past the end of a 1 MiB image leaves one whose
    // length copier-header detection would take for a headered image.
    let mut rom = image(MIB);
    let marker: Vec<u8> = (0..0x10).collect();
    rom.write(SnesAddr::new(0x008000), &marker).unwrap();
    let patched = asar
        .patch(
            &rom,
            &source("lorom\norg $208000\nfillbyte $42\nfill $200\n"),
        )
        .unwrap()
        .rom;
    assert!(!patched.has_copier_header());
    assert_eq!(patched.len(), MIB + 32 * KIB, "padded to a whole bank");
    assert_eq!(read(&patched, SnesAddr::new(0x008000), 0x10), marker);
    assert_eq!(patched.data()[..MIB - 0x200], rom.data()[..MIB - 0x200]);
    assert_eq!(
        read(&patched, SnesAddr::new(0x208000), 0x200),
        [0x42; 0x200]
    );
    assert!(patched.data()[MIB + 0x200..].iter().all(|&b| b == 0));
}

#[test]
fn sa1_images_stay_sa1() {
    let Some(asar) = common::asar() else { return };
    let rom = image_mapped(MIB, 0x23);
    let patched = asar
        .patch(&rom, &source("sa1rom\nfreecode\nCode:\n    RTL\n"))
        .unwrap();
    assert_eq!(patched.rom.mapping(), Mapping::Sa1Rom);
    assert_eq!(patched.output.label("Code"), Some(SnesAddr::new(0x108008)));
}

#[test]
fn the_same_inputs_give_the_same_output() {
    let Some(asar) = common::asar() else { return };
    let rom = image(MIB);
    let patch = source(
        "lorom
org $008000
    autoclean JSL Code
freecode
Code:
    db !value
    RTL
print hex(Code)
",
    )
    .define("value", "$42");
    let a = asar.patch(&rom, &patch).unwrap();
    let b = asar.patch(&rom, &patch).unwrap();
    assert_eq!(a.rom.data(), b.rom.data());
    assert_eq!(a.output, b.output);

    // From several threads at once, which the library cannot do by itself.
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| asar.patch(&rom, &patch).unwrap()))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for r in results {
        assert_eq!(r.rom.data(), a.rom.data());
    }

    // Nothing carries over from one patch to the next.
    let later = asar.patch(&rom, &source("lorom\norg $008000\nJML Code\n"));
    assert!(matches!(later, Err(AsarError::Failed { .. })), "{later:?}");
    let later = asar.patch(&rom, &source("lorom\norg $008000\ndb !value\n"));
    assert!(matches!(later, Err(AsarError::Failed { .. })), "{later:?}");
}

#[test]
fn includes_come_from_disk_or_memory() {
    let Some(asar) = common::asar() else { return };
    let rom = image(MIB);
    let main = "lorom\norg $008000\nincsrc \"near.asm\"\nincsrc \"far.asm\"\n";

    let dir = std::env::temp_dir().join(format!("kobo-asar-{}", std::process::id()));
    let lib = dir.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(dir.join("main.asm"), main).unwrap();
    std::fs::write(dir.join("near.asm"), "db !a\n").unwrap();
    std::fs::write(lib.join("far.asm"), "db !b\n").unwrap();
    let from_disk = asar.patch(
        &rom,
        &Patch::new(dir.join("main.asm"))
            .include_path(&lib)
            .define("a", "$11")
            .define("!b", "$22"),
    );
    std::fs::remove_dir_all(&dir).unwrap();
    let from_disk = from_disk.unwrap();
    assert_eq!(
        read(&from_disk.rom, SnesAddr::new(0x008000), 2),
        [0x11, 0x22]
    );

    let dir = virtual_dir();
    let from_memory = asar
        .patch(
            &rom,
            &Patch::source(dir.join("main.asm"), main)
                .file(dir.join("near.asm"), "db !a\n")
                .file(dir.join("lib/far.asm"), "db !b\n")
                .include_path(dir.join("lib"))
                .define("a", "$11")
                .define("b", "$22"),
        )
        .unwrap();
    assert_eq!(from_memory.rom.data(), from_disk.rom.data());
}

/// Two Kobo blocks as `docs/toolchain.md` sets them up, with their tags at
/// file offsets `0x87FF8` and `0x90000`, the second ending at `0x98000`.
fn boundary_image() -> Rom {
    let mut rom = image(MIB);
    let mut space = rats::FreeSpace::scan(&rom);
    let a = space.alloc(&mut rom, 0x8000, rats::Contents::Code).unwrap();
    let b = space.alloc(&mut rom, 0x7FF8, rats::Contents::Code).unwrap();
    assert_eq!((a, b), (SnesAddr::new(0x118000), SnesAddr::new(0x128008)));
    rom
}

#[test]
fn asar_bank_boundary_damage_is_caught() {
    let Some(asar) = common::asar() else { return };
    let rom = boundary_image();
    let result = asar.patch(
        &rom,
        &source("lorom\nfreecode cleaned\nfillbyte $00\nfill $8000\n"),
    );
    let Err(AsarError::Damaged { damage, .. }) = &result else {
        panic!("Asar 1.91's overwrite was not caught: {result:?}")
    };
    assert_eq!(
        damage,
        &[Damage {
            block: RatsBlock {
                start: SnesAddr::new(0x128008),
                len: 0x7FF8
            },
            changed: (SnesAddr::new(0x12FFF8), SnesAddr::new(0x12FFFF)),
            tagged: true,
        }]
    );
    let message = result.unwrap_err().to_string();
    assert!(message.contains("$128008"), "{message}");
}

#[test]
fn autoclean_releases_blocks() {
    let Some(asar) = common::asar() else { return };
    // A hook at $008000 into a Kobo block of $100 bytes, taken over by a
    // patch whose new code Asar puts where the old block was.
    let mut rom = image(MIB);
    let mut space = rats::FreeSpace::scan(&rom);
    let old = space.alloc(&mut rom, 0x100, rats::Contents::Code).unwrap();
    rom.write(old, &[0x11; 0x100]).unwrap();
    rom.write(SnesAddr::new(0x008000), &[0x5C]).unwrap();
    rom.write_u24(SnesAddr::new(0x008001), old.raw()).unwrap();
    for len in [0x80, 0x100, 0x200] {
        let patch = source(&format!(
            "lorom\norg $008000\n    autoclean JML Main\nfreecode\nMain:\nfillbyte $22\nfill ${len:X}\n"
        ));
        let patched = asar.patch(&rom, &patch).unwrap();
        let main = patched.output.label("Main").unwrap();
        assert_eq!(patched.rom.pc(main), rom.pc(old), "Asar reuses the space");
        assert_eq!(rats::blocks(&patched.rom).len(), 1);
        assert_eq!(tag_before(&patched.rom, main), Some(len));
    }
    // Erased and not reused.
    let patched = asar
        .patch(
            &rom,
            &source("lorom\norg $008000\n    autoclean $108008\n    db $6B\n"),
        )
        .unwrap();
    assert!(rats::blocks(&patched.rom).is_empty());
}
