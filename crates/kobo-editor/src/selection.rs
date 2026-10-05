//! What can be selected, and where it is in the picture.

use eframe::egui::{Pos2, Rect, Vec2};
use kobo_core::edit::{self, ObjectLayer};
use kobo_core::expand::{LoadedLevel, ObjectRef, object_sizes};
use kobo_core::source::level::Level;
use kobo_core::video::SpriteScene;

/// A selectable thing in a level.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Item {
    Object(ObjectRef),
    /// A sprite, by its index in the level's list.
    Sprite(usize),
}

impl Item {
    pub fn object(layer: ObjectLayer, index: usize) -> Self {
        Self::Object(ObjectRef { layer, index })
    }

    /// Whether the level still has it.
    pub fn exists(self, level: &Level) -> bool {
        match self {
            Item::Object(o) => objects(level, o.layer).is_some_and(|list| o.index < list.len()),
            Item::Sprite(i) => i < level.sprites.list.len(),
        }
    }
}

/// A layer's object list, if the level has one.
pub fn objects(
    level: &Level,
    layer: ObjectLayer,
) -> Option<&Vec<kobo_core::level::objects::Object>> {
    match layer {
        ObjectLayer::One => Some(&level.layer1),
        ObjectLayer::Two => match &level.layer2 {
            kobo_core::source::level::Layer2::Objects(list) => Some(list),
            _ => None,
        },
    }
}

/// What a picture says of where things are: the load it was drawn from,
/// and the level as it was then, whose indices its objects and sprites
/// have.
pub struct Geometry {
    pub loaded: LoadedLevel,
    pub level: Level,
    /// Each sprite's box in level pixels: what the sprite capture drew
    /// for it, or its tile.
    pub sprites: Vec<Rect>,
}

const TILE: f32 = 16.0;

impl Geometry {
    pub fn new(loaded: LoadedLevel, scene: Option<SpriteScene>, level: Level) -> Self {
        let sizes = object_sizes(loaded.video.object_select);
        let mut captures: Vec<Option<Rect>> = Vec::new();
        let mut used = Vec::new();
        if let Some(scene) = &scene {
            for capture in &scene.captures {
                let mut bounds: Option<Rect> = None;
                for object in &capture.objects {
                    let (w, h) = sizes[usize::from(object.large)];
                    let r = Rect::from_min_size(
                        Pos2::new(object.x as f32, object.y as f32),
                        Vec2::new(w as f32, h as f32),
                    );
                    bounds = Some(bounds.map_or(r, |b| b.union(r)));
                }
                captures.push(bounds);
                used.push(false);
            }
        }
        let sprites = level
            .sprites
            .list
            .iter()
            .map(|sprite| {
                let tile = Rect::from_min_size(
                    Pos2::new(sprite.x as f32 * TILE, sprite.y as f32 * TILE),
                    Vec2::splat(TILE),
                );
                let found = scene.as_ref().and_then(|scene| {
                    scene.captures.iter().enumerate().position(|(i, c)| {
                        !used[i]
                            && c.id == sprite.id
                            && c.x == i32::from(sprite.x)
                            && c.y == i32::from(sprite.y)
                    })
                });
                match found {
                    Some(i) => {
                        used[i] = true;
                        // Keep the tile in the box, for a sprite drawn
                        // away from where it stands.
                        captures[i].map_or(tile, |b| b.union(tile))
                    }
                    None => tile,
                }
            })
            .collect();
        Self {
            loaded,
            level,
            sprites,
        }
    }

    /// The tiles an object drew, as level-pixel rectangles.
    pub fn object_tiles(&self, object: ObjectRef) -> Vec<Rect> {
        self.loaded
            .objects
            .tiles(&self.loaded.tiles, object)
            .into_iter()
            .map(|(x, y)| tile_rect(x as i32, y as i32))
            .collect()
    }

