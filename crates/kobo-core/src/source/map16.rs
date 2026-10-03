//! Map16 page files: one page of 256 foreground tiles, in Lunar Magic's
//! numbering, with what each tile acts like.
//!
//! ```toml
//! [tiles]
//! 0x200 = { acts = 0x130, gfx = ["0A0 2", "0A1 2", "0B0 2", "0B1 2 p"] }
//! ```
//!
//! One tile per line, keyed by its full number. `gfx` is its four 8x8
//! tiles in reading order (top left, top right, bottom left, bottom right,
//! which is not the game's storage order), each as the 8x8 tile number in
//! hex, the palette row, and any of `x` and `y` (flips) and `p` (priority).
//! `acts` is the tile it acts like. A tile the file of a page past 1 does
//! not list is empty: four references to 8x8 tile 0 in palette 0, acting
//! like `$130`, as a fresh Lunar Magic install's acts-like table has every
//! tile past page 1. The manifest's `[map16]` table says which page a file
//! is.
//!
//! Pages 0 and 1 are the game's own tables, which Lunar Magic rewrites in
//! place ([`GamePage`]): their files list only the tiles a project changes,
//! each with `acts`, `gfx`, or both, and a tile or a setting a file leaves
//! out keeps the clean ROM's, which acts like itself. Some of their tiles
//! have a definition per object tileset (`map16::GameTables`); their
//! graphics are in the tileset files instead.
//!
//! A tileset file, in the manifest's `[map16_tileset]` table, holds an
//! object tileset's own tiles: those of pages 0 and 1 that the game keeps
//! per tileset, left out meaning the clean ROM's, and, when page 2 is per
//! tileset (Lunar Magic's "tileset specific" page 2), the tileset's page 2,
//! left out meaning empty. They have no `acts`, which is the same in every
//! tileset. Tilesets that share the game's table of their own tiles share
//! one file's tiles of pages 0 and 1, which the manifest notes after it
//! ([`crate::map16::TILESET_SHARING`]); page 2 is per tileset exactly when
//! a tileset file lists a tile of it.
//!
//! The pipes file, in the manifest's `[map16_pipes]`, holds the game's
//! other definitions of some page 1 tiles: the vertical pipes' colours and
//! the diagonal pipes ([`Pipes`]).
//!
//! Background pages, in the manifest's `[map16_bg]` table, are the same
//! without `acts`: page `P` is page `P % 16` of BG Map16 table `P / 16`,
//! and a tile's key is its number in the table plus `$1000` times the
//! table, so that the file names the table's pages as the table's number.
//! Pages 0 and 1 of table 0 are the game's own, so as with the foreground's,
//! a tile their files leave out keeps the clean ROM's.
//!
//! Throughout, a tile a file leaves out is what the build starts from: the
//! clean ROM's on the game's own pages, an empty one past them.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use toml_edit::{DocumentMut, Item, Table, Value};

use super::{SourceError, hex, invalid, own_line_comments};
use crate::map16::{Map16Tile, Tile8Ref};

/// Tiles in a page.
pub const PAGE_TILES: u16 = 0x100;
/// The pages a page file can be: 0 to `$7F`. Pages 0 and 1 are the game's
/// own tables ([`GamePage`]).
pub const PAGES: RangeInclusive<u8> = 0x00..=0x7F;
/// The pages that are the game's own tables.
pub const GAME_PAGES: RangeInclusive<u8> = 0x00..=0x01;
/// The tiles a tileset file can list: pages 0 to 2.
pub const TILESET_TILES: RangeInclusive<u16> = 0x000..=0x2FF;
/// What a tile of a page past 1 acts like when its file does not say.
pub const DEFAULT_ACTS: u16 = 0x130;

/// Foreground pages 2 to `$7F`, whose tiles act like others; background
/// pages, `$00` to `$FF`, whose tiles do not; or an object tileset's own
/// tiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageKind {
    Foreground,
    Background,
    Tileset,
}

