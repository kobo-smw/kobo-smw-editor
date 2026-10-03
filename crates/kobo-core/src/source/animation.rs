//! ExAnimation in project files: a level's `[animation]` table (its
//! settings and its list), and the global list's file, which has the list
//! alone (`kobo_core::exanimation` has the format they encode).
//!
//! ```toml
//! [animation]
//! game_tiles = true          # the settings, all four when any is given
//! game_colours = true
//! level_list = true
//! global_list = true
//! alt_file = 0x61            # ExGFX 60-63 for sources; 0x60 when left out
//! custom_keep = 0xFF00       # custom triggers that keep their state (0xFFFF)
//! custom_set = 0x0012        # custom triggers the load sets (0)
//! manual = [
//!     { trigger = 0x0, frame = 0x01 },
//! ]
//! slots = [
//!     { slot = 0x00, type = 0x04, trigger = 0x03, vram = 0x2000, frames = [0xAD00, 0xAD80], triggered = [0xAE00, 0xAE80] },  # 4 8x8s, line; ON/OFF
//!     { slot = 0x03, type = 0x14, colour = 0x21, frames = [0x001F, 0x03E0] },  # Palette and working copies
//!     { slot = 0x09, type = 0x18, colour = 0x31, colours = 4, delay = 2 },  # Palette rotate right
//! ]
//! ```
//!
//! A slot's frames are words: a RAM address in bank `$7E` (or an offset
//! into the alternative file, with `alt = true`), or for a single colour
//! the colour itself. `triggered` is the second set of a trigger that has
//! one; `delay` is how often a rotation turns.

use toml_edit::{Array, InlineTable, Table, Value};

use super::level::{check_keys, inline_flag, int, keys_of, opt, read_list, req};
use super::{Comments, SourceError, Writer, hex, invalid};
use crate::exanimation::{FIRST_ALT_FILE, Kind, List, Settings, Slot, second_set};
use crate::names;

const SETTINGS: [(&str, u8); 4] = [
    ("game_tiles", Settings::NO_GAME_TILES),
    ("game_colours", Settings::NO_GAME_COLOURS),
    ("level_list", Settings::NO_LEVEL),
    ("global_list", Settings::NO_GLOBAL),
];

/// The keys a list takes, and with `settings`, those of a level's.
fn keys(settings: bool) -> Vec<&'static str> {
    let mut keys = vec![
        "alt_file",
        "custom_keep",
        "custom_set",
        "manual",
        "count",
        "slots",
    ];
    if settings {
        keys.extend(SETTINGS.map(|(k, _)| k));
        keys.push("other_bits");
    }
    keys
}

/// Writes table `table`: the level's settings byte if it has one of its
/// own, and its list.
pub(crate) fn write(out: &mut Writer, table: &str, settings: Option<u8>, list: Option<&List>) {
    out.table(table);
    if let Some(byte) = settings {
        for (key, bit) in SETTINGS {
            out.key(table, key, byte & bit == 0);
        }
        if byte & 0x0F != 0 {
            out.key(table, "other_bits", hex(byte as u32 & 0x0F, 1));
        }
    }
    let Some(list) = list else { return };
    if list.alt_file != 0 {
        out.key(
            table,
            "alt_file",
            hex((FIRST_ALT_FILE + list.alt_file as u16) as u32, 2),
        );
    }
    if list.custom_keep != 0xFFFF {
        out.key(table, "custom_keep", hex(list.custom_keep as u32, 4));
    }
    if list.custom_set != 0 {
        out.key(table, "custom_set", hex(list.custom_set as u32, 4));
    }
    if !list.manual.is_empty() {
        let manual: Vec<(u8, u8)> = list.manual.iter().map(|(&t, &f)| (t, f)).collect();
        out.list(table, "manual", &manual, |(trigger, frame)| {
            (
                format!(
                    "{{ trigger = {}, frame = {} }}",
                    hex(*trigger as u32, 1),
                    hex(*frame as u32, 2)
                ),
                None,
            )
        });
    }
    let highest = list.slots.keys().next_back().map_or(0, |&s| s + 1);
    if list.count != highest {
        out.key(table, "count", list.count);
    }
    let slots: Vec<(u8, &Slot)> = list.slots.iter().map(|(&n, s)| (n, s)).collect();
    out.list(table, "slots", &slots, |(n, s)| slot_line(*n, s));
}

