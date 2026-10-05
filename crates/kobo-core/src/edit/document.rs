//! One level file, open for editing.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Edit, EditError};
use crate::source::Comments;
use crate::source::level::Level;

/// The level and its comments at one point in the history.
#[derive(Clone, Debug)]
struct Snapshot {
    /// What the step that left this state was, for "Undo move object".
    label: String,
    level: Level,
    comments: Comments,
}

/// A level file, open: what it holds now, the text on disk, and the
/// edits that can be undone and redone.
#[derive(Clone, Debug)]
pub struct LevelDocument {
    path: PathBuf,
    level: Level,
    comments: Comments,
    /// `level.to_toml(comments)`, kept with them.
    text: String,
    /// The file's text as last read or written here.
    disk_text: String,
    /// What that text holds, if it parses: the document is modified when
    /// it differs.
    disk: Option<(Level, Comments)>,
    /// Each step's state before it, newest last, and its label.
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

/// What reading a level file again found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reload {
    /// The file is as last read or written.
    Unchanged,
    /// It changed and was read, as an undo step.
    Reloaded,
    /// It changed while the document has unsaved edits, and was left for
    /// the caller to choose: the file's text is here, to pass to
    /// [`LevelDocument::set_text`] if the file should win.
    Conflict(String),
}

impl LevelDocument {
    /// Opens the level file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, EditError> {
        let path = path.into();
        let text = read(&path)?;
        Self::from_text(path, text)
    }

    /// A document for `path` from its text, as if read from it.
    pub fn from_text(path: impl Into<PathBuf>, text: String) -> Result<Self, EditError> {
        let (level, comments) = Level::from_toml(&text)?;
        let formatted = level.to_toml(&comments);
        Ok(Self {
            path: path.into(),
            disk: Some((level.clone(), comments.clone())),
            level,
            comments,
            text: formatted,
            disk_text: text,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn level(&self) -> &Level {
        &self.level
    }

    pub fn comments(&self) -> &Comments {
        &self.comments
    }

    /// The file as saving would write it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the document holds something the file does not. A file
    /// written by hand in another layout is not modified until an edit
    /// changes what it holds.
    pub fn is_modified(&self) -> bool {
        self.disk
            .as_ref()
            .is_none_or(|(level, comments)| *level != self.level || *comments != self.comments)
    }

    /// Applies `edits` in order as one undo step called `label`. If one
    /// fails, none is applied.
    pub fn apply(&mut self, label: impl Into<String>, edits: &[Edit]) -> Result<(), EditError> {
        let mut level = self.level.clone();
        let mut comments = self.comments.clone();
        for edit in edits {
            edit.apply(&mut level, &mut comments)?;
        }
        self.replace(label.into(), level, comments);
        Ok(())
    }

    /// Replaces the document with `text`, as an outside edit or the source
    /// pane's: one undo step. On a parse error nothing changes.
    pub fn set_text(&mut self, label: impl Into<String>, text: &str) -> Result<(), EditError> {
        let (level, comments) = Level::from_toml(text)?;
        self.replace(label.into(), level, comments);
        Ok(())
    }

    fn replace(&mut self, label: String, level: Level, comments: Comments) {
        if level == self.level && comments == self.comments {
            return;
        }
        let before = Snapshot {
            label,
            level: std::mem::replace(&mut self.level, level),
            comments: std::mem::replace(&mut self.comments, comments),
        };
        self.undo.push(before);
        self.redo.clear();
        self.text = self.level.to_toml(&self.comments);
    }

    /// The label of the step [`LevelDocument::undo`] would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// The label of the step [`LevelDocument::redo`] would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Undoes the last step; false if there is none.
    pub fn undo(&mut self) -> bool {
        Self::step(
            &mut self.undo,
            &mut self.redo,
            &mut self.level,
            &mut self.comments,
        ) && self.refresh()
    }

    /// Redoes the last undone step; false if there is none.
    pub fn redo(&mut self) -> bool {
        Self::step(
            &mut self.redo,
            &mut self.undo,
            &mut self.level,
            &mut self.comments,
        ) && self.refresh()
    }

    fn step(
        from: &mut Vec<Snapshot>,
        to: &mut Vec<Snapshot>,
        level: &mut Level,
        comments: &mut Comments,
    ) -> bool {
        let Some(snapshot) = from.pop() else {
            return false;
        };
        to.push(Snapshot {
            label: snapshot.label,
            level: std::mem::replace(level, snapshot.level),
            comments: std::mem::replace(comments, snapshot.comments),
        });
        true
    }

    fn refresh(&mut self) -> bool {
        self.text = self.level.to_toml(&self.comments);
        true
    }

    /// Writes the file, if the document holds something it does not.
    pub fn save(&mut self) -> Result<(), EditError> {
        if !self.is_modified() {
            return Ok(());
        }
        fs::write(&self.path, &self.text).map_err(|source| EditError::Io {
            path: self.path.clone(),
            source,
        })?;
        self.disk_text = self.text.clone();
        self.disk = Some((self.level.clone(), self.comments.clone()));
        Ok(())
    }

    /// Reads the file again after it may have changed on disk. A change
    /// is taken as an undo step when the document has no unsaved edits,
    /// and left to the caller when it has.
    pub fn reload(&mut self) -> Result<Reload, EditError> {
        let text = read(&self.path)?;
        if text == self.disk_text {
            return Ok(Reload::Unchanged);
        }
        if self.is_modified() {
            return Ok(Reload::Conflict(text));
        }
        // A file that does not parse leaves the document as it was, and
        // the error for the caller; the next change is read again.
        self.take_disk_text(text)?;
        Ok(Reload::Reloaded)
    }

    /// Takes `text`, read from disk, as the file's: the document is
    /// replaced with it (one undo step) and is no longer modified.
    pub fn take_disk_text(&mut self, text: String) -> Result<(), EditError> {
        self.set_text("Change on disk", &text)?;
        self.disk = Some((self.level.clone(), self.comments.clone()));
        self.disk_text = text;
        Ok(())
    }

    /// Keeps the document's edits over the file's `text`: the file on disk
    /// is now `text`, and saving overwrites it.
    pub fn keep_over(&mut self, text: String) {
        self.disk = Level::from_toml(&text).ok();
        self.disk_text = text;
    }
}

fn read(path: &Path) -> Result<String, EditError> {
    fs::read_to_string(path).map_err(|source| EditError::Io {
        path: path.to_path_buf(),
        source,
    })
}
