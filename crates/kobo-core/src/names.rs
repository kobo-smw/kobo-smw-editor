//! Names for the numbers a level is made of: objects, extended objects,
//! sprites, tilesets, and music. Kobo writes them as trailing comments
//! after the numbers in project files and shows them in the editor; the
//! numbers stay the data. Level modes are named by
//! [`LevelMode::name`](crate::level::LevelMode::name), next to what the
//! library says a mode means.
//!
//! The names are data, in `names.toml` beside this file, compiled in and
//! parsed on first use: one `ID = "Name"` line per number, so a table
//! reads like the list it is, a correction is a one-line diff, and
//! nobody needs to know Rust to review one. The tests hold the file to
//! its rules (every number of a table present, in order, names short,
//! ASCII, and distinct), so a malformed file fails them rather than a
//! user's build.
//!
//! These are the vanilla game's names. Custom sprites and objects take
//! theirs from the user's tools.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::level::objects::Object;

/// The name of a number the game does nothing sensible with.
pub const UNUSED: &str = "Unused";

/// The longest name any table may hold, in characters.
pub const MAX_LEN: usize = 44;

const SOURCE: &str = include_str!("names.toml");

/// A group of object tilesets that share their objects `2E`-`3F`: the
/// game picks the object routines by tileset through the table at
/// `$0DA41E` (`CODE_0DA415`), which has five distinct targets, numbered
/// here in the order the tilesets first reach them. Lunar Magic's "T"
/// value in its header dialog seems to be this number: its help puts the
/// mushroom ledge (`3C`, rope set) at T = 2.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ObjectSet {
    /// Tilesets 0, 7, and C.
    Normal = 0,
    /// Tileset 1.
    Castle = 1,
    /// Tilesets 2, 6, and 8.
    Rope = 2,
    /// Tilesets 3, 9, A, B, and E.
    Underground = 3,
    /// Tilesets 4, 5, and D.
    GhostHouse = 4,
}

impl ObjectSet {
    pub const ALL: [ObjectSet; 5] = [
        ObjectSet::Normal,
        ObjectSet::Castle,
        ObjectSet::Rope,
        ObjectSet::Underground,
        ObjectSet::GhostHouse,
    ];

    /// The set an object tileset (the primary header's FG/BG GFX
    /// setting) draws its objects with. The game has none for tileset F.
    pub fn of_tileset(tileset: u8) -> Option<ObjectSet> {
        use ObjectSet::*;
        const BY_TILESET: [ObjectSet; 15] = [
            Normal,
            Castle,
            Rope,
            Underground,
            GhostHouse,
            GhostHouse,
            Rope,
            Normal,
            Rope,
            Underground,
            Underground,
            Underground,
            Normal,
            GhostHouse,
            Underground,
        ];
        BY_TILESET.get(tileset as usize).copied()
    }

    /// The set's table in `names.toml`.
    fn key(self) -> &'static str {
        match self {
            ObjectSet::Normal => "normal",
            ObjectSet::Castle => "castle",
            ObjectSet::Rope => "rope",
            ObjectSet::Underground => "underground",
            ObjectSet::GhostHouse => "ghost_house",
        }
    }
}

/// The first standard object that depends on the object set.
const FIRST_SET_OBJECT: u8 = 0x2E;
const LAST_OBJECT: u8 = 0x3F;

/// One table: a name per id, `None` where the table has no line.
struct Table(Vec<Option<String>>);

impl Table {
    /// Takes a table's lines, checking that every id is two upper-case
    /// hex digits. Order and completeness are the tests' business.
    fn parse(what: &str, lines: BTreeMap<String, String>) -> Table {
        let mut names = vec![None; 0x100];
        for (key, name) in lines {
            let valid =
                key.len() == 2 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'));
            assert!(
                valid,
                "names.toml [{what}]: id {key:?} is not two upper-case hex digits"
            );
            let id = u8::from_str_radix(&key, 16).expect("checked hex");
            names[id as usize] = Some(name);
        }
        Table(names)
    }

