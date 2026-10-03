//! A project's source files: the manifest, the level files, and the Map16
//! page files, in TOML.
//!
//! Kobo owns the formatting. Every file is written the same way from the
//! same content, one object or sprite per line in data order, numbers the
//! game uses in hex, positions in decimal, and bytes Kobo does not
//! interpret as a hex string. A user's comments on lines of their own
//! survive reformatting, wherever they stand ([`Comments`]); a comment
//! after an entry or a value on its line is Kobo's, and is rewritten.

use std::collections::BTreeMap;
use std::fmt::Display;

use thiserror::Error;
use toml_edit::{Decor, DocumentMut, Item};

pub mod animation;
pub mod level;
pub mod map16;
pub mod pixi;
pub mod project;

/// A user's own-line comments, by where they stood, so that a file
/// written again has them in the same places. A place is [`Comments::TOP`]
/// for the lines before the first item, [`Comments::END`] for those after
/// the last, a table's name (`header`) for those before its `[header]`
/// line, `header.screens` for those before a key, `layer1.objects[3]` for
/// those before the fourth entry of an array, and `layer1.objects[]` for
/// those before its closing bracket.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Comments {
    places: BTreeMap<String, Vec<String>>,
}

impl Comments {
    /// Before the first item of the file.
    pub const TOP: &'static str = "";
    /// After the last item of the file.
    pub const END: &'static str = "<end>";

    /// The comment lines before `place`, each starting with `#`.
    pub fn before(&self, place: &str) -> &[String] {
        self.places.get(place).map_or(&[], Vec::as_slice)
    }

    /// Keeps `lines` for `place`, after any already there.
    pub fn add(&mut self, place: impl Into<String>, lines: impl IntoIterator<Item = String>) {
        let lines: Vec<String> = lines.into_iter().collect();
        if !lines.is_empty() {
            self.places.entry(place.into()).or_default().extend(lines);
        }
    }

    /// Moves the comments before `from` to before `to`.
    pub fn rename(&mut self, from: &str, to: &str) {
        if let Some(lines) = self.places.remove(from) {
            self.add(to, lines);
        }
    }

    /// Every place with comments, in order.
    pub fn places(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.places.iter().map(|(p, l)| (p.as_str(), l.as_slice()))
    }

    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }
}

fn prefix(decor: &Decor) -> &str {
    decor.prefix().and_then(|p| p.as_str()).unwrap_or("")
}

/// The own-line comments of a document: before its first item, before
/// each table's header line and each key of a table, and after its last
/// item. An array's entries are read by the array's reader.
pub(crate) fn read_comments(doc: &DocumentMut) -> Comments {
    let mut comments = Comments::default();
    let root = doc.as_table();
    for (i, (name, item)) in root.iter().enumerate() {
        let place = if i == 0 { Comments::TOP } else { name };
        match item {
            Item::Table(table) => {
                comments.add(place, own_line_comments(prefix(table.decor()), false));
                for (key, _) in table.iter() {
                    if let Some(k) = table.key(key) {
                        let lines = own_line_comments(prefix(k.leaf_decor()), false);
                        comments.add(format!("{name}.{key}"), lines);
                    }
                }
            }
            _ => {
                if let Some(k) = root.key(name) {
                    comments.add(place, own_line_comments(prefix(k.leaf_decor()), false));
                }
            }
        }
    }
    let trailing = doc.trailing().as_str().unwrap_or("");
    comments.add(Comments::END, own_line_comments(trailing, false));
    comments
}

/// A file in Kobo's format, with the user's comments put back before
/// their places as it is written.
pub(crate) struct Writer<'a> {
    text: String,
    comments: &'a Comments,
}

impl<'a> Writer<'a> {
    /// Starts a file with the comments at its top, and a blank line after
    /// them when there are any.
    pub fn new(comments: &'a Comments) -> Self {
        let mut writer = Self {
            text: String::new(),
            comments,
        };
        writer.lines(Comments::TOP);
        if !writer.text.is_empty() {
            writer.text.push('\n');
        }
        writer
    }

    fn lines(&mut self, place: &str) {
        for line in self.comments.before(place) {
            self.text += line;
            self.text.push('\n');
        }
    }

