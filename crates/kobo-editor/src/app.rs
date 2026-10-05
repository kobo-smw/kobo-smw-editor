//! The editor's window: the project, the open levels, and the panels.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Key, KeyboardShortcut, Layout, Modifiers, RichText};
use kobo_core::edit::{self, Edit, LevelDocument, Reload, Workspace};
use kobo_core::render::{RenderOptions, Sprites};
use kobo_core::{Rom, config};

use crate::canvas::{self, Camera, Drag};
use crate::palette::{PaletteState, Placing};
use crate::picture::Picture;
use crate::preview::{Previewer, Rendered};
use crate::selection::{Geometry, Item};
use crate::watch::Watcher;
use crate::{inspector, outline, palette, source, theme};

/// How the canvas shows a level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct View {
    pub grid: bool,
    pub screens: bool,
    pub sprites: SpriteView,
    pub player: bool,
    pub source: bool,
    /// Markers where the player enters.
    pub entrances: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpriteView {
    Drawn,
    Markers,
    Hidden,
}

impl View {
    fn render_options(self) -> RenderOptions {
        RenderOptions {
            sprites: match self.sprites {
                SpriteView::Drawn => Sprites::Drawn,
                SpriteView::Markers => Sprites::Markers,
                SpriteView::Hidden => Sprites::Hidden,
            },
            player: self.player,
        }
    }
}

/// A move or other edit already in the document whose picture is not in
/// yet: the canvas draws it over the old picture until it is.
pub struct Pending {
    pub items: Vec<Item>,
    pub delta: (i32, i32),
    pub request: u64,
}

/// A level open in the editor: its document, its picture, and how it is
/// being looked at. Kept while other levels are shown, so unsaved edits
/// stay.
pub struct OpenLevel {
    pub number: u16,
    pub document: LevelDocument,
    pub picture: Option<Picture>,
    /// The last picture's load and what it says of objects and sprites.
    pub geometry: Option<Geometry>,
    pub diagnostics: Vec<String>,
    pub entries: Vec<crate::preview::Entry>,
    pub render_error: Option<String>,
    pub rendered_in: Option<Duration>,
    /// The newest preview asked for, and the one shown.
    pub requested: u64,
    pub shown: u64,
    pub selection: Vec<Item>,
    pub camera: Option<Camera>,
    /// Where the canvas was on the last frame, in screen points.
    pub canvas: egui::Rect,
    /// Where on the level the canvas's menu was opened.
    pub menu_at: Option<egui::Pos2>,
    pub drag: Option<Drag>,
    pub pending: Option<Pending>,
    /// Bring the selection into view on the next frame.
    pub focus: bool,
    /// The selection the outline last scrolled to.
    pub outline_scrolled_to: Option<Item>,
    /// The file changed on disk while the document had unsaved edits:
    /// the file's text, until the user chooses.
    pub conflict: Option<String>,
    /// The file on disk does not parse.
    pub disk_error: Option<String>,
    pub source: source::SourceState,
}

impl OpenLevel {
    fn new(number: u16, document: LevelDocument) -> Self {
        Self {
            number,
            document,
            picture: None,
            geometry: None,
            diagnostics: Vec::new(),
            entries: Vec::new(),
            render_error: None,
            rendered_in: None,
            requested: 0,
            shown: 0,
            selection: Vec::new(),
            camera: None,
            canvas: egui::Rect::NOTHING,
            menu_at: None,
            drag: None,
            pending: None,
            focus: false,
            outline_scrolled_to: None,
            conflict: None,
            disk_error: None,
            source: source::SourceState::default(),
        }
    }

    /// Whether the picture shows the document as it is.
    pub fn up_to_date(&self) -> bool {
        self.shown == self.requested && self.render_error.is_none()
    }

    /// Drops selected items the document no longer has.
    fn keep_valid_selection(&mut self) {
        let level = self.document.level();
        self.selection.retain(|item| item.exists(level));
    }

    pub fn file_name(&self) -> String {
        self.document
            .path()
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }
}

/// What the left panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LeftTab {
    Levels,
    Add,
    Outline,
}

/// Where the clean ROM comes from, or why it could not be loaded.
pub(crate) enum Clean {
    Loaded(Arc<Rom>),
    Missing(String),
}

