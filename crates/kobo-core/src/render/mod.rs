//! Drawing tiles into images.
//!
//! [`LayerTiles`] models the 8x8 tiles a level's layers can reference,
//! laid out as the game uploads them to VRAM: FG1, FG2, BG1, FG3 at tiles
//! `0x000`, `0x080`, `0x100`, `0x180`. Decoding captured VRAM also
//! includes animated tiles and other dynamic uploads.

mod level;

pub use level::{
    LevelRender, RenderError, RenderOptions, Sprites, render_level, render_level_with_control,
    render_loaded, render_loaded_with_control,
};

use crate::expand::{LevelTiles, LoadedLevel};
use crate::gfx::{self, Bpp, GfxError, Tile8};
use crate::image::RgbImage;
use crate::map16::{Map16Tile, Tile8Ref};
use crate::palette::{Color15, Palette};
use crate::rom::Rom;
use crate::video::{BossScene, LAYER_BITS, Layer1Registers, Layer3, Screen, SpriteObject, Window};

/// Number of 8x8 tiles addressable by a tilemap word.
pub const LAYER_TILE_COUNT: usize = 0x400;

/// VRAM tile index each layer GFX slot starts at.
pub const LAYER_SLOT_BASE: [usize; 4] = [0x000, 0x080, 0x100, 0x180];

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LayerTiles {
    pub tiles: Vec<Tile8>,
}

impl LayerTiles {
    pub fn blank() -> Self {
        Self {
            tiles: vec![Tile8::default(); LAYER_TILE_COUNT],
        }
    }

    /// Loads the four layer GFX files of an object tileset, applying the
    /// game's fourth-plane rule as it would on upload.
    pub fn for_object_tileset(rom: &Rom, tileset: u8) -> Result<Self, GfxError> {
        let files = gfx::object_tileset_files(rom, tileset)?;
        let mut out = Self::blank();
        let reader = gfx::GfxReader::new(rom)?;
        for (slot, &file) in files.iter().enumerate() {
            let gfx = reader.read(file)?;
            let mut tiles = gfx.tiles();
            if gfx.bpp() == Some(Bpp::Three) {
                for t in gfx::vram_upper_palette_tiles(file, tileset, tiles.len()) {
                    for px in tiles[t].pixels.iter_mut().flatten() {
                        if *px != 0 {
                            *px |= 8;
                        }
                    }
                }
            }
            let base = LAYER_SLOT_BASE[slot];
            for (i, tile) in tiles.into_iter().enumerate().take(0x80) {
                out.tiles[base + i] = tile;
            }
        }
        Ok(out)
    }

    /// Decodes the layer tile area of VRAM (4bpp tiles from byte 0) as
    /// the game uploaded it, including animated tiles and ExGFX.
    pub fn from_vram(vram: &[u8]) -> Self {
        let mut out = Self::blank();
        for (i, tile) in out.tiles.iter_mut().enumerate() {
            let start = i * 32;
            if start + 32 <= vram.len() {
                *tile = Tile8::decode(Bpp::Four, &vram[start..start + 32]);
            }
        }
        out
    }

    pub fn get(&self, index: u16) -> &Tile8 {
        &self.tiles[index as usize % LAYER_TILE_COUNT]
    }
}

/// The opaque pixels of an 8x8 tile as (`dx`, `dy`, value), where they
/// land once the tile is flipped. Colour 0 is transparent.
pub fn tile_pixels(
    tile: &Tile8,
    flip_x: bool,
    flip_y: bool,
) -> impl Iterator<Item = (usize, usize, u8)> + '_ {
    tile.pixels.iter().enumerate().flat_map(move |(ty, line)| {
        line.iter()
            .enumerate()
            .filter(|&(_, &px)| px != 0)
            .map(move |(tx, &px)| {
                (
                    if flip_x { 7 - tx } else { tx },
                    if flip_y { 7 - ty } else { ty },
                    px,
                )
            })
    })
}

/// Draws one 8x8 tile. Colour 0 is transparent.
pub fn draw_tile8(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    tile: &Tile8,
    row: &[[u8; 3]; 16],
    flip_x: bool,
    flip_y: bool,
) {
    for (dx, dy, px) in tile_pixels(tile, flip_x, flip_y) {
        img.put(x + dx as u32, y + dy as u32, row[px as usize]);
    }
}

/// Stacking order of the layers in Mode 1 terms: layer 3 low priority
/// (1), layer 3 high (3), layer 2 low (5), layer 1 low (6), layer 2 high
/// (8), layer 1 high (9). Objects with priority 0-3 slot in at 2, 4, 7,
/// and 10. With the BG3 priority bit, high-priority layer 3 moves in
/// front of everything (11). Mode 7's single layer sits between object
/// priorities 0 and 1 (3). Zero is transparent.
pub const LAYER3_LOW: u8 = 1;
pub const LAYER3_HIGH: u8 = 3;
pub const LAYER2_LOW: u8 = 5;
pub const LAYER1_LOW: u8 = 6;
pub const LAYER2_HIGH: u8 = 8;
pub const LAYER1_HIGH: u8 = 9;
pub const LAYER3_FRONT: u8 = 11;
pub const MODE7_LAYER1: u8 = 3;
pub const OBJECT_PRIORITIES: [u8; 4] = [2, 4, 7, 10];

/// One pixel of one layer: its CGRAM colour (0 is transparent, as on the
/// PPU) and its stacking priority.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LayerPixel {
    pub color: u8,
    pub priority: u8,
}

/// Indices of the layers in [`LevelLayers`].
pub const BG1: usize = 0;
pub const BG2: usize = 1;
pub const BG3: usize = 2;
pub const OBJ: usize = 3;
/// The backdrop's bit in `CGADSUB`.
const BACKDROP_BIT: u8 = 0x20;
/// Objects using sprite palettes 0-3 never take part in colour math.
const FIRST_MATH_OBJECT_COLOR: u8 = 0xC0;

