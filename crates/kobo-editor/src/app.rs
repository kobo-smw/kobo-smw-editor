//! The editor's window: the project, the open levels, and the panels.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, Key, KeyboardShortcut, Layout, Modifiers, PointerButton, RichText,
};
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
    /// Marks of what changed since the last commit (the Changes tab).
    pub changes: bool,
    /// Layers left out of the picture (`RenderOptions::hidden_layers`).
    pub hidden_layers: u8,
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
            hidden_layers: self.hidden_layers,
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
    /// The picture's pixels, for saving it.
    pub image: Option<std::sync::Arc<egui::ColorImage>>,
    /// The last picture's load and what it says of objects and sprites.
    pub geometry: Option<Geometry>,
    pub diagnostics: Vec<String>,
    pub entries: Vec<crate::preview::Entry>,
    /// The sprites the sprite capture gave up on, by tile, and why.
    pub failed_sprites: Vec<(i32, i32, String)>,
    /// Other levels that do not build, which the picture's build left out.
    pub left_out: Vec<(u16, String)>,
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
    /// Bring the selection into view on the next frame; with `centre`,
    /// to the middle of the view even if it is in view already.
    pub focus: bool,
    pub centre: bool,
    /// Bring this entrance into view once the picture has it.
    pub look_at: Option<crate::preview::EntryKind>,
    /// The build the picture is of.
    pub built: Option<std::sync::Arc<kobo_core::Rom>>,
    /// The selection the outline last scrolled to.
    pub outline_scrolled_to: Option<Item>,
    /// The file changed on disk while the document had unsaved edits:
    /// the file's text, until the user chooses.
    pub conflict: Option<String>,
    /// The file on disk does not parse.
    pub disk_error: Option<String>,
    pub source: source::SourceState,
    /// The level as the last commit has it, for the Changes tab.
    pub head: Option<crate::changes::Head>,
    pub no_head: Option<crate::changes::NoHead>,
}

impl OpenLevel {
    fn new(number: u16, document: LevelDocument) -> Self {
        Self {
            number,
            document,
            picture: None,
            image: None,
            geometry: None,
            diagnostics: Vec::new(),
            entries: Vec::new(),
            failed_sprites: Vec::new(),
            left_out: Vec::new(),
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
            centre: false,
            look_at: None,
            built: None,
            outline_scrolled_to: None,
            conflict: None,
            disk_error: None,
            source: source::SourceState::default(),
            head: None,
            no_head: None,
        }
    }

