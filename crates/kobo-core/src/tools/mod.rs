//! Running the toolchain's programs on a ROM.
//!
//! [`Tool::locate`] finds each: a path the user configured, or the build
//! Kobo pins, fetched on first use ([`pinned`]). Asar runs in the process
//! through its library ([`crate::asar`]). The others run on a copy of the
//! ROM in a copy of their folder; what they change is checked with
//! [`rats::Snapshot`] as Asar's patches are. docs/toolchain.md has what
//! each requires of a ROM.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::addr::SnesAddr;
use crate::config::ConfigError;
use crate::rats::{self, Damage};
use crate::rom::{Rom, RomError};

pub mod pinned;

pub use pinned::{Located, Origin, Tool};

/// Finds PIXI: the configured folder (`KOBO_PIXI`, `tools.pixi`), or the
/// pinned build, fetched on first use. The folder holds `pixi` and its
/// files, and no Asar library, which the stage lays beside it.
pub fn pixi() -> Result<Located, ToolError> {
    Tool::Pixi.locate()
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(
        "no {tool} configured; set {env} or `tools.{key}` in the config file (Kobo never \
         bundles {tool}, which has no licence)"
    )]
    NotConfigured {
        tool: &'static str,
        env: &'static str,
        key: &'static str,
    },
    #[error(
        "Kobo pins no build of {tool} for {platform}; set {env} or `tools.{key}` in the \
         config file"
    )]
    NoBuild {
        tool: &'static str,
        platform: &'static str,
        env: &'static str,
        key: &'static str,
    },
    #[error("no cache directory for downloaded tools; set KOBO_TOOL_CACHE")]
    NoCache,
    #[error(
        "{tool} {version} is not a version Kobo pins ({pinned}); configure a path to it, or \
         take a pinned one"
    )]
    NoVersion {
        tool: &'static str,
        version: String,
        pinned: String,
    },
    #[error(
        "{tool} {version} is not in the tool cache ({}) and downloads are off; run \
         `kobo tools fetch` online, or configure a path to it",
        cache.display()
    )]
    NotCached {
        tool: &'static str,
        version: String,
        cache: PathBuf,
    },
    #[error(
        "{file} is not in the tool cache ({}) and downloads are off (KOBO_OFFLINE)",
        cache.display()
    )]
    ReleaseNotCached { file: String, cache: PathBuf },
    #[error("downloading {url}: {message}")]
    Download { url: String, message: String },
    #[error("{url} is not the build Kobo pins: expected {expected}, got {found}")]
    Checksum {
        url: String,
        expected: String,
        found: String,
    },
    #[error("{}: {message}", path.display())]
    Archive { path: PathBuf, message: String },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{tool} failed (exit {status}):\n{output}")]
    Failed {
        tool: &'static str,
        status: String,
        output: String,
    },
    #[error("{tool} damaged {} tagged blocks; the first: {}", damage.len(), damage[0])]
    Damaged {
        tool: &'static str,
        damage: Vec<Damage>,
    },
    #[error("{} is missing", .0.display())]
    Missing(PathBuf),
    #[error(transparent)]
    Rom(#[from] RomError),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ToolError + '_ {
    move |source| ToolError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Hashes a file or a folder tree: every file's path relative to `root`
/// and its contents, in sorted order, so the hash changes whenever what a
/// tool could read there does.
pub fn hash_tree(hash: &mut Sha1, root: &Path) -> Result<(), ToolError> {
    hash_tree_except(hash, root, &|_| false)
}

/// [`hash_tree`] without the files `skip` gives true for, by their paths
/// relative to `root`.
pub fn hash_tree_except(
    hash: &mut Sha1,
    root: &Path,
    skip: &dyn Fn(&Path) -> bool,
) -> Result<(), ToolError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut Vec::new(), &mut files)?;
    files.retain(|f| !skip(f));
    files.sort();
    for relative in files {
        // A file root is itself, with an empty relative path.
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&relative)
        };
        hash.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(())
}