    /// An item's box in level pixels; for an object that drew nothing,
    /// the tile it is placed at.
    pub fn bounds(&self, item: Item) -> Option<Rect> {
        match item {
            Item::Sprite(i) => self.sprites.get(i).copied(),
            Item::Object(object) => match self.loaded.objects.bounds(&self.loaded.tiles, object) {
                Some([x, y, w, h]) => Some(Rect::from_min_size(
                    Pos2::new(x as f32 * TILE, y as f32 * TILE),
                    Vec2::new(w as f32 * TILE, h as f32 * TILE),
                )),
                None => {
                    let list = objects(&self.level, object.layer)?;
                    let placed = list.get(object.index)?;
                    // A screen exit stands for its whole screen.
                    if let kobo_core::level::objects::Object::ScreenExit(exit) = placed {
                        let size = self.size();
                        let start = f32::from(exit.screen) * 256.0;
                        return Some(if self.loaded.tiles.vertical {
                            Rect::from_min_size(Pos2::new(0.0, start), Vec2::new(size.x, 256.0))
                        } else {
                            Rect::from_min_size(Pos2::new(start, 0.0), Vec2::new(256.0, size.y))
                        });
                    }
                    let (x, y) = edit::object_position(placed)?;
                    Some(tile_rect(i32::from(x), i32::from(y)))
                }
            },
        }
    }

    /// What is under a point, front first: a sprite, then layer 1's
    /// object, then layer 2's.
    pub fn item_at(&self, at: Pos2, sprites: bool) -> Option<Item> {
        if sprites {
            // Later sprites are drawn behind earlier ones; take the
            // smallest box under the point, the most specific.
            let hit = self
                .sprites
                .iter()
                .enumerate()
                .filter(|(_, r)| r.contains(at))
                .min_by(|a, b| a.1.area().total_cmp(&b.1.area()));
            if let Some((i, _)) = hit {
                return Some(Item::Sprite(i));
            }
        }
        if at.x < 0.0 || at.y < 0.0 {
            return None;
        }
        let (x, y) = ((at.x / TILE) as usize, (at.y / TILE) as usize);
        let tiles = &self.loaded.tiles;
        [ObjectLayer::One, ObjectLayer::Two]
            .into_iter()
            .find_map(|layer| self.loaded.objects.owner_at(tiles, layer, x, y))
            .map(Item::Object)
    }

    /// Every item whose box meets `area`.
    pub fn items_in(&self, area: Rect, sprites: bool) -> Vec<Item> {
        let mut items = Vec::new();
        for layer in [ObjectLayer::One, ObjectLayer::Two] {
            let count = objects(&self.level, layer).map_or(0, Vec::len);
            for index in 0..count {
                let item = Item::object(layer, index);
                // What has no place (screen exits) is not caught by a box.
                let placed = objects(&self.level, layer)
                    .and_then(|l| l.get(index))
                    .and_then(edit::object_position)
                    .is_some();
                if placed && self.bounds(item).is_some_and(|b| b.intersects(area)) {
                    items.push(item);
                }
            }
        }
        if sprites {
            for (i, r) in self.sprites.iter().enumerate() {
                if r.intersects(area) {
                    items.push(Item::Sprite(i));
                }
            }
        }
        items
    }

    /// The level's size in pixels, as loaded.
    pub fn size(&self) -> Vec2 {
        let (w, h) = self.loaded.tiles.size();
        Vec2::new(w as f32 * TILE, h as f32 * TILE)
    }
}

pub fn tile_rect(x: i32, y: i32) -> Rect {
    Rect::from_min_size(
        Pos2::new(x as f32 * TILE, y as f32 * TILE),
        Vec2::splat(TILE),
    )
}

/// A name for an item, as the inspector and status bar show it.
pub fn describe(level: &Level, item: Item) -> String {
    let tileset = level.header.object_tileset;
    match item {
        Item::Object(o) => {
            let Some(object) = objects(level, o.layer).and_then(|l| l.get(o.index)) else {
                return String::new();
            };
            kobo_core::names::object(object, tileset)
                .unwrap_or("Object")
                .to_string()
        }
        Item::Sprite(i) => level
            .sprites
            .list
            .get(i)
            .map(|s| kobo_core::names::sprite(s.id).to_string())
            .unwrap_or_default(),
    }
}
