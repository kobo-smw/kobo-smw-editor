//! The build's tool stages: the project's Asar patches, early and late,
//! and AddmusicK. The patch stages need Asar's library (configured, or the
//! pinned build in the cache) and no ROM; the music stage needs AddmusicK
//! (`KOBO_ADDMUSICK`) and the vanilla ROM, and UberASM Tool (configured, or
//! the pinned build in the cache) runs with or without it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use kobo_core::SnesAddr;
use kobo_core::build::{self, Cache, Project};
use kobo_core::source::project::Manifest;
use kobo_core::tools::Tool;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kobo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, file: &str, text: &str) {
    let path = dir.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn patches_apply_early_and_late_in_order() {
    if common::asar().is_none() {
        return;
    }
    let base = common::synthetic_base();
    let dir = temp_dir("patches");
    write(&dir, "asm/shared.asm", "!value = $42\n");
    write(
        &dir,
        "asm/early.asm",
        "lorom\norg $0FF200\ndb $01, $02\nfreedata\nearly: db \"EARLY\"\norg $0FF210\ndl early\n",
    );
    write(
        &dir,
        "asm/late.asm",
        "lorom\nincsrc \"shared.asm\"\norg $0FF201\ndb !value\n",
    );
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            early_patches: vec![PathBuf::from("asm/early.asm")],
            late_patches: vec![PathBuf::from("asm/late.asm")],
            ..Manifest::default()
        },
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    };
    let built = build::build_on(&base, &project, None).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    // The late patch wrote over the early one's second byte.
    assert_eq!(
        built.read(SnesAddr::new(0x0FF200), 2).unwrap(),
        [0x01, 0x42]
    );
    let early = built.read_ptr(SnesAddr::new(0x0FF210)).unwrap();
    assert_eq!(built.read(early, 5).unwrap(), b"EARLY");
    assert!(built.internal_header().checksum_pair_valid());
    assert_eq!(
        build::build_on(&base, &project, None).unwrap().data(),
        built.data()
    );

    // An included file is an input: changing it changes the key.
    let cache_dir = temp_dir("patches-cache");
    let cache = Cache::new(cache_dir.clone());
    build::build_on(&base, &project, Some(&cache)).unwrap();
    write(&dir, "asm/shared.asm", "!value = $43\n");
    let rebuilt = build::build_on(&base, &project, Some(&cache)).unwrap();
    assert_eq!(rebuilt.read_u8(SnesAddr::new(0x0FF201)).unwrap(), 0x43);

    // So is one Asar finds from the project's folder, outside the patch's.
    write(&dir, "shared/v.asm", "!value = $44\n");
    write(
        &dir,
        "asm/late.asm",
        "lorom\nincsrc \"shared/v.asm\"\norg $0FF201\ndb !value\n",
    );
    let rebuilt = build::build_on(&base, &project, Some(&cache)).unwrap();
    assert_eq!(rebuilt.read_u8(SnesAddr::new(0x0FF201)).unwrap(), 0x44);
    let keys = build::stage_keys(&base, &project).unwrap();
    write(&dir, "shared/v.asm", "!value = $45\n");
    assert_ne!(build::stage_keys(&base, &project).unwrap(), keys);
    let rebuilt = build::build_on(&base, &project, Some(&cache)).unwrap();
    assert_eq!(rebuilt.read_u8(SnesAddr::new(0x0FF201)).unwrap(), 0x45);

    // What cannot be included does not change the key: the build's
    // output, a repository's files.
    let keys = build::stage_keys(&base, &project).unwrap();
    write(&dir, "build.sfc", "a ROM");
    write(&dir, ".git/index", "a commit");
    assert_eq!(build::stage_keys(&base, &project).unwrap(), keys);
    let _ = fs::remove_dir_all(&cache_dir);

    // A failing patch names itself.
    write(
        &dir,
        "asm/late.asm",
        "lorom\norg $0FF201\nnot an instruction\n",
    );
    let error = build::build_on(&base, &project, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("late.asm"), "{error}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn addmusick_inserts_the_music() {
    let Some(tool) = std::env::var_os("KOBO_ADDMUSICK") else {
        eprintln!("skipping: KOBO_ADDMUSICK is not set");
        return;
    };
    assert!(Path::new(&tool).is_dir(), "KOBO_ADDMUSICK must be a folder");
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let dir = temp_dir("music");
    fs::create_dir_all(dir.join("music")).unwrap();
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            music: Some(PathBuf::from("music")),
            ..Manifest::default()
        },
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    assert_eq!(built.read(SnesAddr::new(0x0E8000), 4).unwrap(), b"@AMK");
    assert_eq!(built.len(), 0x10_0000);
    assert_eq!(build::build(&clean, &project).unwrap().data(), built.data());
    let _ = fs::remove_dir_all(&dir);
}

