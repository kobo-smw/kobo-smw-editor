//! Play from here: a build that starts in the open level at a tile
//! (`kobo_core::playtest`), unsaved edits included, written beside the
//! project's build as `play.sfc` and opened with what the system opens a
//! ROM with.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use kobo_core::asar::Asar;
use kobo_core::playtest::{self, Start};

use crate::app::App;

/// The power-ups the player can start with, by `Start::powerup`.
pub const POWERUPS: [&str; 4] = ["Small Mario", "Super Mario", "Cape Mario", "Fire Mario"];

#[derive(Default)]
pub struct PlayState {
    running: Option<(JoinHandle<Result<PathBuf, String>>, Start)>,
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
    if app.play.busy() {
        return;
    }
    app.sync_levels();
    let Some(workspace) = app.workspace().cloned() else {
        return;
    };
    let out = workspace.project().root.join("play.sfc");
    let ctx = app.ctx().clone();
    let handle = std::thread::spawn(move || {
        let result = (|| {
            let asar = Asar::configured().map_err(|e| e.to_string())?;
            let rom = playtest::build(&workspace, &start, &asar).map_err(|e| e.to_string())?;
            std::fs::write(&out, rom.data()).map_err(|e| format!("{}: {e}", out.display()))?;
            Ok(out)
        })();
        ctx.request_repaint();
        result
    });
    app.play.running = Some((handle, start));
    app.say(format!(
        "Building level {:03X} to play from ({}, {})…",
        start.level, start.x, start.y
    ));
}

/// Opens the build once it is done.
pub fn poll(app: &mut App) {
    let Some((handle, _)) = &app.play.running else {
        return;
    };
    if !handle.is_finished() {
        app.ctx().request_repaint_after(Duration::from_millis(100));
        return;
    }
    let (handle, start) = app.play.running.take().expect("checked");
    match handle.join() {
        Ok(Ok(path)) => {
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
/// in the clicked tile, his top a tile above it.
pub fn start_at(level: u16, (x, y): (f32, f32), powerup: u8) -> Start {
    let tile = |p: f32| (p / 16.0).floor().max(0.0) as u16;
    Start {
        level,
        x: tile(x),
        y: tile(y).saturating_sub(1),
        powerup,
    }
}
