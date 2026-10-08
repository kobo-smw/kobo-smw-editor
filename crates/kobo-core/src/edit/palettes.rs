//! The shared palettes, open for editing: the game's colour tables as the
//! project changes them (`source::palettes`). A colour set back to the
//! clean ROM's leaves the file; a project with no file gets
//! `palettes/shared.toml`, named in the manifest when saved. Undo keeps
//! whole snapshots, as a level's does.

use std::fs;
use std::path::{Path, PathBuf};

use crate::build::Project;
use crate::palette::Color15;
use crate::rom::{Rom, RomError};
use crate::source::palettes::{self, COLOURS, SharedPalettes};
use crate::source::project::{MANIFEST, Manifest};

#[derive(Debug, thiserror::Error)]
pub enum PalettesError {
    #[error("colour {0} is past the shared palettes' {COLOURS}")]
    NoColour(u16),
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
    #[error(transparent)]
    Rom(#[from] RomError),
}

#[derive(Clone, Debug)]
struct Step {
    label: String,
    palettes: SharedPalettes,
}

/// The shared palettes, open.
#[derive(Clone, Debug)]
pub struct PalettesDocument {
    /// The file, relative to the project's folder.
    path: PathBuf,
    /// Whether the manifest names it.
    listed: bool,
    top: Vec<String>,
    /// The clean ROM's colours.
    clean: Vec<Color15>,
    palettes: SharedPalettes,
    saved: SharedPalettes,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl PalettesDocument {
    /// Opens the shared palettes of the project in memory, whose clean ROM
    /// is `clean`, reading the file again for its comments.
    pub fn open(project: &Project, clean: &Rom) -> Result<Self, PalettesError> {
        let listed = project.manifest.shared_palettes.clone();
        let top = match &listed {
            Some(file) => {
                let path = project.root.join(file);
                let text = fs::read_to_string(&path).map_err(|source| PalettesError::Io {
                    path: path.clone(),
                    source,
                })?;
                SharedPalettes::from_toml(&text)
                    .map_err(|source| PalettesError::File { path, source })?
                    .1
            }
            None => Vec::new(),
        };
        Ok(Self {
            path: listed
                .clone()
                .unwrap_or_else(|| PathBuf::from("palettes").join("shared.toml")),
            listed: listed.is_some(),
            top,
            clean: palettes::read_all(clean)?,
            palettes: project.shared_palettes.clone(),
            saved: project.shared_palettes.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Colour `n` as the project has it.
    pub fn colour(&self, n: u16) -> Color15 {
        self.palettes
            .colours
            .get(&n)
            .copied()
            .unwrap_or_else(|| self.clean_colour(n))
    }

    /// Colour `n` as the clean ROM has it.
    pub fn clean_colour(&self, n: u16) -> Color15 {
        self.clean.get(usize::from(n)).copied().unwrap_or_default()
    }

    pub fn is_changed(&self, n: u16) -> bool {
        self.palettes.colours.contains_key(&n)
    }

    fn set_in(
        &self,
        palettes: &mut SharedPalettes,
        n: u16,
        colour: Color15,
    ) -> Result<(), PalettesError> {
        if n >= COLOURS {
            return Err(PalettesError::NoColour(n));
        }
        if colour == self.clean_colour(n) {
            palettes.colours.remove(&n);
        } else {
            palettes.colours.insert(n, colour);
        }
        Ok(())
    }

    /// Sets colour `n`, as one undo step.
    pub fn set(
        &mut self,
        label: impl Into<String>,
        n: u16,
        colour: Color15,
    ) -> Result<(), PalettesError> {
        let mut palettes = self.palettes.clone();
        self.set_in(&mut palettes, n, colour)?;
        if palettes != self.palettes {
            self.undo.push(Step {
                label: label.into(),
                palettes: std::mem::replace(&mut self.palettes, palettes),
            });
            self.redo.clear();
        }
        Ok(())
    }

    /// Sets colour `n` as part of the last step: a value being dragged.
    pub fn amend(&mut self, n: u16, colour: Color15) -> Result<(), PalettesError> {
        if self.undo.is_empty() {
            return self.set("Change a shared colour", n, colour);
        }
        let mut palettes = self.palettes.clone();
        self.set_in(&mut palettes, n, colour)?;
        self.palettes = palettes;
        self.redo.clear();
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.palettes != self.saved
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
        let palettes = std::mem::replace(&mut self.palettes, step.palettes);
        self.redo.push(Step {
            label: step.label,
            palettes,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let palettes = std::mem::replace(&mut self.palettes, step.palettes);
        self.undo.push(Step {
            label: step.label,
            palettes,
        });
        true
    }

    /// Writes the file into the project's folder `root`, and names it in
    /// the manifest when it is new. Returns the files written.
    pub fn save(&mut self, root: &Path) -> Result<Vec<PathBuf>, PalettesError> {
        let path = root.join(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| PalettesError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, self.palettes.to_toml(&self.top)).map_err(|source| PalettesError::Io {
            path: path.clone(),
            source,
        })?;
        let mut written = vec![path];
        if !self.listed {
            let manifest_path = root.join(MANIFEST);
            let text = fs::read_to_string(&manifest_path).map_err(|source| PalettesError::Io {
                path: manifest_path.clone(),
                source,
            })?;
            let (mut manifest, comments) =
                Manifest::from_toml(&text).map_err(|source| PalettesError::File {
                    path: manifest_path.clone(),
                    source,
                })?;
            manifest.shared_palettes = Some(self.path.clone());
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| {
                PalettesError::Io {
                    path: manifest_path.clone(),
                    source,
                }
            })?;
            written.push(manifest_path);
            self.listed = true;
        }
        self.saved = self.palettes.clone();
        Ok(written)
    }

    /// Puts the colours into `project`, so that a build has the unsaved
    /// edits.
    pub fn apply_to(&self, project: &mut Project) {
        project.shared_palettes = self.palettes.clone();
        if !self.palettes.is_empty() || self.listed {
            project.manifest.shared_palettes = Some(self.path.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> PalettesDocument {
        PalettesDocument {
            path: PathBuf::from("palettes/shared.toml"),
            listed: false,
            top: Vec::new(),
            clean: (0..COLOURS).map(Color15).collect(),
            palettes: SharedPalettes::default(),
            saved: SharedPalettes::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    #[test]
    fn a_colour_set_back_leaves_the_file_and_undo_follows() {
        let mut doc = document();
        doc.set("Change", 13, Color15(0x7FFF)).unwrap();
        assert!(doc.is_changed(13));
        assert_eq!(doc.colour(13), Color15(0x7FFF));
        doc.amend(13, Color15(0x1234)).unwrap();
        assert_eq!(doc.colour(13), Color15(0x1234));
        doc.set("Back", 13, Color15(13)).unwrap();
        assert!(!doc.is_changed(13));
        assert!(!doc.is_modified());
        assert!(doc.undo());
        assert_eq!(doc.colour(13), Color15(0x1234));
        assert!(doc.set("Past", COLOURS, Color15(0)).is_err());
    }
}