/// One tile: its graphics, and the tile it acts like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Map16Entry {
    pub gfx: Map16Tile,
    pub acts: u16,
}

impl Default for Map16Entry {
    fn default() -> Self {
        Self {
            gfx: Map16Tile::default(),
            acts: DEFAULT_ACTS,
        }
    }
}

/// A page file's tiles, by full tile number.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Map16Page {
    pub tiles: BTreeMap<u16, Map16Entry>,
}

/// The own-line comments of a page file, kept when Kobo writes it again:
/// those at the top and those before each tile.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct PageComments {
    pub top: Vec<String>,
    pub tiles: BTreeMap<u16, Vec<String>>,
}

/// A tile of page 0 or 1 as a project changes it: what it acts like, its
/// graphics, or both; `None` keeps the clean ROM's.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct GameTile {
    pub acts: Option<u16>,
    pub gfx: Option<Map16Tile>,
}

/// The file of page 0 or 1: the tiles a project changes.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct GamePage {
    pub tiles: BTreeMap<u16, GameTile>,
}

impl GamePage {
    /// What tile `number` acts like: the file's setting, or itself.
    pub fn acts(&self, number: u16) -> u16 {
        self.tiles
            .get(&number)
            .and_then(|t| t.acts)
            .unwrap_or(number)
    }

    /// Writes the page in Kobo's format.
    pub fn to_toml(&self, comments: &PageComments) -> String {
        write_tiles(
            self.tiles.iter().map(|(&n, t)| (n, t.acts, t.gfx)),
            comments,
        )
    }

    /// Reads the file of page `page` (0 or 1).
    pub fn from_toml(page: u8, text: &str) -> Result<(Self, PageComments), SourceError> {
        let (raw, comments) = read_tiles(text, &page_tiles(page), true, &page_what(page))?;
        let mut tiles = BTreeMap::new();
        for (number, (acts, gfx)) in raw {
            if acts.is_none() && gfx.is_none() {
                return Err(invalid(
                    format!("tiles.{}", hex(number as u32, 3)),
                    "a tile of page 0 or 1 sets acts, gfx, or both",
                ));
            }
            tiles.insert(number, GameTile { acts, gfx });
        }
        Ok((Self { tiles }, comments))
    }
}

/// The game's other definitions of some page 1 tiles, as a project changes
/// them: the vertical pipes' colour sets 0, 2, and 3 (`$133`-`$13A`, by the
/// screen they are on; set 1 is page 1's own) and the diagonal pipes, which
/// replace `$1C4`-`$1C7` and `$1EC`-`$1EF` in object tilesets 0 and 7
/// (`map16::pipe_address`). A tile left out keeps the clean ROM's.
///
/// ```toml
/// [colours.0]
/// 0x133 = { gfx = ["0A0 4", "0A1 4", "0B0 4", "0B1 4"] }
///
/// [diagonal]
/// 0x1C4 = { gfx = ["0F8 2", "1AC 2", "1A8 2", "1AD 2"] }
/// ```
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Pipes {
    /// By set (0, 2, or 3) and tile.
    pub colours: BTreeMap<(u8, u16), Map16Tile>,
    /// By tile.
    pub diagonal: BTreeMap<u16, Map16Tile>,
}

/// The colour sets a pipes file can list.
pub const PIPE_SETS: [u8; 3] = [0, 2, 3];

impl Pipes {
    pub fn is_empty(&self) -> bool {
        self.colours.is_empty() && self.diagonal.is_empty()
    }