/// How a background layer's tiles go into [`LevelLayers`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerStyle {
    /// Index into [`LevelLayers::layers`]: [`BG1`], [`BG2`], or [`BG3`].
    pub layer: usize,
    /// Stacking priority of tiles without and with the priority bit.
    pub priorities: [u8; 2],
    /// ORed into every tile's palette row, as the game's upload routine
    /// does for layer 2 objects in object tileset 3.
    pub palette_mask: u8,
}

/// A level drawn layer by layer, before the PPU's screen designation and
/// colour math combine the layers into a picture. Layer 1, layer 2,
/// layer 3, and the objects each keep their own pixels, so the same
/// buffers serve as the main screen and the subscreen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LevelLayers {
    pub width: u32,
    pub height: u32,
    pub layers: [Vec<LayerPixel>; 4],
}

impl LevelLayers {
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width * height) as usize;
        Self {
            width,
            height,
            layers: std::array::from_fn(|_| vec![LayerPixel::default(); len]),
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32)
            .then(|| (y as u32 * self.width + x as u32) as usize)
    }

    /// Sets a background layer pixel unless a higher-priority one is there.
    fn put(&mut self, layer: usize, at: usize, color: u8, priority: u8) {
        let p = &mut self.layers[layer][at];
        if color != 0 && priority > p.priority {
            *p = LayerPixel { color, priority };
        }
    }

    /// Sets an object pixel: the first opaque object in OAM order wins,
    /// whatever its priority.
    fn put_object(&mut self, at: usize, color: u8, priority: u8) {
        let p = &mut self.layers[OBJ][at];
        if color != 0 && p.color == 0 {
            *p = LayerPixel { color, priority };
        }
    }

    /// The topmost opaque pixel among the layers enabled in `mask` (a `TM`
    /// or `TS` value), with its layer index.
    fn pick(&self, mask: u8, at: usize) -> Option<(usize, LayerPixel)> {
        let mut best: Option<(usize, LayerPixel)> = None;
        for (layer, bit) in LAYER_BITS.iter().enumerate() {
            if mask & bit == 0 {
                continue;
            }
            let p = self.layers[layer][at];
            if p.color != 0 && best.is_none_or(|(_, b)| p.priority > b.priority) {
                best = Some((layer, p));
            }
        }
        best
    }

    /// Combines the layers the way the PPU does: the main screen shows the
    /// topmost enabled layer or the backdrop (CGRAM colour 0), and pixels
    /// whose layer is enabled in `CGADSUB` are added to or subtracted from
    /// the subscreen's topmost pixel, or the fixed colour where the
    /// subscreen is transparent (in which case the result is not halved).
    /// Objects on sprite palettes 0-3 are exempt.
    ///
    /// `window` is in screen coordinates, so it is for layers that are one
    /// fixed screen (a boss arena): it hides the layers it masks on each
    /// screen and is the colour window `CGWSEL` clips and prevents math
    /// against. Without one, every pixel is outside the colour window.
    pub fn compose(&self, palette: &Palette, screen: &Screen, window: Option<&Window>) -> RgbImage {
        let mut img = RgbImage::new(self.width, self.height);
        let add_subscreen = screen.math_select & 0x02 != 0;
        let halve = screen.color_math & 0x40 != 0;
        let subtract = screen.color_math & 0x80 != 0;
        for (at, out) in img.pixels.iter_mut().enumerate() {
            let (mut hidden, mut in_window) = ([0; 2], false);
            if let Some(window) = window {
                let (x, y) = (at % self.width as usize, at / self.width as usize);
                hidden = window.hidden(x, y);
                in_window = window.color(x, y);
            }
            let prevented = screen.prevents_math(in_window);
            let clipped = screen.clips_to_black(in_window);
            let (color, bit, exempt) = match self.pick(screen.main & !hidden[0], at) {
                Some((layer, p)) => (
                    palette.colors[p.color as usize],
                    LAYER_BITS[layer],
                    layer == OBJ && p.color < FIRST_MATH_OBJECT_COLOR,
                ),
                None => (palette.colors[0], BACKDROP_BIT, false),
            };
            let main = if clipped { Color15(0) } else { color };
            let math = screen.color_math & bit != 0 && !exempt && !prevented;
            let result = if math {
                let (operand, halve) = match (add_subscreen, self.pick(screen.sub & !hidden[1], at))
                {
                    (true, Some((_, p))) => (palette.colors[p.color as usize], halve && !clipped),
                    (true, None) => (screen.fixed_color, false),
                    (false, _) => (screen.fixed_color, halve && !clipped),
                };
                color_math(main, operand, subtract, halve)
            } else {
                main
            };
            *out = result.to_rgb8();
        }
        img
    }
}

/// Adds or subtracts two colours per channel in the PPU's five bits,
/// optionally halving the result, clamped to the channel range.
fn color_math(main: Color15, operand: Color15, subtract: bool, halve: bool) -> Color15 {
    let channel = |m: u8, o: u8| -> u8 {
        let (m, o) = (m as i32, o as i32);
        let v = if subtract { (m - o).max(0) } else { m + o };
        let v = if halve { v / 2 } else { v };
        v.min(31) as u8
    };
    Color15::from_rgb5(
        channel(main.r(), operand.r()),
        channel(main.g(), operand.g()),
        channel(main.b(), operand.b()),
    )
}

impl LevelLayers {
    /// Draws the 8x8 tile a reference names into a background layer, in
    /// the palette row and at the priority the reference selects.
    pub fn draw_tile_ref(
        &mut self,
        style: LayerStyle,
        x: i32,
        y: i32,
        r: Tile8Ref,
        tiles: &LayerTiles,
    ) {
        let color_base = ((r.palette() | style.palette_mask) & 7) * 16;
        let priority = style.priorities[r.priority() as usize];
        for (dx, dy, px) in tile_pixels(tiles.get(r.tile()), r.flip_x(), r.flip_y()) {
            if let Some(at) = self.index(x + dx as i32, y + dy as i32) {
                self.put(style.layer, at, color_base + px, priority);
            }
        }
    }

