//! Play from here: a build that starts in the open level at a tile
//! (`kobo_core::playtest`), unsaved edits included, written into the
//! user's cache (`playtest::rom_path`) and opened with what the system
//! opens a ROM with.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use kobo_core::asar::Asar;
use kobo_core::playtest::{self, Start};

use crate::app::{App, OpenLevel};

/// The power-ups the player can start with, by `Start::powerup`.
pub const POWERUPS: [&str; 4] = ["Small Mario", "Super Mario", "Cape Mario", "Fire Mario"];

#[derive(Default)]
pub struct PlayState {
    running: Option<JoinHandle<Result<(PathBuf, Start), String>>>,
    /// The power-up last chosen, which F5 starts with.
    pub powerup: u8,
}

impl PlayState {
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
}

/// Builds the project to start at `start`, on a worker thread.
pub fn start(app: &mut App, start: Start) {
    spawn(app, start.level, Some((start.x, start.y)), start.powerup);
}

/// Builds the project to start in `level` where its main entrance puts
/// the player, found by loading it, on a worker thread.
pub fn from_level_start(app: &mut App, level: u16, powerup: u8) {
    spawn(app, level, None, powerup);
}

/// Plays the open level from where its main entrance puts the player.
pub fn from_start(app: &mut App, powerup: u8) {
    if let Some(level) = app.current_number() {
        from_level_start(app, level, powerup);
    }
}

fn spawn(app: &mut App, level: u16, at: Option<(u16, u16)>, powerup: u8) {
    if app.play.busy() {
        return;
    }
    app.sync_levels();
    let Some(workspace) = app.workspace().cloned() else {
        return;
    };
    app.play.powerup = powerup;
    let Some(out) = playtest::rom_path(&workspace.project().root) else {
        app.say("Could not build to play: there is no cache folder to put it in");
        return;
    };
    let ctx = app.ctx().clone();
    let handle = std::thread::spawn(move || {
        let result = (|| {
            let (x, y) = match at {
                Some(at) => at,
                None => playtest::entrance_tile(&workspace, level).map_err(|e| e.to_string())?,
            };
            let start = Start {
                level,
                x,
                y,
                powerup,
            };
            let asar = Asar::configured().map_err(|e| e.to_string())?;
            let rom = playtest::build(&workspace, &start, &asar).map_err(|e| e.to_string())?;
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&out, rom.data()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok((out, start))
        })();
        ctx.request_repaint();
        result
    });
    app.play.running = Some(handle);
    app.say(match at {
        Some((x, y)) => format!("Building level {level:03X} to play from ({x}, {y})…"),
        None => format!("Building level {level:03X} to play from its start…"),
    });
}

/// Opens the build once it is done.
pub fn poll(app: &mut App) {
    let Some(handle) = &app.play.running else {
        return;
    };
    if !handle.is_finished() {
        app.ctx().request_repaint_after(Duration::from_millis(100));
        return;
    }
    let handle = app.play.running.take().expect("checked");
    match handle.join() {
        Ok(Ok((path, start))) => {
            crate::start::reveal(app, &path);
            app.say(format!(
                "Playing level {:03X} from ({}, {}) as {}: {}",
                start.level,
                start.x,
                start.y,
                POWERUPS[usize::from(start.powerup & 3)],
                path.display()
            ));
        }
        Ok(Err(e)) => app.say(format!("Could not build to play: {e}")),
        Err(_) => app.say("The build to play stopped unexpectedly"),
    }
}

/// Where the player starts for a click at level pixel (`x`, `y`): standing
/// in the clicked tile, his top a tile above it; and when the click is on
/// something (the ground, a wall), on top of it: on the first surface
/// above the click with room for him, if one is near.
pub fn start_at(open: &OpenLevel, (x, y): (f32, f32), powerup: u8) -> Start {
    let tile = |p: f32| (p / 16.0).floor().max(0.0) as u16;
    let (x, mut y) = (tile(x), tile(y));
    if let (Some(geometry), Some(rom)) = (&open.geometry, open.built.as_deref()) {
        let tiles = &geometry.loaded.tiles;
        let (width, height) = tiles.size();
        let acts = |x: u16, y: u16| {
            (usize::from(x) < width && usize::from(y) < height).then(|| {
                let tile = tiles.tile_at(usize::from(x), usize::from(y));
                kobo_core::map16::pages::acts_like_in(rom, tile)
            })
        };
        let solid = |y: u16| acts(x, y).is_some_and(|a| (SOLID_FROM..SOLID_TO).contains(&a));
        let on_something = acts(x, y).is_some_and(|a| a != BLANK);
        if on_something
            && let Some(top) = (0..=CLIMB)
                .filter_map(|up| y.checked_sub(up))
                .find(|&top| top >= 2 && solid(top) && !solid(top - 1) && !solid(top - 2))
        {
            y = top - 1;
        }
    }
    Start {
        level: open.number,
        x,
        y: y.saturating_sub(1),
        powerup,
    }
}

/// The empty tile.
const BLANK: u16 = 0x025;
/// The tiles the player stands on: page 1's, which are the game's ledges,
/// blocks, slopes, and walls; page 0's are its passable ones (water,
/// coins, vines, the background, the inside of the ground).
const SOLID_FROM: u16 = 0x100;
const SOLID_TO: u16 = 0x200;
/// How far up a start on something looks for its top.
const CLIMB: u16 = 16;