    /// Whether the picture shows the document as it is.
    #[cfg(test)]
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
pub(crate) enum LeftTab {
    Levels,
    Add,
    Outline,
    Changes,
    Find,
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
    /// The left panel's tab: `levels`, `add`, `outline`, or `changes`.
    pub tab: Option<String>,
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
    /// The overworld's names of the levels, read when first asked for.
    level_names: std::sync::OnceLock<Vec<Option<String>>>,
    /// The levels shown before this one, and after it once gone back.
    history: Vec<u16>,
    future: Vec<u16>,
    /// An entrance to bring into view when its level is opened next.
    pub(crate) look_at: Option<(u16, crate::preview::EntryKind)>,
    /// A level the user asked to take out of the project, to confirm.
    pub(crate) removing: Option<u16>,
    /// Where the level panel's copy goes.
    pub copy_to: u16,
    pub(crate) left: LeftTab,
    /// The command palette and the shortcuts window.
    pub commands: crate::commands::CommandState,
    /// Every level as a picture, in place of the canvas.
    pub overview: crate::overview::Overview,
    /// What the outline is narrowed to.
    pub outline_filter: String,
    pub find: crate::find::FindState,
    pub palette: PaletteState,
    /// What a click on the canvas places, while choosing from the palette.
    pub placing: Option<Placing>,
    /// The layer the palette's objects go on.
    pub place_layer: edit::ObjectLayer,
    /// What was copied, to paste in any level.
    pub clipboard: crate::clipboard::Clipboard,
    /// Where the game puts the player for a main entrance's settings, from
    /// the clean ROM.
    pub entrance_tables: Option<kobo_core::entrance::MainEntranceTables>,
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
    pub backgrounds: crate::backgrounds::Backgrounds,
    pub play: crate::play::PlayState,
    /// The project window is open.
    pub project_open: bool,
    startup: Startup,
    screenshot_frames: Option<u32>,
    /// The window's title as last set.
    shown_title: String,
    /// The levels shown last, newest first: only these keep their
    /// pictures, which are large.
    viewed: std::collections::VecDeque<u16>,
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
                changes: false,
                hidden_layers: 0,
            },
            level_filter: String::new(),
            all_levels: false,
            adding: None,
            history: Vec::new(),
            level_names: Default::default(),
            future: Vec::new(),
            look_at: None,
            removing: None,
            copy_to: 0,
            left: match (startup.palette, startup.tab.as_deref()) {
                (true, _) | (_, Some("add" | "sprites" | "map16")) => LeftTab::Add,
                (_, Some("outline")) => LeftTab::Outline,
                (_, Some("changes")) => LeftTab::Changes,
                (_, Some("find")) => LeftTab::Find,
                _ => LeftTab::Levels,
            },
            outline_filter: String::new(),
            find: Default::default(),
            commands: Default::default(),
            overview: Default::default(),
            palette: PaletteState::default(),
            placing: None,
            place_layer: edit::ObjectLayer::One,
            clipboard: Default::default(),
            entrance_tables: None,
            status: None,
            editing: None,
            confirm_close: false,
            allow_close: false,
            build: Default::default(),
            backgrounds: Default::default(),
            play: Default::default(),
            project_open: false,
            startup: startup.clone(),
            screenshot_frames: None,
            shown_title: String::new(),
            viewed: Default::default(),
        };
        if let Clean::Loaded(rom) = &app.clean {
            app.entrance_tables = kobo_core::entrance::MainEntranceTables::read(rom).ok();
        }
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
        app.overview.open = startup.tab.as_deref() == Some("overview");
        app.project_open = startup.tab.as_deref() == Some("project");
        app.backgrounds.open = startup.tab.as_deref() == Some("backgrounds");
        match startup.tab.as_deref() {
            Some("sprites") => app.palette.show(crate::palette::Kind::Sprites),
            Some("map16") => app.palette.show(crate::palette::Kind::Map16),
            _ => {}
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
        self.overview.forget();
        self.overview.open = false;
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

    /// Shows level `number`, which the Back command then leaves for the
    /// one shown before.
    pub fn open_level(&mut self, number: u16) {
        let before = self.current;
        self.show_level(number);
        if let Some(before) = before
            && self.current != Some(before)
        {
            self.history.push(before);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
            self.future.clear();
        }
    }

    /// Goes back to the level shown before this one, or with `forward`
    /// on again.
    pub fn go_back(&mut self, forward: bool) {
        let level = if forward {
            self.future.pop()
        } else {
            self.history.pop()
        };
        let Some(level) = level else {
            return;
        };
        let before = self.current;
        self.show_level(level);
        if let Some(before) = before
            && self.current == Some(level)
        {
            if forward {
                self.history.push(before);
            } else {
                self.future.push(before);
            }
        }
    }

    pub fn can_go_back(&self, forward: bool) -> bool {
        !if forward { &self.future } else { &self.history }.is_empty()
    }

    fn show_level(&mut self, number: u16) {
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
        // Pictures are kept for the levels shown last; another one's goes,
        // and is drawn again when it is shown again.
        self.viewed.retain(|&n| n != number);
        self.viewed.push_front(number);
        while self.viewed.len() > KEPT_PICTURES {
            if let Some(old) = self.viewed.pop_back()
                && let Some(open) = self.open.get_mut(&old)
            {
                open.picture = None;
                open.image = None;
                open.built = None;
                open.requested = u64::MAX;
            }
        }
        let has_layer2 = self.open.get(&number).is_some_and(|o| {
            matches!(
                o.document.level().layer2,
                kobo_core::source::level::Layer2::Objects(_)
            )
        });
        if !has_layer2 {
            self.place_layer = edit::ObjectLayer::One;
        }
        if let Some((_, kind)) = self.look_at.take_if(|(n, _)| *n == number)
            && let Some(open) = self.open.get_mut(&number)
        {
            open.look_at = Some(kind);
        }
        self.request_preview(number);
    }

    /// Where a screen exit of level `number` leads: the level, and the
    /// secondary entrance it comes in by, if one. `None` for an entrance no
    /// level has.
    pub fn exit_leads(
        &self,
        number: u16,
        exit: kobo_core::level::objects::ScreenExit,
    ) -> Option<(u16, Option<u16>)> {
        let target = edit::ExitTarget::of(exit, number);
        if target.secondary {
            let level = self.workspace()?.entrance_level(target.destination)?;
            Some((level, Some(target.destination)))
        } else {
            Some((target.destination, None))
        }
    }

    /// Opens the level a screen exit leads to, its entrance in view. A
    /// level the project does not list is offered to add first.
    pub fn follow_exit(&mut self, (level, entrance): (u16, Option<u16>)) {
        let kind = match entrance {
            Some(id) => crate::preview::EntryKind::Secondary(id),
            None => crate::preview::EntryKind::Main,
        };
        self.look_at = Some((level, kind));
        if self.workspace().and_then(|w| w.level_path(level)).is_none() {
            self.adding = Some(level);
        } else {
            self.open_level(level);
        }
    }

    /// The name the overworld gives level `number`, in title case, if it
    /// has one: read once from the clean ROM, whose names a build keeps.
    pub fn level_name(&self, number: u16) -> Option<&str> {
        let workspace = self.workspace.as_ref()?;
        let names = self.level_names.get_or_init(|| {
            (0..0x200)
                .map(|n| {
                    kobo_core::level::level_name(workspace.clean(), n)
                        .map(|name| kobo_core::names::title_case(&name))
                })
                .collect()
        });
        names.get(usize::from(number))?.as_deref()
    }

    /// Puts the open level back as its file has it, as one undo step.
    pub fn back_to_saved(&mut self) {
        let Some(number) = self.current else { return };
        let Some(open) = self.open.get_mut(&number) else {
            return;
        };
        let path = open.document.path().to_path_buf();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                open.document
                    .set_text("Back to the saved file", &text)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                open.keep_valid_selection();
                self.request_preview(number);
            }
            Err(e) => self.say(format!("Could not go back to {}: {e}", path.display())),
        }
    }

    /// Whether level `number` is open with unsaved edits.
    pub fn is_modified(&self, number: u16) -> bool {
        self.open
            .get(&number)
            .is_some_and(|o| o.document.is_modified())
    }

    /// Puts the level's document in the workspace and asks for its
    /// picture.
    pub fn request_preview(&mut self, number: u16) {
        self.overview.changed(number);
        self.find.changed();
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
                    rom,
                    loaded,
                    sprites,
                    diagnostics,
                    failed_sprites,
                    entries,
                    left_out,
                    took,
                } = rendered;
                open.left_out = left_out;
                open.failed_sprites = failed_sprites;
                // The secondary entrances follow; until they do, the last
                // ones stay.
                let old: Vec<_> = open
                    .entries
                    .drain(..)
                    .filter(|e| matches!(e.kind, crate::preview::EntryKind::Secondary(_)))
                    .collect();
                open.entries = entries;
                open.entries.extend(old);
                open.built = Some(rom);
                open.picture = Some(Picture::new(ctx, &image));
                open.image = Some(std::sync::Arc::new(image));
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

    /// An open level and the palette, borrowed together.
    pub fn level_and_palette(&mut self, number: u16) -> (Option<&OpenLevel>, &mut PaletteState) {
        (self.open.get(&number), &mut self.palette)
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

    pub fn save_all_levels(&mut self) {
        self.save_all();
    }

    pub fn undo_step(&mut self, redo: bool) {
        self.undo(redo);
    }

    /// Draws the open level again, after a view setting the picture
    /// depends on changed.
    pub fn redraw(&mut self) {
        if let Some(number) = self.current {
            self.request_preview(number);
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
                        self.overview.forget();
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

    /// Imports a Lunar Magic MWL file into the project as the level it
    /// was saved from, and opens it.
    pub fn import_mwl(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("A level Lunar Magic saved as an MWL file")
            .add_filter("Lunar Magic level", &["mwl"])
            .pick_file()
        else {
            return;
        };
        let (Some(workspace), Clean::Loaded(clean)) = (&self.workspace, &self.clean) else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => return self.say(format!("{}: {e}", path.display())),
        };
        // An open level with unsaved edits is not written over.
        let number = kobo_core::mwl::MwlFile::parse(&bytes)
            .ok()
            .and_then(|f| f.decode(None).ok())
            .map(|m| m.info.level);
        if let Some(number) = number
            && self.is_modified(number)
        {
            return self.say(format!(
                "Level {number:03X} has unsaved edits; save or undo them before importing over it"
            ));
        }
        let root = workspace.project().root.clone();
        match kobo_core::import::import_mwl(&bytes, clean, &root, None) {
            Ok(report) => {
                let keep: Vec<(u16, kobo_core::source::level::Level)> = self
                    .open
                    .values()
                    .filter(|o| !report.levels.contains(&o.number))
                    .map(|o| (o.number, o.document.level().clone()))
                    .collect();
                for number in &report.levels {
                    self.open.remove(number);
                }
                if let Some(workspace) = &mut self.workspace
                    && let Err(e) = workspace.reload(&keep)
                {
                    self.project_error = Some(e.to_string());
                }
                let notes = report.notes.join("; ");
                if let Some(&number) = report.levels.first() {
                    self.open_level(number);
                    self.say(if notes.is_empty() {
                        format!("Imported level {number:03X}")
                    } else {
                        format!("Imported level {number:03X}: {notes}")
                    });
                }
            }
            Err(e) => self.say(format!("Could not import {}: {e}", path.display())),
        }
    }

    /// Saves the open level's picture as a PNG file.
    pub fn save_picture(&mut self) {
        let Some(open) = self.current() else { return };
        let Some(image) = open.image.clone() else {
            return;
        };
        let name = format!("level-{:03X}.png", open.number);
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save the level's picture")
            .set_file_name(&name)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        let mut rgb = kobo_core::image::RgbImage::new(image.size[0] as u32, image.size[1] as u32);
        for (i, pixel) in image.pixels.iter().enumerate() {
            rgb.pixels[i] = [pixel.r(), pixel.g(), pixel.b()];
        }
        match rgb.write_png(&path) {
            Ok(()) => self.say(format!("Saved {}", path.display())),
            Err(e) => self.say(format!("{}: {e}", path.display())),
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        crate::commands::keys(self, ctx);
        if self.commands.open {
            return;
        }
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let shift_command = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        let (save, undo, redo, redo_y, build, find) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&command(Key::S)),
                i.consume_shortcut(&command(Key::Z)),
                i.consume_shortcut(&shift_command(Key::Z)),
                i.consume_shortcut(&command(Key::Y)),
                i.consume_shortcut(&command(Key::B)),
                i.consume_shortcut(&shift_command(Key::F)),
            )
        });
        if find {
            self.left = LeftTab::Find;
        }
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
        let (back, forward) = ctx.input_mut(|i| {
            // Taken before the canvas, which moves the selection by arrows.
            let mut alt = |key| !typing && i.consume_key(Modifiers::ALT, key);
            let (left, right) = (alt(Key::ArrowLeft), alt(Key::ArrowRight));
            (
                left || i.pointer.button_pressed(PointerButton::Extra1),
                right || i.pointer.button_pressed(PointerButton::Extra2),
            )
        });
        if back || forward {
            self.go_back(forward);
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
                self.edit_menu(ui);
                self.view_menu(ui);
            }
            if let Some(workspace) = &self.workspace {
                let root = &workspace.project().root;
                let name = root.file_name().map_or_else(
                    || root.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                ui.label(RichText::new(name).strong());
            }
            if self.workspace.is_some() {
                for (forward, arrow, tip) in [
                    (false, "←", "Back to the level shown before (Alt+Left)"),
                    (true, "→", "Forward again (Alt+Right)"),
                ] {
                    let button = ui
                        .add_enabled(self.can_go_back(forward), egui::Button::new(arrow).small())
                        .on_hover_text(tip);
                    if button.clicked() {
                        self.go_back(forward);
                    }
                }
            }
            if let Some(open) = self.current() {
                ui.label(RichText::new("/").color(theme::MUTED));
                ui.label(RichText::new(format!("{:03X}", open.number)).strong());
                if let Some(name) = self.level_name(open.number) {
                    ui.label(RichText::new(name).color(theme::MUTED));
                }
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
                let play = ui
                    .add_enabled(
                        self.current().is_some() && !self.play.busy(),
                        egui::Button::new("▶ Play"),
                    )
                    .on_hover_text(
                        "Play the level from its start (F5: from where the mouse is; the canvas's menu: from there, with a power-up)",
                    );
                if play.clicked() {
                    crate::play::from_start(self, self.play.powerup);
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
                if ui
                    .button("Commands")
                    .on_hover_text("Find any command, level, or thing to add (Ctrl+K)")
                    .clicked()
                {
                    self.commands.open = true;
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
            let before = (self.view.sprites, self.view.player, self.view.hidden_layers);
            ui.label(RichText::new("Layers").color(theme::MUTED));
            for (bit, name, about) in [
                (1u8, "1", "Layer 1: the level's objects"),
                (2, "2", "Layer 2: the background, or layer 2's objects"),
                (4, "3", "Layer 3: water, tides, a status-bar backdrop"),
            ] {
                let mut shown = self.view.hidden_layers & bit == 0;
                if ui
                    .toggle_value(&mut shown, name)
                    .on_hover_text(about)
                    .changed()
                {
                    self.view.hidden_layers ^= bit;
                }
            }
            ui.separator();
            ui.label(RichText::new("Sprites").color(theme::MUTED));
            ui.selectable_value(&mut self.view.sprites, SpriteView::Drawn, "Drawn");
            ui.selectable_value(&mut self.view.sprites, SpriteView::Markers, "IDs");
            ui.selectable_value(&mut self.view.sprites, SpriteView::Hidden, "Off");
            ui.toggle_value(&mut self.view.player, "Player");
            if before != (self.view.sprites, self.view.player, self.view.hidden_layers)
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
        if ui
            .button("All levels as pictures")
            .on_hover_text("Every level of the project at a glance")
            .clicked()
        {
            self.overview.open = true;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.level_filter)
                .hint_text("Find a level: 105, castle…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let Some(workspace) = &self.workspace else {
            return;
        };
        ui.checkbox(&mut self.all_levels, "The game's own levels too")
            .on_hover_text("Levels the project does not list build as the game has them. Choose one to add it.");
        let filter = self.level_filter.trim().to_lowercase();
        let listed: Vec<u16> = if self.all_levels {
            (0..0x200).collect()
        } else {
            workspace.levels().collect()
        };
        // What a level is found by, and shown with: its name or else its
        // tileset, and its screens and whether it is vertical.
        let about = |number: u16| {
            let name = self.level_name(number).map(str::to_string);
            match workspace.level(number) {
                Some(level) => {
                    let tileset = kobo_core::names::object_tileset(level.header.object_tileset)
                        .unwrap_or("?");
                    let screens = level.header.screens;
                    let size = match (level.header.level_mode.layer1_vertical(), screens) {
                        (true, _) => format!("vertical, {screens}"),
                        (false, 1) => "1 screen".to_string(),
                        (false, n) => format!("{n} screens"),
                    };
                    (name, tileset.to_string(), size)
                }
                None => (name, "the game's own".to_string(), String::new()),
            }
        };
        let levels: Vec<(u16, ListedLevel)> = listed
            .into_iter()
            .map(|n| (n, about(n)))
            .filter(|(n, (name, tileset, _))| {
                filter.is_empty()
                    || format!("{n:03x}").contains(&filter)
                    || tileset.to_lowercase().contains(&filter)
                    || name
                        .as_ref()
                        .is_some_and(|name| name.to_lowercase().contains(&filter))
            })
            .collect();
        let mut clicked = None;
        let mut menu = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                    for (number, (name, tileset, size)) in levels {
                        let modified = self
                            .open
                            .get(&number)
                            .is_some_and(|o| o.document.is_modified());
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
                        let (text, color) = match &name {
                            Some(name) => (name.as_str(), theme::TEXT),
                            None => (tileset.as_str(), theme::MUTED),
                        };
                        job.append(
                            &format!("  {text}"),
                            0.0,
                            egui::TextFormat::simple(egui::FontId::proportional(12.5), color),
                        );
                        let selected = self.current == Some(number);
                        let row = egui::Button::selectable(selected, job)
                            .right_text(RichText::new(size).size(11.5).color(theme::MUTED));
                        let response = ui.add(row);
                        let response = if name.is_some() {
                            response.on_hover_text(tileset.as_str())
                        } else {
                            response
                        };
                        if response.clicked() {
                            clicked = Some(number);
                        }
                        let listed = workspace.level_path(number).is_some();
                        response.context_menu(|ui| {
                            if ui
                                .button(if listed {
                                    "Open"
                                } else {
                                    "Add to the project…"
                                })
                                .clicked()
                            {
                                clicked = Some(number);
                                ui.close();
                            }
                            if listed {
                                ui.menu_button("Play from its start", |ui| {
                                    for (i, name) in crate::play::POWERUPS.iter().enumerate() {
                                        if ui.button(*name).clicked() {
                                            menu = Some((number, ListAction::Play(i as u8)));
                                            ui.close();
                                        }
                                    }
                                });
                                ui.separator();
                                if ui.button("Take out of the project…").clicked() {
                                    menu = Some((number, ListAction::Remove));
                                    ui.close();
                                }
                            }
                        });
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
        match menu {
            Some((number, ListAction::Play(powerup))) => {
                crate::play::from_level_start(self, number, powerup);
            }
            Some((number, ListAction::Remove)) => self.removing = Some(number),
            None => {}
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, hover: Option<canvas::Hover>) {
        ui.horizontal_centered(|ui| {
            if let Some(hover) = hover {
                ui.label(format!("({}, {})", hover.x, hover.y));
                ui.label(RichText::new(format!("screen {:02X}", hover.screen)).color(theme::MUTED));
                if let Some(tile) = hover.tile {
                    ui.label(RichText::new(format!("Map16 {tile:03X}")).color(theme::MUTED));
                    let acts = self
                        .current()
                        .and_then(|o| o.built.as_deref())
                        .map(|rom| kobo_core::map16::pages::acts_like_in(rom, tile));
                    if let Some(acts) = acts.filter(|&a| a != tile) {
                        ui.label(
                            RichText::new(format!("acts like {acts:03X}")).color(theme::MUTED),
                        );
                    }
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
        let mut go_to = None;
        if let Some((other, why)) = open.left_out.first() {
            let more = match open.left_out.len() {
                1 => String::new(),
                n => format!(" (and {} more)", n - 1),
            };
            ui.add(egui::Label::new(
                RichText::new(format!(
                    "Level {other:03X} does not build{more}, so this picture has the game's own: {why}"
                ))
                .color(theme::WARNING),
            ).wrap());
            if ui.small_button(format!("Open level {other:03X}")).clicked() {
                go_to = Some(*other);
            }
        }
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
        if let Some(other) = go_to {
            self.open_level(other);
        }
    }

    /// The Edit menu: undo, the clipboard, and selecting.
    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        use crate::clipboard::Action;
        ui.menu_button("Edit", |ui| {
            let (undo, redo) = self.current().map_or((None, None), |o| {
                (
                    o.document.undo_label().map(str::to_owned),
                    o.document.redo_label().map(str::to_owned),
                )
            });
            let undo_text = undo
                .as_ref()
                .map_or("Undo".to_string(), |l| format!("Undo {l}"));
            if ui
                .add_enabled(
                    undo.is_some(),
                    egui::Button::new(undo_text).shortcut_text("Ctrl+Z"),
                )
                .clicked()
            {
                self.undo(false);
                ui.close();
            }
            let redo_text = redo
                .as_ref()
                .map_or("Redo".to_string(), |l| format!("Redo {l}"));
            if ui
                .add_enabled(
                    redo.is_some(),
                    egui::Button::new(redo_text).shortcut_text("Ctrl+Shift+Z"),
                )
                .clicked()
            {
                self.undo(true);
                ui.close();
            }
            let history: Vec<String> = self
                .current()
                .map(|o| {
                    o.document
                        .undo_labels()
                        .take(20)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            ui.add_enabled_ui(!history.is_empty(), |ui| {
                ui.menu_button("Undo back to…", |ui| {
                    for (steps, label) in history.iter().enumerate() {
                        if ui.button(label).clicked() {
                            for _ in 0..=steps {
                                self.undo(false);
                            }
                            ui.close();
                        }
                    }
                });
            });
            let modified = self.current().is_some_and(|o| o.document.is_modified());
            if ui
                .add_enabled(modified, egui::Button::new("Back to the saved file"))
                .on_hover_text("The level as its file has it, as one step that undo takes back")
                .clicked()
            {
                self.back_to_saved();
                ui.close();
            }
            ui.separator();
            let selected = self.current().is_some_and(|o| !o.selection.is_empty());
            let ctx = ui.ctx().clone();
            for (text, keys, action, enabled) in [
                ("Cut", "Ctrl+X", Action::Cut, selected),
                ("Copy", "Ctrl+C", Action::Copy, selected),
                ("Paste", "Ctrl+V", Action::Paste, !self.clipboard.is_empty()),
                ("Duplicate", "Ctrl+D", Action::Duplicate, selected),
            ] {
                if ui
                    .add_enabled(enabled, egui::Button::new(text).shortcut_text(keys))
                    .clicked()
                {
                    crate::clipboard::run(self, &ctx, action, None, None);
                    ui.close();
                }
            }
            if ui
                .add_enabled(selected, egui::Button::new("Delete").shortcut_text("Del"))
                .clicked()
            {
                let selection = self
                    .current()
                    .map(|o| o.selection.clone())
                    .unwrap_or_default();
                if self.apply("Delete", canvas::delete_edits(&selection))
                    && let Some(open) = self.current_mut()
                {
                    open.selection.clear();
                }
                ui.close();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Select everything").shortcut_text("Ctrl+A"))
                .clicked()
                && let Some(open) = self.current_mut()
            {
                let level = open.document.level();
                let mut items: Vec<Item> = (0..level.layer1.len())
                    .map(|i| Item::object(edit::ObjectLayer::One, i))
                    .collect();
                items.extend((0..level.sprites.list.len()).map(Item::Sprite));
                open.selection = items;
                ui.close();
            }
            if ui
                .add(egui::Button::new("Select nothing").shortcut_text("Esc"))
                .clicked()
            {
                if let Some(open) = self.current_mut() {
                    open.selection.clear();
                }
                ui.close();
            }
        });
    }

    /// The View menu: what the canvas shows, and the panels.
    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View", |ui| {
            let before = (self.view.sprites, self.view.player, self.view.hidden_layers);
            for (bit, name) in [(1u8, "Layer 1"), (2, "Layer 2"), (4, "Layer 3")] {
                let mut shown = self.view.hidden_layers & bit == 0;
                if ui.checkbox(&mut shown, name).changed() {
                    self.view.hidden_layers ^= bit;
                }
            }
            ui.separator();
            ui.checkbox(&mut self.view.screens, "Screen boundaries");
            ui.checkbox(&mut self.view.grid, "Grid (G)");
            ui.checkbox(&mut self.view.entrances, "Entrance markers");
            ui.checkbox(&mut self.view.player, "The player at the start");
            ui.separator();
            ui.label(RichText::new("Sprites").small().color(theme::MUTED));
            ui.radio_value(
                &mut self.view.sprites,
                SpriteView::Drawn,
                "Drawn as the game draws them",
            );
            ui.radio_value(
                &mut self.view.sprites,
                SpriteView::Markers,
                "As their numbers",
            );
            ui.radio_value(&mut self.view.sprites, SpriteView::Hidden, "Hidden");
            ui.separator();
            ui.checkbox(&mut self.view.source, "The level file beside the canvas");
            if ui.button("All levels as pictures").clicked() {
                self.overview.open = true;
                ui.close();
            }
            if ui
                .add(egui::Button::new("Keyboard shortcuts").shortcut_text("F1"))
                .clicked()
            {
                self.commands.shortcuts = true;
                ui.close();
            }
            if before != (self.view.sprites, self.view.player, self.view.hidden_layers) {
                self.redraw();
            }
        });
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

    /// Asks before taking a level out of the project.
    fn confirm_removing(&mut self, ctx: &egui::Context) {
        let Some(number) = self.removing else { return };
        let (mut remove, mut cancel) = (false, false);
        egui::Modal::new(egui::Id::new("confirm-remove")).show(ctx, |ui| {
            ui.heading(format!("Take level {number:03X} out of the project?"));
            ui.label("Its file is deleted, and the project builds it as the game has it.");
            if self.is_modified(number) {
                ui.colored_label(theme::WARNING, "Its unsaved edits go with it.");
            }
            ui.horizontal(|ui| {
                remove = ui.button("Take it out").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel {
            self.removing = None;
        }
        if remove {
            self.removing = None;
            let Some(workspace) = &mut self.workspace else {
                return;
            };
            match workspace.remove_level(number) {
                Ok(path) => {
                    self.open.remove(&number);
                    if self.current == Some(number) {
                        self.current = None;
                        if let Some(first) = self.first_level() {
                            self.open_level(first);
                        }
                    }
                    self.overview.changed(number);
                    self.say(format!(
                        "Took level {number:03X} out; {} is deleted",
                        path.display()
                    ));
                }
                Err(e) => self.say(format!("Could not take level {number:03X} out: {e}")),
            }
        }
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

    /// The window's title: the project, the level, and whether anything is
    /// unsaved; set when it changes.
    fn title(&mut self, ctx: &egui::Context) {
        let mut title = String::from("Kobo");
        if let Some(workspace) = &self.workspace {
            let root = &workspace.project().root;
            let name = root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            title = format!("{name} — Kobo");
            if let Some(open) = self.current() {
                title = match self.level_name(open.number) {
                    Some(level) => format!("{name} / {:03X} {level} — Kobo", open.number),
                    None => format!("{name} / {:03X} — Kobo", open.number),
                };
            }
            if self.modified().count() > 0 {
                title = format!("● {title}");
            }
        }
        if self.shown_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
    }

    /// With `--screenshot`, saves the window once the level is drawn.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.startup.screenshot.clone() else {
            return;
        };
        let ready = self.current().is_some_and(|o| {
            (o.picture.is_some() || o.render_error.is_some())
                && o.shown == o.requested
                && !self.palette.busy()
                && !self.build.busy()
                && !self.backgrounds.busy()
                && !(self.overview.open && self.overview.busy())
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

/// How many levels keep their pictures while others are shown.
const KEPT_PICTURES: usize = 4;
/// What the level list's menu asks for of a level.
enum ListAction {
    Play(u8),
    Remove,
}

/// What the level list shows of a level: its name on the overworld, its
/// tileset, and its size.
type ListedLevel = (Option<String>, String, String);

/// Levels the Back command remembers.
const HISTORY: usize = 100;

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
        self.title(&ctx);
        self.follow_files();
        crate::build::poll(self);
        crate::play::poll(self);
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
            .default_size(270.0)
            .frame(theme::side_frame())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.left, LeftTab::Levels, "Levels");
                    ui.selectable_value(&mut self.left, LeftTab::Add, "Add");
                    ui.selectable_value(&mut self.left, LeftTab::Outline, "Outline");
                    ui.selectable_value(&mut self.left, LeftTab::Changes, "Changes")
                        .on_hover_text("What differs from the last commit");
                    ui.selectable_value(&mut self.left, LeftTab::Find, "Find")
                        .on_hover_text("Objects and sprites in every level (Ctrl+Shift+F)");
                });
                ui.separator();
                self.view.changes = self.left == LeftTab::Changes;
                match self.left {
                    LeftTab::Levels => self.level_list(ui),
                    LeftTab::Add => palette::show(self, ui),
                    LeftTab::Outline => outline::show(self, ui),
                    LeftTab::Changes => crate::changes::show(self, ui),
                    LeftTab::Find => crate::find::show(self, ui),
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
                if self.overview.open {
                    crate::overview::show(self, ui);
                    return;
                }
                self.banners(ui);
                self.view_bar(ui);
                canvas::show(self, ui);
            });
        crate::build::window(self, &ctx);
        crate::backgrounds::window(self, &ctx);
        crate::project::window(self, &ctx);
        crate::commands::window(self, &ctx);
        self.confirm_adding(&ctx);
        self.confirm_removing(&ctx);
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
