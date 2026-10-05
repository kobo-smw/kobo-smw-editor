//! The start screen, and the project menu: choosing the clean ROM,
//! opening a project or a recent one, and making a new one, empty, from a
//! hack, or from a baserom template.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use eframe::egui::{self, Align, Layout, RichText};
use kobo_core::{Rom, RomIdentity, config, import, template};

use crate::app::{App, Clean};
use crate::theme;

/// How many recent projects are remembered.
const RECENT: usize = 8;

#[derive(Default)]
pub struct StartState {
    /// Recent projects, newest first.
    pub recent: Vec<PathBuf>,
    /// A project being made on a worker thread: what, and its outcome,
    /// the folder to open.
    task: Option<(String, JoinHandle<Result<PathBuf, String>>)>,
    error: Option<String>,
    /// The template chosen for a new project.
    template: Option<String>,
}

impl StartState {
    /// Puts `dir` first among the recent projects.
    pub fn remember(&mut self, dir: &Path) {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        self.recent.retain(|d| *d != dir);
        self.recent.insert(0, dir);
        self.recent.truncate(RECENT);
    }

    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}

/// A folder for a new project: one that does not exist yet or is empty.
fn pick_new_folder() -> Option<Result<PathBuf, String>> {
    let dir = rfd::FileDialog::new()
        .set_title("A folder for the new project (empty, or a new one)")
        .pick_folder()?;
    let empty = std::fs::read_dir(&dir).map_or(true, |mut entries| entries.next().is_none());
    Some(if empty {
        Ok(dir)
    } else {
        Err(format!(
            "{} has files in it already; a new project needs an empty folder",
            dir.display()
        ))
    })
}

/// Lets the user choose the clean ROM, checks it, and records it in the
/// config file.
fn choose_rom(app: &mut App) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("The clean Super Mario World (USA) ROM")
        .add_filter("SNES ROM", &["sfc", "smc"])
        .pick_file()
    else {
        return;
    };
    let rom = match Rom::load(&path) {
        Ok(rom) => rom,
        Err(e) => {
            app.start.error = Some(e.to_string());
            return;
        }
    };
    if rom.identify() != RomIdentity::VanillaUsa {
        app.start.error = Some(format!(
            "{} is not the clean Super Mario World (USA) ROM: its SHA-1 is {}, not 6b47bb75d16514b6a476aa0c73a683a2a4c18765",
            path.display(),
            rom.sha1_hex()
        ));
        return;
    }
    match config::set_vanilla_rom(&path) {
        Ok(file) => {
            app.start.error = None;
            app.say(format!("The clean ROM is recorded in {}", file.display()));
        }
        Err(e) => app.start.error = Some(format!("Could not record it: {e}")),
    }
    app.clean = Clean::Loaded(Arc::new(rom));
}

/// Opens a project, remembering it.
fn open(app: &mut App, ctx: &egui::Context, dir: &Path) {
    app.open_project(ctx, dir);
    if app.workspace().is_some() {
        app.start.remember(dir);
        app.start.error = None;
        if let Some(level) = app.first_level() {
            app.open_level(level);
        }
    } else {
        app.start.error = app.project_error.take();
    }
}

/// Starts making a project on a worker thread.
fn spawn(
    app: &mut App,
    what: String,
    work: impl FnOnce(&Rom) -> Result<PathBuf, String> + Send + 'static,
) {
    let Clean::Loaded(clean) = &app.clean else {
        return;
    };
    let clean = clean.clone();
    app.start.error = None;
    app.start.task = Some((what, std::thread::spawn(move || work(&clean))));
}