    fn get(&'static self, id: u8) -> Option<&'static str> {
        self.0[id as usize].as_deref()
    }

    #[cfg(test)]
    fn ids(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=0xFF).filter(|&id| self.0[id as usize].is_some())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    objects: BTreeMap<String, String>,
    set_objects: BTreeMap<String, BTreeMap<String, String>>,
    lunar_magic_objects: BTreeMap<String, String>,
    extended_objects: BTreeMap<String, String>,
    sprites: BTreeMap<String, String>,
    object_tilesets: BTreeMap<String, String>,
    sprite_tilesets: BTreeMap<String, String>,
    music: BTreeMap<String, String>,
    submaps: BTreeMap<String, String>,
    exanimation_types: BTreeMap<String, String>,
    exanimation_triggers: BTreeMap<String, String>,
    overworld_exanimation_triggers: BTreeMap<String, String>,
    layer3_scroll: BTreeMap<String, String>,
    entrance_actions: BTreeMap<String, String>,
    backgrounds: BTreeMap<String, String>,
}

struct Names {
    objects: Table,
    set_objects: [Table; 5],
    lunar_magic_objects: Table,
    extended_objects: Table,
    sprites: Table,
    object_tilesets: Table,
    sprite_tilesets: Table,
    music: Table,
    submaps: Table,
    exanimation_types: Table,
    exanimation_triggers: Table,
    overworld_exanimation_triggers: Table,
    layer3_scroll: Table,
    entrance_actions: Table,
    /// The game's backgrounds, by address.
    backgrounds: BTreeMap<u32, String>,
}

static NAMES: LazyLock<Names> = LazyLock::new(|| {
    let mut source: Source =
        toml::from_str(SOURCE).unwrap_or_else(|e| panic!("names.toml does not parse: {e}"));
    let set_objects = ObjectSet::ALL.map(|set| {
        let lines = source
            .set_objects
            .remove(set.key())
            .unwrap_or_else(|| panic!("names.toml has no [set_objects.{}]", set.key()));
        Table::parse(set.key(), lines)
    });
    assert!(
        source.set_objects.is_empty(),
        "names.toml has unknown object sets: {:?}",
        source.set_objects.keys().collect::<Vec<_>>()
    );
    Names {
        objects: Table::parse("objects", source.objects),
        set_objects,
        lunar_magic_objects: Table::parse("lunar_magic_objects", source.lunar_magic_objects),
        extended_objects: Table::parse("extended_objects", source.extended_objects),
        sprites: Table::parse("sprites", source.sprites),
        object_tilesets: Table::parse("object_tilesets", source.object_tilesets),
        sprite_tilesets: Table::parse("sprite_tilesets", source.sprite_tilesets),
        music: Table::parse("music", source.music),
        submaps: Table::parse("submaps", source.submaps),
        exanimation_types: Table::parse("exanimation_types", source.exanimation_types),
        exanimation_triggers: Table::parse("exanimation_triggers", source.exanimation_triggers),
        overworld_exanimation_triggers: Table::parse(
            "overworld_exanimation_triggers",
            source.overworld_exanimation_triggers,
        ),
        layer3_scroll: Table::parse("layer3_scroll", source.layer3_scroll),
        entrance_actions: Table::parse("entrance_actions", source.entrance_actions),
        backgrounds: source
            .backgrounds
            .into_iter()
            .map(|(key, name)| {
                let valid =
                    key.len() == 6 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'));
                assert!(valid, "names.toml [backgrounds]: {key:?} is not an address");
                (u32::from_str_radix(&key, 16).expect("checked hex"), name)
            })
            .collect(),
    }
});

/// A standard object (`01`-`3F`) as the game draws it in a level of
/// this object tileset. `2E`-`3F` depend on the tileset's
/// [`ObjectSet`]; `22`-`2D` are unused in the game (Lunar Magic's are
/// [`lunar_magic_object`]). `None` for `00` (see [`extended_object`]),
/// numbers past `3F`, and `2E`-`3F` in a tileset with no object set.
pub fn standard_object(number: u8, tileset: u8) -> Option<&'static str> {
    match number {
        0x01..FIRST_SET_OBJECT => NAMES.objects.get(number),
        FIRST_SET_OBJECT..=LAST_OBJECT => {
            let set = ObjectSet::of_tileset(tileset)?;
            NAMES.set_objects[set as usize].get(number)
        }
        _ => None,
    }
}

