//! Simple raster images and PNG output.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use thiserror::Error;

use crate::gfx::Tile8;

#[derive(Debug, Error)]
pub enum ImageError {
    #[error("failed to write {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("PNG encoding failed: {0}")]
    Png(#[from] png::EncodingError),
    #[error("PNG decoding failed: {0}")]
    PngDecode(#[from] png::DecodingError),
    #[error("{0}")]
    Format(String),
}

/// An image of palette indices, row-major, top-left origin, with the
/// palette it previews with. Graphics files are these: the pixel is the
/// colour index, the palette only a preview.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub palette: Vec<[u8; 3]>,
}

impl IndexedImage {
    /// The image as an 8-bit indexed PNG, the same bytes for the same image.
    pub fn to_png(&self) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(self.palette.iter().flatten().copied().collect::<Vec<u8>>());
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&self.pixels)?;
        }
        Ok(out)
    }

    /// Reads an indexed or grayscale PNG of 8 bits or fewer per pixel as
    /// palette indices (a grayscale value is its index).
    pub fn from_png(bytes: &[u8]) -> Result<Self, ImageError> {
        let mut decoder = png::Decoder::new(io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::IDENTITY);
        let mut reader = decoder.read_info()?;
        let info = reader.info().clone();
        if !matches!(
            info.color_type,
            png::ColorType::Indexed | png::ColorType::Grayscale
        ) || info.bit_depth == png::BitDepth::Sixteen
        {
            return Err(ImageError::Format(
                "a graphics file is an indexed PNG, whose pixels are colour indices".into(),
            ));
        }
        let palette = info
            .palette
            .as_deref()
            .map(|p| p.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
            .unwrap_or_default();
        let mut buf = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buf)?;
        let bits = info.bit_depth as usize;
        let (width, height) = (info.width, info.height);
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for row in buf[..frame.buffer_size()].chunks(frame.line_size) {
            for x in 0..width as usize {
                let bit = x * bits;
                let byte = row[bit / 8];
                let shift = 8 - bits - bit % 8;
                pixels.push((byte >> shift) & ((1u16 << bits) - 1) as u8);
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
            palette,
        })
    }
}

/// An 8-bit RGB image, row-major, top-left origin.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
}

impl RgbImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![[0; 3]; (width * height) as usize],
        }
    }

    pub fn put(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = rgb;
        }
    }

    pub fn write_png(&self, path: impl AsRef<Path>) -> Result<(), ImageError> {
        let path = path.as_ref();
        let io_err = |source| ImageError::Io {
            path: path.to_path_buf(),
            source,
        };
        let file = File::create(path).map_err(io_err)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let flat: Vec<u8> = self.pixels.iter().flatten().copied().collect();
        writer.write_image_data(&flat)?;
        Ok(())
    }
}

/// Lays tiles out left to right, top to bottom, `columns` per row, mapping
/// each pixel through `palette`.
pub fn tile_sheet(tiles: &[Tile8], columns: u32, palette: &[[u8; 3]]) -> RgbImage {
    let columns = columns.max(1);
    let rows = (tiles.len() as u32).div_ceil(columns);
    let mut img = RgbImage::new(columns * 8, rows * 8);
    for (i, tile) in tiles.iter().enumerate() {
        let ox = (i as u32 % columns) * 8;
        let oy = (i as u32 / columns) * 8;
        for (y, row) in tile.pixels.iter().enumerate() {
            for (x, &px) in row.iter().enumerate() {
                let rgb = palette.get(px as usize).copied().unwrap_or([255, 0, 255]);
                img.put(ox + x as u32, oy + y as u32, rgb);
            }
        }
    }
    img
}

/// A grayscale ramp with `colors` entries, black first.
pub fn grayscale(colors: usize) -> Vec<[u8; 3]> {
    (0..colors)
        .map(|i| {
            let v = (i * 255 / colors.saturating_sub(1).max(1)) as u8;
            [v, v, v]
        })
        .collect()
}
