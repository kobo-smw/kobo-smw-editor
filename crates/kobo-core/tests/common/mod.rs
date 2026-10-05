//! Shared helpers for ROM-backed integration tests: the tiers' gates
//! ([`kobo_core::tiers`]), the bases builds go onto, and the helpers
//! several test files share. Each test binary uses some of them.
#![allow(dead_code)]

pub mod failures;
pub mod fixtures;
pub mod lm;
pub mod render_hashes;
pub mod swap;
pub mod temp;

use std::path::{Path, PathBuf};

use kobo_core::tiers::{self, Tier};
use kobo_core::tools::{Tool, ToolError};
use kobo_core::{Rom, RomIdentity, SnesAddr, bps, config};

/// `f` of every item, in their order, on every core: a corpus test's
/// levels are independent, and one core takes it most of a full run.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    results.lock().unwrap()[i] = Some(r);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("every item ran"))
        .collect()
}

/// Records that the running test skips for want of `tier`, after printing
/// why, or fails if the tier is required (`KOBO_REQUIRE_<TIER>`,
/// `KOBO_REQUIRE_ALL`). With `KOBO_SKIP_LOG` set, the skip is appended to
/// that file, which `cargo xtask verify` reports: libtest hides a passing
/// test's output, so a skip is otherwise invisible.
pub fn skip(tier: Tier, why: impl std::fmt::Display) {
    assert!(
        !tier.required(),
        "{} is set, and the {} tier is missing: {why}",
        if std::env::var_os(tier.require_var()).is_some() {
            tier.require_var()
        } else {
            tiers::REQUIRE_ALL_ENV_VAR.to_owned()
        },
        tier.name()
    );
    eprintln!("skipping: {why}");
    log_skip(tier.name(), &why.to_string());
}

/// Appends a skip to `KOBO_SKIP_LOG`: the test, the tier, and why.
fn log_skip(tier: &str, why: &str) {
    use std::io::Write;
    let Some(log) = std::env::var_os(tiers::SKIP_LOG_ENV_VAR) else {
        return;
    };
    let binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .map(|s| {
            s.rsplit_once('-')
                .map_or(s.clone(), |(name, _)| name.to_owned())
        })
        .unwrap_or_default();
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("?");
    let line = format!(
        "{binary}::{test}\t{tier}\t{}\n",
        why.replace(['\t', '\n'], " ")
    );
    // One write of one short line: appends from parallel tests do not mix.
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// A tier's paths, or `None` (after [`skip`]) when it is not set.
pub fn tier_paths(tier: Tier) -> Option<Vec<PathBuf>> {
    match tier.resolve() {
        Ok(Some((paths, _))) => Some(paths),
        Ok(None) => {
            skip(
                tier,
                format_args!("{} is not set, nor `{}`", tier.env_var(), tier.key()),
            );
            None
        }
        Err(e) => panic!("invalid {} configuration: {e}", tier.name()),
    }
}

/// A tier's path, or `None` (after [`skip`]) when it is not set.
pub fn tier_path(tier: Tier) -> Option<PathBuf> {
    tier_paths(tier).map(|paths| paths.into_iter().next().unwrap())
}

/// A test that found nothing to check in its tier (no hack of the corpus
/// in a fixture, say) says so, which fails it when the tier is required:
/// a test that checked nothing has not passed. Nothing is said for a
/// tier that is not set, which has said so already.
pub fn none_checked(tier: Tier, checked: usize, what: impl std::fmt::Display) {
    if checked == 0 && tier.resolve().is_ok_and(|r| r.is_some()) {
        skip(tier, format_args!("nothing to check: {what}"));
    }
}

/// The configured vanilla ROM, or `None` (after [`skip`]) so the calling
/// test can return early and pass.
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
            skip(Tier::Rom, "no vanilla ROM configured");
            None
        }
        Err(e) => panic!("invalid ROM configuration: {e}"),
    }
}

/// The ROM the emulator dumps being compared against were made from:
/// `KOBO_ORACLE_ROM` (`tests.oracle_rom`), or else the vanilla ROM.
pub fn oracle_rom() -> Option<Rom> {
    match tiers::oracle_rom().expect("valid configuration") {
        Some(path) => Some(Rom::load(&path).expect("the oracle ROM must load")),
        None => vanilla(),
    }
}