    /// Draws a 16x16 tile into a background layer at pixel position
    /// (`x`, `y`), quadrant by quadrant.
    pub fn draw_map16(
        &mut self,
        style: LayerStyle,
        x: i32,
        y: i32,
        tile: &Map16Tile,
        tiles: &LayerTiles,
    ) {
        for qy in 0..2 {
            for qx in 0..2 {
                let r = tile.quadrant(qx, qy);
                self.draw_tile_ref(style, x + 8 * qx as i32, y + 8 * qy as i32, r, tiles);
            }
        }
    }
}

/// Draws one 8x8 tile reference using the palette row it selects.
pub fn draw_tile_ref(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    r: Tile8Ref,
    tiles: &LayerTiles,
    palette: &Palette,
) {
    let row = palette.row_rgb8(r.palette() as usize);
    draw_tile8(img, x, y, tiles.get(r.tile()), &row, r.flip_x(), r.flip_y());
}

/// Draws a 16x16 tile at pixel position (`x`, `y`).
pub fn draw_map16_tile(
    img: &mut RgbImage,
    x: u32,
    y: u32,
    tile: &Map16Tile,
    tiles: &LayerTiles,
    palette: &Palette,
) {
    for qy in 0..2 {
        for qx in 0..2 {
            let r = tile.quadrant(qx, qy);
            draw_tile_ref(img, x + 8 * qx as u32, y + 8 * qy as u32, r, tiles, palette);
        }
    }
}

/// Renders a run of Map16 definitions in tile number order, `columns` per
/// row, over a solid background colour; a `None` leaves its cell blank.
pub fn map16_sheet(
    definitions: &[Option<Map16Tile>],
    tiles: &LayerTiles,
    palette: &Palette,
    background: [u8; 3],
    columns: u32,
) -> RgbImage {
    let columns = columns.max(1);
    let rows = (definitions.len() as u32).div_ceil(columns);
    let mut img = RgbImage::new(columns * 16, rows * 16);
    img.pixels.fill(background);
    for (i, tile) in definitions.iter().enumerate() {
        let Some(tile) = tile else { continue };
        let x = (i as u32 % columns) * 16;
        let y = (i as u32 / columns) * 16;
        draw_map16_tile(&mut img, x, y, tile, tiles, palette);
    }
    img
}

/// Renders a level: its layer 1 tile grid, its layer 2 (background
/// tilemap or objects), and its layer 3, combined by the screen
/// designation and colour math the level set up. Sprites are left out;
/// see [`level_layers`] and [`draw_sprite_scene`] to include them.
pub fn level_image(level: &LoadedLevel, layer_tiles: &LayerTiles, palette: &Palette) -> RgbImage {
    compose_level(level, &level_layers(level, layer_tiles), palette)
}

/// Draws a level's background layers, ready for sprites to be added
/// before [`compose_level`] turns them into a picture. A boss arena is
/// one fixed screen, drawn from its captured video-mode bands with the
/// objects of its drawing pass.
pub fn level_layers(level: &LoadedLevel, layer_tiles: &LayerTiles) -> LevelLayers {
    if let Some(scene) = &level.scene.boss {
        return boss_layers(scene, &level.video.vram);
    }
    let (w, h) = level.tiles.size();
    let mut layers = LevelLayers::new(w as u32 * 16, h as u32 * 16);
    draw_layer1(&mut layers, &level.tiles, layer_tiles);
    draw_layer2(
        &mut layers,
        &level.tiles,
        level.scene.layer2_offset(),
        layer_tiles,
    );
    if let Some(layer3) = &level.scene.layer3 {
        draw_layer3(&mut layers, layer3, &level.video.vram);
    }
    layers
}

/// Turns drawn layers into the picture the level shows. Only a boss
/// arena, being one fixed screen, has a window to apply.
pub fn compose_level(level: &LoadedLevel, layers: &LevelLayers, palette: &Palette) -> RgbImage {
    let window = level.scene.boss.as_ref().map(|scene| &scene.window);
    layers.compose(palette, &level.scene.screen, window)
}

/// Draws the layer 1 tile grid.
fn draw_layer1(layers: &mut LevelLayers, tiles: &LevelTiles, layer_tiles: &LayerTiles) {
    let style = LayerStyle {
        layer: BG1,
        priorities: [LAYER1_LOW, LAYER1_HIGH],
        palette_mask: 0,
    };
    let (w, h) = tiles.size();
    for y in 0..h {
        for x in 0..w {
            if let Some(tile) = tiles.map16_at(tiles.tile_at(x, y), x, y) {
                layers.draw_map16(style, (x * 16) as i32, (y * 16) as i32, tile, layer_tiles);
            }
        }
    }
}

/// Draws layer 3 from the captured tilemap: where the game showed it on
/// the entry screen, continued unstretched across the level along the
/// axes it scrolls on (a parallax layer keeps the entry screen's phase).
/// Along an axis the layer does not scroll it keeps its screen position:
/// horizontally the entry view repeats every 256 pixels, since every
/// screen of the level shows the same fixed backdrop; vertically it stays
/// inside the entry screen's 224-pixel band, where a tide sits while the
/// camera rests.
fn draw_layer3(layers: &mut LevelLayers, layer3: &Layer3, vram: &[u8]) {
    let tilemap = Tilemap::layer3(layer3);
    let priority_bit = layer3.high_priority_in_front();
    for ly in 0..layers.height {
        let Some(ty) = layer3_axis(layer3, 1, ly as i32) else {
            continue;
        };
        for lx in 0..layers.width {
            let Some(tx) = layer3_axis(layer3, 0, lx as i32) else {
                continue;
            };
            let Some((color, high)) = tilemap.pixel(vram, tx, ty) else {
                continue;
            };
            let priority = match (high, priority_bit) {
                (true, true) => LAYER3_FRONT,
                (true, false) => LAYER3_HIGH,
                (false, _) => LAYER3_LOW,
            };
            let at = (ly * layers.width + lx) as usize;
            layers.put(BG3, at, color, priority);
        }
    }
}