    /// Writes the file in Kobo's format, with `top`, the comments before
    /// its first table.
    pub fn to_toml(&self, top: &[String]) -> String {
        let mut out = String::new();
        for line in top {
            out += &format!("{line}\n");
        }
        if !top.is_empty() {
            out.push('\n');
        }
        let mut tables: Vec<(String, Vec<(u16, Map16Tile)>)> = Vec::new();
        for set in PIPE_SETS {
            let tiles: Vec<_> = self
                .colours
                .range((set, 0)..=(set, u16::MAX))
                .map(|(&(_, t), &g)| (t, g))
                .collect();
            if !tiles.is_empty() {
                tables.push((format!("colours.{set}"), tiles));
            }
        }
        if !self.diagonal.is_empty() {
            tables.push((
                "diagonal".into(),
                self.diagonal.iter().map(|(&t, &g)| (t, g)).collect(),
            ));
        }
        for (i, (name, tiles)) in tables.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out += &format!("[{name}]\n");
            for (tile, gfx) in tiles {
                out += &format!(
                    "{} = {{ gfx = {} }}\n",
                    hex(*tile as u32, 3),
                    gfx_text(*gfx)
                );
            }
        }
        out
    }

    /// Reads a pipes file, with the comments before its first table.
    pub fn from_toml(text: &str) -> Result<(Self, Vec<String>), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut out = Self::default();
        let mut top = None;
        let mut take_top = |table: &Table| {
            if top.is_none() {
                top = Some(own_line_comments(
                    table
                        .decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .unwrap_or(""),
                    false,
                ));
            }
        };
        for (key, item) in doc.iter() {
            match key {
                "colours" => {
                    let sets = item
                        .as_table()
                        .ok_or_else(|| invalid("colours", "must be a table of sets"))?;
                    for (set_key, set_item) in sets.iter() {
                        let at = format!("colours.{set_key}");
                        let set = set_key
                            .parse::<u8>()
                            .ok()
                            .filter(|s| PIPE_SETS.contains(s))
                            .ok_or_else(|| invalid(&at, "a set here is 0, 2, or 3"))?;
                        let table = set_item
                            .as_table()
                            .ok_or_else(|| invalid(&at, "must be a table"))?;
                        take_top(table);
                        for (tile, gfx) in read_pipe_tiles(table, &at, |t| {
                            crate::map16::PIPE_COLOUR_TILES.contains(&t)
                        })? {
                            out.colours.insert((set, tile), gfx);
                        }
                    }
                }
                "diagonal" => {
                    let table = item
                        .as_table()
                        .ok_or_else(|| invalid("diagonal", "must be a table"))?;
                    take_top(table);
                    out.diagonal = read_pipe_tiles(table, "diagonal", |t| {
                        crate::map16::diagonal_pipe_tiles().any(|d| d == t)
                    })?;
                }
                _ => return Err(invalid("file", format!("unknown key `{key}`"))),
            }
        }
        Ok((out, top.unwrap_or_default()))
    }
}

/// A pipes file table's tiles, each `{ gfx = [...] }`, keyed by a tile
/// `allowed` takes.
fn read_pipe_tiles(
    table: &Table,
    at: &str,
    allowed: impl Fn(u16) -> bool,
) -> Result<BTreeMap<u16, Map16Tile>, SourceError> {
    let mut tiles = BTreeMap::new();
    for (key, item) in table.iter() {
        let at = format!("{at}.{key}");
        let tile = key
            .strip_prefix("0x")
            .and_then(|h| u16::from_str_radix(h, 16).ok())
            .filter(|&t| allowed(t))
            .ok_or_else(|| {
                invalid(
                    &at,
                    "a tile here is 0x133 to 0x13A for the colours, 0x1C4 to 0x1C7 or 0x1EC to \
                     0x1EF for the diagonal pipes",
                )
            })?;
        let (_, gfx) = read_entry(item, &at, false)?;
        let gfx = gfx.ok_or_else(|| invalid(&at, "sets gfx"))?;
        if tiles.insert(tile, gfx).is_some() {
            return Err(invalid(&at, "is listed twice"));
        }
    }
    Ok(tiles)
}

/// A tile's four 8x8 tiles in reading order, as a file writes them.
fn gfx_text(t: Map16Tile) -> String {
    let gfx: Vec<String> = [t.top_left, t.top_right, t.bottom_left, t.bottom_right]
        .iter()
        .map(|r| format!("\"{}\"", tile8_text(*r)))
        .collect();
    format!("[{}]", gfx.join(", "))
}

