//! The overworld file: what a project changes of the clean ROM's overworld
//! ([`crate::overworld::Changes`]), named by the manifest's `[overworld]
//! file`. A row, place, name, or event the file leaves out is the clean
//! ROM's.
//!
//! ```toml
//! [layer1.main]                  # a row of 32 16x16 tiles, three hex digits each
//! 0x05 = "000 000 058 010 ..."
//!
//! [layer2.submaps]               # a row of 64 8x8 tiles, the properties then the number
//! 0x10 = "1C58 1C58 ..."
//!
//! [levels]                       # a place's translevel and direction byte
//! "main 5,10" = [0x13, 0xC0]
//!
//! [names]                        # 19 tiles at most; \xNN for a tile that is not a letter
//! 0x13 = "DONUT PLAINS 2"
//!
//! [level_events]                 # a translevel's event, which passing it makes
//! 0x13 = 0x0A
//!
//! [event_tiles]                  # where the event tile data's 2x2 blocks start
//! split = 0x0900
//!
//! [border]                       # a row of the border on layer 3: 32 tilemap words, ----
//! 0x00 = "38FE 38FE ---- ..."    # where its stripe image writes nothing; [title] likewise,
//!                                # the title screen's layer 3, which Lunar Magic edits there
//!
//! [tiles]                        # a layer 1 16x16 tile's four 8x8 tiles, as tilemap words:
//! 0x0C1 = "0CA0 0CB0 0CA1 0CB1"  # top left, bottom left, top right, bottom right
//!
//! [graphics]                     # a submap's graphics list (Lunar Magic's 200-206): 16 words
//! 0x01 = "8014 007F ..."        # in its slot order, AN2 first
//!
//! [palettes.0x00]                # Lunar Magic's overworld palettes, 0-6 each map's, 7-13 each
//! 0x0 = "#000000 #F8F8F8 ..."    # once Special World is passed: rows of 16 colours
//!
//! [animation.0x01]               # a submap's ExAnimation: its settings and list, as a
//! game_tiles = false             # level's [animation] (source::animation), the level
//! slots = [ ... ]                # list's setting `submap_list`; and [animation.global].
//!                                # Triggers 01-08 are events (manual frames 8-F name them),
//!                                # each with a second set.
//!
//! [options]                      # Lunar Magic's: the event path fade off, a speed to reveal at,
//! reveal_speed = 0x06            # and FG1-2 merged into SP3-4 (the list then has FG1-2's
//! merge_fg = true                # files in SP3-4's slots); and its Extra Options that are
//! life_exchange = false          # bytes of the game's code (overworld::GAME_OPTIONS), all on
//!                                # in the game
//!
//! [tables]                       # the tables kept in place, their bytes (overworld::TABLES)
//! music = "02 03 04 06 07 09 05"
//!
//! [crush]                        # events, places, VRAM: all 24 when any changes
//! list = [[0x06, 0x0419, 0x2052], ...]
//!
//! [reveal]                       # the layer 1 tiles events turn into others
//! list = [[0x6E, 0x58], ...]
//!
//! [start]                        # where a new game puts Mario and Luigi: submap, x, y
//! mario = [0x01, 0x0068, 0x0078] # (submap 0 the main map; x and y in pixels)
//! luigi = [0x01, 0x0068, 0x0078]
//!
//! [level_flags]                  # a translevel's settings at a new game: directions
//! 0x28 = 0x03                    # (bits 0-3), save prompt (10), no entry once passed (20)
//!
//! [[event]]
//! number = 0x05
//! layer1 = [0x0123, 0x2345]      # its layer 1 tile's place and VRAM address
//! blocks = [
//!     { place = 0x1A22, tiles = "1C58 ..." },   # 36 tiles (6x6) or 4 (2x2)
//! ]
//! extras = [                     # further tiles, in the order they are made
//!     { layer1 = 0x0215, tile = 0x068 },         # a layer 1 tile set outright
//!     { place = 0x1114, tiles = "1C58 ..." },    # a layer 2 block
//! ]
//! ```
//!
//! The maps are `main` and `submaps` (the six submaps share one).

use toml_edit::{DocumentMut, Item, Table, Value};

use super::{SourceError, hex, invalid};
use crate::overworld::{self, Changes, Crush, Event, EventBlock, ExtraTile, NAME_TILES, Start};

const MAP_NAMES: [&str; 2] = ["main", "submaps"];

fn map_number(at: &str, name: &str) -> Result<u8, SourceError> {
    MAP_NAMES
        .iter()
        .position(|&n| n == name)
        .map(|n| n as u8)
        .ok_or_else(|| invalid(at, format!("{name:?} is not a map: main or submaps")))
}

