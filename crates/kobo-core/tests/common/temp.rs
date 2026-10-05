//! Scratch folders that go away with the test, even one that panics.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Keeps a failed test's scratch folders, for a look at what it wrote.
pub const KEEP_ENV_VAR: &str = "KOBO_KEEP_TEMP";

/// A folder under the system's temporary folder, removed when dropped:
/// on success, and on a panic unless `KOBO_KEEP_TEMP` is set, which
/// prints where it was left. Lunar Magic's workspaces hold a copy of the
/// vanilla ROM, so none is left behind by accident.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty folder named after `name`, apart from every other.
    pub fn new(name: &str) -> TempDir {
        let dir = TempDir::unmade(name);
        std::fs::create_dir_all(&dir.0).unwrap();
        dir
    }

    /// [`TempDir::new`]'s path, not made yet: for an operation that
    /// requires its output folder not to exist.
    pub fn unmade(name: &str) -> TempDir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        // `kobo-test-`: the library's own scratch folders are `kobo-<name>-`,
        // and one of the same name would empty this one.
        let dir = std::env::temp_dir().join(format!("kobo-test-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::thread::panicking() && std::env::var_os(KEEP_ENV_VAR).is_some() {
            eprintln!("kept {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
