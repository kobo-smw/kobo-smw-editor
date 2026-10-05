//! Editing a project: the changes the editor makes and a script can make,
//! as values applied to an open level file.
//!
//! A [`LevelDocument`] is one level file, open: the [`Level`] and the
//! user's [`Comments`] as [`Level::from_toml`] read them. An [`Edit`]
//! changes the level and moves the comments before an entry with it;
//! saving writes [`Level::to_toml`], so a file saved here is in Kobo's
//! format. Every change, an outside one read back from disk included, is
//! one undo step, kept as a snapshot of the level and its comments.

pub mod diff;
mod document;
mod previews;
mod workspace;

pub use document::{LevelDocument, Reload};
pub use previews::{object_previews, sprite_previews};
pub use workspace::{Preview, Workspace, WorkspaceError};

use thiserror::Error;

use crate::entrance::LevelSettings;
use crate::level::objects::ScreenExit;
use crate::level::objects::{Layout, Object};
use crate::level::size::LevelSize;
use crate::level::{Layer2Kind, PrimaryHeader, SecondaryHeader};
use crate::source::SourceError;
use crate::source::level::{Entrance, Layer2, Level, Sprite};

pub use crate::level::objects::ObjectLayer;

/// The array a layer's objects are in, as
/// [`Comments`](crate::source::Comments) names it.
pub fn object_list(layer: ObjectLayer) -> &'static str {
    match layer {
        ObjectLayer::One => "layer1.objects",
        ObjectLayer::Two => "layer2.objects",
    }
}

/// The array a level's secondary entrances are in.
pub const ENTRANCE_LIST: &str = "entrances.list";

/// The array a level's sprites are in.
pub const SPRITE_LIST: &str = "sprites.list";

/// One change to a level.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Edit {
    /// Puts `object` at `index` of the layer's list, before the entry
    /// there; `index` may be the list's length.
    InsertObject {
        layer: ObjectLayer,
        index: usize,
        object: Object,
    },
    RemoveObject {
        layer: ObjectLayer,
        index: usize,
    },
    /// Changes an object where it is in the list: its place, its size, or
    /// what it is.
    ReplaceObject {
        layer: ObjectLayer,
        index: usize,
        object: Object,
    },
    /// Moves an object within the list, which changes what it draws over
    /// and what draws over it.
    ReorderObject {
        layer: ObjectLayer,
        from: usize,
        to: usize,
    },
    InsertSprite {
        index: usize,
        sprite: Sprite,
    },
    RemoveSprite {
        index: usize,
    },
    ReplaceSprite {
        index: usize,
        sprite: Sprite,
    },
    /// Moves a sprite within the list. The game's loader stops at the
    /// first sprite past the screen it loads, so the list must stay in
    /// screen order; [`move_sprites`] and [`insert_sprite`] keep it so.
    ReorderSprite {
        from: usize,
        to: usize,
    },
    SetHeader(PrimaryHeader),
    SetEntrance(SecondaryHeader),
    /// Lunar Magic's level size: a horizontal level's height.
    SetSize(LevelSize),
    /// Lunar Magic's settings for the level and its entrances.
    SetSettings(LevelSettings),
    /// Changes one of the secondary entrances that lead into the level.
    ReplaceEntrance {
        index: usize,
        entrance: Entrance,
    },
    /// Adds a secondary entrance into the level; its number must be one
    /// no other level has ([`Workspace::free_entrance`]).
    InsertEntrance {
        index: usize,
        entrance: Entrance,
    },
    RemoveEntrance {
        index: usize,
    },
}