/// Lunar Magic 3.70's folder, or `None` (after [`skip`]): its tests run
/// it under Wine and `xvfb-run`, so on Linux only.
pub fn lunar_magic() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        skip(
            Tier::LunarMagic,
            "Lunar Magic's tests run it under Wine, on Linux only",
        );
        return None;
    }
    tier_path(Tier::LunarMagic)
}

/// Lunar Magic hack ROMs to exercise, from `KOBO_LM_ROMS` or
/// `tests.lm_roms` ([`kobo_core::tiers::expand_roms`]), loaded one at a
/// time, the vanilla ROM left out. Empty (after [`skip`]) when neither is
/// set. A `.bps` entry is a patch, applied to the configured vanilla ROM.
pub fn lunar_magic_roms() -> impl Iterator<Item = (PathBuf, Rom)> {
    let paths = tier_paths(Tier::LmRoms).unwrap_or_default();
    let mut clean = None;
    paths.into_iter().filter_map(move |p| {
        let rom = load_corpus_rom(&p, &mut clean);
        (rom.identify() != RomIdentity::VanillaUsa).then_some((p, rom))
    })
}

/// The corpus's paths, unloaded: for a test that loads them on every core
/// ([`par_map`] and [`load_corpus_rom`]).
pub fn lunar_magic_rom_paths() -> Vec<PathBuf> {
    tier_paths(Tier::LmRoms).unwrap_or_default()
}

