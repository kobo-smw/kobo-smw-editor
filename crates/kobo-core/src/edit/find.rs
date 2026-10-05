//! Finding objects and sprites across a project's levels, by name or
//! number: "goomba", "sprite 0F", "extended 41", "exit".

use super::{ObjectLayer, Workspace, object_position};
use crate::level::objects::Object;
use crate::names;
use crate::source::level::{Layer2, Level};

/// An entry of a level's lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    Object(ObjectLayer, usize),
    Sprite(usize),
}

/// One entry found: where it is, what it is called, and the words it was
/// found by.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Found {
    pub level: u16,
    pub entry: Entry,
    pub name: String,
    /// `object 21`, `extended 41`, `sprite 0F`, and so on.
    pub kind: String,
    pub at: Option<(u16, u16)>,
}

/// What an object is called and its kind's words.
fn object_words(object: &Object, tileset: u8) -> (String, String) {
    let name = names::object(object, tileset)
        .unwrap_or("Object")
        .to_string();
    let kind = match object {
        Object::Standard { number, .. } => format!("object {number:02X}"),
        Object::Extended { number, .. } => format!("extended {number:02X}"),
        Object::ScreenExit(exit) => format!("exit screen {:02X}", exit.screen),
        Object::Lunar { number, .. } => format!("lunar magic {number:02X}"),
        Object::Unplaced(_) => "unplaced".to_string(),
    };
    (name, kind)
}

/// Every entry of `level` with its words.
fn entries(number: u16, level: &Level) -> Vec<Found> {
    let tileset = level.header.object_tileset;
    let mut found = Vec::new();
    let layer2: &[Object] = match &level.layer2 {
        Layer2::Objects(list) => list,
        _ => &[],
    };
    for (layer, list) in [
        (ObjectLayer::One, &level.layer1[..]),
        (ObjectLayer::Two, layer2),
    ] {
        for (index, object) in list.iter().enumerate() {
            let (name, kind) = object_words(object, tileset);
            found.push(Found {
                level: number,
                entry: Entry::Object(layer, index),
                name,
                kind,
                at: object_position(object),
            });
        }
    }
    for (index, sprite) in level.sprites.list.iter().enumerate() {
        found.push(Found {
            level: number,
            entry: Entry::Sprite(index),
            name: names::sprite(sprite.id).to_string(),
            kind: format!("sprite {:02X}", sprite.id),
            at: Some((sprite.x, sprite.y)),
        });
    }
    found
}

/// Whether every word of `query` is in the entry's name or kind, a word
/// that is a number matching the kind's number whole.
fn matches(found: &Found, words: &[String]) -> bool {
    let name = found.name.to_lowercase();
    let kind = found.kind.to_lowercase();
    let number = kind.rsplit(' ').next().unwrap_or_default();
    words.iter().all(|word| {
        let is_number = word.len() <= 2 && word.chars().all(|c| c.is_ascii_hexdigit());
        if is_number && !name.split([' ', ',']).any(|w| w == word.as_str()) {
            number.trim_start_matches('0') == word.trim_start_matches('0')
        } else {
            name.contains(word.as_str()) || kind.contains(word.as_str())
        }
    })
}

/// The entries of every level of `workspace` that `query` finds, in level
/// order and each level's drawing order. An empty query finds nothing.
pub fn find(workspace: &Workspace, query: &str) -> Vec<Found> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut levels: Vec<u16> = workspace.levels().collect();
    levels.sort_unstable();
    levels
        .into_iter()
        .filter_map(|n| Some((n, workspace.level(n)?)))
        .flat_map(|(n, level)| entries(n, level))
        .filter(|found| matches(found, &words))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, kind: &str) -> Found {
        Found {
            level: 0x105,
            entry: Entry::Sprite(0),
            name: name.to_string(),
            kind: kind.to_string(),
            at: None,
        }
    }

    #[test]
    fn words_find_by_name_and_numbers_whole() {
        let goomba = found("Goomba", "sprite 0F");
        let words = |q: &str| -> Vec<String> { q.split(' ').map(str::to_string).collect() };
        assert!(matches(&goomba, &words("goomba")));
        assert!(matches(&goomba, &words("sprite 0f")));
        assert!(matches(&goomba, &words("f")));
        assert!(!matches(&goomba, &words("sprite 0")));
        assert!(!matches(&goomba, &words("koopa")));
        // A number that is a word of the name finds by the name.
        let switch = found("Switch palace 1", "object 2E");
        assert!(matches(&switch, &words("1")));
    }
}
