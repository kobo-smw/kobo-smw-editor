//! Level files.
//!
//! ```toml
//! [header]
//! screens = 9
//! mode = 0x00
//! # ...
//!
//! [entrance]
//! screen = 0
//! # ...
//!
//! [layer1]
//! objects = [
//!     { obj = 0x05, x = 3, y = 10, height = 1, width = 5 },
//!     { ext = 0x2D, x = 52, y = 5 },
//!     { exit = 1, dest = 0x13 },
//! ]
//!
//! [layer2]
//! background = 0x0CE8FE
//!
//! [sprites]
//! memory = 0
//! list = [
//!     { id = 0x0F, x = 20, y = 10 },
//! ]
//! ```
//!
//! Objects are in drawing order. Positions are absolute tiles. A standard
//! object's settings byte is written as its handler reads it
//! ([`Settings`]): `width` and `height` in tiles, `type`, `length`, or,
//! for the tileset-specific objects, `settings`. `lm` entries are Lunar
//! Magic's placed objects, `raw` its unplaced ones, with their bytes.
//! An `exit` is a screen exit: in the game's format `dest` is the low byte
//! and `high` the flag bit it keeps; with `lm_format`, Lunar Magic's, `dest`
//! is the whole destination, a level or with `secondary` an entrance, up to
//! `1FFF` (past `1FF`, its long exit). `[header]`'s `rows` and
//! `bottom_row` are Lunar Magic's level size, and `split` its `T` in a
//! level whose layer 2 has no objects (with them it is implied).
//! `[entrance]` holds the game's secondary header and Lunar Magic's
//! per-level settings ([`LevelSettings`]), `[midway]` a midway entrance
//! with settings of its own.
//!
//! `[graphics]` is Lunar Magic's graphics list, written only when it
//! changes what the level loads: one key per slot by Lunar Magic's names
//! (`sp1` to `lg4`, `an2`, `lt3`), AN2's bits as `bypass`, `layer3_files`,
//! and `layer3_tilemap`, and LT3's settings as `tilemap_size` (the bytes
//! loaded) and `tilemap_vram` (where they go); `[graphics.layer3]` has
//! Lunar Magic's layer 3 settings. (Reviewed 2026-10-04, when LT3's
//! nibble, once one `tilemap` number, was split into those two.)

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

pub use super::Comments;
use super::{
    SourceError, Writer, hex, hex_bytes, invalid, own_line_comments, parse_hex_bytes, read_comments,
};
use crate::addr::SnesAddr;
use crate::entrance::{
    Background, EntranceSettings, LevelSettings, MidwayEntrance, SeparateMidway, offset_bits,
    offset_rows,
};
use crate::exgfx::{self, GraphicsList, slot};
use crate::level::objects::{Object, ScreenExit, Settings};
use crate::level::size::{LevelSize, SIZES};
use crate::level::{LevelMode, PrimaryHeader, SecondaryHeader};
use crate::names;
use crate::palette::{Color15, CustomPalette, Palette};
use crate::sprites::{SpriteEntry, SpriteHeader};

/// A level as its source file has it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Level {
    pub header: PrimaryHeader,
    /// Lunar Magic's level size: the height a horizontal level trades
    /// screens for, and the camera showing the last row whole.
    pub size: LevelSize,
    pub entrance: SecondaryHeader,
    /// Lunar Magic's settings for the main and midway entrances and the
    /// level's layers, which a build writes with Kobo's code for them.
    pub settings: LevelSettings,
    pub layer1: Vec<Object>,
    pub layer2: Layer2,
    pub sprites: Sprites,
    /// The secondary entrances that lead here.
    pub entrances: Vec<Entrance>,
    /// The level's own palette, in Lunar Magic's layout, if it has one.
    pub palette: Option<CustomPalette>,
    /// Lunar Magic's graphics list for the level, when it changes what the
    /// level loads ([`GraphicsList::is_used`]).
    pub graphics: Option<GraphicsList>,
    /// Lunar Magic's ExAnimation list for the level.
    pub animation: Option<crate::exanimation::List>,
    /// The level's ExAnimation settings byte (`$03FE00`), when it is not
    /// what a build gives a level that does not say
    /// ([`crate::exanimation::Settings::default_for`]).
    pub animation_settings: Option<u8>,
}

/// A secondary entrance, in the level it leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entrance {
    /// Its number, which screen exits name: the game's tables hold `000`
    /// to `1FF`, Lunar Magic's moved ones up to `1FFF`.
    pub id: u16,
    /// Screen, 0 to 31.
    pub screen: u8,
    /// X position setting, 0 to 7.
    pub x: u8,
    /// Y position setting, 0 to 15.
    pub y: u8,
    /// Entrance action, 0 to 7.
    pub action: u8,
    /// Foreground and background initial positions, 0 to 3 each; with a
    /// relative camera, the offset's low bits.
    pub fg_position: u8,
    pub bg_position: u8,
    /// Lunar Magic's settings: bits 4-7 of its `$05FE00` byte and its two
    /// further tables. Bit 3, Lunar Magic's copy of the destination's bit
    /// 8, is the level's and is not kept; builds write it where Kobo's
    /// exit code reads it.
    pub settings: EntranceSettings,
}

impl Entrance {
    /// From its bytes in the tables (the first, the destination, aside),
    /// and its Lunar Magic settings, read from those and further tables.
    pub fn from_bytes(id: u16, bytes: [u8; 3], settings: EntranceSettings) -> Self {
        let [fa, fc, fe] = bytes;
        Self {
            id,
            screen: fc & 0x1F,
            x: fc >> 5,
            y: fa & 0x0F,
            action: fe & 0x07,
            fg_position: (fa >> 4) & 0x03,
            bg_position: fa >> 6,
            settings,
        }
    }

    /// Its bytes in the tables at `$05FA00`, `$05FC00`, and `$05FE00`, and
    /// in Lunar Magic's two further ones.
    pub fn to_bytes(self) -> ([u8; 3], [u8; 2]) {
        let (flags, extra) = self.settings.to_bytes();
        if let Some(bytes) = self.settings.overworld {
            return (bytes, extra);
        }
        (
            [
                self.bg_position << 6 | (self.fg_position & 0x03) << 4 | (self.y & 0x0F),
                self.x << 5 | (self.screen & 0x1F),
                flags | (self.action & 0x07),
            ],
            extra,
        )
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Layer2 {
    /// Nothing, as the boss arenas' modes have it.
    None,
    Objects(Vec<Object>),
    /// A background tilemap already in the clean ROM, by address.
    VanillaBackground(SnesAddr),
    /// A background of the level's own, in Lunar Magic's layout.
    Background(BackgroundTiles),
}

/// A background's tiles: two halves of 16 columns side by side, as
/// [`BACKGROUND_ROWS`] rows of 32 tiles, each the left half's row then the
/// right half's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackgroundTiles {
    /// The BG Map16 table its tile numbers are in, 0 to 15.
    pub table: u8,
    /// 32 rows, Lunar Magic's own format; or 27, the game's, whose tiles
    /// all share one high byte and whose table is 0.
    pub rows: usize,
    /// `BACKGROUND_ROWS * 32` tiles; past `rows` rows they are 0.
    pub tiles: Vec<u16>,
}

/// Rows a background's tiles are written as.
pub const BACKGROUND_ROWS: usize = 32;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Sprites {
    /// Sprite memory setting, 0 to 31.
    pub memory: u8,
    pub buoyancy: bool,
    pub buoyancy_no_layer2: bool,
    pub list: Vec<Sprite>,
}

/// A sprite at absolute tile coordinates.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sprite {
    pub id: u8,
    pub x: u16,
    pub y: u16,
    /// Extra bits, 0 to 3.
    pub extra_bits: u8,
    /// Extension bytes, for sprites a tool gives some.
    pub extension: Vec<u8>,
}

impl Sprites {
    /// From a parsed list, placing each sprite as the game does on a
    /// level of that orientation.
    pub fn from_entries(header: SpriteHeader, entries: &[SpriteEntry], vertical: bool) -> Self {
        let list = entries
            .iter()
            .map(|entry| {
                let (x, y) = entry.tile_position(vertical);
                Sprite {
                    id: entry.id,
                    x: x as u16,
                    y: y as u16,
                    extra_bits: entry.extra_bits,
                    extension: entry.extension.clone(),
                }
            })
            .collect();
        Self {
            memory: header.memory,
            buoyancy: header.buoyancy,
            buoyancy_no_layer2: header.buoyancy_no_layer2,
            list,
        }
    }

    /// Whether the list needs Lunar Magic's new sprite system (see
    /// [`crate::sprites::needs_new_system`]) in a level of that
    /// orientation.
    pub fn needs_new_system(&self, vertical: bool) -> bool {
        crate::sprites::needs_new_system(&self.to_entries(vertical, false).1)
    }

    /// Whether the list needs Lunar Magic's sprite loader: the new sprite
    /// system, or more sprites than the game's loader reads.
    pub fn needs_lunar_magic(&self, vertical: bool) -> bool {
        self.list.len() > crate::sprites::GAME_MAX_SPRITES || self.needs_new_system(vertical)
    }

    /// The header and entries to encode, with `new_sprite_system` for the
    /// list format. Positions a list cannot hold are left for the encoder
    /// to refuse.
    pub fn to_entries(
        &self,
        vertical: bool,
        new_sprite_system: bool,
    ) -> (SpriteHeader, Vec<SpriteEntry>) {
        let header = SpriteHeader {
            memory: self.memory,
            buoyancy: self.buoyancy,
            buoyancy_no_layer2: self.buoyancy_no_layer2,
            new_sprite_system,
        };
        let entries = self
            .list
            .iter()
            .map(|sprite| {
                let (along, across) = if vertical {
                    (sprite.y, sprite.x)
                } else {
                    (sprite.x, sprite.y)
                };
                SpriteEntry {
                    id: sprite.id,
                    extra_bits: sprite.extra_bits,
                    screen: (along / 16).min(0xFF) as u8,
                    x: (along % 16) as u8,
                    y: across,
                    extension: sprite.extension.clone(),
                }
            })
            .collect();
        (header, entries)
    }
}

