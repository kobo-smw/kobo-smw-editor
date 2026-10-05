//! The test suite's opt-in tiers: what each needs, and where it is found.
//!
//! Unit tests always run. Every other test needs something that is never
//! committed (the vanilla ROM, a tool, Lunar Magic, a hack corpus, emulator
//! dumps) and skips when it is missing, unless the tier is required:
//! `KOBO_REQUIRE_<TIER>` (`KOBO_REQUIRE_ROM`, `KOBO_REQUIRE_LUNAR_MAGIC`, ...),
//! or `KOBO_REQUIRE_ALL` for every tier. A tier's data comes from its
//! environment variable, then from the config file: `roms.smw`,
//! `tools.<key>`, or the `[tests]` table ([`crate::config::Tests`]).
//! `cargo xtask tiers` prints what this machine has; docs/testing.md has
//! what each tier checks.

use std::env;
use std::path::{Path, PathBuf};

use crate::config::{self, ConfigError, Setting};
use crate::tools::{Located, Tool, ToolError};

/// Turns every tier into a requirement.
pub const REQUIRE_ALL_ENV_VAR: &str = "KOBO_REQUIRE_ALL";
/// A file every skipped test appends a line to, for `cargo xtask verify`.
pub const SKIP_LOG_ENV_VAR: &str = "KOBO_SKIP_LOG";
/// Turns on the checks of every level (`tests.full_render`).
pub const FULL_RENDER_ENV_VAR: &str = "KOBO_FULL_RENDER";
/// The ROM the emulator dumps are of, if not the vanilla ROM.
pub const ORACLE_ROM_ENV_VAR: &str = "KOBO_ORACLE_ROM";

/// Something a group of tests needs that is never committed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tier {
    /// The vanilla ROM.
    Rom,
    /// A program of the toolchain, configured or the pinned build cached.
    Tool(Tool),
    /// Lunar Magic 3.70's folder, run under Wine.
    LunarMagic,
    /// The Lunar Magic hack corpus.
    LmRoms,
    /// Lunar Magic's MWL exports of the corpus.
    Mwl,
    /// Emulator dumps of every level.
    Oracle,
    /// Emulator dumps of the boss arenas.
    BossOracle,
    /// Emulator frames, for whole pictures.
    VideoOracle,
    /// SingleStepTests' 65816 suite.
    Cpu,
    /// The SA-1 reference ROM.
    Sa1Reference,
}

impl Tier {
    pub const ALL: [Tier; 15] = [
        Tier::Rom,
        Tier::Tool(Tool::Asar),
        Tier::Tool(Tool::Pixi),
        Tier::Tool(Tool::UberAsm),
        Tier::Tool(Tool::Sa1Pack),
        Tier::Tool(Tool::Gps),
        Tier::Tool(Tool::AddmusicK),
        Tier::LunarMagic,
        Tier::LmRoms,
        Tier::Mwl,
        Tier::Oracle,
        Tier::BossOracle,
        Tier::VideoOracle,
        Tier::Cpu,
        Tier::Sa1Reference,
    ];