#[derive(Debug, Error)]
pub enum EditError {
    #[error("{what} {index} does not exist; the list has {len}")]
    NoEntry {
        what: &'static str,
        index: usize,
        len: usize,
    },
    #[error("the level has no layer 2 objects")]
    NoLayer2,
    #[error("({x}, {y}) is outside the level, which is {width} by {height} tiles")]
    Outside {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
    #[error("{0}")]
    Source(#[from] SourceError),
    #[error("{path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A level as a copy of it under another number starts: the same but for
/// the secondary entrances into it, which are each one level's by their
/// number, and stay with the original.
pub fn copy_of(level: &Level) -> Level {
    Level {
        entrances: Vec::new(),
        ..level.clone()
    }
}

/// What a screen exit leads to, whichever format it is kept in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitTarget {
    /// The screen it is taken from, 0 to 31.
    pub screen: u8,
    /// A level (`000`-`1FF`), or with `secondary` a secondary entrance
    /// (up to `1FFF` in Lunar Magic's format).
    pub destination: u16,
    pub secondary: bool,
    /// In Lunar Magic's format: water, for a secondary exit; the midway
    /// entrance, for a normal one.
    pub water: bool,
}

impl ExitTarget {
    /// What `exit`, in level `level`, leads to: in the game's format the
    /// destination's bit 8 is the level's own.
    pub fn of(exit: ScreenExit, level: u16) -> Self {
        let lunar = exit.in_lunar_magic_format(level);
        Self {
            screen: exit.screen,
            destination: lunar.lunar_magic_destination(),
            secondary: exit.flags & ScreenExit::SECONDARY != 0,
            water: lunar.flags & ScreenExit::WATER != 0,
        }
    }

    /// The exit that leads here from level `level`: in the game's format
    /// when `lunar` is false and the game's can say it, so that changing
    /// an exit does not alone make a build install Lunar Magic's layout;
    /// in Lunar Magic's otherwise.
    pub fn exit(self, level: u16, lunar: bool) -> ScreenExit {
        let flags = if self.water { ScreenExit::WATER } else { 0 }
            | if self.secondary {
                ScreenExit::SECONDARY
            } else {
                0
            };
        let exit = ScreenExit::lunar_magic(self.screen, flags, self.destination);
        match exit.in_game_format(level) {
            Some(game) if !lunar => game,
            _ => exit,
        }
    }
}

/// The screen the game's sprite loader files a sprite under: its column
/// of screens in a horizontal level, its row of them in a vertical one.
pub fn sprite_screen(level: &Level, sprite: &Sprite) -> u16 {
    if level.header.level_mode.layer1_vertical() {
        sprite.y / 16
    } else {
        sprite.x / 16
    }
}

/// Where a sprite goes in `list` (which does not hold it) for the
/// loader to reach it: after the last sprite on its screen or an earlier
/// one. Sprites the user has put out of that order stay as they are.
fn sprite_slot(level: &Level, list: &[Sprite], sprite: &Sprite) -> usize {
    let screen = sprite_screen(level, sprite);
    list.iter()
        .rposition(|other| sprite_screen(level, other) <= screen)
        .map_or(0, |i| i + 1)
}

/// The edit that adds `sprite` where the loader reaches it, and the index
/// it will have.
pub fn insert_sprite(level: &Level, sprite: Sprite) -> (Edit, usize) {
    let index = sprite_slot(level, &level.sprites.list, &sprite);
    (Edit::InsertSprite { index, sprite }, index)
}

/// The edits that change sprites (each `(index, sprite)` replacing the
/// entry at `index`), moving each one whose screen changes to where the
/// loader reaches it; and the index each ends up at, in the same order.
pub fn move_sprites(
    level: &Level,
    moves: &[(usize, Sprite)],
) -> Result<(Vec<Edit>, Vec<usize>), EditError> {
    let mut scratch = level.clone();
    let mut comments = crate::source::Comments::default();
    let mut edits = Vec::new();
    // Where each moved sprite is now, as earlier moves shift the list.
    let mut at: Vec<usize> = moves.iter().map(|(i, _)| *i).collect();
    for (n, (_, sprite)) in moves.iter().enumerate() {
        let index = at[n];
        let replace = Edit::ReplaceSprite {
            index,
            sprite: sprite.clone(),
        };
        replace.apply(&mut scratch, &mut comments)?;
        edits.push(replace);
        let list = &scratch.sprites.list;
        let unmoved = sprite_screen(level, &level.sprites.list[moves[n].0]);
        if sprite_screen(&scratch, sprite) == unmoved {
            continue;
        }
        let mut rest = list.clone();
        rest.remove(index);
        let to = sprite_slot(&scratch, &rest, sprite);
        if to != index {
            let reorder = Edit::ReorderSprite { from: index, to };
            reorder.apply(&mut scratch, &mut comments)?;
            edits.push(reorder);
            for other in &mut at {
                *other = if *other == index {
                    to
                } else if index < to && (index + 1..=to).contains(other) {
                    *other - 1
                } else if to < index && (to..index).contains(other) {
                    *other + 1
                } else {
                    *other
                };
            }
        }
    }
    Ok((edits, at))
}

/// The line of `text`, a level file in Kobo's format, that holds entry
/// `index` of array `list` (`layer1.objects`), counted from 0.
pub fn entry_line(text: &str, list: &str, index: usize) -> Option<usize> {
    let (table, key) = list.split_once('.')?;
    let header = format!("[{table}]");
    let opening = format!("{key} = [");
    let mut lines = text.lines().enumerate();
    lines.find(|(_, line)| *line == header)?;
    lines.find(|(_, line)| *line == opening)?;
    lines
        .take_while(|(_, line)| *line != "]")
        .filter(|(_, line)| line.trim_start().starts_with('{'))
        .nth(index)
        .map(|(n, _)| n)
}

/// One value a standard object's settings byte holds, as its level file
/// names it: `width` and `height` in tiles, `type`, `length`, or the whole
/// byte as `settings` where the object's handler is tileset-specific.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SettingField {
    pub name: &'static str,
    pub value: u16,
    pub min: u16,
    pub max: u16,
}

/// The values a standard object's settings byte holds
/// ([`Settings`](crate::level::objects::Settings)).
pub fn setting_fields(number: u8, settings: u8) -> Vec<SettingField> {
    use crate::level::objects::Settings;
    let (hi, lo) = (u16::from(settings >> 4), u16::from(settings & 0x0F));
    let field = |name, value, min, max| SettingField {
        name,
        value,
        min,
        max,
    };
    let height = field("height", hi + 1, 1, 16);
    let width = field("width", lo + 1, 1, 16);
    match Settings::of(number) {
        Settings::HeightWidth => vec![height, width],
        Settings::HeightType => vec![height, field("type", lo, 0, 15)],
        Settings::TypeWidth => vec![field("type", hi, 0, 15), width],
        Settings::Height => vec![height],
        Settings::Width => vec![width],
        Settings::Length => vec![field("length", u16::from(settings) + 1, 1, 256)],
        Settings::Raw => vec![field("settings", u16::from(settings), 0, 255)],
    }
}

/// The settings byte with field `name` set to `value`, clamped to the
/// field's range; the byte as it was for a field the object does not
/// have.
pub fn with_setting(number: u8, settings: u8, name: &str, value: u16) -> u8 {
    use crate::level::objects::Settings;
    let Some(field) = setting_fields(number, settings)
        .into_iter()
        .find(|f| f.name == name)
    else {
        return settings;
    };
    let value = value.clamp(field.min, field.max);
    match name {
        "height" => (settings & 0x0F) | (((value - 1) as u8) << 4),
        "width" => (settings & 0xF0) | (value - 1) as u8,
        // A type and a width keep the type in the high nibble.
        "type" if Settings::of(number) == Settings::TypeWidth => {
            (settings & 0x0F) | ((value as u8) << 4)
        }
        "type" => (settings & 0xF0) | value as u8,
        "length" => (value - 1) as u8,
        _ => value as u8,
    }
}

/// Where an object is placed, in tiles, if it has a place.
pub fn object_position(object: &Object) -> Option<(u16, u16)> {
    match *object {
        Object::Standard { x, y, .. }
        | Object::Extended { x, y, .. }
        | Object::Lunar { x, y, .. } => Some((x, y)),
        Object::ScreenExit(_) | Object::Unplaced(_) => None,
    }
}

/// The object placed at (`x`, `y`) instead; one with no place is as it
/// was.
pub fn object_at(object: &Object, x: u16, y: u16) -> Object {
    let mut object = object.clone();
    match &mut object {
        Object::Standard { x: ox, y: oy, .. }
        | Object::Extended { x: ox, y: oy, .. }
        | Object::Lunar { x: ox, y: oy, .. } => {
            *ox = x;
            *oy = y;
        }
        Object::ScreenExit(_) | Object::Unplaced(_) => {}
    }
    object
}

/// How a layer of the level places its objects.
pub fn layout(level: &Level, layer: ObjectLayer) -> Layout {
    let mode = level.header.level_mode;
    let vertical = match layer {
        ObjectLayer::One => mode.layer1_vertical(),
        ObjectLayer::Two => mode.layer2() == Layer2Kind::VerticalObjects,
    };
    if vertical {
        Layout::Vertical
    } else {
        Layout::Horizontal
    }
}

/// The size of a layer of the level in tiles, (width, height): what its
/// objects and sprites may be placed in.
pub fn layer_size(level: &Level, layer: ObjectLayer) -> (u16, u16) {
    let screens = u16::from(level.header.screens.max(1));
    match layout(level, layer) {
        Layout::Horizontal => (screens * 16, level.size.rows() as u16),
        Layout::Vertical => (32, screens * 16),
    }
}

fn objects_mut(level: &mut Level, layer: ObjectLayer) -> Result<&mut Vec<Object>, EditError> {
    match layer {
        ObjectLayer::One => Ok(&mut level.layer1),
        ObjectLayer::Two => match &mut level.layer2 {
            Layer2::Objects(list) => Ok(list),
            _ => Err(EditError::NoLayer2),
        },
    }
}

fn check_index(what: &'static str, index: usize, len: usize) -> Result<(), EditError> {
    if index < len {
        Ok(())
    } else {
        Err(EditError::NoEntry { what, index, len })
    }
}

fn check_place(level: &Level, layer: ObjectLayer, at: Option<(u16, u16)>) -> Result<(), EditError> {
    let Some((x, y)) = at else { return Ok(()) };
    let (width, height) = layer_size(level, layer);
    if x < width && y < height {
        Ok(())
    } else {
        Err(EditError::Outside {
            x,
            y,
            width,
            height,
        })
    }
}

impl Edit {
    /// Applies the edit to `level`, moving `comments` with their entries.
    /// On an error nothing has changed.
    pub fn apply(
        &self,
        level: &mut Level,
        comments: &mut crate::source::Comments,
    ) -> Result<(), EditError> {
        match self {
            Edit::InsertObject {
                layer,
                index,
                object,
            } => {
                check_place(level, *layer, object_position(object))?;
                let list = objects_mut(level, *layer)?;
                check_index("object", *index, list.len() + 1)?;
                list.insert(*index, object.clone());
                comments.insert_entry(object_list(*layer), *index);
            }
            Edit::RemoveObject { layer, index } => {
                let list = objects_mut(level, *layer)?;
                check_index("object", *index, list.len())?;
                let len = list.len();
                list.remove(*index);
                comments.remove_entry(object_list(*layer), *index, len);
            }
            Edit::ReplaceObject {
                layer,
                index,
                object,
            } => {
                check_place(level, *layer, object_position(object))?;
                let list = objects_mut(level, *layer)?;
                check_index("object", *index, list.len())?;
                list[*index] = object.clone();
            }
            Edit::ReorderObject { layer, from, to } => {
                let list = objects_mut(level, *layer)?;
                check_index("object", *from, list.len())?;
                check_index("object", *to, list.len())?;
                let object = list.remove(*from);
                list.insert(*to, object);
                comments.move_entry(object_list(*layer), *from, *to);
            }
            Edit::InsertSprite { index, sprite } => {
                check_place(level, ObjectLayer::One, Some((sprite.x, sprite.y)))?;
                let list = &mut level.sprites.list;
                check_index("sprite", *index, list.len() + 1)?;
                list.insert(*index, sprite.clone());
                comments.insert_entry(SPRITE_LIST, *index);
            }
            Edit::RemoveSprite { index } => {
                let list = &mut level.sprites.list;
                check_index("sprite", *index, list.len())?;
                let len = list.len();
                list.remove(*index);
                comments.remove_entry(SPRITE_LIST, *index, len);
            }
            Edit::ReplaceSprite { index, sprite } => {
                check_place(level, ObjectLayer::One, Some((sprite.x, sprite.y)))?;
                let list = &mut level.sprites.list;
                check_index("sprite", *index, list.len())?;
                list[*index] = sprite.clone();
            }
            Edit::ReorderSprite { from, to } => {
                let list = &mut level.sprites.list;
                check_index("sprite", *from, list.len())?;
                check_index("sprite", *to, list.len())?;
                let sprite = list.remove(*from);
                list.insert(*to, sprite);
                comments.move_entry(SPRITE_LIST, *from, *to);
            }
            Edit::SetHeader(header) => level.header = *header,
            Edit::SetEntrance(entrance) => level.entrance = *entrance,
            Edit::SetSize(size) => level.size = *size,
            Edit::SetSettings(settings) => level.settings = *settings,
            Edit::ReplaceEntrance { index, entrance } => {
                let list = &mut level.entrances;
                check_index("entrance", *index, list.len())?;
                list[*index] = *entrance;
            }
            Edit::InsertEntrance { index, entrance } => {
                let list = &mut level.entrances;
                check_index("entrance", *index, list.len() + 1)?;
                list.insert(*index, *entrance);
                comments.insert_entry(ENTRANCE_LIST, *index);
            }
            Edit::RemoveEntrance { index } => {
                let list = &mut level.entrances;
                check_index("entrance", *index, list.len())?;
                let len = list.len();
                list.remove(*index);
                comments.remove_entry(ENTRANCE_LIST, *index, len);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
