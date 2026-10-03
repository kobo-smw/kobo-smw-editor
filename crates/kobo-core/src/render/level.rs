//! Rendering a level in one call: load it, capture its sprites, and draw
//! everything in the order the picture needs.

use thiserror::Error;

use super::level_layers;
use super::{LayerTiles, compose_level, draw_objects, draw_sprite_marker, draw_sprite_scene};
use crate::expand::{self, Diagnostic, ExpandError, LoadedLevel};
use crate::image::RgbImage;
use crate::operation::{Operation, Stage};
use crate::rom::Rom;
use crate::sprites::{self, SpriteError};
use crate::video::{SpriteScene, UndrawnSprite};

/// How a level's sprites are shown.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Sprites {
    /// As the game's sprite engine draws them, with an ID marker for
    /// every entry that draws nothing.
    #[default]
    Drawn,
    /// Every entry as an ID marker, without running the sprite engine.
    Markers,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderOptions {
    pub sprites: Sprites,
    /// Draw the player where the level is entered.
    pub player: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            sprites: Sprites::Drawn,
            player: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    Operation(#[from] crate::operation::OperationError),
    #[error(transparent)]
    Expand(#[from] ExpandError),
    #[error(transparent)]
    Sprites(#[from] SpriteError),
}

/// A rendered level, with what it was rendered from.
#[derive(Debug)]
pub struct LevelRender {
    pub image: RgbImage,
    pub level: LoadedLevel,
    /// The sprites drawn into the picture, when they were
    /// ([`Sprites::Drawn`] in an ordinary level).
    pub sprites: Option<SpriteScene>,
    /// Every pass the CPU core gave up on, from loading and from the
    /// sprite capture; see [`expand::summarize`].
    pub diagnostics: Vec<Diagnostic>,
}

/// Loads `level` by running the ROM's own code and renders it.
pub fn render_level(
    rom: &Rom,
    level: u16,
    options: RenderOptions,
) -> Result<LevelRender, RenderError> {
    render_controlled(rom, level, options, None)
}

/// Like [`render_level`], with one control shared by loading, sprite capture,
/// and both CPUs. Cancellation and exhausted budgets fail the operation;
/// they never return a successful but partial image. Poll the handle from
/// another thread for progress. Pixel composition checks at phase boundaries.
pub fn render_level_with_control(
    rom: &Rom,
    level: u16,
    options: RenderOptions,
    operation: &Operation,
) -> Result<LevelRender, RenderError> {
    render_controlled(rom, level, options, Some(operation))
}

fn render_controlled(
    rom: &Rom,
    level: u16,
    options: RenderOptions,
    operation: Option<&Operation>,
) -> Result<LevelRender, RenderError> {
    let level = expand::expand_controlled(rom, level, false, operation)?.0;
    let (image, scene_diagnostics, sprites) = loaded_controlled(rom, &level, options, operation)?;
    let mut diagnostics = level.diagnostics.clone();
    diagnostics.extend(scene_diagnostics);
    Ok(LevelRender {
        image,
        level,
        sprites,
        diagnostics,
    })
}

/// Renders a level that is already loaded, returning the picture and the
/// sprite capture's diagnostics. Graphics and colours come from what the
/// game uploaded to VRAM and CGRAM, so ExGFX, custom palettes, and
/// animated tiles are covered. A boss arena is its own fixed screen,
/// whose drawing pass already holds its sprites and the player.
pub fn render_loaded(
    rom: &Rom,
    level: &LoadedLevel,
    options: RenderOptions,
) -> Result<(RgbImage, Vec<Diagnostic>), RenderError> {
    let (image, diagnostics, _) = loaded_controlled(rom, level, options, None)?;
    Ok((image, diagnostics))
}

/// Renders a loaded level with cancellation, progress, and an instruction budget.
pub fn render_loaded_with_control(
    rom: &Rom,
    level: &LoadedLevel,
    options: RenderOptions,
    operation: &Operation,
) -> Result<(RgbImage, Vec<Diagnostic>), RenderError> {
    let (image, diagnostics, _) = loaded_controlled(rom, level, options, Some(operation))?;
    Ok((image, diagnostics))
}

fn loaded_controlled(
    rom: &Rom,
    level: &LoadedLevel,
    options: RenderOptions,
    operation: Option<&Operation>,
) -> Result<(RgbImage, Vec<Diagnostic>, Option<SpriteScene>), RenderError> {
    if let Some(op) = operation {
        op.check()?;
    }
    let video = &level.video;
    let mut layers = level_layers(level, &LayerTiles::from_vram(&video.vram));
    let mut markers: Vec<UndrawnSprite> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut sprites = None;
    if options.sprites != Sprites::Hidden && level.scene.boss.is_none() {
        let list = sprites::read_sprites_at(rom, level.sprite_data_ptr())?;
        if options.sprites == Sprites::Markers {
            markers.extend(list.sprites.iter().map(|sprite| {
                let (x, y) = sprite.tile_position(level.tiles.vertical);
                UndrawnSprite {
                    x,
                    y,
                    id: sprite.id,
                }
            }));
        } else {
            let scene = expand::capture_controlled(rom, level, &list, operation)?;
            draw_sprite_scene(
                &mut layers,
                &scene,
                level.scene.layer2_offset(),
                &video.vram,
            );
            markers = scene.undrawn.clone();
            diagnostics = scene.diagnostics.clone();
            sprites = Some(scene);
        }
    }
    // The player's OAM slots follow most of the sprites', so he goes
    // behind them: objects already in the layer stay in front.
    if options.player {
        draw_objects(
            &mut layers,
            &level.scene.player,
            video.object_select,
            &video.vram,
        );
    }
    if let Some(op) = operation {
        op.stage(Stage::Composing)?;
    }
    let mut image = compose_level(level, &layers, &video.palette());
    for UndrawnSprite { x, y, id } in markers {
        draw_sprite_marker(&mut image, x as u32 * 16, y as u32 * 16, id, &video.vram);
    }
    if let Some(op) = operation {
        op.stage(Stage::Finished)?;
    }
    Ok((image, diagnostics, sprites))
}
