//! The watch window: RAM variables read after every frame while a build
//! plays in Mesen (`kobo_core::emulator`), a write to one able to pause the
//! game, and code to stop at, picked from the labels of Kobo's and the
//! project's code that the build played last wrote beside it. Play and
//! Build and play give Mesen the script for the lists; the window reads the
//! report it writes beside the ROM.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, DragValue, Grid, RichText};
use kobo_core::build::Symbol;
use kobo_core::emulator::{self, Breakpoint, Report, Watch};
use kobo_core::ram::RamAddr;

use crate::app::App;
use crate::theme;

pub struct WatchState {
    pub open: bool,
    pub watches: Vec<Watch>,
    /// Whether the player is shown on the canvas: the script reports the
    /// game mode, the level, and the player's place after the watches.
    pub show_player: bool,
    /// Code to stop at.
    pub breaks: Vec<Breakpoint>,
    /// The watch being added: its name, address, and size.
    name: String,
    addr: u32,
    size: u8,
    /// The labels of the build played last (its `.sym`), and the
    /// breakpoint being added: a label's text, or an address.
    labels: Vec<Symbol>,
    label: String,
    code_addr: u32,
    /// The ROM the last build played was, whose report is read, and the
    /// report as last read, and when.
    playing: Option<PathBuf>,
    report: Option<Report>,
    read_at: Option<Instant>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            open: false,
            watches: Vec::new(),
            show_player: true,
            breaks: Vec::new(),
            name: String::new(),
            addr: 0,
            size: 0,
            labels: Vec::new(),
            label: String::new(),
            code_addr: 0x00_8000,
            playing: None,
            report: None,
            read_at: None,
        }
    }
}

/// What the script reports after the watches for the player's marker.
fn player_watches() -> [Watch; 4] {
    use kobo_core::ram::{GAME_MODE, LEVEL_NUMBER, PLAYER_X, PLAYER_Y};
    let watch = |name: &str, addr, size| Watch {
        name: name.into(),
        addr,
        size,
        pause_on_write: false,
    };
    [
        watch("game mode", GAME_MODE, 1),
        watch("level", LEVEL_NUMBER, 2),
        watch("player x", PLAYER_X, 2),
        watch("player y", PLAYER_Y, 2),
    ]
}

impl WatchState {
    /// The level the build is playing and the player's place in it, from
    /// the last report, while in a level (game mode `$14`).
    pub fn player(&self) -> Option<(u16, u16, u16)> {
        if !self.show_player {
            return None;
        }
        let values = &self.report.as_ref()?.values;
        let [mode, level, x, y] = values.get(self.watches.len()..self.watches.len() + 4)? else {
            return None;
        };
        (*mode == 0x14).then_some((*level & 0x1FF, *x, *y))
    }

    /// Before Mesen opens the ROM at `rom`: the script for the watches,
    /// beside it; `None` without watches. The report of an earlier play goes.
    pub fn script_for(&mut self, rom: &Path) -> Result<Option<PathBuf>, String> {
        let _ = std::fs::remove_file(emulator::report_path(rom));
        self.report = None;
        self.playing = Some(rom.to_path_buf());
        if let Ok(text) = std::fs::read_to_string(rom.with_extension("sym")) {
            self.labels = Symbol::read_wla(&text);
        }
        let mut watches = self.watches.clone();
        if self.show_player {
            watches.extend(player_watches());
        }
        if watches.is_empty() && self.breaks.is_empty() {
            return Ok(None);
        }
        let built = kobo_core::Rom::load(rom).map_err(|e| e.to_string())?;
        let map = kobo_core::ram::RamMap::of(&built);
        let script =
            emulator::mesen_script(&watches, &self.breaks, map, &emulator::report_path(rom));
        let path = emulator::script_path(rom);
        std::fs::write(&path, script).map_err(|e| e.to_string())?;
        Ok(Some(path))
    }

    /// A breakpoint at the label `typed` of the build played last, or at
    /// `addr` (named `typed`, or by its address) if there is no such label.
    pub fn add_break(&mut self, typed: &str, addr: u32) {
        let breakpoint = match self.labels.iter().find(|s| s.name == typed) {
            Some(symbol) => Breakpoint {
                name: symbol.name.clone(),
                addr: symbol.addr,
            },
            None => Breakpoint {
                name: if typed.is_empty() {
                    format!("${addr:06X}")
                } else {
                    typed.to_string()
                },
                addr: kobo_core::SnesAddr::new(addr),
            },
        };
        self.breaks.push(breakpoint);
    }

