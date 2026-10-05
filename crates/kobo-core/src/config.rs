//! Per-user configuration.
//!
//! The vanilla ROM never lives inside a project, so the path to it comes
//! from the user's environment: the `KOBO_SMW_ROM` variable first, then the
//! `roms.smw` key in the user config file. A tool's path is found the same
//! way (Asar's library from `KOBO_ASAR_LIB`, then `tools.asar`), and
//! overrides the build Kobo pins of it ([`crate::tools::Tool::locate`]).
//! The `[tests]` table is the test suite's: where its opt-in tiers find
//! their data ([`crate::tiers`]). Kobo itself never reads it.

use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

pub const ROM_ENV_VAR: &str = "KOBO_SMW_ROM";
pub const ASAR_ENV_VAR: &str = "KOBO_ASAR_LIB";
pub const ADDMUSICK_ENV_VAR: &str = "KOBO_ADDMUSICK";
pub const SA1PACK_ENV_VAR: &str = "KOBO_SA1PACK";
pub const UBERASM_ENV_VAR: &str = "KOBO_UBERASM";
pub const GPS_ENV_VAR: &str = "KOBO_GPS";
pub const PIXI_ENV_VAR: &str = "KOBO_PIXI";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "no vanilla SMW ROM configured; set {ROM_ENV_VAR} or add `roms.smw` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoVanillaRom,
    #[error(
        "no Asar library configured; set {ASAR_ENV_VAR} or add `tools.asar` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoAsar,
    #[error(
        "no AddmusicK configured; set {ADDMUSICK_ENV_VAR} or add `tools.addmusick` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoAddmusick,
    #[error(
        "no SA-1 Pack configured; set {SA1PACK_ENV_VAR} or add `tools.sa1pack` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoSa1Pack,
    #[error(
        "no UberASM Tool configured; set {UBERASM_ENV_VAR} or add `tools.uberasm` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoUberasm,
    #[error(
        "no GPS configured; set {GPS_ENV_VAR} or add `tools.gps` to {}",
        config_path().map(|p| p.display().to_string()).unwrap_or_default()
    )]
    NoGps,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub roms: Roms,
    #[serde(default)]
    pub tools: Tools,
    #[serde(default)]
    pub tests: Tests,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roms {
    /// Path to the vanilla SMW ROM.
    pub smw: Option<PathBuf>,
}

/// Paths to tools. One given here is used instead of the build Kobo pins,
/// and a build that runs it is no longer reproducible elsewhere.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    /// Path to Asar's shared library: `libasar.so`, `libasar.dylib`, or
    /// `asar.dll`.
    pub asar: Option<PathBuf>,
    /// Path to a PIXI folder: the program, built for the platform, and
    /// its files.
    pub pixi: Option<PathBuf>,
    /// Path to an AddmusicK folder: the program and the files it reads
    /// beside it. Kobo never bundles AddmusicK, which has no licence.
    pub addmusick: Option<PathBuf>,
    /// Path to an UberASM Tool folder: the program, built for the
    /// platform, and its files.
    pub uberasm: Option<PathBuf>,
    /// Path to a GPS folder: the program, built for the platform, and its
    /// files. Kobo never bundles GPS, which has no licence.
    pub gps: Option<PathBuf>,
    /// Path to an SA-1 Pack folder, the one holding `asm/sa1.asm`. Kobo
    /// never bundles SA-1 Pack, which has no licence.
    pub sa1pack: Option<PathBuf>,
}

/// Where the test suite's opt-in tiers find their data, each overridden
/// by an environment variable ([`crate::tiers::Tier`]). A path may start
/// with `~/`. Kobo itself never reads these.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tests {
    /// Lunar Magic hacks (`KOBO_LM_ROMS`): ROMs, `.bps` patches of the
    /// vanilla ROM, or folders of either.
    pub lm_roms: Option<Vec<PathBuf>>,
    /// Lunar Magic 3.70's folder (`KOBO_LUNAR_MAGIC`).
    pub lunar_magic: Option<PathBuf>,
    /// Lunar Magic's MWL exports (`KOBO_MWL_DIR`).
    pub mwl_dir: Option<PathBuf>,
    /// Emulator dumps (`KOBO_ORACLE_DIR`).
    pub oracle_dir: Option<PathBuf>,
    /// The ROM the dumps are of, if not the vanilla ROM (`KOBO_ORACLE_ROM`).
    pub oracle_rom: Option<PathBuf>,
    /// Emulator dumps of the boss arenas (`KOBO_BOSS_ORACLE_DIR`).
    pub boss_oracle_dir: Option<PathBuf>,
    /// Folders of emulator frames (`KOBO_VIDEO_ORACLE_DIRS`).
    pub video_oracle_dirs: Option<Vec<PathBuf>>,
    /// SingleStepTests' 65816 `v1` folder (`KOBO_65816_TESTS`).
    pub cpu_tests: Option<PathBuf>,
    /// The SA-1 reference ROM (`KOBO_SA1_REFERENCE`, docs/sa1.md).
    pub sa1_reference: Option<PathBuf>,
    /// Whether the slow checks of every level run (`KOBO_FULL_RENDER`).
    pub full_render: Option<bool>,
}

