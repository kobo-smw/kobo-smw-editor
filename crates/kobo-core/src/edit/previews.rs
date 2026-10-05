//! Pictures of objects as a level draws them, for a palette.
//!
//! Only the ROM's loader knows what an object looks like, and how depends
//! on the level: its tileset, its Map16 pages, its graphics and palette.
//! So the pictures come from the level itself: a copy of it with the
//! objects laid out apart on a grid and nothing else is built and loaded,
//! and each object's picture is the tiles it drew there
//! ([`crate::expand::ObjectMap`]), drawn from the copy's video memory.

use super::workspace::{Workspace, WorkspaceError};
use super::{ObjectLayer, object_at};
use crate::addr::SnesAddr;
use crate::expand::{self, ObjectRef};
use crate::image::RgbImage;
use crate::level::objects::Object;
use crate::level::size::LevelSize;
use crate::level::{Layer2Kind, LevelMode};
use crate::operation::Operation;
use crate::render::{LayerTiles, draw_map16_tile};
use crate::source::level::{Layer2, Level};

/// A cell of the grid, in tiles: room for an object at its default size
/// and the tiles some draw left of or below their place.
const CELL: (u16, u16) = (16, 13);
/// Where in its cell an object is placed.
const PLACE: (u16, u16) = (6, 3);
/// A horizontal level's screens, the most there are.
const SCREENS: u16 = 32;
/// The background the copy has when the level's own cannot be kept: the
/// vanilla one at `$0CD900`.
const BACKGROUND: u32 = 0x0C_D900;

/// The level the pictures are taken from: `base` with `objects` on its
/// grid and nothing else, horizontal, at the game's height.
fn sheet(base: &Level, objects: &[Object]) -> Level {
    let columns = SCREENS * 16 / CELL.0;
    let layer1 = objects
        .iter()
        .enumerate()
        .map(|(i, object)| {
            let (cx, cy) = (i as u16 % columns, i as u16 / columns);
            object_at(object, cx * CELL.0 + PLACE.0, cy * CELL.1 + PLACE.1)
        })
        .collect();
    let mode = base.header.level_mode;
    let keep_background = mode.layer2() == Layer2Kind::Background && !mode.layer1_vertical();
    let (level_mode, layer2) = if keep_background {
        (mode, base.layer2.clone())
    } else {
        (
            LevelMode(0x00),
            Layer2::VanillaBackground(SnesAddr::new(BACKGROUND)),
        )
    };
    let mut header = base.header;
    header.screens = SCREENS as u8;
    header.level_mode = level_mode;
    Level {
        header,
        size: LevelSize::default(),
        layer1,
        layer2,
        sprites: Default::default(),
        ..base.clone()
    }
}

/// How many objects one copy holds.
fn per_sheet() -> usize {
    let rows = (LevelSize::default().rows() as u16) / CELL.1;
    usize::from(SCREENS * 16 / CELL.0 * rows)
}

/// A picture of each of `objects` as level `number` of `workspace` would
/// draw it with `base`'s settings, on `background`; `None` for one that
/// draws nothing there, or that the level's loader cannot load. Placing
/// them takes one build and load for every [`per_sheet`] objects, and a
/// few more to find an object that stops a load.
pub fn object_previews(
    workspace: &Workspace,
    number: u16,
    base: &Level,
    objects: &[Object],
    background: [u8; 3],
    operation: &Operation,
) -> Result<Vec<Option<RgbImage>>, WorkspaceError> {
    let mut pictures = Vec::with_capacity(objects.len());
    let mut copy = workspace.clone();
    for batch in objects.chunks(per_sheet()) {
        pictures.extend(pictured(
            &mut copy, number, base, batch, background, operation,
        )?);
    }
    Ok(pictures)
}

/// The pictures of one sheet of objects. When the load fails, each half
/// is tried apart, down to the objects that fail alone.
fn pictured(
    copy: &mut Workspace,
    number: u16,
    base: &Level,
    batch: &[Object],
    background: [u8; 3],
    operation: &Operation,
) -> Result<Vec<Option<RgbImage>>, WorkspaceError> {
    match sheet_pictures(copy, number, base, batch, background, operation) {
        Ok(pictures) => Ok(pictures),
        Err(e @ WorkspaceError::Render(crate::render::RenderError::Operation(_))) => Err(e),
        Err(_) if batch.len() == 1 => Ok(vec![None]),
        Err(_) => {
            let (a, b) = batch.split_at(batch.len() / 2);
            let mut pictures = pictured(copy, number, base, a, background, operation)?;
            pictures.extend(pictured(copy, number, base, b, background, operation)?);
            Ok(pictures)
        }
    }
}

fn sheet_pictures(
    copy: &mut Workspace,
    number: u16,
    base: &Level,
    batch: &[Object],
    background: [u8; 3],
    operation: &Operation,
) -> Result<Vec<Option<RgbImage>>, WorkspaceError> {
    copy.set_level(number, &sheet(base, batch));
    let rom = copy.build()?;
    operation
        .check()
        .map_err(crate::render::RenderError::from)?;
    let loaded = expand::expand_level_with_control(&rom, number, operation)
        .map_err(crate::render::RenderError::from)?;
    let tiles = LayerTiles::from_vram(&loaded.video.vram);
    let palette = loaded.video.palette();
    let pictures = (0..batch.len())
        .map(|index| {
            let object = ObjectRef {
                layer: ObjectLayer::One,
                index,
            };
            let [x0, y0, w, h] = loaded.objects.bounds(&loaded.tiles, object)?;
            let mut image = RgbImage::new(w as u32 * 16, h as u32 * 16);
            image.pixels.fill(background);
            for (x, y) in loaded.objects.tiles(&loaded.tiles, object) {
                // A tile another object drew over is that one's.
                let offset = loaded.tiles.offset(x, y);
                if loaded.objects.owner(offset) != Some(object) {
                    continue;
                }
                let n = loaded.tiles.tile_at(x, y);
                if let Some(tile) = loaded.tiles.map16_at(n, x, y) {
                    let (px, py) = ((x - x0) as u32 * 16, (y - y0) as u32 * 16);
                    draw_map16_tile(&mut image, px, py, tile, &tiles, &palette);
                }
            }
            Some(image)
        })
        .collect();
    Ok(pictures)
}

