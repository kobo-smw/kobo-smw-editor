//! The build window: building the project as the editor has it, stage by
//! stage, into a ROM and optionally a BPS patch of the clean ROM.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Layout, RichText};
use kobo_core::build::{BuildError, Stage, StageEvent};
use kobo_core::edit::WorkspaceError;
use kobo_core::{bps, build};

use crate::app::App;
use crate::theme;

/// Where a stage is, as the window shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    Waiting,
    Cached,
    Running,
    Done(Duration),
    Failed,
}

/// What a finished build made.
#[derive(Clone, Debug)]
struct Built {
    path: PathBuf,
    bytes: usize,
    sha1: String,
    bps: Option<(PathBuf, usize)>,
    took: Duration,
}

/// A failed build: what it said, and the level it was about, if one.
#[derive(Clone, Debug)]
struct Failure {
    message: String,
    level: Option<u16>,
}

/// A build in progress: its thread, and what it says as it goes.
type Running = (
    JoinHandle<Result<Built, Failure>>,
    Receiver<(Stage, StageEvent)>,
);

pub struct BuildState {
    pub open: bool,
    /// Open the ROM once built, with what the system opens it with.
    play: bool,
    /// Also write a BPS patch beside the ROM.
    pub bps: bool,
    stages: Vec<(Stage, Status)>,
    running: Option<Running>,
    result: Option<Result<Built, Failure>>,
    notes: Vec<String>,
}

impl Default for BuildState {
    fn default() -> Self {
        Self {
            open: false,
            play: false,
            bps: true,
            stages: Stage::ALL.iter().map(|&s| (s, Status::Waiting)).collect(),
            running: None,
            result: None,
            notes: Vec::new(),
        }
    }
}

impl BuildState {
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Whether the last build finished and wrote its ROM.
    #[cfg(test)]
    pub fn succeeded(&self) -> bool {
        matches!(self.result, Some(Ok(_)))
    }

    /// The stages that ran or came from the cache.
    #[cfg(test)]
    pub fn stages_done(&self) -> usize {
        self.stages
            .iter()
            .filter(|(_, s)| matches!(s, Status::Cached | Status::Done(_)))
            .count()
    }
}

/// Starts building the project as the editor has it: every open level's
/// unsaved edits included, as in its pictures.
pub fn start(app: &mut App) {
    if app.build.busy() {
        return;
    }
    app.sync_levels();
    let Some(workspace) = app.workspace().cloned() else {
        return;
    };
    let root = workspace.project().root.clone();
    let out = root.join("build.sfc");
    let patch = app.build.bps.then(|| root.join("build.bps"));
    let mut notes = build::warnings(workspace.project());
    if let Ok(tools) = build::locate_tools(workspace.project()) {
        notes.extend(tools.iter().filter_map(|t| t.note()));
    }
    let (send, events) = mpsc::channel();
    let ctx = app.ctx().clone();
    let handle = std::thread::spawn(move || {
        let started = Instant::now();
        let mut report = |stage, event| {
            let _ = send.send((stage, event));
            ctx.request_repaint();
        };
        let (rom, symbols) = workspace.build_with_symbols(&mut report).map_err(|e| {
            let level = match &e {
                WorkspaceError::Build(BuildError::Level { level, .. }) => Some(*level),
                _ => None,
            };
            Failure {
                message: e.to_string(),
                level,
            }
        })?;
        let write = |path: &PathBuf, bytes: &[u8]| {
            std::fs::write(path, bytes).map_err(|e| Failure {
                message: format!("{}: {e}", path.display()),
                level: None,
            })
        };
        write(&out, rom.data())?;
        // Beside it, the names of Kobo's and the project's code, for an
        // emulator's debugger.
        write(
            &out.with_extension("sym"),
            build::Symbol::wla(&symbols).as_bytes(),
        )?;
        let bps = match patch {
            Some(path) => {
                let bytes = bps::create(workspace.clean().data(), rom.data());
                write(&path, &bytes)?;
                Some((path, bytes.len()))
            }
            None => None,
        };
        Ok(Built {
            path: out,
            bytes: rom.data().len(),
            sha1: rom.sha1_hex(),
            bps,
            took: started.elapsed(),
        })
    });
    let state = &mut app.build;
    state.open = true;
    state.result = None;
    state.notes = notes;
    state.stages = Stage::ALL.iter().map(|&s| (s, Status::Waiting)).collect();
    state.running = Some((handle, events));
}