    /// A line of the file as it is, with no comments of its own.
    pub fn line(&mut self, line: impl Display) {
        self.text += &format!("{line}\n");
    }

    /// A `[name]` line, after a blank line unless the file has none yet.
    pub fn table(&mut self, name: &str) {
        if !(self.text.is_empty() || self.text.ends_with("\n\n")) {
            self.text.push('\n');
        }
        self.lines(name);
        self.text += &format!("[{name}]\n");
    }

    /// A `key = value` line of table `table`.
    pub fn key(&mut self, table: &str, key: &str, value: impl Display) {
        self.lines(&format!("{table}.{key}"));
        self.text += &format!("{key} = {value}\n");
    }

    /// The comments before a key the file leaves out, such as a flag that
    /// is not set, so they are not lost with it.
    pub fn key_comments(&mut self, table: &str, key: &str) {
        self.lines(&format!("{table}.{key}"));
    }

    /// A `key = [` ... `]` array of table `table`, one entry per line
    /// with Kobo's note after it, and the user's comments before the
    /// entries they preceded.
    pub fn list<T>(
        &mut self,
        table: &str,
        key: &str,
        items: &[T],
        line: impl Fn(&T) -> (String, Option<String>),
    ) {
        let place = format!("{table}.{key}");
        self.lines(&place);
        self.text += &format!("{key} = [\n");
        let comments = self.comments;
        let entry_comments = |text: &mut String, place: &str| {
            for comment in comments.before(place) {
                *text += &format!("    {comment}\n");
            }
        };
        for (i, item) in items.iter().enumerate() {
            entry_comments(&mut self.text, &format!("{place}[{i}]"));
            let (text, note) = line(item);
            self.text += &format!("    {text},");
            if let Some(note) = note {
                self.text += &format!("  # {note}");
            }
            self.text.push('\n');
        }
        entry_comments(&mut self.text, &format!("{place}[]"));
        self.text += "]\n";
    }

    /// The file, with the comments after its last item.
    pub fn finish(mut self) -> String {
        if !self.comments.before(Comments::END).is_empty() {
            self.text.push('\n');
            self.lines(Comments::END);
        }
        self.text
    }
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error(transparent)]
    Toml(#[from] toml_edit::TomlError),
    #[error("{at}: {message}")]
    Invalid { at: String, message: String },
}

pub(crate) fn invalid(at: impl Into<String>, message: impl Into<String>) -> SourceError {
    SourceError::Invalid {
        at: at.into(),
        message: message.into(),
    }
}

/// `0x05`, `0x105`: a number the game uses, in at least `digits` digits.
pub(crate) fn hex(value: u32, digits: usize) -> String {
    format!("0x{value:0digits$X}")
}

/// Bytes as `"11 5A"`.
pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("\"{}\"", parts.join(" "))
}

pub(crate) fn parse_hex_bytes(at: &str, text: &str) -> Result<Vec<u8>, SourceError> {
    text.split_whitespace()
        .map(|part| {
            u8::from_str_radix(part, 16)
                .ok()
                .filter(|_| part.len() == 2)
                .ok_or_else(|| invalid(at, format!("{part:?} is not a hex byte")))
        })
        .collect()
}

