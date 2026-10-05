//! Watching the project's folder, so a change made outside the editor
//! (a text editor, `git checkout`, a script) is followed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use notify::{RecursiveMode, Watcher as _};

pub struct Watcher {
    /// Kept for as long as the folder is watched.
    _watcher: notify::RecommendedWatcher,
    changes: Receiver<PathBuf>,
}

impl Watcher {
    pub fn new(root: &Path, ctx: egui::Context) -> notify::Result<Self> {
        let (send, changes) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                if !(event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove()) {
                    return;
                }
                for path in event.paths {
                    let _ = send.send(path);
                }
                ctx.request_repaint();
            })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            changes,
        })
    }

    /// The files that changed since the last call, each once.
    pub fn changed(&self) -> BTreeSet<PathBuf> {
        self.changes.try_iter().collect()
    }
}
