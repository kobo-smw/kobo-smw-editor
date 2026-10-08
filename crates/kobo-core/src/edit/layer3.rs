//! A level's layer 3 tilemap, open for editing: the ExGFX file its
//! graphics list's LT3 slot names when the list loads layer 3's tilemap
//! (`T`, docs/lunar-magic-install.md, "Per-level graphics lists"), as the
//! tilemap words it holds. The project keeps it as a `.bin` file, its
//! bytes as they are.
//!
//! The file's words go to VRAM from `$5000` (`$5800` for the bottom half),
//! layer 3's tilemap, in 32x32 screens; LT3's settings leave the first
//! words out when the file goes under the status bar ([`Tilemap::skip`]).
//! A level given a tilemap gets the first free ExGFX number, a `$1000`-byte
//! file of blank tiles (`$38FC`, as the game's own layer 3 is), placed
//! under the status bar.

use std::fs;
use std::path::{Path, PathBuf};

use crate::build::Project;
use crate::exgfx::{self, GraphicsList, slot};
use crate::map16::Tile8Ref;
use crate::source::level::Level;
use crate::source::project::{ExGfxFile, MANIFEST, Manifest};

/// The word a blank layer 3 tile is: tile `FC`, palette 6, in front.
pub const BLANK: u16 = 0x38FC;

/// Where a level's layer 3 tilemap comes from and goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tilemap {
    /// The ExGFX file.
    pub file: u16,
    /// LT3's `DDFF`: where it goes (`DD`) and how much loads (`FF`).
    pub settings: u8,
}

impl Tilemap {
    /// The level's, if its graphics list loads one.
    pub fn of(level: &Level) -> Option<Self> {
        let list = level.graphics.filter(GraphicsList::layer3_tilemap)?;
        Some(Self {
            file: list.file(slot::LT3),
            settings: list.tilemap_settings(),
        })
    }

    /// Bytes it loads: `$2000`, `$1000`, or `$800`.
    pub fn bytes(&self) -> usize {
        let size = usize::from(self.settings & 3);
        usize::from(exgfx::TILEMAP_SIZES[if size == 3 { 0 } else { size }])
    }

    /// The VRAM word address of the file's first word: `$5000`, or
    /// `$5800` for the bottom half.
    pub fn base(&self) -> u16 {
        if self.settings >> 2 == 3 {
            0x5800
        } else {
            0x5000
        }
    }

    /// Words at the file's start that do not load: those under the
    /// status bar, or its last row.
    pub fn skip(&self) -> usize {
        [0xA0, 0, 0x80, 0][usize::from(self.settings >> 2)]
    }

    /// Where file word `index` shows in layer 3's 64x64 tilemap, in 8x8
    /// tiles: screens of 32x32, the first two side by side above the next
    /// two.
    pub fn place(&self, index: usize) -> (usize, usize) {
        let at = usize::from(self.base() - 0x5000) + index;
        let screen = at / 0x400;
        let (row, column) = (at % 0x400 / 32, at % 32);
        (screen % 2 * 32 + column, screen / 2 * 32 + row)
    }

    /// The file word shown at (`x`, `y`) of layer 3's tilemap, if the file
    /// reaches there and it loads.
    pub fn index_at(&self, x: usize, y: usize, words: usize) -> Option<usize> {
        if x >= 64 || y >= 64 {
            return None;
        }
        let screen = y / 32 * 2 + x / 32;
        let at = screen * 0x400 + y % 32 * 32 + x % 32;
        let index = at.checked_sub(usize::from(self.base() - 0x5000))?;
        (index >= self.skip() && index < words.min(self.bytes() / 2)).then_some(index)
    }
}