/// The tiles of page `page`, inclusive, as page `$FF`'s last is `$FFFF`.
fn page_tiles(page: u8) -> RangeInclusive<u16> {
    let first = page as u16 * PAGE_TILES;
    first..=first + (PAGE_TILES - 1)
}

fn page_what(page: u8) -> String {
    format!("a tile of page {}", hex(page as u32, 2))
}

impl Map16Page {
    /// The tile, or the empty one if the file does not list it.
    pub fn tile(&self, number: u16) -> Map16Entry {
        self.tiles.get(&number).copied().unwrap_or_default()
    }

    /// Writes the page in Kobo's format.
    pub fn to_toml(&self, kind: PageKind, comments: &PageComments) -> String {
        write_tiles(
            self.tiles.iter().map(|(&n, e)| {
                let acts = (kind == PageKind::Foreground).then_some(e.acts);
                (n, acts, Some(e.gfx))
            }),
            comments,
        )
    }

    /// Reads the file of page `page`, or for [`PageKind::Tileset`] that of
    /// object tileset `page`.
    pub fn from_toml(
        kind: PageKind,
        page: u8,
        text: &str,
    ) -> Result<(Self, PageComments), SourceError> {
        let (range, what) = match kind {
            PageKind::Tileset => (
                TILESET_TILES,
                format!("a tile of tileset {}", hex(page as u32, 1)),
            ),
            _ => (page_tiles(page), page_what(page)),
        };
        let (raw, comments) = read_tiles(text, &range, kind == PageKind::Foreground, &what)?;
        let tiles = raw
            .into_iter()
            .map(|(number, (acts, gfx))| {
                let entry = Map16Entry {
                    gfx: gfx.unwrap_or_default(),
                    acts: acts.unwrap_or(DEFAULT_ACTS),
                };
                (number, entry)
            })
            .collect();
        Ok((Self { tiles }, comments))
    }
}

/// A file's `[tiles]`, each with what it acts like and its graphics where
/// given.
fn write_tiles(
    tiles: impl Iterator<Item = (u16, Option<u16>, Option<Map16Tile>)>,
    comments: &PageComments,
) -> String {
    let mut out = String::new();
    for line in &comments.top {
        out += &format!("{line}\n");
    }
    if !comments.top.is_empty() {
        out.push('\n');
    }
    out += "[tiles]\n";
    for (number, acts, gfx) in tiles {
        for line in comments.tiles.get(&number).into_iter().flatten() {
            out += &format!("{line}\n");
        }
        let mut fields = Vec::new();
        if let Some(acts) = acts {
            fields.push(format!("acts = {}", hex(acts as u32, 3)));
        }
        if let Some(t) = gfx {
            fields.push(format!("gfx = {}", gfx_text(t)));
        }
        out += &format!("{} = {{ {} }}\n", hex(number as u32, 3), fields.join(", "));
    }
    out
}

/// What a tile's entry gives: what it acts like, and its graphics.
type RawTile = (Option<u16>, Option<Map16Tile>);

