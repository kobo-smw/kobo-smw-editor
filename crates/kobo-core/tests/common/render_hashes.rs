//! A hash of every level's picture, the line format `render_hashes`
//! prints and `tests/fixtures/render_hashes/` holds: diff two runs to
//! check that a change left every picture as it was.
//!
//! A line is `LLL drawn markers diagnostics`: the level, a SHA-1 of its
//! picture with sprites drawn, one with markers and no player, and the
//! first 16 hex digits of a SHA-1 of what the passes reported
//! ([`kobo_core::expand::summarize`]), or `-` if nothing: a pass that stops
//! working can leave the picture as it was. A level that fails to render
//! has its error in place of the hashes. With tiles asked for, a line that
//! loaded ends in `tiles=` and a SHA-1 of what the load resolved
//! ([`tiles_hash`]).
//! Included by `examples/render_hashes.rs` too.
#![allow(dead_code)]

use kobo_core::Rom;
use kobo_core::expand;
use kobo_core::render::{self, RenderOptions, Sprites};
use sha1::{Digest, Sha1};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A SHA-1 of a picture, as the lines give it.
fn picture_hash(image: &kobo_core::image::RgbImage) -> String {
    let mut hasher = Sha1::new();
    hasher.update(image.width.to_le_bytes());
    hasher.update(image.height.to_le_bytes());
    hasher.update(image.pixels.as_flattened());
    hex(&hasher.finalize())
}

/// A SHA-1 of what a level's load resolves, graphics and palettes aside:
/// the layer 1 grid, the background tilemap, and the foreground and BG
/// Map16 definitions the level's tiles use (`examples/tiles_diff.rs`
/// compares the same, part by part). Two loads with the same hash resolve
/// alike.
pub fn tiles_hash(tiles: &kobo_core::expand::LevelTiles) -> String {
    let mut hasher = Sha1::new();
    let field = |hasher: &mut Sha1, bytes: &[u8]| {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    field(&mut hasher, &tiles.low);
    field(&mut hasher, &tiles.high);
    let used: std::collections::BTreeSet<u16> = tiles
        .low
        .iter()
        .zip(&tiles.high)
        .map(|(&l, &h)| u16::from_le_bytes([l, h]))
        .collect();
    for t in used {
        field(
            &mut hasher,
            format!("{t:X} {:?}", tiles.map16.get(&t)).as_bytes(),
        );
    }
    match &tiles.layer2_tilemap {
        Some((low, high)) => {
            field(&mut hasher, b"background");
            field(&mut hasher, low);
            field(&mut hasher, high);
            field(&mut hasher, &(tiles.layer2_screen_len as u64).to_le_bytes());
            let used: std::collections::BTreeSet<usize> = low
                .iter()
                .zip(high)
                .map(|(&l, &h)| usize::from(u16::from_le_bytes([l, h])))
                .collect();
            for t in used {
                field(
                    &mut hasher,
                    format!("{t:X} {:?}", tiles.bg_map16.get(t)).as_bytes(),
                );
            }
        }
        None => field(&mut hasher, b"none"),
    }
    hex(&hasher.finalize())
}

/// The line of one level.
pub fn line(rom: &Rom, level: u16) -> String {
    line_with(rom, level, false)
}

/// The line of one level, ending in `tiles=` and its [`tiles_hash`] when
/// `tiles` is set (a level that loads but does not draw too, after its
/// error). The level is loaded once, and both pictures drawn
/// from that load, as `render_level` draws each from a load of its own.
pub fn line_with(rom: &Rom, level: u16, tiles: bool) -> String {
    let loaded = match expand::expand_level(rom, level) {
        Ok(loaded) => loaded,
        Err(e) => return format!("{level:03X} error: {e}"),
    };
    let picture = |options| {
        render::render_loaded(rom, &loaded, options)
            .map(|(image, scene)| {
                // What render_level reports: the load's, then the pass's.
                let mut said = loaded.diagnostics.clone();
                said.extend(scene);
                (picture_hash(&image), expand::summarize(&said))
            })
            .map_err(|e| format!("error: {e}"))
    };
    let drawn = picture(RenderOptions::default());
    let markers = picture(RenderOptions {
        sprites: Sprites::Markers,
        player: false,
        hidden_layers: 0,
    });
    let line = match (drawn, markers) {
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
    };
    if tiles {
        line + &format!(" tiles={}", tiles_hash(&loaded.tiles))
    } else {
        line
    }
}

/// `work` on each of `items`, in their order, on every core: each worker
/// takes the next item when it is done with one, so that a slow level
/// does not hold a fixed share of the rest behind it.
pub fn on_every_core<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let result = work(item);
                    results.lock().expect("no worker panics holding it")[i] = Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("no worker panics holding it")
        .into_iter()
        .map(|r| r.expect("every item was taken"))
        .collect()
}

/// The lines of `levels`, in their order, on every core.
pub fn lines(rom: &Rom, levels: &[u16]) -> Vec<String> {
    lines_with(rom, levels, false)
}

/// [`lines`], with each level's [`tiles_hash`] when `tiles` is set.
pub fn lines_with(rom: &Rom, levels: &[u16], tiles: bool) -> Vec<String> {
    on_every_core(levels, |&level| line_with(rom, level, tiles))
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