/// The level's graphics list with a layer 3 tilemap from ExGFX `file`
/// under the status bar, `$1000` bytes: its own list, or the default one,
/// with `T` set; the rest is as it was.
pub fn with_tilemap(level: &Level, file: u16) -> GraphicsList {
    let mut list = level.graphics.unwrap_or(GraphicsList::DEFAULT);
    list.0[slot::AN2] |= exgfx::LAYER3_TILEMAP;
    // DD 0 (under the status bar), FF 1 ($1000 bytes).
    list.0[slot::LT3] = 0x1000 | (file & 0x0FFF);
    list
}

/// The first ExGFX number the project has no file for.
pub fn free_file(project: &Project) -> Option<u16> {
    (exgfx::EXGFX_FIRST..=exgfx::EXGFX_LAST).find(|n| !project.manifest.exgfx.contains_key(n))
}

#[derive(Debug, thiserror::Error)]
pub enum Layer3Error {
    #[error("ExGFX{0:X} is an image; a layer 3 tilemap is a .bin file of words")]
    Image(u16),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Manifest {
        path: PathBuf,
        source: crate::source::SourceError,
    },
}

#[derive(Clone, Debug)]
struct Step {
    label: String,
    words: Vec<u16>,
}

/// A layer 3 tilemap file, open.
#[derive(Clone, Debug)]
pub struct TilemapDocument {
    file: u16,
    path: PathBuf,
    words: Vec<u16>,
    /// The words as the project's file has them: `None` for a file the
    /// project does not have yet.
    saved: Option<Vec<u16>>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl TilemapDocument {
    /// Opens ExGFX `file` as the project has it, or as a new file of blank
    /// tiles, `bytes` long.
    pub fn open(project: &Project, file: u16, bytes: usize) -> Result<Self, Layer3Error> {
        match project.manifest.exgfx.get(&file) {
            Some(entry) if !entry.is_binary() => Err(Layer3Error::Image(file)),
            Some(entry) => {
                let path = project.root.join(&entry.path);
                let data = fs::read(&path).map_err(|source| Layer3Error::Io {
                    path: path.clone(),
                    source,
                })?;
                let words: Vec<u16> = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|w| u16::from_le_bytes(*w))
                    .collect();
                Ok(Self {
                    file,
                    path: entry.path.clone(),
                    saved: Some(words.clone()),
                    words,
                    undo: Vec::new(),
                    redo: Vec::new(),
                })
            }
            None => Ok(Self {
                file,
                path: PathBuf::from("graphics").join(format!("ExGFX{file:X}.bin")),
                words: vec![BLANK; bytes / 2],
                saved: None,
                undo: Vec::new(),
                redo: Vec::new(),
            }),
        }
    }

    pub fn file(&self) -> u16 {
        self.file
    }

    pub fn words(&self) -> &[u16] {
        &self.words
    }

    pub fn word(&self, index: usize) -> Option<Tile8Ref> {
        self.words.get(index).copied().map(Tile8Ref)
    }

    fn written(&self, cells: &[(usize, Tile8Ref)]) -> Vec<u16> {
        let mut words = self.words.clone();
        for &(index, r) in cells {
            if let Some(word) = words.get_mut(index) {
                *word = r.0;
            }
        }
        words
    }

    /// Sets the words at `cells`, as one undo step.
    pub fn set(&mut self, label: impl Into<String>, cells: &[(usize, Tile8Ref)]) {
        let words = self.written(cells);
        if words != self.words {
            self.undo.push(Step {
                label: label.into(),
                words: std::mem::replace(&mut self.words, words),
            });
            self.redo.clear();
        }
    }

    /// Sets the words at `cells` as part of the last step: a stroke.
    pub fn amend(&mut self, cells: &[(usize, Tile8Ref)]) {
        if self.undo.is_empty() {
            return self.set("Draw on layer 3", cells);
        }
        self.words = self.written(cells);
        self.redo.clear();
    }

    /// Whether there is anything to save: a change to the project's file,
    /// or a new file.
    pub fn is_modified(&self) -> bool {
        self.saved.as_ref() != Some(&self.words)
    }

