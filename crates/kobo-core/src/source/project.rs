//! The project manifest, `kobo.toml`.
//!
//! ```toml
//! format = 1
//!
//! [rom]
//! size = "1M"
//!
//! [map16]
//! 0x02 = "map16/02.toml"
//!
//! [levels]
//! 0x105 = "levels/yoshis-island-1.toml"
//! ```
//!
//! The level table is the only place level numbers live; file names and
//! folders are free. A level the table does not list keeps the clean
//! ROM's content. The Map16 table does the same for pages 2 to `$7F`; a
//! page it does not list does not exist. Pages 0 and 1, the game's, list
//! only what a project changes, as do tileset files for the tiles the game
//! keeps per object tileset (`[map16_tileset]`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use toml_edit::DocumentMut;

use super::{Comments, SourceError, Writer, invalid, map16, read_comments};
use crate::level::LEVEL_COUNT;

/// The manifest format this version of Kobo writes. A newer one is
/// refused; older ones are migrated when there are any.
pub const FORMAT: u32 = 1;

pub const MANIFEST: &str = "kobo.toml";

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Manifest {
    /// The size to expand the ROM to, if the project sets one.
    pub rom_size: Option<usize>,
    /// Whether the ROM runs on the SA-1, through SA-1 Pack.
    pub sa1: bool,
    /// Whether GFX files are stored as LC_LZ3 rather than LC_LZ2 (SA-1
    /// builds only, whose decompressor SA-1 Pack supplies).
    pub lz3: bool,
    /// Asar patches applied before AddmusicK and the tools, in order.
    pub early_patches: Vec<PathBuf>,
    /// Asar patches applied after the tools, before the levels, in order.
    pub late_patches: Vec<PathBuf>,
    /// A folder of UberASM Tool's input files (`list.txt`, `level/`, ...),
    /// laid over the user's UberASM Tool folder.
    pub uberasm: Option<PathBuf>,
    /// A folder of GPS's input files (`list.txt`, `blocks/`, `routines/`),
    /// laid over the user's GPS folder.
    pub gps: Option<PathBuf>,
    /// A folder of PIXI's input files (`list.txt`, `sprites/`,
    /// `routines/`, ...), laid over PIXI's folder.
    pub pixi: Option<PathBuf>,
    /// A folder of AddmusicK's input files (`Addmusic_list.txt`, `music/`,
    /// `samples/`, ...), laid over the user's AddmusicK folder.
    pub music: Option<PathBuf>,
    /// GFX file (`00` to `33`) to indexed PNG, relative to the project
    /// directory.
    pub gfx: BTreeMap<u8, PathBuf>,
    /// GFX files stored as 4bpp, Lunar Magic's way (`[graphics] bpp = 4`):
    /// the files the game keeps as 3bpp take 16 colours. A project with
    /// ExGFX or graphics lists has them so whether it says so or not.
    pub four_bpp: bool,
    /// ExGFX file (`0x80` to `0xFFF`) to an indexed PNG, or a `.bin` file
    /// of its bytes as they are.
    pub exgfx: BTreeMap<u16, ExGfxFile>,
    /// The older graphics lists Lunar Magic's objects `24` and `25` name,
    /// by number: four files, first to fourth (FG1/SP1, FG2/SP2, BG1/SP3,
    /// FG3/SP4).
    pub bypass_lists: BTreeMap<u8, [u8; 4]>,
    /// Map16 page to file, relative to the project directory.
    pub map16: BTreeMap<u8, PathBuf>,
    /// BG Map16 page (table * 16 + page) to file.
    pub map16_bg: BTreeMap<u8, PathBuf>,
    /// Object tileset to the file of its own tiles (`source::map16`).
    pub map16_tileset: BTreeMap<u8, PathBuf>,
    /// The file of the vertical pipes' colours and the diagonal pipes
    /// (`[map16_pipes] file`, `source::map16::Pipes`).
    pub map16_pipes: Option<PathBuf>,
    /// Level number to file, relative to the project directory.
    pub levels: BTreeMap<u16, PathBuf>,
    /// Lunar Magic's global ExAnimation list's file (`source::animation`).
    pub animation_global: Option<PathBuf>,
    /// The uncompressed ExGFX files `0x60` to `0x63` ExAnimation takes
    /// sources from, as their bytes.
    pub animation_files: BTreeMap<u16, PathBuf>,
}

