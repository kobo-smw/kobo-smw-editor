//! The overworld, open for editing: the clean ROM's overworld with the
//! project's changes (`source::overworld`), as one [`Overworld`] the editor
//! changes whole. The file holds what differs from the clean ROM's, worked
//! out again after every change, so a tile set back to the clean ROM's
//! leaves it; a project with no file gets `overworld.toml`, named in the
//! manifest when saved. Undo keeps whole snapshots of the changes.

use std::fs;
use std::path::{Path, PathBuf};

use crate::build::Project;
use crate::overworld::{Changes, Overworld, OverworldError};
use crate::rom::Rom;
use crate::source::overworld as file;
use crate::source::project::{MANIFEST, Manifest};

#[derive(Debug, thiserror::Error)]
pub enum OverworldEditError {
    #[error(transparent)]
    Overworld(#[from] OverworldError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        source: crate::source::SourceError,
    },
}

#[derive(Clone, Debug)]
struct Step {
    label: String,
    changes: Changes,
}

/// The project's overworld, open.
#[derive(Clone, Debug)]
pub struct OverworldDocument {
    /// The file, relative to the project's folder.
    path: PathBuf,
    /// Whether the manifest names it.
    listed: bool,
    top: Vec<String>,
    /// The clean ROM's overworld, in Lunar Magic's layout's shape.
    clean: Overworld,
    /// The clean ROM's with the changes.
    current: Overworld,
    changes: Changes,
    saved: Changes,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl OverworldDocument {
    /// Opens the overworld of the project in memory, whose clean ROM is
    /// `clean`, reading the file again for its comments.
    pub fn open(project: &Project, clean: &Rom) -> Result<Self, OverworldEditError> {
        let listed = project.manifest.overworld.clone();
        let top = match &listed {
            Some(name) => {
                let path = project.root.join(name);
                let text = fs::read_to_string(&path).map_err(|source| OverworldEditError::Io {
                    path: path.clone(),
                    source,
                })?;
                file::from_toml(&text)
                    .map_err(|source| OverworldEditError::File { path, source })?
                    .1
            }
            None => Vec::new(),
        };
        let clean = Overworld::read(clean)?.in_lunar_magic_shape();
        let changes = project.overworld.clone().unwrap_or_default();
        let current = clean.clone().with(&changes)?;
        Ok(Self {
            path: listed
                .clone()
                .unwrap_or_else(|| PathBuf::from("overworld.toml")),
            listed: listed.is_some(),
            top,
            clean,
            current,
            saved: changes.clone(),
            changes,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// The overworld as the project has it.
    pub fn overworld(&self) -> &Overworld {
        &self.current
    }

    /// The clean ROM's.
    pub fn clean(&self) -> &Overworld {
        &self.clean
    }

    pub fn changes(&self) -> &Changes {
        &self.changes
    }

    /// Changes the overworld with `change`, as one undo step; or, with
    /// `amend`, as part of the last one (a stroke being drawn).
    pub fn change(
        &mut self,
        label: impl Into<String>,
        amend: bool,
        change: impl FnOnce(&mut Overworld),
    ) -> Result<(), OverworldEditError> {
        let mut next = self.current.clone();
        change(&mut next);
        let mut changes = next.changes_from(&self.clean);
        // What only the changes hold, which the overworld read gives none of.
        changes.graphics = self.changes.graphics.clone();
        changes.reveal_speed = self.changes.reveal_speed;
        self.commit(label, amend, changes)
    }

    /// Changes what only the changes hold (the submaps' graphics lists, the
    /// path reveal speed) with `change`, as one undo step.
    pub fn change_settings(
        &mut self,
        label: impl Into<String>,
        change: impl FnOnce(&mut Changes),
    ) -> Result<(), OverworldEditError> {
        let mut changes = self.changes.clone();
        change(&mut changes);
        self.commit(label, false, changes)
    }

    fn commit(
        &mut self,
        label: impl Into<String>,
        amend: bool,
        changes: Changes,
    ) -> Result<(), OverworldEditError> {
        if changes == self.changes {
            return Ok(());
        }
        // What the changes make, which is what a build of them makes.
        let current = self.clean.clone().with(&changes)?;
        let before = std::mem::replace(&mut self.changes, changes);
        self.current = current;
        if !amend || self.undo.is_empty() {
            self.undo.push(Step {
                label: label.into(),
                changes: before,
            });
        }
        self.redo.clear();
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.changes != self.saved
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    fn restore(&mut self, changes: Changes) -> Changes {
        // The changes came from this document, so they apply.
        if let Ok(current) = self.clean.clone().with(&changes) {
            self.current = current;
        }
        std::mem::replace(&mut self.changes, changes)
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let changes = self.restore(step.changes);
        self.redo.push(Step {
            label: step.label,
            changes,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let changes = self.restore(step.changes);
        self.undo.push(Step {
            label: step.label,
            changes,
        });
        true
    }

    /// Writes the file into the project's folder `root`, and names it in
    /// the manifest when it is new. Returns the files written.
    pub fn save(&mut self, root: &Path) -> Result<Vec<PathBuf>, OverworldEditError> {
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| OverworldEditError::Io { path, source }
        };
        let path = root.join(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        fs::write(&path, file::to_toml(&self.changes, &self.top)).map_err(io(&path))?;
        let mut written = vec![path];
        if !self.listed {
            let manifest_path = root.join(MANIFEST);
            let text = fs::read_to_string(&manifest_path).map_err(io(&manifest_path))?;
            let (mut manifest, comments) =
                Manifest::from_toml(&text).map_err(|source| OverworldEditError::File {
                    path: manifest_path.clone(),
                    source,
                })?;
            manifest.overworld = Some(self.path.clone());
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(io(&manifest_path))?;
            written.push(manifest_path);
            self.listed = true;
        }
        self.saved = self.changes.clone();
        Ok(written)
    }

    /// Puts the changes into `project`, so that a build has the unsaved
    /// edits: a project whose overworld is the clean ROM's and has no file
    /// keeps the game's layout.
    pub fn apply_to(&self, project: &mut Project) {
        if self.changes.is_empty() && !self.listed {
            project.overworld = None;
        } else {
            project.overworld = Some(self.changes.clone());
            project.manifest.overworld = Some(self.path.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overworld::layer1_index;
    use crate::rom::RomIdentity;

    fn vanilla() -> Option<Rom> {
        let rom = Rom::load(crate::config::vanilla_rom_path().ok()?).ok()?;
        (rom.identify() == RomIdentity::VanillaUsa).then_some(rom)
    }

    /// What only the changes hold outlives an edit of the overworld, and
    /// changes as an undo step of its own.
    #[test]
    fn graphics_lists_and_the_reveal_speed_outlive_an_edit() {
        let Some(clean) = vanilla() else {
            eprintln!("skipping: no vanilla ROM configured");
            return;
        };
        let mut project = Project::default();
        let mut changes = Changes {
            reveal_speed: Some(6),
            ..Default::default()
        };
        changes
            .graphics
            .insert(1, crate::exgfx::GraphicsList([0x8014; 16]));
        project.overworld = Some(changes.clone());
        let mut doc = OverworldDocument::open(&project, &clean).unwrap();
        let at = layer1_index(0, 3, 4);
        doc.change("Paint", false, |ow| ow.layer1[at] = 0x58)
            .unwrap();
        assert_eq!(doc.changes().reveal_speed, Some(6));
        assert_eq!(doc.changes().graphics, changes.graphics);
        doc.change_settings("Speed", |c| c.reveal_speed = Some(0x10))
            .unwrap();
        assert_eq!(doc.changes().reveal_speed, Some(0x10));
        assert_eq!(doc.changes().layer1.len(), 1);
        assert!(doc.undo());
        assert_eq!(doc.changes().reveal_speed, Some(6));
    }

    /// A tile changed is a row of the file; set back, it leaves; a stroke
    /// is one undo step.
    #[test]
    fn a_tile_set_back_leaves_the_file_and_a_stroke_is_one_step() {
        let Some(clean) = vanilla() else {
            eprintln!("skipping: no vanilla ROM configured");
            return;
        };
        let project = Project::default();
        let mut doc = OverworldDocument::open(&project, &clean).unwrap();
        assert!(doc.changes().is_empty());
        let at = layer1_index(0, 3, 4);
        let was = doc.overworld().layer1[at];
        doc.change("Paint", false, |ow| ow.layer1[at] = 0x58)
            .unwrap();
        doc.change("Paint", true, |ow| ow.layer1[at + 1] = 0x58)
            .unwrap();
        assert_eq!(doc.changes().layer1.len(), 1);
        assert!(doc.is_modified());
        assert_eq!(doc.undo_label(), Some("Paint"));
        assert!(doc.undo());
        assert!(doc.changes().is_empty());
        assert_eq!(doc.overworld().layer1[at], was);
        assert!(!doc.undo());
        assert!(doc.redo());
        assert_eq!(doc.overworld().layer1[at + 1], 0x58);
        doc.change("Back", false, |ow| {
            ow.layer1[at] = was;
            ow.layer1[at + 1] = clean_tile(&clean, at + 1);
        })
        .unwrap();
        assert!(doc.changes().is_empty());
        // A project given no overworld keeps the game's layout.
        let mut project = Project::default();
        doc.apply_to(&mut project);
        assert!(project.overworld.is_none());
    }

    fn clean_tile(clean: &Rom, at: usize) -> u16 {
        Overworld::read(clean).unwrap().layer1[at]
    }
}
