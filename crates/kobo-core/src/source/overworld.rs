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
//! [crush]                        # events, places, VRAM: all 24 when any changes
//! list = [[0x06, 0x0419, 0x2052], ...]
//!
//! [reveal]                       # the layer 1 tiles events turn into others
//! list = [[0x6E, 0x58], ...]
//!
//! [[event]]
//! number = 0x05
//! layer1 = [0x0123, 0x2345]      # its layer 1 tile's place and VRAM address
//! blocks = [
//!     { place = 0x1A22, tiles = "1C58 ..." },   # 36 tiles (6x6) or 4 (2x2)
//! ]
//! ```
//!
//! The maps are `main` and `submaps` (the six submaps share one).

use toml_edit::{DocumentMut, Item, Table, Value};

use super::{SourceError, hex, invalid};
use crate::overworld::{Changes, Crush, Event, EventBlock, NAME_TILES};

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
        sections.push(text);
    }
    out + &sections.join("\n")
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
                        let place = inline
                            .get("place")
                            .map(|v| value_int(&at, v, 0xFFFF))
                            .transpose()?
                            .ok_or_else(|| invalid(&at, "a block has a place"))?;
                        let text = inline
                            .get("tiles")
                            .and_then(Value::as_str)
                            .ok_or_else(|| invalid(&at, "a block has tiles"))?;
                        let count = text.split_whitespace().count();
                        if count != 36 && count != 4 {
                            return Err(invalid(&at, "a block has 36 tiles (6x6) or 4 (2x2)"));
                        }
                        blocks.push(EventBlock {
                            place: place as u16,
                            tiles: parse_words(&at, text, count, 0xFFFF)?,
                        });
                    }
                    changes.events.insert(
                        n,
                        Event {
                            layer1: (layer1[0] as u16, layer1[1] as u16),
                            blocks,
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
        changes.events.insert(
            5,
            Event {
                layer1: (0x123, 0x2345),
                blocks: vec![EventBlock {
                    place: 0x1A22,
                    tiles: vec![0x1C58; 4],
                }],
            },
        );
        changes.events.insert(6, Event::default());
        let text = to_toml(&changes, &["# Mine.".into()]);
        let (read, top) = from_toml(&text).unwrap();
        assert_eq!(read, changes, "{text}");
        assert_eq!(top, ["# Mine."]);
    }
}