/// Opens the project a finished task made.
pub fn poll(app: &mut App, ctx: &egui::Context) {
    if app
        .start
        .task
        .as_ref()
        .is_some_and(|(_, t)| t.is_finished())
    {
        let (_, task) = app.start.task.take().expect("checked");
        match task.join() {
            Ok(Ok(dir)) => open(app, ctx, &dir),
            Ok(Err(e)) => app.start.error = Some(e),
            Err(_) => app.start.error = Some("making the project stopped unexpectedly".into()),
        }
    }
    if app.start.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn new_empty(app: &mut App) {
    let dir = match pick_new_folder() {
        Some(Ok(dir)) => dir,
        Some(Err(e)) => {
            app.start.error = Some(e);
            return;
        }
        None => return,
    };
    spawn(app, "Making the project".into(), move |_| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let manifest = dir.join(kobo_core::source::project::MANIFEST);
        let text =
            "# A Kobo project. Levels it does not list build as the game has them.\nformat = 1\n";
        std::fs::write(&manifest, text).map_err(|e| format!("{}: {e}", manifest.display()))?;
        Ok(dir)
    });
}

fn new_from_hack(app: &mut App) {
    let Some(hack) = rfd::FileDialog::new()
        .set_title("A hack to import: a ROM, or a BPS patch of the clean ROM")
        .add_filter("ROM or patch", &["sfc", "smc", "bps"])
        .pick_file()
    else {
        return;
    };
    let dir = match pick_new_folder() {
        Some(Ok(dir)) => dir,
        Some(Err(e)) => {
            app.start.error = Some(e);
            return;
        }
        None => return,
    };
    let name = hack
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    spawn(app, format!("Importing {name}"), move |clean| {
        let rom = import::read_hack(&hack, clean).map_err(|e| e.to_string())?;
        import::import_rom(&rom, clean, &dir, false).map_err(|e| e.to_string())?;
        Ok(dir)
    });
}

fn new_from_template(app: &mut App, name: String) {
    let dir = match pick_new_folder() {
        Some(Ok(dir)) => dir,
        Some(Err(e)) => {
            app.start.error = Some(e);
            return;
        }
        None => return,
    };
    spawn(
        app,
        format!("Setting {name} up (downloaded once, then cached)"),
        move |clean| {
            let recipe = template::template(&name).map_err(|e| e.to_string())?;
            recipe.create(clean, &dir).map_err(|e| e.to_string())?;
            Ok(dir)
        },
    );
}

fn card(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(theme::PANEL_RAISED)
        .corner_radius(8)
        .inner_margin(14)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title.to_uppercase())
                    .small()
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
            contents(ui);
        });
    ui.add_space(10.0);
}

/// The start screen.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("KOBO")
                        .size(40.0)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.label(
                    RichText::new("Super Mario World levels, built from source")
                        .color(theme::MUTED),
                );
                ui.add_space(24.0);
            });
            let width = 620.0_f32.min(ui.available_width() - 40.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - width) / 2.0);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    body(app, ui, &ctx);
                });
            });
        });
}

