//! The start screen, and the project menu: choosing the clean ROM,
//! opening a project or a recent one, and making a new one, empty, from a
//! hack, or from a baserom template.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use eframe::egui::{self, Align, Layout, RichText};
use kobo_core::{Rom, RomIdentity, config, import, template};

use rfd::AsyncFileDialog;

use crate::app::{App, Clean};
use crate::dialogs::{self, Pick, Purpose};
use crate::theme;

/// How many recent projects are remembered.
const RECENT: usize = 8;

#[derive(Default)]
pub struct StartState {
    /// Recent projects, newest first.
    pub recent: Vec<PathBuf>,
    /// A project being made on a worker thread: what, and its outcome,
    /// the folder to open and what its import left out.
    task: Option<(String, JoinHandle<Result<Made, String>>)>,
    error: Option<String>,
    /// The template chosen for a new project.
    template: Option<String>,
    /// A locked hack chosen to import, waiting on the user's say.
    locked: Option<PathBuf>,
    /// What the last import left out, until the user closes it.
    pub report: Option<ImportReport>,
}

/// A new project, waiting on its folder.
pub enum NewProject {
    Empty,
    Hack(PathBuf),
    Template(String),
}

/// A project made, and what its import says.
type Made = (PathBuf, Option<ImportReport>);

/// What an import from a hack carried and left out, for the user.
pub struct ImportReport {
    pub hack: String,
    pub locked: bool,
    pub summary: String,
    /// What the hack changes that the project does not carry.
    pub left_out: Option<String>,
    /// Notes on the whole hack.
    pub notes: Vec<String>,
    /// Notes on single levels.
    pub level_notes: Vec<String>,
}