/// A file's `[tiles]`, keyed by numbers in `range`; `what` says what a
/// number there is, for when one is not.
fn read_tiles(
    text: &str,
    range: &RangeInclusive<u16>,
    acts: bool,
    what: &str,
) -> Result<(BTreeMap<u16, RawTile>, PageComments), SourceError> {
    let doc: DocumentMut = text.parse()?;
    let mut comments = PageComments::default();
    for (key, _) in doc.iter() {
        if key != "tiles" {
            return Err(invalid("file", format!("unknown key `{key}`")));
        }
    }
    let Some(item) = doc.get("tiles") else {
        return Ok((BTreeMap::new(), comments));
    };
    let table: &Table = item
        .as_table()
        .ok_or_else(|| invalid("tiles", "must be a table"))?;
    comments.top = own_line_comments(
        table
            .decor()
            .prefix()
            .and_then(|p| p.as_str())
            .unwrap_or(""),
        false,
    );
    let mut tiles = BTreeMap::new();
    for (key, item) in table.iter() {
        let at = format!("tiles.{key}");
        let number = key
            .strip_prefix("0x")
            .and_then(|h| u16::from_str_radix(h, 16).ok())
            .filter(|n| range.contains(n))
            .ok_or_else(|| {
                invalid(
                    &at,
                    format!(
                        "{what} is {} to {}",
                        hex(*range.start() as u32, 3),
                        hex(*range.end() as u32, 3)
                    ),
                )
            })?;
        if let Some(prefix) = table
            .key(key)
            .and_then(|k| k.leaf_decor().prefix())
            .and_then(|p| p.as_str())
        {
            // A key's prefix starts on its own line: the comment after
            // the entry before it is that entry's suffix.
            let own = own_line_comments(prefix, false);
            if !own.is_empty() {
                comments.tiles.insert(number, own);
            }
        }
        let entry = read_entry(item, &at, acts)?;
        if tiles.insert(number, entry).is_some() {
            return Err(invalid(&at, "is listed twice"));
        }
    }
    Ok((tiles, comments))
}

fn read_entry(item: &Item, at: &str, acts_allowed: bool) -> Result<RawTile, SourceError> {
    let t = item
        .as_inline_table()
        .ok_or_else(|| invalid(at, "must be an inline table { acts, gfx }"))?;
    for (key, _) in t.iter() {
        if !(key == "gfx" || key == "acts" && acts_allowed) {
            return Err(invalid(at, format!("unknown key `{key}`")));
        }
    }
    let acts = match t.get("acts") {
        None => None,
        Some(v) => Some(
            v.as_integer()
                .filter(|n| (0..=0x7FFF).contains(n))
                .ok_or_else(|| invalid(format!("{at}.acts"), "a tile number is 0x000 to 0x7FFF"))?
                as u16,
        ),
    };
    let gfx = match t.get("gfx") {
        None => None,
        Some(v) => {
            let at = format!("{at}.gfx");
            let refs: Vec<Tile8Ref> = v
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or_else(|| invalid(&at, "must be four 8x8 tiles"))?
                .iter()
                .map(|v: &Value| {
                    v.as_str().and_then(parse_tile8).ok_or_else(|| {
                        invalid(&at, "an 8x8 tile is \"TTT P\" with optional x, y, p")
                    })
                })
                .collect::<Result<_, _>>()?;
            Some(Map16Tile {
                top_left: refs[0],
                top_right: refs[1],
                bottom_left: refs[2],
                bottom_right: refs[3],
            })
        }
    };
    Ok((acts, gfx))
}

/// `0A0 2`, `1F3 4 xp`.
fn tile8_text(r: Tile8Ref) -> String {
    let mut flags = String::new();
    for (set, c) in [(r.flip_x(), 'x'), (r.flip_y(), 'y'), (r.priority(), 'p')] {
        if set {
            flags.push(c);
        }
    }
    let base = format!("{:03X} {}", r.tile(), r.palette());
    if flags.is_empty() {
        base
    } else {
        format!("{base} {flags}")
    }
}