/// An ExGFX file of the project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExGfxFile {
    pub path: PathBuf,
    /// Bits a pixel of a PNG file takes in the ROM: 2, 3, 4, or 8.
    /// A `.bin` file has none.
    pub bpp: u8,
}

impl ExGfxFile {
    /// Whether the file is its bytes as they are, not an image.
    pub fn is_binary(&self) -> bool {
        self.path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
    }
}

/// The key a level is listed under.
fn level_key(level: u16) -> String {
    format!("0x{level:03X}")
}

impl Manifest {
    /// Writes the manifest in Kobo's format, with the user's comments
    /// before their places.
    pub fn to_toml(&self, comments: &Comments) -> String {
        let mut out = Writer::new(comments);
        out.line(format_args!("format = {FORMAT}"));
        if self.rom_size.is_some() || self.sa1 || self.lz3 {
            out.table("rom");
        }
        if let Some(size) = self.rom_size {
            out.key("rom", "size", toml_edit::Value::from(size_text(size)));
        } else {
            out.key_comments("rom", "size");
        }
        if self.sa1 {
            out.key("rom", "sa1", true);
        } else {
            out.key_comments("rom", "sa1");
        }
        if self.lz3 {
            out.key("rom", "lz3", true);
        } else {
            out.key_comments("rom", "lz3");
        }
        let paths = |list: &[PathBuf]| {
            let quoted: Vec<String> = list.iter().map(|p| quoted(p)).collect();
            format!("[{}]", quoted.join(", "))
        };
        if !self.early_patches.is_empty() || !self.late_patches.is_empty() {
            out.table("patches");
            if self.early_patches.is_empty() {
                out.key_comments("patches", "early");
            } else {
                out.key("patches", "early", paths(&self.early_patches));
            }
            if self.late_patches.is_empty() {
                out.key_comments("patches", "late");
            } else {
                out.key("patches", "late", paths(&self.late_patches));
            }
        }
        if let Some(music) = &self.music {
            out.table("music");
            out.key("music", "dir", quoted(music));
        }
        if let Some(uberasm) = &self.uberasm {
            out.table("uberasm");
            out.key("uberasm", "dir", quoted(uberasm));
        }
        if let Some(pixi) = &self.pixi {
            out.table("pixi");
            out.key("pixi", "dir", quoted(pixi));
        }
        if let Some(gps) = &self.gps {
            out.table("gps");
            out.key("gps", "dir", quoted(gps));
        }
        if self.four_bpp {
            out.table("graphics");
            out.key("graphics", "bpp", 4);
        }
        for (name, pages) in [
            ("gfx", &self.gfx),
            ("map16", &self.map16),
            ("map16_bg", &self.map16_bg),
            ("map16_tileset", &self.map16_tileset),
        ] {
            if !pages.is_empty() {
                out.table(name);
                for (page, path) in pages {
                    out.key(name, &format!("0x{page:02X}"), quoted(path));
                }
            }
        }
        if let Some(pipes) = &self.map16_pipes {
            out.table("map16_pipes");
            out.key("map16_pipes", "file", quoted(pipes));
        }
        if !self.exgfx.is_empty() {
            out.table("exgfx");
            for (file, entry) in &self.exgfx {
                let key = format!("0x{file:03X}");
                if entry.bpp == 4 || entry.is_binary() {
                    out.key("exgfx", &key, quoted(&entry.path));
                } else {
                    out.key(
                        "exgfx",
                        &key,
                        format!("{{ file = {}, bpp = {} }}", quoted(&entry.path), entry.bpp),
                    );
                }
            }
        }
        if !self.bypass_lists.is_empty() {
            out.table("bypass_lists");
            for (n, files) in &self.bypass_lists {
                let files: Vec<String> = files.iter().map(|f| format!("0x{f:02X}")).collect();
                out.key(
                    "bypass_lists",
                    &format!("0x{n:02X}"),
                    format!("[{}]", files.join(", ")),
                );
            }
        }
        if self.animation_global.is_some() || !self.animation_files.is_empty() {
            out.table("animation");
            match &self.animation_global {
                Some(path) => out.key("animation", "global", quoted(path)),
                None => out.key_comments("animation", "global"),
            }
            for (file, path) in &self.animation_files {
                out.key("animation", &format!("0x{file:02X}"), quoted(path));
            }
        }
        out.table("levels");
        for (level, path) in &self.levels {
            out.key("levels", &level_key(*level), quoted(path));
        }
        out.finish()
    }