/// Takes the build's progress and outcome.
pub fn poll(app: &mut App) {
    let state = &mut app.build;
    let Some((handle, events)) = &state.running else {
        return;
    };
    for (stage, event) in events.try_iter() {
        if let Some((_, status)) = state.stages.iter_mut().find(|(s, _)| *s == stage) {
            *status = match event {
                StageEvent::Cached => Status::Cached,
                StageEvent::Started => Status::Running,
                StageEvent::Finished(took) => Status::Done(took),
            };
        }
    }
    if !handle.is_finished() {
        app.ctx().request_repaint_after(Duration::from_millis(100));
        return;
    }
    let (handle, _) = state.running.take().expect("checked");
    let result = handle.join().unwrap_or_else(|_| {
        Err(Failure {
            message: "the build stopped unexpectedly".into(),
            level: None,
        })
    });
    if result.is_err() {
        for (_, status) in &mut state.stages {
            if *status == Status::Running {
                *status = Status::Failed;
            }
        }
    }
    if state.play
        && let Ok(built) = &result
    {
        let path = built.path.clone();
        state.play = false;
        state.result = Some(result);
        crate::start::reveal(app, &path);
        app.say(format!("Built and opened {}", path.display()));
        return;
    }
    state.play = false;
    let message = match &result {
        Ok(built) => format!(
            "Built {} in {:.1} s",
            built.path.display(),
            built.took.as_secs_f32()
        ),
        Err(failure) => format!("Build failed: {}", failure.message),
    };
    state.result = Some(result);
    app.say(message);
}

/// The build window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.build.open {
        return;
    }
    let mut open = app.build.open;
    let mut go_to = None;
    let mut build_again = false;
    let mut play = false;
    egui::Window::new("Build")
        .open(&mut open)
        .resizable(false)
        .default_width(380.0)
        .anchor(egui::Align2::RIGHT_TOP, [-16.0, 56.0])
        .show(ctx, |ui| {
            let state = &mut app.build;
            ui.label(
                RichText::new(
                    "The project as the editor has it, unsaved edits included, onto the clean ROM.",
                )
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(4.0);
            for (stage, status) in &state.stages {
                ui.horizontal(|ui| {
                    let (mark, color) = match status {
                        Status::Waiting => ("·", theme::MUTED),
                        Status::Cached | Status::Done(_) => ("✔", theme::OK),
                        Status::Running => ("…", theme::ACCENT),
                        Status::Failed => ("✖", theme::ERROR),
                    };
                    ui.label(RichText::new(mark).color(color).monospace());
                    ui.label(stage_name(*stage));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match status {
                        Status::Cached => {
                            ui.label(RichText::new("cached").small().color(theme::MUTED));
                        }
                        Status::Done(took) => {
                            ui.label(
                                RichText::new(format!("{:.2} s", took.as_secs_f32()))
                                    .small()
                                    .monospace()
                                    .color(theme::MUTED),
                            );
                        }
                        Status::Running => {
                            ui.spinner();
                        }
                        _ => {}
                    });
                });
            }
            for note in &state.notes {
                ui.label(RichText::new(note).small().color(theme::WARNING));
            }
            ui.separator();
            match &state.result {
                Some(Ok(built)) => {
                    ui.label(RichText::new(format!(
                        "{} · {} KB · {:.1} s",
                        built.path.display(),
                        built.bytes / 1024,
                        built.took.as_secs_f32()
                    )));
                    ui.label(
                        RichText::new(format!("SHA-1 {}", built.sha1))
                            .small()
                            .monospace()
                            .color(theme::MUTED),
                    );
                    if let Some((path, bytes)) = &built.bps {
                        ui.label(format!("{} · {} KB", path.display(), bytes.div_ceil(1024)));
                    }
                }
                Some(Err(failure)) => {
                    ui.colored_label(theme::ERROR, &failure.message);
                    if let Some(level) = failure.level
                        && ui.button(format!("Open level {level:03X}")).clicked()
                    {
                        go_to = Some(level);
                    }
                }
                None if state.busy() => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Building…");
                    });
                }
                None => {}
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.bps, "Also a BPS patch")
                    .on_hover_text("build.bps, from the clean ROM to the build: what you share");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let button =
                        egui::Button::new(RichText::new("Build").strong()).fill(theme::ACCENT_DIM);
                    build_again = ui
                        .add_enabled(!state.busy(), button)
                        .on_hover_text("Ctrl+B")
                        .clicked();
                    play = ui
                        .add_enabled(!state.busy(), egui::Button::new("Build and play"))
                        .on_hover_text(
                            "Build, then open the ROM with what your system opens it with: your emulator",
                        )
                        .clicked();
                });
            });
        });
    app.build.open = open;
    if build_again || play {
        app.build.play = play;
        start(app);
    }
    if let Some(level) = go_to {
        app.open_level(level);
    }
}

/// A stage as the window names it, with the tool it runs.
fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Base => "Base",
        Stage::SpriteBlocks => "Sprite blocks",
        Stage::Install => "Lunar Magic's layout",
        Stage::EarlyPatches => "Early patches",
        Stage::Music => "Music (AddmusicK)",
        Stage::Graphics => "Graphics",
        Stage::Map16 => "Map16",
        Stage::Sprites => "Sprites (PIXI)",
        Stage::Blocks => "Blocks (GPS)",
        Stage::UberAsm => "UberASM",
        Stage::LatePatches => "Late patches",
        Stage::Levels => "Levels",
    }
}