/// One of the objects Lunar Magic adds: `22`-`29` and `2D`.
pub fn lunar_magic_object(number: u8) -> Option<&'static str> {
    NAMES.lunar_magic_objects.get(number)
}

/// An extended object, standard object `00` with this number in its
/// third byte. `02` and `03` are Lunar Magic's.
pub fn extended_object(number: u8) -> &'static str {
    NAMES
        .extended_objects
        .get(number)
        .expect("every extended object has a name")
}

/// A sprite number as a level's sprite list has it, in the vanilla game.
pub fn sprite(number: u8) -> &'static str {
    NAMES
        .sprites
        .get(number)
        .expect("every sprite number has a name")
}

/// An object tileset, the primary header's FG/BG GFX setting (`0`-`F`).
pub fn object_tileset(tileset: u8) -> Option<&'static str> {
    NAMES.object_tilesets.get(tileset)
}

/// A sprite tileset, the primary header's sprite GFX setting (`0`-`F`).
pub fn sprite_tileset(tileset: u8) -> Option<&'static str> {
    NAMES.sprite_tilesets.get(tileset)
}

/// The primary header's music setting (`0`-`7`), as the vanilla song
/// table maps it.
pub fn music(setting: u8) -> Option<&'static str> {
    NAMES.music.get(setting)
}

/// An overworld submap (`OWPlayerSubmap`, `0`-`6`), as the game has them.
pub fn submap(submap: u8) -> Option<&'static str> {
    NAMES.submaps.get(submap)
}

/// One of the game's backgrounds (`level::game_backgrounds`), by its
/// address.
pub fn game_background(address: crate::addr::SnesAddr) -> Option<&'static str> {
    NAMES.backgrounds.get(&address.raw()).map(String::as_str)
}

/// A level name as the overworld spells it (`level::level_name`), in title
/// case: "YOSHI'S ISLAND 1" as "Yoshi's Island 1", "of" kept small.
pub fn title_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (i, word) in name.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if i > 0 && word.eq_ignore_ascii_case("of") {
            out.push_str("of");
            continue;
        }
        let mut start = true;
        for c in word.chars() {
            out.push(if start {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            });
            start = c == '-';
        }
    }
    out
}

/// An ExAnimation slot's type (`01`-`1B`).
pub fn exanimation_type(kind: u8) -> Option<&'static str> {
    NAMES.exanimation_types.get(kind)
}

/// An ExAnimation trigger (`00`-`4F`).
pub fn exanimation_trigger(trigger: u8) -> Option<&'static str> {
    NAMES.exanimation_triggers.get(trigger)
}

/// An ExAnimation trigger in an overworld list (`00`-`4F`): `01`-`08`
/// are events, the rest a level's.
pub fn overworld_exanimation_trigger(trigger: u8) -> Option<&'static str> {
    NAMES
        .overworld_exanimation_triggers
        .get(trigger)
        .or_else(|| exanimation_trigger(trigger))
}

/// A layer 3 scroll setting of Lunar Magic's (`00`-`1F`,
/// [`crate::exgfx::Layer3Settings`]).
pub fn layer3_scroll(setting: u8) -> Option<&'static str> {
    NAMES.layer3_scroll.get(setting)
}