/// Layer 3 tilemap coordinate shown at a level coordinate along one axis
/// (0 horizontal, 1 vertical), or `None` where the layer is not drawn.
fn layer3_axis(layer3: &Layer3, axis: usize, at: i32) -> Option<i32> {
    let position = layer3.position[axis] as i32;
    let offset = at - layer3.camera[axis] as i32;
    if layer3.scroll_per_16[axis] != 0 {
        Some(position + offset)
    } else if axis == 0 {
        Some(position + offset.rem_euclid(256))
    } else {
        (0..224).contains(&offset).then_some(position + offset)
    }
}

/// A background layer's tilemap and character data, as the PPU's
/// registers describe them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tilemap {
    /// `BGnSC`: tilemap base and size.
    pub screens: u8,
    /// Byte address of the character data in VRAM.
    pub character_base: usize,
    pub bpp: Bpp,
    /// Tile side in pixels: 8, or 16 with the layer's `BGMODE` size bit.
    pub tile_side: usize,
}

impl Tilemap {
    /// Mode 1's layer 3: 2bpp with 8x8 tiles.
    pub fn layer3(layer3: &Layer3) -> Self {
        Self {
            screens: layer3.tilemap,
            character_base: layer3.character_base as usize,
            bpp: Bpp::Two,
            tile_side: 8,
        }
    }

    /// Mode 1's layer 1: 4bpp.
    pub fn layer1(layer: &Layer1Registers) -> Self {
        Self {
            screens: layer.tilemap,
            character_base: layer.character_base as usize,
            bpp: Bpp::Four,
            tile_side: if layer.mode & 0x10 != 0 { 16 } else { 8 },
        }
    }

    /// CGRAM colour index relative to the layer's first palette and
    /// priority bit of the pixel at a tilemap position (which wraps), or
    /// `None` where it is transparent.
    pub fn pixel(&self, vram: &[u8], x: i32, y: i32) -> Option<(u8, bool)> {
        let side = self.tile_side;
        let (x, y) = (
            x.rem_euclid(64 * side as i32) as usize,
            y.rem_euclid(64 * side as i32) as usize,
        );
        let wide = (self.screens as usize & 1) + 1;
        let tall = ((self.screens as usize >> 1) & 1) + 1;
        let col = (x / side) % (32 * wide);
        let row = (y / side) % (32 * tall);
        let at = ((self.screens as usize >> 2) << 11)
            + (row / 32 * wide + col / 32) * 0x800
            + (row % 32 * 32 + col % 32) * 2;
        let tile = Tile8Ref(u16::from_le_bytes([
            video_byte(vram, at),
            video_byte(vram, at + 1),
        ]));
        let flip = |flipped: bool, n: usize| if flipped { side - 1 - n } else { n };
        let (px, py) = (flip(tile.flip_x(), x % side), flip(tile.flip_y(), y % side));
        // A 16x16 tile is four characters: the next one, and the row below.
        let number = (tile.tile() as usize + px / 8 + py / 8 * 16) & 0x3FF;
        let start = self.character_base + number * self.bpp.bytes_per_tile();
        let color = character_pixel(vram, start, self.bpp, px % 8, py % 8);
        (color != 0).then_some((
            color + tile.palette() * self.bpp.colors() as u8,
            tile.priority(),
        ))
    }
}

/// One pixel of an 8x8 character whose data starts at byte `start` of
/// VRAM: planes 0 and 1 interleaved row by row, then planes 2 and 3.
pub fn character_pixel(vram: &[u8], start: usize, bpp: Bpp, px: usize, py: usize) -> u8 {
    (0..bpp.bits() as usize).fold(0, |color, plane| {
        let bits = video_byte(vram, start + plane / 2 * 16 + py * 2 + plane % 2);
        color | ((bits >> (7 - px)) & 1) << plane
    })
}

/// Draws captured sprite objects into the object layer, front to back:
/// the first opaque object at a pixel wins, and carries its OAM priority
/// against the background layers.
///
/// Captures with characters of their own ([`crate::video::DynamicObjects`])
/// are drawn from `vram` with those in place. Objects riding on layer 2
/// go wherever `layer2_offset` (see
/// [`crate::video::LevelScene::layer2_offset`]) puts that layer, once per
/// 256 pixels across the level.
pub fn draw_sprite_scene(
    layers: &mut LevelLayers,
    scene: &crate::video::SpriteScene,
    layer2_offset: [i32; 2],
    vram: &[u8],
) {
    draw_objects(layers, &scene.objects, scene.object_select, vram);
    for dynamic in &scene.dynamic {
        let vram = dynamic.patched(vram);
        draw_objects(layers, &dynamic.objects, scene.object_select, &vram);
    }
    let [dx, dy] = layer2_offset;
    let repeated: Vec<_> = (0..=layers.width as i32 / 256 + 1)
        .flat_map(|screen| {
            scene.layer2_objects.iter().map(move |o| SpriteObject {
                x: (o.x + dx).rem_euclid(256) + (screen - 1) * 256,
                y: o.y + dy,
                ..*o
            })
        })
        .collect();
    draw_objects(layers, &repeated, scene.object_select, vram);
}

