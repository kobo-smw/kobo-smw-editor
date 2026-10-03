//! Shared helpers for ROM-backed integration tests.

use kobo_core::tools::{Tool, ToolError};
use kobo_core::{Rom, RomIdentity, SnesAddr, bps, config};

/// The configured vanilla ROM, or `None` (after printing why) so the
/// calling test can return early and pass.
#[allow(dead_code)]
pub fn vanilla() -> Option<Rom> {
    match config::vanilla_rom_path() {
        Ok(path) => {
            let rom = Rom::load(&path).expect("configured vanilla ROM must load");
            assert_eq!(
                rom.identify(),
                RomIdentity::VanillaUsa,
                "configured vanilla ROM has the wrong headerless SHA-1: {}",
                path.display()
            );
            Some(rom)
        }
        Err(config::ConfigError::NoVanillaRom) => {
            assert!(
                std::env::var_os("KOBO_REQUIRE_ROM").is_none(),
                "strict validation requires a vanilla ROM"
            );
            eprintln!("skipping: no vanilla ROM configured");
            None
        }
        Err(e) => panic!("invalid ROM configuration: {e}"),
    }
}

/// The ROM the emulator dumps being compared against were made from:
/// `KOBO_ORACLE_ROM`, or else the vanilla ROM.
#[allow(dead_code)]
pub fn oracle_rom() -> Option<Rom> {
    match std::env::var_os("KOBO_ORACLE_ROM") {
        Some(path) => Some(Rom::load(path).expect("KOBO_ORACLE_ROM must load")),
        None => vanilla(),
    }
}

/// Lunar Magic hack ROMs to exercise, from the `:`-separated paths in
/// `KOBO_LM_ROMS`, loaded one at a time. Empty (after printing why) when the
/// variable is unset. A `.bps` entry is a patch, applied to the configured
/// vanilla ROM.
#[allow(dead_code)]
pub fn lunar_magic_roms() -> impl Iterator<Item = (std::path::PathBuf, Rom)> {
    let paths: Vec<_> = match std::env::var_os("KOBO_LM_ROMS") {
        None => {
            eprintln!("skipping Lunar Magic ROMs: KOBO_LM_ROMS is not set");
            Vec::new()
        }
        Some(list) => {
            assert!(!list.is_empty(), "KOBO_LM_ROMS is set but empty");
            std::env::split_paths(&list).collect()
        }
    };
    let mut clean = None;
    paths.into_iter().map(move |p| {
        let is_patch = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bps"));
        let rom = if is_patch {
            let clean = clean.get_or_insert_with(|| {
                vanilla().expect("a .bps entry in KOBO_LM_ROMS needs the vanilla ROM")
            });
            let patch = std::fs::read(&p).expect("listed BPS patch must be readable");
            let patched =
                bps::apply_to_rom(&patch, clean).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            Rom::from_bytes(patched.data).expect("patched Lunar Magic ROM must load")
        } else {
            Rom::load(&p).expect("listed Lunar Magic ROM must load")
        };
        (p, rom)
    })
}

/// A 512 KiB LoROM image laid out as the vanilla ROM is where a build
/// reads it, with no game data: every level's layer data is an empty list
/// and every sprite list empty, the unused space in bank `$07` is `$FF`,
/// and Lunar Magic's gate is clear.
#[allow(dead_code)]
pub fn synthetic_base() -> Rom {
    use kobo_core::level::{LEVEL_COUNT, tables};
    let mut data = vec![0xFF; 0x8_0000];
    data[0x7FC0..0x7FD5].copy_from_slice(b"KOBO SYNTHETIC BASE  ");
    data[0x7FD5] = 0x20;
    data[0x7FD7] = 0x09;
    let mut rom = Rom::from_bytes(data).unwrap();
    let empty_objects = SnesAddr::new(0x068000);
    rom.write(empty_objects, &[0, 0, 0, 0, 0, 0xFF]).unwrap();
    let empty_sprites = SnesAddr::new(0x07C000);
    rom.write(empty_sprites, &[0x00, 0xFF]).unwrap();
    for n in 0..LEVEL_COUNT as u32 {
        rom.write_ptr(tables::LAYER1_PTRS.add(3 * n), empty_objects)
            .unwrap();
        rom.write_ptr(tables::LAYER2_PTRS.add(3 * n), empty_objects)
            .unwrap();
        rom.write_u16(tables::SPRITE_PTRS.add(2 * n), empty_sprites.offset())
            .unwrap();
        for table in tables::SECONDARY_HEADERS {
            rom.write_u8(table.add(n), 0).unwrap();
        }
    }
    rom.fix_checksum().unwrap();
    rom
}