/// The single character a name tile stands for, if it is one.
fn name_char(tile: u8) -> Option<char> {
    match tile {
        0x00..=0x19 => Some((b'A' + tile) as char),
        0x1C => Some('-'),
        0x1F => Some(' '),
        0x5A => Some('#'),
        0x5D => Some('\''),
        0x63..=0x6C => Some((b'0' + tile - 0x63) as char),
        _ => None,
    }
}

fn name_tile(c: char) -> Option<u8> {
    (0..=0x7F).find(|&t| name_char(t) == Some(c) && c != '\\')
}

/// A name as text: its characters, `\xNN` for a tile that is none, the
/// spaces after it left out.
pub fn name_text(tiles: &[u8; NAME_TILES]) -> String {
    let mut text: String = tiles
        .iter()
        .map(|&t| match name_char(t) {
            Some(c) => c.to_string(),
            None => format!("\\x{t:02X}"),
        })
        .collect();
    while text.ends_with(' ') {
        text.pop();
    }
    text
}

pub fn parse_name(at: &str, text: &str) -> Result<[u8; NAME_TILES], SourceError> {
    let mut tiles = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.by_ref().skip(1).take(2).collect();
            let tile = u8::from_str_radix(&code, 16)
                .map_err(|_| invalid(at, format!("\\x{code} is not a tile")))?;
            tiles.push(tile);
        } else {
            let tile = name_tile(c.to_ascii_uppercase()).ok_or_else(|| {
                invalid(
                    at,
                    format!("{c:?} is no tile of the names' font; write \\xNN"),
                )
            })?;
            tiles.push(tile);
        }
    }
    if tiles.len() > NAME_TILES {
        return Err(invalid(
            at,
            format!("{} tiles, where a name has {NAME_TILES}", tiles.len()),
        ));
    }
    let mut name = [0x1F; NAME_TILES];
    name[..tiles.len()].copy_from_slice(&tiles);
    Ok(name)
}

fn words(values: &[u16], digits: usize) -> String {
    let parts: Vec<String> = values.iter().map(|v| format!("{v:0digits$X}")).collect();
    parts.join(" ")
}

fn parse_words(at: &str, text: &str, count: usize, most: u16) -> Result<Vec<u16>, SourceError> {
    let values: Vec<u16> = text
        .split_whitespace()
        .map(|w| {
            u16::from_str_radix(w, 16)
                .ok()
                .filter(|&v| v <= most)
                .ok_or_else(|| invalid(at, format!("{w:?} is not a tile")))
        })
        .collect::<Result<_, _>>()?;
    if values.len() != count {
        return Err(invalid(
            at,
            format!("{} tiles, where a row has {count}", values.len()),
        ));
    }
    Ok(values)
}