/// The files under `at`, following links but not into a folder it is
/// already inside (`open`, canonical paths), which would never end.
fn collect_files(
    root: &Path,
    at: &Path,
    open: &mut Vec<PathBuf>,
    files: &mut Vec<PathBuf>,
) -> Result<(), ToolError> {
    let meta = fs::metadata(at).map_err(io_error(at))?;
    if meta.is_file() {
        files.push(at.strip_prefix(root).unwrap_or(at).to_path_buf());
        return Ok(());
    }
    let canonical = fs::canonicalize(at).map_err(io_error(at))?;
    if open.contains(&canonical) {
        return Ok(());
    }
    open.push(canonical);
    for entry in fs::read_dir(at).map_err(io_error(at))? {
        let entry = entry.map_err(io_error(at))?;
        collect_files(root, &entry.path(), open, files)?;
    }
    open.pop();
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), ToolError> {
    if fs::metadata(from).map_err(io_error(from))?.is_file() {
        fs::copy(from, to).map_err(io_error(from))?;
        return Ok(());
    }
    fs::create_dir_all(to).map_err(io_error(to))?;
    for entry in fs::read_dir(from).map_err(io_error(from))? {
        let entry = entry.map_err(io_error(from))?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// Copies the files under `from` that `keep` takes, by their path from
/// `from`, into `to`.
pub(crate) fn copy_tree_where(
    from: &Path,
    to: &Path,
    keep: &dyn Fn(&Path) -> bool,
) -> Result<(), ToolError> {
    fn walk(
        root: &Path,
        at: &Path,
        to: &Path,
        keep: &dyn Fn(&Path) -> bool,
    ) -> Result<(), ToolError> {
        for entry in fs::read_dir(at).map_err(io_error(at))? {
            let path = entry.map_err(io_error(at))?.path();
            let relative = path.strip_prefix(root).expect("under the root");
            if path.is_dir() {
                walk(root, &path, to, keep)?;
            } else if keep(relative) {
                let target = to.join(relative);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(io_error(parent))?;
                }
                fs::copy(&path, &target).map_err(io_error(&path))?;
            }
        }
        Ok(())
    }
    walk(from, from, to, keep)
}

/// Lays a project's folder over a tool's copy, leaving out hidden files
/// such as `.gitkeep`, which UberASM Tool would take for a library file.
fn copy_overlay(from: &Path, to: &Path) -> Result<(), ToolError> {
    fs::create_dir_all(to).map_err(io_error(to))?;
    for entry in fs::read_dir(from).map_err(io_error(from))? {
        let entry = entry.map_err(io_error(from))?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let target = to.join(entry.file_name());
        if fs::metadata(&path).map_err(io_error(&path))?.is_dir() {
            copy_overlay(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

/// The name Callisto's resources include its header by.
pub const CALLISTO_HEADER: &str = "callisto.asm";

/// In every `.asm` file under `dir`, points an include of `callisto.asm`
/// by that name (`"callisto.asm"`, in any case) at `header`, by its full
/// path. The tools pass Asar no include paths, so it would look for the
/// file in the including file's folder, where Callisto's own Asar finds it
/// anywhere (`build::callisto_header`); a copy of it there would be taken
/// for a resource by a tool that assembles every file of a folder
/// (UberASM Tool's `library/`). Only build copies are changed.
pub(crate) fn point_callisto_includes(dir: &Path, header: &Path) -> Result<(), ToolError> {
    let name = format!("\"{CALLISTO_HEADER}\"");
    let target = format!("\"{}\"", header.to_string_lossy().replace('\\', "/"));
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let path = entry.map_err(io_error(dir))?.path();
        if path.is_dir() {
            point_callisto_includes(&path, header)?;
            continue;
        }
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("asm"))
        {
            continue;
        }
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        let mut changed = false;
        while i < bytes.len() {
            if bytes[i..].len() >= name.len()
                && bytes[i..i + name.len()].eq_ignore_ascii_case(name.as_bytes())
            {
                out.extend_from_slice(target.as_bytes());
                i += name.len();
                changed = true;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        if changed {
            fs::write(&path, out).map_err(io_error(&path))?;
        }
    }
    Ok(())
}

/// A scratch folder, removed when dropped.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Result<Self, ToolError> {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("kobo-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The name the tools load Asar's library by on this platform, and the
/// name it has in Kobo's pinned build.
pub const ASAR_LIBRARY_NAME: &str = if cfg!(windows) {
    "asar.dll"
} else if cfg!(target_os = "macos") {
    "libasar.dylib"
} else {
    "libasar.so"
};

/// The variable the loader searches for shared libraries by. Windows
/// looks beside the program first, and needs none.
const LIBRARY_PATH_VAR: &str = if cfg!(target_os = "macos") {
    "DYLD_LIBRARY_PATH"
} else {
    "LD_LIBRARY_PATH"
};

/// A program run in a copy of its own folder, the way the toolchain's
/// programs expect: with the project's files laid over the copy and Asar's
/// library beside it, on `rom.sfc` there.
struct FolderTool<'a> {
    name: &'static str,
    /// The program's file name, without `.exe`.
    program: &'static str,
    args: &'a [&'a str],
    /// Files the copy must not keep, such as an options file that would
    /// replace the arguments.
    remove: &'a [&'a str],
    /// Blocks the tool may rewrite in place, as their interface has it:
    /// damage to them is not damage.
    owns: fn(&Rom) -> Vec<SnesAddr>,
}

impl FolderTool<'_> {
    fn run(
        &self,
        rom: &Rom,
        tool: &Path,
        overlay: &Path,
        asar: &Path,
        callisto: Option<&Path>,
    ) -> Result<Rom, ToolError> {
        let scratch = Scratch::new(self.program)?;
        let work = &scratch.0;
        copy_tree(tool, work)?;
        copy_overlay(overlay, work)?;
        if let Some(header) = callisto {
            point_callisto_includes(work, header)?;
        }
        for file in self.remove {
            let _ = fs::remove_file(work.join(file));
        }
        // A tool folder that has Asar's library keeps its own: upstream's
        // Windows releases are 32-bit, with a 32-bit library, where Kobo's is
        // the 64-bit one it loads itself. Kobo's pinned builds carry none.
        let library = work.join(ASAR_LIBRARY_NAME);
        if !library.exists() {
            fs::copy(asar, &library).map_err(io_error(asar))?;
        }
        let rom_path = work.join("rom.sfc");
        rom.save(&rom_path)?;
        let program = work.join(if cfg!(windows) {
            format!("{}.exe", self.program)
        } else {
            self.program.to_owned()
        });
        let mut command = Command::new(&program);
        command
            .args(self.args)
            .current_dir(work)
            .stdin(Stdio::null())
            // .NET programs need no ICU this way, which some systems lack.
            .env("DOTNET_SYSTEM_GLOBALIZATION_INVARIANT", "1");
        if !cfg!(windows) {
            command.env(LIBRARY_PATH_VAR, work);
        }
        let output = command.output().map_err(io_error(&program))?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(ToolError::Failed {
                tool: self.name,
                status: output.status.to_string(),
                output: text,
            });
        }
        let after = Rom::from_headerless(fs::read(&rom_path).map_err(io_error(&rom_path))?)?;
        // By file offset: a pointer may name a block through a mirror.
        let owned: Vec<_> = (self.owns)(rom)
            .into_iter()
            .filter_map(|at| rom.pc(at).ok())
            .collect();
        let damage: Vec<_> = rats::Snapshot::take(rom)
            .check(&after, &[])
            .into_iter()
            .filter(|d| rom.pc(d.block.start).is_ok_and(|pc| !owned.contains(&pc)))
            .collect();
        if !damage.is_empty() {
            return Err(ToolError::Damaged {
                tool: self.name,
                damage,
            });
        }
        Ok(after)
    }
}