    /// Its name, as `cargo xtask tiers` prints it and `KOBO_REQUIRE_<NAME>`
    /// spells it in capitals.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Rom => "rom",
            Tier::Tool(tool) => tool.key(),
            Tier::LunarMagic => "lunar_magic",
            Tier::LmRoms => "lm_roms",
            Tier::Mwl => "mwl",
            Tier::Oracle => "oracle",
            Tier::BossOracle => "boss_oracle",
            Tier::VideoOracle => "video_oracle",
            Tier::Cpu => "65816_tests",
            Tier::Sa1Reference => "sa1_reference",
        }
    }

    /// The environment variable that sets it.
    pub fn env_var(self) -> &'static str {
        match self {
            Tier::Rom => config::ROM_ENV_VAR,
            Tier::Tool(tool) => tool.env_var(),
            Tier::LunarMagic => "KOBO_LUNAR_MAGIC",
            Tier::LmRoms => "KOBO_LM_ROMS",
            Tier::Mwl => "KOBO_MWL_DIR",
            Tier::Oracle => "KOBO_ORACLE_DIR",
            Tier::BossOracle => "KOBO_BOSS_ORACLE_DIR",
            Tier::VideoOracle => "KOBO_VIDEO_ORACLE_DIRS",
            Tier::Cpu => "KOBO_65816_TESTS",
            Tier::Sa1Reference => "KOBO_SA1_REFERENCE",
        }
    }

    /// Its key in the config file.
    pub fn key(self) -> String {
        match self {
            Tier::Rom => "roms.smw".into(),
            Tier::Tool(tool) => format!("tools.{}", tool.key()),
            Tier::LunarMagic => "tests.lunar_magic".into(),
            Tier::LmRoms => "tests.lm_roms".into(),
            Tier::Mwl => "tests.mwl_dir".into(),
            Tier::Oracle => "tests.oracle_dir".into(),
            Tier::BossOracle => "tests.boss_oracle_dir".into(),
            Tier::VideoOracle => "tests.video_oracle_dirs".into(),
            Tier::Cpu => "tests.cpu_tests".into(),
            Tier::Sa1Reference => "tests.sa1_reference".into(),
        }
    }

    /// The variable that makes its absence a failure.
    pub fn require_var(self) -> String {
        format!("KOBO_REQUIRE_{}", self.name().to_ascii_uppercase())
    }

    /// Whether its absence fails a test: its own variable or
    /// [`REQUIRE_ALL_ENV_VAR`] is set.
    pub fn required(self) -> bool {
        env::var_os(self.require_var()).is_some() || env::var_os(REQUIRE_ALL_ENV_VAR).is_some()
    }

    /// What the tier holds and enables, in a line.
    pub fn describe(self) -> &'static str {
        match self {
            Tier::Rom => "the vanilla ROM: every ROM-backed test",
            Tier::Tool(Tool::Asar) => "Asar's library: Kobo's patches and builds",
            Tier::Tool(Tool::Pixi) => "PIXI: the sprites stage",
            Tier::Tool(Tool::UberAsm) => "UberASM Tool: the UberASM stage",
            Tier::Tool(Tool::Sa1Pack) => {
                "SA-1 Pack: SA-1 builds and every build test's SA-1 variant"
            }
            Tier::Tool(Tool::Gps) => "GPS: the blocks stage",
            Tier::Tool(Tool::AddmusicK) => "AddmusicK: the music stage",
            Tier::LunarMagic => {
                "Lunar Magic 3.70 (Wine): save checks, exports, Kobo's code against Lunar Magic's"
            }
            Tier::LmRoms => "the Lunar Magic hack corpus: every corpus test",
            Tier::Mwl => "MWL exports of the corpus: MWL round trips, size tables",
            Tier::Oracle => "emulator dumps: tile grids, layer 3, sprite slots",
            Tier::BossOracle => "emulator dumps of the boss arenas",
            Tier::VideoOracle => "emulator frames: whole pictures",
            Tier::Cpu => "SingleStepTests: every 65816 opcode",
            Tier::Sa1Reference => "the SA-1 reference ROM: its picture baseline",
        }
    }

    /// Where the tier's setting comes from, and its paths: one for most
    /// tiers, any number for [`Tier::LmRoms`] (folders expanded,
    /// [`expand_roms`]) and [`Tier::VideoOracle`]. `None` if it is not set.
    /// A tool's tier is the configured path or the cached pinned build
    /// ([`Tool::locate_offline`]).
    pub fn resolve(self) -> Result<Option<(Vec<PathBuf>, Origin)>, TierError> {
        if let Tier::Tool(tool) = self {
            return match tool.locate_offline() {
                Ok(Located { path, origin, .. }) => Ok(Some((vec![path], Origin::Tool(origin)))),
                Err(
                    ToolError::NotCached { .. }
                    | ToolError::NotConfigured { .. }
                    | ToolError::NoBuild { .. }
                    | ToolError::NoCache,
                ) => Ok(None),
                Err(e) => Err(TierError::Tool(e)),
            };
        }
        let list = matches!(self, Tier::LmRoms | Tier::VideoOracle);
        let set = match env::var_os(self.env_var()).filter(|v| !v.is_empty()) {
            Some(value) if list => Some((
                env::split_paths(&value).collect::<Vec<_>>(),
                Setting::Env(self.env_var()),
            )),
            Some(value) => Some((vec![PathBuf::from(value)], Setting::Env(self.env_var()))),
            None => {
                let config = config::load()?;
                let t = &config.tests;
                let paths = match self {
                    Tier::Rom => config.roms.smw.map(|p| vec![p]),
                    Tier::LunarMagic => t.lunar_magic.clone().map(|p| vec![p]),
                    Tier::LmRoms => t.lm_roms.clone(),
                    Tier::Mwl => t.mwl_dir.clone().map(|p| vec![p]),
                    Tier::Oracle => t.oracle_dir.clone().map(|p| vec![p]),
                    Tier::BossOracle => t.boss_oracle_dir.clone().map(|p| vec![p]),
                    Tier::VideoOracle => t.video_oracle_dirs.clone(),
                    Tier::Cpu => t.cpu_tests.clone().map(|p| vec![p]),
                    Tier::Sa1Reference => t.sa1_reference.clone().map(|p| vec![p]),
                    Tier::Tool(_) => unreachable!(),
                };
                paths.map(|paths| {
                    let setting = Setting::File {
                        key: self.key(),
                        path: config::config_path().unwrap_or_default(),
                    };
                    (paths.into_iter().map(expand_home).collect(), setting)
                })
            }
        };
        let Some((paths, setting)) = set else {
            return Ok(None);
        };
        if paths.is_empty() {
            return Err(TierError::Empty {
                tier: self.name(),
                setting,
            });
        }
        let paths = if self == Tier::LmRoms {
            expand_roms(&paths)?
        } else {
            paths
        };
        if paths.is_empty() {
            return Err(TierError::Empty {
                tier: self.name(),
                setting,
            });
        }
        Ok(Some((paths, Origin::Setting(setting))))
    }

    /// [`Tier::resolve`]'s first path.
    pub fn path(self) -> Result<Option<PathBuf>, TierError> {
        Ok(self
            .resolve()?
            .map(|(paths, _)| paths.into_iter().next().unwrap()))
    }
}