impl Tools {
    fn get(&self, key: &str) -> Option<&PathBuf> {
        match key {
            "asar" => self.asar.as_ref(),
            "pixi" => self.pixi.as_ref(),
            "addmusick" => self.addmusick.as_ref(),
            "uberasm" => self.uberasm.as_ref(),
            "gps" => self.gps.as_ref(),
            "sa1pack" => self.sa1pack.as_ref(),
            _ => None,
        }
    }
}

/// Where a configured path was set.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Setting {
    /// An environment variable.
    Env(&'static str),
    /// A key (`tools.asar`) in the config file at `path`.
    File { key: String, path: PathBuf },
}

impl fmt::Display for Setting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Setting::Env(var) => write!(f, "{var}"),
            Setting::File { key, path } => write!(f, "`{key}` in {}", path.display()),
        }
    }
}

/// Location of the user config file, if a config directory exists on this
/// platform.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("kobo").join("config.toml"))
}

/// Loads the user config. A missing file yields the default config.
pub fn load() -> Result<Config, ConfigError> {
    let Some(path) = config_path() else {
        return Ok(Config::default());
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(source) => return Err(ConfigError::Io { path, source }),
    };
    toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
}

/// Resolves the path to the vanilla SMW ROM.
pub fn vanilla_rom_path() -> Result<PathBuf, ConfigError> {
    if let Some(p) = env::var_os(ROM_ENV_VAR).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    load()?.roms.smw.ok_or(ConfigError::NoVanillaRom)
}

/// The path configured for a tool, and where: the environment variable
/// `env`, then `tools.<key>` in the config file. `None` if neither is set.
pub fn tool_path(
    key: &'static str,
    env: &'static str,
) -> Result<Option<(PathBuf, Setting)>, ConfigError> {
    if let Some(p) = env::var_os(env).filter(|p| !p.is_empty()) {
        return Ok(Some((PathBuf::from(p), Setting::Env(env))));
    }
    let config = load()?;
    Ok(config.tools.get(key).map(|p| {
        let setting = Setting::File {
            key: format!("tools.{key}"),
            path: config_path().unwrap_or_default(),
        };
        (p.clone(), setting)
    }))
}

fn configured(
    key: &'static str,
    env: &'static str,
    missing: ConfigError,
) -> Result<PathBuf, ConfigError> {
    tool_path(key, env)?.map(|(p, _)| p).ok_or(missing)
}

/// Resolves the path to the AddmusicK folder: `KOBO_ADDMUSICK`, then
/// `tools.addmusick`.
pub fn addmusick_path() -> Result<PathBuf, ConfigError> {
    configured("addmusick", ADDMUSICK_ENV_VAR, ConfigError::NoAddmusick)
}

/// Resolves the configured UberASM Tool folder: `KOBO_UBERASM`, then
/// `tools.uberasm`. [`crate::tools::Tool::locate`] falls back on the
/// pinned build.
pub fn uberasm_path() -> Result<PathBuf, ConfigError> {
    configured("uberasm", UBERASM_ENV_VAR, ConfigError::NoUberasm)
}

/// Resolves the path to the GPS folder: `KOBO_GPS`, then `tools.gps`.
pub fn gps_path() -> Result<PathBuf, ConfigError> {
    configured("gps", GPS_ENV_VAR, ConfigError::NoGps)
}

/// Resolves the path to the SA-1 Pack folder: `KOBO_SA1PACK`, then
/// `tools.sa1pack`.
pub fn sa1pack_path() -> Result<PathBuf, ConfigError> {
    configured("sa1pack", SA1PACK_ENV_VAR, ConfigError::NoSa1Pack)
}

/// Resolves the configured Asar library: `KOBO_ASAR_LIB`, then
/// `tools.asar`. [`crate::tools::Tool::locate`] falls back on the pinned
/// build.
pub fn asar_library_path() -> Result<PathBuf, ConfigError> {
    configured("asar", ASAR_ENV_VAR, ConfigError::NoAsar)
}