/// Runs GPS on a copy of `rom`, in a copy of its folder `tool` with the
/// project's GPS folder (`list.txt`, `blocks/`, `routines/`) laid over it.
/// GPS patches the acts-like chain in bank `$06`, which Kobo's install has
/// in the shape GPS expects (docs/toolchain.md).
pub fn gps(
    rom: &Rom,
    tool: &Path,
    files: &Path,
    asar: &Path,
    callisto: Option<&Path>,
) -> Result<Rom, ToolError> {
    FolderTool {
        name: "GPS",
        program: "gps",
        args: &["rom.sfc"],
        remove: &[],
        // GPS applies its list to the acts-like tables in place.
        owns: |rom| {
            use crate::map16::pages::{ACTS_LIKE, ACTS_LIKE_UPPER};
            let mut owned = Vec::new();
            if let Ok(at) = rom.read_u24(ACTS_LIKE) {
                owned.push(SnesAddr::new(at));
            }
            // The pointer to pages $40 on is kept less $8000.
            if let Ok(at) = rom.read_u24(ACTS_LIKE_UPPER).map(|a| a + 0x8000) {
                owned.push(SnesAddr::new(at));
            }
            owned
        },
    }
    .run(rom, tool, files, asar, callisto)
}

/// What a PIXI folder holds as input: its list, the sprites of each type,
/// and the shared routines and the user's additions to its patches
/// (PIXI's `src/config.h`, `DefaultPaths`). The rest of a PIXI folder is
/// PIXI itself: the program, its patches, its documentation.
pub const PIXI_INPUTS: &[&str] = &[
    "list.txt",
    "sprites",
    "shooters",
    "generators",
    "extended",
    "cluster",
    "misc_sprites",
    "routines",
    "asm/ExtraDefines",
    "asm/ExtraHijacks",
];

/// Copies the inputs of the PIXI folder `from` ([`PIXI_INPUTS`]) into
/// `to`, for a project to build them with Kobo's PIXI, and returns those
/// it found. A folder without a list is refused, as a build would refuse
/// it.
pub fn copy_pixi_inputs(from: &Path, to: &Path) -> Result<Vec<&'static str>, ToolError> {
    let list = from.join("list.txt");
    if !list.is_file() {
        return Err(ToolError::Missing(list));
    }
    let mut found = Vec::new();
    for &input in PIXI_INPUTS {
        let path = from.join(input);
        if !path.exists() {
            continue;
        }
        let target = to.join(input);
        if path.is_dir() {
            copy_overlay(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io_error(parent))?;
            }
            fs::copy(&path, &target).map_err(io_error(&path))?;
        }
        found.push(input);
    }
    Ok(found)
}