impl Level {
    /// Writes the level in Kobo's format, with the user's comments before
    /// their places.
    pub fn to_toml(&self, comments: &Comments) -> String {
        let mut out = Writer::new(comments);
        let h = &self.header;
        out.table("header");
        let mut key = |k, v| out.key("header", k, v);
        key("screens", h.screens.to_string());
        key(
            "mode",
            named(hex(h.level_mode.0 as u32, 2), h.level_mode.name()),
        );
        key(
            "tileset",
            named(
                hex(h.object_tileset as u32, 1),
                names::object_tileset(h.object_tileset),
            ),
        );
        key(
            "sprite_tileset",
            named(
                hex(h.sprite_tileset as u32, 1),
                names::sprite_tileset(h.sprite_tileset),
            ),
        );
        key("music", named(h.music.to_string(), names::music(h.music)));
        key("time", h.time.to_string());
        key("bg_palette", h.bg_palette.to_string());
        key("fg_palette", h.fg_palette.to_string());
        key("sprite_palette", h.sprite_palette.to_string());
        key("back_area", h.back_area.to_string());
        key("item_memory", h.item_memory.to_string());
        key("vertical_scroll", h.vertical_scroll.to_string());
        key("layer3_priority", h.layer3_priority.to_string());
        if self.size.mode != 0 {
            out.key("header", "rows", self.size.rows());
        } else {
            out.key_comments("header", "rows");
        }
        if self.size.bottom_row {
            out.key("header", "bottom_row", true);
        } else {
            out.key_comments("header", "bottom_row");
        }
        if self.size.split {
            out.key("header", "split", true);
        } else {
            out.key_comments("header", "split");
        }

        let e = &self.entrance;
        let lm = &self.settings;
        out.table("entrance");
        out.key("entrance", "screen", e.entrance_screen);
        let (x_high, y_high) = lm.tile_position.unwrap_or((0, 0));
        out.key("entrance", "x", x_high << 3 | e.entrance_x);
        out.key("entrance", "y", (y_high as u16) << 4 | e.entrance_y as u16);
        if lm.tile_position.is_some() {
            out.key("entrance", "method", 2);
        } else {
            out.key_comments("entrance", "method");
        }
        out.key("entrance", "action", e.entrance_action);
        out.key(
            "entrance",
            "midway_screen",
            (lm.midway.screen_high as u8) << 4 | e.midway_screen,
        );
        camera_keys(
            &mut out,
            "entrance",
            e.fg_position,
            e.bg_position,
            lm.relative,
        );
        // With separate settings, the horizontal one: the nibble, and H.
        out.key(
            "entrance",
            "layer2_scroll",
            e.layer2_scroll | (lm.layer2_horizontal_high as u8) << 4,
        );
        match lm.layer2_vertical_scroll {
            Some(v) => out.key("entrance", "layer2_vertical_scroll", v),
            None => out.key_comments("entrance", "layer2_vertical_scroll"),
        }
        out.key("entrance", "layer3", e.layer3);
        out.key("entrance", "no_yoshi_intro", e.no_yoshi_intro);
        out.key("entrance", "vertical_position", e.vertical_position);
        if e.unknown {
            out.key("entrance", "unknown", true);
        } else {
            out.key_comments("entrance", "unknown");
        }
        for (key, value) in [
            ("slippery", lm.slippery),
            ("water", lm.water),
            ("face_left", lm.face_left),
            ("smart_spawn", lm.smart_spawn),
        ] {
            if value {
                out.key("entrance", key, true);
            } else {
                out.key_comments("entrance", key);
            }
        }
        if lm.spawn_range != 0 {
            out.key("entrance", "spawn_range", lm.spawn_range);
        } else {
            out.key_comments("entrance", "spawn_range");
        }
        if !lm.auto_screens {
            out.key("entrance", "auto_screens", false);
        } else {
            out.key_comments("entrance", "auto_screens");
        }
        match lm.background {
            Background::Height(27) => {}
            Background::Height(rows) => out.key("entrance", "bg_height", rows),
            Background::Offset(rows) => out.key("entrance", "bg_offset", rows),
            Background::Absolute => out.key("entrance", "bg_offset", "\"absolute\""),
        }
        match lm.midway.separate {
            None => {}
            Some(SeparateMidway::Redirect(level)) => {
                out.table("midway");
                out.key("midway", "redirect", hex(level as u32, 3));
            }
            Some(SeparateMidway::Entrance(m)) => {
                out.table("midway");
                out.key("midway", "x", m.x);
                out.key("midway", "y", m.y);
                out.key("midway", "action", m.action);
                camera_keys(&mut out, "midway", m.fg_position, m.bg_position, m.relative);
                for (key, value) in [
                    ("slippery", m.slippery),
                    ("water", m.water),
                    ("face_left", m.face_left),
                ] {
                    if value {
                        out.key("midway", key, true);
                    } else {
                        out.key_comments("midway", key);
                    }
                }
            }
        }

        let tileset = h.object_tileset;
        out.table("layer1");
        out.list("layer1", "objects", &self.layer1, |o| {
            object_line(o, tileset)
        });
        match &self.layer2 {
            Layer2::None => {}
            Layer2::Objects(objects) => {
                out.table("layer2");
                out.list("layer2", "objects", objects, |o| object_line(o, tileset));
            }
            Layer2::VanillaBackground(addr) => {
                out.table("layer2");
                out.key("layer2", "background", hex(addr.raw(), 6));
            }
            Layer2::Background(bg) => {
                out.table("layer2");
                out.key("layer2", "table", hex(bg.table as u32, 1));
                out.key("layer2", "rows", bg.rows);
                let digits = if bg.tiles.iter().any(|&t| t > 0xFFF) {
                    4
                } else {
                    3
                };
                let mut tiles = String::from("\"\"\"\n");
                for row in bg.tiles.chunks(32).take(bg.rows) {
                    let cells: Vec<String> = row.iter().map(|t| format!("{t:0digits$X}")).collect();
                    tiles += &cells.join(" ");
                    tiles += "\n";
                }
                tiles += "\"\"\"";
                out.key("layer2", "tiles", tiles);
            }
        }

        let s = &self.sprites;
        out.table("sprites");
        out.key("sprites", "memory", s.memory);
        for (flag, set) in [
            ("buoyancy", s.buoyancy),
            ("buoyancy_no_layer2", s.buoyancy_no_layer2),
        ] {
            if set {
                out.key("sprites", flag, true);
            } else {
                out.key_comments("sprites", flag);
            }
        }
        out.list("sprites", "list", &s.list, sprite_line);
        if !self.entrances.is_empty() {
            out.table("entrances");
            out.list("entrances", "list", &self.entrances, entrance_line);
        }
        if let Some(palette) = &self.palette {
            out.table("palette");
            out.key(
                "palette",
                "back_area",
                format!("\"{}\"", color_text(palette.back_area)),
            );
            let rows: Vec<(usize, &[Color15])> =
                palette.palette.colors.chunks(16).enumerate().collect();
            out.list("palette", "colors", &rows, |(row, colors)| {
                let cells: Vec<String> = colors.iter().map(|&c| color_text(c)).collect();
                (
                    format!("\"{}\"", cells.join(" ")),
                    Some(format!("row {row:X}")),
                )
            });
        }
        if let Some(list) = &self.graphics {
            write_graphics(&mut out, list);
        }
        if self.animation.is_some() || self.animation_settings.is_some() {
            super::animation::write(
                &mut out,
                "animation",
                self.animation_settings,
                self.animation.as_ref(),
            );
        }
        out.finish()
    }