/// The changes in Kobo's format, with `top`, the comments before them.
pub fn to_toml(changes: &Changes, top: &[String]) -> String {
    let mut out = String::new();
    for line in top {
        out += &format!("{line}\n");
    }
    if !top.is_empty() {
        out.push('\n');
    }
    let mut sections: Vec<String> = Vec::new();
    for (layer, rows, digits) in [
        ("layer1", &changes.layer1, 3),
        ("layer2", &changes.layer2, 4),
    ] {
        for (map, name) in MAP_NAMES.iter().enumerate() {
            let mine: Vec<_> = rows.range((map as u8, 0)..=(map as u8, u8::MAX)).collect();
            if mine.is_empty() {
                continue;
            }
            let mut text = format!("[{layer}.{name}]\n");
            for (&(_, row), tiles) in mine {
                text += &format!(
                    "{} = \"{}\"\n",
                    hex(u32::from(row), 2),
                    words(tiles, digits)
                );
            }
            sections.push(text);
        }
    }
    if !changes.levels.is_empty() {
        let mut text = String::from("[levels]\n");
        for (&(map, x, y), &(t, d)) in &changes.levels {
            text += &format!(
                "\"{} {x},{y}\" = [{}, {}]\n",
                MAP_NAMES[usize::from(map)],
                hex(u32::from(t), 2),
                hex(u32::from(d), 2)
            );
        }
        sections.push(text);
    }
    if !changes.names.is_empty() {
        let mut text = String::from("[names]\n");
        for (&t, name) in &changes.names {
            let quoted = toml_edit::Value::from(name_text(name)).to_string();
            text += &format!("{} = {}\n", hex(u32::from(t), 2), quoted.trim());
        }
        sections.push(text);
    }
    for (name, rows) in [("border", &changes.border), ("title", &changes.title)] {
        if rows.is_empty() {
            continue;
        }
        let mut text = format!("[{name}]\n");
        for (&r, row) in rows {
            let cells: Vec<String> = row
                .iter()
                .map(|c| c.map_or("----".to_string(), |w| format!("{w:04X}")))
                .collect();
            text += &format!("{} = \"{}\"\n", hex(u32::from(r), 2), cells.join(" "));
        }
        sections.push(text);
    }
    if !changes.tiles.is_empty() {
        let mut text = String::from("[tiles]\n");
        for (&n, tile) in &changes.tiles {
            text += &format!("{} = \"{}\"\n", hex(u32::from(n), 3), words(tile, 4));
        }
        sections.push(text);
    }
    if !changes.graphics.is_empty() {
        let mut text = String::from("[graphics]\n");
        for (&submap, list) in &changes.graphics {
            let words: Vec<String> = list.0.iter().map(|w| format!("{w:04X}")).collect();
            text += &format!("{} = \"{}\"\n", hex(u32::from(submap), 2), words.join(" "));
        }
        sections.push(text);
    }
    if let Some(palettes) = &changes.palettes {
        for (n, palette) in palettes.chunks(0x200).enumerate() {
            let mut text = format!("[palettes.{}]\n", hex(n as u32, 2));
            for (row, colours) in palette.chunks(32).enumerate() {
                let line: Vec<String> = colours
                    .chunks(2)
                    .map(|c| {
                        crate::source::level::color_text(crate::palette::Color15(
                            u16::from_le_bytes([c[0], c[1]]),
                        ))
                    })
                    .collect();
                text += &format!("{} = \"{}\"\n", hex(row as u32, 1), line.join(" "));
            }
            sections.push(text);
        }
    }
    if let Some(animation) = &changes.animation {
        let comments = super::Comments::default();
        let mut out = super::Writer::new(&comments);
        for (n, (byte, list)) in animation
            .settings
            .iter()
            .zip(&animation.submaps)
            .enumerate()
        {
            if *byte != 0 || list.is_some() {
                let table = format!("animation.{}", hex(n as u32, 2));
                let settings = (*byte != 0).then_some(*byte);
                super::animation::write_as(
                    &mut out,
                    &table,
                    "submap_list",
                    true,
                    settings,
                    list.as_ref(),
                );
            }
        }
        if let Some(list) = &animation.global {
            super::animation::write_as(&mut out, "animation.global", "", true, None, Some(list));
        }
        let text = out.finish();
        // The install alone, with no list or setting: the table empty.
        sections.push(if text.is_empty() {
            "[animation]\n".to_string()
        } else {
            text
        });
    }
    if changes.reveal_speed.is_some() || changes.merge_fg || !changes.options.is_empty() {
        let mut text = String::from("[options]\n");
        if let Some(speed) = changes.reveal_speed {
            text += &format!("reveal_speed = {}\n", hex(u32::from(speed), 2));
        }
        if changes.merge_fg {
            text += "merge_fg = true\n";
        }
        // In GAME_OPTIONS's order.
        for option in &overworld::GAME_OPTIONS {
            if let Some(on) = changes.options.get(option.name) {
                text += &format!("{} = {on}\n", option.name);
            }
        }
        sections.push(text);
    }
    if !changes.tables.is_empty() {
        let mut text = String::from("[tables]\n");
        for (name, bytes) in &changes.tables {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            text += &format!("{name} = \"{}\"\n", hex.join(" "));
        }
        sections.push(text);
    }
    if let Some(split) = changes.event_split {
        sections.push(format!(
            "[event_tiles]\nsplit = {}\n",
            hex(u32::from(split), 4)
        ));
    }
    if !changes.level_events.is_empty() {
        let mut text = String::from("[level_events]\n");
        for (&t, &e) in &changes.level_events {
            text += &format!("{} = {}\n", hex(u32::from(t), 2), hex(u32::from(e), 2));
        }
        sections.push(text);
    }
    if let Some(crush) = &changes.crush {
        let mut text = String::from("[crush]\nlist = [\n");
        for c in crush {
            text += &format!(
                "    [{}, {}, {}],\n",
                hex(u32::from(c.event), 2),
                hex(u32::from(c.place), 4),
                hex(u32::from(c.vram), 4)
            );
        }
        sections.push(text + "]\n");
    }
    if let Some(reveal) = &changes.reveal {
        let mut text = String::from("[reveal]\nlist = [\n");
        for (from, to) in reveal {
            text += &format!(
                "    [{}, {}],\n",
                hex(u32::from(*from), 2),
                hex(u32::from(*to), 2)
            );
        }
        sections.push(text + "]\n");
    }
    if !changes.level_flags.is_empty() {
        let mut text = String::from("[level_flags]\n");
        for (&t, &f) in &changes.level_flags {
            text += &format!("{} = {}\n", hex(u32::from(t), 2), hex(u32::from(f), 2));
        }
        sections.push(text);
    }
    if changes.start.is_some() {
        let mut text = String::from("[start]\n");
        if let Some(start) = &changes.start {
            for (who, p) in ["mario", "luigi"].iter().zip(start) {
                text += &format!(
                    "{who} = [{}, {}, {}]\n",
                    hex(u32::from(p.submap), 2),
                    hex(u32::from(p.x), 4),
                    hex(u32::from(p.y), 4)
                );
            }
        }
        sections.push(text);
    }
    for (&n, event) in &changes.events {
        let mut text = format!(
            "[[event]]\nnumber = {}\nlayer1 = [{}, {}]\n",
            hex(u32::from(n), 2),
            hex(u32::from(event.layer1.0), 4),
            hex(u32::from(event.layer1.1), 4)
        );
        if event.blocks.is_empty() {
            text += "blocks = []\n";
        } else {
            text += "blocks = [\n";
            for block in &event.blocks {
                text += &format!(
                    "    {{ place = {}, tiles = \"{}\" }},\n",
                    hex(u32::from(block.place), 4),
                    words(&block.tiles, 4)
                );
            }
            text += "]\n";
        }
        if !event.extras.is_empty() {
            text += "extras = [\n";
            for extra in &event.extras {
                text += &match extra {
                    ExtraTile::Layer1 { place, tile } => format!(
                        "    {{ layer1 = {}, tile = {} }},\n",
                        hex(u32::from(*place), 4),
                        hex(u32::from(*tile), 3)
                    ),
                    ExtraTile::Layer2(block) => format!(
                        "    {{ place = {}, tiles = \"{}\" }},\n",
                        hex(u32::from(block.place), 4),
                        words(&block.tiles, 4)
                    ),
                };
            }
            text += "]\n";
        }
        sections.push(text);
    }
    out + &sections.join("\n")
}

