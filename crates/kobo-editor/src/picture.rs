//! A level's picture on the GPU, in pieces no larger than any graphics
//! driver takes as one texture: a level can be 8192 pixels wide, or
//! taller still with Lunar Magic's sizes.

use eframe::egui::{self, Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, Vec2};

/// The largest side of one texture.
const CHUNK: usize = 2048;

pub struct Picture {
    /// Each piece and where it sits in the level, in level pixels.
    chunks: Vec<(TextureHandle, Rect)>,
    pub size: Vec2,
    /// The whole picture small, averaged, for the minimap.
    pub small: TextureHandle,
}

/// The longest side of the small picture.
const SMALL: usize = 1024;

/// `image` shrunk by a whole factor so its longest side is at most
/// [`SMALL`], each pixel the average of those it covers.
fn shrink(image: &ColorImage) -> ColorImage {
    let [width, height] = image.size;
    let factor = width.max(height).div_ceil(SMALL).max(1);
    let (w, h) = ((width / factor).max(1), (height / factor).max(1));
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..factor {
                for dx in 0..factor {
                    let p = image.pixels[(y * factor + dy) * width + x * factor + dx];
                    sum[0] += u32::from(p.r());
                    sum[1] += u32::from(p.g());
                    sum[2] += u32::from(p.b());
                }
            }
            let n = (factor * factor) as u32;
            pixels.push(Color32::from_rgb(
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
            ));
        }
    }
    ColorImage::new([w, h], pixels)
}

impl Picture {
    pub fn new(ctx: &egui::Context, image: &ColorImage) -> Self {
        let [width, height] = image.size;
        let mut chunks = Vec::new();
        for top in (0..height).step_by(CHUNK) {
            for left in (0..width).step_by(CHUNK) {
                let (w, h) = (CHUNK.min(width - left), CHUNK.min(height - top));
                let mut pixels = Vec::with_capacity(w * h);
                for row in top..top + h {
                    let start = row * width + left;
                    pixels.extend_from_slice(&image.pixels[start..start + w]);
                }
                let piece = ColorImage::new([w, h], pixels);
                let texture = ctx.load_texture(
                    format!("level-{left}-{top}"),
                    piece,
                    egui::TextureOptions::NEAREST,
                );
                let at = Rect::from_min_size(
                    Pos2::new(left as f32, top as f32),
                    Vec2::new(w as f32, h as f32),
                );
                chunks.push((texture, at));
            }
        }
        let small = ctx.load_texture("level-small", shrink(image), egui::TextureOptions::LINEAR);
        Self {
            chunks,
            size: Vec2::new(width as f32, height as f32),
            small,
        }
    }

    /// Draws the part of the picture in `source` (level pixels) into
    /// `target` (screen points), tinted.
    pub fn draw(&self, painter: &Painter, source: Rect, target: Rect, tint: Color32) {
        let scale = target.size() / source.size();
        for (texture, at) in &self.chunks {
            let part = at.intersect(source);
            if !part.is_positive() {
                continue;
            }
            let uv = Rect::from_min_max(
                ((part.min - at.min) / at.size()).to_pos2(),
                ((part.max - at.min) / at.size()).to_pos2(),
            );
            let screen = Rect::from_min_max(
                target.min + (part.min - source.min) * scale,
                target.min + (part.max - source.min) * scale,
            );
            painter.image(texture.id(), screen, uv, tint);
        }
    }
}
