//! Whole-picture comparison against emulator frames: `KOBO_VIDEO_ORACLE_DIRS`
//! lists (`:`-separated) directories of `KOBO_ORACLE_VIDEO=1` dumps of the
//! vanilla ROM, or of the ROM `KOBO_ORACLE_ROM` names. Each level is rendered with sprites, cropped to the
//! emulator's camera, and compared pixel by pixel below the status bar.
//! Sprites animate and move and tides and animated tiles cycle, so
//! agreement is high rather than total; the threshold catches layers,
//! palettes, colour math, and the player's placement going wrong.

mod common;

use kobo_core::render;
use std::path::{Path, PathBuf};

/// The status bar occupies the top of the picture with its own scroll.
const STATUS_BAR_LINES: usize = 36;
/// Mesen's frame is 239 lines with the 224-line picture inside; the
/// picture's first line lands on one of these frame rows depending on the
/// capture, so the comparison tries each and keeps the best.
const FRAME_PADDING: std::ops::RangeInclusive<i32> = 4..=9;
const MIN_MATCH: f64 = 0.85;

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
    // The layer 1 position from work RAM: the PPU's scroll registers keep
    // only ten bits, which is not enough for a vertical level.
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
            let name = e
                .expect("oracle directory must be readable")
                .file_name()
                .into_string()
                .ok()?;
            let hex = name.strip_prefix("level_")?.strip_suffix(".ppm")?;
            u16::from_str_radix(hex, 16).ok()
        })
        .collect();
    levels.sort_unstable();
    levels
}

#[test]
fn rendered_levels_match_emulator_frames() {
    let Some(rom) = common::oracle_rom() else {
        return;
    };
    let Some(list) = std::env::var_os("KOBO_VIDEO_ORACLE_DIRS") else {
        eprintln!("skipping: KOBO_VIDEO_ORACLE_DIRS is not set");
        return;
    };
    let dirs: Vec<PathBuf> = std::env::split_paths(&list).collect();
    assert!(
        !dirs.is_empty(),
        "configured video oracle has no directories"
    );
    let mut failures = Vec::new();
    for dir in &dirs {
        let levels = levels_in(dir);
        assert!(
            !levels.is_empty(),
            "{}: configured video oracle has no frames",
            dir.display()
        );
        for level in levels {
            let frame = read_frame(dir, level).unwrap();
            let rendered = render::render_level(&rom, level, Default::default()).unwrap();
            let (img, loaded) = (rendered.image, rendered.level);
            let (rate, pad) = FRAME_PADDING
                .map(|pad| (match_rate(&frame, &img, pad), pad))
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            eprintln!(
                "{}: level {level:03X} mode {} matches {:.1}% (padding {pad})",
                dir.display(),
                loaded.tiles.level_mode,
                rate * 100.0
            );
            if rate < MIN_MATCH {
                failures.push(format!("level {level:03X}: {:.1}%", rate * 100.0));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Fraction of picture pixels below the status bar whose 15-bit colour
/// equals the rendered level's at the camera position, with the picture
/// starting `pad` rows into the frame.
fn match_rate(frame: &Frame, img: &kobo_core::image::RgbImage, pad: i32) -> f64 {
    let (mut same, mut total) = (0usize, 0usize);
    for sy in STATUS_BAR_LINES..224 {
        let fy = sy as i32 + pad;
        if fy < 0 || fy >= frame.height as i32 {
            continue;
        }
        for sx in 0..frame.width.min(256) {
            let (x, y) = (frame.camera.0 + sx as i32, frame.camera.1 + sy as i32);
            if x < 0 || y < 0 || x >= img.width as i32 || y >= img.height as i32 {
                continue;
            }
            total += 1;
            let want = frame.pixels[fy as usize * frame.width + sx].map(|c| c >> 3);
            let got = img.pixels[(y as u32 * img.width + x as u32) as usize].map(|c| c >> 3);
            same += (want == got) as usize;
        }
    }
    if total == 0 {
        0.0
    } else {
        same as f64 / total as f64
    }
}
