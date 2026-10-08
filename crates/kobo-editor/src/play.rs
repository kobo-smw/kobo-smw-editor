//! Play from here: a build that starts in the open level at a tile, or at
//! its main entrance (`kobo_core::playtest`), unsaved edits included,
//! written into the user's cache (`playtest::rom_path`) and opened with
//! what the system opens a ROM with. The game starts as the play settings
//! say (the power-up, the switch palaces, the ON/OFF switch), which the
//! menu beside the Play button sets.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use eframe::egui::{self, RichText};
use kobo_core::asar::Asar;
use kobo_core::playtest::{self, SWITCHES, Settings, Start};

use crate::app::{App, OpenLevel};
use crate::theme;

/// The power-ups the player can start with, by `Start::powerup`.
pub const POWERUPS: [&str; 4] = ["Small Mario", "Super Mario", "Cape Mario", "Fire Mario"];

#[derive(Default)]
pub struct PlayState {
    running: Option<JoinHandle<Result<(PathBuf, Start), String>>>,
    /// How the game starts: the power-up last chosen, which F5 and Play
    /// start with, the switch palaces, and the ON/OFF switch.
    pub settings: Settings,
}

impl PlayState {
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
}

/// Builds the project to start at `start`, on a worker thread, as the
/// play settings say but for its power-up.
pub fn start(app: &mut App, start: Start) {
    spawn(app, start.level, start.at, start.settings.powerup);
}

/// Builds the project to start in `level` at its main entrance, on a
/// worker thread.
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
    app.play.settings.powerup = powerup;
    let settings = app.play.settings;
    let Some(out) = playtest::rom_path(&workspace.project().root) else {
        app.say("Could not build to play: there is no cache folder to put it in");
        return;
    };
    let ctx = app.ctx().clone();
    let handle = std::thread::spawn(move || {
        let result = (|| {
            let start = Start {
                level,
                at,
                settings,
            };
            let asar = Asar::configured().map_err(|e| e.to_string())?;
            let (rom, symbols) = playtest::build_with_symbols(&workspace, &start, &asar)
                .map_err(|e| e.to_string())?;
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&out, rom.data()).map_err(|e| format!("{}: {e}", out.display()))?;
            // An emulator's debugger finds the code's names beside it.
            let sym = out.with_extension("sym");
            std::fs::write(&sym, kobo_core::build::Symbol::wla(&symbols))
                .map_err(|e| format!("{}: {e}", sym.display()))?;
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
            let from = match start.at {
                Some((x, y)) => format!("({x}, {y})"),
                None => "its start".to_string(),
            };
            app.say(format!(
                "Playing level {:03X} from {from} as {}: {}",
                start.level,
                POWERUPS[usize::from(start.settings.powerup & 3)],
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
        at: Some((x, y.saturating_sub(1))),
        settings: Settings {
            powerup,
            ..Settings::default()
        },
    }
}

/// The menu beside the Play button: how the game starts, for every way
/// of playing.
pub fn settings_menu(app: &mut App, button: &egui::Response) {
    let settings = &mut app.play.settings;
    egui::Popup::from_toggle_button_response(button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(200.0);
            ui.label(RichText::new("Start as").small().color(theme::MUTED));
            for (i, name) in POWERUPS.iter().enumerate() {
                ui.radio_value(&mut settings.powerup, i as u8, *name);
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("Switch palaces pressed")
                    .small()
                    .color(theme::MUTED),
            );
            for (i, name) in SWITCHES.iter().enumerate() {
                let mut on = settings.switches & 1 << i != 0;
                if ui.checkbox(&mut on, *name).changed() {
                    settings.switches ^= 1 << i;
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("ON/OFF switch").small().color(theme::MUTED));
            ui.horizontal(|ui| {
                ui.radio_value(&mut settings.off, false, "On");
                ui.radio_value(&mut settings.off, true, "Off");
            });
        });
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