/// Sprites one sheet holds: one every [`SPRITE_SPACING`] tiles along 32
/// screens.
const SPRITES_PER_SHEET: usize = 64;
/// Tiles between sprites on a sheet, so that one does not reach another.
const SPRITE_SPACING: u16 = 8;
/// The row sprites stand on: on the ground the sheet has at row 24.
const SPRITE_ROW: u16 = 22;

/// The level sprite pictures come from: `base` with flat ground and
/// `ids` spaced along it, nothing else.
fn sprite_sheet(base: &Level, ids: &[u8]) -> Level {
    // Two long ledges cover the 512 columns.
    let ground = |x| Object::Standard {
        number: 0x21,
        x,
        y: 24,
        settings: 0xFF,
    };
    let objects = [ground(0), ground(256)];
    let mut level = sheet(base, &objects);
    // The sheet's objects were laid on its grid; put them back.
    level.layer1 = objects.to_vec();
    level.sprites.list = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| crate::source::level::Sprite {
            id,
            x: i as u16 * SPRITE_SPACING + 4,
            y: SPRITE_ROW,
            extra_bits: 0,
            extension: Vec::new(),
        })
        .collect();
    level
}

/// A picture of each of `ids` as level `number` of `workspace` would draw
/// it on its first frame, with `base`'s settings: its sprite graphics,
/// palette, and background, cut from a render of them standing apart;
/// `None` for one that draws nothing (a generator, a scroll command).
pub fn sprite_previews(
    workspace: &Workspace,
    number: u16,
    base: &Level,
    ids: &[u8],
    operation: &Operation,
) -> Result<Vec<Option<RgbImage>>, WorkspaceError> {
    use crate::render::{RenderOptions, Sprites};
    let mut pictures = Vec::with_capacity(ids.len());
    let mut copy = workspace.clone();
    for batch in ids.chunks(SPRITES_PER_SHEET) {
        let level = sprite_sheet(base, batch);
        copy.set_level(number, &level);
        let options = RenderOptions {
            sprites: Sprites::Drawn,
            player: false,
            hidden_layers: 0,
        };
        let preview = copy.preview(number, options, operation)?;
        let render = preview.render;
        let sizes = crate::expand::object_sizes(render.level.video.object_select);
        let captures = render.sprites.map(|s| s.captures).unwrap_or_default();
        for sprite in &level.sprites.list {
            let capture = captures.iter().find(|c| {
                c.id == sprite.id && c.x == i32::from(sprite.x) && c.y == i32::from(sprite.y)
            });
            let picture = capture.and_then(|c| {
                let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for object in &c.objects {
                    let (w, h) = sizes[usize::from(object.large)];
                    x0 = x0.min(object.x);
                    y0 = y0.min(object.y);
                    x1 = x1.max(object.x + w);
                    y1 = y1.max(object.y + h);
                }
                crop(&render.image, x0, y0, x1, y1)
            });
            pictures.push(picture);
        }
    }
    Ok(pictures)
}

/// Layers a background's picture leaves out: all but layer 2.
const ALL_BUT_LAYER2: u8 = 1 | 4 | 16;

/// A picture of layer 2 as level `number` of `workspace` would draw it with
/// `base`'s settings and `layer2` for it: two screens of the level with
/// nothing else in it, and nothing else drawn.
pub fn background_preview(
    workspace: &Workspace,
    number: u16,
    base: &Level,
    layer2: &Layer2,
    operation: &Operation,
) -> Result<RgbImage, WorkspaceError> {
    use crate::render::{RenderOptions, Sprites};
    let mut level = Level {
        layer1: Vec::new(),
        layer2: layer2.clone(),
        sprites: crate::source::level::Sprites {
            list: Vec::new(),
            ..base.sprites.clone()
        },
        ..base.clone()
    };
    level.header.screens = 2;
    level.entrance.entrance_screen = 0;
    let mut copy = workspace.clone();
    copy.set_level(number, &level);
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
        hidden_layers: ALL_BUT_LAYER2,
    };
    Ok(copy.preview(number, options, operation)?.render.image)
}

/// The part of `image` from (`x0`, `y0`) to (`x1`, `y1`), clipped to it;
/// `None` if nothing is left.
fn crop(image: &RgbImage, x0: i32, y0: i32, x1: i32, y1: i32) -> Option<RgbImage> {
    let (w, h) = (image.width as i32, image.height as i32);
    let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w), y1.min(h));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut out = RgbImage::new((x1 - x0) as u32, (y1 - y0) as u32);
    for y in y0..y1 {
        for x in x0..x1 {
            out.pixels[((y - y0) * (x1 - x0) + (x - x0)) as usize] =
                image.pixels[(y * w + x) as usize];
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_the_level() {
        let rows = LevelSize::default().rows() as u16;
        let n = per_sheet() as u16;
        let columns = SCREENS * 16 / CELL.0;
        let last_row = (n - 1) / columns;
        assert!(last_row * CELL.1 + PLACE.1 < rows);
        assert_eq!(n, 64);
    }
}