/// With SA-1 Pack (`KOBO_SA1PACK`), Asar, and the vanilla ROM: the SA-1
/// base's levels, imported and built back as an SA-1 project, read back
/// the same and render the same as the base. The check of all 512
/// pictures is `render_hashes` (docs/testing.md).
#[test]
fn sa1_projects_build_onto_sa1_pack() {
    if std::env::var_os("KOBO_SA1PACK").is_none() {
        eprintln!("skipping: KOBO_SA1PACK is not set");
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let manifest = Manifest {
        sa1: true,
        ..Manifest::default()
    };
    let base = build::base_image(&clean, &manifest).unwrap();
    assert!(base.mapping().is_sa1());
    let dir = temp_dir("sa1");
    // An SA-1 ROM is compared with the SA-1 base, not the clean ROM.
    let report =
        kobo_core::import::import_rom(&base, &clean, &temp_dir("sa1-changed"), false).unwrap();
    assert!(report.levels.is_empty(), "{:?}", report.levels);
    let report = kobo_core::import::import_rom(&base, &clean, &dir, true).unwrap();
    assert_eq!(report.levels.len(), 512);
    assert!(report.unmodelled.is_empty(), "{:?}", report.unmodelled);
    let project = Project::load(&dir).unwrap();
    assert!(project.manifest.sa1);
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    assert!(kobo_core::import::diff_levels(&built, &base).is_empty());
    for level in [0x105, 0x0D3, 0x0CB, 0x1C7] {
        let options = kobo_core::render::RenderOptions::default();
        let a = kobo_core::render::render_level(&base, level, options).unwrap();
        let b = kobo_core::render::render_level(&built, level, options).unwrap();
        assert!(a.image.pixels == b.image.pixels, "level {level:03X}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sa1_images_past_4m_take_sa1_packs_larger_patches() {
    if std::env::var_os("KOBO_SA1PACK").is_none() {
        eprintln!("skipping: KOBO_SA1PACK is not set");
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let small = build::base_image(
        &clean,
        &Manifest {
            sa1: true,
            ..Manifest::default()
        },
    )
    .unwrap();
    for size in [0x60_0000, 0x80_0000] {
        let manifest = Manifest {
            sa1: true,
            rom_size: Some(size),
            ..Manifest::default()
        };
        let base = build::base_image(&clean, &manifest).unwrap();
        assert_eq!(base.len(), size);
        assert!(base.mapping().is_sa1());
        assert!(base.internal_header().checksum_pair_valid());
        // The levels read as they do on 4 MiB and under.
        assert!(kobo_core::import::diff_levels(&base, &small).is_empty());
    }
}

/// With UberASM Tool (configured, or the pinned build in the cache), Asar,
/// and the vanilla ROM: a project's level code goes in, the same bytes
/// twice.
#[test]
fn uberasm_inserts_level_code() {
    if common::tool(Tool::UberAsm, "KOBO_REQUIRE_UBERASM").is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let dir = temp_dir("uberasm");
    write(
        &dir,
        "uberasm/list.txt",
        "verbose: off\nlevel:\n105 kobo_test.asm\noverworld:\ngamemode:\n\
         global: other/global_code.asm\nstatusbar: other/status_code.asm\n\
         macrolib: other/macro_library.asm\nfreeram: $7FAC80\n",
    );
    write(
        &dir,
        "uberasm/level/kobo_test.asm",
        "main:\n    LDA #$42\n    STA $0DBF\n    RTL\n",
    );
    // What keeps an empty folder in git is not a library file.
    write(&dir, "uberasm/library/.gitkeep", "");
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            uberasm: Some(PathBuf::from("uberasm")),
            ..Manifest::default()
        },
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    let base = build::base_image(&clean, &project.manifest).unwrap();
    assert_ne!(built.data(), base.data());
    // The level's code went in.
    let code = [0xA9, 0x42, 0x8D, 0xBF, 0x0D, 0x6B];
    assert!(built.data().windows(code.len()).any(|w| w == code));
    assert!(!base.data().windows(code.len()).any(|w| w == code));
    assert_eq!(build::build(&clean, &project).unwrap().data(), built.data());

    // A folder without a list is refused, not run with the tool's own.
    fs::remove_file(dir.join("uberasm/list.txt")).unwrap();
    let error = build::build(&clean, &project).unwrap_err().to_string();
    assert!(error.contains("list.txt"), "{error}");
    let _ = fs::remove_dir_all(&dir);
}

/// UberASM Tool on an image with no game data, 1 MiB with the title it
/// checks for, so it runs without a ROM: CI runs Kobo's pinned build on
/// every platform, with the pinned Asar beside it.
#[test]
fn uberasm_runs_without_a_rom() {
    let Some(tool) = common::tool(Tool::UberAsm, "KOBO_REQUIRE_UBERASM") else {
        return;
    };
    let Some(asar) = common::tool(Tool::Asar, "KOBO_REQUIRE_ASAR") else {
        return;
    };
    let mut data = vec![0; 0x10_0000];
    data[0x7FC0..0x7FD5].copy_from_slice(b"SUPER MARIOWORLD     ");
    data[0x7FD5] = 0x20;
    data[0x7FD7] = 0x0A;
    let rom = kobo_core::Rom::from_bytes(data).unwrap();
    let dir = temp_dir("uberasm-synthetic");
    write(
        &dir,
        "list.txt",
        "verbose: off\nlevel:\n105 kobo_test.asm\noverworld:\ngamemode:\n\
         global: other/global_code.asm\nstatusbar: other/status_code.asm\n\
         macrolib: other/macro_library.asm\nfreeram: $7FAC80\n",
    );
    write(
        &dir,
        "level/kobo_test.asm",
        "main:\n    LDA #$42\n    STA $0DBF\n    RTL\n",
    );
    let out = kobo_core::tools::uberasm(&rom, &tool, &dir, &asar).unwrap();
    let code = [0xA9, 0x42, 0x8D, 0xBF, 0x0D, 0x6B];
    assert!(out.data().windows(code.len()).any(|w| w == code));
    let _ = fs::remove_dir_all(&dir);
}

/// With GPS (`KOBO_GPS`, a folder with the program built for the platform
/// and its files), the blocks stage inserts a block into Kobo's acts-like
/// chain: tile `$200` acts like `$025`, and its entries are GPS's.
#[test]
fn gps_inserts_blocks() {
    if std::env::var_os("KOBO_GPS").is_none() {
        eprintln!("skipping: KOBO_GPS is not set");
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let dir = temp_dir("gps");
    write(&dir, "blocks/list.txt", "200:0025 test.asm\n");
    write(
        &dir,
        "blocks/blocks/test.asm",
        "db $42\nJMP Done : JMP Done : JMP Done : JMP Done : JMP Done : JMP Done\n\
         JMP Done : JMP Done : JMP Done : JMP Done\nDone:\n    RTL\n",
    );
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            gps: Some(PathBuf::from("blocks")),
            ..Manifest::default()
        },
        levels: Vec::new(),
        map16: Vec::new(),
        map16_bg: Vec::new(),
        gfx: Vec::new(),
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    assert_eq!(
        kobo_core::map16::pages::acts_like(&built, 0x200).unwrap(),
        Some(0x025)
    );
    // GPS's own entry for a block touched from below.
    let installed = build::build_on(
        &clean,
        &Project {
            manifest: Manifest::default(),
            map16: vec![(0x02, Default::default())],
            ..project.clone()
        },
        None,
    )
    .unwrap();
    let entry = kobo_core::SnesAddr::new(0x06F690);
    assert_ne!(
        built.read(entry, 16).unwrap(),
        installed.read(entry, 16).unwrap()
    );
    assert_eq!(build::build(&clean, &project).unwrap().data(), built.data());
    let _ = fs::remove_dir_all(&dir);
}

/// With SA-1 Pack, Asar, and the vanilla ROM: an SA-1 project with `[rom]
/// lz3` has `$0FFFEB` set for LC_LZ3 before SA-1 Pack goes in, so that it
/// puts in its LC_LZ3 decompressor, and every GFX file written again as
/// LC_LZ3. The game's own routine (SA-1 Pack's) reads every table file back
/// as the clean ROM's, and levels load the same graphics and draw the same
/// as in an SA-1 build that keeps LC_LZ2. The check of all 512 pictures is
/// `render_hashes` (docs/testing.md). A LoROM project may not ask for it.
#[test]
fn sa1_projects_store_gfx_as_lz3() {
    if std::env::var_os("KOBO_SA1PACK").is_none() {
        eprintln!("skipping: KOBO_SA1PACK is not set");
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let project = |lz3| Project {
        manifest: Manifest {
            sa1: true,
            lz3,
            ..Manifest::default()
        },
        ..Default::default()
    };
    let lz2 = build::build(&clean, &project(false)).unwrap();
    let lz3 = build::build(&clean, &project(true)).unwrap();
    assert_eq!(
        lz3.read_u8(kobo_core::gfx::COMPRESSION_SETTING).unwrap(),
        kobo_core::gfx::COMPRESSION_LZ3
    );
    let vanilla = kobo_core::gfx::GfxReader::new(&clean).unwrap();
    let reader = kobo_core::gfx::GfxReader::new(&lz3).unwrap();
    assert_eq!(reader.compression(), kobo_core::gfx::Compression::Lz3);
    let (mut before, mut after) = (0, 0);
    for index in 0..kobo_core::gfx::GFX_FILE_COUNT {
        let want = vanilla.read(index).unwrap();
        let got = reader.read(index).unwrap();
        assert!(got.data == want.data, "GFX{index:02X}");
        before += want.compressed_len;
        after += got.compressed_len;
        if index < 0x32 {
            let read = kobo_core::expand::decompress_gfx_file(&lz3, index, want.data.len());
            assert!(
                read.unwrap() == want.data,
                "GFX{index:02X}: the game reads it differently"
            );
        }
    }
    eprintln!("GFX files: {after} bytes as LC_LZ3, {before} as the clean ROM's LC_LZ2");
    for level in [0x105, 0x0C7, 0x1C7] {
        let a = kobo_core::expand::expand_level(&lz2, level).unwrap();
        let b = kobo_core::expand::expand_level(&lz3, level).unwrap();
        assert!(a.video.vram == b.video.vram, "level {level:03X}: VRAM");
        let options = kobo_core::render::RenderOptions::default();
        let a = kobo_core::render::render_level(&lz2, level, options).unwrap();
        let b = kobo_core::render::render_level(&lz3, level, options).unwrap();
        assert!(a.image.pixels == b.image.pixels, "level {level:03X}");
    }
    let lorom = Project {
        manifest: Manifest {
            lz3: true,
            ..Manifest::default()
        },
        ..Default::default()
    };
    let error = build::build(&clean, &lorom).unwrap_err().to_string();
    assert!(error.contains("SA-1"), "{error}");
}

/// A PIXI folder with one sprite, `00`, with two extension bytes when its
/// extra bits say custom: it stores `$42` in `$0DBF` every frame.
fn pixi_files(dir: &Path) {
    write(dir, "pixi/list.txt", "00 test.cfg\n");
    write(
        dir,
        "pixi/sprites/test.cfg",
        "01\n36\n00 00 00 00 00 00\n00 00\ntest.asm\n2:0\n",
    );
    write(
        dir,
        "pixi/sprites/test.asm",
        "print \"INIT \",pc\n    RTL\nprint \"MAIN \",pc\n    LDA #$42\n    STA $0DBF\n    RTL\n",
    );
}

/// With PIXI (configured, or the pinned build in the cache), Asar, and
/// the vanilla ROM: the sprites stage inserts the project's sprites with
/// MeiMei off, after Kobo's install (PIXI refuses a ROM without the
/// acts-like pointer at `$06F624` and the `JML` at `$00F6E4`), and the
/// levels stage sizes each sprite entry by the table PIXI left.
#[test]
fn pixi_inserts_sprites() {
    if common::tool(Tool::Pixi, "KOBO_REQUIRE_PIXI").is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    let dir = temp_dir("pixi");
    pixi_files(&dir);
    let (mut level, _) = kobo_core::import::read_level(&clean, 0x105).unwrap();
    // In screen order, as a list must be.
    let at = level.sprites.list.iter().position(|s| s.x > 20).unwrap();
    level.sprites.list.insert(
        at,
        kobo_core::source::level::Sprite {
            id: 0x00,
            x: 20,
            y: 20,
            extra_bits: 2,
            extension: vec![0x12, 0x34],
        },
    );
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            pixi: Some(PathBuf::from("pixi")),
            ..Manifest::default()
        },
        levels: vec![(0x105, level.clone())],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    // PIXI's size table and marker, its sprite's code, and the level with
    // its sprite's two extension bytes.
    let sizes = kobo_core::sprites::pixi_size_table(&built)
        .unwrap()
        .unwrap();
    assert_eq!(sizes[2 * 0x100], 5);
    let code = [0xA9, 0x42, 0x8D, 0xBF, 0x0D, 0x6B];
    assert!(built.data().windows(code.len()).any(|w| w == code));
    assert_eq!(
        kobo_core::import::read_level(&built, 0x105).unwrap().0,
        level
    );
    assert_eq!(build::build(&clean, &project).unwrap().data(), built.data());
    // PIXI reads the version digits at $0FF0B4, which a build leaves $FF,
    // as a Lunar Magic 3 ROM's: its goal tape code for taller levels.
    assert_eq!(built.read(SnesAddr::new(0x0FF0B4), 4).unwrap(), [0xFF; 4]);

    // A folder without a list is refused, not run with PIXI's own.
    fs::remove_file(dir.join("pixi/list.txt")).unwrap();
    let error = build::build(&clean, &project).unwrap_err().to_string();
    assert!(error.contains("list.txt"), "{error}");
    let _ = fs::remove_dir_all(&dir);
}

/// PIXI on an image with no game data, 1 MiB with the bytes it checks for
/// (the acts-like pointer at `$06F624`, a `JML` at `$00F6E4`, and at
/// `$029B39` anything but the zero that means another patch), so it runs
/// without a ROM: CI runs Kobo's pinned build on every platform, with the
/// pinned Asar beside it.
#[test]
fn pixi_runs_without_a_rom() {
    let Some(tool) = common::tool(Tool::Pixi, "KOBO_REQUIRE_PIXI") else {
        return;
    };
    let Some(asar) = common::tool(Tool::Asar, "KOBO_REQUIRE_ASAR") else {
        return;
    };
    let mut data = vec![0; 0x10_0000];
    data[0x7FC0..0x7FD5].copy_from_slice(b"SUPER MARIOWORLD     ");
    data[0x7FD5] = 0x20;
    data[0x7FD7] = 0x0A;
    let mut rom = kobo_core::Rom::from_bytes(data).unwrap();
    rom.write(SnesAddr::new(0x06F624), &[0x00, 0x80, 0x11])
        .unwrap();
    rom.write(SnesAddr::new(0x00F6E4), &[0x5C, 0x00, 0x80, 0x12])
        .unwrap();
    rom.write(SnesAddr::new(0x029B39), &[0xFF, 0xFF]).unwrap();
    let dir = temp_dir("pixi-synthetic");
    pixi_files(&dir);
    let out = kobo_core::tools::run_pixi(&rom, &tool, &dir.join("pixi"), &asar).unwrap();
    let code = [0xA9, 0x42, 0x8D, 0xBF, 0x0D, 0x6B];
    assert!(out.data().windows(code.len()).any(|w| w == code));
    assert!(kobo_core::sprites::pixi_size_table(&out).unwrap().is_some());
    let _ = fs::remove_dir_all(&dir);
}

/// PIXI's 255-sprite option, on unless `-d255spl`, moves the load flags to
/// `$7FAF00` and clears bit 0 of `$0FFFE0`, which lets a build take a level
/// of up to 255 sprites; Kobo's loader then spawns the entries past 128,
/// setting PIXI's flags. Without PIXI, 129 are refused.
#[test]
fn pixi_lets_a_level_have_255_sprites() {
    if common::tool(Tool::Pixi, "KOBO_REQUIRE_PIXI").is_none() {
        return;
    }
    let Some(clean) = common::vanilla() else {
        return;
    };
    if common::asar().is_none() {
        return;
    }
    use kobo_core::ram::RamAddr;
    let dir = temp_dir("pixi-255");
    pixi_files(&dir);
    let (mut level, _) = kobo_core::import::read_level(&clean, 0x105).unwrap();
    level.sprites.list = (0..255)
        .map(|i| kobo_core::source::level::Sprite {
            id: 0x0F,
            x: 18 + i,
            y: 8 + i % 12,
            extra_bits: 0,
            extension: Vec::new(),
        })
        .collect();
    let project = Project {
        root: dir.clone(),
        manifest: Manifest {
            pixi: Some(PathBuf::from("pixi")),
            ..Manifest::default()
        },
        levels: vec![(0x105, level.clone())],
        ..Default::default()
    };
    let built = build::build(&clean, &project).unwrap();
    // A tool's output is never taken for a Lunar Magic save (clean room).
    assert!(!kobo_core::clean_room::saved_by_lunar_magic(&built));
    assert_eq!(kobo_core::sprites::max_sprites(&built), 255);
    assert_eq!(
        kobo_core::import::read_level(&built, 0x105).unwrap().0,
        level
    );
    let played = kobo_core::expand::play_level(&built, 0x105, 1500, |frame, ram| {
        let x = 0x30 + 3 * frame as u16;
        ram.set_u16(RamAddr::new(0x7E_0094), x);
        ram.set_u8(RamAddr::new(0x7E_0096), 0x40);
        ram.set_u8(RamAddr::new(0x7E_1497), 0x7F);
        for slot in 0..12 {
            ram.set_u8(RamAddr::new(0x7E_14C8 + slot), 0);
        }
    })
    .unwrap();
    let flags: Vec<u8> = (0..255).map(|i| played.peek(0x7F_AF00 + i)).collect();
    let loaded: Vec<usize> = (0..255).filter(|&i| flags[i] != 0).collect();
    assert!(loaded.iter().any(|&i| i > 200), "{loaded:?}");

    let without = Project {
        manifest: Manifest::default(),
        ..project
    };
    let error = build::build(&clean, &without).unwrap_err().to_string();
    assert!(error.contains("255-sprite loader"), "{error}");
    let _ = fs::remove_dir_all(&dir);
}