    /// Reads a level file, with the own-line comments to keep when it is
    /// written again.
    pub fn from_toml(text: &str) -> Result<(Self, Comments), SourceError> {
        let doc: DocumentMut = text.parse()?;
        let mut comments = read_comments(&doc);
        check_keys(
            doc.as_table(),
            "file",
            &[
                "header",
                "entrance",
                "midway",
                "layer1",
                "layer2",
                "sprites",
                "entrances",
                "palette",
                "graphics",
                "animation",
            ],
        )?;

        let (header, size) = read_header(table(&doc, "header")?)?;
        let midway = match doc.get("midway") {
            None => None,
            Some(item) => Some(
                item.as_table()
                    .ok_or_else(|| invalid("midway", "must be a table"))?,
            ),
        };
        let (entrance, settings) = read_entrance(table(&doc, "entrance")?, midway)?;
        let layer1 = table(&doc, "layer1")?;
        check_keys(layer1, "layer1", &["objects"])?;
        let layer1 = read_list(layer1, "objects", "layer1", &mut comments, read_object)?;
        let layer2 = match doc.get("layer2") {
            None => Layer2::None,
            Some(item) => {
                let t = item
                    .as_table()
                    .ok_or_else(|| invalid("layer2", "must be a table"))?;
                check_keys(
                    t,
                    "layer2",
                    &["objects", "background", "table", "rows", "tiles"],
                )?;
                if t.contains_key("tiles") {
                    if t.contains_key("objects") || t.contains_key("background") {
                        return Err(invalid(
                            "layer2",
                            "`tiles` is a background of its own: leave out `objects` and \
                             `background`",
                        ));
                    }
                    Layer2::Background(read_background(t)?)
                } else {
                    if t.contains_key("table") || t.contains_key("rows") {
                        return Err(invalid("layer2", "`table` and `rows` go with `tiles`"));
                    }
                    match (t.get("objects"), t.get("background")) {
                        (Some(_), None) => Layer2::Objects(read_list(
                            t,
                            "objects",
                            "layer2",
                            &mut comments,
                            read_object,
                        )?),
                        (None, Some(bg)) => {
                            let addr = int(bg, "layer2.background", 0xFF_FFFF)?;
                            Layer2::VanillaBackground(SnesAddr::new(addr))
                        }
                        _ => {
                            return Err(invalid(
                                "layer2",
                                "needs one of `objects`, `background`, and `tiles`",
                            ));
                        }
                    }
                }
            }
        };
        let sprites = table(&doc, "sprites")?;
        check_keys(
            sprites,
            "sprites",
            &["memory", "buoyancy", "buoyancy_no_layer2", "list"],
        )?;
        let sprites = Sprites {
            memory: field(sprites, "sprites", "memory", 0x1F)? as u8,
            buoyancy: flag(sprites, "sprites", "buoyancy")?,
            buoyancy_no_layer2: flag(sprites, "sprites", "buoyancy_no_layer2")?,
            list: read_list(sprites, "list", "sprites", &mut comments, read_sprite)?,
        };
        let entrances = match doc.get("entrances") {
            None => Vec::new(),
            Some(item) => {
                let t = item
                    .as_table()
                    .ok_or_else(|| invalid("entrances", "must be a table"))?;
                check_keys(t, "entrances", &["list"])?;
                read_list(t, "list", "entrances", &mut comments, read_entrance_entry)?
            }
        };
        let graphics = match doc.get("graphics") {
            None => None,
            Some(item) => Some(read_graphics(
                item.as_table()
                    .ok_or_else(|| invalid("graphics", "must be a table"))?,
            )?),
        };
        let (animation_settings, animation) = match doc.get("animation") {
            None => (None, None),
            Some(item) => super::animation::read(
                item.as_table()
                    .ok_or_else(|| invalid("animation", "must be a table"))?,
                "animation",
                true,
                &mut comments,
            )?,
        };
        let palette = match doc.get("palette") {
            None => None,
            Some(item) => Some(read_palette(
                item.as_table()
                    .ok_or_else(|| invalid("palette", "must be a table"))?,
            )?),
        };
        Ok((
            Self {
                header,
                size,
                entrance,
                settings,
                layer1,
                layer2,
                sprites,
                entrances,
                palette,
                graphics,
                animation,
                animation_settings,
            },
            comments,
        ))
    }
}

/// A graphics list's keys: the slots in load order, then the rest, with
/// the high bits Kobo reads out of each.
const GRAPHICS_SLOTS: [(&str, usize); 16] = [
    ("sp1", slot::SP1),
    ("sp2", slot::SP2),
    ("sp3", slot::SP3),
    ("sp4", slot::SP4),
    ("fg1", slot::FG1),
    ("fg2", slot::FG2),
    ("bg1", slot::BG1),
    ("fg3", slot::FG3),
    ("bg2", slot::BG2),
    ("bg3", slot::BG3),
    ("lg1", slot::LG1),
    ("lg2", slot::LG2),
    ("lg3", slot::LG3),
    ("lg4", slot::LG4),
    ("an2", slot::AN2),
    ("lt3", slot::LT3),
];

/// The bits of a slot's word its key leaves to others: AN2's and LT3's
/// high nibbles, and the layer 3 settings' ([`exgfx::SETTINGS_SLOTS`]).
fn graphics_high_bits(s: usize) -> u16 {
    if s == slot::AN2 || s == slot::LT3 || exgfx::SETTINGS_SLOTS.contains(&s) {
        0xF000
    } else {
        0
    }
}

fn write_graphics(out: &mut Writer<'_>, list: &GraphicsList) {
    out.table("graphics");
    for (key, set) in [
        ("bypass", list.bypass()),
        ("layer3_files", list.layer3_files()),
        ("layer3_tilemap", list.layer3_tilemap()),
    ] {
        if set {
            out.key("graphics", key, true);
        } else {
            out.key_comments("graphics", key);
        }
    }
    let tilemap = list.tilemap_settings();
    if list.0[slot::LT3] != exgfx::EMPTY && tilemap != 0 {
        let size = match exgfx::TILEMAP_SIZES.get((tilemap & 3) as usize) {
            Some(&bytes) => hex(bytes as u32, 4),
            None => "3".into(),
        };
        out.key("graphics", "tilemap_size", size);
        let vram = exgfx::TILEMAP_VRAM[(tilemap >> 2) as usize];
        out.key("graphics", "tilemap_vram", hex(vram as u32, 4));
    } else {
        out.key_comments("graphics", "tilemap_size");
        out.key_comments("graphics", "tilemap_vram");
    }
    for (key, s) in GRAPHICS_SLOTS {
        let word = match list.0[s] {
            exgfx::EMPTY => exgfx::EMPTY,
            word => word & !graphics_high_bits(s),
        };
        out.key("graphics", key, hex(word as u32, 2));
    }
    if list.has_layer3() {
        write_layer3(out, &list.layer3());
    }
}

/// `[graphics.layer3]`: Lunar Magic's layer 3 settings, written when the
/// list has any of its own ([`GraphicsList::has_layer3`]).
fn write_layer3(out: &mut Writer<'_>, s: &exgfx::Layer3Settings) {
    const T: &str = "graphics.layer3";
    out.table(T);
    let flag = |out: &mut Writer<'_>, key: &str, set: bool| {
        if set {
            out.key(T, key, true);
        } else {
            out.key_comments(T, key);
        }
    };
    flag(out, "advanced", s.advanced);
    for (key, value) in [("horizontal", s.horizontal), ("vertical", s.vertical)] {
        out.key(
            T,
            key,
            named(hex(value as u32, 2), names::layer3_scroll(value)),
        );
    }
    out.key(T, "x", [0, 4, 8, 16][s.x as usize & 3]);
    out.key(T, "y", s.y);
    flag(out, "cgadsub", s.cgadsub);
    flag(out, "subscreen", s.subscreen);
    flag(out, "sync_fix", s.sync_fix);
    flag(out, "sprites_air", s.sprites_air);
    if s.tides_act_as != 0 {
        out.key(T, "tides_act_as", hex(s.tides_act_as as u32, 1));
    } else {
        out.key_comments(T, "tides_act_as");
    }
    flag(out, "unknown", s.unknown);
}

fn read_layer3(t: &Table) -> Result<exgfx::Layer3Settings, SourceError> {
    const T: &str = "graphics.layer3";
    check_keys(
        t,
        T,
        &[
            "advanced",
            "horizontal",
            "vertical",
            "x",
            "y",
            "cgadsub",
            "subscreen",
            "sync_fix",
            "sprites_air",
            "tides_act_as",
            "unknown",
        ],
    )?;
    let x = match field(t, T, "x", 16)? {
        0 => 0,
        4 => 1,
        8 => 2,
        16 => 3,
        _ => return Err(invalid(format!("{T}.x"), "must be 0, 4, 8, or 16")),
    };
    let y = t
        .get("y")
        .ok_or_else(|| invalid(format!("{T}.y"), "is missing"))?
        .as_integer()
        .filter(|y| (-0x400..0x400).contains(y))
        .ok_or_else(|| invalid(format!("{T}.y"), "must be an integer from -1024 to 1023"))?;
    let tides_act_as = match t.get("tides_act_as") {
        None => 0,
        Some(item) => int(item, &format!("{T}.tides_act_as"), 0x0F)? as u8,
    };
    Ok(exgfx::Layer3Settings {
        advanced: flag(t, T, "advanced")?,
        horizontal: field(t, T, "horizontal", 0x1F)? as u8,
        vertical: field(t, T, "vertical", 0x1F)? as u8,
        x,
        y: y as i16,
        cgadsub: flag(t, T, "cgadsub")?,
        subscreen: flag(t, T, "subscreen")?,
        sync_fix: flag(t, T, "sync_fix")?,
        sprites_air: flag(t, T, "sprites_air")?,
        tides_act_as,
        unknown: flag(t, T, "unknown")?,
    })
}

fn read_graphics(t: &Table) -> Result<GraphicsList, SourceError> {
    let mut keys: Vec<&str> = GRAPHICS_SLOTS.iter().map(|(k, _)| *k).collect();
    keys.extend([
        "bypass",
        "layer3_files",
        "layer3_tilemap",
        "tilemap_size",
        "tilemap_vram",
        "layer3",
    ]);
    check_keys(t, "graphics", &keys)?;
    let mut list = GraphicsList([0; 16]);
    for (key, s) in GRAPHICS_SLOTS {
        let word = field(t, "graphics", key, 0xFFFF)? as u16;
        if word & graphics_high_bits(s) != 0 && word != exgfx::EMPTY {
            return Err(invalid(
                format!("graphics.{key}"),
                "is a file, 0xFFF at most, or 0xFFFF for none",
            ));
        }
        list.0[s] = word;
    }
    for (key, bit) in [
        ("bypass", exgfx::BYPASS),
        ("layer3_files", exgfx::LAYER3_FILES),
        ("layer3_tilemap", exgfx::LAYER3_TILEMAP),
    ] {
        if flag(t, "graphics", key)? {
            list.0[slot::AN2] |= bit;
        }
    }
    let mut tilemap = 0;
    if let Some(item) = t.get("tilemap_size") {
        let bytes = int(item, "graphics.tilemap_size", 0x2000)? as u16;
        tilemap |= match exgfx::TILEMAP_SIZES.iter().position(|&b| b == bytes) {
            Some(size) => size as u16,
            None if bytes == 3 => 3,
            None => {
                return Err(invalid(
                    "graphics.tilemap_size",
                    "is 0x2000, 0x1000, or 0x800 bytes (or 3, the size Lunar Magic does not offer)",
                ));
            }
        };
    }
    if let Some(item) = t.get("tilemap_vram") {
        let vram = int(item, "graphics.tilemap_vram", 0xFFFF)? as u16;
        let at = exgfx::TILEMAP_VRAM.iter().position(|&v| v == vram);
        tilemap |= (at.ok_or_else(|| {
            invalid(
                "graphics.tilemap_vram",
                "is 0x50A0 (under the status bar), 0x5000, 0x5080, or 0x5800",
            )
        })? as u16)
            << 2;
    }
    if tilemap != 0 {
        if list.0[slot::LT3] == exgfx::EMPTY {
            return Err(invalid(
                "graphics.tilemap_size",
                "needs lt3 to be a file, not 0xFFFF",
            ));
        }
        list.0[slot::LT3] |= tilemap << 12;
    }
    if let Some(item) = t.get("layer3") {
        let settings = read_layer3(
            item.as_table()
                .ok_or_else(|| invalid("graphics.layer3", "must be a table"))?,
        )?;
        list.set_layer3(&settings).map_err(|slot| {
            invalid(
                "graphics.layer3",
                format!(
                    "{slot} is 0xFFFF, whose setting Lunar Magic reads as F; these settings \
                     need another there"
                ),
            )
        })?;
    }
    Ok(list)
}

