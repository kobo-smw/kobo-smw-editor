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