/// Runs PIXI on a copy of `rom`, in a copy of its folder `tool` with the
/// project's PIXI folder (`list.txt`, `sprites/`, `routines/`, ...) laid
/// over it: without prompts, and with MeiMei off, since it reads the
/// wrong bytes of a headerless image and Kobo's level writer sizes sprite
/// entries by PIXI's table itself (docs/toolchain.md). The files for
/// Lunar Magic's sprite display (`.ssc`, `.mwt`, `.mw2`, `.s16`) are not
/// kept.
pub fn run_pixi(
    rom: &Rom,
    tool: &Path,
    files: &Path,
    asar: &Path,
    callisto: Option<&Path>,
) -> Result<Rom, ToolError> {
    // Without one, PIXI would run its own folder's list.
    let list = files.join("list.txt");
    if !list.is_file() {
        return Err(ToolError::Missing(list));
    }
    let run = |args: &[&str]| {
        FolderTool {
            name: "PIXI",
            program: "pixi",
            args,
            remove: &[],
            owns: |_| Vec::new(),
        }
        .run(rom, tool, files, asar, callisto)
    };
    match run(&["--script-mode", "-meimei-off", "-l", "list.txt", "rom.sfc"]) {
        // PIXI before 1.43 has no `--script-mode`; with no input to read,
        // its prompts take their defaults, which the option would have.
        Err(ToolError::Failed { output, .. }) if output.contains("\"--script-mode\"") => {
            run(&["-meimei-off", "-l", "list.txt", "rom.sfc"])
        }
        result => result,
    }
}

/// Runs AddmusicK on a copy of `rom`: in a copy of the AddmusicK folder
/// `tool`, with the project's music folder `music` laid over it and
/// Asar's library `asar` beside it, as AddmusicK wants.
pub fn addmusick(
    rom: &Rom,
    tool: &Path,
    music: &Path,
    asar: &Path,
    callisto: Option<&Path>,
) -> Result<Rom, ToolError> {
    FolderTool {
        name: "AddmusicK",
        program: "AddmusicK",
        args: &["-noblock", "rom.sfc"],
        // AddmusicK reads its options file in place of its arguments.
        remove: &["Addmusic_options.txt"],
        owns: |_| Vec::new(),
    }
    .run(rom, tool, music, asar, callisto)
}

/// Runs UberASM Tool on a copy of `rom`, in a copy of its folder `tool`
/// with the project's UberASM folder (`list.txt`, `level/`, `library/`,
/// ...) laid over it. Upstream's release is built for 32-bit Windows;
/// Kobo's pinned builds are 64-bit, with Asar's native library laid beside
/// them (docs/toolchain.md).
pub fn uberasm(
    rom: &Rom,
    tool: &Path,
    files: &Path,
    asar: &Path,
    callisto: Option<&Path>,
) -> Result<Rom, ToolError> {
    // Without one, the tool would run its own folder's list.
    let list = files.join("list.txt");
    if !list.is_file() {
        return Err(ToolError::Missing(list));
    }
    FolderTool {
        name: "UberASM Tool",
        program: "UberASMTool",
        args: &["list.txt", "rom.sfc"],
        remove: &[],
        owns: |_| Vec::new(),
    }
    .run(rom, tool, files, asar, callisto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_of_callistos_header_point_at_one_file() {
        let dir = Scratch::new("test-callisto").unwrap();
        let library = dir.0.join("library");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("a.asm"), "incsrc \"Callisto.asm\"\ndb 0\n").unwrap();
        fs::write(library.join("b.asm"), "incsrc \"other.asm\"\n").unwrap();
        fs::write(library.join("c.txt"), "incsrc \"callisto.asm\"\n").unwrap();
        let header = Path::new("/scratch/root/callisto.asm");
        point_callisto_includes(&dir.0, header).unwrap();
        let read = |name: &str| fs::read_to_string(library.join(name)).unwrap();
        assert_eq!(
            read("a.asm"),
            "incsrc \"/scratch/root/callisto.asm\"\ndb 0\n"
        );
        assert_eq!(read("b.asm"), "incsrc \"other.asm\"\n");
        assert_eq!(read("c.txt"), "incsrc \"callisto.asm\"\n");
        // Nothing is added beside the files.
        assert_eq!(fs::read_dir(&library).unwrap().count(), 3);
    }
}