/// Where a tier's setting came from.
#[derive(Clone, Debug)]
pub enum Origin {
    Setting(Setting),
    Tool(crate::tools::Origin),
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Setting(setting) => write!(f, "{setting}"),
            Origin::Tool(crate::tools::Origin::Configured(setting)) => write!(f, "{setting}"),
            Origin::Tool(crate::tools::Origin::Pinned { release, .. }) => {
                write!(f, "the pinned build ({release}), cached")
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TierError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Tool(ToolError),
    #[error("{tier} is set by {setting}, which names nothing")]
    Empty {
        tier: &'static str,
        setting: Setting,
    },
    #[error("cannot list {path}: {source}")]
    Folder {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// `~/` at the start of a path from the config file, the home folder.
pub fn expand_home(path: PathBuf) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path,
    }
}

/// The corpus a list names: a file is itself, and a folder its ROMs
/// (`.smc`, `.sfc`) and `.bps` patches in name order, a patch left out
/// when a ROM of the same name is beside it (the patch applied); folders
/// are not searched further down.
pub fn expand_roms(entries: &[PathBuf]) -> Result<Vec<PathBuf>, TierError> {
    let mut out = Vec::new();
    for entry in entries {
        if !entry.is_dir() {
            out.push(entry.clone());
            continue;
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(entry)
            .map_err(|source| TierError::Folder {
                path: entry.clone(),
                source,
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        let is_rom = |p: &Path| has_extension(p, &["smc", "sfc"]);
        let applied = |p: &Path| {
            files
                .iter()
                .any(|r| is_rom(r) && r.file_stem() == p.file_stem())
        };
        out.extend(
            files
                .iter()
                .filter(|p| is_rom(p) || (has_extension(p, &["bps"]) && !applied(p)))
                .cloned(),
        );
    }
    Ok(out)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The emulator dumps' ROM: `KOBO_ORACLE_ROM`, then `tests.oracle_rom`;
/// `None` for the vanilla ROM.
pub fn oracle_rom() -> Result<Option<PathBuf>, ConfigError> {
    if let Some(p) = env::var_os(ORACLE_ROM_ENV_VAR).filter(|p| !p.is_empty()) {
        return Ok(Some(PathBuf::from(p)));
    }
    Ok(config::load()?.tests.oracle_rom.map(expand_home))
}

/// Whether the checks of every level run: `KOBO_FULL_RENDER`, then
/// `tests.full_render`.
pub fn full_render() -> Result<bool, ConfigError> {
    if let Some(v) = env::var_os(FULL_RENDER_ENV_VAR) {
        return Ok(!v.is_empty() && v != "0");
    }
    Ok(config::load()?.tests.full_render.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_variables_keep_their_names() {
        assert_eq!(Tier::Rom.require_var(), "KOBO_REQUIRE_ROM");
        assert_eq!(Tier::Tool(Tool::Asar).require_var(), "KOBO_REQUIRE_ASAR");
        assert_eq!(
            Tier::Tool(Tool::Sa1Pack).require_var(),
            "KOBO_REQUIRE_SA1PACK"
        );
        assert_eq!(
            Tier::Tool(Tool::UberAsm).require_var(),
            "KOBO_REQUIRE_UBERASM"
        );
        assert_eq!(Tier::LunarMagic.require_var(), "KOBO_REQUIRE_LUNAR_MAGIC");
        let names: std::collections::HashSet<_> = Tier::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(names.len(), Tier::ALL.len());
    }

    #[test]
    fn a_folder_lists_its_roms_and_the_patches_not_applied_beside_them() {
        let dir = env::temp_dir().join(format!("kobo-tiers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("roms")).unwrap();
        std::fs::create_dir_all(dir.join("patches/deeper")).unwrap();
        for f in [
            "roms/b.smc",
            "roms/a.SFC",
            "roms/a.bps",
            "roms/c.bps",
            "roms/notes.txt",
            "patches/x.bps",
        ] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        std::fs::write(dir.join("patches/deeper/y.bps"), b"").unwrap();
        let listed =
            expand_roms(&[dir.join("roms"), dir.join("patches"), dir.join("one.smc")]).unwrap();
        assert_eq!(
            listed,
            [
                dir.join("roms/a.SFC"),
                dir.join("roms/b.smc"),
                dir.join("roms/c.bps"),
                dir.join("patches/x.bps"),
                dir.join("one.smc"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn home_is_expanded_at_the_start_only() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/roms".into()), home.join("roms"));
        assert_eq!(expand_home("/a/~/b".into()), PathBuf::from("/a/~/b"));
        assert_eq!(expand_home("~user/x".into()), PathBuf::from("~user/x"));
    }
}