/// Draws OAM objects (in level coordinates) into the object layer, front
/// to back, with the sizes and character base `object_select` selects.
/// Objects already in the layer stay in front of these.
pub fn draw_objects(
    layers: &mut LevelLayers,
    objects: &[SpriteObject],
    object_select: u8,
    vram: &[u8],
) {
    let sizes = crate::expand::object_sizes(object_select);
    for object in objects {
        let size = sizes[object.large as usize];
        let priority = OBJECT_PRIORITIES[(object.attr >> 4 & 3) as usize];
        let color_base = 128 + (object.attr >> 1 & 7) * 16;
        for dy in 0..size.1 {
            for dx in 0..size.0 {
                let Some(at) = layers.index(object.x + dx, object.y + dy) else {
                    continue;
                };
                if let Some(color) = object_pixel(vram, object_select, object, size, dx, dy) {
                    layers.put_object(at, color_base + color, priority);
                }
            }
        }
    }
}

/// Colour index (1-15) of a pixel of an OAM object of `size`, or `None`
/// where it is transparent. `dx`/`dy` are unflipped offsets within the
/// object.
fn object_pixel(
    vram: &[u8],
    object_select: u8,
    object: &SpriteObject,
    (width, height): (i32, i32),
    dx: i32,
    dy: i32,
) -> Option<u8> {
    let attr = object.attr;
    let tx = if attr & 0x40 != 0 { width - 1 - dx } else { dx } as usize;
    let ty = if attr & 0x80 != 0 {
        height - 1 - dy
    } else {
        dy
    } as usize;
    let start = object.character_address(object_select, tx / 8, ty / 8);
    let color = character_pixel(vram, start, Bpp::Four, tx % 8, ty % 8);
    (color != 0).then_some(color)
}

/// Draws layer 2 where the entry camera sees it, (`dx`, `dy`) pixels from
/// the layer 1 grid: the background tilemap
/// repeated every two screens, or the layer 2 objects from their own
/// region of the tile grid, displaced from the layer 1 grid by the
/// difference between the layer 2 and layer 1 positions at entry (layer 2
/// scroll settings offset the layer or move it at another rate). Beyond
/// the entry screen the layer continues unstretched.
///
/// A vertical level's background is the same two-screen-wide tilemap
/// (mode `$0A` keeps layer 2 horizontal: `$5B` bit 1 clear) spanning the
/// level's full 32-tile width, and the game scrolls it slowly so the
/// 27 rows cover the whole descent. A static render cannot reproduce
/// that parallax, so the background is tiled down the level instead.
fn draw_layer2(
    layers: &mut LevelLayers,
    tiles: &LevelTiles,
    [dx, dy]: [i32; 2],
    layer_tiles: &LayerTiles,
) {
    let background = tiles.shows_background();
    if !background && tiles.layer2_objects().is_none() {
        return;
    }
    let style = LayerStyle {
        layer: BG2,
        priorities: [LAYER2_LOW, LAYER2_HIGH],
        palette_mask: if background {
            0
        } else {
            tiles.layer2_palette_mask()
        },
    };
    let rows = tiles.layer2_bg_rows().min(crate::expand::SCREEN_ROWS) as i32;
    // Layer 2 tile (tx, ty) covers level pixels from (tx * 16 + dx, ty * 16 + dy).
    let first = |d: i32| (-d).div_euclid(16);
    let last = |d: i32, extent: u32| (extent as i32 - d).div_euclid(16);
    for ty in first(dy)..=last(dy, layers.height) {
        for tx in first(dx)..=last(dx, layers.width) {
            let tile = if background {
                let (col, row) = (tx.rem_euclid(32) as usize, ty.rem_euclid(rows) as usize);
                let n = tiles.layer2_bg_tile(col / 16, col % 16, row).unwrap();
                tiles.bg_map16.get(n as usize - 0x200)
            } else {
                let (Ok(x), Ok(y)) = (usize::try_from(tx), usize::try_from(ty)) else {
                    continue;
                };
                tiles
                    .layer2_object_tile(x, y)
                    .and_then(|n| tiles.map16_at(n, x, y))
            };
            if let Some(tile) = tile {
                layers.draw_map16(style, tx * 16 + dx, ty * 16 + dy, tile, layer_tiles);
            }
        }
    }
}

/// Fixed-screen arenas use the ROM's video-mode bands, not its collision
/// Map16 grid: layer 1 is whatever mode each band of the screen is in,
/// and the objects are those of the arena's drawing pass. In particular,
/// Mode 7 interleaves tile numbers in the low VRAM bytes and packed 8bpp
/// pixels in the high bytes.
fn boss_layers(scene: &BossScene, vram: &[u8]) -> LevelLayers {
    let mut layers = LevelLayers::new(256, 224);
    for y in 0..224usize {
        let Some(band) = scene.bands.iter().rev().find(|b| b.start <= y) else {
            continue;
        };
        let layer = &band.layer;
        let tilemap = Tilemap::layer1(layer);
        for x in 0..256usize {
            // The picture's first row is the PPU's scanline 1.
            let pixel = match layer.mode & 7 {
                7 => mode7_pixel(vram, &layer.mode7, x, y + 1).map(|color| (color, MODE7_LAYER1)),
                1 => tilemap
                    .pixel(
                        vram,
                        x as i32 + layer.scroll[0] as i32,
                        y as i32 + 1 + layer.scroll[1] as i32,
                    )
                    .map(|(color, high)| (color, [LAYER1_LOW, LAYER1_HIGH][high as usize])),
                _ => None,
            };
            if let Some((color, priority)) = pixel {
                layers.put(BG1, y * 256 + x, color, priority);
            }
        }
    }
    draw_objects(&mut layers, &scene.objects, scene.object_select, vram);
    layers
}

fn video_byte(vram: &[u8], address: usize) -> u8 {
    vram.get(address & 0xFFFF).copied().unwrap_or(0)
}

