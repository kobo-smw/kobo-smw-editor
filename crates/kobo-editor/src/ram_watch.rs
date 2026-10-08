//! The watch window: RAM variables read after every frame while a build
//! plays in Mesen (`kobo_core::emulator`), a write to one able to pause the
//! game. Play and Build and play give Mesen the script for the list; the
//! window reads the report it writes beside the ROM.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, DragValue, Grid, RichText};
use kobo_core::emulator::{self, Report, Watch};
use kobo_core::ram::RamAddr;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct WatchState {
    pub open: bool,
    pub watches: Vec<Watch>,
    /// The watch being added: its name, address, and size.
    name: String,
    addr: u32,
    size: u8,
    /// The ROM the last build played was, whose report is read, and the
    /// report as last read, and when.
    playing: Option<PathBuf>,
    report: Option<Report>,
    read_at: Option<Instant>,
}

impl WatchState {
    /// Before Mesen opens the ROM at `rom`: the script for the watches,
    /// beside it; `None` without watches. The report of an earlier play goes.
    pub fn script_for(&mut self, rom: &Path) -> Result<Option<PathBuf>, String> {
        let _ = std::fs::remove_file(emulator::report_path(rom));
        self.report = None;
        self.playing = Some(rom.to_path_buf());
        if self.watches.is_empty() {
            return Ok(None);
        }
        let built = kobo_core::Rom::load(rom).map_err(|e| e.to_string())?;
        let map = kobo_core::ram::RamMap::of(&built);
        let script = emulator::mesen_script(&self.watches, map, &emulator::report_path(rom));
        let path = emulator::script_path(rom);
        std::fs::write(&path, script).map_err(|e| e.to_string())?;
        Ok(Some(path))
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
    let status = match (&state.playing, &state.report) {
        (None, _) => "Not playing: Play or Build and play starts Mesen with these.".to_string(),
        (Some(_), None) => "Waiting for Mesen's first frame.".to_string(),
        (Some(_), Some(r)) => match r.paused {
            Some(p) => format!(
                "Paused at frame {}: {:X} written {:02X}.",
                p.frame, p.addr, p.value
            ),
            None => format!("Frame {}.", r.frame),
        },
    };
    ui.label(RichText::new(status).small());
}