    pub fn in_project(&self) -> bool {
        self.saved.is_some()
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
        let words = std::mem::replace(&mut self.words, step.words);
        self.redo.push(Step {
            label: step.label,
            words,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let words = std::mem::replace(&mut self.words, step.words);
        self.undo.push(Step {
            label: step.label,
            words,
        });
        true
    }

    fn bytes(&self) -> Vec<u8> {
        self.words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// Writes the file into the project's folder `root`, and into the
    /// manifest when it is new. Returns the files written.
    pub fn save(&mut self, root: &Path) -> Result<Vec<PathBuf>, Layer3Error> {
        let path = root.join(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| Layer3Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, self.bytes()).map_err(|source| Layer3Error::Io {
            path: path.clone(),
            source,
        })?;
        let mut written = vec![path];
        if self.saved.is_none() {
            let manifest_path = root.join(MANIFEST);
            let text = fs::read_to_string(&manifest_path).map_err(|source| Layer3Error::Io {
                path: manifest_path.clone(),
                source,
            })?;
            let (mut manifest, comments) =
                Manifest::from_toml(&text).map_err(|source| Layer3Error::Manifest {
                    path: manifest_path.clone(),
                    source,
                })?;
            manifest.exgfx.insert(self.file, self.entry());
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| {
                Layer3Error::Io {
                    path: manifest_path.clone(),
                    source,
                }
            })?;
            written.push(manifest_path);
        }
        self.saved = Some(self.words.clone());
        Ok(written)
    }

    fn entry(&self) -> ExGfxFile {
        ExGfxFile {
            path: self.path.clone(),
            bpp: 4,
        }
    }

    /// Puts the words into `project`, so that a build has the unsaved
    /// edits.
    pub fn apply_to(&self, project: &mut Project) {
        let bytes = self.bytes();
        match project.exgfx.iter_mut().find(|(n, _)| *n == self.file) {
            Some((_, data)) => *data = bytes,
            None => {
                project.exgfx.push((self.file, bytes));
                project.exgfx.sort_by_key(|(n, _)| *n);
            }
        }
        project.manifest.exgfx.insert(self.file, self.entry());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilemap_places_its_words_as_its_settings_say() {
        // Under the status bar: the first $A0 words do not load.
        let under = Tilemap {
            file: 0x80,
            settings: 0b0001,
        };
        assert_eq!(
            (under.bytes(), under.base(), under.skip()),
            (0x1000, 0x5000, 0xA0)
        );
        assert_eq!(under.place(0xA0), (0, 5));
        assert_eq!(
            under.place(0x400),
            (32, 0),
            "the second screen beside the first"
        );
        assert_eq!(under.index_at(0, 5, 0x800), Some(0xA0));
        assert_eq!(under.index_at(0, 4, 0x800), None, "under the status bar");
        assert_eq!(under.index_at(0, 32, 0x800), None, "past $1000 bytes");
        // The bottom half: from $5800, all of it.
        let bottom = Tilemap {
            file: 0x80,
            settings: 0b1110,
        };
        assert_eq!(
            (bottom.bytes(), bottom.base(), bottom.skip()),
            (0x800, 0x5800, 0)
        );
        assert_eq!(bottom.place(0), (0, 32));
        assert_eq!(bottom.index_at(0, 32, 0x400), Some(0));
    }

    #[test]
    fn a_level_given_a_tilemap_keeps_the_rest_of_its_list() {
        let (mut level, _) = Level::from_toml(crate::edit::tests::LEVEL).unwrap();
        let list = with_tilemap(&level, 0x123);
        assert!(list.layer3_tilemap());
        assert!(!list.bypass(), "the tilesets' files still load");
        level.graphics = Some(list);
        assert_eq!(
            Tilemap::of(&level),
            Some(Tilemap {
                file: 0x123,
                settings: 1
            })
        );
    }
}