/// An entrance's action (`0`-`7`): how the player comes in.
pub fn entrance_action(action: u8) -> Option<&'static str> {
    NAMES.entrance_actions.get(action)
}

/// The name of a decoded object in a level of this object tileset:
/// whichever table its kind and number belong to.
pub fn object(object: &Object, tileset: u8) -> Option<&'static str> {
    match object {
        Object::Standard { number, .. } => standard_object(*number, tileset),
        Object::Extended { number, .. } => Some(extended_object(*number)),
        Object::ScreenExit(_) => Some(extended_object(0x00)),
        Object::Lunar { number, .. } => lunar_magic_object(*number),
        Object::Unplaced(bytes) => {
            let (a, b) = (*bytes.first()?, *bytes.get(1)?);
            match ((a & 0x60) >> 1) | (b >> 4) {
                0 => bytes.get(2).map(|&number| extended_object(number)),
                number => lunar_magic_object(number),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Layer2Kind, LevelMode};
    use std::collections::HashSet;
    use std::ops::RangeInclusive;

    fn ids(range: RangeInclusive<u8>) -> Vec<u8> {
        range.collect()
    }

    fn tables() -> Vec<(&'static str, &'static Table, Vec<u8>)> {
        let mut all = vec![
            ("objects", &NAMES.objects, ids(0x01..=0x2D)),
            (
                "lunar_magic_objects",
                &NAMES.lunar_magic_objects,
                [ids(0x22..=0x29), vec![0x2D]].concat(),
            ),
            (
                "extended_objects",
                &NAMES.extended_objects,
                ids(0x00..=0xFF),
            ),
            ("sprites", &NAMES.sprites, ids(0x00..=0xFF)),
            ("object_tilesets", &NAMES.object_tilesets, ids(0x00..=0x0F)),
            ("sprite_tilesets", &NAMES.sprite_tilesets, ids(0x00..=0x0F)),
            ("music", &NAMES.music, ids(0x00..=0x07)),
            ("submaps", &NAMES.submaps, ids(0x00..=0x06)),
            (
                "exanimation_types",
                &NAMES.exanimation_types,
                ids(0x01..=0x1B),
            ),
            (
                "exanimation_triggers",
                &NAMES.exanimation_triggers,
                ids(0x00..=0x4F),
            ),
            (
                "overworld_exanimation_triggers",
                &NAMES.overworld_exanimation_triggers,
                ids(0x01..=0x08),
            ),
            ("layer3_scroll", &NAMES.layer3_scroll, ids(0x00..=0x1F)),
            (
                "entrance_actions",
                &NAMES.entrance_actions,
                ids(0x00..=0x07),
            ),
        ];
        for set in ObjectSet::ALL {
            all.push((
                set.key(),
                &NAMES.set_objects[set as usize],
                ids(0x2E..=0x3F),
            ));
        }
        all
    }

    fn check_name(what: &str, id: u8, name: &str) {
        assert!(!name.is_empty(), "{what} {id:02X}: empty name");
        assert!(
            name.len() <= MAX_LEN,
            "{what} {id:02X}: {name:?} is longer than {MAX_LEN}"
        );
        assert!(
            name.bytes().all(|b| b.is_ascii_graphic() || b == b' '),
            "{what} {id:02X}: {name:?} is not printable ASCII on one line"
        );
        assert_eq!(
            name,
            name.trim(),
            "{what} {id:02X}: {name:?} has outer spaces"
        );
        assert!(
            !name.contains("  "),
            "{what} {id:02X}: {name:?} has a double space"
        );
    }

    fn check_distinct<'a>(what: &str, names: impl IntoIterator<Item = &'a str>) {
        let mut seen = HashSet::new();
        for name in names {
            assert!(
                name == UNUSED || seen.insert(name),
                "{what}: {name:?} appears twice"
            );
        }
    }

    #[test]
    fn every_table_has_exactly_its_numbers() {
        for (what, table, expected) in tables() {
            assert_eq!(table.ids().collect::<Vec<_>>(), expected, "[{what}]");
        }
    }

    #[test]
    fn names_are_short_ascii_lines() {
        for (what, table, _) in tables() {
            for id in table.ids() {
                check_name(what, id, table.get(id).unwrap());
            }
        }
        for mode in 0..0x20 {
            check_name("level mode", mode, LevelMode(mode).name().unwrap());
        }
        for name in NAMES.backgrounds.values() {
            check_name("background", 0, name);
        }
    }

    #[test]
    fn names_are_distinct() {
        for (what, table, _) in tables() {
            check_distinct(what, table.ids().map(|id| table.get(id).unwrap()));
        }
        // What one tileset offers, shared and set-specific together.
        for tileset in 0..0x0F {
            let names = (0x01..=LAST_OBJECT).map(|n| standard_object(n, tileset).unwrap());
            check_distinct(&format!("objects of tileset {tileset:X}"), names);
        }
        check_distinct(
            "level modes",
            (0..0x20).map(|m| LevelMode(m).name().unwrap()),
        );
        check_distinct(
            "backgrounds",
            NAMES.backgrounds.values().map(String::as_str),
        );
    }

    #[test]
    fn level_names_read_in_title_case() {
        assert_eq!(title_case("YOSHI'S ISLAND 1"), "Yoshi's Island 1");
        assert_eq!(title_case("#1 IGGY'S CASTLE"), "#1 Iggy's Castle");
        assert_eq!(title_case("FOREST OF ILLUSION 1"), "Forest of Illusion 1");
        assert_eq!(title_case("CHOCO-GHOST HOUSE"), "Choco-Ghost House");
    }

    #[test]
    fn file_lists_ids_in_order() {
        let mut section = "";
        let mut last: Option<u32> = None;
        for line in SOURCE.lines() {
            if line.starts_with('[') {
                (section, last) = (line, None);
            } else if line.starts_with('#') {
                continue;
            } else if let Some((key, _)) = line.split_once(" = ") {
                let id = u32::from_str_radix(key, 16).unwrap();
                assert!(last < Some(id), "{section}: {key} is out of order");
                last = Some(id);
            }
        }
    }

    #[test]
    fn tilesets_map_to_object_sets_as_the_game_dispatches() {
        let groups: [&[u8]; 5] = [
            &[0, 7, 12],
            &[1],
            &[2, 6, 8],
            &[3, 9, 10, 11, 14],
            &[4, 5, 13],
        ];
        for (set, tilesets) in ObjectSet::ALL.into_iter().zip(groups) {
            for &t in tilesets {
                assert_eq!(ObjectSet::of_tileset(t), Some(set), "tileset {t:X}");
            }
        }
        assert_eq!(ObjectSet::of_tileset(0x0F), None);
        assert_eq!(standard_object(0x3C, 0x0F), None);
        assert_eq!(standard_object(0x21, 0x0F), Some("Long ground ledge"));
        assert_eq!(standard_object(0x3C, 0x02), Some("Mushroom ledge"));
        assert_eq!(standard_object(0x00, 0x00), None);
        assert_eq!(standard_object(0x40, 0x00), None);
    }

    #[test]
    fn unused_objects_are_where_the_game_has_no_routine() {
        // Objects before these run the water surface routine past the end
        // of its table: `OBJRepTileWithTop` in each set's dispatch.
        let first_real = [0x30, 0x34, 0x32, 0x34, 0x2E];
        for (set, first) in ObjectSet::ALL.into_iter().zip(first_real) {
            let table = &NAMES.set_objects[set as usize];
            for n in FIRST_SET_OBJECT..=LAST_OBJECT {
                let unused = table.get(n) == Some(UNUSED);
                assert_eq!(unused, n < first, "{set:?} object {n:02X}");
            }
        }
        for n in 0x01..FIRST_SET_OBJECT {
            assert_eq!(
                NAMES.objects.get(n) == Some(UNUSED),
                n >= 0x22,
                "object {n:02X}"
            );
        }
        for n in 0x00..=0xFF {
            let unused = matches!(n, 0x04..=0x0F | 0x98..=0xFF);
            assert_eq!(
                extended_object(n) == UNUSED,
                unused,
                "extended object {n:02X}"
            );
        }
    }

    #[test]
    fn sprite_names_follow_the_loader_ranges() {
        // `LoadSprFromLevel` hands C9-CA to the shooters, CB-D9 to the
        // generators (D2 and D9 stop them), and E7 on to the scroll
        // commands, of which it has fifteen.
        for n in 0x00..=0xFF {
            let name = sprite(n);
            assert_eq!(
                name.starts_with("Shooter: "),
                matches!(n, 0xC9..=0xCA),
                "{n:02X}"
            );
            let generator = matches!(n, 0xCB..=0xD9) && !matches!(n, 0xD2 | 0xD9);
            assert_eq!(name.starts_with("Generator: "), generator, "{n:02X}");
            assert!(
                !name.starts_with("Scroll: ") || matches!(n, 0xE8..=0xF5),
                "{n:02X}"
            );
            assert!(n < 0xF6 || name == UNUSED, "{n:02X}");
        }
    }

    #[test]
    fn level_mode_names_agree_with_the_mode() {
        for mode in (0..0x20).map(LevelMode) {
            let name = mode.name().unwrap();
            assert_eq!(
                name.starts_with("Vertical"),
                mode.layer1_vertical(),
                "{mode}: {name}"
            );
            let layer2 = if name == UNUSED || name.starts_with("Boss") {
                Layer2Kind::None
            } else if name.contains("background") {
                Layer2Kind::Background
            } else {
                assert!(name.contains("layer 2"), "{mode}: {name}");
                let plain = name.replace("solid ", "");
                let vertical = if plain.contains("vertical layer 2") {
                    true
                } else if plain.contains("horizontal layer 2") {
                    false
                } else {
                    mode.layer1_vertical()
                };
                if vertical {
                    Layer2Kind::VerticalObjects
                } else {
                    Layer2Kind::HorizontalObjects
                }
            };
            assert_eq!(layer2, mode.layer2(), "{mode}: {name}");
        }
        assert_eq!(LevelMode(0x20).name(), None);
    }

    #[test]
    fn decoded_objects_find_their_table() {
        let standard = Object::Standard {
            number: 0x3C,
            x: 0,
            y: 0,
            settings: 0,
        };
        assert_eq!(object(&standard, 0x00), Some("Arches"));
        assert_eq!(object(&standard, 0x02), Some("Mushroom ledge"));
        let extended = Object::Extended {
            number: 0x41,
            x: 0,
            y: 0,
        };
        assert_eq!(object(&extended, 0x00), Some("Dragon coin"));
        let lunar = Object::Lunar {
            number: 0x22,
            x: 0,
            y: 0,
            data: vec![0, 0],
        };
        assert_eq!(object(&lunar, 0x00), Some("Direct Map16, page 0"));
        // A time limit (28) and a long screen exit (extended 02).
        assert_eq!(
            object(&Object::Unplaced(vec![0x40, 0x81, 0x03]), 0),
            Some("Time limit bypass")
        );
        let long_exit = Object::Unplaced(vec![0x00, 0x00, 0x02, 0x05, 0x06]);
        assert_eq!(
            object(&long_exit, 0),
            Some("Long screen exit (Lunar Magic)")
        );
        assert_eq!(sprite(0x0F), "Goomba");
        assert_eq!(music(0x08), None);
        assert_eq!(object_tileset(0x10), None);
        assert_eq!(sprite_tileset(0x0D), Some("Wendy and Lemmy"));
    }
}