/// `#RRGGBB`, each channel the SNES 5-bit value times 8.
pub(crate) fn color_text(c: Color15) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r() * 8, c.g() * 8, c.b() * 8)
}

pub(crate) fn parse_color(at: &str, text: &str) -> Result<Color15, SourceError> {
    let bad = || {
        invalid(
            at,
            format!("{text:?} is not a colour #RRGGBB of multiples of 8"),
        )
    };
    let hex = text
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .ok_or_else(bad)?;
    let channel = |i: usize| {
        u8::from_str_radix(&hex[i..i + 2], 16)
            .ok()
            .filter(|v| v % 8 == 0)
            .map(|v| v / 8)
            .ok_or_else(bad)
    };
    Ok(Color15::from_rgb5(channel(0)?, channel(2)?, channel(4)?))
}

fn read_palette(t: &Table) -> Result<CustomPalette, SourceError> {
    check_keys(t, "palette", &["back_area", "colors"])?;
    let back_area = t
        .get("back_area")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("palette.back_area", "must be a colour \"#RRGGBB\""))?;
    let back_area = parse_color("palette.back_area", back_area)?;
    let rows = t
        .get("colors")
        .and_then(|v| v.as_array())
        .filter(|a| a.len() == 16)
        .ok_or_else(|| invalid("palette.colors", "must be 16 rows of 16 colours"))?;
    let mut palette = Palette::default();
    for (row, value) in rows.iter().enumerate() {
        let at = format!("palette.colors[{row}]");
        let text = value
            .as_str()
            .ok_or_else(|| invalid(&at, "must be a string of 16 colours"))?;
        let cells: Vec<&str> = text.split_whitespace().collect();
        if cells.len() != 16 {
            return Err(invalid(&at, format!("has {} colours, not 16", cells.len())));
        }
        for (i, cell) in cells.iter().enumerate() {
            palette.colors[row * 16 + i] = parse_color(&at, cell)?;
        }
    }
    Ok(CustomPalette { back_area, palette })
}

fn object_line(object: &Object, tileset: u8) -> (String, Option<String>) {
    let text = match object {
        Object::Standard {
            number,
            x,
            y,
            settings,
        } => {
            let (hi, lo) = (settings >> 4, settings & 0x0F);
            let fields = match Settings::of(*number) {
                Settings::HeightWidth => format!("height = {}, width = {}", hi + 1, lo + 1),
                Settings::HeightType => format!("height = {}, type = {lo}", hi + 1),
                Settings::TypeWidth => format!("type = {hi}, width = {}", lo + 1),
                Settings::Height if lo == 0 => format!("height = {}", hi + 1),
                Settings::Height => format!("height = {}, unused = {lo}", hi + 1),
                Settings::Width if hi == 0 => format!("width = {}", lo + 1),
                Settings::Width => format!("unused = {hi}, width = {}", lo + 1),
                Settings::Length => format!("length = {}", *settings as u16 + 1),
                Settings::Raw => format!("settings = {}", hex(*settings as u32, 2)),
            };
            format!(
                "{{ obj = {}, x = {x}, y = {y}, {fields} }}",
                hex(*number as u32, 2)
            )
        }
        Object::Extended { number, x, y } => {
            format!("{{ ext = {}, x = {x}, y = {y} }}", hex(*number as u32, 2))
        }
        Object::ScreenExit(exit) => {
            // In Lunar Magic's format, the whole destination, up to 1FFF.
            let lunar = exit.flags & ScreenExit::LUNAR_MAGIC != 0;
            let dest = if lunar {
                exit.lunar_magic_destination()
            } else {
                exit.destination as u16
            };
            let mut text = format!(
                "{{ exit = {}, dest = {}",
                exit.screen,
                hex(dest as u32, if dest > 0xFF { 3 } else { 2 })
            );
            let secondary = exit.flags & 0x02 != 0;
            for (bit, name) in [
                (0x08, if secondary { "water" } else { "midway" }),
                (0x04, "lm_format"),
                (0x02, "secondary"),
                (if lunar { 0 } else { 0x01 }, "high"),
            ] {
                if exit.flags & bit != 0 {
                    text += &format!(", {name} = true");
                }
            }
            text + " }"
        }
        Object::Lunar { number, x, y, data } => format!(
            "{{ lm = {}, x = {x}, y = {y}, data = {} }}",
            hex(*number as u32, 2),
            hex_bytes(data)
        ),
        Object::Unplaced(bytes) => format!("{{ raw = {} }}", hex_bytes(bytes)),
    };
    (text, names::object(object, tileset).map(str::to_owned))
}

/// `fg_position` and `bg_position`, or with a relative camera the offset
/// in rows they make with `F` (`Fffbb`).
/// The camera's keys: the game's two positions, or Lunar Magic's
/// `relative` setting in their place.
fn camera_keys(out: &mut Writer, table: &str, fg: u8, bg: u8, relative: Option<bool>) {
    match relative {
        None => {
            out.key(table, "fg_position", fg);
            out.key(table, "bg_position", bg);
        }
        Some(high) => out.key(table, "relative", offset_rows(high, fg << 2 | bg)),
    }
}

fn entrance_line(e: &Entrance) -> (String, Option<String>) {
    let s = &e.settings;
    if let Some(bytes) = s.overworld {
        let text = format!(
            "{{ id = {}, overworld = {} }}",
            hex(e.id as u32, 3),
            hex_bytes(&bytes)
        );
        return (text, None);
    }
    let (x_high, y_high) = s.tile_position.unwrap_or((0, 0));
    let mut text = format!(
        "{{ id = {}, screen = {}, x = {}, y = {}",
        hex(e.id as u32, 3),
        e.screen,
        x_high << 3 | e.x,
        (y_high as u16) << 4 | e.y as u16,
    );
    if s.tile_position.is_some() {
        text += ", method = 2";
    }
    text += &format!(", action = {}", e.action);
    // A secondary entrance's offset is `Fbbff`.
    match s.relative {
        None => {
            text += &format!(
                ", fg_position = {}, bg_position = {}",
                e.fg_position, e.bg_position
            )
        }
        Some(high) => {
            text += &format!(
                ", relative = {}",
                offset_rows(high, e.bg_position << 2 | e.fg_position)
            )
        }
    }
    for (key, value) in [
        ("slippery", s.slippery),
        ("water", s.water),
        ("face_left", s.face_left),
    ] {
        if value {
            text += &format!(", {key} = true");
        }
    }
    (text + " }", None)
}

fn sprite_line(sprite: &Sprite) -> (String, Option<String>) {
    let mut text = format!(
        "{{ id = {}, x = {}, y = {}",
        hex(sprite.id as u32, 2),
        sprite.x,
        sprite.y
    );
    if sprite.extra_bits != 0 {
        text += &format!(", extra = {}", sprite.extra_bits);
    }
    if !sprite.extension.is_empty() {
        text += &format!(", data = {}", hex_bytes(&sprite.extension));
    }
    (text + " }", Some(names::sprite(sprite.id).to_owned()))
}

/// A value, and Kobo's name for it after it if it has one.
fn named(value: String, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{value}  # {name}"),
        None => value,
    }
}

fn table<'a>(doc: &'a DocumentMut, key: &str) -> Result<&'a Table, SourceError> {
    doc.get(key)
        .ok_or_else(|| invalid(key, "is missing"))?
        .as_table()
        .ok_or_else(|| invalid(key, "must be a table"))
}

fn read_background(t: &Table) -> Result<BackgroundTiles, SourceError> {
    let table = t
        .get("table")
        .map(|v| int(v, "layer2.table", 15))
        .transpose()?
        .unwrap_or(0) as u8;
    let rows = t
        .get("rows")
        .map(|v| int(v, "layer2.rows", 32))
        .transpose()?
        .unwrap_or(32) as usize;
    if rows != 27 && rows != 32 {
        return Err(invalid("layer2.rows", "a background has 27 rows or 32"));
    }
    let text = t
        .get("tiles")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("layer2.tiles", "must be a string of rows of tiles"))?;
    let mut tiles = Vec::with_capacity(BACKGROUND_ROWS * 32);
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let at = format!("layer2.tiles row {i}");
        let row: Vec<u16> = line
            .split_whitespace()
            .map(|cell| {
                u16::from_str_radix(cell, 16)
                    .map_err(|_| invalid(&at, format!("{cell:?} is not a tile number")))
            })
            .collect::<Result<_, _>>()?;
        if row.len() != 32 {
            return Err(invalid(&at, format!("has {} tiles, not 32", row.len())));
        }
        tiles.extend(row);
    }
    if tiles.len() != rows * 32 {
        return Err(invalid(
            "layer2.tiles",
            format!("has {} rows, not {rows}", tiles.len() / 32),
        ));
    }
    tiles.resize(BACKGROUND_ROWS * 32, 0);
    Ok(BackgroundTiles { table, rows, tiles })
}

