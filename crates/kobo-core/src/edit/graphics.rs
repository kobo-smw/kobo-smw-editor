//! A graphics file, open for editing: one of the game's GFX files (`00` to
//! `33`) or one of the project's ExGFX files, as the indexed image the
//! project keeps it as (`gfx::tiles_to_image`: 16 tiles to a row, a pixel's
//! value its colour index).
//!
//! A GFX file the project does not have starts as the clean ROM's, as the
//! build would store it (16 colours where Lunar Magic's graphics formats
//! store a 3bpp file as 4bpp, with the fourth plane of the tiles the game
//! draws in colours 8 to 15 set), and saving adds it to the project as
//! `graphics/GFXnn.png`, as import names it. An ExGFX file is edited only
//! when the project has it as a PNG; a `.bin` file is its bytes, with no
//! colours to draw in. Undo keeps whole snapshots of the image, which is at
//! most a few tens of KB.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::build::Project;
use crate::gfx::{self, Bpp, GfxFormat};
use crate::image::IndexedImage;
use crate::rom::Rom;
use crate::source::project::{ExGfxFile, MANIFEST, Manifest};

/// A graphics file by its number.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum GraphicsFile {
    /// One of the game's, `00` to `33`.
    Gfx(u8),
    /// An ExGFX file, `80` to `FFF`.
    ExGfx(u16),
}

impl GraphicsFile {
    /// The file a graphics list's slot names: `00`-`33` the game's,
    /// `80`-`FFF` ExGFX, anything else none.
    pub fn from_list(file: u16) -> Option<Self> {
        match file {
            0..=0x33 => Some(Self::Gfx(file as u8)),
            crate::exgfx::EXGFX_FIRST..=crate::exgfx::EXGFX_LAST => Some(Self::ExGfx(file)),
            _ => None,
        }
    }
}

