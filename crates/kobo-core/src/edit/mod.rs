//! Editing a project: the changes the editor makes and a script can make,
//! as values applied to an open level file.
//!
//! A [`LevelDocument`] is one level file, open: the [`Level`] and the
//! user's [`Comments`] as [`Level::from_toml`] read them. An [`Edit`]
//! changes the level and moves the comments before an entry with it;
//! saving writes [`Level::to_toml`], so a file saved here is in Kobo's
//! format. Every change, an outside one read back from disk included, is
//! one undo step, kept as a snapshot of the level and its comments.

mod document;
mod workspace;

pub use document::{LevelDocument, Reload};
pub use workspace::{Preview, Workspace, WorkspaceError};

use thiserror::Error;

use crate::level::objects::{Layout, Object};
use crate::level::{Layer2Kind, PrimaryHeader, SecondaryHeader};
use crate::source::SourceError;
use crate::source::level::{Layer2, Level, Sprite};

pub use crate::level::objects::ObjectLayer;

/// The array a layer's objects are in, as
/// [`Comments`](crate::source::Comments) names it.
pub fn object_list(layer: ObjectLayer) -> &'static str {
    match layer {
        ObjectLayer::One => "layer1.objects",
        ObjectLayer::Two => "layer2.objects",
    }
}

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
    SetHeader(PrimaryHeader),
    SetEntrance(SecondaryHeader),
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
            Edit::SetHeader(header) => level.header = *header,
            Edit::SetEntrance(entrance) => level.entrance = *entrance,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
