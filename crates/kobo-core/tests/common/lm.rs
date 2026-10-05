//! Lunar Magic's command line, run headlessly under Wine through
//! `tools/lunar-magic/lm` (Linux only: [`super::lunar_magic`] gates it).
//! Always on a copy of a ROM, in a workspace of its own.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use kobo_core::Rom;

use super::temp::TempDir;

/// One Lunar Magic at a time in a test binary: the runs share a Wine
/// prefix, and `xvfb-run -a` can hand two of them the same display. The
/// wrapper also takes a lock of its own, for other processes.
static RUNNING: Mutex<()> = Mutex::new(());

/// A folder holding `rom.smc`, the copy Lunar Magic works on, and, for a
/// save, the clean ROM where Lunar Magic's restore system wants it
/// (`sysLMRestore/smwOrig.smc`). Removed when dropped.
pub struct Workspace {
    dir: TempDir,
    lunar_magic: PathBuf,
}

impl Workspace {
    /// A workspace with `rom` as `rom.smc`, and `clean` as Lunar Magic's
    /// original for a save. An export of a whole ROM wants `header`, a
    /// 512-byte copier header: Lunar Magic asks before it touches a
    /// headerless ROM that way, which a headless run never gets past.
    pub fn new(
        lunar_magic: &Path,
        name: &str,
        rom: &Rom,
        clean: Option<&Rom>,
        header: bool,
    ) -> Workspace {
        let dir = TempDir::new(&format!("lm-{name}"));
        if let Some(clean) = clean {
            std::fs::create_dir_all(dir.join("sysLMRestore")).unwrap();
            std::fs::write(dir.join("sysLMRestore/smwOrig.smc"), headered(clean)).unwrap();
        }
        let bytes = if header {
            headered(rom)
        } else {
            rom.data().to_vec()
        };
        std::fs::write(dir.join("rom.smc"), bytes).unwrap();
        Workspace {
            dir,
            lunar_magic: lunar_magic.to_path_buf(),
        }
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Runs Lunar Magic with `args` in the workspace, and requires it to
    /// succeed.
    pub fn run(&self, args: &[&str]) {
        run(&self.lunar_magic, &self.dir, args);
    }

    /// `rom.smc` as Lunar Magic left it.
    pub fn rom(&self) -> Rom {
        Rom::load(self.dir.join("rom.smc")).unwrap()
    }
}

/// Runs Lunar Magic with `args` in `dir`, and requires it to succeed.
pub fn run(lunar_magic: &Path, dir: &Path, args: &[&str]) {
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/lunar-magic/lm");
    let _running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    let output = Command::new(&wrapper)
        .args(args)
        .current_dir(dir)
        .env("KOBO_LM_DIR", lunar_magic)
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", wrapper.display()));
    assert!(
        output.status.success(),
        "Lunar Magic {args:?}: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
}

/// `rom` with a zero copier header in front.
pub fn headered(rom: &Rom) -> Vec<u8> {
    let mut out = vec![0; 0x200];
    out.extend_from_slice(rom.data());
    out
}

/// Lunar Magic's `-ExportAllMap16` file of a copy of `rom`.
pub fn export_map16(lunar_magic: &Path, rom: &Rom, name: &str) -> Vec<u8> {
    let ws = Workspace::new(lunar_magic, &format!("map16-{name}"), rom, None, true);
    ws.run(&["-ExportAllMap16", "rom.smc", "all.map16"]);
    std::fs::read(ws.path().join("all.map16")).unwrap()
}