/// A corpus entry: a ROM, or a `.bps` patch applied to the vanilla ROM,
/// which `clean` keeps once loaded.
pub fn load_corpus_rom(path: &Path, clean: &mut Option<Rom>) -> Rom {
    let is_patch = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bps"));
    if is_patch {
        let clean = clean.get_or_insert_with(|| {
            vanilla().expect("a .bps entry in the corpus needs the vanilla ROM")
        });
        let patch = std::fs::read(path).expect("listed BPS patch must be readable");
        let patched =
            bps::apply_to_rom(&patch, clean).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Rom::from_bytes(patched.data).expect("patched Lunar Magic ROM must load")
    } else {
        Rom::load(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }
}

/// Whether the checks of every level run (`KOBO_FULL_RENDER`, or
/// `tests.full_render`): a test that draws a few levels of a build to
/// compare with its base draws all 512 ([`every_level_draws_the_same`]).
pub fn full_render() -> bool {
    tiers::full_render().expect("valid configuration")
}

/// With the checks of every level on ([`full_render`]), every level of `b`
/// draws as in `a`, drawn and as markers, but those in `except`, which
/// must still differ: a fixed exception comes off the list. Off, nothing.
pub fn every_level_draws_the_same(a: &Rom, b: &Rom, except: &[u16], what: &str) {
    if !full_render() {
        return;
    }
    let levels: Vec<u16> = (0..0x200).collect();
    let differ = render_hashes::pictures_differ(a, b, &levels);
    let hex = |l: &[u16]| {
        l.iter()
            .map(|l| format!("{l:03X}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let unexpected: Vec<String> = differ
        .iter()
        .filter(|(l, _)| !except.contains(l))
        .map(|(l, which)| format!("{l:03X} ({which})"))
        .collect();
    let fixed: Vec<u16> = except
        .iter()
        .copied()
        .filter(|l| !differ.iter().any(|(d, _)| d == l))
        .collect();
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "{what}: levels that draw differently: {}; listed as different but the same: {}",
        unexpected.join(", "),
        hex(&fixed)
    );
    eprintln!(
        "{what}: all 512 levels draw the same{}",
        if except.is_empty() {
            String::new()
        } else {
            format!(" but {}", hex(except))
        }
    );
}

/// Corpus hacks some tests are written around: (name, headerless SHA-1).
pub mod hacks {
    pub const GRAND_POO_WORLD_2: (&str, &str) = (
        "Grand Poo World 2",
        "390583d5faa0cc02e0c4f414f7638228661b2dc9",
    );
    pub const INVICTUS: (&str, &str) = ("Invictus 1.0", "6dd24c31b5d8c568aab0de6d68855f609cbe8f08");
}

/// The corpus hack with headerless SHA-1 `sha1`, for a test written
/// around one hack's content; `None` (after [`skip`]) when the corpus
/// does not hold it, which fails the test when the corpus is required.
pub fn corpus_hack((name, sha1): (&str, &str)) -> Option<(PathBuf, Rom)> {
    let found = lunar_magic_roms().find(|(_, rom)| rom.sha1_hex() == sha1);
    // Without a corpus, lunar_magic_roms has said so already.
    if found.is_none() && Tier::LmRoms.resolve().is_ok_and(|r| r.is_some()) {
        skip(
            Tier::LmRoms,
            format_args!("the corpus does not hold {name} ({sha1})"),
        );
    }
    found
}

/// A small deterministic generator for seeded scenarios: a failure names
/// its seed, and the same seed gives the same cases on every platform.
pub struct Lcg(pub u64);

impl Lcg {
    /// A number below `n`.
    pub fn next(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as u32
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.next(100) < percent
    }

    /// One of `list`.
    pub fn pick<T: Copy>(&mut self, list: &[T]) -> T {
        list[self.next(list.len() as u32) as usize]
    }
}

/// A number from the environment variable `var` (a seed or a count of
/// cases), or `default` when it is not set.
pub fn env_number<T: std::str::FromStr>(var: &str, default: T) -> T {
    match std::env::var(var) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("{var} must be a number, not {value:?}")),
        Err(_) => default,
    }
}

/// A 512 KiB LoROM image laid out as the vanilla ROM is where a build
/// reads it, with no game data: every level's layer data is an empty list
/// and every sprite list empty, the unused space in bank `$07` is `$FF`,
/// and Lunar Magic's gate is clear.
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
pub fn sa1_base(clean: &Rom) -> Option<Rom> {
    tool(Tool::Sa1Pack)?;
    asar()?;
    Some(base_as(clean, true))
}

/// `project` built onto the clean ROM, and again as an SA-1 project when
/// SA-1 Pack is configured: each build with the base it went onto, which
/// is what its levels that the project leaves alone draw as.
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
pub fn sa1_variants(clean: &Rom) -> Vec<bool> {
    std::iter::once(false)
        .chain(sa1_base(clean).map(|_| true))
        .collect()
}

/// The image a project builds onto, SA-1 Pack's with `sa1`: what its
/// levels are read from and the levels it leaves alone draw as. Each is
/// made once a process: SA-1 Pack takes Asar a while.
pub fn base_as(clean: &Rom, sa1: bool) -> Rom {
    use std::sync::Mutex;
    type Key = ([u8; 20], bool);
    static BASES: Mutex<Vec<(Key, Vec<u8>)>> = Mutex::new(Vec::new());
    let key = (clean.sha1(), sa1);
    let copy = |bytes: &[u8]| Rom::from_headerless(bytes.to_vec()).unwrap();
    if let Some((_, base)) = BASES.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return copy(base);
    }
    let manifest = kobo_core::source::project::Manifest {
        sa1,
        ..Default::default()
    };
    let base = kobo_core::build::base_image(clean, &manifest).expect("the base must build");
    BASES.lock().unwrap().push((key, base.data().to_vec()));
    base
}

/// `project` built onto the clean ROM, as an SA-1 project with `sa1`.
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
pub fn asar() -> Option<kobo_core::asar::Asar> {
    let path = tool(Tool::Asar)?;
    Some(
        kobo_core::asar::Asar::load(&path)
            .unwrap_or_else(|e| panic!("Asar at {} must load: {e}", path.display())),
    )
}

/// A tool's path, configured or the pinned build already in the cache, or
/// `None` (after [`skip`]). `KOBO_REQUIRE_<TOOL>` makes its absence a
/// failure.
pub fn tool(tool: Tool) -> Option<PathBuf> {
    tool_version(tool, None)
}

/// [`tool`] at `version`, one kobo-tools builds beside the default.
pub fn tool_version(tool: Tool, version: Option<&str>) -> Option<PathBuf> {
    match tool.locate_version_offline(version) {
        Ok(located) => Some(located.path),
        Err(
            e @ (ToolError::NotCached { .. }
            | ToolError::NotConfigured { .. }
            | ToolError::NoBuild { .. }
            | ToolError::NoCache),
        ) => {
            skip(Tier::Tool(tool), e);
            None
        }
        Err(e) => panic!("invalid {tool} configuration: {e}"),
    }
}