/// The clean ROM with SA-1 Pack applied, as an SA-1 build's base stage
/// makes it, or `None` (after printing why): it needs SA-1 Pack
/// (`KOBO_SA1PACK`, or `tools.sa1pack` in the configuration file) and
/// Asar. `KOBO_REQUIRE_SA1PACK` makes a missing SA-1 Pack a failure.
#[allow(dead_code)]
pub fn sa1_base(clean: &Rom) -> Option<Rom> {
    tool(Tool::Sa1Pack, "KOBO_REQUIRE_SA1PACK")?;
    asar()?;
    let manifest = kobo_core::source::project::Manifest {
        sa1: true,
        ..Default::default()
    };
    Some(kobo_core::build::base_image(clean, &manifest).expect("SA-1 Pack must apply"))
}

/// `project` built onto the clean ROM, and again as an SA-1 project when
/// SA-1 Pack is configured: each build with the base it went onto, which
/// is what its levels that the project leaves alone draw as.
#[allow(dead_code)]
pub fn builds(clean: &Rom, project: &kobo_core::build::Project) -> Vec<(Rom, Rom)> {
    let mut out = vec![(
        kobo_core::build::base_image(clean, &project.manifest).unwrap(),
        kobo_core::build::build(clean, project).unwrap(),
    )];
    if !project.manifest.sa1 && sa1_base(clean).is_some() {
        let mut sa1 = project.clone();
        sa1.manifest.sa1 = true;
        out.push((
            kobo_core::build::base_image(clean, &sa1.manifest).unwrap(),
            kobo_core::build::build(clean, &sa1).unwrap(),
        ));
    }
    out
}

/// Whether a test runs as an SA-1 project too: when SA-1 Pack is
/// configured.
#[allow(dead_code)]
pub fn sa1_variants(clean: &Rom) -> Vec<bool> {
    std::iter::once(false)
        .chain(sa1_base(clean).map(|_| true))
        .collect()
}

/// The image a project builds onto, SA-1 Pack's with `sa1`: what its
/// levels are read from and the levels it leaves alone draw as.
#[allow(dead_code)]
pub fn base_as(clean: &Rom, sa1: bool) -> Rom {
    let manifest = kobo_core::source::project::Manifest {
        sa1,
        ..Default::default()
    };
    kobo_core::build::base_image(clean, &manifest).unwrap()
}

/// `project` built onto the clean ROM, as an SA-1 project with `sa1`.
#[allow(dead_code)]
pub fn build_as(
    clean: &Rom,
    project: &kobo_core::build::Project,
    sa1: bool,
) -> Result<Rom, kobo_core::build::BuildError> {
    let mut project = project.clone();
    project.manifest.sa1 = sa1;
    kobo_core::build::build(clean, &project)
}

/// The Asar library, or `None` (after printing why) so the calling test
/// can return early and pass: the configured one, or the pinned build if
/// the cache has it (`kobo tools fetch`). Tests never download it.
/// `KOBO_REQUIRE_ASAR` makes a missing library a failure.
#[allow(dead_code)]
pub fn asar() -> Option<kobo_core::asar::Asar> {
    let path = tool(Tool::Asar, "KOBO_REQUIRE_ASAR")?;
    Some(
        kobo_core::asar::Asar::load(&path)
            .unwrap_or_else(|e| panic!("Asar at {} must load: {e}", path.display())),
    )
}

/// A tool's path, configured or the pinned build already in the cache, or
/// `None` (after printing why). The variable `require` makes its absence
/// a failure.
#[allow(dead_code)]
pub fn tool(tool: Tool, require: &str) -> Option<std::path::PathBuf> {
    match tool.locate_offline() {
        Ok(located) => Some(located.path),
        Err(
            e @ (ToolError::NotCached { .. }
            | ToolError::NotConfigured { .. }
            | ToolError::NoBuild { .. }
            | ToolError::NoCache),
        ) => {
            assert!(
                std::env::var_os(require).is_none(),
                "{require} is set, and {tool} is missing: {e}"
            );
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("invalid {tool} configuration: {e}"),
    }
}

/// Lunar Magic's `-ExportAllMap16` file of a copy of `rom`.
#[allow(dead_code)]
pub fn export_map16(lunar_magic: &std::path::Path, rom: &Rom, name: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("kobo-lm-map16-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut headered = vec![0; 0x200];
    headered.extend_from_slice(rom.data());
    std::fs::write(dir.join("rom.smc"), headered).unwrap();
    let lm = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/lunar-magic/lm");
    let status = std::process::Command::new(&lm)
        .args(["-ExportAllMap16", "rom.smc", "all.map16"])
        .current_dir(&dir)
        .env("KOBO_LM_DIR", lunar_magic)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "Lunar Magic -ExportAllMap16: {status:?}"
    );
    let file = std::fs::read(dir.join("all.map16")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    file
}