    /// Reads a manifest, with the own-line comments to keep when it is
    /// written again.
    pub fn from_toml(text: &str) -> Result<(Self, Comments), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut comments = read_comments(&doc);
        for (key, _) in doc.iter() {
            if ![
                "format",
                "rom",
                "patches",
                "music",
                "uberasm",
                "pixi",
                "gps",
                "graphics",
                "gfx",
                "exgfx",
                "bypass_lists",
                "map16",
                "map16_bg",
                "map16_tileset",
                "map16_pipes",
                "animation",
                "levels",
            ]
            .contains(&key)
            {
                return Err(invalid(MANIFEST, format!("unknown key `{key}`")));
            }
        }
        let format = doc
            .get("format")
            .and_then(|f| f.as_integer())
            .ok_or_else(|| invalid("format", "must be an integer"))?;
        if format != FORMAT as i64 {
            return Err(invalid(
                "format",
                format!("this Kobo reads format {FORMAT}, not {format}"),
            ));
        }
        let mut manifest = Self::default();
        if let Some(rom) = doc.get("rom") {
            let rom = rom
                .as_table()
                .ok_or_else(|| invalid("rom", "must be a table"))?;
            for (key, item) in rom.iter() {
                match key {
                    "size" => {
                        let text = item.as_str().ok_or_else(|| {
                            invalid("rom.size", "must be a string such as \"1M\"")
                        })?;
                        manifest.rom_size = Some(parse_size(text).ok_or_else(|| {
                            invalid("rom.size", format!("{text:?} is not a size such as \"1M\""))
                        })?);
                    }
                    "sa1" => {
                        manifest.sa1 = item
                            .as_bool()
                            .ok_or_else(|| invalid("rom.sa1", "must be true or false"))?;
                    }
                    "lz3" => {
                        manifest.lz3 = item
                            .as_bool()
                            .ok_or_else(|| invalid("rom.lz3", "must be true or false"))?;
                    }
                    _ => return Err(invalid("rom", format!("unknown key `{key}`"))),
                }
            }
            // SA-1 Pack's patches past 4 MiB make exactly 6 or 8 MiB.
            if let Some(size) = manifest.rom_size.filter(|&s| manifest.sa1 && s > 0x40_0000)
                && ![0x60_0000, 0x80_0000].contains(&size)
            {
                return Err(invalid(
                    "rom.size",
                    format!(
                        "an SA-1 image past 4M is \"6M\" or \"8M\", not \"{}\"",
                        size_text(size)
                    ),
                ));
            }
        }
        if let Some(patches) = doc.get("patches") {
            let patches = patches
                .as_table()
                .ok_or_else(|| invalid("patches", "must be a table"))?;
            for (key, item) in patches.iter() {
                let at = format!("patches.{key}");
                let list = item
                    .as_array()
                    .ok_or_else(|| invalid(&at, "must be a list of patch files"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(PathBuf::from)
                            .ok_or_else(|| invalid(&at, "must be a list of patch files"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match key {
                    "early" => manifest.early_patches = list,
                    "late" => manifest.late_patches = list,
                    _ => return Err(invalid("patches", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(animation) = doc.get("animation") {
            let animation = animation
                .as_table()
                .ok_or_else(|| invalid("animation", "must be a table"))?;
            for (key, item) in animation.iter() {
                let at = format!("animation.{key}");
                let path = item
                    .as_str()
                    .map(PathBuf::from)
                    .ok_or_else(|| invalid(&at, "must be a file path"))?;
                if key == "global" {
                    manifest.animation_global = Some(path);
                    continue;
                }
                let file = key
                    .strip_prefix("0x")
                    .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                    .filter(|n| (0x60..=0x63).contains(n))
                    .ok_or_else(|| {
                        invalid(&at, "is `global` or an alternative file, 0x60 to 0x63")
                    })?;
                manifest.animation_files.insert(file, path);
            }
        }
        if let Some(music) = doc.get("music") {
            let music = music
                .as_table()
                .ok_or_else(|| invalid("music", "must be a table"))?;
            for (key, item) in music.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("music.dir", "must be a folder path"))?;
                        manifest.music = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("music", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(uberasm) = doc.get("uberasm") {
            let uberasm = uberasm
                .as_table()
                .ok_or_else(|| invalid("uberasm", "must be a table"))?;
            for (key, item) in uberasm.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("uberasm.dir", "must be a folder path"))?;
                        manifest.uberasm = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("uberasm", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(pixi) = doc.get("pixi") {
            let pixi = pixi
                .as_table()
                .ok_or_else(|| invalid("pixi", "must be a table"))?;
            for (key, item) in pixi.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("pixi.dir", "must be a folder path"))?;
                        manifest.pixi = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("pixi", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(gps) = doc.get("gps") {
            let gps = gps
                .as_table()
                .ok_or_else(|| invalid("gps", "must be a table"))?;
            for (key, item) in gps.iter() {
                match key {
                    "dir" => {
                        let dir = item
                            .as_str()
                            .ok_or_else(|| invalid("gps.dir", "must be a folder path"))?;
                        manifest.gps = Some(PathBuf::from(dir));
                    }
                    _ => return Err(invalid("gps", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(pipes) = doc.get("map16_pipes") {
            let pipes = pipes
                .as_table()
                .ok_or_else(|| invalid("map16_pipes", "must be a table"))?;
            for (key, item) in pipes.iter() {
                match key {
                    "file" => {
                        let file = item
                            .as_str()
                            .ok_or_else(|| invalid("map16_pipes.file", "must be a file path"))?;
                        manifest.map16_pipes = Some(PathBuf::from(file));
                    }
                    _ => return Err(invalid("map16_pipes", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(graphics) = doc.get("graphics") {
            let graphics = graphics
                .as_table()
                .ok_or_else(|| invalid("graphics", "must be a table"))?;
            for (key, item) in graphics.iter() {
                match key {
                    "bpp" => match item.as_integer() {
                        Some(3) => manifest.four_bpp = false,
                        Some(4) => manifest.four_bpp = true,
                        _ => return Err(invalid("graphics.bpp", "is 3 (the game's) or 4")),
                    },
                    _ => return Err(invalid("graphics", format!("unknown key `{key}`"))),
                }
            }
        }
        if let Some(files) = doc.get("exgfx") {
            let files = files
                .as_table()
                .ok_or_else(|| invalid("exgfx", "must be a table"))?;
            for (key, item) in files.iter() {
                let at = format!("exgfx.{key}");
                let file = key
                    .strip_prefix("0x")
                    .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                    .filter(|n| (0x80..=0xFFF).contains(n))
                    .ok_or_else(|| invalid(&at, "an ExGFX file is 0x080 to 0xFFF"))?;
                let bad = || invalid(&at, "must be a file path, or { file = path, bpp = n }");
                let entry = if let Some(path) = item.as_str() {
                    ExGfxFile {
                        path: PathBuf::from(path),
                        bpp: 4,
                    }
                } else {
                    let t = item.as_inline_table().ok_or_else(bad)?;
                    let mut path = None;
                    let mut bpp = 4;
                    for (k, v) in t.iter() {
                        match k {
                            "file" => path = Some(PathBuf::from(v.as_str().ok_or_else(bad)?)),
                            "bpp" => {
                                bpp = v
                                    .as_integer()
                                    .filter(|n| [2, 3, 4, 8].contains(n))
                                    .ok_or_else(|| invalid(&at, "bpp is 2, 3, 4, or 8"))?
                                    as u8
                            }
                            _ => return Err(invalid(&at, format!("unknown key `{k}`"))),
                        }
                    }
                    ExGfxFile {
                        path: path.ok_or_else(bad)?,
                        bpp,
                    }
                };
                if manifest.exgfx.insert(file, entry).is_some() {
                    return Err(invalid(&at, "is listed twice"));
                }
            }
        }
        if let Some(lists) = doc.get("bypass_lists") {
            let lists = lists
                .as_table()
                .ok_or_else(|| invalid("bypass_lists", "must be a table"))?;
            for (key, item) in lists.iter() {
                let at = format!("bypass_lists.{key}");
                let n = key
                    .strip_prefix("0x")
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .ok_or_else(|| invalid(&at, "a list is 0x00 to 0xFF"))?;
                let bad = || invalid(&at, "must be four GFX files, [0x00, 0x01, 0x02, 0x03]");
                let files: Vec<u8> = item
                    .as_array()
                    .ok_or_else(bad)?
                    .iter()
                    .map(|v| {
                        v.as_integer()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or_else(bad)
                    })
                    .collect::<Result<_, _>>()?;
                let files: [u8; 4] = files.try_into().map_err(|_| bad())?;
                if manifest.bypass_lists.insert(n, files).is_some() {
                    return Err(invalid(&at, "is listed twice"));
                }
            }
        }
        for (name, range) in [
            ("gfx", 0x00..=0x33),
            ("map16", map16::PAGES),
            ("map16_bg", 0x00..=0xFF),
            ("map16_tileset", 0x00..=crate::map16::TILESET_COUNT - 1),
        ] {
            let Some(pages) = doc.get(name) else { continue };
            let pages = pages
                .as_table()
                .ok_or_else(|| invalid(name, "must be a table"))?;
            for (key, item) in pages.iter() {
                let at = format!("{name}.{key}");
                let page = key
                    .strip_prefix("0x")
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .filter(|n| range.contains(n))
                    .ok_or_else(|| {
                        invalid(
                            &at,
                            format!(
                                "a page here is 0x{:02X} to 0x{:02X}",
                                range.start(),
                                range.end()
                            ),
                        )
                    })?;
                let path = item
                    .as_str()
                    .ok_or_else(|| invalid(&at, "must be a file path"))?;
                let list = match name {
                    "gfx" => &mut manifest.gfx,
                    "map16" => &mut manifest.map16,
                    "map16_bg" => &mut manifest.map16_bg,
                    _ => &mut manifest.map16_tileset,
                };
                if list.insert(page, PathBuf::from(path)).is_some() {
                    return Err(invalid(&at, "is listed twice"));
                }
            }
        }
        if let Some(levels) = doc.get("levels") {
            let levels = levels
                .as_table()
                .ok_or_else(|| invalid("levels", "must be a table"))?;
            for (key, item) in levels.iter() {
                let at = format!("levels.{key}");
                let level = key
                    .strip_prefix("0x")
                    .and_then(|hex| u16::from_str_radix(hex, 16).ok())
                    .filter(|&n| n < LEVEL_COUNT)
                    .ok_or_else(|| invalid(&at, "a level number is 0x000 to 0x1FF"))?;
                let path = item
                    .as_str()
                    .ok_or_else(|| invalid(&at, "must be a file path"))?;
                if manifest.levels.insert(level, PathBuf::from(path)).is_some() {
                    return Err(invalid(&at, "is listed twice"));
                }
                // Written back under the key's canonical spelling.
                let canonical = format!("levels.{}", level_key(level));
                if at != canonical {
                    comments.rename(&at, &canonical);
                }
            }
        }
        Ok((manifest, comments))
    }
}

/// A path as the manifest writes it: a TOML string, with forward slashes
/// on every platform.
fn quoted(path: &std::path::Path) -> String {
    toml_edit::Value::from(path.to_string_lossy().replace('\\', "/")).to_string()
}

/// `512K`, `1M`, `3M`, or a byte count.
pub fn parse_size(text: &str) -> Option<usize> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, 'K' | 'k') => (&text[..i], 1 << 10),
        (i, 'M' | 'm') => (&text[..i], 1 << 20),
        _ => (text, 1),
    };
    digits.parse::<usize>().ok()?.checked_mul(unit)
}

fn size_text(size: usize) -> String {
    match size {
        s if s % (1 << 20) == 0 => format!("{}M", s >> 20),
        s if s % (1 << 10) == 0 => format!("{}K", s >> 10),
        s => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let manifest = Manifest {
            rom_size: Some(0x18_0000),
            sa1: true,
            lz3: true,
            early_patches: vec![PathBuf::from("asm/fastrom.asm")],
            late_patches: vec![PathBuf::from("asm/a.asm"), PathBuf::from("asm/b.asm")],
            music: Some(PathBuf::from("music")),
            uberasm: Some(PathBuf::from("uberasm")),
            gps: Some(PathBuf::from("blocks")),
            pixi: Some(PathBuf::from("sprites")),
            gfx: BTreeMap::from([(0x00, PathBuf::from("graphics/GFX00.png"))]),
            four_bpp: true,
            animation_global: Some(PathBuf::from("animation/global.toml")),
            animation_files: BTreeMap::from([(0x61, PathBuf::from("animation/ExGFX61.bin"))]),
            exgfx: BTreeMap::from([
                (
                    0x80,
                    ExGfxFile {
                        path: PathBuf::from("graphics/ExGFX80.png"),
                        bpp: 4,
                    },
                ),
                (
                    0x100,
                    ExGfxFile {
                        path: PathBuf::from("graphics/ExGFX100.png"),
                        bpp: 2,
                    },
                ),
                (
                    0xFFF,
                    ExGfxFile {
                        path: PathBuf::from("graphics/ExGFXFFF.bin"),
                        bpp: 4,
                    },
                ),
            ]),
            bypass_lists: BTreeMap::from([(0x01, [0x10, 0x11, 0x12, 0x7F])]),
            map16: BTreeMap::from([(0x10, PathBuf::from("map16/10.toml"))]),
            map16_bg: BTreeMap::from([(0x01, PathBuf::from("map16/bg-01.toml"))]),
            map16_tileset: BTreeMap::from([(0x05, PathBuf::from("map16/tileset-05.toml"))]),
            map16_pipes: Some(PathBuf::from("map16/pipes.toml")),
            levels: BTreeMap::from([
                (0x105, PathBuf::from("world1/yoshis-island-1.toml")),
                (0x0C7, PathBuf::from("title.toml")),
            ]),
        };
        let text = manifest.to_toml(&Comments::default());
        assert_eq!(
            text,
            "format = 1\n\n[rom]\nsize = \"1536K\"\nsa1 = true\nlz3 = true\n\n[patches]\n\
             early = [\"asm/fastrom.asm\"]\nlate = [\"asm/a.asm\", \"asm/b.asm\"]\n\n\
             [music]\ndir = \"music\"\n\n[uberasm]\ndir = \"uberasm\"\n\n[pixi]\ndir = \"sprites\"\n\n[gps]\ndir = \"blocks\"\n\n\
             [graphics]\nbpp = 4\n\n\
             [gfx]\n0x00 = \"graphics/GFX00.png\"\n\n[map16]\n0x10 = \"map16/10.toml\"\n\n[map16_bg]\n0x01 = \"map16/bg-01.toml\"\n\n\
             [map16_tileset]\n0x05 = \"map16/tileset-05.toml\"\n\n\
             [map16_pipes]\nfile = \"map16/pipes.toml\"\n\n\
             [exgfx]\n0x080 = \"graphics/ExGFX80.png\"\n\
             0x100 = { file = \"graphics/ExGFX100.png\", bpp = 2 }\n\
             0xFFF = \"graphics/ExGFXFFF.bin\"\n\n\
             [bypass_lists]\n0x01 = [0x10, 0x11, 0x12, 0x7F]\n\n\
             [animation]\nglobal = \"animation/global.toml\"\n\
             0x61 = \"animation/ExGFX61.bin\"\n\n\
             [levels]\n\
             0x0C7 = \"title.toml\"\n0x105 = \"world1/yoshis-island-1.toml\"\n"
        );
        let (again, comments) = Manifest::from_toml(&text).unwrap();
        assert_eq!(again, manifest);
        assert!(comments.is_empty());
    }

    #[test]
    fn sa1_sizes_past_4m_are_sa1_packs() {
        let read = |rom: &str| Manifest::from_toml(&format!("format = 1\n[rom]\n{rom}\n"));
        assert!(read("size = \"6M\"\nsa1 = true").is_ok());
        assert!(read("sa1 = true\nsize = \"8M\"").is_ok());
        assert!(read("size = \"4M\"\nsa1 = true").is_ok());
        assert!(read("size = \"5M\"").is_ok());
        let error = read("size = \"5M\"\nsa1 = true").unwrap_err().to_string();
        assert!(error.contains("\"6M\" or \"8M\""), "{error}");
        assert!(read("size = \"4608K\"\nsa1 = true").is_err());
    }

    #[test]
    fn paths_are_written_as_toml_strings() {
        // Quotes and backslashes are valid in file names on Linux and macOS.
        let manifest = Manifest {
            early_patches: vec![PathBuf::from("asm/a\"b.asm")],
            late_patches: vec![PathBuf::from("asm/c'd\"e.asm")],
            music: Some(PathBuf::from("music/a\"b")),
            levels: BTreeMap::from([(0x105, PathBuf::from("x\"y.toml"))]),
            ..Manifest::default()
        };
        let text = manifest.to_toml(&Comments::default());
        assert_eq!(Manifest::from_toml(&text).unwrap().0, manifest, "{text}");
    }

    #[test]
    fn comments_survive_formatting() {
        let text = "\
# My hack.
format = 1
# The ROM.
[rom]
# Room for music.
size = \"2M\"
# The levels.
[levels]
# The title screen.
0x0C7 = \"title.toml\"
# World 1, under a longer key.
0x0105 = \"w1.toml\"
# That is all.
";
        let (manifest, comments) = Manifest::from_toml(text).unwrap();
        let formatted = manifest.to_toml(&comments);
        assert_eq!(
            formatted,
            "\
# My hack.

format = 1

# The ROM.
[rom]
# Room for music.
size = \"2M\"

# The levels.
[levels]
# The title screen.
0x0C7 = \"title.toml\"
# World 1, under a longer key.
0x105 = \"w1.toml\"

# That is all.
"
        );
        let (again, comments) = Manifest::from_toml(&formatted).unwrap();
        assert_eq!(again.to_toml(&comments), formatted);
    }

    #[test]
    fn refusals() {
        let bad = |text: &str| Manifest::from_toml(text).is_err();
        assert!(bad("format = 2\n"));
        assert!(bad("[levels]\n"));
        assert!(bad("format = 1\n[levels]\n0x200 = \"a.toml\"\n"));
        assert!(bad("format = 1\n[levels]\n105 = \"a.toml\"\n"));
        assert!(bad("format = 1\nextra = 1\n"));
        assert!(bad("format = 1\n[rom]\nsize = \"big\"\n"));
        assert!(!bad("format = 1\n[map16]\n0x01 = \"a.toml\"\n"));
        assert!(!bad("format = 1\n[map16_tileset]\n0x0E = \"a.toml\"\n"));
        assert!(bad("format = 1\n[map16_tileset]\n0x0F = \"a.toml\"\n"));
        assert!(bad("format = 1\n[map16]\n0x80 = \"a.toml\"\n"));
        assert!(!bad("format = 1\n"));
    }
}