impl std::fmt::Display for GraphicsFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gfx(n) => write!(f, "GFX{n:02X}"),
            Self::ExGfx(n) => write!(f, "ExGFX{n:X}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum GraphicsError {
    #[error("there is no GFX{0:02X}; the game's files are 00 to 33")]
    NoGfx(u8),
    #[error("the project has no ExGFX{0:X}")]
    NoExGfx(u16),
    #[error("ExGFX{0:X} is a .bin file, its bytes as they are; import it as a PNG to draw in it")]
    Binary(u16),
    #[error("{file} {message}")]
    Image { file: GraphicsFile, message: String },
    #[error("colour {value} is past the {colors} {file} can hold")]
    Colour {
        file: GraphicsFile,
        value: u8,
        colors: usize,
    },
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

/// A step: its label, and the image before it.
#[derive(Clone, Debug)]
struct Step {
    label: String,
    image: IndexedImage,
}

/// A graphics file, open: the image now and as saved, and the steps to
/// undo and redo.
#[derive(Clone, Debug)]
pub struct GraphicsDocument {
    file: GraphicsFile,
    /// The file in the project's folder: where it is, or where saving
    /// puts it.
    path: PathBuf,
    /// Tiles in the file, which its image keeps.
    tiles: usize,
    colors: usize,
    image: IndexedImage,
    /// The image as the project's file has it: `None` for a game's file
    /// the project does not have yet.
    saved: Option<IndexedImage>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl GraphicsDocument {
    /// Opens `file` as `project` has it, or as the clean ROM does.
    pub fn open(project: &Project, clean: &Rom, file: GraphicsFile) -> Result<Self, GraphicsError> {
        let image_error = |message: String| GraphicsError::Image { file, message };
        match file {
            GraphicsFile::Gfx(index) => {
                if index >= gfx::GFX_FILE_COUNT {
                    return Err(GraphicsError::NoGfx(index));
                }
                let reader = gfx::GfxReader::new(clean).map_err(|e| image_error(e.to_string()))?;
                let vanilla = reader.read(index).map_err(|e| image_error(e.to_string()))?;
                let converts =
                    project.lunar_magic_graphics() && crate::exgfx::converts(vanilla.format);
                let colors = if converts {
                    16
                } else {
                    vanilla.format.colors()
                };
                let tiles = vanilla.tile_count();
                let path = project
                    .manifest
                    .gfx
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| gfx_path(index));
                let listed = project.gfx.iter().find(|(n, _)| *n == index);
                let (image, saved) = match listed {
                    Some((_, image)) => (image.clone(), Some(image.clone())),
                    None => {
                        let decoded = if converts {
                            gfx::decode_tiles(Bpp::Four, &crate::exgfx::stored_4bpp(&vanilla))
                        } else {
                            vanilla.tiles()
                        };
                        (gfx::tiles_to_image(&decoded, colors), None)
                    }
                };
                Ok(Self::new(file, path, tiles, colors, image, saved))
            }
            GraphicsFile::ExGfx(number) => {
                let entry = project
                    .manifest
                    .exgfx
                    .get(&number)
                    .ok_or(GraphicsError::NoExGfx(number))?;
                if entry.is_binary() {
                    return Err(GraphicsError::Binary(number));
                }
                let path = project.root.join(&entry.path);
                let bytes = fs::read(&path).map_err(|source| GraphicsError::Io {
                    path: path.clone(),
                    source,
                })?;
                let image =
                    IndexedImage::from_png(&bytes).map_err(|e| image_error(e.to_string()))?;
                let colors = bpp_of(entry).colors();
                let tiles = image.height as usize / 8 * 16;
                Ok(Self::new(
                    file,
                    entry.path.clone(),
                    tiles,
                    colors,
                    image.clone(),
                    Some(image),
                ))
            }
        }
    }

    fn new(
        file: GraphicsFile,
        path: PathBuf,
        tiles: usize,
        colors: usize,
        image: IndexedImage,
        saved: Option<IndexedImage>,
    ) -> Self {
        Self {
            file,
            path,
            tiles,
            colors,
            image,
            saved,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn file(&self) -> GraphicsFile {
        self.file
    }

    pub fn image(&self) -> &IndexedImage {
        &self.image
    }

    /// Colours a pixel can be: 4, 8, or 16.
    pub fn colors(&self) -> usize {
        self.colors
    }

    pub fn tiles(&self) -> usize {
        self.tiles
    }

    /// The pixel's colour index, or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.image.width && y < self.image.height)
            .then(|| self.image.pixels[(y * self.image.width + x) as usize])
    }

    fn check(&self, value: u8) -> Result<(), GraphicsError> {
        if usize::from(value) < self.colors {
            Ok(())
        } else {
            Err(GraphicsError::Colour {
                file: self.file,
                value,
                colors: self.colors,
            })
        }
    }

    fn painted(&self, pixels: &[(u32, u32)], value: u8) -> IndexedImage {
        let mut image = self.image.clone();
        for &(x, y) in pixels {
            if x < image.width && y < image.height {
                image.pixels[(y * image.width + x) as usize] = value;
            }
        }
        image
    }

    fn step(&mut self, label: String, image: IndexedImage) {
        if image != self.image {
            self.undo.push(Step {
                label,
                image: std::mem::replace(&mut self.image, image),
            });
            self.redo.clear();
        }
    }

    /// Sets `pixels` to colour `value`, as one undo step.
    pub fn paint(
        &mut self,
        label: impl Into<String>,
        pixels: &[(u32, u32)],
        value: u8,
    ) -> Result<(), GraphicsError> {
        self.check(value)?;
        let image = self.painted(pixels, value);
        self.step(label.into(), image);
        Ok(())
    }

    /// Sets `pixels` to colour `value` as part of the last step: a stroke
    /// of the pencil is one step however far it goes.
    pub fn amend_paint(&mut self, pixels: &[(u32, u32)], value: u8) -> Result<(), GraphicsError> {
        self.check(value)?;
        if self.undo.is_empty() {
            return self.paint("Draw", pixels, value);
        }
        self.image = self.painted(pixels, value);
        self.redo.clear();
        Ok(())
    }

    /// Fills the area of one colour around (`x`, `y`) with `value`,
    /// within its 8x8 tile, as one undo step.
    pub fn fill(
        &mut self,
        label: impl Into<String>,
        x: u32,
        y: u32,
        value: u8,
    ) -> Result<(), GraphicsError> {
        self.check(value)?;
        let Some(from) = self.pixel(x, y) else {
            return Ok(());
        };
        let (left, top) = (x / 8 * 8, y / 8 * 8);
        let mut seen = [[false; 8]; 8];
        let mut todo = vec![(x, y)];
        let mut area = Vec::new();
        while let Some((px, py)) = todo.pop() {
            let (tx, ty) = ((px - left) as usize, (py - top) as usize);
            if seen[ty][tx] || self.pixel(px, py) != Some(from) {
                continue;
            }
            seen[ty][tx] = true;
            area.push((px, py));
            if px > left {
                todo.push((px - 1, py));
            }
            if px < left + 7 {
                todo.push((px + 1, py));
            }
            if py > top {
                todo.push((px, py - 1));
            }
            if py < top + 7 {
                todo.push((px, py + 1));
            }
        }
        let image = self.painted(&area, value);
        self.step(label.into(), image);
        Ok(())
    }

    /// Replaces the whole image with `image` (an indexed PNG drawn
    /// elsewhere, say), as one undo step: it must be the file's size, 16
    /// tiles to a row, and keep to its colours.
    pub fn replace(
        &mut self,
        label: impl Into<String>,
        image: IndexedImage,
    ) -> Result<(), GraphicsError> {
        gfx::image_to_tiles(&image, self.tiles, self.colors).map_err(|message| {
            GraphicsError::Image {
                file: self.file,
                message,
            }
        })?;
        let image = IndexedImage {
            palette: self.image.palette.clone(),
            ..image
        };
        self.step(label.into(), image);
        Ok(())
    }

    pub fn is_modified(&self) -> bool {
        self.saved.as_ref() != Some(&self.image) && !(self.saved.is_none() && self.undo.is_empty())
    }

    /// Whether the project has the file, or saving will add it.
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
        let image = std::mem::replace(&mut self.image, step.image);
        self.redo.push(Step {
            label: step.label,
            image,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let image = std::mem::replace(&mut self.image, step.image);
        self.undo.push(Step {
            label: step.label,
            image,
        });
        true
    }

    /// Writes the image into the project's folder `root`, and the file
    /// into the manifest when it is new to it. Returns the files written.
    pub fn save(&mut self, root: &Path) -> Result<Vec<PathBuf>, GraphicsError> {
        let path = root.join(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| GraphicsError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let png = self.image.to_png().map_err(|e| GraphicsError::Image {
            file: self.file,
            message: e.to_string(),
        })?;
        fs::write(&path, png).map_err(|source| GraphicsError::Io {
            path: path.clone(),
            source,
        })?;
        let mut written = vec![path];
        if let (None, GraphicsFile::Gfx(index)) = (&self.saved, self.file) {
            let manifest_path = root.join(MANIFEST);
            let text = fs::read_to_string(&manifest_path).map_err(|source| GraphicsError::Io {
                path: manifest_path.clone(),
                source,
            })?;
            let (mut manifest, comments) =
                Manifest::from_toml(&text).map_err(|source| GraphicsError::Manifest {
                    path: manifest_path.clone(),
                    source,
                })?;
            manifest.gfx.insert(index, self.path.clone());
            fs::write(&manifest_path, manifest.to_toml(&comments)).map_err(|source| {
                GraphicsError::Io {
                    path: manifest_path.clone(),
                    source,
                }
            })?;
            written.push(manifest_path);
        }
        self.saved = Some(self.image.clone());
        Ok(written)
    }

    /// Puts the image into `project`, in place of what it read from the
    /// file, so that a build has the unsaved edits. A game's file the
    /// document has not changed stays out.
    pub fn apply_to(&self, project: &mut Project) -> Result<(), GraphicsError> {
        if self.saved.is_none() && self.undo.is_empty() {
            return Ok(());
        }
        match self.file {
            GraphicsFile::Gfx(index) => {
                match project.gfx.iter_mut().find(|(n, _)| *n == index) {
                    Some((_, image)) => *image = self.image.clone(),
                    None => {
                        project.gfx.push((index, self.image.clone()));
                        project.gfx.sort_by_key(|(n, _)| *n);
                    }
                }
                project.manifest.gfx.insert(index, self.path.clone());
            }
            GraphicsFile::ExGfx(number) => {
                let bpp = project
                    .manifest
                    .exgfx
                    .get(&number)
                    .map_or(Bpp::Four, bpp_of);
                let tiles = gfx::image_to_tiles(&self.image, self.tiles, bpp.colors()).map_err(
                    |message| GraphicsError::Image {
                        file: self.file,
                        message,
                    },
                )?;
                let data = GfxFormat::Planar(bpp).encode(&tiles);
                match project.exgfx.iter_mut().find(|(n, _)| *n == number) {
                    Some((_, bytes)) => *bytes = data,
                    None => project.exgfx.push((number, data)),
                }
            }
        }
        Ok(())
    }
}

/// The files a level loads, each with its slot's name: as its graphics
/// list names them when the list is on (Lunar Magic's "Super GFX
/// Bypass"), else its tilesets' files and the game's layer 3 files. A
/// slot that loads no file is left out.
pub fn level_files(
    level: &crate::source::level::Level,
    clean: &Rom,
) -> Vec<(&'static str, GraphicsFile)> {
    use crate::exgfx::{GraphicsList, slot};
    let mut out = Vec::new();
    let list = level.graphics.filter(GraphicsList::bypass);
    match list {
        Some(list) => {
            let mut slots = vec![
                slot::FG1,
                slot::FG2,
                slot::BG1,
                slot::FG3,
                slot::BG2,
                slot::BG3,
                slot::SP1,
                slot::SP2,
                slot::SP3,
                slot::SP4,
            ];
            if list.layer3_files() {
                slots.extend([slot::LG1, slot::LG2, slot::LG3, slot::LG4]);
            }
            slots.push(slot::AN2);
            for s in slots {
                if let Some(file) = GraphicsFile::from_list(list.file(s)) {
                    out.push((GraphicsList::SLOTS[s], file));
                }
            }
        }
        None => {
            let header = &level.header;
            let layers = gfx::object_tileset_files(clean, header.object_tileset).ok();
            for (name, file) in ["FG1", "FG2", "BG1", "FG3"]
                .into_iter()
                .zip(layers.into_iter().flatten())
            {
                out.push((name, GraphicsFile::Gfx(file)));
            }
            let sprites = gfx::sprite_tileset_files(clean, header.sprite_tileset).ok();
            for (name, file) in ["SP1", "SP2", "SP3", "SP4"]
                .into_iter()
                .zip(sprites.into_iter().flatten())
            {
                out.push((name, GraphicsFile::Gfx(file)));
            }
            for (name, file) in [("LG1", 0x28), ("LG2", 0x29), ("LG3", 0x2A), ("LG4", 0x2B)] {
                out.push((name, GraphicsFile::Gfx(file)));
            }
        }
    }
    out
}

fn gfx_path(index: u8) -> PathBuf {
    PathBuf::from("graphics").join(format!("GFX{index:02X}.png"))
}

fn bpp_of(file: &ExGfxFile) -> Bpp {
    match file.bpp {
        2 => Bpp::Two,
        3 => Bpp::Three,
        _ => Bpp::Four,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(colors: usize) -> GraphicsDocument {
        let image = IndexedImage {
            width: 128,
            height: 16,
            pixels: vec![0; 128 * 16],
            palette: crate::image::grayscale(colors),
        };
        GraphicsDocument::new(
            GraphicsFile::Gfx(0x14),
            gfx_path(0x14),
            32,
            colors,
            image,
            None,
        )
    }

    #[test]
    fn a_stroke_is_one_step_and_undo_takes_it_back() {
        let mut doc = document(8);
        assert!(!doc.is_modified(), "a game's file opened is not a change");
        doc.paint("Draw", &[(1, 1)], 3).unwrap();
        doc.amend_paint(&[(2, 1), (3, 1)], 3).unwrap();
        assert_eq!(doc.pixel(3, 1), Some(3));
        assert!(doc.is_modified());
        assert!(doc.undo());
        assert_eq!(doc.pixel(1, 1), Some(0));
        assert_eq!(doc.pixel(3, 1), Some(0));
        assert!(doc.redo());
        assert_eq!(doc.pixel(2, 1), Some(3));
        assert!(
            doc.paint("Draw", &[(0, 0)], 8).is_err(),
            "3bpp has 8 colours"
        );
    }

    #[test]
    fn an_image_replaces_the_file_whole_if_it_fits() {
        let mut doc = document(8);
        let mut image = doc.image().clone();
        image.pixels[0] = 7;
        doc.replace("Import", image.clone()).unwrap();
        assert_eq!(doc.pixel(0, 0), Some(7));
        image.pixels[0] = 8;
        assert!(doc.replace("Import", image).is_err(), "past 8 colours");
        let small = IndexedImage {
            height: 8,
            pixels: vec![0; 128 * 8],
            ..doc.image().clone()
        };
        assert!(doc.replace("Import", small).is_err(), "too few tiles");
    }

    #[test]
    fn a_fill_keeps_to_its_tile_and_colour() {
        let mut doc = document(16);
        // A wall down column 4 of the first tile.
        let wall: Vec<(u32, u32)> = (0..8).map(|y| (4, y)).collect();
        doc.paint("Draw", &wall, 1).unwrap();
        doc.fill("Fill", 0, 0, 5).unwrap();
        assert_eq!(doc.pixel(3, 7), Some(5));
        assert_eq!(doc.pixel(4, 0), Some(1), "the wall stays");
        assert_eq!(doc.pixel(5, 0), Some(0), "past the wall stays");
        assert_eq!(doc.pixel(8, 0), Some(0), "the next tile stays");
        assert_eq!(doc.undo_label(), Some("Fill"));
    }
}
