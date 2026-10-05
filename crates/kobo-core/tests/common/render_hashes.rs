//! A hash of every level's picture, the line format `render_hashes`
//! prints and `tests/fixtures/render_hashes/` holds: diff two runs to
//! check that a change left every picture as it was.
//!
//! A line is `LLL drawn markers diagnostics`: the level, a SHA-1 of its
//! picture with sprites drawn, one with markers and no player, and the
//! first 16 hex digits of a SHA-1 of what the passes reported
//! ([`kobo_core::expand::summarize`]), or `-` if nothing: a pass that stops
//! working can leave the picture as it was. A level that fails to render
//! has its error in place of the hashes.
//! Included by `examples/render_hashes.rs` too.
#![allow(dead_code)]

use kobo_core::Rom;
use kobo_core::expand;
use kobo_core::render::{self, RenderOptions, Sprites};
use sha1::{Digest, Sha1};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn picture(rom: &Rom, level: u16, options: RenderOptions) -> Result<(String, Vec<String>), String> {
    let rendered = render::render_level(rom, level, options).map_err(|e| format!("error: {e}"))?;
    let image = rendered.image;
    let mut hasher = Sha1::new();
    hasher.update(image.width.to_le_bytes());
    hasher.update(image.height.to_le_bytes());
    hasher.update(image.pixels.as_flattened());
    Ok((
        hex(&hasher.finalize()),
        expand::summarize(&rendered.diagnostics),
    ))
}

/// The line of one level.
pub fn line(rom: &Rom, level: u16) -> String {
    let drawn = picture(rom, level, RenderOptions::default());
    let markers = picture(
        rom,
        level,
        RenderOptions {
            sprites: Sprites::Markers,
            player: false,
        },
    );
    match (drawn, markers) {
        (Ok((drawn, mut said)), Ok((markers, more))) => {
            said.extend(more);
            let diagnostics = if said.is_empty() {
                "-".to_string()
            } else {
                hex(&Sha1::digest(said.join("\n").as_bytes())[..8])
            };
            format!("{level:03X} {drawn} {markers} {diagnostics}")
        }
        (Err(e), _) | (_, Err(e)) => format!("{level:03X} {e}"),
    }
}

/// The lines of `levels`, in their order, on every core.
pub fn lines(rom: &Rom, levels: &[u16]) -> Vec<String> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    std::thread::scope(|scope| {
        let handles: Vec<_> = levels
            .chunks(levels.len().div_ceil(workers).max(1))
            .map(|chunk| {
                scope.spawn(move || chunk.iter().map(|&l| line(rom, l)).collect::<Vec<_>>())
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a render worker must not panic"))
            .collect()
    })
}

/// The levels of `levels` whose pictures (drawn and as markers, not what
/// the passes reported) differ between `a` and `b`, each with which:
/// `drawn`, `markers`, or both, or the error one gave.
pub fn pictures_differ(a: &Rom, b: &Rom, levels: &[u16]) -> Vec<(u16, String)> {
    let (x, y) = std::thread::scope(|scope| {
        let x = scope.spawn(|| lines(a, levels));
        let y = lines(b, levels);
        (x.join().expect("a render worker must not panic"), y)
    });
    let mut out = Vec::new();
    for (&level, (x, y)) in levels.iter().zip(x.iter().zip(&y)) {
        let (x, y): (Vec<&str>, Vec<&str>) = (x.split(' ').collect(), y.split(' ').collect());
        if x.get(1) == Some(&"error:") || y.get(1) == Some(&"error:") {
            if x != y {
                out.push((level, "an error".into()));
            }
            continue;
        }
        let which: Vec<&str> = [(1, "drawn"), (2, "markers")]
            .into_iter()
            .filter(|&(i, _)| x.get(i) != y.get(i))
            .map(|(_, name)| name)
            .collect();
        if !which.is_empty() {
            out.push((level, which.join(" and ")));
        }
    }
    out
}

/// The levels on which two runs' lines differ.
pub fn differ(a: &[String], b: &[String]) -> Vec<String> {
    let level = |l: &String| l.get(..3).map(str::to_owned).unwrap_or_default();
    let mut out = Vec::new();
    for l in a {
        if !b.contains(l) {
            out.push(level(l));
        }
    }
    for l in b {
        let n = level(l);
        if !a.contains(l) && !out.contains(&n) {
            out.push(n);
        }
    }
    out.sort();
    out
}