    /// Reads the report again, a few times a second.
    fn refresh(&mut self) {
        let Some(rom) = &self.playing else { return };
        if self
            .read_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(200))
        {
            return;
        }
        self.read_at = Some(Instant::now());
        if let Ok(text) = std::fs::read_to_string(emulator::report_path(rom))
            && let Some(report) = emulator::read_report(&text)
        {
            self.report = Some(report);
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.ram_watch.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Watch")
        .open(&mut open)
        .default_width(420.0)
        .show(ctx, |ui| show(&mut app.ram_watch, ui));
    if !open {
        app.ram_watch.open = false;
    }
    if app.ram_watch.playing.is_some() {
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

fn show(state: &mut WatchState, ui: &mut egui::Ui) {
    state.refresh();
    ui.label(
        RichText::new(
            "Read after every frame while a build plays in Mesen (Play, Build and play), which \
             needs its script window's access to I/O allowed. A watch set to pause stops the \
             game when the variable is written.",
        )
        .small()
        .color(theme::MUTED),
    );
    ui.checkbox(&mut state.show_player, "Show the player on the canvas")
        .on_hover_text("While the build plays the level open, a box where the player is (applies from the next play)");
    let values = state.report.as_ref().map(|r| r.values.clone());
    let mut remove = None;
    Grid::new("watch-list")
        .num_columns(5)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for (i, w) in state.watches.iter_mut().enumerate() {
                ui.label(&w.name);
                ui.label(RichText::new(format!("${:06X}", w.addr.vanilla())).monospace());
                ui.checkbox(&mut w.pause_on_write, "pause").on_hover_text(
                    "Pause the game when it is written (applies from the next play)",
                );
                let value = values.as_ref().and_then(|v| v.get(i));
                ui.label(
                    RichText::new(match value {
                        Some(v) if w.size == 2 => format!("{v:04X}"),
                        Some(v) => format!("{v:02X}"),
                        None => "-".into(),
                    })
                    .monospace(),
                );
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        state.watches.remove(i);
    }
    ui.horizontal(|ui| {
        if state.size == 0 {
            state.size = 1;
            state.addr = 0x7E_0000;
        }
        ui.add(
            egui::TextEdit::singleline(&mut state.name)
                .hint_text("Name")
                .desired_width(120.0),
        );
        ui.add(
            DragValue::new(&mut state.addr)
                .range(0x7E_0000..=0x7F_FFFF)
                .hexadecimal(6, false, true)
                .prefix("$"),
        )
        .on_hover_text(
            "The variable's RAM address, as the game has it (an SA-1 build's moves with it)",
        );
        ui.selectable_value(&mut state.size, 1, "byte");
        ui.selectable_value(&mut state.size, 2, "word");
        if ui.button("Add").clicked() {
            let name = if state.name.trim().is_empty() {
                format!("${:06X}", state.addr)
            } else {
                state.name.trim().to_string()
            };
            state.watches.push(Watch {
                name,
                addr: RamAddr::new(state.addr),
                size: state.size,
                pause_on_write: false,
            });
            state.name.clear();
        }
    });
    breaks(state, ui);
    let status = match (&state.playing, &state.report) {
        (None, _) => "Not playing: Play or Build and play starts Mesen with these.".to_string(),
        (Some(_), None) => "Waiting for Mesen's first frame.".to_string(),
        (Some(_), Some(r)) => match (r.paused, r.stopped) {
            (_, Some((i, frame))) => format!(
                "Stopped at frame {frame}: {}.",
                state.breaks.get(i).map_or("?", |b| b.name.as_str())
            ),
            (Some(p), None) => format!(
                "Paused at frame {}: {:X} written {:02X}.",
                p.frame, p.addr, p.value
            ),
            (None, None) => format!("Frame {}.", r.frame),
        },
    };
    ui.label(RichText::new(status).small());
}

/// The code to stop at, and a breakpoint added by a label of the build
/// played last or by an address.
fn breaks(state: &mut WatchState, ui: &mut egui::Ui) {
    ui.separator();
    ui.label(RichText::new("Break at").strong());
    let mut remove = None;
    Grid::new("break-list")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for (i, b) in state.breaks.iter().enumerate() {
                ui.label(&b.name);
                ui.label(RichText::new(format!("${:06X}", b.addr.raw())).monospace());
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        state.breaks.remove(i);
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.label)
                .hint_text("Label")
                .desired_width(180.0),
        )
        .on_hover_text("A label of Kobo's or the project's code, from the build played last");
        ui.add(
            DragValue::new(&mut state.code_addr)
                .range(0..=0xFF_FFFF)
                .hexadecimal(6, false, true)
                .prefix("$"),
        )
        .on_hover_text("The code's address on the bus, for code without a label");
        if ui.button("Add").clicked() {
            let typed = state.label.trim().to_string();
            state.add_break(&typed, state.code_addr);
            state.label.clear();
        }
    });
    // The labels that start with what is typed, to pick from.
    let typed = state.label.trim().to_string();
    if !typed.is_empty() {
        let mut picked = None;
        for symbol in state
            .labels
            .iter()
            .filter(|s| s.name.starts_with(&typed) && s.name != typed)
            .take(8)
        {
            if ui
                .small_button(format!("{} ${:06X}", symbol.name, symbol.addr.raw()))
                .clicked()
            {
                picked = Some(symbol.clone());
            }
        }
        if let Some(symbol) = picked {
            state.label = symbol.name;
            state.code_addr = symbol.addr.raw();
        }
    }
    if state.labels.is_empty() {
        ui.label(
            RichText::new("Labels come from the build played last (its .sym).")
                .small()
                .color(theme::MUTED),
        );
    }
}