/// Options given on the command line.
#[derive(Default, Clone)]
pub struct Startup {
    pub project: Option<PathBuf>,
    pub level: Option<u16>,
    /// Select these layer 1 objects once the level is open.
    pub select: Vec<usize>,
    pub source: bool,
    /// Show the palette rather than the level list.
    pub palette: bool,
    /// Build once the level is open.
    pub build: bool,
    /// Save a picture of the window here once the level's picture is in,
    /// then quit: for documentation and checks without a display.
    pub screenshot: Option<PathBuf>,
}

pub struct App {
    pub(crate) clean: Clean,
    workspace: Option<Workspace>,
    pub(crate) project_error: Option<String>,
    /// The start screen's state, and the recent projects.
    pub start: crate::start::StartState,
    /// A project to switch to (or `Some(None)`, the start screen), waiting
    /// on what to do with unsaved edits.
    switching: Option<Option<PathBuf>>,
    ctx: egui::Context,
    open: BTreeMap<u16, OpenLevel>,
    current: Option<u16>,
    previewer: Previewer,
    watcher: Option<Watcher>,
    pub view: View,
    level_filter: String,
    /// List the levels the project leaves as the game's too.
    all_levels: bool,
    /// A level the user asked to add from the game's own, to confirm.
    pub(crate) adding: Option<u16>,
    /// Where the level panel's copy goes.
    pub copy_to: u16,
    left: LeftTab,
    /// What the outline is narrowed to.
    pub outline_filter: String,
    pub palette: PaletteState,
    /// What a click on the canvas places, while choosing from the palette.
    pub placing: Option<Placing>,
    /// The layer the palette's objects go on.
    pub place_layer: edit::ObjectLayer,
    /// What was copied, to paste in any level.
    pub clipboard: crate::clipboard::Clipboard,
    /// A message for the status bar, and when it was given.
    status: Option<(String, Instant)>,
    /// The inspector widget whose change is being made, so that dragging
    /// a value is one undo step.
    pub editing: Option<egui::Id>,
    /// The window was asked to close with unsaved edits.
    confirm_close: bool,
    allow_close: bool,
    /// The build window and the build in progress.
    pub build: crate::build::BuildState,
    startup: Startup,
    screenshot_frames: Option<u32>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        theme::apply(&cc.egui_ctx);
        let clean = match config::vanilla_rom_path() {
            Ok(path) => match Rom::load(&path) {
                Ok(rom) if rom.identify() == kobo_core::RomIdentity::VanillaUsa => {
                    Clean::Loaded(Arc::new(rom))
                }
                Ok(rom) => Clean::Missing(format!(
                    "{} is not the clean Super Mario World (USA) ROM (SHA-1 {}).",
                    path.display(),
                    rom.sha1_hex()
                )),
                Err(e) => Clean::Missing(e.to_string()),
            },
            Err(e) => Clean::Missing(e.to_string()),
        };
        let mut app = Self {
            clean,
            workspace: None,
            project_error: None,
            start: Default::default(),
            switching: None,
            ctx: cc.egui_ctx.clone(),
            open: BTreeMap::new(),
            current: None,
            previewer: Previewer::new(cc.egui_ctx.clone()),
            watcher: None,
            view: View {
                grid: false,
                screens: true,
                sprites: SpriteView::Drawn,
                player: true,
                source: startup.source,
                entrances: true,
            },
            level_filter: String::new(),
            all_levels: false,
            adding: None,
            copy_to: 0,
            left: if startup.palette {
                LeftTab::Add
            } else {
                LeftTab::Levels
            },
            outline_filter: String::new(),
            palette: PaletteState::default(),
            placing: None,
            place_layer: edit::ObjectLayer::One,
            clipboard: Default::default(),
            status: None,
            editing: None,
            confirm_close: false,
            allow_close: false,
            build: Default::default(),
            startup: startup.clone(),
            screenshot_frames: None,
        };
        if let Some(storage) = cc.storage {
            app.start.recent = eframe::get_value(storage, RECENT_KEY).unwrap_or_default();
        }
        if let Some(dir) = &startup.project {
            app.open_project(&cc.egui_ctx, dir);
            if app.workspace.is_some() {
                app.start.remember(dir);
            }
            let level = startup.level.or_else(|| app.first_level());
            if let Some(level) = level {
                app.open_level(level);
                if let Some(open) = app.open.get_mut(&level) {
                    open.focus = !startup.select.is_empty();
                    open.selection = startup
                        .select
                        .iter()
                        .map(|&index| Item::object(edit::ObjectLayer::One, index))
                        .collect();
                }
            }
        }
        if startup.build {
            crate::build::start(&mut app);
        }
        app
    }

    pub(crate) fn first_level(&self) -> Option<u16> {
        self.workspace.as_ref()?.levels().next()
    }

    pub fn say(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now()));
    }

    /// Opens the project `dir`, or goes back to the start screen with
    /// `None`; with unsaved edits, once the user says what to do with
    /// them.
    pub fn switch_project(&mut self, to: Option<PathBuf>) {
        if self.modified().count() > 0 {
            self.switching = Some(to);
        } else {
            let ctx = self.ctx.clone();
            crate::start::switch(self, &ctx, to);
        }
    }

    /// Back to the start screen, dropping the open levels.
    pub(crate) fn close_project(&mut self) {
        self.workspace = None;
        self.watcher = None;
        self.open.clear();
        self.current = None;
        self.placing = None;
        self.palette.forget_pictures();
    }

    pub(crate) fn open_project(&mut self, ctx: &egui::Context, dir: &Path) {
        let Clean::Loaded(clean) = &self.clean else {
            return;
        };
        match Workspace::open(dir, clean.clone()) {
            Ok(workspace) => {
                self.close_project();
                self.watcher = Watcher::new(dir, ctx.clone()).ok();
                self.workspace = Some(workspace);
                self.project_error = None;
                self.open.clear();
                self.current = None;
            }
            Err(e) => self.project_error = Some(format!("{}: {e}", dir.display())),
        }
    }

    pub fn open_level(&mut self, number: u16) {
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = self.open.entry(number) {
            let Some(path) = workspace.level_path(number) else {
                return;
            };
            match LevelDocument::open(&path) {
                Ok(document) => {
                    entry.insert(OpenLevel::new(number, document));
                }
                Err(e) => {
                    self.say(format!("Could not open level {number:03X}: {e}"));
                    return;
                }
            }
        }
        self.current = Some(number);
        let has_layer2 = self.open.get(&number).is_some_and(|o| {
            matches!(
                o.document.level().layer2,
                kobo_core::source::level::Layer2::Objects(_)
            )
        });
        if !has_layer2 {
            self.place_layer = edit::ObjectLayer::One;
        }
        self.request_preview(number);
    }

    /// Puts the level's document in the workspace and asks for its
    /// picture.
    pub fn request_preview(&mut self, number: u16) {
        let (Some(workspace), Some(open)) = (&mut self.workspace, self.open.get_mut(&number))
        else {
            return;
        };
        workspace.set_level(number, open.document.level());
        open.requested = self
            .previewer
            .request(workspace, number, self.view.render_options());
    }

    fn take_preview(&mut self, ctx: &egui::Context) {
        self.take_entries();
        let Some(done) = self.previewer.poll() else {
            return;
        };
        let Some(open) = self.open.values_mut().find(|o| o.requested == done.id) else {
            return;
        };
        open.shown = done.id;
        match done.result {
            Ok(rendered) => {
                let Rendered {
                    image,
                    loaded,
                    sprites,
                    diagnostics,
                    entries,
                    took,
                } = rendered;
                // The secondary entrances follow; until they do, the last
                // ones stay.
                let old: Vec<_> = open
                    .entries
                    .drain(..)
                    .filter(|e| matches!(e.kind, crate::preview::EntryKind::Secondary(_)))
                    .collect();
                open.entries = entries;
                open.entries.extend(old);
                open.picture = Some(Picture::new(ctx, &image));
                open.geometry = Some(Geometry::new(
                    loaded,
                    sprites,
                    open.document.level().clone(),
                ));
                open.diagnostics = diagnostics;
                open.render_error = None;
                open.rendered_in = Some(took);
                if open.pending.as_ref().is_some_and(|p| p.request <= done.id) {
                    open.pending = None;
                }
            }
            Err(e) => {
                open.render_error = Some(e);
                open.pending = None;
            }
        }
    }

    /// The secondary entrances found for a level's picture: with the main
    /// entrance and the midway, which came with it.
    fn take_entries(&mut self) {
        let Some((id, found)) = self.previewer.take_entries() else {
            return;
        };
        if let Some(open) = self.open.values_mut().find(|o| o.shown == id) {
            open.entries
                .retain(|e| !matches!(e.kind, crate::preview::EntryKind::Secondary(_)));
            open.entries.extend(found);
        }
    }

    /// Applies edits to the open level as one undo step.
    pub fn apply(&mut self, label: &str, edits: Vec<Edit>) -> bool {
        let Some(number) = self.current else {
            return false;
        };
        let Some(open) = self.open.get_mut(&number) else {
            return false;
        };
        match open.document.apply(label, &edits) {
            Ok(()) => {
                open.keep_valid_selection();
                self.request_preview(number);
                true
            }
            Err(e) => {
                self.say(format!("{label}: {e}"));
                false
            }
        }
    }

    /// Applies edits as part of the last undo step: a value being dragged.
    pub fn amend(&mut self, edits: Vec<Edit>) {
        let Some(number) = self.current else { return };
        let Some(open) = self.open.get_mut(&number) else {
            return;
        };
        match open.document.amend(&edits) {
            Ok(()) => self.request_preview(number),
            Err(e) => self.say(e.to_string()),
        }
    }

    pub fn workspace(&self) -> Option<&Workspace> {
        self.workspace.as_ref()
    }

    pub fn current_number(&self) -> Option<u16> {
        self.current
    }

    pub fn open_mut(&mut self, number: u16) -> Option<&mut OpenLevel> {
        self.open.get_mut(&number)
    }

    pub fn current(&self) -> Option<&OpenLevel> {
        self.open.get(&self.current?)
    }

    pub fn current_mut(&mut self) -> Option<&mut OpenLevel> {
        self.open.get_mut(&self.current?)
    }

    fn undo(&mut self, redo: bool) {
        let Some(number) = self.current else { return };
        let Some(open) = self.open.get_mut(&number) else {
            return;
        };
        let label = if redo {
            open.document.redo_label()
        } else {
            open.document.undo_label()
        }
        .map(str::to_owned);
        let done = if redo {
            open.document.redo()
        } else {
            open.document.undo()
        };
        if done {
            open.keep_valid_selection();
            open.pending = None;
            self.request_preview(number);
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
    }

    fn save_all(&mut self) {
        let mut saved = Vec::new();
        let mut failed = Vec::new();
        for open in self.open.values_mut() {
            if !open.document.is_modified() {
                continue;
            }
            match open.document.save() {
                Ok(()) => {
                    open.conflict = None;
                    saved.push(open.file_name());
                }
                Err(e) => failed.push(e.to_string()),
            }
        }
        if !failed.is_empty() {
            self.say(failed.join("; "));
        } else if !saved.is_empty() {
            self.say(format!("Saved {}", saved.join(", ")));
        }
    }

    fn modified(&self) -> impl Iterator<Item = &OpenLevel> {
        self.open.values().filter(|o| o.document.is_modified())
    }

    /// Follows the files that changed outside the editor.
    fn follow_files(&mut self) {
        let Some(watcher) = &self.watcher else { return };
        let changed: Vec<PathBuf> = watcher
            .changed()
            .into_iter()
            .filter(|p| matters(p))
            .collect();
        if changed.is_empty() {
            return;
        }
        let mut project_changed = false;
        let mut redraw = Vec::new();
        for path in &changed {
            let open = self
                .open
                .values_mut()
                .find(|o| same_file(o.document.path(), path));
            let Some(open) = open else {
                project_changed = true;
                continue;
            };
            match open.document.reload() {
                Ok(Reload::Unchanged) => {}
                Ok(Reload::Reloaded) => {
                    open.disk_error = None;
                    open.keep_valid_selection();
                    redraw.push(open.number);
                }
                Ok(Reload::Conflict(text)) => {
                    open.disk_error = None;
                    open.conflict = Some(text);
                }
                Err(e) => open.disk_error = Some(e.to_string()),
            }
        }
        if project_changed {
            let keep: Vec<(u16, kobo_core::source::level::Level)> = self
                .open
                .values()
                .map(|o| (o.number, o.document.level().clone()))
                .collect();
            if let Some(workspace) = &mut self.workspace {
                match workspace.reload(&keep) {
                    Ok(()) => {
                        self.project_error = None;
                        self.palette.forget_pictures();
                        redraw.extend(self.current);
                    }
                    Err(e) => self.project_error = Some(e.to_string()),
                }
            }
        }
        redraw.sort_unstable();
        redraw.dedup();
        for number in redraw {
            if Some(number) == self.current {
                self.request_preview(number);
            } else if let Some(open) = self.open.get_mut(&number) {
                // Drawn again when it is shown again.
                open.requested = u64::MAX;
            }
        }
    }

    /// Puts every open level's state in the workspace, as a build of it
    /// should have them.
    pub fn sync_levels(&mut self) {
        if let Some(workspace) = &mut self.workspace {
            for open in self.open.values() {
                workspace.set_level(open.number, open.document.level());
            }
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let shift_command = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let (save, undo, redo, redo_y, build) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&command(Key::S)),
                i.consume_shortcut(&command(Key::Z)),
                i.consume_shortcut(&shift_command(Key::Z)),
                i.consume_shortcut(&command(Key::Y)),
                i.consume_shortcut(&command(Key::B)),
            )
        });
        if save {
            self.save_all();
        }
        // A text field undoes its own typing.
        let typing = ctx.egui_wants_keyboard_input();
        if undo && !typing {
            self.undo(false);
        }
        if (redo || redo_y) && !typing {
            self.undo(true);
        }
        if build {
            crate::build::start(self);
        }
        if !typing {
            canvas::keys(self, ctx);
            crate::clipboard::keys(self, ctx);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new("KOBO").strong().color(theme::ACCENT));
            if self.workspace.is_some() {
                crate::start::menu(self, ui);
            }
            if let Some(workspace) = &self.workspace {
                let root = &workspace.project().root;
                let name = root.file_name().map_or_else(
                    || root.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                ui.label(RichText::new(name).strong());
            }
            if let Some(open) = self.current() {
                ui.label(RichText::new("/").color(theme::MUTED));
                ui.label(RichText::new(format!("{:03X}", open.number)).strong());
                if open.document.is_modified() {
                    ui.label(RichText::new("● modified").color(theme::ACCENT).small());
                }
            }
            if self.workspace.is_none() {
                return;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let building = self.build.busy();
                let build = ui
                    .add_enabled(
                        self.workspace.is_some() && !building,
                        egui::Button::new(RichText::new("⚙ Build").strong())
                            .fill(theme::ACCENT_DIM),
                    )
                    .on_hover_text("Build the project into build.sfc (Ctrl+B)");
                if build.clicked() {
                    crate::build::start(self);
                }
                let modified = self.modified().count();
                let save = ui
                    .add_enabled(modified > 0, egui::Button::new("Save"))
                    .on_hover_text("Save every changed level (Ctrl+S)");
                if save.clicked() {
                    self.save_all();
                }
                let (undo_label, redo_label) = self.current().map_or((None, None), |o| {
                    (
                        o.document.undo_label().map(str::to_owned),
                        o.document.redo_label().map(str::to_owned),
                    )
                });
                let redo = ui
                    .add_enabled(redo_label.is_some(), egui::Button::new("Redo"))
                    .on_hover_text(format!(
                        "Redo {} (Ctrl+Shift+Z)",
                        redo_label.unwrap_or_default()
                    ));
                if redo.clicked() {
                    self.undo(true);
                }
                let undo = ui
                    .add_enabled(undo_label.is_some(), egui::Button::new("Undo"))
                    .on_hover_text(format!("Undo {} (Ctrl+Z)", undo_label.unwrap_or_default()));
                if undo.clicked() {
                    self.undo(false);
                }
                ui.separator();
                ui.toggle_value(&mut self.view.source, "Source")
                    .on_hover_text("Show the level file beside the canvas");
            });
        });
    }

    fn view_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.view.screens, "Screens");
            ui.toggle_value(&mut self.view.grid, "Grid");
            ui.toggle_value(&mut self.view.entrances, "Entrances")
                .on_hover_text(
                    "Where the player enters: the start, the midway, and each secondary entrance",
                );
            ui.separator();
            let before = (self.view.sprites, self.view.player);
            ui.label(RichText::new("Sprites").color(theme::MUTED));
            ui.selectable_value(&mut self.view.sprites, SpriteView::Drawn, "Drawn");
            ui.selectable_value(&mut self.view.sprites, SpriteView::Markers, "IDs");
            ui.selectable_value(&mut self.view.sprites, SpriteView::Hidden, "Off");
            ui.toggle_value(&mut self.view.player, "Player");
            if before != (self.view.sprites, self.view.player)
                && let Some(number) = self.current
            {
                self.request_preview(number);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(open) = self.current_mut()
                    && let Some(camera) = &mut open.camera
                {
                    if ui.button("Fit").clicked() {
                        camera.fit_height = true;
                    }
                    if ui.button("+").clicked() {
                        camera.zoom_by(2.0);
                    }
                    ui.label(RichText::new(format!("{:.0}%", camera.zoom * 100.0)).monospace());
                    if ui.button("−").clicked() {
                        camera.zoom_by(0.5);
                    }
                }
            });
        });
    }

    fn level_list(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.level_filter)
                .hint_text("Find a level")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let Some(workspace) = &self.workspace else {
            return;
        };
        ui.checkbox(&mut self.all_levels, "The game's own levels too")
            .on_hover_text("Levels the project does not list build as the game has them. Choose one to add it.");
        let filter = self.level_filter.trim().to_ascii_uppercase();
        let listed: Vec<u16> = if self.all_levels {
            (0..0x200).collect()
        } else {
            workspace.levels().collect()
        };
        let levels: Vec<u16> = listed
            .into_iter()
            .filter(|n| filter.is_empty() || format!("{n:03X}").contains(&filter))
            .collect();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                    for number in levels {
                        let modified = self
                            .open
                            .get(&number)
                            .is_some_and(|o| o.document.is_modified());
                        let about = match workspace.level(number) {
                            Some(level) => {
                                let tileset =
                                    kobo_core::names::object_tileset(level.header.object_tileset)
                                        .unwrap_or("?");
                                format!("{tileset} · {}", level.header.screens)
                            }
                            None => "the game's own".to_string(),
                        };
                        let mut job = egui::text::LayoutJob::default();
                        let font = egui::FontId::monospace(12.5);
                        job.append(
                            &format!("{number:03X}"),
                            0.0,
                            egui::TextFormat::simple(font, theme::TEXT),
                        );
                        if modified {
                            job.append(
                                " ●",
                                0.0,
                                egui::TextFormat::simple(
                                    egui::FontId::proportional(12.0),
                                    theme::ACCENT,
                                ),
                            );
                        }
                        job.append(
                            &format!("  {about}"),
                            0.0,
                            egui::TextFormat::simple(
                                egui::FontId::proportional(12.0),
                                theme::MUTED,
                            ),
                        );
                        let selected = self.current == Some(number);
                        if ui.selectable_label(selected, job).clicked() {
                            clicked = Some(number);
                        }
                    }
                });
            });
        if let Some(number) = clicked {
            if workspace.level_path(number).is_some() {
                self.open_level(number);
            } else {
                self.adding = Some(number);
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, hover: Option<canvas::Hover>) {
        ui.horizontal_centered(|ui| {
            if let Some(hover) = hover {
                ui.label(format!("({}, {})", hover.x, hover.y));
                ui.label(RichText::new(format!("screen {:02X}", hover.screen)).color(theme::MUTED));
                if let Some(tile) = hover.tile {
                    ui.label(RichText::new(format!("Map16 {tile:03X}")).color(theme::MUTED));
                }
                if let Some(name) = hover.owner {
                    ui.label(RichText::new(name).color(theme::MUTED));
                }
            }
            let recent = self
                .status
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(8));
            if recent && let Some(message) = self.status() {
                ui.separator();
                ui.label(message);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let Some(open) = self.current() else { return };
                if let Some(error) = &open.render_error {
                    ui.label(RichText::new(format!("Not drawn: {error}")).color(theme::ERROR));
                } else if self.previewer.busy() {
                    ui.spinner();
                    ui.label("Drawing…");
                } else if let Some(took) = open.rendered_in {
                    ui.label(
                        RichText::new(format!("Up to date · {:.2} s", took.as_secs_f32()))
                            .color(theme::MUTED),
                    );
                }
                let selected = open.selection.len();
                if selected > 0 {
                    ui.label(format!("{selected} selected"));
                }
            });
        });
    }

    fn banners(&mut self, ui: &mut egui::Ui) {
        if let Some(error) = &self.project_error {
            ui.colored_label(theme::ERROR, error);
        }
        let Some(number) = self.current else { return };
        let Some(open) = self.open.get_mut(&number) else {
            return;
        };
        if let Some(error) = &open.disk_error {
            ui.colored_label(
                theme::ERROR,
                format!("{} does not read: {error}", open.file_name()),
            );
        }
        let mut take = false;
        let mut keep = false;
        if open.conflict.is_some() {
            ui.horizontal(|ui| {
                ui.colored_label(
                    theme::ACCENT,
                    format!(
                        "{} changed on disk, and has edits here too.",
                        open.file_name()
                    ),
                );
                take = ui.button("Take the file's").clicked();
                keep = ui.button("Keep mine").clicked();
            });
        }
        if take && let Some(text) = open.conflict.take() {
            match open.document.take_disk_text(text) {
                Ok(()) => {
                    open.keep_valid_selection();
                    self.request_preview(number);
                }
                Err(e) => open.disk_error = Some(e.to_string()),
            }
        } else if keep && let Some(text) = open.conflict.take() {
            open.document.keep_over(text);
        }
    }

    /// Asks what to do with unsaved edits before switching projects.
    fn confirm_switch(&mut self, ctx: &egui::Context) {
        let Some(to) = self.switching.clone() else {
            return;
        };
        let names: Vec<String> = self.modified().map(OpenLevel::file_name).collect();
        let (mut go, mut cancel) = (false, false);
        egui::Modal::new(egui::Id::new("confirm-switch")).show(ctx, |ui| {
            ui.heading("Save your changes?");
            ui.label(format!("Unsaved: {}", names.join(", ")));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save_all();
                    go = self.modified().count() == 0;
                }
                go |= ui.button("Don't save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel {
            self.switching = None;
        }
        if go {
            self.switching = None;
            crate::start::switch(self, ctx, to);
        }
    }

    /// Adds level `number` to the project as `level`, and opens it.
    pub fn add_level(&mut self, number: u16, level: &kobo_core::source::level::Level) {
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        match workspace.add_level(number, level) {
            Ok(path) => {
                self.say(format!("Added level {number:03X} as {}", path.display()));
                self.open_level(number);
            }
            Err(e) => self.say(format!("Could not add level {number:03X}: {e}")),
        }
    }

    /// Asks before adding a level the project leaves as the game's own.
    fn confirm_adding(&mut self, ctx: &egui::Context) {
        let Some(number) = self.adding else { return };
        let mut add = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("confirm-add")).show(ctx, |ui| {
            ui.heading(format!("Level {number:03X}"));
            ui.label("The project does not list this level, so it builds as the game has it.");
            ui.label("Add it to the project to edit it, starting from the game's own.");
            ui.horizontal(|ui| {
                add = ui.button("Add to the project").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel {
            self.adding = None;
        }
        if add {
            self.adding = None;
            let level = self.workspace.as_ref().map(|w| w.clean_level(number));
            match level {
                Some(Ok(level)) => self.add_level(number, &level),
                Some(Err(e)) => self.say(format!("Could not read level {number:03X}: {e}")),
                None => {}
            }
        }
    }

    /// The status bar's last message.
    pub fn status(&self) -> Option<&str> {
        self.status.as_ref().map(|(message, _)| message.as_str())
    }

    /// Whether the project lists level `number`.
    pub fn has_level(&self, number: u16) -> bool {
        self.workspace
            .as_ref()
            .is_some_and(|w| w.level_path(number).is_some())
    }

    fn close_requests(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.modified().count() > 0
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if !self.confirm_close {
            return;
        }
        let names: Vec<String> = self.modified().map(OpenLevel::file_name).collect();
        let mut close = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("confirm-close")).show(ctx, |ui| {
            ui.heading("Save your changes?");
            ui.label(format!("Unsaved: {}", names.join(", ")));
            ui.horizontal(|ui| {
                if ui.button("Save and close").clicked() {
                    self.save_all();
                    close = self.modified().count() == 0;
                }
                if ui.button("Close without saving").clicked() {
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if close {
            self.allow_close = true;
            self.confirm_close = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if cancel {
            self.confirm_close = false;
        }
    }

    /// With `--screenshot`, saves the window once the level is drawn.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.startup.screenshot.clone() else {
            return;
        };
        let ready = self.current().is_some_and(|o| {
            o.picture.is_some() && o.up_to_date() && !self.palette.busy() && !self.build.busy()
        }) || matches!(self.clean, Clean::Missing(_))
            || self.workspace.is_none();
        match &mut self.screenshot_frames {
            None if ready => self.screenshot_frames = Some(0),
            Some(frames) => {
                *frames += 1;
                // A few frames for the layout to settle, then one more for
                // the picture to arrive.
                if *frames == 3 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
            }
            None => {}
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let mut rgb =
                kobo_core::image::RgbImage::new(image.size[0] as u32, image.size[1] as u32);
            for (i, pixel) in image.pixels.iter().enumerate() {
                rgb.pixels[i] = [pixel.r(), pixel.g(), pixel.b()];
            }
            if let Err(e) = rgb.write_png(&path) {
                eprintln!("{}: {e}", path.display());
            }
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Whether a changed file could matter to the project: not the build's
/// output, git's own files, or an editor's scratch files.
fn matters(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    !(path.components().any(|c| c.as_os_str() == ".git")
        || name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || matches!(ext.as_deref(), Some("sfc" | "smc" | "bps")))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

/// Where the recent projects are kept in the editor's storage.
const RECENT_KEY: &str = "kobo-recent-projects";

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT_KEY, &self.start.recent);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_preview(&ctx);
        crate::start::poll(self, &ctx);
        self.follow_files();
        crate::build::poll(self);
        self.shortcuts(&ctx);
        if self.previewer.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Panel::top("top")
            .frame(theme::bar_frame())
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .frame(theme::bar_frame())
            .show(ui, |ui| {
                let hovered = self.current().and_then(|_| canvas::last_hover(&ctx));
                self.status_bar(ui, hovered);
            });
        if self.workspace.is_none() {
            egui::CentralPanel::default().show(ui, |ui| crate::start::show(self, ui));
            self.close_requests(&ctx);
            self.screenshot(&ctx);
            return;
        }
        egui::Panel::left("levels")
            .resizable(true)
            .default_size(220.0)
            .frame(theme::side_frame())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.left, LeftTab::Levels, "Levels");
                    ui.selectable_value(&mut self.left, LeftTab::Add, "Add");
                    ui.selectable_value(&mut self.left, LeftTab::Outline, "Outline");
                });
                ui.separator();
                match self.left {
                    LeftTab::Levels => self.level_list(ui),
                    LeftTab::Add => palette::show(self, ui),
                    LeftTab::Outline => outline::show(self, ui),
                }
            });
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .frame(theme::side_frame())
            .show(ui, |ui| inspector::show(self, ui));
        if self.view.source {
            egui::Panel::right("source")
                .resizable(true)
                .default_size(520.0)
                .frame(theme::side_frame())
                .show(ui, |ui| source::show(self, ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::CANVAS))
            .show(ui, |ui| {
                self.banners(ui);
                self.view_bar(ui);
                canvas::show(self, ui);
            });
        crate::build::window(self, &ctx);
        self.confirm_adding(&ctx);
        self.confirm_switch(&ctx);
        self.close_requests(&ctx);
        self.screenshot(&ctx);
    }
}

/// Colours for the panels.
pub fn color_for(item: Item) -> Color32 {
    match item {
        Item::Object(_) => theme::SELECTION,
        Item::Sprite(_) => theme::SPRITE,
    }
}