fn mode7_pixel(vram: &[u8], mode: &crate::video::Mode7, x: usize, y: usize) -> Option<u8> {
    let signed13 = |n: u16| ((n << 3) as i16 >> 3) as i32;
    let center = mode.center.map(signed13);
    let scroll = mode.scroll.map(signed13);
    // The PPU clips the scroll-to-center differences to ten magnitude
    // bits while retaining the sign from bit 13.
    let delta: [i32; 2] = std::array::from_fn(|axis| {
        let n = scroll[axis] - center[axis];
        if n & 0x2000 != 0 {
            n | !0x3FF
        } else {
            n & 0x3FF
        }
    });
    let x = if mode.control & 1 != 0 { 255 - x } else { x } as i32;
    let y = if mode.control & 2 != 0 { 255 - y } else { y } as i32;
    let position: [i32; 2] = std::array::from_fn(|axis| {
        let a = mode.matrix[axis * 2] as i32;
        let b = mode.matrix[axis * 2 + 1] as i32;
        let row = [a * delta[0], b * delta[1], b * y]
            .map(|n| n & !63)
            .iter()
            .sum::<i32>();
        (row + a * x + center[axis] * 256) >> 8
    });
    let outside = position.iter().any(|&n| !(0..1024).contains(&n));
    let tile = if outside && mode.control & 0x80 != 0 {
        if mode.control & 0x40 == 0 {
            return None;
        }
        0
    } else {
        let col = (position[0] & 1023) as usize / 8;
        let row = (position[1] & 1023) as usize / 8;
        video_byte(vram, 2 * (row * 128 + col)) as usize
    };
    let pixel = tile * 64 + (position[1] & 7) as usize * 8 + (position[0] & 7) as usize;
    let color = video_byte(vram, pixel * 2 + 1);
    (color != 0).then_some(color)
}

/// VRAM byte offset of the layer 3 font (GFX28 at word `$4000`): tiles
/// `0`-`9` then `A`-`Z`, 2bpp.
const FONT_VRAM_OFFSET: usize = 0x8000;

/// Draws a hex digit or letter from the layer 3 font in VRAM.
pub fn draw_font_glyph(img: &mut RgbImage, x: u32, y: u32, vram: &[u8], glyph: char, rgb: [u8; 3]) {
    let index = match glyph {
        '0'..='9' => glyph as usize - '0' as usize,
        'A'..='Z' => glyph as usize - 'A' as usize + 10,
        _ => return,
    };
    let start = FONT_VRAM_OFFSET + index * 16;
    if start + 16 > vram.len() {
        return;
    }
    let tile = Tile8::decode(Bpp::Two, &vram[start..start + 16]);
    let row = [[0, 0, 0], rgb, rgb, rgb];
    let mut palette = [[0u8; 3]; 16];
    palette[..4].copy_from_slice(&row);
    draw_tile8(img, x, y, &tile, &palette, false, false);
}

/// Draws a sprite marker: a box outline with the sprite number inside.
pub fn draw_sprite_marker(img: &mut RgbImage, x: u32, y: u32, id: u8, vram: &[u8]) {
    let outline = [255, 255, 255];
    let fill = [0, 0, 0];
    for i in 0..16 {
        img.put(x + i, y, outline);
        img.put(x + i, y + 15, outline);
        img.put(x, y + i, outline);
        img.put(x + 15, y + i, outline);
    }
    for dy in 4..12 {
        for dx in 0..16 {
            img.put(x + dx, y + dy, fill);
        }
    }
    let text = format!("{id:02X}");
    let mut chars = text.chars();
    draw_font_glyph(img, x, y + 4, vram, chars.next().unwrap(), outline);
    draw_font_glyph(img, x + 8, y + 4, vram, chars.next().unwrap(), outline);
}

