//! The project's global ExAnimation list, open for editing: the list every
//! level runs unless its settings turn it off (`source::animation`'s global
//! file). A project with none gets `animation/global.toml`, named in the
//! manifest's `[animation]` when saved. Undo keeps whole snapshots.

use std::fs;
use std::path::{Path, PathBuf};

use crate::build::Project;
use crate::exanimation::{self, List};
use crate::source::Comments;
use crate::source::project::{MANIFEST, Manifest};

#[derive(Debug, thiserror::Error)]
pub enum AnimationError {
    #[error("ExAnimation: {0}")]
    Refused(String),
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
    list: Option<List>,
}

/// The global ExAnimation list, open.
#[derive(Clone, Debug)]
pub struct GlobalAnimation {
    path: PathBuf,
    listed: bool,
    comments: Comments,
    list: Option<List>,
    saved: Option<List>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl GlobalAnimation {
    /// Opens the project's global list, reading its file again for its
    /// comments.
    pub fn open(project: &Project) -> Result<Self, AnimationError> {
        let listed = project.manifest.animation_global.clone();
        let comments = match &listed {
            Some(file) => {
                let path = project.root.join(file);
                let text = fs::read_to_string(&path).map_err(|source| AnimationError::Io {
                    path: path.clone(),
                    source,
                })?;
                crate::source::animation::global_from_toml(&text)
                    .map_err(|source| AnimationError::File { path, source })?
                    .1
            }
            None => Comments::default(),
        };
        Ok(Self {
            path: listed
                .clone()
                .unwrap_or_else(|| PathBuf::from("animation").join("global.toml")),
            listed: listed.is_some(),
            comments,
            list: project.animation_global.clone(),
            saved: project.animation_global.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn list(&self) -> Option<&List> {
        self.list.as_ref()
    }

    /// Sets the list, or none, as one undo step. A list a build would
    /// refuse is refused.
    pub fn set(
        &mut self,
        label: impl Into<String>,
        list: Option<List>,
    ) -> Result<(), AnimationError> {
        if let Some(why) = list.as_ref().and_then(exanimation::refusal) {
            return Err(AnimationError::Refused(why));
        }
        if list != self.list {
            self.undo.push(Step {
                label: label.into(),
                list: std::mem::replace(&mut self.list, list),
            });
            self.redo.clear();
        }
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.list != self.saved
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let list = std::mem::replace(&mut self.list, step.list);
        self.redo.push(Step {
            label: step.label,
            list,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let list = std::mem::replace(&mut self.list, step.list);
        self.undo.push(Step {
            label: step.label,
            list,
        });
        true
    }

    /// Writes the list's file into the project's folder `root`, and names
    /// it in the manifest, or takes it out of the manifest when there is
    /// no list. Returns the files written.
    pub fn save(&mut self, root: &Path) -> Result<Vec<PathBuf>, AnimationError> {
        let mut written = Vec::new();
        if let Some(list) = &self.list {
            let path = root.join(&self.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| AnimationError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let text = crate::source::animation::global_to_toml(list, &self.comments);
            fs::write(&path, text).map_err(|source| AnimationError::Io {
                path: path.clone(),
                source,
            })?;
            written.push(path);
        }
        if self.listed != self.list.is_some() {
            let manifest_path = root.join(MANIFEST);
            let text = fs::read_to_string(&manifest_path).map_err(|source| AnimationError::Io {
                path: manifest_path.clone(),
                source,
            })?;
            let (mut manifest, comments) =
                Manifest::from_toml(&text).map_err(|source| AnimationError::File {
                    path: manifest_path.clone(),
                    source,
                })?;
            manifest.animation_global = self.list.as_ref().map(|_| self.path.clone());
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| {
                AnimationError::Io {
                    path: manifest_path.clone(),
                    source,
                }
            })?;
            written.push(manifest_path);
            self.listed = self.list.is_some();
        }
        self.saved = self.list.clone();
        Ok(written)
    }

    /// Puts the list into `project`, so that a build has the unsaved edits.
    pub fn apply_to(&self, project: &mut Project) {
        project.animation_global = self.list.clone();
        project.manifest.animation_global = self.list.as_ref().map(|_| self.path.clone());
    }
}
