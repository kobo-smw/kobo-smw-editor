//! Compares each captured sprite's part of a level picture with an
//! emulator frame: `KOBO_ORACLE_VIDEO=1` dumps (`tools/oracle/dump.sh`,
//! `dump_hack.sh`) of the ROM given. For every level with a frame in the
//! directories, the level is rendered with sprites, and the pixels each
//! sprite entry's objects cover, where they are on the emulator's screen,
//! are compared with the frame; the whole visible picture is scored too.
//! A sprite that has animated or moved by the frame scores lower; one
//! drawn with the wrong tiles, colours, or not at all scores much lower,
//! which is what this is for on a hack with custom sprites.
//!
//! `cargo run --release --example sprite_oracle -- rom.smc dumpdir...`
//!
//! Levels whose frame lands the sprite off the screen are silent about it.
//! The last table sums up per sprite number (with the entry's extra bits)
//! over every level seen, worst first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kobo_core::image::RgbImage;
use kobo_core::{Rom, expand, render, sprites};

/// The status bar occupies the top of the picture with its own scroll.
const STATUS_BAR_LINES: i32 = 36;
/// Mesen's frame is 239 lines with the 224-line picture inside; the
/// picture's first line lands on one of these frame rows depending on the
/// capture, so the comparison tries each and keeps the best.
const FRAME_PADDING: std::ops::RangeInclusive<i32> = 4..=9;

struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 3]>,
    camera: (i32, i32),
}

fn read_frame(dir: &Path, level: u16) -> Option<Frame> {
    let tag = format!("level_{level:03X}");
    let ppm = std::fs::read(dir.join(format!("{tag}.ppm"))).ok()?;
    let wram = std::fs::read(dir.join(format!("{tag}.wram.bin"))).ok()?;
    let mut parts = ppm.splitn(4, |&b| b == b'\n');
    assert_eq!(parts.next()?, b"P6");
    let dims: Vec<usize> = std::str::from_utf8(parts.next()?)
        .ok()?
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    parts.next()?;
    let data = parts.next()?;
    let pixels = data.as_chunks::<3>().0.to_vec();
    let camera = |at: usize| u16::from_le_bytes([wram[at], wram[at + 1]]) as i32;
    Some(Frame {
        width: dims[0],
        height: dims[1],
        pixels,
        camera: (camera(0x1A), camera(0x1C)),
    })
}

fn levels_in(dir: &Path) -> Vec<u16> {
    let mut levels: Vec<u16> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let hex = name.strip_prefix("level_")?.strip_suffix(".ppm")?;
            u16::from_str_radix(hex, 16).ok()
        })
        .collect();
    levels.sort_unstable();
    levels
}