impl ImportReport {
    pub(crate) fn new(hack: String, locked: bool, report: &import::Report) -> Self {
        let summary = format!(
            "{} levels and {} Map16 pages imported.",
            report.levels.len(),
            report.map16.len()
        );
        let changed: usize = report.unmodelled.iter().map(|(_, len)| len).sum();
        let blocks: usize = report.unread_blocks.iter().map(|b| b.len).sum();
        let left_out = (changed + blocks > 0).then(|| {
            format!(
                "The hack changes {} KB of the game outside its levels, Map16, and graphics, and adds {} KB that no level uses: its own code, patches, and tools' work. The project does not carry these, so its levels may look and play otherwise than in the hack until their sources are in the project (kobo.toml's patches and tools).",
                changed.div_ceil(1024),
                blocks.div_ceil(1024)
            )
        });
        let (level_notes, notes) = report
            .notes
            .iter()
            .cloned()
            .partition(|note| note.starts_with("level "));
        Self {
            hack,
            locked,
            summary,
            left_out,
            notes,
            level_notes,
        }
    }
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

/// Asks for a folder for a new project.
fn pick_new_folder(app: &mut App, new: NewProject) {
    dialogs::ask(
        app,
        Purpose::NewFolder(new),
        Pick::Folder,
        AsyncFileDialog::new().set_title("A folder for the new project (empty, or a new one)"),
    );
}

/// Makes the new project in `dir`, a folder that does not exist yet or is
/// empty.
pub(crate) fn folder_chosen(app: &mut App, new: NewProject, dir: PathBuf) {
    let empty = std::fs::read_dir(&dir).map_or(true, |mut entries| entries.next().is_none());
    if !empty {
        app.start.error = Some(format!(
            "{} has files in it already; a new project needs an empty folder",
            dir.display()
        ));
        return;
    }
    match new {
        NewProject::Empty => new_empty(app, dir),
        NewProject::Hack(hack) => new_from_hack(app, hack, dir),
        NewProject::Template(name) => new_from_template(app, name, dir),
    }
}

/// Asks for the clean ROM.
fn ask_for_rom(app: &mut App) {
    dialogs::ask(
        app,
        Purpose::CleanRom,
        Pick::File,
        AsyncFileDialog::new()
            .set_title("The clean Super Mario World (USA) ROM")
            .add_filter("SNES ROM", &["sfc", "smc"]),
    );
}

/// Checks the clean ROM the user chose, and records it in the config file.
pub(crate) fn choose_rom(app: &mut App, path: &Path) {
    let rom = match Rom::load(path) {
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
    match config::set_vanilla_rom(path) {
        Ok(file) => {
            app.start.error = None;
            app.say(format!("The clean ROM is recorded in {}", file.display()));
        }
        Err(e) => app.start.error = Some(format!("Could not record it: {e}")),
    }
    app.entrance_tables = kobo_core::entrance::MainEntranceTables::read(&rom).ok();
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
    work: impl FnOnce(&Rom) -> Result<Made, String> + Send + 'static,
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
            Ok(Ok((dir, report))) => {
                open(app, ctx, &dir);
                if app.workspace().is_some() {
                    app.start.report = report;
                }
            }
            Ok(Err(e)) => app.start.error = Some(e),
            Err(_) => app.start.error = Some("making the project stopped unexpectedly".into()),
        }
    }
    if app.start.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn new_empty(app: &mut App, dir: PathBuf) {
    spawn(app, "Making the project".into(), move |_| {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let manifest = dir.join(kobo_core::source::project::MANIFEST);
        let text =
            "# A Kobo project. Levels it does not list build as the game has them.\nformat = 1\n";
        std::fs::write(&manifest, text).map_err(|e| format!("{}: {e}", manifest.display()))?;
        Ok((dir, None))
    });
}

/// A hack was chosen: one its author locked is imported only once the
/// user knows how little of it comes.
pub(crate) fn hack_chosen(app: &mut App, hack: PathBuf) {
    let Clean::Loaded(clean) = &app.clean else {
        return;
    };
    match import::read_hack(&hack, clean) {
        Ok(rom) if kobo_core::gfx::is_locked(&rom) => app.start.locked = Some(hack),
        Ok(_) => pick_new_folder(app, NewProject::Hack(hack)),
        Err(e) => app.start.error = Some(format!("{}: {e}", hack.display())),
    }
}

fn new_from_hack(app: &mut App, hack: PathBuf, dir: PathBuf) {
    let name = hack
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    spawn(app, format!("Importing {name}"), move |clean| {
        let rom = import::read_hack(&hack, clean).map_err(|e| e.to_string())?;
        let report = import::import_rom(&rom, clean, &dir, false).map_err(|e| e.to_string())?;
        let locked = kobo_core::gfx::is_locked(&rom);
        Ok((dir, Some(ImportReport::new(name, locked, &report))))
    });
}

fn new_from_template(app: &mut App, name: String, dir: PathBuf) {
    spawn(
        app,
        format!("Setting {name} up (downloaded once, then cached)"),
        move |clean| {
            let recipe = template::template(&name).map_err(|e| e.to_string())?;
            recipe.create(clean, &dir).map_err(|e| e.to_string())?;
            Ok((dir, None))
        },
    );
}

/// Asks before importing a hack its author locked.
pub fn confirm_locked(app: &mut App, ctx: &egui::Context) {
    let Some(hack) = app.start.locked.clone() else {
        return;
    };
    let (mut import, mut cancel) = (false, false);
    egui::Modal::new(egui::Id::new("confirm-locked")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading("This hack is locked");
        ui.label(format!(
            "{}'s author locked it against editing: Lunar Magic will not open it, and its levels are partly hidden or encoded by its own code.",
            hack.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ));
        ui.label("Kobo can import what it can read of its levels and Map16, but not its graphics or its code, so the project's levels will not look or play as the hack's, and some will not build.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            import = ui.button("Import what Kobo can").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if cancel {
        app.start.locked = None;
    }
    if import {
        app.start.locked = None;
        pick_new_folder(app, NewProject::Hack(hack));
    }
}

/// What the last import left out, until the user closes it.
pub fn report_window(app: &mut App, ctx: &egui::Context) {
    let Some(report) = &app.start.report else {
        return;
    };
    let mut open = true;
    egui::Window::new(format!("Imported {}", report.hack))
        .open(&mut open)
        .default_width(480.0)
        .collapsible(false)
        .show(ctx, |ui| {
            if report.locked {
                ui.add(
                    egui::Label::new(
                        RichText::new("The hack is locked by its author: its graphics and its own code are not imported, so its levels will not look or play as the hack's.")
                            .color(theme::WARNING),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
            }
            ui.label(&report.summary);
            if let Some(left_out) = &report.left_out {
                ui.add(egui::Label::new(RichText::new(left_out).color(theme::MUTED)).wrap());
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for note in &report.notes {
                    ui.add(egui::Label::new(format!("• {note}")).wrap());
                }
                if !report.level_notes.is_empty() {
                    egui::CollapsingHeader::new(format!(
                        "Notes on single levels ({})",
                        report.level_notes.len()
                    ))
                    .show(ui, |ui| {
                        for note in &report.level_notes {
                            ui.label(RichText::new(note).small());
                        }
                    });
                }
            });
        });
    if !open {
        app.start.report = None;
    }
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
        ask_for_rom(app);
    }
    let ready = matches!(app.clean, Clean::Loaded(_)) && !busy;

    let mut open_dir: Option<PathBuf> = None;
    card(ui, "Open", |ui| {
        if ui
            .add_enabled(ready, egui::Button::new("Open a project folder…"))
            .clicked()
        {
            dialogs::ask(
                app,
                Purpose::OpenProject,
                Pick::Folder,
                AsyncFileDialog::new().set_title("A project folder"),
            );
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
        Some(0) => pick_new_folder(app, NewProject::Empty),
        Some(1) => dialogs::ask(
            app,
            Purpose::Hack,
            Pick::File,
            AsyncFileDialog::new()
                .set_title("A hack to import: a ROM, or a BPS patch of the clean ROM")
                .add_filter("ROM or patch", &["sfc", "smc", "bps"]),
        ),
        Some(2) => {
            if let Some(name) = app.start.template.clone() {
                pick_new_folder(app, NewProject::Template(name));
            }
        }
        _ => {}
    }
}

/// The project menu in the top bar: open, new, recent, close, quit.
pub fn menu(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button("Project", |ui| {
        if ui.button("Open a project folder…").clicked() {
            ui.close();
            dialogs::ask(
                app,
                Purpose::SwitchProject,
                Pick::Folder,
                AsyncFileDialog::new().set_title("A project folder"),
            );
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
        if ui
            .button("This project…")
            .on_hover_text("What it holds, and what its build installs and runs")
            .clicked()
        {
            ui.close();
            app.project_open = true;
        }
        ui.separator();
        if ui
            .button("Import a Lunar Magic level (MWL)…")
            .on_hover_text("As the level it was saved from, in Kobo's format")
            .clicked()
        {
            ui.close();
            dialogs::ask(
                app,
                Purpose::Mwl,
                Pick::File,
                AsyncFileDialog::new()
                    .set_title("A level Lunar Magic saved as an MWL file")
                    .add_filter("Lunar Magic level", &["mwl"]),
            );
        }
        if ui
            .add_enabled(app.current().is_some_and(|o| o.image.is_some()), egui::Button::new("Save the level's picture…"))
            .clicked()
        {
            ui.close();
            let name = app
                .current()
                .map(|o| format!("level-{:03X}.png", o.number))
                .unwrap_or_default();
            dialogs::ask(
                app,
                Purpose::Picture,
                Pick::Save,
                AsyncFileDialog::new()
                    .set_title("Save the level's picture")
                    .set_file_name(&name)
                    .add_filter("PNG", &["png"]),
            );
        }
        let file = app.current().map(|o| o.document.path().to_path_buf());
        if ui
            .add_enabled(file.is_some(), egui::Button::new("Open the level file elsewhere"))
            .on_hover_text("In the editor your system opens it with; the editor here follows what is saved there")
            .clicked()
        {
            ui.close();
            if let Some(file) = file {
                reveal(app, &file);
            }
        }
        if ui.button("Show the project folder").clicked() {
            ui.close();
            if let Some(root) = app.workspace().map(|w| w.project().root.clone()) {
                reveal(app, &root);
            }
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
        if ui
            .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
            .clicked()
        {
            ui.close();
            app.quit();
        }
    });
}

/// Opens a file or folder with what the system opens it with.
pub(crate) fn reveal(app: &mut App, path: &Path) {
    // The tests open nothing.
    if cfg!(test) {
        return;
    }
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        app.say(format!("Could not open {}: {e}", path.display()));
    }
}

/// Opens the project `dir`, or goes back to the start screen, once any
/// unsaved edits are dealt with.
pub fn switch(app: &mut App, ctx: &egui::Context, to: Option<PathBuf>) {
    match to {
        Some(dir) => open(app, ctx, &dir),
        None => app.close_project(),
    }
}