/// Renders a palette as a 16x16 grid of `cell`-pixel swatches.
pub fn palette_swatch(palette: &Palette, cell: u32) -> RgbImage {
    let mut img = RgbImage::new(16 * cell, 16 * cell);
    for row in 0..16 {
        for col in 0..16 {
            let rgb = palette.get(row, col).to_rgb8();
            for dy in 0..cell {
                for dx in 0..cell {
                    img.put(col as u32 * cell + dx, row as u32 * cell + dy, rgb);
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod video_tests {
    use super::*;
    use crate::video::{Band, Mode7};

    #[test]
    fn mode7_interleaving_transform_and_overflow() {
        let mut vram = vec![0; 0x10000];
        let mut mode = Mode7 {
            matrix: [256, 0, 0, 256],
            ..Mode7::default()
        };
        vram[2 * (128 + 2)] = 3; // Tile at world (16, 8).
        vram[2 * (3 * 64 + 2 * 8 + 1) + 1] = 42;
        assert_eq!(mode7_pixel(&vram, &mode, 17, 10), Some(42));
        assert_eq!(mode7_pixel(&vram, &mode, 18, 10), None);
        mode.matrix = [0, 256, -256, 0];
        mode.center = [16, 16];
        mode.scroll = [16, 16];
        assert_eq!(mode7_pixel(&vram, &mode, 6, 1), Some(42));

        mode = Mode7 {
            matrix: [256, 0, 0, 256],
            scroll: [0x1FFF, 0],
            ..Mode7::default()
        };
        vram[2 * 127] = 4;
        vram[2 * (4 * 64 + 7) + 1] = 25;
        vram[2 * 7 + 1] = 9;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), Some(25)); // Wrap -1 to 1023.
        mode.control = 0x80;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), None);
        mode.control = 0xC0;
        assert_eq!(mode7_pixel(&vram, &mode, 0, 0), Some(9)); // Outside uses tile 0.
    }

    #[test]
    fn tilemap_uses_character_base_second_screen_and_flips() {
        let mut vram = vec![0; 0x10000];
        let layer = Layer1Registers {
            mode: 1,
            tilemap: 0x59,
            character_base: 0xE000,
            ..Layer1Registers::default()
        };
        // Second horizontal screen, tile 2, palette 3, high priority, both flips.
        vram[0xB800..0xB802].copy_from_slice(&0xEC02u16.to_le_bytes());
        vram[0xE000 + 2 * 32 + 14] = 1;
        let tilemap = Tilemap::layer1(&layer);
        assert_eq!(tilemap.pixel(&vram, 256, 0), Some((49, true)));
        assert_eq!(tilemap.pixel(&vram, 257, 0), None);
        assert_eq!(tilemap.pixel(&vram, 768, 0), Some((49, true)));
    }

    #[test]
    fn layer3_axes_continue_the_entry_view_or_keep_the_screen() {
        let mut layer3 = Layer3 {
            position: [100, 64],
            camera: [32, 192],
            scroll_per_16: [8, 0],
            ..Default::default()
        };
        assert_eq!(layer3_axis(&layer3, 0, 32), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 48), Some(116)); // unstretched
        assert_eq!(layer3_axis(&layer3, 0, 31), Some(99));
        // A vertically fixed layer stays inside the entry screen's band.
        assert_eq!(layer3_axis(&layer3, 1, 192), Some(64));
        assert_eq!(layer3_axis(&layer3, 1, 415), Some(287));
        assert_eq!(layer3_axis(&layer3, 1, 416), None);
        assert_eq!(layer3_axis(&layer3, 1, 191), None);
        // A horizontally fixed layer repeats the entry screen.
        layer3.scroll_per_16 = [0, 16];
        assert_eq!(layer3_axis(&layer3, 0, 32), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 288), Some(100));
        assert_eq!(layer3_axis(&layer3, 0, 31), Some(355));
        assert_eq!(layer3_axis(&layer3, 1, 0), Some(64 - 192));
    }

    #[test]
    fn color_math_adds_subtracts_and_halves_in_five_bits() {
        let main = Color15::from_rgb5(31, 0, 20);
        let sub = Color15::from_rgb5(0, 31, 20);
        assert_eq!(
            color_math(main, sub, false, false),
            Color15::from_rgb5(31, 31, 31)
        );
        assert_eq!(
            color_math(main, sub, true, false),
            Color15::from_rgb5(31, 0, 0)
        );
        assert_eq!(
            color_math(main, sub, false, true),
            Color15::from_rgb5(15, 15, 20)
        );
        assert_eq!(
            color_math(main, sub, true, true),
            Color15::from_rgb5(15, 0, 0)
        );
    }

    /// One pixel per column: layer 1 (colour 1, red) at column 1, layer 2
    /// (colour 2, blue) at columns 1 and 2, an object on sprite palette 0
    /// (colour 129, green) at column 3 and one on palette 4 (colour 193,
    /// also green) at column 4, all low priority; nothing at column 0.
    fn layers() -> (LevelLayers, Palette) {
        let mut layers = LevelLayers::new(5, 1);
        layers.put(BG1, 1, 1, LAYER1_LOW);
        layers.put(BG2, 1, 2, LAYER2_LOW);
        layers.put(BG2, 2, 2, LAYER2_LOW);
        layers.put_object(3, 129, OBJECT_PRIORITIES[0]);
        layers.put_object(4, 193, OBJECT_PRIORITIES[0]);
        let mut palette = Palette::default();
        palette.colors[1] = Color15::from_rgb5(31, 0, 0);
        palette.colors[2] = Color15::from_rgb5(0, 0, 31);
        palette.colors[129] = Color15::from_rgb5(0, 31, 0);
        palette.colors[193] = Color15::from_rgb5(0, 31, 0);
        (layers, palette)
    }

    fn rgb5(image: &RgbImage) -> Vec<[u8; 3]> {
        image.pixels.iter().map(|p| p.map(|c| c >> 3)).collect()
    }

    #[test]
    fn compose_follows_screen_designation_and_color_math() {
        let (layers, palette) = layers();
        let fixed = Color15::from_rgb5(8, 8, 8);
        // Vanilla: layer 2 on the subscreen shows through the backdrop only.
        let screen = Screen::vanilla(fixed);
        assert_eq!(
            rgb5(&layers.compose(&palette, &screen, None)),
            [[8, 8, 8], [31, 0, 0], [0, 0, 31], [0, 31, 0], [0, 31, 0]]
        );
        // Half-brightness modes: objects and the backdrop halve with layer
        // 2, except objects on palettes 0-3 and pixels over a transparent
        // subscreen, which take the fixed colour unhalved.
        let half = Screen {
            color_math: 0x70,
            ..screen
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &half, None)),
            [[8, 8, 8], [31, 0, 0], [0, 0, 15], [0, 31, 0], [8, 31, 8]]
        );
        // The spotlight rooms subtract the fixed colour from everything and
        // halve, with the fixed colour as the operand; "prevent inside the
        // colour window" prevents nothing without a window.
        let dark = Screen {
            main: 0x17,
            sub: 0x00,
            color_math: 0xFF,
            math_select: 0x20,
            fixed_color: fixed,
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &dark, None)),
            [[0, 0, 0], [11, 0, 0], [0, 0, 11], [0, 31, 0], [0, 11, 0]]
        );
        // Clipping to black and preventing math everywhere.
        let clipped = Screen {
            math_select: 0xF2,
            ..screen
        };
        assert_eq!(rgb5(&layers.compose(&palette, &clipped, None)), [[0; 3]; 5]);
        // Level mode $1E: only layer 1 on the main screen, added to the
        // objects and layer 2 beneath it.
        let translucent = Screen {
            main: 0x01,
            sub: 0x16,
            color_math: 0x21,
            math_select: 0x02,
            fixed_color: fixed,
        };
        assert_eq!(
            rgb5(&layers.compose(&palette, &translucent, None)),
            [[8, 8, 8], [31, 0, 31], [0, 0, 31], [0, 31, 0], [0, 31, 0]]
        );
    }

    #[test]
    fn layer3_pixels_come_from_the_2bpp_tilemap_with_flips() {
        let mut vram = vec![0; 0x10000];
        let tilemap = Tilemap::layer3(&Layer3 {
            tilemap: 0x53,
            character_base: 0x8000,
            ..Default::default()
        });
        // Tile 2, palette 3, high priority, X flip, at column 1 of the
        // bottom-left 32x32 screen (row 32).
        let word: u16 = 0x2002 | (3 << 10) | 0x4000;
        vram[0xB000 + 2..0xB000 + 4].copy_from_slice(&word.to_le_bytes());
        vram[0x8000 + 2 * 16] = 0x80; // plane 0, row 0, leftmost pixel
        vram[0x8000 + 2 * 16 + 1] = 0x80; // plane 1
        assert_eq!(tilemap.pixel(&vram, 15, 256), Some((15, true)));
        assert_eq!(tilemap.pixel(&vram, 8, 256), None);
        assert_eq!(tilemap.pixel(&vram, 15 + 512, 256 + 512), Some((15, true)));
        assert_eq!(tilemap.pixel(&vram, 15 - 512, 256), Some((15, true)));
    }

    /// An arena in one Mode 1 band with two 8x8 objects on sprite palette
    /// 0, tile 2 (colour 129, red) at x = -1 and tile 3 (colour 130, green,
    /// priority 3) at x = 0, and the game's window setup: BG1 and objects
    /// masked inside window 1, colour math prevented outside it.
    fn arena() -> (BossScene, Vec<u8>, Palette, Screen) {
        let object = |x, tile, attr| SpriteObject {
            x,
            y: 0,
            tile,
            attr,
            large: false,
        };
        let scene = BossScene {
            bands: vec![Band {
                start: 0,
                layer: Layer1Registers {
                    mode: 1,
                    ..Layer1Registers::default()
                },
            }],
            window: Window {
                rows: vec![[1, 0]; 224],
                masks: [0x11, 0x00],
                select: [0x02, 0x00, 0x32],
                ..Window::default()
            },
            objects: vec![object(-1, 2, 0x00), object(0, 3, 0x30)],
            object_select: 0,
        };
        let mut vram = vec![0; 0x10000];
        vram[2 * 32] = 0xFF; // Object tile 2, top row, plane 0.
        vram[3 * 32 + 1] = 0xFF; // Object tile 3, top row, plane 1.
        let mut palette = Palette::default();
        palette.colors[129] = Color15::from_rgb5(31, 0, 0);
        palette.colors[130] = Color15::from_rgb5(0, 31, 0);
        let screen = Screen {
            main: 0x15,
            sub: 0x00,
            color_math: 0x20,
            math_select: 0x20,
            fixed_color: Color15::from_rgb5(0, 0, 31),
        };
        (scene, vram, palette, screen)
    }

    #[test]
    fn arena_objects_respect_oam_order_signed_x_and_the_window() {
        let (mut scene, vram, mut palette, screen) = arena();
        scene.window.rows[0] = [2, 2];
        let picture = |scene: &BossScene| {
            let layers = boss_layers(scene, &vram);
            rgb5(&layers.compose(&palette, &screen, Some(&scene.window)))
        };
        let row = picture(&scene);
        assert_eq!(row[0], [31, 0, 0]);
        assert_eq!(row[1], [31, 0, 0]); // First in OAM blocks the higher-priority second.
        assert_eq!(row[2], [0, 0, 31]); // The window masks both and shows the back area.
        assert_eq!(row[7], [0, 31, 0]); // Signed X clips the first object.
        assert_eq!(row[8], [0, 0, 0]); // Outside the window the backdrop stays black.
        scene.objects.reverse();
        assert_eq!(picture(&scene)[1], [0, 31, 0]);
        // The backdrop is CGRAM colour 0, whatever a hack makes it; the
        // back area colour is added to it inside the window only.
        palette.colors[0] = Color15::from_rgb5(4, 0, 0);
        let layers = boss_layers(&scene, &vram);
        let row = rgb5(&layers.compose(&palette, &screen, Some(&scene.window)));
        assert_eq!((row[2], row[8]), ([4, 0, 31], [4, 0, 0]));
    }

    #[test]
    fn arena_layer_1_stacks_by_band_mode() {
        let (mut scene, mut vram, mut palette, screen) = arena();
        palette.colors[1] = Color15::from_rgb5(31, 31, 31);
        // Mode 1: the empty tilemap at word $1000 shows character 0, whose
        // top row is solid colour 1. Without its priority bit it covers
        // object priority 0 and is covered by priority 3.
        vram[0] = 0xFF;
        scene.objects[0].x = 8;
        scene.objects[1].x = 16;
        scene.bands[0].layer.tilemap = 0x10;
        scene.bands[0].layer.scroll = [0, 0xFFFF]; // Scanline 1 is tilemap row 0.
        let layers = boss_layers(&scene, &vram);
        let row = rgb5(&layers.compose(&palette, &screen, Some(&scene.window)));
        assert_eq!(row[0], [31, 31, 31]);
        assert_eq!(row[8], [31, 31, 31]);
        assert_eq!(row[16], [0, 31, 0]);
        // Mode 7: the first pixel of tile 0 is colour 1, and the layer sits
        // between object priorities 0 and 1.
        scene.bands[0].layer.mode = 7;
        scene.bands[0].layer.mode7.matrix = [256, 0, 0, 256];
        scene.bands[0].layer.mode7.scroll = [0, 0x1FFF];
        scene.objects[0].x = 0;
        scene.objects[1] = SpriteObject {
            x: 8,
            attr: 0x10,
            ..scene.objects[1]
        };
        vram[0] = 0;
        vram[1] = 1;
        let layers = boss_layers(&scene, &vram);
        let row = rgb5(&layers.compose(&palette, &screen, Some(&scene.window)));
        assert_eq!(row[0], [31, 31, 31]); // Over object priority 0.
        assert_eq!(row[8], [0, 31, 0]); // Under object priority 1.
    }
}