/// A rectangle of level pixels, `x0..x1` by `y0..y1`.
#[derive(Clone, Copy)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Rect {
    fn intersect(self, other: Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    fn union(self, other: Rect) -> Rect {
        Rect {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }
}

/// Pixels of `area` that agree between the frame (with the picture
/// starting `pad` rows into it) and the rendered level: (same, compared).
fn agreement(frame: &Frame, img: &RgbImage, pad: i32, area: Rect) -> (usize, usize) {
    let (mut same, mut total) = (0, 0);
    for y in area.y0..area.y1 {
        let sy = y - frame.camera.1;
        let fy = sy + pad;
        if !(STATUS_BAR_LINES..224).contains(&sy) || fy < 0 || fy >= frame.height as i32 {
            continue;
        }
        for x in area.x0..area.x1 {
            let sx = x - frame.camera.0;
            if x < 0 || y < 0 || sx < 0 || sx >= frame.width.min(256) as i32 {
                continue;
            }
            if x >= img.width as i32 || y >= img.height as i32 {
                continue;
            }
            total += 1;
            let want = frame.pixels[fy as usize * frame.width + sx as usize].map(|c| c >> 3);
            let got = img.pixels[(y as u32 * img.width + x as u32) as usize].map(|c| c >> 3);
            same += (want == got) as usize;
        }
    }
    (same, total)
}

fn percent((same, total): (usize, usize)) -> f64 {
    if total == 0 {
        0.0
    } else {
        same as f64 * 100.0 / total as f64
    }
}

#[derive(Default)]
struct Tally {
    levels: Vec<(u16, f64)>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let rom_path = args
        .next()
        .expect("usage: sprite_oracle <rom> <dumpdir>...");
    let dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    assert!(!dirs.is_empty(), "usage: sprite_oracle <rom> <dumpdir>...");
    let rom = Rom::load(&rom_path).expect("ROM must load");
    // Per (sprite number, extra bits): the score in each level it was seen.
    let mut tally: BTreeMap<(u8, u8), Tally> = BTreeMap::new();
    for dir in &dirs {
        for level in levels_in(dir) {
            let frame = read_frame(dir, level).unwrap();
            let rendered = match render::render_level(&rom, level, Default::default()) {
                Ok(rendered) => rendered,
                Err(error) => {
                    eprintln!("level {level:03X}: {error}");
                    continue;
                }
            };
            let img = &rendered.image;
            let screen = Rect {
                x0: frame.camera.0,
                y0: frame.camera.1,
                x1: frame.camera.0 + 256,
                y1: frame.camera.1 + 224,
            };
            let (whole, pad) = FRAME_PADDING
                .map(|pad| (agreement(&frame, img, pad, screen), pad))
                .max_by_key(|(a, _)| a.0)
                .unwrap();
            println!(
                "{} level {level:03X}: whole picture {:.1}% (padding {pad})",
                dir.display(),
                percent(whole)
            );
            let Some(scene) = &rendered.sprites else {
                continue;
            };
            let sizes = expand::object_sizes(scene.object_select);
            let list = sprites::read_sprites_at(&rom, rendered.level.sprite_data_ptr()).unwrap();
            let vertical = rendered.level.tiles.vertical;
            for capture in &scene.captures {
                let Some(area) = capture
                    .objects
                    .iter()
                    .map(|o| {
                        let (w, h) = sizes[o.large as usize];
                        Rect {
                            x0: o.x,
                            y0: o.y,
                            x1: o.x + w,
                            y1: o.y + h,
                        }
                    })
                    .reduce(Rect::union)
                else {
                    continue;
                };
                let seen = agreement(&frame, img, pad, area.intersect(screen));
                if seen.1 == 0 {
                    continue;
                }
                let extra_bits = list
                    .sprites
                    .iter()
                    .find(|entry| {
                        let (x, y) = entry.tile_position(vertical);
                        entry.id == capture.id && (x as i32, y as i32) == (capture.x, capture.y)
                    })
                    .map_or(0, |entry| entry.extra_bits);
                let rate = percent(seen);
                println!(
                    "  sprite {:02X} (extra bits {extra_bits}) at ({}, {}): {} px {rate:.1}%",
                    capture.id, capture.x, capture.y, seen.1
                );
                tally
                    .entry((capture.id, extra_bits))
                    .or_default()
                    .levels
                    .push((level, rate));
            }
        }
    }
    let mut rows: Vec<_> = tally
        .into_iter()
        .map(|((id, extra_bits), t)| {
            let n = t.levels.len();
            let mean = t.levels.iter().map(|(_, r)| r).sum::<f64>() / n as f64;
            let (worst_level, worst) = t
                .levels
                .iter()
                .copied()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            (mean, id, extra_bits, n, worst, worst_level)
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("\nsprite  extra  seen   mean   worst (level)");
    for (mean, id, extra_bits, n, worst, worst_level) in rows {
        println!(
            "  {id:02X}     {extra_bits}    {n:3}  {mean:5.1}%  {worst:5.1}% ({worst_level:03X})"
        );
    }
}