pub(super) fn check_keys<'a>(
    keys: impl IntoIterator<Item = (&'a str, impl Sized)>,
    at: &str,
    allowed: &[&str],
) -> Result<(), SourceError> {
    for (key, _) in keys {
        if !allowed.contains(&key) {
            return Err(invalid(at, format!("unknown key `{key}`")));
        }
    }
    Ok(())
}

pub(super) fn int_value(value: &Value, at: &str, max: u32) -> Result<u32, SourceError> {
    value
        .as_integer()
        .ok_or_else(|| invalid(at, "must be an integer"))
        .and_then(|n| {
            u32::try_from(n)
                .ok()
                .filter(|&n| n <= max)
                .ok_or_else(|| invalid(at, format!("{n} is out of range (0 to {max})")))
        })
}

pub(super) fn int(item: &Item, at: &str, max: u32) -> Result<u32, SourceError> {
    int_value(
        item.as_value()
            .ok_or_else(|| invalid(at, "must be a value"))?,
        at,
        max,
    )
}

pub(super) fn field(table: &Table, at: &str, key: &str, max: u32) -> Result<u32, SourceError> {
    let at = format!("{at}.{key}");
    int(
        table.get(key).ok_or_else(|| invalid(&at, "is missing"))?,
        &at,
        max,
    )
}

pub(super) fn flag(table: &Table, at: &str, key: &str) -> Result<bool, SourceError> {
    match table.get(key) {
        None => Ok(false),
        Some(item) => item
            .as_bool()
            .ok_or_else(|| invalid(format!("{at}.{key}"), "must be true or false")),
    }
}

fn read_header(t: &Table) -> Result<(PrimaryHeader, LevelSize), SourceError> {
    check_keys(
        t,
        "header",
        &[
            "screens",
            "mode",
            "tileset",
            "sprite_tileset",
            "music",
            "time",
            "bg_palette",
            "fg_palette",
            "sprite_palette",
            "back_area",
            "item_memory",
            "vertical_scroll",
            "layer3_priority",
            "rows",
            "bottom_row",
            "split",
        ],
    )?;
    let f = |key, max| field(t, "header", key, max).map(|n| n as u8);
    let rows = if t.contains_key("rows") {
        field(t, "header", "rows", 0xFFFF)? as usize
    } else {
        27
    };
    let mode = LevelSize::with_rows(rows).ok_or_else(|| {
        let heights: Vec<String> = SIZES.iter().map(|(h, _)| (h / 16).to_string()).collect();
        invalid(
            "header.rows",
            format!(
                "must be one of Lunar Magic's heights: {}",
                heights.join(", ")
            ),
        )
    })?;
    let size = LevelSize {
        mode,
        bottom_row: flag(t, "header", "bottom_row")?,
        split: flag(t, "header", "split")?,
    };
    let screens = f("screens", 32)?;
    if screens == 0 {
        return Err(invalid("header.screens", "must be 1 to 32"));
    }
    let header = PrimaryHeader {
        bg_palette: f("bg_palette", 7)?,
        screens,
        back_area: f("back_area", 7)?,
        level_mode: LevelMode(f("mode", 0x1F)?),
        layer3_priority: flag(t, "header", "layer3_priority")?,
        music: f("music", 7)?,
        sprite_tileset: f("sprite_tileset", 0x0F)?,
        time: f("time", 3)?,
        sprite_palette: f("sprite_palette", 7)?,
        fg_palette: f("fg_palette", 7)?,
        item_memory: f("item_memory", 3)?,
        vertical_scroll: f("vertical_scroll", 3)?,
        object_tileset: f("tileset", 0x0F)?,
    };
    Ok((header, size))
}

/// An entrance's X and Y settings' low bits, and with position method 2
/// the high ones.
type Position = (u8, u16, Option<(u8, u8)>);

/// A table's values by key, for readers that inline tables and tables
/// share.
struct Fields<'a> {
    at: &'a str,
    get: &'a dyn Fn(&str) -> Option<&'a Value>,
}

impl Fields<'_> {
    fn path(&self, key: &str) -> String {
        format!("{}.{key}", self.at)
    }

    fn uint(&self, key: &str, max: u32) -> Result<Option<u32>, SourceError> {
        (self.get)(key)
            .map(|v| int_value(v, &self.path(key), max))
            .transpose()
    }

    fn required(&self, key: &str, max: u32) -> Result<u32, SourceError> {
        self.uint(key, max)?
            .ok_or_else(|| invalid(self.path(key), "is missing"))
    }

    fn int(&self, key: &str, min: i64, max: i64) -> Result<Option<i64>, SourceError> {
        (self.get)(key)
            .map(|v| {
                v.as_integer()
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| {
                        invalid(
                            self.path(key),
                            format!("must be an integer from {min} to {max}"),
                        )
                    })
            })
            .transpose()
    }

    fn flag(&self, key: &str) -> Result<bool, SourceError> {
        (self.get)(key)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| invalid(self.path(key), "must be true or false"))
            })
            .transpose()
            .map(|b| b.unwrap_or(false))
    }

    /// An entrance's position: `x` and `y`, which position method 2
    /// (`method = 2`) lets go past the game's settings to a tile, 0 to 31
    /// and 0 to 1023. Returns the low bits and, with method 2, the high.
    fn position(&self) -> Result<Position, SourceError> {
        match self.uint("method", 2)?.unwrap_or(1) {
            1 => Ok((
                self.required("x", 7)? as u8,
                self.required("y", 15)? as u16,
                None,
            )),
            2 => {
                let (x, y) = (self.required("x", 31)?, self.required("y", 1023)?);
                Ok((
                    (x & 7) as u8,
                    (y & 15) as u16,
                    Some(((x >> 3) as u8, (y >> 4) as u8)),
                ))
            }
            _ => Err(invalid(self.path("method"), "must be 1 or 2")),
        }
    }

    /// `fg_position` and `bg_position`, or `relative`: the offset in rows
    /// of layer 1 from the player, whose low four bits the two positions
    /// hold, `ff` above `bb` (or, `bb_above`, `bb` above `ff`). Returns
    /// the positions and `F` for a relative camera.
    fn camera(&self, bb_above: bool) -> Result<(u8, u8, Option<bool>), SourceError> {
        match self.int("relative", -16, 15)? {
            Some(rows) => {
                if (self.get)("fg_position").is_some() || (self.get)("bg_position").is_some() {
                    return Err(invalid(
                        self.path("relative"),
                        "replaces `fg_position` and `bg_position`",
                    ));
                }
                let (high, low) = offset_bits(rows as i8);
                let (upper, lower) = (low >> 2, low & 3);
                Ok(if bb_above {
                    (lower, upper, Some(high))
                } else {
                    (upper, lower, Some(high))
                })
            }
            None => Ok((
                self.required("fg_position", 3)? as u8,
                self.required("bg_position", 3)? as u8,
                None,
            )),
        }
    }
}

/// The `[entrance]` table and the `[midway]` one, if the level has it.
fn read_entrance(
    t: &Table,
    midway: Option<&Table>,
) -> Result<(SecondaryHeader, LevelSettings), SourceError> {
    check_keys(
        t,
        "entrance",
        &[
            "screen",
            "x",
            "y",
            "method",
            "action",
            "midway_screen",
            "fg_position",
            "bg_position",
            "relative",
            "layer2_scroll",
            "layer2_vertical_scroll",
            "layer3",
            "no_yoshi_intro",
            "vertical_position",
            "unknown",
            "slippery",
            "water",
            "face_left",
            "smart_spawn",
            "spawn_range",
            "auto_screens",
            "bg_height",
            "bg_offset",
        ],
    )?;
    let get = |key: &str| t.get(key).and_then(Item::as_value);
    let f = Fields {
        at: "entrance",
        get: &get,
    };
    let (x, y, tile_position) = f.position()?;
    let (fg_position, bg_position, relative) = f.camera(false)?;
    let midway_screen = f.required("midway_screen", 31)? as u8;
    let layer2_scroll = f.required("layer2_scroll", 31)? as u8;
    let layer2_vertical_scroll = f.uint("layer2_vertical_scroll", 31)?.map(|v| v as u8);
    if layer2_scroll > 15 && layer2_vertical_scroll.is_none() {
        return Err(invalid(
            "entrance.layer2_scroll",
            "is past 15, which only a separate horizontal setting (with `layer2_vertical_scroll`) can be",
        ));
    }
    let header = SecondaryHeader {
        layer2_scroll: layer2_scroll & 0x0F,
        entrance_y: y as u8,
        layer3: f.required("layer3", 3)? as u8,
        entrance_action: f.required("action", 7)? as u8,
        entrance_x: x,
        midway_screen: midway_screen & 0x0F,
        fg_position,
        bg_position,
        no_yoshi_intro: f.flag("no_yoshi_intro")?,
        unknown: f.flag("unknown")?,
        vertical_position: f.flag("vertical_position")?,
        entrance_screen: f.required("screen", 31)? as u8,
    };
    let background = match (t.get("bg_height"), t.get("bg_offset")) {
        (Some(_), Some(_)) => {
            return Err(invalid("entrance.bg_offset", "replaces `bg_height`"));
        }
        (None, Some(item)) if item.as_str() == Some("absolute") => Background::Absolute,
        (None, Some(_)) => Background::Offset(f.int("bg_offset", -15, 15)?.unwrap_or(0) as i8),
        (_, None) => match f.int("bg_height", 1, 32)? {
            Some(rows) => Background::Height(rows as u8),
            None => Background::Height(27),
        },
    };
    let settings = LevelSettings {
        slippery: f.flag("slippery")?,
        water: f.flag("water")?,
        tile_position,
        smart_spawn: f.flag("smart_spawn")?,
        spawn_range: f.uint("spawn_range", 3)?.unwrap_or(0) as u8,
        layer2_vertical_scroll,
        layer2_horizontal_high: layer2_scroll > 15,
        auto_screens: match t.get("auto_screens") {
            None => true,
            Some(_) => f.flag("auto_screens")?,
        },
        relative,
        face_left: f.flag("face_left")?,
        background,
        midway: crate::entrance::Midway {
            screen_high: midway_screen & 0x10 != 0,
            separate: midway.map(read_midway).transpose()?,
        },
    };
    Ok((header, settings))
}