fn body(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    if let Some(error) = &app.start.error {
        ui.colored_label(theme::ERROR, error);
        ui.add_space(8.0);
    }
    if let Some((what, _)) = &app.start.task {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(what);
        });
        ui.add_space(8.0);
    }
    let busy = app.start.busy();

    let mut choose = false;
    card(ui, "The clean ROM", |ui| match &app.clean {
        Clean::Loaded(_) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("✔").color(theme::OK));
                ui.label("Super Mario World (USA), checked");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    choose = ui.small_button("Change…").clicked();
                });
            });
            if let Ok(path) = config::vanilla_rom_path() {
                ui.add(
                    egui::Label::new(
                        RichText::new(path.display().to_string())
                            .small()
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
            }
        }
        Clean::Missing(why) => {
            ui.label("Kobo builds every project onto a clean Super Mario World (USA) ROM, which you provide. It stays outside your projects; Kobo only records where it is.");
            ui.label(RichText::new(why).small().color(theme::MUTED));
            choose = ui.button("Choose the ROM…").clicked();
        }
    });
    if choose {
        choose_rom(app);
    }
    let ready = matches!(app.clean, Clean::Loaded(_)) && !busy;

    let mut open_dir: Option<PathBuf> = None;
    card(ui, "Open", |ui| {
        if ui
            .add_enabled(ready, egui::Button::new("Open a project folder…"))
            .clicked()
        {
            open_dir = rfd::FileDialog::new().pick_folder();
        }
        if !app.start.recent.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Recent").small().color(theme::MUTED));
            for dir in &app.start.recent {
                let exists = dir.join(kobo_core::source::project::MANIFEST).exists();
                let name = dir.file_name().map_or_else(
                    || dir.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &name,
                    0.0,
                    egui::TextFormat::simple(egui::FontId::proportional(14.0), theme::TEXT),
                );
                job.append(
                    &format!("   {}", dir.display()),
                    0.0,
                    egui::TextFormat::simple(egui::FontId::proportional(12.0), theme::MUTED),
                );
                let row = ui
                    .add_enabled(ready && exists, egui::Button::selectable(false, job))
                    .on_disabled_hover_text("The folder has no kobo.toml any more.");
                if row.clicked() {
                    open_dir = Some(dir.clone());
                }
            }
        }
    });
    if let Some(dir) = open_dir {
        open(app, ctx, &dir);
    }

    let mut action = None;
    card(ui, "New project", |ui| {
        let row = |ui: &mut egui::Ui, button: &str, about: &str| {
            ui.horizontal(|ui| {
                let clicked = ui
                    .add_enabled(
                        ready,
                        egui::Button::new(button).min_size(egui::vec2(150.0, 0.0)),
                    )
                    .clicked();
                ui.label(RichText::new(about).color(theme::MUTED));
                clicked
            })
            .inner
        };
        if row(
            ui,
            "Empty…",
            "Every level as the game has it, until you add it to the project",
        ) {
            action = Some(0);
        }
        if row(
            ui,
            "From a hack…",
            "A ROM or BPS patch: its changed levels, Map16, and graphics",
        ) {
            action = Some(1);
        }
        ui.horizontal(|ui| {
            let names: Vec<String> = template::templates().keys().cloned().collect();
            if app.start.template.is_none() {
                app.start.template = names.first().cloned();
            }
            let clicked = ui
                .add_enabled(
                    ready && app.start.template.is_some(),
                    egui::Button::new("From a baserom…").min_size(egui::vec2(150.0, 0.0)),
                )
                .clicked();
            let chosen = app.start.template.clone().unwrap_or_default();
            let label = template::templates()
                .get(&chosen)
                .map_or(chosen.clone(), |t| format!("{} {}", t.title, t.version));
            egui::ComboBox::from_id_salt("template")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (name, recipe) in template::templates() {
                        ui.selectable_value(
                            &mut app.start.template,
                            Some(name.clone()),
                            format!("{} {}", recipe.title, recipe.version),
                        );
                    }
                });
            if clicked {
                action = Some(2);
            }
        });
        ui.label(
            RichText::new("A baserom is downloaded from where its authors publish it and checked; Kobo carries none of it.")
                .small()
                .color(theme::MUTED),
        );
    });
    match action {
        Some(0) => new_empty(app),
        Some(1) => new_from_hack(app),
        Some(2) => {
            if let Some(name) = app.start.template.clone() {
                new_from_template(app, name);
            }
        }
        _ => {}
    }
}

/// The project menu in the top bar: open, new, recent, close.
pub fn menu(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("Project", |ui| {
        if ui.button("Open a project folder…").clicked() {
            ui.close();
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                app.switch_project(Some(dir));
            }
        }
        let recent: Vec<PathBuf> = app.start.recent.iter().skip(1).cloned().collect();
        if !recent.is_empty() {
            ui.menu_button("Recent", |ui| {
                for dir in recent {
                    if ui.button(dir.display().to_string()).clicked() {
                        ui.close();
                        app.switch_project(Some(dir));
                    }
                }
            });
        }
        ui.separator();
        if ui.button("Keyboard shortcuts").clicked() {
            ui.close();
            app.commands.shortcuts = true;
        }
        if ui.button("Close the project").clicked() {
            ui.close();
            app.switch_project(None);
        }
    });
}

/// Opens the project `dir`, or goes back to the start screen, once any
/// unsaved edits are dealt with.
pub fn switch(app: &mut App, ctx: &egui::Context, to: Option<PathBuf>) {
    match to {
        Some(dir) => open(app, ctx, &dir),
        None => app.close_project(),
    }
}