fn words(list: &[u16]) -> String {
    let parts: Vec<String> = list.iter().map(|&w| hex(w as u32, 4)).collect();
    format!("[{}]", parts.join(", "))
}

fn slot_line(n: u8, s: &Slot) -> (String, Option<String>) {
    let mut text = format!(
        "{{ slot = {}, type = {}",
        hex(n as u32, 2),
        hex(s.kind as u32, 2)
    );
    if s.trigger != 0 {
        text += &format!(", trigger = {}", hex(s.trigger as u32, 2));
    }
    match Kind::of(s.kind) {
        Some(Kind::Tiles) => {
            text += &format!(", vram = {}", hex(s.dest as u32 & 0x7FFF, 4));
        }
        _ => {
            text += &format!(", colour = {}", hex(s.colour() as u32, 2));
            if s.colours() != 1 {
                text += &format!(", colours = {}", s.colours());
            }
        }
    }
    if s.alternative() {
        text += ", alt = true";
    }
    if Kind::of(s.kind) == Some(Kind::Rotation) {
        text += &format!(", delay = {}", s.frames_less_one as u32 + 1);
    } else if second_set(s.trigger) {
        let half = s.frames.len() / 2;
        text += &format!(
            ", frames = {}, triggered = {}",
            words(&s.frames[..half]),
            words(&s.frames[half..])
        );
    } else {
        text += &format!(", frames = {}", words(&s.frames));
    }
    let name = match (
        names::exanimation_type(s.kind),
        names::exanimation_trigger(s.trigger).filter(|_| s.trigger != 0),
    ) {
        (Some(kind), Some(trigger)) => Some(format!("{kind}; {trigger}")),
        (Some(kind), None) => Some(kind.to_owned()),
        _ => None,
    };
    (text + " }", name)
}

/// Reads table `at`: the settings byte, when `settings` allows them and it
/// gives them, and the list, when it has `slots`.
pub(crate) fn read(
    t: &Table,
    at: &str,
    settings: bool,
    comments: &mut Comments,
) -> Result<(Option<u8>, Option<List>), SourceError> {
    check_keys(t.iter(), at, &keys(settings))?;
    let mut byte = None;
    if settings && SETTINGS.iter().any(|(k, _)| t.contains_key(k)) {
        let mut b = 0u8;
        for (key, bit) in SETTINGS {
            let on = match t.get(key) {
                None => true,
                Some(item) => item
                    .as_bool()
                    .ok_or_else(|| invalid(format!("{at}.{key}"), "must be true or false"))?,
            };
            if !on {
                b |= bit;
            }
        }
        if let Some(item) = t.get("other_bits") {
            b |= int(item, &format!("{at}.other_bits"), 0x0F)? as u8;
        }
        byte = Some(b);
    } else if t.contains_key("other_bits") {
        return Err(invalid(at, "`other_bits` goes with the four settings"));
    }
    if !t.contains_key("slots") {
        for key in ["alt_file", "custom_keep", "custom_set", "manual", "count"] {
            if t.contains_key(key) {
                return Err(invalid(format!("{at}.{key}"), "goes with `slots`"));
            }
        }
        return Ok((byte, None));
    }
    let get = |key: &str, max: u32| {
        t.get(key)
            .map(|item| int(item, &format!("{at}.{key}"), max))
            .transpose()
    };
    let alt_file = match get("alt_file", 0x63)? {
        None => 0,
        Some(n) if n >= FIRST_ALT_FILE as u32 => (n - FIRST_ALT_FILE as u32) as u8,
        Some(_) => return Err(invalid(format!("{at}.alt_file"), "is 0x60 to 0x63")),
    };
    let mut list = List {
        alt_file,
        custom_keep: get("custom_keep", 0xFFFF)?.unwrap_or(0xFFFF) as u16,
        custom_set: get("custom_set", 0xFFFF)?.unwrap_or(0) as u16,
        ..List::default()
    };
    if t.contains_key("manual") {
        let entries = read_list(t, "manual", at, comments, |e, at| {
            keys_of(e, at, &["trigger", "frame"])?;
            Ok((
                req(e, at, "trigger", 0x0F)? as u8,
                req(e, at, "frame", 0xFF)? as u8,
            ))
        })?;
        for (trigger, frame) in entries {
            if list.manual.insert(trigger, frame).is_some() {
                return Err(invalid(
                    format!("{at}.manual"),
                    format!("manual trigger {trigger:X} is set twice"),
                ));
            }
        }
    }
    for (n, slot) in read_list(t, "slots", at, comments, read_slot)? {
        if list.slots.insert(n, slot).is_some() {
            return Err(invalid(
                format!("{at}.slots"),
                format!("slot {n:02X} is listed twice"),
            ));
        }
    }
    let highest = list.slots.keys().next_back().map_or(0, |&s| s + 1);
    list.count = match get("count", 32)? {
        Some(n) if (n as u8) < highest => {
            return Err(invalid(
                format!("{at}.count"),
                format!("is less than the slots used ({highest})"),
            ));
        }
        Some(n) => n as u8,
        None => highest,
    };
    Ok((byte, Some(list)))
}