/// The comment lines in a decor prefix that stand on lines of their own.
/// The text before the first line break follows the previous entry on its
/// line, so it is Kobo's and is dropped.
pub(crate) fn own_line_comments(prefix: &str, after_entry: bool) -> Vec<String> {
    let lines: Vec<&str> = prefix.split('\n').collect();
    let skip = usize::from(after_entry && lines.len() > 1);
    lines[skip..]
        .iter()
        .map(|line| line.trim())
        .filter(|line| line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// A project file as it is, and as Kobo writes it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Formatted {
    pub path: std::path::PathBuf,
    pub text: String,
    pub formatted: String,
}

impl Formatted {
    /// Whether Kobo would write the file differently.
    pub fn changed(&self) -> bool {
        self.text != self.formatted
    }
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("reading {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Source {
        path: std::path::PathBuf,
        #[source]
        source: SourceError,
    },
}

/// Every text file of the project in `dir` that Kobo owns the format of,
/// with the user's own-line comments kept: the manifest and the files it
/// lists (`kobo fmt`).
pub fn format_project(dir: &std::path::Path) -> Result<Vec<Formatted>, FormatError> {
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(&path)
            .map(|text| (path.clone(), text))
            .map_err(|source| FormatError::Io { path, source })
    };
    let source_error = |path: &std::path::Path| {
        let path = path.to_path_buf();
        move |source| FormatError::Source { path, source }
    };
    let (path, text) = read(dir.join(project::MANIFEST))?;
    let (manifest, comments) = project::Manifest::from_toml(&text).map_err(source_error(&path))?;
    let formatted = manifest.to_toml(&comments);
    let mut files = vec![Formatted {
        path,
        text,
        formatted,
    }];
    for file in manifest.levels.values() {
        let (path, text) = read(dir.join(file))?;
        let (level, comments) = level::Level::from_toml(&text).map_err(source_error(&path))?;
        let formatted = level.to_toml(&comments);
        files.push(Formatted {
            path,
            text,
            formatted,
        });
    }
    if let Some(file) = &manifest.animation_global {
        let (path, text) = read(dir.join(file))?;
        let (list, comments) = animation::global_from_toml(&text).map_err(source_error(&path))?;
        let formatted = animation::global_to_toml(&list, &comments);
        files.push(Formatted {
            path,
            text,
            formatted,
        });
    }
    let pages = [
        (map16::PageKind::Foreground, &manifest.map16),
        (map16::PageKind::Background, &manifest.map16_bg),
        (map16::PageKind::Tileset, &manifest.map16_tileset),
    ];
    for (kind, pages) in pages {
        for (&page, file) in pages {
            let (path, text) = read(dir.join(file))?;
            let formatted =
                if kind == map16::PageKind::Foreground && map16::GAME_PAGES.contains(&page) {
                    let (tiles, comments) =
                        map16::GamePage::from_toml(page, &text).map_err(source_error(&path))?;
                    tiles.to_toml(&comments)
                } else {
                    let (tiles, comments) = map16::Map16Page::from_toml(kind, page, &text)
                        .map_err(source_error(&path))?;
                    tiles.to_toml(kind, &comments)
                };
            files.push(Formatted {
                path,
                text,
                formatted,
            });
        }
    }
    if let Some(file) = &manifest.pixi_compiled {
        let (path, text) = read(dir.join(file))?;
        let (compiled, comments) = pixi::Compiled::from_toml(&text).map_err(source_error(&path))?;
        let formatted = compiled.to_toml(&comments);
        files.push(Formatted {
            path,
            text,
            formatted,
        });
    }
    if let Some(file) = &manifest.map16_pipes {
        let (path, text) = read(dir.join(file))?;
        let (pipes, top) = map16::Pipes::from_toml(&text).map_err(source_error(&path))?;
        let formatted = pipes.to_toml(&top);
        files.push(Formatted {
            path,
            text,
            formatted,
        });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::map16::{Map16Entry, Map16Page, PageComments};
    use super::*;

    #[test]
    fn format_project_covers_map16_pages() {
        let dir = std::env::temp_dir().join(format!("kobo-fmt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("map16")).unwrap();
        std::fs::write(
            dir.join(project::MANIFEST),
            "format = 1\n\n[map16]\n0x02 = \"map16/02.toml\"\n\n[levels]\n",
        )
        .unwrap();
        let mut page = Map16Page::default();
        let entry = Map16Entry {
            acts: 0x130,
            ..Map16Entry::default()
        };
        page.tiles.insert(0x205, entry);
        let text = page.to_toml(map16::PageKind::Foreground, &PageComments::default());
        std::fs::write(dir.join("map16/02.toml"), &text).unwrap();
        let files = format_project(&dir).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| !f.changed()), "{files:?}");

        // Loosely spaced, with a comment: Kobo's spacing, the comment kept.
        let loose = format!("# mine\n{}", text.replace(" = ", "="));
        std::fs::write(dir.join("map16/02.toml"), &loose).unwrap();
        let files = format_project(&dir).unwrap();
        let page_file = &files[1];
        assert!(page_file.changed());
        assert!(
            page_file.formatted.starts_with("# mine\n"),
            "{}",
            page_file.formatted
        );
        assert_eq!(page_file.formatted.replacen("# mine\n\n", "", 1), text);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