fn parse_tile8(text: &str) -> Option<Tile8Ref> {
    let mut parts = text.split_whitespace();
    let tile = u16::from_str_radix(parts.next()?, 16)
        .ok()
        .filter(|&t| t < 0x400)?;
    let palette = parts.next()?.parse::<u8>().ok().filter(|&p| p < 8)?;
    let flags = parts.next().unwrap_or("");
    if parts.next().is_some() || !flags.chars().all(|c| "xyp".contains(c)) {
        return None;
    }
    Some(Tile8Ref::new(
        tile,
        palette,
        flags.contains('p'),
        flags.contains('x'),
        flags.contains('y'),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(bytes: [u8; 8], acts: u16) -> Map16Entry {
        Map16Entry {
            gfx: Map16Tile::from_bytes(bytes),
            acts,
        }
    }

    #[test]
    fn a_page_round_trips_with_its_comments() {
        let mut page = Map16Page::default();
        page.tiles.insert(
            0x200,
            entry([0xA0, 0x08, 0xB0, 0x08, 0xA1, 0x08, 0xB1, 0x28], 0x130),
        );
        page.tiles.insert(
            0x2FF,
            entry([0xFF, 0xC2, 0xFF, 0xC2, 0xFF, 0x82, 0xFF, 0x82], 0x025),
        );
        let mut comments = PageComments::default();
        comments.top.push("# Castle tiles".into());
        comments.tiles.insert(0x2FF, vec!["# unused".into()]);
        let text = page.to_toml(PageKind::Foreground, &comments);
        assert_eq!(
            text,
            "# Castle tiles\n\n[tiles]\n\
             0x200 = { acts = 0x130, gfx = [\"0A0 2\", \"0A1 2\", \"0B0 2\", \"0B1 2 p\"] }\n\
             # unused\n\
             0x2FF = { acts = 0x025, gfx = [\"2FF 0 xy\", \"2FF 0 y\", \"2FF 0 xy\", \"2FF 0 y\"] }\n"
        );
        let (back, back_comments) =
            Map16Page::from_toml(PageKind::Foreground, 0x02, &text).unwrap();
        assert_eq!(back, page);
        assert_eq!(back_comments, comments);
        assert_eq!(back.to_toml(PageKind::Foreground, &back_comments), text);
    }

    #[test]
    fn background_page_ff_round_trips() {
        let mut page = Map16Page::default();
        for tile in [0xFF00, 0xFFFF] {
            page.tiles
                .insert(tile, entry([1, 0, 2, 0, 3, 0, 4, 0], DEFAULT_ACTS));
        }
        let text = page.to_toml(PageKind::Background, &PageComments::default());
        let (back, _) = Map16Page::from_toml(PageKind::Background, 0xFF, &text).unwrap();
        assert_eq!(back, page);
        let error = Map16Page::from_toml(PageKind::Background, 0xFE, &text)
            .unwrap_err()
            .to_string();
        assert!(error.contains("0xFE00 to 0xFEFF"), "{error}");
    }

    #[test]
    fn a_missing_tile_is_empty() {
        let (page, _) = Map16Page::from_toml(
            PageKind::Foreground,
            0x02,
            "[tiles]\n0x201 = { gfx = [\"001 1\", \"002 1\", \"003 1\", \"004 1\"] }\n",
        )
        .unwrap();
        assert_eq!(page.tile(0x200), Map16Entry::default());
        assert_eq!(page.tile(0x201).acts, DEFAULT_ACTS);
        assert_eq!(page.tile(0x201).gfx.top_right.tile(), 2);
    }

    #[test]
    fn bad_input_is_refused() {
        for (text, what) in [
            ("[tiles]\n0x300 = { acts = 0x25 }\n", "tile of another page"),
            ("[tiles]\n0x200 = { acts = 0x8000 }\n", "acts out of range"),
            (
                "[tiles]\n0x200 = { gfx = [\"400 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "8x8 tile past 3FF",
            ),
            (
                "[tiles]\n0x200 = { gfx = [\"000 8\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "palette 8",
            ),
            (
                "[tiles]\n0x200 = { gfx = [\"000 0 z\", \"0 0\", \"0 0\", \"0 0\"] }\n",
                "unknown flag",
            ),
            ("[tiles]\n0x200 = { gfx = [\"000 0\"] }\n", "one 8x8 tile"),
            ("[tiles]\n0x200 = { act = 0x25 }\n", "unknown key"),
            ("[pages]\n", "unknown table"),
        ] {
            assert!(
                Map16Page::from_toml(PageKind::Foreground, 0x02, text).is_err(),
                "{what}"
            );
        }
    }
}

#[cfg(test)]
mod pipe_tests {
    use super::*;

    #[test]
    fn a_pipes_file_round_trips_and_refuses_other_tiles() {
        let text = "# Red pipes on every screen.\n\n\
                    [colours.0]\n\
                    0x133 = { gfx = [\"0A0 4\", \"0A1 4\", \"0B0 4\", \"0B1 4\"] }\n\n\
                    [colours.3]\n\
                    0x13A = { gfx = [\"000 0\", \"001 0 x\", \"010 0 y\", \"011 0 p\"] }\n\n\
                    [diagonal]\n\
                    0x1EC = { gfx = [\"0F8 2\", \"1AC 2\", \"1A8 2\", \"1AD 2\"] }\n";
        let (pipes, top) = Pipes::from_toml(text).unwrap();
        assert_eq!(top, ["# Red pipes on every screen."]);
        assert_eq!(pipes.colours.len(), 2);
        assert!(pipes.colours.contains_key(&(3, 0x13A)));
        assert!(pipes.diagonal.contains_key(&0x1EC));
        assert_eq!(pipes.to_toml(&top), text);
        for bad in [
            "[colours.1]\n0x133 = { gfx = [\"0 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
            "[colours.0]\n0x13B = { gfx = [\"0 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
            "[diagonal]\n0x1C8 = { gfx = [\"0 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
            "[diagonal]\n0x1C4 = { acts = 0x130 }\n",
            "[tiles]\n",
        ] {
            assert!(Pipes::from_toml(bad).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod game_tests {
    use super::*;

    #[test]
    fn a_game_page_lists_only_what_changes() {
        let text = "[tiles]\n\
                    0x025 = { acts = 0x130 }\n\
                    # A question block that looks like another.\n\
                    0x0FF = { acts = 0x0FF, gfx = [\"060 4\", \"061 4\", \"062 4 x\", \"063 4 p\"] }\n";
        let (page, comments) = GamePage::from_toml(0x00, text).unwrap();
        assert_eq!(page.acts(0x025), 0x130);
        assert_eq!(page.acts(0x026), 0x026, "a tile left out acts like itself");
        assert_eq!(page.tiles[&0x025].gfx, None);
        let gfx = page.tiles[&0x0FF].gfx.unwrap();
        assert_eq!(gfx.top_right.tile(), 0x061);
        assert!(gfx.bottom_left.flip_x());
        assert_eq!(page.to_toml(&comments), text);
        for (text, what) in [
            ("[tiles]\n0x100 = { acts = 0x25 }\n", "a tile of page 1"),
            ("[tiles]\n0x025 = {}\n", "neither acts nor gfx"),
            ("[tiles]\n0x025 = { act = 0x25 }\n", "unknown key"),
        ] {
            assert!(GamePage::from_toml(0x00, text).is_err(), "{what}");
        }
        assert!(GamePage::from_toml(0x01, "[tiles]\n0x1FF = { acts = 0x25 }\n").is_ok());
    }

    #[test]
    fn a_tileset_file_holds_pages_0_to_2_without_acts() {
        let text = "[tiles]\n\
                    0x073 = { gfx = [\"0A0 2\", \"0A1 2\", \"0B0 2\", \"0B1 2\"] }\n\
                    0x2FF = { gfx = [\"001 1\", \"002 1\", \"003 1\", \"004 1\"] }\n";
        let (page, comments) = Map16Page::from_toml(PageKind::Tileset, 0x05, text).unwrap();
        assert_eq!(page.tiles.len(), 2);
        assert_eq!(page.to_toml(PageKind::Tileset, &comments), text);
        for text in [
            "[tiles]\n0x300 = { gfx = [\"0 0\", \"0 0\", \"0 0\", \"0 0\"] }\n",
            "[tiles]\n0x073 = { acts = 0x25 }\n",
        ] {
            assert!(Map16Page::from_toml(PageKind::Tileset, 0x05, text).is_err());
        }
    }
}