fn frame_words(t: &InlineTable, at: &str, key: &str) -> Result<Vec<u16>, SourceError> {
    let at = format!("{at}.{key}");
    let array: &Array = t
        .get(key)
        .ok_or_else(|| invalid(&at, "is missing"))?
        .as_array()
        .ok_or_else(|| invalid(&at, "must be an array of words"))?;
    array
        .iter()
        .map(|v: &Value| {
            v.as_integer()
                .and_then(|n| u16::try_from(n).ok())
                .ok_or_else(|| invalid(&at, "must be words, 0 to 0xFFFF"))
        })
        .collect()
}

fn read_slot(t: &InlineTable, at: &str) -> Result<(u8, Slot), SourceError> {
    let n = req(t, at, "slot", 0x1F)? as u8;
    let kind = req(t, at, "type", 0xFF)? as u8;
    let trigger = opt(t, at, "trigger", 0xFF)?.unwrap_or(0) as u8;
    let alt = inline_flag(t, at, "alt")?;
    let kind_of = Kind::of(kind)
        .ok_or_else(|| invalid(format!("{at}.type"), format!("{kind:02X} is not 01 to 1B")))?;
    let (dest, frames_less_one, frames) = match kind_of {
        Kind::Tiles => {
            keys_of(
                t,
                at,
                &[
                    "slot",
                    "type",
                    "trigger",
                    "vram",
                    "alt",
                    "frames",
                    "triggered",
                ],
            )?;
            let vram = req(t, at, "vram", 0x7FFF)? as u16;
            let (count, frames) = read_frames(t, at, trigger)?;
            (vram | if alt { 0x8000 } else { 0 }, count, frames)
        }
        Kind::Colours | Kind::Rotation => {
            let rotation = kind_of == Kind::Rotation;
            let allowed: &[&str] = if rotation {
                &[
                    "slot", "type", "trigger", "colour", "colours", "alt", "delay",
                ]
            } else {
                &[
                    "slot",
                    "type",
                    "trigger",
                    "colour",
                    "colours",
                    "alt",
                    "frames",
                    "triggered",
                ]
            };
            keys_of(t, at, allowed)?;
            let colour = req(t, at, "colour", 0xFF)? as u16;
            let colours = opt(t, at, "colours", 0x80)?.unwrap_or(1) as u16;
            if colours == 0 {
                return Err(invalid(format!("{at}.colours"), "is 1 to 128"));
            }
            let dest = colour | (colours - 1) << 8 | if alt { 0x8000 } else { 0 };
            if rotation {
                let delay = opt(t, at, "delay", 0x100)?.unwrap_or(1);
                if delay == 0 {
                    return Err(invalid(format!("{at}.delay"), "is 1 to 256"));
                }
                (dest, (delay - 1) as u8, Vec::new())
            } else {
                let (count, frames) = read_frames(t, at, trigger)?;
                (dest, count, frames)
            }
        }
    };
    Ok((
        n,
        Slot {
            kind,
            trigger,
            frames_less_one,
            dest,
            frames,
        },
    ))
}

