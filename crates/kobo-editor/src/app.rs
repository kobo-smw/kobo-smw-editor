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
    /// Every object and sprite of the level (the outline).
    Objects,
    Add,
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
    /// The left panel's tab or a window to show, as `--tab` names them.
    pub tab: Option<String>,
    /// Build once the level is open.
    pub build: bool,
    /// Save a picture of the window here once the level's picture is in,
    /// then quit: for documentation and checks without a display.
    pub screenshot: Option<PathBuf>,
    /// Keep frames at least `FRAME` apart, for a window drawn without
    /// vsync (`main.rs`), which would otherwise draw as fast as it can
    /// while something animates.
    pub pace: bool,
}

/// What an edit was to, for undo and redo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LastEdit {
    Level,
    Map16,
    Graphics(edit::GraphicsFile),
    Palettes,
    Layer3(u16),
    GlobalAnimation,
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
    /// The level list.
    pub levels: crate::levels::LevelsState,
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
    /// The window of changes since the last commit is open.
    pub changes_open: bool,
    /// The command palette and the shortcuts window.
    pub commands: crate::commands::CommandState,
    /// Every level as a picture, in place of the canvas.
    pub overview: crate::overview::Overview,
    /// Small pictures of levels, for the overview and the level list.
    pub thumbnails: crate::thumbnails::Thumbnails,
    /// The outline's search, order, and what it lists.
    pub outline: crate::outline::OutlineState,
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
    /// The file dialog open, if one is.
    pub dialogs: crate::dialogs::Dialogs,
    /// The project window is open.
    pub project_open: bool,
    /// The project's Map16, open with it, or why it did not open.
    map16: Option<Result<edit::Map16Document, String>>,
    /// Counts the Map16's changes, for pictures drawn from it.
    map16_generation: u64,
    /// What the last edit was to, which undo and redo take first.
    last_edit: LastEdit,
    /// The graphics files open for drawing in, by file.
    graphics: BTreeMap<edit::GraphicsFile, edit::GraphicsDocument>,
    /// Counts the graphics files' changes, for pictures drawn from them.
    graphics_generation: u64,
    /// The graphics window.
    pub graphics_editor: crate::graphics::GraphicsEditor,
    /// The shared palettes, opened when first shown, or why they did not
    /// open.
    palettes: Option<Result<edit::PalettesDocument, String>>,
    /// The shared palettes window.
    pub palettes_editor: crate::palettes::PalettesEditor,
    /// The layer 3 tilemaps open for drawing on, by ExGFX file.
    tilemaps: BTreeMap<u16, edit::TilemapDocument>,
    /// Counts the tilemaps' changes, for pictures drawn from them.
    layer3_generation: u64,
    /// The layer 3 window.
    pub layer3_editor: crate::layer3::Layer3Editor,
    /// The global ExAnimation list, opened when first shown.
    global_animation: Option<Result<edit::GlobalAnimation, String>>,
    pub global_animation_window: crate::animation::GlobalWindow,
    /// The Map16 window.
    pub map16_editor: crate::map16::Map16Editor,
    startup: Startup,
    screenshot_frames: Option<u32>,
    /// The window's title as last set.
    shown_title: String,
    /// The levels shown last, newest first: only these keep their
    /// pictures, which are large.
    viewed: std::collections::VecDeque<u16>,
    /// When the last frame began, for `Startup::pace`.
    last_frame: Option<Instant>,
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
            levels: Default::default(),
            adding: None,
            history: Vec::new(),
            level_names: Default::default(),
            future: Vec::new(),
            look_at: None,
            removing: None,
            copy_to: 0,
            left: match (startup.palette, startup.tab.as_deref()) {
                (true, _) | (_, Some("add" | "sprites" | "map16")) => LeftTab::Add,
                (_, Some("objects" | "outline")) => LeftTab::Objects,
                _ => LeftTab::Levels,
            },
            changes_open: startup.tab.as_deref() == Some("changes"),
            outline: Default::default(),
            find: Default::default(),
            commands: Default::default(),
            overview: Default::default(),
            thumbnails: Default::default(),
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
            dialogs: Default::default(),
            project_open: false,
            map16: None,
            map16_generation: 0,
            last_edit: LastEdit::Level,
            graphics: BTreeMap::new(),
            graphics_generation: 0,
            graphics_editor: Default::default(),
            palettes: None,
            palettes_editor: Default::default(),
            tilemaps: BTreeMap::new(),
            layer3_generation: 0,
            layer3_editor: Default::default(),
            global_animation: None,
            global_animation_window: Default::default(),
            map16_editor: Default::default(),
            startup: startup.clone(),
            screenshot_frames: None,
            shown_title: String::new(),
            viewed: Default::default(),
            last_frame: None,
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
        app.graphics_editor.open = startup.tab.as_deref() == Some("graphics");
        app.palettes_editor.open = startup.tab.as_deref() == Some("palettes");
        app.layer3_editor.open = startup.tab.as_deref() == Some("layer3");
        if startup.tab.as_deref() == Some("map16-editor") {
            // At the cement block, which every tileset has.
            app.map16_editor.show_tile(0x130);
        }
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
        if !self.unsaved().is_empty() {
            self.switching = Some(to);
        } else {
            let ctx = self.ctx.clone();
            crate::start::switch(self, &ctx, to);
        }
    }

    /// Quits, once any unsaved edits are dealt with.
    pub(crate) fn quit(&mut self) {
        if !self.unsaved().is_empty() {
            self.confirm_close = true;
        } else {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Back to the start screen, dropping the open levels.
    pub(crate) fn close_project(&mut self) {
        self.thumbnails.forget();
        self.levels = Default::default();
        self.find.changed();
        self.overview.open = false;
        self.workspace = None;
        self.watcher = None;
        self.open.clear();
        self.current = None;
        self.placing = None;
        self.palette.forget_pictures();
        self.map16 = None;
        self.last_edit = LastEdit::Level;
        self.map16_editor.forget();
        self.graphics.clear();
        self.graphics_editor.forget();
        self.palettes = None;
        self.tilemaps.clear();
        self.layer3_editor.forget();
        self.global_animation = None;
    }

    pub(crate) fn open_project(&mut self, ctx: &egui::Context, dir: &Path) {
        let Clean::Loaded(clean) = &self.clean else {
            return;
        };
        let clean = clean.clone();
        match Workspace::open(dir, clean.clone()) {
            Ok(workspace) => {
                self.close_project();
                self.watcher = Watcher::new(dir, ctx.clone()).ok();
                self.map16 =
                    Some(edit::Map16Document::open(dir, &clean).map_err(|e| e.to_string()));
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
        self.thumbnails.changed(number);
        self.find.changed();
        self.levels.changed();
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
                self.last_edit = LastEdit::Level;
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

    /// The project's Map16, if its files opened.
    pub fn map16(&self) -> Option<&edit::Map16Document> {
        self.map16.as_ref()?.as_ref().ok()
    }

    /// Why the project's Map16 files did not open.
    pub fn map16_error(&self) -> Option<&str> {
        self.map16.as_ref()?.as_ref().err().map(String::as_str)
    }

    pub fn map16_generation(&self) -> u64 {
        self.map16_generation
    }

    /// Changes a Map16 tile, as one undo step or, while a value is being
    /// dragged, part of the last.
    pub fn apply_map16(&mut self, label: &str, change: edit::TileChange, amend: bool) {
        let Some(Ok(map16)) = &mut self.map16 else {
            return;
        };
        let result = if amend {
            map16.amend(&[change])
        } else {
            map16.apply(label, &[change])
        };
        match result {
            Ok(()) => {
                self.last_edit = LastEdit::Map16;
                self.map16_changed();
            }
            Err(e) => self.say(format!("{label}: {e}")),
        }
    }

    /// Changes a background Map16 tile, as one undo step or, while a value
    /// is being dragged, part of the last.
    pub fn apply_map16_background(
        &mut self,
        label: &str,
        tile: u16,
        gfx: kobo_core::map16::Map16Tile,
        amend: bool,
    ) {
        let Some(Ok(map16)) = &mut self.map16 else {
            return;
        };
        if amend {
            map16.amend_background(&[(tile, gfx)]);
        } else {
            map16.apply_background(label, &[(tile, gfx)]);
        }
        self.last_edit = LastEdit::Map16;
        self.map16_changed();
    }

    /// Changes a vertical pipe's tile in colour set 0, 2, or 3.
    pub fn apply_pipe_colour(
        &mut self,
        label: &str,
        set: u8,
        tile: u16,
        gfx: kobo_core::map16::Map16Tile,
    ) {
        let Some(Ok(map16)) = &mut self.map16 else {
            return;
        };
        map16.apply_pipe_colour(label, set, tile, gfx);
        self.last_edit = LastEdit::Map16;
        self.map16_changed();
    }

    /// After the Map16 changed: every level's picture may have, so the
    /// open one is drawn again and the rest when next shown.
    fn map16_changed(&mut self) {
        self.map16_generation += 1;
        if let (Some(workspace), Some(Ok(map16))) = (&mut self.workspace, &self.map16) {
            workspace.set_map16(map16);
        }
        self.thumbnails.forget();
        self.palette.forget_pictures();
        for open in self.open.values_mut() {
            open.requested = u64::MAX;
        }
        if let Some(number) = self.current {
            self.request_preview(number);
        }
    }

    /// The shared palettes, if they are open.
    pub fn palettes(&self) -> Option<&edit::PalettesDocument> {
        self.palettes.as_ref()?.as_ref().ok()
    }

    /// The shared palettes, opened if they are not yet.
    pub fn palettes_document(&mut self) -> Result<&edit::PalettesDocument, String> {
        if self.palettes.is_none() {
            let (Some(workspace), Clean::Loaded(clean)) = (&self.workspace, &self.clean) else {
                return Err("No project is open.".into());
            };
            let opened =
                edit::PalettesDocument::open(workspace.project(), clean).map_err(|e| e.to_string());
            self.palettes = Some(opened);
        }
        match self.palettes.as_ref().expect("opened above") {
            Ok(document) => Ok(document),
            Err(e) => Err(e.clone()),
        }
    }

    /// Sets a shared colour, as one undo step or, while a value is being
    /// dragged, part of the last.
    pub fn set_shared_colour(&mut self, n: u16, colour: kobo_core::palette::Color15, amend: bool) {
        let Some(Ok(palettes)) = &mut self.palettes else {
            return;
        };
        let result = if amend {
            palettes.amend(n, colour)
        } else {
            palettes.set("Change a shared colour", n, colour)
        };
        match result {
            Ok(()) => {
                self.last_edit = LastEdit::Palettes;
                self.palettes_changed();
            }
            Err(e) => self.say(e.to_string()),
        }
    }

    /// After the shared palettes changed: the levels are built with them
    /// and drawn again.
    fn palettes_changed(&mut self) {
        if let (Some(workspace), Some(Ok(palettes))) = (&mut self.workspace, &self.palettes) {
            workspace.set_palettes(palettes);
        }
        self.thumbnails.forget();
        self.palettes_pictures_changed();
    }

    /// Every picture drawn with the palettes is drawn again: the open
    /// level now, the rest when next shown.
    fn palettes_pictures_changed(&mut self) {
        self.palette.forget_pictures();
        for open in self.open.values_mut() {
            open.requested = u64::MAX;
        }
        if let Some(number) = self.current {
            self.request_preview(number);
        }
    }

    /// The global ExAnimation list, opened if it is not yet.
    pub fn global_animation(&mut self) -> Result<&edit::GlobalAnimation, String> {
        if self.global_animation.is_none() {
            let Some(workspace) = &self.workspace else {
                return Err("No project is open.".into());
            };
            self.global_animation =
                Some(edit::GlobalAnimation::open(workspace.project()).map_err(|e| e.to_string()));
        }
        match self.global_animation.as_ref().expect("opened above") {
            Ok(global) => Ok(global),
            Err(e) => Err(e.clone()),
        }
    }

    /// Sets the global ExAnimation list, as one undo step.
    pub fn set_global_animation(
        &mut self,
        label: &str,
        list: Option<kobo_core::exanimation::List>,
    ) {
        let Some(Ok(global)) = &mut self.global_animation else {
            return;
        };
        match global.set(label, list) {
            Ok(()) => {
                self.last_edit = LastEdit::GlobalAnimation;
                self.global_animation_changed();
            }
            Err(e) => self.say(e.to_string()),
        }
    }

    /// After the global list changed: the levels are built with it.
    fn global_animation_changed(&mut self) {
        if let (Some(workspace), Some(Ok(global))) = (&mut self.workspace, &self.global_animation) {
            workspace.set_global_animation(global);
        }
        self.thumbnails.forget();
        for open in self.open.values_mut() {
            open.requested = u64::MAX;
        }
        if let Some(number) = self.current {
            self.request_preview(number);
        }
    }

    pub fn layer3_generation(&self) -> u64 {
        self.layer3_generation
    }

    /// A layer 3 tilemap, opened for drawing on if it is not yet: ExGFX
    /// `file` as the project has it, or a new one `bytes` long.
    pub fn tilemap_document(
        &mut self,
        file: u16,
        bytes: usize,
    ) -> Result<&edit::TilemapDocument, String> {
        if !self.tilemaps.contains_key(&file) {
            let Some(workspace) = &self.workspace else {
                return Err("No project is open.".into());
            };
            let document = edit::TilemapDocument::open(workspace.project(), file, bytes)
                .map_err(|e| e.to_string())?;
            self.tilemaps.insert(file, document);
        }
        Ok(&self.tilemaps[&file])
    }

    /// A layer 3 tilemap already open.
    pub fn open_tilemap(&self, file: u16) -> Option<&edit::TilemapDocument> {
        self.tilemaps.get(&file)
    }

    /// Gives the open level a layer 3 tilemap: a new ExGFX file of blank
    /// tiles, which its graphics list loads, as one undo step of the
    /// level; saving writes the file.
    pub fn give_tilemap(&mut self) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let Some(level) = self.current().map(|o| o.document.level().clone()) else {
            return;
        };
        let free = (kobo_core::exgfx::EXGFX_FIRST..=kobo_core::exgfx::EXGFX_LAST).find(|n| {
            !workspace.project().manifest.exgfx.contains_key(n) && !self.tilemaps.contains_key(n)
        });
        let Some(file) = free else {
            self.say("The project has every ExGFX number.");
            return;
        };
        let list = edit::layer3::with_tilemap(&level, file);
        let tilemap = edit::layer3::Tilemap::of(&kobo_core::source::level::Level {
            graphics: Some(list),
            ..level
        })
        .expect("the list loads a tilemap");
        if let Err(e) = self.tilemap_document(file, tilemap.bytes()) {
            self.say(e);
            return;
        }
        if let (Some(workspace), Some(document)) = (&mut self.workspace, self.tilemaps.get(&file)) {
            workspace.set_tilemap(document);
        }
        self.apply(
            "Give layer 3 a tilemap",
            vec![Edit::SetGraphics(Some(list))],
        );
    }

    /// Draws on a layer 3 tilemap: `cells` set, as a step of its own or,
    /// through a stroke, part of the last.
    pub fn draw_tilemap(
        &mut self,
        file: u16,
        cells: &[(usize, kobo_core::map16::Tile8Ref)],
        stroke: bool,
    ) {
        let Some(tilemap) = self.tilemaps.get_mut(&file) else {
            return;
        };
        if stroke {
            tilemap.amend(cells);
        } else {
            tilemap.set("Draw on layer 3", cells);
        }
        self.last_edit = LastEdit::Layer3(file);
        self.layer3_generation += 1;
    }

    /// After a layer 3 tilemap changed: the level is built with it and
    /// drawn again.
    pub fn tilemap_changed(&mut self, file: u16) {
        self.layer3_generation += 1;
        if let (Some(workspace), Some(tilemap)) = (&mut self.workspace, self.tilemaps.get(&file)) {
            workspace.set_tilemap(tilemap);
        }
        self.thumbnails.forget();
        for open in self.open.values_mut() {
            open.requested = u64::MAX;
        }
        if let Some(number) = self.current {
            self.request_preview(number);
        }
    }

    pub fn graphics_generation(&self) -> u64 {
        self.graphics_generation
    }

    /// A graphics file, opened for drawing in if it is not yet.
    pub fn graphics_document(
        &mut self,
        file: edit::GraphicsFile,
    ) -> Result<&edit::GraphicsDocument, String> {
        if !self.graphics.contains_key(&file) {
            let (Some(workspace), Clean::Loaded(clean)) = (&self.workspace, &self.clean) else {
                return Err("No project is open.".into());
            };
            let document = edit::GraphicsDocument::open(workspace.project(), clean, file)
                .map_err(|e| e.to_string())?;
            self.graphics.insert(file, document);
        }
        Ok(&self.graphics[&file])
    }

    /// A graphics file already open.
    #[cfg(test)]
    pub fn open_graphics(&self, file: edit::GraphicsFile) -> Option<&edit::GraphicsDocument> {
        self.graphics.get(&file)
    }

    /// Draws in a graphics file: `pixels` in colour `value`, as a step of
    /// its own or, through a stroke, part of the last. The level is drawn
    /// again once the stroke ends ([`App::graphics_changed`]).
    pub fn paint(
        &mut self,
        file: edit::GraphicsFile,
        pixels: &[(u32, u32)],
        value: u8,
        stroke: bool,
    ) {
        let Some(graphics) = self.graphics.get_mut(&file) else {
            return;
        };
        let result = if stroke {
            graphics.amend_paint(pixels, value)
        } else {
            graphics.paint(format!("Draw in {file}"), pixels, value)
        };
        match result {
            Ok(()) => {
                self.last_edit = LastEdit::Graphics(file);
                self.graphics_generation += 1;
            }
            Err(e) => self.say(e.to_string()),
        }
    }

    /// Fills an area of one colour in a graphics file, within its tile.
    pub fn fill(&mut self, file: edit::GraphicsFile, x: u32, y: u32, value: u8) {
        let Some(graphics) = self.graphics.get_mut(&file) else {
            return;
        };
        match graphics.fill(format!("Fill in {file}"), x, y, value) {
            Ok(()) => {
                self.last_edit = LastEdit::Graphics(file);
                self.graphics_changed(file);
            }
            Err(e) => self.say(e.to_string()),
        }
    }

    /// Draws a graphics file from an indexed PNG, as one undo step.
    pub fn import_graphics(&mut self, file: edit::GraphicsFile, path: &Path) {
        let image = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                kobo_core::image::IndexedImage::from_png(&bytes).map_err(|e| e.to_string())
            });
        let result = match (image, self.graphics.get_mut(&file)) {
            (Ok(image), Some(graphics)) => graphics
                .replace(format!("Import {file}"), image)
                .map_err(|e| e.to_string()),
            (Err(e), _) => Err(e),
            (_, None) => return,
        };
        match result {
            Ok(()) => {
                self.last_edit = LastEdit::Graphics(file);
                self.graphics_changed(file);
            }
            Err(e) => self.say(format!("{}: {e}", path.display())),
        }
    }

    /// Saves a graphics file as an indexed PNG, previewed in `palette`.
    pub fn export_graphics(&mut self, file: edit::GraphicsFile, path: &Path) {
        let Some(graphics) = self.graphics.get(&file) else {
            return;
        };
        let palette = self.graphics_editor.preview.clone();
        let image = kobo_core::image::IndexedImage {
            palette: if palette.is_empty() {
                graphics.image().palette.clone()
            } else {
                palette
            },
            ..graphics.image().clone()
        };
        let result = image
            .to_png()
            .map_err(|e| e.to_string())
            .and_then(|png| std::fs::write(path, png).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.say(format!("Saved {file} as {}", path.display())),
            Err(e) => self.say(format!("{}: {e}", path.display())),
        }
    }

    /// After a graphics file changed: the levels are built with it and
    /// drawn again, the open one now and the rest when next shown.
    pub fn graphics_changed(&mut self, file: edit::GraphicsFile) {
        self.graphics_generation += 1;
        let (Some(workspace), Some(graphics)) = (&mut self.workspace, self.graphics.get(&file))
        else {
            return;
        };
        if let Err(e) = workspace.set_graphics(graphics) {
            self.say(e.to_string());
            return;
        }
        self.thumbnails.forget();
        self.palette.forget_pictures();
        for open in self.open.values_mut() {
            open.requested = u64::MAX;
        }
        if let Some(number) = self.current {
            self.request_preview(number);
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
        let done = match self.last_edit {
            LastEdit::Level => false,
            LastEdit::Map16 => self.undo_map16(redo),
            LastEdit::Graphics(file) => self.undo_graphics(file, redo),
            LastEdit::Palettes => self.undo_palettes(redo),
            LastEdit::Layer3(file) => self.undo_tilemap(file, redo),
            LastEdit::GlobalAnimation => self.undo_global_animation(redo),
        };
        if done {
            return;
        }
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

    /// Undoes or redoes the Map16's last step, if it has one.
    fn undo_map16(&mut self, redo: bool) -> bool {
        let Some(Ok(map16)) = &mut self.map16 else {
            return false;
        };
        let label = if redo {
            map16.redo_label()
        } else {
            map16.undo_label()
        }
        .map(str::to_owned);
        let done = if redo { map16.redo() } else { map16.undo() };
        if done {
            self.map16_changed();
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
        done
    }

    /// Undoes or redoes the global ExAnimation list's last step.
    fn undo_global_animation(&mut self, redo: bool) -> bool {
        let Some(Ok(global)) = &mut self.global_animation else {
            return false;
        };
        let label = if redo {
            global.redo_label()
        } else {
            global.undo_label()
        }
        .map(str::to_owned);
        let done = if redo { global.redo() } else { global.undo() };
        if done {
            self.global_animation_changed();
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
        done
    }

    /// Undoes or redoes a layer 3 tilemap's last step, if it has one.
    fn undo_tilemap(&mut self, file: u16, redo: bool) -> bool {
        let Some(tilemap) = self.tilemaps.get_mut(&file) else {
            return false;
        };
        let label = if redo {
            tilemap.redo_label()
        } else {
            tilemap.undo_label()
        }
        .map(str::to_owned);
        let done = if redo { tilemap.redo() } else { tilemap.undo() };
        if done {
            self.tilemap_changed(file);
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
        done
    }

    /// Undoes or redoes the shared palettes' last step, if they have one.
    fn undo_palettes(&mut self, redo: bool) -> bool {
        let Some(Ok(palettes)) = &mut self.palettes else {
            return false;
        };
        let label = if redo {
            palettes.redo_label()
        } else {
            palettes.undo_label()
        }
        .map(str::to_owned);
        let done = if redo {
            palettes.redo()
        } else {
            palettes.undo()
        };
        if done {
            self.palettes_changed();
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
        done
    }

    /// Undoes or redoes a graphics file's last step, if it has one.
    fn undo_graphics(&mut self, file: edit::GraphicsFile, redo: bool) -> bool {
        let Some(graphics) = self.graphics.get_mut(&file) else {
            return false;
        };
        let label = if redo {
            graphics.redo_label()
        } else {
            graphics.undo_label()
        }
        .map(str::to_owned);
        let done = if redo {
            graphics.redo()
        } else {
            graphics.undo()
        };
        if done {
            self.graphics_changed(file);
            let verb = if redo { "Redid" } else { "Undid" };
            self.say(format!("{verb}: {}", label.unwrap_or_default()));
        }
        done
    }

    /// The labels of the step undo and redo would take: of what was
    /// edited last, else the level.
    fn undo_labels(&self) -> (Option<String>, Option<String>) {
        let level = self.current().map_or((None, None), |o| {
            (
                o.document.undo_label().map(str::to_owned),
                o.document.redo_label().map(str::to_owned),
            )
        });
        let (undo, redo) = match self.last_edit {
            LastEdit::Level => (None, None),
            LastEdit::Map16 => self
                .map16()
                .map_or((None, None), |m| (m.undo_label(), m.redo_label())),
            LastEdit::Graphics(file) => self
                .graphics
                .get(&file)
                .map_or((None, None), |g| (g.undo_label(), g.redo_label())),
            LastEdit::Palettes => self
                .palettes()
                .map_or((None, None), |p| (p.undo_label(), p.redo_label())),
            LastEdit::Layer3(file) => self
                .tilemaps
                .get(&file)
                .map_or((None, None), |t| (t.undo_label(), t.redo_label())),
            LastEdit::GlobalAnimation => match &self.global_animation {
                Some(Ok(g)) => (g.undo_label(), g.redo_label()),
                _ => (None, None),
            },
        };
        (
            undo.map(str::to_owned).or(level.0),
            redo.map(str::to_owned).or(level.1),
        )
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
        if let Some(Ok(map16)) = &mut self.map16
            && map16.is_modified()
        {
            match map16.save() {
                Ok(_) => saved.push("the Map16".into()),
                Err(e) => failed.push(e.to_string()),
            }
        }
        if let Some(root) = self.workspace.as_ref().map(|w| w.project().root.clone()) {
            for graphics in self.graphics.values_mut().filter(|g| g.is_modified()) {
                match graphics.save(&root) {
                    Ok(_) => saved.push(graphics.file().to_string()),
                    Err(e) => failed.push(e.to_string()),
                }
            }
            if let Some(Ok(palettes)) = &mut self.palettes
                && palettes.is_modified()
            {
                match palettes.save(&root) {
                    Ok(_) => saved.push("the shared palettes".into()),
                    Err(e) => failed.push(e.to_string()),
                }
            }
            if let Some(Ok(global)) = &mut self.global_animation
                && global.is_modified()
            {
                match global.save(&root) {
                    Ok(_) => saved.push("the global ExAnimation".into()),
                    Err(e) => failed.push(e.to_string()),
                }
            }
            for tilemap in self.tilemaps.values_mut().filter(|t| t.is_modified()) {
                match tilemap.save(&root) {
                    Ok(_) => saved.push(format!("ExGFX{:X}", tilemap.file())),
                    Err(e) => failed.push(e.to_string()),
                }
            }
        }
        if !failed.is_empty() {
            self.say(failed.join("; "));
        } else if !saved.is_empty() {
            self.say(format!("Saved {}", saved.join(", ")));
        }
    }

    /// What has unsaved edits: the levels' files, and the Map16.
    fn unsaved(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .open
            .values()
            .filter(|o| o.document.is_modified())
            .map(OpenLevel::file_name)
            .collect();
        if self.map16().is_some_and(edit::Map16Document::is_modified) {
            names.push("the Map16".into());
        }
        names.extend(
            self.graphics
                .values()
                .filter(|g| g.is_modified())
                .map(|g| g.file().to_string()),
        );
        if self
            .palettes()
            .is_some_and(edit::PalettesDocument::is_modified)
        {
            names.push("the shared palettes".into());
        }
        if matches!(&self.global_animation, Some(Ok(g)) if g.is_modified()) {
            names.push("the global ExAnimation".into());
        }
        names.extend(
            self.tilemaps
                .values()
                .filter(|t| t.is_modified())
                .map(|t| format!("ExGFX{:X}", t.file())),
        );
        names
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
            // The Map16 follows its files unless it has unsaved edits,
            // which stay over them.
            let map16_saved = !self.map16().is_some_and(edit::Map16Document::is_modified);
            if map16_saved
                && let (Some(workspace), Clean::Loaded(clean)) = (&self.workspace, &self.clean)
            {
                let root = workspace.project().root.clone();
                let reopened = edit::Map16Document::open(&root, clean).map_err(|e| e.to_string());
                self.map16 = Some(reopened);
                self.map16_generation += 1;
            }
            // So are the graphics files: those without unsaved edits are
            // opened again from the files when next drawn in.
            self.graphics.retain(|_, g| g.is_modified());
            self.graphics_generation += 1;
            if !self
                .palettes()
                .is_some_and(edit::PalettesDocument::is_modified)
            {
                self.palettes = None;
            }
            self.tilemaps.retain(|_, t| t.is_modified());
            self.layer3_generation += 1;
            if !matches!(&self.global_animation, Some(Ok(g)) if g.is_modified()) {
                self.global_animation = None;
            }
            if let Some(workspace) = &mut self.workspace {
                let result = workspace.reload(&keep);
                if let Some(Ok(map16)) = &self.map16
                    && map16.is_modified()
                {
                    workspace.set_map16(map16);
                }
                for graphics in self.graphics.values() {
                    let _ = workspace.set_graphics(graphics);
                }
                if let Some(Ok(palettes)) = &self.palettes {
                    workspace.set_palettes(palettes);
                }
                for tilemap in self.tilemaps.values() {
                    workspace.set_tilemap(tilemap);
                }
                if let Some(Ok(global)) = &self.global_animation {
                    workspace.set_global_animation(global);
                }
                match result {
                    Ok(()) => {
                        self.project_error = None;
                        self.palette.forget_pictures();
                        self.thumbnails.forget();
                        self.levels.changed();
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
    pub fn import_mwl(&mut self, path: &Path) {
        let (Some(workspace), Clean::Loaded(clean)) = (&self.workspace, &self.clean) else {
            return;
        };
        let bytes = match std::fs::read(path) {
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
                self.levels.changed();
                for number in &report.levels {
                    self.thumbnails.changed(*number);
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
    pub fn save_picture(&mut self, path: &Path) {
        let Some(open) = self.current() else { return };
        let Some(image) = open.image.clone() else {
            return;
        };
        let mut rgb = kobo_core::image::RgbImage::new(image.size[0] as u32, image.size[1] as u32);
        for (i, pixel) in image.pixels.iter().enumerate() {
            rgb.pixels[i] = [pixel.r(), pixel.g(), pixel.b()];
        }
        match rgb.write_png(path) {
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
            // egui ignores an extra Shift, so Ctrl+Shift+Z is taken before
            // Ctrl+Z would take it.
            let redo = i.consume_shortcut(&shift_command(Key::Z));
            (
                i.consume_shortcut(&command(Key::S)),
                i.consume_shortcut(&command(Key::Z)),
                redo,
                i.consume_shortcut(&command(Key::Y)),
                i.consume_shortcut(&command(Key::B)),
                i.consume_shortcut(&shift_command(Key::F)),
            )
        });
        if find {
            self.left = LeftTab::Levels;
            self.levels.focus_search = true;
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
                crate::changes::menu(self, ui);
                ui.separator();
                self.undo_buttons(ui);
                ui.separator();
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
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Quit").on_hover_text("Ctrl+Q").clicked() {
                        self.quit();
                    }
                });
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
                let settings = ui
                    .button("▾")
                    .on_hover_text("How the game starts: power-up, switch palaces, ON/OFF");
                crate::play::settings_menu(self, &settings);
                let play = ui
                    .add_enabled(
                        self.current().is_some() && !self.play.busy(),
                        egui::Button::new("▶ Play"),
                    )
                    .on_hover_text(
                        "Play the level from its start (F5: from where the mouse is; the canvas's menu: from there, with a power-up)",
                    );
                if play.clicked() {
                    crate::play::from_start(self, self.play.settings.powerup);
                }
                let modified = self.unsaved().len();
                let save = ui
                    .add_enabled(modified > 0, egui::Button::new(icon("💾")))
                    .on_hover_text("Save every changed level (Ctrl+S)")
                    .on_disabled_hover_text("Nothing to save");
                if save.clicked() {
                    self.save_all();
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

    /// Undo and redo, as arrows, named by what they would undo and redo.
    fn undo_buttons(&mut self, ui: &mut egui::Ui) {
        let (undo_label, redo_label) = self.undo_labels();
        let undo = ui
            .add_enabled(undo_label.is_some(), egui::Button::new(icon("⟲")))
            .on_hover_text(format!(
                "Undo {} (Ctrl+Z)",
                undo_label.as_deref().unwrap_or_default()
            ))
            .on_disabled_hover_text("Nothing to undo");
        if undo.clicked() {
            self.undo(false);
        }
        let redo = ui
            .add_enabled(redo_label.is_some(), egui::Button::new(icon("⟳")))
            .on_hover_text(format!(
                "Redo {} (Ctrl+Shift+Z)",
                redo_label.as_deref().unwrap_or_default()
            ))
            .on_disabled_hover_text("Nothing to redo");
        if redo.clicked() {
            self.undo(true);
        }
    }

    fn view_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| self.view_bar_buttons(ui));
    }

    fn view_bar_buttons(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.view.screens, icon("▥"))
                .on_hover_text("Screen boundaries");
            ui.toggle_value(&mut self.view.grid, icon("▦"))
                .on_hover_text("The tile grid (G)");
            ui.toggle_value(&mut self.view.entrances, icon("⚑"))
                .on_hover_text(
                    "Entrances: where the player enters, at the start, the midway, and each secondary entrance",
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
            for (view, glyph, about) in [
                (SpriteView::Drawn, "🐢", "Sprites drawn as the game draws them"),
                (SpriteView::Markers, "🔢", "Sprites as their numbers"),
                (SpriteView::Hidden, "🚫", "Sprites hidden"),
            ] {
                ui.selectable_value(&mut self.view.sprites, view, icon(glyph))
                    .on_hover_text(about);
            }
            ui.toggle_value(&mut self.view.player, icon("🏃"))
                .on_hover_text("The player where the level starts");
            if before != (self.view.sprites, self.view.player, self.view.hidden_layers)
                && let Some(number) = self.current
            {
                self.request_preview(number);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(open) = self.current_mut()
                    && let Some(camera) = &mut open.camera
                {
                    if ui
                        .button(icon("⛶"))
                        .on_hover_text("Fit the level's height to the view")
                        .clicked()
                    {
                        camera.fit_height = true;
                    }
                    if ui.button(icon("+")).on_hover_text("Zoom in").clicked() {
                        camera.zoom_by(2.0);
                    }
                    ui.label(RichText::new(format!("{:.0}%", camera.zoom * 100.0)).monospace());
                    if ui.button(icon("−")).on_hover_text("Zoom out").clicked() {
                        camera.zoom_by(0.5);
                    }
                }
            });
        });
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
            if ui
                .button("Map16")
                .on_hover_text("The project's Map16 tiles, as the open level draws them")
                .clicked()
            {
                self.map16_editor.open = true;
                ui.close();
            }
            if ui
                .button("Graphics")
                .on_hover_text("The level's graphics files, to draw in")
                .clicked()
            {
                self.graphics_editor.open = true;
                ui.close();
            }
            if ui
                .button("Layer 3")
                .on_hover_text("The level's layer 3 tilemap, to draw on")
                .clicked()
            {
                self.layer3_editor.open = true;
                ui.close();
            }
            if ui
                .button("Shared palettes")
                .on_hover_text("The game's colour tables, which levels without a palette of their own draw from")
                .clicked()
            {
                self.palettes_editor.open = true;
                ui.close();
            }
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
        let names = self.unsaved();
        let (mut go, mut cancel) = (false, false);
        egui::Modal::new(egui::Id::new("confirm-switch")).show(ctx, |ui| {
            ui.heading("Save your changes?");
            ui.label(format!("Unsaved: {}", names.join(", ")));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save_all();
                    go = self.unsaved().is_empty();
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
                self.levels.changed();
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
                    self.thumbnails.changed(number);
                    self.levels.changed();
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
            && !self.unsaved().is_empty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if !self.confirm_close {
            return;
        }
        let names = self.unsaved();
        let mut close = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("confirm-close")).show(ctx, |ui| {
            ui.heading("Save your changes?");
            ui.label(format!("Unsaved: {}", names.join(", ")));
            ui.horizontal(|ui| {
                if ui.button("Save and close").clicked() {
                    self.save_all();
                    close = self.unsaved().is_empty();
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
            if !self.unsaved().is_empty() {
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
                && !self.thumbnails.busy()
                && !self.thumbnails.waiting()
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
/// output, what an emulator playing it writes beside it (its battery
/// save, save states, cheats), git's own files, or an editor's scratch
/// files.
pub(crate) fn matters(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let emulator = matches!(
        ext.as_str(),
        "srm" | "sav" | "rtc" | "cht" | "bst" | "mss" | "sts" | "frz" | "zst" | "oops" | "state"
    ) || ext.starts_with("state")
        || (ext.len() == 3 && ext.starts_with('0') && ext.bytes().all(|b| b.is_ascii_digit()));
    !(path.components().any(|c| c.as_os_str() == ".git")
        || name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || emulator
        || matches!(ext.as_str(), "sfc" | "smc" | "bps"))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

/// The shortest time between frames when the editor paces them: 60 a
/// second.
const FRAME: Duration = Duration::from_micros(16_667);

/// How many levels keep their pictures while others are shown.
const KEPT_PICTURES: usize = 4;
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
        // Once a frame, not again for another pass of the same one.
        if self.startup.pace && ctx.current_pass_index() == 0 {
            if let Some(wait) = self
                .last_frame
                .and_then(|at| FRAME.checked_sub(at.elapsed()))
            {
                std::thread::sleep(wait);
            }
            self.last_frame = Some(Instant::now());
        }
        self.take_preview(&ctx);
        crate::start::poll(self, &ctx);
        crate::dialogs::poll(self);
        self.thumbnails.poll(&ctx, self.workspace.as_ref());
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
            crate::start::confirm_locked(self, &ctx);
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
                    ui.selectable_value(&mut self.left, LeftTab::Levels, "Levels")
                        .on_hover_text(
                            "The project's levels, and finding what they hold (Ctrl+Shift+F)",
                        );
                    ui.selectable_value(&mut self.left, LeftTab::Objects, "Objects")
                        .on_hover_text("Every object and sprite of the level");
                    ui.selectable_value(&mut self.left, LeftTab::Add, "Add")
                        .on_hover_text("Objects, sprites, and Map16 tiles to place");
                });
                ui.separator();
                match self.left {
                    LeftTab::Levels => crate::levels::show(self, ui),
                    LeftTab::Objects => outline::show(self, ui),
                    LeftTab::Add => palette::show(self, ui),
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
        crate::map16::window(self, &ctx);
        crate::graphics::window(self, &ctx);
        crate::palettes::window(self, &ctx);
        crate::layer3::window(self, &ctx);
        crate::animation::window(self, &ctx);
        crate::project::window(self, &ctx);
        crate::commands::window(self, &ctx);
        crate::changes::window(self, &ctx);
        crate::start::report_window(self, &ctx);
        self.confirm_adding(&ctx);
        self.confirm_removing(&ctx);
        self.confirm_switch(&ctx);
        self.close_requests(&ctx);
        self.screenshot(&ctx);
    }
}

/// A button's symbol, a little larger than its text would be.
fn icon(glyph: &str) -> RichText {
    RichText::new(glyph).size(15.0)
}

/// Colours for the panels.
pub fn color_for(item: Item) -> Color32 {
    match item {
        Item::Object(_) => theme::SELECTION,
        Item::Sprite(_) => theme::SPRITE,
    }
}