/// A layer 2 block: `{ place, tiles }`.
fn parse_block(at: &str, inline: &toml_edit::InlineTable) -> Result<EventBlock, SourceError> {
    let place = inline
        .get("place")
        .map(|v| value_int(at, v, 0xFFFF))
        .transpose()?
        .ok_or_else(|| invalid(at, "a block has a place"))?;
    let text = inline
        .get("tiles")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(at, "a block has tiles"))?;
    let count = text.split_whitespace().count();
    if count != 36 && count != 4 {
        return Err(invalid(at, "a block has 36 tiles (6x6) or 4 (2x2)"));
    }
    Ok(EventBlock {
        place: place as u16,
        tiles: parse_words(at, text, count, 0xFFFF)?,
    })
}

fn int(at: &str, item: Option<&Item>, most: u32) -> Result<u32, SourceError> {
    item.and_then(Item::as_integer)
        .filter(|&v| (0..=i64::from(most)).contains(&v))
        .map(|v| v as u32)
        .ok_or_else(|| invalid(at, format!("must be a number up to {}", hex(most, 2))))
}

fn value_int(at: &str, value: &Value, most: u32) -> Result<u32, SourceError> {
    value
        .as_integer()
        .filter(|&v| (0..=i64::from(most)).contains(&v))
        .map(|v| v as u32)
        .ok_or_else(|| invalid(at, format!("must be a number up to {}", hex(most, 2))))
}

fn numbers(at: &str, value: &Value, count: usize, most: u32) -> Result<Vec<u32>, SourceError> {
    let list = value
        .as_array()
        .filter(|a| a.len() == count)
        .ok_or_else(|| invalid(at, format!("must be a list of {count} numbers")))?;
    list.iter().map(|v| value_int(at, v, most)).collect()
}