/// A slot's frames, and its frames less one.
fn read_frames(t: &InlineTable, at: &str, trigger: u8) -> Result<(u8, Vec<u16>), SourceError> {
    let mut frames = frame_words(t, at, "frames")?;
    let limit = if second_set(trigger) { 0x80 } else { 0x100 };
    if frames.is_empty() || frames.len() > limit {
        return Err(invalid(
            format!("{at}.frames"),
            format!("has 1 to {limit} frames"),
        ));
    }
    let count = (frames.len() - 1) as u8;
    match (second_set(trigger), t.contains_key("triggered")) {
        (true, true) => {
            let second = frame_words(t, at, "triggered")?;
            if second.len() != frames.len() {
                return Err(invalid(
                    format!("{at}.triggered"),
                    "has as many frames as `frames`",
                ));
            }
            frames.extend(second);
        }
        (true, false) => {
            return Err(invalid(
                format!("{at}.triggered"),
                "is missing: the trigger has a second set of frames",
            ));
        }
        (false, true) => {
            return Err(invalid(
                format!("{at}.triggered"),
                "only a trigger with a second set of frames has one",
            ));
        }
        (false, false) => {}
    }
    Ok((count, frames))
}

/// The global list's file: `[animation]` with a list and no settings.
pub fn global_to_toml(list: &List, comments: &Comments) -> String {
    let mut out = Writer::new(comments);
    write(&mut out, "animation", None, Some(list));
    out.finish()
}

pub fn global_from_toml(text: &str) -> Result<(List, Comments), SourceError> {
    let doc: toml_edit::DocumentMut = text.parse()?;
    let mut comments = super::read_comments(&doc);
    check_keys(doc.as_table().iter(), "file", &["animation"])?;
    let t = doc
        .get("animation")
        .and_then(|i| i.as_table())
        .ok_or_else(|| invalid("animation", "must be a table"))?;
    let (_, list) = read(t, "animation", false, &mut comments)?;
    let list = list.ok_or_else(|| invalid("animation.slots", "is missing"))?;
    Ok((list, comments))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "\
[animation]
alt_file = 0x61
custom_keep = 0xFF00
custom_set = 0x0012
manual = [
    { trigger = 0x0, frame = 0x01 },
    { trigger = 0x2, frame = 0x02 },
]
count = 12
slots = [
    { slot = 0x00, type = 0x04, trigger = 0x03, vram = 0x2000, frames = [0xAD00, 0xAD80], triggered = [0xAE00, 0xAE80] },  # 4 8x8s, line; ON/OFF
    # A colour.
    { slot = 0x03, type = 0x14, colour = 0x21, alt = true, frames = [0x001F, 0x03E0] },  # Palette and working copies
    { slot = 0x09, type = 0x18, colour = 0x31, colours = 4, delay = 2 },  # Palette rotate right
]
";

    #[test]
    fn global_lists_read_and_write_back() {
        let (list, comments) = global_from_toml(TEXT).unwrap();
        assert_eq!(global_to_toml(&list, &comments), TEXT);
        assert_eq!(list.count, 12);
        assert_eq!(list.slots[&0].frames.len(), 4);
        assert_eq!(list.slots[&3].dest, 0x8021);
        assert_eq!(list.slots[&9].dest, 0x0331);
        assert_eq!(list.slots[&9].frames_less_one, 1);
        // Through the ROM's bytes and back.
        let (back, _) = List::parse(&list.to_bytes()).unwrap();
        assert_eq!(back, list);
    }

    #[test]
    fn mistakes_are_refused() {
        for (bad, what) in [
            (
                "{ slot = 0x00, type = 0x04, trigger = 0x03, vram = 0x2000, frames = [0xAD00] }",
                "triggered",
            ),
            (
                "{ slot = 0x00, type = 0x04, vram = 0x2000, frames = [0xAD00], triggered = [1] }",
                "triggered",
            ),
            (
                "{ slot = 0x00, type = 0x1C, vram = 0x2000, frames = [0xAD00] }",
                "type",
            ),
            (
                "{ slot = 0x00, type = 0x18, colour = 0x21, frames = [1] }",
                "frames",
            ),
            (
                "{ slot = 0x00, type = 0x04, vram = 0x2000, frames = [] }",
                "frames",
            ),
        ] {
            let text = format!("[animation]\nslots = [\n    {bad},\n]\n");
            let error = global_from_toml(&text).unwrap_err().to_string();
            assert!(error.contains(what), "{bad}: {error}");
        }
    }
}