fn read_midway(t: &Table) -> Result<SeparateMidway, SourceError> {
    let get = |key: &str| t.get(key).and_then(Item::as_value);
    let f = Fields {
        at: "midway",
        get: &get,
    };
    if t.contains_key("redirect") {
        check_keys(t, "midway", &["redirect"])?;
        return Ok(SeparateMidway::Redirect(
            f.required("redirect", 0x1FF)? as u16
        ));
    }
    check_keys(
        t,
        "midway",
        &[
            "x",
            "y",
            "action",
            "fg_position",
            "bg_position",
            "relative",
            "slippery",
            "water",
            "face_left",
        ],
    )?;
    let (fg_position, bg_position, relative) = f.camera(false)?;
    Ok(SeparateMidway::Entrance(MidwayEntrance {
        slippery: f.flag("slippery")?,
        water: f.flag("water")?,
        action: f.required("action", 7)? as u8,
        x: f.required("x", 31)? as u8,
        y: f.required("y", 1023)? as u16,
        fg_position,
        bg_position,
        relative,
        face_left: f.flag("face_left")?,
    }))
}

pub(super) fn read_list<T>(
    t: &Table,
    key: &str,
    name: &str,
    comments: &mut Comments,
    read: impl Fn(&InlineTable, &str) -> Result<T, SourceError>,
) -> Result<Vec<T>, SourceError> {
    let at = format!("{name}.{key}");
    let array: &Array = t
        .get(key)
        .ok_or_else(|| invalid(&at, "is missing"))?
        .as_array()
        .ok_or_else(|| invalid(&at, "must be an array"))?;
    let mut items = Vec::with_capacity(array.len());
    for (i, value) in array.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let prefix = value
            .decor()
            .prefix()
            .and_then(|p| p.as_str())
            .unwrap_or("");
        // What follows `[` or the previous entry on its line is Kobo's.
        comments.add(at.clone(), own_line_comments(prefix, true));
        let entry = value
            .as_inline_table()
            .ok_or_else(|| invalid(&at, "must be an inline table"))?;
        items.push(read(entry, &at)?);
    }
    // Comments before the closing bracket stay there.
    let trailing = own_line_comments(array.trailing().as_str().unwrap_or(""), true);
    comments.add(format!("{at}[]"), trailing);
    Ok(items)
}

/// An inline table's integer, if present.
pub(super) fn opt(
    t: &InlineTable,
    at: &str,
    key: &str,
    max: u32,
) -> Result<Option<u32>, SourceError> {
    t.get(key)
        .map(|v| int_value(v, &format!("{at}.{key}"), max))
        .transpose()
}

pub(super) fn req(t: &InlineTable, at: &str, key: &str, max: u32) -> Result<u32, SourceError> {
    opt(t, at, key, max)?.ok_or_else(|| invalid(format!("{at}.{key}"), "is missing"))
}

pub(super) fn inline_flag(t: &InlineTable, at: &str, key: &str) -> Result<bool, SourceError> {
    match t.get(key) {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| invalid(format!("{at}.{key}"), "must be true or false")),
    }
}

fn bytes(t: &InlineTable, at: &str, key: &str) -> Result<Vec<u8>, SourceError> {
    let at = format!("{at}.{key}");
    let text = t
        .get(key)
        .ok_or_else(|| invalid(&at, "is missing"))?
        .as_str()
        .ok_or_else(|| invalid(&at, "must be a string of hex bytes"))?;
    parse_hex_bytes(&at, text)
}

pub(super) fn keys_of(t: &InlineTable, at: &str, allowed: &[&str]) -> Result<(), SourceError> {
    check_keys(t.iter(), at, allowed)
}

fn read_object(t: &InlineTable, at: &str) -> Result<Object, SourceError> {
    let kinds: Vec<&str> = ["obj", "ext", "exit", "lm", "raw"]
        .into_iter()
        .filter(|k| t.contains_key(k))
        .collect();
    let [kind] = kinds[..] else {
        return Err(invalid(
            at,
            "needs exactly one of `obj`, `ext`, `exit`, `lm`, `raw`",
        ));
    };
    let pos = |t: &InlineTable| -> Result<(u16, u16), SourceError> {
        Ok((
            req(t, at, "x", 0xFFFF)? as u16,
            req(t, at, "y", 0xFFFF)? as u16,
        ))
    };
    Ok(match kind {
        "obj" => {
            let number = req(t, at, "obj", 0x3F)? as u8;
            let (x, y) = pos(t)?;
            let size = |key| {
                opt(t, at, key, 16)?
                    .filter(|&n| n > 0)
                    .map(|n| n as u8 - 1)
                    .ok_or_else(|| invalid(format!("{at}.{key}"), "must be 1 to 16"))
            };
            let nibble = |key| opt(t, at, key, 15).map(|n| n.unwrap_or(0) as u8);
            let layout = Settings::of(number);
            let allowed: &[&str] = match layout {
                Settings::HeightWidth => &["height", "width"],
                Settings::HeightType => &["height", "type"],
                Settings::TypeWidth => &["type", "width"],
                Settings::Height => &["height", "unused"],
                Settings::Width => &["unused", "width"],
                Settings::Length => &["length"],
                Settings::Raw => &["settings"],
            };
            let mut all = vec!["obj", "x", "y"];
            all.extend_from_slice(allowed);
            keys_of(t, at, &all)?;
            let settings = match layout {
                Settings::HeightWidth => size("height")? << 4 | size("width")?,
                Settings::HeightType => size("height")? << 4 | nibble("type")?,
                Settings::TypeWidth => nibble("type")? << 4 | size("width")?,
                Settings::Height => size("height")? << 4 | nibble("unused")?,
                Settings::Width => nibble("unused")? << 4 | size("width")?,
                Settings::Length => {
                    let length = req(t, at, "length", 256)?;
                    if length == 0 {
                        return Err(invalid(format!("{at}.length"), "must be 1 to 256"));
                    }
                    (length - 1) as u8
                }
                Settings::Raw => req(t, at, "settings", 0xFF)? as u8,
            };
            Object::Standard {
                number,
                x,
                y,
                settings,
            }
        }
        "ext" => {
            keys_of(t, at, &["ext", "x", "y"])?;
            let number = req(t, at, "ext", 0xFF)? as u8;
            let (x, y) = pos(t)?;
            Object::Extended { number, x, y }
        }
        "exit" => {
            keys_of(
                t,
                at,
                &[
                    "exit",
                    "dest",
                    "midway",
                    "water",
                    "lm_format",
                    "secondary",
                    "high",
                ],
            )?;
            let secondary = inline_flag(t, at, "secondary")?;
            let (w, wrong) = if secondary {
                ("water", "midway")
            } else {
                ("midway", "water")
            };
            if t.contains_key(wrong) {
                return Err(invalid(
                    at,
                    format!(
                        "`{wrong}` needs `secondary` {}",
                        if secondary { "unset" } else { "set" }
                    ),
                ));
            }
            let flags = (inline_flag(t, at, w)? as u8) << 3 | (secondary as u8) << 1;
            let screen = req(t, at, "exit", 0x1F)? as u8;
            if inline_flag(t, at, "lm_format")? {
                // The whole destination: a level, or a secondary entrance
                // up to 1FFF, which a long exit names. A normal exit past
                // 1FF names no level, but hacks have them, and Lunar
                // Magic's code takes them as they are.
                if t.contains_key("high") {
                    return Err(invalid(
                        at,
                        "`high` is for the game's format; in Lunar Magic's, `dest` is the \
                         whole destination",
                    ));
                }
                let dest = req(t, at, "dest", 0x1FFF)?;
                Object::ScreenExit(ScreenExit::lunar_magic(screen, flags, dest as u16))
            } else {
                Object::ScreenExit(ScreenExit {
                    screen,
                    flags: flags | inline_flag(t, at, "high")? as u8,
                    destination: req(t, at, "dest", 0xFF)? as u8,
                })
            }
        }
        "lm" => {
            keys_of(t, at, &["lm", "x", "y", "data"])?;
            let number = req(t, at, "lm", 0x3F)? as u8;
            let (x, y) = pos(t)?;
            Object::Lunar {
                number,
                x,
                y,
                data: bytes(t, at, "data")?,
            }
        }
        _ => {
            keys_of(t, at, &["raw"])?;
            Object::Unplaced(bytes(t, at, "raw")?)
        }
    })
}

fn read_entrance_entry(t: &InlineTable, at: &str) -> Result<Entrance, SourceError> {
    // Up to 1FFF, past the game's 512 in tables Lunar Magic moves.
    let id = req(t, at, "id", 0x1FFF)? as u16;
    if t.contains_key("overworld") {
        keys_of(t, at, &["id", "overworld"])?;
        let raw = bytes(t, at, "overworld")?;
        let bytes: [u8; 3] = raw
            .try_into()
            .map_err(|_| invalid(format!("{at}.overworld"), "must be three bytes"))?;
        let settings = EntranceSettings {
            overworld: Some(bytes),
            ..EntranceSettings::default()
        };
        return Ok(Entrance::from_bytes(id, bytes, settings));
    }
    keys_of(
        t,
        at,
        &[
            "id",
            "screen",
            "x",
            "y",
            "method",
            "action",
            "fg_position",
            "bg_position",
            "relative",
            "slippery",
            "water",
            "face_left",
        ],
    )?;
    let get = |key: &str| t.get(key);
    let f = Fields { at, get: &get };
    let (x, y, tile_position) = f.position()?;
    let (fg_position, bg_position, relative) = f.camera(true)?;
    Ok(Entrance {
        id,
        screen: req(t, at, "screen", 0x1F)? as u8,
        x,
        y: y as u8,
        action: req(t, at, "action", 7)? as u8,
        fg_position,
        bg_position,
        settings: EntranceSettings {
            slippery: f.flag("slippery")?,
            tile_position,
            relative,
            face_left: f.flag("face_left")?,
            water: f.flag("water")?,
            overworld: None,
        },
    })
}

