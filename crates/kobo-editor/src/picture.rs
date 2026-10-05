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
        Self {
            chunks,
            size: Vec2::new(width as f32, height as f32),
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