/// Reads an overworld file, with the comments before its first table.
pub fn from_toml(text: &str) -> Result<(Changes, Vec<String>), SourceError> {
    let doc: DocumentMut = text.parse()?;
    let mut changes = Changes::default();
    // The comments before the first table: the file's own, which are kept.
    let top: Vec<String> = text
        .lines()
        .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
        .filter(|l| l.trim_start().starts_with('#'))
        .map(|l| l.trim().to_string())
        .collect();
    for (key, item) in doc.iter() {
        match key {
            "layer1" | "layer2" => {
                let (count, most, rows) = if key == "layer1" {
                    (32, 0x1FF, &mut changes.layer1)
                } else {
                    (64, 0xFFFF, &mut changes.layer2)
                };
                let maps = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of maps"))?;
                for (map_name, map_item) in maps.iter() {
                    let at = format!("{key}.{map_name}");
                    let map = map_number(&at, map_name)?;
                    let table: &Table = map_item
                        .as_table()
                        .ok_or_else(|| invalid(&at, "must be a table of rows"))?;
                    for (row_key, row) in table.iter() {
                        let at = format!("{at}.{row_key}");
                        let row_n = row_key
                            .strip_prefix("0x")
                            .and_then(|h| u8::from_str_radix(h, 16).ok())
                            // A map has as many rows as columns.
                            .filter(|&r| usize::from(r) < count)
                            .ok_or_else(|| invalid(&at, "a row is 0x00 and up"))?;
                        let text = row
                            .as_str()
                            .ok_or_else(|| invalid(&at, "must be a row of tiles"))?;
                        rows.insert((map, row_n), parse_words(&at, text, count, most)?);
                    }
                }
            }
            "levels" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of places"))?;
                for (place, value) in table.iter() {
                    let at = format!("levels.{place}");
                    let bad = || invalid(&at, "a place is \"main 5,10\" or \"submaps 5,10\"");
                    let (map_name, xy) = place.split_once(' ').ok_or_else(bad)?;
                    let (x, y) = xy.split_once(',').ok_or_else(bad)?;
                    let (x, y): (u8, u8) = (
                        x.trim().parse().ok().filter(|&v| v < 32).ok_or_else(bad)?,
                        y.trim().parse().ok().filter(|&v| v < 32).ok_or_else(bad)?,
                    );
                    let pair = numbers(
                        &at,
                        value
                            .as_value()
                            .ok_or_else(|| invalid(&at, "must be [translevel, directions]"))?,
                        2,
                        0xFF,
                    )?;
                    changes.levels.insert(
                        (map_number(&at, map_name)?, x, y),
                        (pair[0] as u8, pair[1] as u8),
                    );
                }
            }
            "names" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of names"))?;
                for (t, value) in table.iter() {
                    let at = format!("names.{t}");
                    let t = t
                        .strip_prefix("0x")
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .filter(|&t| t < 0x60)
                        .ok_or_else(|| invalid(&at, "a translevel is 0x00 to 0x5F"))?;
                    let text = value
                        .as_str()
                        .ok_or_else(|| invalid(&at, "must be a name"))?;
                    changes.names.insert(t, parse_name(&at, text)?);
                }
            }
            "border" | "title" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of rows"))?;
                for (r, value) in table.iter() {
                    let at = format!("{key}.{r}");
                    let r = r
                        .strip_prefix("0x")
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .filter(|&r| usize::from(r) < crate::overworld::BORDER_ROWS)
                        .ok_or_else(|| invalid(&at, "a row is 0x00 to 0x1F"))?;
                    let text = value
                        .as_str()
                        .ok_or_else(|| invalid(&at, "must be 32 words or ----"))?;
                    let cells = text
                        .split_whitespace()
                        .map(|w| {
                            if w == "----" {
                                Ok(None)
                            } else {
                                u16::from_str_radix(w, 16)
                                    .map(Some)
                                    .map_err(|_| invalid(&at, format!("{w:?} is not a word")))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if cells.len() != 32 {
                        return Err(invalid(&at, "must be 32 words or ----"));
                    }
                    if key == "border" {
                        changes.border.insert(r, cells);
                    } else {
                        changes.title.insert(r, cells);
                    }
                }
            }
            "tiles" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of tiles"))?;
                for (n, value) in table.iter() {
                    let at = format!("tiles.{n}");
                    let n = n
                        .strip_prefix("0x")
                        .and_then(|h| u16::from_str_radix(h, 16).ok())
                        .filter(|&n| usize::from(n) < crate::overworld::MAX_TILES)
                        .ok_or_else(|| invalid(&at, "a tile is 0x000 to 0x1FF"))?;
                    let text = value
                        .as_str()
                        .ok_or_else(|| invalid(&at, "must be 4 hex words"))?;
                    let words = parse_words(&at, text, 4, 0xFFFF)?;
                    changes.tiles.insert(n, words.try_into().expect("4 words"));
                }
            }
            "graphics" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of lists"))?;
                for (submap, value) in table.iter() {
                    let at = format!("graphics.{submap}");
                    let submap = submap
                        .strip_prefix("0x")
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .filter(|&s| s <= 6)
                        .ok_or_else(|| invalid(&at, "a submap is 0x00 to 0x06"))?;
                    let text = value
                        .as_str()
                        .ok_or_else(|| invalid(&at, "must be 16 hex words"))?;
                    let words = parse_words(&at, text, 16, 0xFFFF)?;
                    let list: [u16; 16] = words.try_into().expect("16 words");
                    changes
                        .graphics
                        .insert(submap, crate::exgfx::GraphicsList(list));
                }
            }
            "palettes" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of palettes"))?;
                let mut bytes = vec![0u8; crate::overworld::PALETTES_LEN];
                for (n, palette) in table.iter() {
                    let at = format!("palettes.{n}");
                    let n = n
                        .strip_prefix("0x")
                        .and_then(|h| usize::from_str_radix(h, 16).ok())
                        .filter(|&n| n < 14)
                        .ok_or_else(|| invalid(&at, "a palette is 0x00 to 0x0D"))?;
                    let rows = palette
                        .as_table()
                        .ok_or_else(|| invalid(&at, "must be a table of rows"))?;
                    for (row, value) in rows.iter() {
                        let at = format!("{at}.{row}");
                        let row = row
                            .strip_prefix("0x")
                            .and_then(|h| usize::from_str_radix(h, 16).ok())
                            .filter(|&r| r < 16)
                            .ok_or_else(|| invalid(&at, "a row is 0x0 to 0xF"))?;
                        let text = value
                            .as_str()
                            .ok_or_else(|| invalid(&at, "must be 16 colours"))?;
                        let colours: Vec<&str> = text.split_whitespace().collect();
                        if colours.len() != 16 {
                            return Err(invalid(&at, "must be 16 colours"));
                        }
                        for (i, c) in colours.iter().enumerate() {
                            let colour = crate::source::level::parse_color(&at, c)?;
                            let o = n * 0x200 + row * 32 + i * 2;
                            bytes[o..o + 2].copy_from_slice(&colour.0.to_le_bytes());
                        }
                    }
                }
                changes.palettes = Some(bytes);
            }
            "animation" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of lists"))?;
                let mut animation = crate::exanimation::OverworldAnimation::default();
                let mut comments = super::Comments::default();
                for (name, list) in table.iter() {
                    let at = format!("animation.{name}");
                    let list = list
                        .as_table()
                        .ok_or_else(|| invalid(&at, "must be a table"))?;
                    if name == "global" {
                        let (_, list) =
                            super::animation::read_as(list, &at, None, true, &mut comments)?;
                        animation.global = Some(list.ok_or_else(|| invalid(&at, "has no slots"))?);
                        continue;
                    }
                    let n = name
                        .strip_prefix("0x")
                        .and_then(|h| usize::from_str_radix(h, 16).ok())
                        .filter(|&n| n < crate::exanimation::OVERWORLD_SUBMAPS)
                        .ok_or_else(|| invalid(&at, "a submap is 0x00 to 0x06, or `global`"))?;
                    let (byte, list) = super::animation::read_as(
                        list,
                        &at,
                        Some("submap_list"),
                        true,
                        &mut comments,
                    )?;
                    animation.settings[n] = byte.unwrap_or(0);
                    animation.submaps[n] = list;
                }
                changes.animation = Some(animation);
            }
            "options" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table"))?;
                for (name, value) in table.iter() {
                    if name == "reveal_speed" || name == "merge_fg" {
                        continue;
                    }
                    if !overworld::GAME_OPTIONS.iter().any(|o| o.name == name) {
                        return Err(invalid(format!("options.{name}"), "is not an option"));
                    }
                    let on = value.as_bool().ok_or_else(|| {
                        invalid(format!("options.{name}"), "must be true or false")
                    })?;
                    changes.options.insert(name.to_string(), on);
                }
                if table.contains_key("reveal_speed") {
                    let speed = int("options.reveal_speed", table.get("reveal_speed"), 0x40)?;
                    if speed == 0 {
                        return Err(invalid("options.reveal_speed", "is 1 to 0x40"));
                    }
                    changes.reveal_speed = Some(speed as u8);
                }
                if let Some(item) = table.get("merge_fg") {
                    changes.merge_fg = item
                        .as_bool()
                        .ok_or_else(|| invalid("options.merge_fg", "must be true or false"))?;
                }
            }
            "tables" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table"))?;
                for (name, value) in table.iter() {
                    let at = format!("tables.{name}");
                    let Some(site) = crate::overworld::TABLES.iter().find(|t| t.name == name)
                    else {
                        return Err(invalid(&at, "is not a table of the overworld's"));
                    };
                    let text = value
                        .as_str()
                        .ok_or_else(|| invalid(&at, "must be hex bytes"))?;
                    let bytes = text
                        .split_whitespace()
                        .map(|b| u8::from_str_radix(b, 16))
                        .collect::<Result<Vec<u8>, _>>()
                        .map_err(|_| invalid(&at, "must be hex bytes"))?;
                    if bytes.len() != site.len {
                        return Err(invalid(&at, format!("has {} bytes", site.len)));
                    }
                    changes.tables.insert(name.to_string(), bytes);
                }
            }
            "event_tiles" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table"))?;
                let split = int("event_tiles.split", table.get("split"), 0xD00)?;
                changes.event_split = Some(split as u16);
            }
            "level_flags" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of settings"))?;
                for (t, value) in table.iter() {
                    let at = format!("level_flags.{t}");
                    let t = t
                        .strip_prefix("0x")
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .filter(|&t| t < 0x60)
                        .ok_or_else(|| invalid(&at, "a translevel is 0x00 to 0x5F"))?;
                    let flags = value
                        .as_value()
                        .map(|v| value_int(&at, v, 0xFF))
                        .transpose()?
                        .ok_or_else(|| invalid(&at, "must be a byte"))?;
                    changes.level_flags.insert(t, flags as u8);
                }
            }
            "level_events" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table of events"))?;
                for (t, value) in table.iter() {
                    let at = format!("level_events.{t}");
                    let t = t
                        .strip_prefix("0x")
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .filter(|&t| t < 0x60)
                        .ok_or_else(|| invalid(&at, "a translevel is 0x00 to 0x5F"))?;
                    let event = value
                        .as_value()
                        .map(|v| value_int(&at, v, 0xFF))
                        .transpose()?
                        .ok_or_else(|| invalid(&at, "must be an event"))?;
                    changes.level_events.insert(t, event as u8);
                }
            }
            "crush" | "reveal" => {
                let list = item
                    .get("list")
                    .and_then(Item::as_array)
                    .ok_or_else(|| invalid(key, "must have a list"))?;
                if key == "crush" {
                    let mut crush = Vec::new();
                    for v in list.iter() {
                        let n = numbers("crush.list", v, 3, 0xFFFF)?;
                        crush.push(Crush {
                            event: n[0] as u8,
                            place: n[1] as u16,
                            vram: n[2] as u16,
                        });
                    }
                    changes.crush = Some(crush);
                } else {
                    let mut reveal = Vec::new();
                    for v in list.iter() {
                        let n = numbers("reveal.list", v, 2, 0xFF)?;
                        reveal.push((n[0] as u8, n[1] as u8));
                    }
                    changes.reveal = Some(reveal);
                }
            }
            "start" => {
                let table = item
                    .as_table()
                    .ok_or_else(|| invalid(key, "must be a table"))?;
                let player = |who: &str| -> Result<Option<Start>, SourceError> {
                    let Some(v) = table.get(who).and_then(Item::as_value) else {
                        return Ok(None);
                    };
                    let at = format!("start.{who}");
                    let n = numbers(&at, v, 3, 0xFFFF)?;
                    if n[0] > 0xFF {
                        return Err(invalid(&at, "a submap is a byte"));
                    }
                    Ok(Some(Start {
                        submap: n[0] as u8,
                        x: n[1] as u16,
                        y: n[2] as u16,
                    }))
                };
                match (player("mario")?, player("luigi")?) {
                    (Some(m), Some(l)) => changes.start = Some([m, l]),
                    (None, None) => {}
                    _ => return Err(invalid(key, "has both mario and luigi or neither")),
                }
            }
            "event" => {
                let events = item
                    .as_array_of_tables()
                    .ok_or_else(|| invalid(key, "must be [[event]] tables"))?;
                for table in events.iter() {
                    let n = int("event.number", table.get("number"), 0x77)? as u8;
                    let at = format!("event {}", hex(u32::from(n), 2));
                    let layer1 = numbers(
                        &at,
                        table
                            .get("layer1")
                            .and_then(Item::as_value)
                            .ok_or_else(|| invalid(&at, "must have layer1"))?,
                        2,
                        0xFFFF,
                    )?;
                    let mut blocks = Vec::new();
                    let list = table
                        .get("blocks")
                        .and_then(Item::as_array)
                        .ok_or_else(|| invalid(&at, "must have blocks"))?;
                    for block in list.iter() {
                        let inline = block
                            .as_inline_table()
                            .ok_or_else(|| invalid(&at, "a block is { place, tiles }"))?;
                        blocks.push(parse_block(&at, inline)?);
                    }
                    let mut extras = Vec::new();
                    if let Some(list) = table.get("extras") {
                        let list = list
                            .as_array()
                            .ok_or_else(|| invalid(&at, "extras must be a list"))?;
                        for extra in list.iter() {
                            let inline = extra.as_inline_table().ok_or_else(|| {
                                invalid(&at, "an extra is { layer1, tile } or { place, tiles }")
                            })?;
                            extras.push(match inline.get("layer1") {
                                Some(place) => ExtraTile::Layer1 {
                                    place: value_int(&at, place, 0xFFFF)? as u16,
                                    tile: inline
                                        .get("tile")
                                        .map(|v| value_int(&at, v, 0xFFFF))
                                        .transpose()?
                                        .ok_or_else(|| invalid(&at, "a layer 1 extra has a tile"))?
                                        as u16,
                                },
                                None => ExtraTile::Layer2(parse_block(&at, inline)?),
                            });
                        }
                    }
                    changes.events.insert(
                        n,
                        Event {
                            layer1: (layer1[0] as u16, layer1[1] as u16),
                            blocks,
                            extras,
                        },
                    );
                }
            }
            _ => return Err(invalid(key, "is not part of an overworld file")),
        }
    }
    Ok((changes, top))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_back_as_written() {
        let name = parse_name("n", "YOSHI'S HOUSE").unwrap();
        assert_eq!(name_text(&name), "YOSHI'S HOUSE");
        let odd = parse_name("n", "A\\x38B").unwrap();
        assert_eq!(&odd[..3], &[0x00, 0x38, 0x01]);
        assert_eq!(name_text(&odd), "A\\x38B");
        assert!(parse_name("n", "TOO LONG A NAME FOR A LEVEL").is_err());
    }

    #[test]
    fn changes_read_back_as_written() {
        let mut changes = Changes::default();
        changes.layer1.insert((0, 5), (0..32).collect());
        changes
            .layer2
            .insert((1, 63), (0..64).map(|v| v * 0x101).collect());
        changes.levels.insert((1, 31, 2), (0x13, 0xC0));
        changes
            .names
            .insert(0x13, parse_name("n", "DONUT PLAINS 2").unwrap());
        changes.crush = Some(vec![Crush {
            event: 6,
            place: 0x419,
            vram: 0x2052,
        }]);
        changes.reveal = Some(vec![(0x6E, 0x58)]);
        changes.start = Some([
            Start {
                submap: 1,
                x: 0x68,
                y: 0x78,
            },
            Start {
                submap: 0,
                x: 0x1C8,
                y: 0x1C8,
            },
        ]);
        changes.level_flags.insert(0x28, 0x13);
        changes.level_events.insert(0x13, 0x0A);
        changes.event_split = Some(0x0A00);
        changes.reveal_speed = Some(6);
        changes.merge_fg = true;
        changes.options.insert("life_exchange".into(), false);
        changes.options.insert("save_prompts".into(), false);
        let mut row = vec![Some(0x38FE); 32];
        row[3] = None;
        row[4] = Some(0x7895);
        changes.border.insert(0x1C, row.clone());
        changes.title.insert(0x05, row);
        changes
            .tiles
            .insert(0x1C1, [0x0CA0, 0x0CB0, 0x4CA0, 0x4CB0]);
        let mut list = crate::exanimation::List {
            count: 1,
            ..Default::default()
        };
        list.slots.insert(
            0,
            crate::exanimation::Slot {
                kind: 0x02,
                trigger: 0x00,
                frames_less_one: 1,
                dest: 0x0400,
                frames: vec![0xAD00, 0xAD40],
            },
        );
        // An event's trigger, which on the overworld has a second set.
        list.slots.insert(
            1,
            crate::exanimation::Slot {
                kind: 0x14,
                trigger: 0x06,
                frames_less_one: 0,
                dest: 0x007A,
                frames: vec![0x001F, 0x03E0],
            },
        );
        list.count = 2;
        let mut animation = crate::exanimation::OverworldAnimation::default();
        animation.settings[1] = 0x40;
        animation.submaps[2] = Some(list.clone());
        animation.global = Some(list);
        changes.animation = Some(animation);
        changes.palettes = Some(
            (0..crate::overworld::PALETTES_LEN / 2)
                .flat_map(|i| ((i as u16).wrapping_mul(37) & 0x7FFF).to_le_bytes())
                .collect(),
        );
        changes
            .graphics
            .insert(1, crate::exgfx::GraphicsList([0x8014; 16]));
        changes
            .tables
            .insert("music".into(), vec![2, 3, 4, 6, 7, 9, 5]);
        changes.events.insert(
            5,
            Event {
                layer1: (0x123, 0x2345),
                blocks: vec![EventBlock {
                    place: 0x1A22,
                    tiles: vec![0x1C58; 4],
                }],
                extras: vec![
                    ExtraTile::Layer1 {
                        place: 0x215,
                        tile: 0x68,
                    },
                    ExtraTile::Layer2(EventBlock {
                        place: 0x1114,
                        tiles: vec![0x105D; 4],
                    }),
                ],
            },
        );
        changes.events.insert(6, Event::default());
        let text = to_toml(&changes, &["# Mine.".into()]);
        let (read, top) = from_toml(&text).unwrap();
        assert_eq!(read, changes, "{text}");
        assert_eq!(top, ["# Mine."]);
    }
}