fn read_sprite(t: &InlineTable, at: &str) -> Result<Sprite, SourceError> {
    keys_of(t, at, &["id", "x", "y", "extra", "data"])?;
    Ok(Sprite {
        id: req(t, at, "id", 0xFF)? as u8,
        x: req(t, at, "x", 0xFFFF)? as u16,
        y: req(t, at, "y", 0xFFFF)? as u16,
        extra_bits: opt(t, at, "extra", 3)?.unwrap_or(0) as u8,
        extension: if t.contains_key("data") {
            bytes(t, at, "data")?
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
# Yoshi's Island 2, shortened.

[header]
# Two screens, for the test.
screens = 2
mode = 0x00  # Horizontal, background
tileset = 0x7  # Normal 2
sprite_tileset = 0x8  # Banzai Bill
music = 0  # Overworld
time = 2
bg_palette = 1
fg_palette = 0
sprite_palette = 0
back_area = 2
item_memory = 0
vertical_scroll = 2
layer3_priority = false

[entrance]
screen = 0
x = 0
y = 11
action = 0
midway_screen = 9
fg_position = 2
bg_position = 2
layer2_scroll = 5
layer3 = 0
no_yoshi_intro = false
vertical_position = false

# The objects follow.
[layer1]
# In drawing order.
objects = [
    { obj = 0x21, x = 0, y = 24, length = 192 },  # Long ground ledge
    # The first pipe.
    { obj = 0x0F, x = 113, y = 21, height = 3, type = 0 },  # Vertical pipe
    { ext = 0x41, x = 17, y = 16 },  # Dragon coin
    { exit = 1, dest = 0x13, water = true, secondary = true },  # Screen exit
    { lm = 0x22, x = 1, y = 2, data = \"11 55\" },  # Direct Map16, page 0
    { raw = \"45 60 21\" },  # Music bypass
    # Last.
]

[layer2]
background = 0x0CD900

[sprites]
memory = 0
# No buoyancy.
list = [
    { id = 0x0F, x = 20, y = 10 },  # Goomba
    { id = 0x35, x = 21, y = 10, extra = 2, data = \"01 02\" },  # Yoshi
]

# The end.
";

    #[test]
    fn a_file_reads_and_writes_back_the_same() {
        let (level, comments) = Level::from_toml(TEXT).unwrap();
        assert_eq!(level.to_toml(&comments), TEXT);
        assert_eq!(
            comments.before(Comments::TOP),
            ["# Yoshi's Island 2, shortened."]
        );
        assert_eq!(
            comments.before("header.screens"),
            ["# Two screens, for the test."]
        );
        assert_eq!(comments.before("layer1"), ["# The objects follow."]);
        assert_eq!(comments.before("layer1.objects"), ["# In drawing order."]);
        assert_eq!(comments.before("layer1.objects[1]"), ["# The first pipe."]);
        assert_eq!(comments.before("layer1.objects[]"), ["# Last."]);
        assert_eq!(comments.before("sprites.list"), ["# No buoyancy."]);
        assert_eq!(comments.before(Comments::END), ["# The end."]);
        assert_eq!(
            level.layer1[0],
            Object::Standard {
                number: 0x21,
                x: 0,
                y: 24,
                settings: 191
            }
        );
        assert_eq!(
            level.layer1[3],
            Object::ScreenExit(ScreenExit {
                screen: 1,
                flags: 0b1010,
                destination: 0x13
            })
        );
        assert_eq!(level.sprites.list[1].extension, [1, 2]);
        assert_eq!(
            level.layer2,
            Layer2::VanillaBackground(SnesAddr::new(0x0CD900))
        );
    }

    #[test]
    fn kobo_comments_are_its_own() {
        let text = TEXT
            .replace("  # Dragon coin", "  # a stale name")
            .replace("  # Horizontal, background", "  # a stale name")
            .replace("[entrance]", "[entrance]  # a stale name");
        assert_ne!(text, TEXT);
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.to_toml(&comments), TEXT);
    }

    #[test]
    fn comments_anywhere_survive_formatting() {
        // Loosely formatted, with a comment before every kind of place.
        let text = "\
# top
[header] # same line: Kobo's
  screens=2
# before mode
mode=0
tileset = 7
sprite_tileset = 8
music = 0
time = 2
bg_palette = 1
fg_palette = 0
sprite_palette = 0
back_area = 2
item_memory = 0
vertical_scroll = 2
layer3_priority = false
# after the header's last key
[entrance]
screen = 0
x = 0
y = 11
action = 0
midway_screen = 9
fg_position = 2
bg_position = 2
layer2_scroll = 5
layer3 = 0
no_yoshi_intro = false
vertical_position = false
# before an unset flag
[layer1]
# before the list
objects = [ # same line: Kobo's
  # first entry
  { obj = 0x21, x = 0, y = 24, length = 192 }, # same line: Kobo's
  # closing
]
[sprites]
# before memory
memory = 0
# before buoyancy, which is unset
list = []
# trailing
";
        let (level, comments) = Level::from_toml(text).unwrap();
        let formatted = level.to_toml(&comments);
        let expected = "\
# top

[header]
screens = 2
# before mode
mode = 0x00  # Horizontal, background
tileset = 0x7  # Normal 2
sprite_tileset = 0x8  # Banzai Bill
music = 0  # Overworld
time = 2
bg_palette = 1
fg_palette = 0
sprite_palette = 0
back_area = 2
item_memory = 0
vertical_scroll = 2
layer3_priority = false

# after the header's last key
[entrance]
screen = 0
x = 0
y = 11
action = 0
midway_screen = 9
fg_position = 2
bg_position = 2
layer2_scroll = 5
layer3 = 0
no_yoshi_intro = false
vertical_position = false

# before an unset flag
[layer1]
# before the list
objects = [
    # first entry
    { obj = 0x21, x = 0, y = 24, length = 192 },  # Long ground ledge
    # closing
]

[sprites]
# before memory
memory = 0
# before buoyancy, which is unset
list = [
]

# trailing
";
        assert_eq!(formatted, expected);
        // Kobo's formatting is a fixed point.
        let (again, comments) = Level::from_toml(&formatted).unwrap();
        assert_eq!(again, level);
        assert_eq!(again.to_toml(&comments), formatted);
    }

    #[test]
    fn backgrounds_round_trip() {
        let (mut level, _) = Level::from_toml(TEXT).unwrap();
        // Three-digit tiles, 27 rows; four-digit, 32 rows in table 5.
        for (table, rows, top) in [(0, 27, 0x0FF), (5, 32, 0x1FFF)] {
            let mut tiles = vec![0; BACKGROUND_ROWS * 32];
            for (i, tile) in tiles.iter_mut().take(rows * 32).enumerate() {
                *tile = (i as u16 * 7) % top;
            }
            level.layer2 = Layer2::Background(BackgroundTiles { table, rows, tiles });
            let text = level.to_toml(&Comments::default());
            let (back, comments) = Level::from_toml(&text).unwrap();
            assert_eq!(back, level);
            assert_eq!(back.to_toml(&comments), text);
            // Other layer 2 keys beside the tiles are refused.
            for extra in ["objects = []", "background = 0x0CD90"] {
                let bad = text.replace("[layer2]\n", &format!("[layer2]\n{extra}\n"));
                assert!(Level::from_toml(&bad).is_err(), "{extra}");
            }
        }
    }

    #[test]
    fn graphics_lists_round_trip() {
        let (mut level, _) = Level::from_toml(TEXT).unwrap();
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::AN2] = 0x8000 | 0x2000 | 0x1000 | 0x0100;
        list.0[slot::LT3] = 0x6123;
        list.0[slot::FG1] = 0xABC;
        list.0[slot::SP1] = 0x3FFF;
        list.0[slot::LG4] |= 0x5000;
        level.graphics = Some(list);
        let text = level.to_toml(&Comments::default());
        assert!(
            text.contains("\n[graphics]\nbypass = true\nlayer3_tilemap = true\n"),
            "{text}"
        );
        assert!(
            text.contains("tilemap_size = 0x0800\ntilemap_vram = 0x5000\nsp1 = 0xFFF\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "\n[graphics.layer3]\nadvanced = true\nhorizontal = 0x00  # None\n\
                 vertical = 0x00  # None\nx = 16\ny = 0\nsprites_air = true\nunknown = true\n"
            ),
            "{text}"
        );
        assert!(text.contains("sp4 = 0xFFFF\n"), "{text}");
        let (back, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(back, level);
        assert_eq!(back.to_toml(&comments), text);
        // A slot a setting shares a word with holds a file, 0xFFF at most.
        let bad = text.replace("sp1 = 0xFFF\n", "sp1 = 0x1FFF\n");
        assert!(Level::from_toml(&bad).is_err());
        // LT3's nibble as its size and place: every value round-trips, size
        // 3 as itself; other sizes and places are refused, as is a
        // setting where LT3 is 0xFFFF.
        for nibble in 0..16u16 {
            let mut level = level.clone();
            level.graphics.as_mut().unwrap().0[slot::LT3] = nibble << 12 | 0x123;
            let text = level.to_toml(&Comments::default());
            assert_eq!(
                text.contains("tilemap_size = 3\n"),
                nibble & 3 == 3,
                "{text}"
            );
            assert_eq!(Level::from_toml(&text).unwrap().0, level);
        }
        for (from, to) in [
            ("tilemap_size = 0x0800\n", "tilemap_size = 0x0400\n"),
            ("tilemap_vram = 0x5000\n", "tilemap_vram = 0x5100\n"),
            ("lt3 = 0x123\n", "lt3 = 0xFFFF\n"),
        ] {
            assert!(text.contains(from), "{text}");
            assert!(Level::from_toml(&text.replace(from, to)).is_err(), "{to}");
        }
        let (head, table) = text.split_once("[graphics.layer3]\n").unwrap();
        for bad in ["x = 3\n", "y = 1024\n", "horizontal = 0x20  # None\n"] {
            let key = bad.split(' ').next().unwrap();
            let line = table
                .lines()
                .find(|l| l.starts_with(&format!("{key} ")))
                .unwrap();
            let bad = format!(
                "{head}[graphics.layer3]\n{}",
                table.replacen(&format!("{line}\n"), bad, 1)
            );
            assert!(Level::from_toml(&bad).is_err(), "{bad}");
        }
        // An empty slot's setting is F: settings that need another there
        // are refused.
        let mut list = GraphicsList::DEFAULT;
        list.0[slot::SP2] = exgfx::EMPTY;
        list.0[slot::LG4] |= 0x1000;
        level.graphics = Some(list);
        let text = level.to_toml(&Comments::default());
        assert!(text.contains("horizontal = 0x10"), "{text}");
        let (back, _) = Level::from_toml(&text).unwrap();
        assert_eq!(back, level);
        let bad = text.replace("horizontal = 0x10", "horizontal = 0x00");
        assert!(Level::from_toml(&bad).is_err());
    }

    #[test]
    fn palettes_round_trip() {
        let (mut level, _) = Level::from_toml(TEXT).unwrap();
        let mut palette = Palette::default();
        for (i, c) in palette.colors.iter_mut().enumerate() {
            *c = Color15::from_rgb5(i as u8 % 32, (i / 32) as u8, 31 - i as u8 % 32);
        }
        level.palette = Some(CustomPalette {
            back_area: Color15::from_rgb5(1, 2, 3),
            palette,
        });
        let text = level.to_toml(&Comments::default());
        assert!(text.contains("back_area = \"#081018\""), "{text}");
        let (back, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(back, level);
        assert_eq!(back.to_toml(&comments), text);
        // Channels are multiples of 8.
        let bad = text.replace("#081018", "#081019");
        assert!(Level::from_toml(&bad).is_err());
    }

    #[test]
    fn animation_round_trips() {
        use crate::exanimation::{List, Slot};
        let (mut level, _) = Level::from_toml(TEXT).unwrap();
        // Settings alone: the game's tiles off.
        level.animation_settings = Some(0x40);
        let text = level.to_toml(&Comments::default());
        assert!(
            text.contains("[animation]\ngame_tiles = false\ngame_colours = true\n"),
            "{text}"
        );
        let (back, _) = Level::from_toml(&text).unwrap();
        assert_eq!(back, level);
        // And a list.
        let mut list = List::default();
        list.slots.insert(
            1,
            Slot {
                kind: 0x13,
                trigger: 0x21,
                frames_less_one: 0,
                dest: 0x0164,
                frames: vec![0x7FFF, 0x001F],
            },
        );
        list.slots.insert(
            0,
            Slot {
                kind: 0x01,
                trigger: 0x00,
                frames_less_one: 1,
                dest: 0x6000,
                frames: vec![0xAD40, 0xAD60],
            },
        );
        list.count = 2;
        level.animation = Some(list);
        let text = level.to_toml(&Comments::default());
        assert!(
            text.contains("{ slot = 0x00, type = 0x01, vram = 0x6000, an2 = true, frames = [0x0040, 0x0060] },  # 1 8x8\n"),
            "{text}"
        );
        assert!(
            text.contains("{ slot = 0x01, type = 0x13, trigger = 0x21, colour = 0x64, colours = 2, frames = [0x7FFF], triggered = [0x001F] },  # Palette; Custom 1"),
            "{text}"
        );
        let (back, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(back, level);
        assert_eq!(back.to_toml(&comments), text);
    }

    #[test]
    fn entrance_bytes() {
        let bytes = [0xAA, 0x24, 0xDB];
        let e = Entrance::from_bytes(0x1BC, bytes, EntranceSettings::from_bytes(bytes, [0, 0]));
        assert_eq!(
            (e.y, e.fg_position, e.bg_position, e.screen, e.x, e.action),
            (10, 2, 2, 4, 1, 3)
        );
        assert!(e.settings.slippery);
        assert_eq!(e.settings.tile_position, Some((1, 0)));
        // Bit 3 of the last byte, Lunar Magic's copy of the destination's
        // bit 8, is not kept.
        assert_eq!(e.to_bytes(), ([0xAA, 0x24, 0xD3], [0, 0]));
    }

    #[test]
    fn lunar_magic_entrance_settings() {
        let text = TEXT
            .replace("x = 0\ny = 11\n", "x = 9\ny = 22\nmethod = 2\n")
            .replace("fg_position = 2\nbg_position = 2\n", "relative = -10\n")
            .replace(
                "vertical_position = false\n",
                "vertical_position = false\nslippery = true\nbg_offset = \"absolute\"\n\n\
                 [midway]\nx = 17\ny = 22\naction = 0\nfg_position = 1\nbg_position = 2\n",
            );
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.settings.tile_position, Some((1, 1)));
        assert_eq!(level.settings.relative, Some(true));
        assert_eq!(
            (level.entrance.fg_position, level.entrance.bg_position),
            (1, 2)
        );
        assert_eq!(level.settings.background, Background::Absolute);
        let Some(SeparateMidway::Entrance(midway)) = level.settings.midway.separate else {
            panic!("{:?}", level.settings.midway)
        };
        assert_eq!((midway.x, midway.y), (17, 22));
        assert_eq!(level.to_toml(&comments), text);
    }

    #[test]
    fn separate_layer2_scroll_settings() {
        // With layer2_vertical_scroll, layer2_scroll is the horizontal
        // setting, past 15 for H.
        let text = TEXT.replace(
            "layer2_scroll = 5\n",
            "layer2_scroll = 18\nlayer2_vertical_scroll = 22\n",
        );
        let (level, comments) = Level::from_toml(&text).unwrap();
        assert_eq!(level.entrance.layer2_scroll, 2);
        assert_eq!(level.settings.layer2_vertical_scroll, Some(22));
        assert!(level.settings.layer2_horizontal_high);
        assert_eq!(level.to_toml(&comments), text);
        let alone = TEXT.replace("layer2_scroll = 5\n", "layer2_scroll = 18\n");
        let error = Level::from_toml(&alone).unwrap_err().to_string();
        assert!(error.contains("layer2_vertical_scroll"), "{error}");
    }

    #[test]
    fn mistakes_are_refused() {
        let bad = |from: &str, to: &str| {
            let text = TEXT.replace(from, to);
            assert_ne!(text, TEXT, "{from}");
            Level::from_toml(&text).unwrap_err().to_string()
        };
        assert!(bad("height = 3, type = 0", "width = 3, type = 0").contains("unknown key `width`"));
        assert!(bad("length = 192", "length = 0").contains("1 to 256"));
        assert!(bad("water = true, secondary", "midway = true, secondary").contains("midway"));
        assert!(bad("{ obj = 0x21,", "{ obj = 0x21, ext = 1,").contains("exactly one"));
        assert!(bad("screens = 2", "screens = 33").contains("out of range"));
        assert!(bad("data = \"11 55\"", "data = \"1 55\"").contains("hex byte"));
        assert!(bad("[sprites]", "[sprite]").contains("unknown key"));
    }

    #[test]
    fn exits_in_lunar_magic_format_name_their_whole_destination() {
        let exit = |entry: &str| {
            let text = TEXT.replace(
                "{ exit = 1, dest = 0x13, water = true, secondary = true }",
                entry,
            );
            Level::from_toml(&text).map(|(level, _)| {
                let back = level.to_toml(&Comments::default());
                let exit = level.layer1.iter().find_map(|o| match o {
                    Object::ScreenExit(e) => Some(*e),
                    _ => None,
                });
                (exit.unwrap(), back.contains(entry))
            })
        };
        // A long exit to entrance 320, as riff2's level 040 has it.
        let (long, same) =
            exit("{ exit = 3, dest = 0x320, lm_format = true, secondary = true }").unwrap();
        assert!(same);
        assert_eq!((long.flags, long.destination), (0x17, 0x20));
        let (short, same) = exit("{ exit = 3, dest = 0x105, lm_format = true }").unwrap();
        assert!(same);
        assert_eq!((short.flags, short.destination), (0x05, 0x05));
        // The game's format keeps its low byte and flag bit.
        let (game, same) = exit("{ exit = 3, dest = 0x05, high = true }").unwrap();
        assert!(same);
        assert_eq!((game.flags, game.destination), (0x01, 0x05));
        // Thirteen bits at most, as a long exit holds them, a normal exit's
        // too (QLDC 2022 `05_Bumpty` has some past 1FF); `high` is the
        // game's.
        let (normal, same) = exit("{ exit = 3, dest = 0x601, lm_format = true }").unwrap();
        assert!(same);
        assert_eq!((normal.flags, normal.destination), (0x34, 0x01));
        let refused = |entry: &str| exit(entry).unwrap_err().to_string();
        assert!(
            refused("{ exit = 3, dest = 0x2000, lm_format = true, secondary = true }")
                .contains("out of range")
        );
        assert!(
            refused("{ exit = 3, dest = 0x05, lm_format = true, high = true }").contains("`high`")
        );
        assert!(refused("{ exit = 3, dest = 0x105 }").contains("out of range"));
    }
}
