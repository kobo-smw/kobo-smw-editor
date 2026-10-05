//! Changes since the last commit: the level file as git's HEAD has it,
//! what differs from it (`edit::diff`), marked on the canvas and listed
//! with a way to take each back.

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align, Layout, RichText};
use kobo_core::edit::diff::{self, EntryChange, LevelDiff, Part};
use kobo_core::edit::{self, Edit, ObjectLayer, Workspace};
use kobo_core::expand::LoadedLevel;
use kobo_core::level::objects::Object;
use kobo_core::names;
use kobo_core::operation::Operation;
use kobo_core::render::{RenderOptions, Sprites};
use kobo_core::source::level::{Layer2, Level};

use crate::app::App;
use crate::selection::{Item, objects};
use crate::theme;

/// The level as the last commit has it.
pub struct Head {
    pub level: Level,
    /// Its load, for where its objects were; on its way from a worker.
    pub loaded: Option<LoadedLevel>,
    pending: Option<Receiver<Option<LoadedLevel>>>,
    /// The diff, and the document text it was made for.
    diff: LevelDiff,
    diffed: String,
}

/// Why there is nothing to compare with.
pub enum NoHead {
    NotInGit,
    NotCommitted,
    Unreadable(String),
}

/// The file's text at HEAD, through git.
fn head_text(path: &Path) -> Result<String, NoHead> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(format!("HEAD:./{name}"))
        .output()
        .map_err(|_| NoHead::NotInGit)?;
    if output.status.success() {
        return String::from_utf8(output.stdout).map_err(|e| NoHead::Unreadable(e.to_string()));
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(if message.contains("not a git repository") {
        NoHead::NotInGit
    } else {
        NoHead::NotCommitted
    })
}

impl Head {
    /// The level's file as HEAD has it, and its load started on a worker
    /// thread, in a copy of the workspace.
    pub fn read(path: &Path, workspace: &Workspace, number: u16) -> Result<Self, NoHead> {
        let text = head_text(path)?;
        let (level, _) = Level::from_toml(&text).map_err(|e| NoHead::Unreadable(e.to_string()))?;
        let (send, pending) = mpsc::channel();
        let mut copy = workspace.clone();
        copy.set_level(number, &level);
        let options = RenderOptions {
            sprites: Sprites::Hidden,
            player: false,
        };
        std::thread::spawn(move || {
            let loaded = copy
                .preview(number, options, &Operation::default())
                .ok()
                .map(|p| p.render.level);
            let _ = send.send(loaded);
        });
        Ok(Self {
            level,
            loaded: None,
            pending: Some(pending),
            diff: LevelDiff::default(),
            diffed: String::new(),
        })
    }

    /// Takes the load once it is in; diffs again if the document changed.
    pub fn update(&mut self, document: &kobo_core::edit::LevelDocument) {
        if let Some(pending) = &self.pending
            && let Ok(loaded) = pending.try_recv()
        {
            self.loaded = loaded;
            self.pending = None;
        }
        if self.diffed != document.text() {
            self.diff = diff::diff(&self.level, document.level());
            self.diffed = document.text().to_string();
        }
    }

    pub fn diff(&self) -> &LevelDiff {
        &self.diff
    }

    /// Where an object of HEAD's stood, in level pixels: what it drew
    /// there, or its tile.
    pub fn bounds(&self, layer: ObjectLayer, index: usize) -> Option<egui::Rect> {
        let tile = 16.0;
        if let Some(loaded) = &self.loaded {
            let object = kobo_core::expand::ObjectRef { layer, index };
            if let Some([x, y, w, h]) = loaded.objects.bounds(&loaded.tiles, object) {
                let offset = match layer {
                    ObjectLayer::One => egui::Vec2::ZERO,
                    ObjectLayer::Two => {
                        let [dx, dy] = loaded.scene.layer2_offset();
                        egui::vec2(dx as f32, dy as f32)
                    }
                };
                return Some(
                    egui::Rect::from_min_size(
                        egui::pos2(x as f32 * tile, y as f32 * tile),
                        egui::vec2(w as f32 * tile, h as f32 * tile),
                    )
                    .translate(offset),
                );
            }
        }
        let object = objects(&self.level, layer)?.get(index)?;
        let (x, y) = edit::object_position(object)?;
        Some(crate::selection::tile_rect(i32::from(x), i32::from(y)))
    }

    /// Where a sprite of HEAD's stood.
    pub fn sprite_bounds(&self, index: usize) -> Option<egui::Rect> {
        let sprite = self.level.sprites.list.get(index)?;
        Some(crate::selection::tile_rect(
            i32::from(sprite.x),
            i32::from(sprite.y),
        ))
    }
}

/// What a changed object or sprite became, in words.
fn what_changed(old: &Object, new: &Object) -> String {
    let mut parts = Vec::new();
    if let (Some(a), Some(b)) = (edit::object_position(old), edit::object_position(new))
        && a != b
    {
        parts.push(format!("({}, {}) → ({}, {})", a.0, a.1, b.0, b.1));
    }
    if let (
        Object::Standard {
            number: n,
            settings: a,
            ..
        },
        Object::Standard { settings: b, .. },
    ) = (old, new)
    {
        for (fa, fb) in edit::setting_fields(*n, *a)
            .into_iter()
            .zip(edit::setting_fields(*n, *b))
        {
            if fa.value != fb.value {
                parts.push(format!("{} {} → {}", fa.name, fa.value, fb.value));
            }
        }
    }
    if parts.is_empty() {
        "moved in the list".to_string()
    } else {
        parts.join(", ")
    }
}

/// One line of the list: its mark, its words, and what selects it.
struct Line {
    mark: &'static str,
    color: egui::Color32,
    text: String,
    select: Option<Item>,
    /// Where to look for it on the canvas, if it is gone.
    look: Option<egui::Rect>,
    revert: Revert,
}

enum Revert {
    Entry(Part, EntryChange),
    Header,
    Entrance,
    Settings,
    Other,
}

fn lines(head: &Head, level: &Level) -> Vec<Line> {
    let tileset = level.header.object_tileset;
    let mut lines = Vec::new();
    let d = head.diff();
    for layer in [ObjectLayer::One, ObjectLayer::Two] {
        let none = Vec::new();
        let old = objects(&head.level, layer).unwrap_or(&none);
        let new = objects(level, layer).unwrap_or(&none);
        let suffix = if layer == ObjectLayer::Two {
            " (layer 2)"
        } else {
            ""
        };
        for &change in d.objects(layer) {
            let part = Part::Objects(layer);
            let line = match change {
                EntryChange::Added(i) => {
                    let name = names::object(&new[i], tileset).unwrap_or("Object");
                    let place = edit::object_position(&new[i])
                        .map_or_else(String::new, |(x, y)| format!(" at ({x}, {y})"));
                    Line {
                        mark: "+",
                        color: theme::OK,
                        text: format!("{name}{place}{suffix}"),
                        select: Some(Item::object(layer, i)),
                        look: None,
                        revert: Revert::Entry(part, change),
                    }
                }
                EntryChange::Removed { old: o, .. } => {
                    let name = names::object(&old[o], tileset).unwrap_or("Object");
                    let place = edit::object_position(&old[o])
                        .map_or_else(String::new, |(x, y)| format!(" at ({x}, {y})"));
                    Line {
                        mark: "−",
                        color: theme::ERROR,
                        text: format!("{name}{place}{suffix}"),
                        select: None,
                        look: head.bounds(layer, o),
                        revert: Revert::Entry(part, change),
                    }
                }
                EntryChange::Changed { old: o, new: i, .. } => {
                    let name = names::object(&new[i], tileset).unwrap_or("Object");
                    Line {
                        mark: "~",
                        color: theme::ACCENT,
                        text: format!("{name}: {}{suffix}", what_changed(&old[o], &new[i])),
                        select: Some(Item::object(layer, i)),
                        look: None,
                        revert: Revert::Entry(part, change),
                    }
                }
            };
            lines.push(line);
        }
    }
    let (old, new) = (&head.level.sprites.list, &level.sprites.list);
    for &change in &d.sprites {
        let line = match change {
            EntryChange::Added(i) => Line {
                mark: "+",
                color: theme::OK,
                text: format!(
                    "{} at ({}, {})",
                    names::sprite(new[i].id),
                    new[i].x,
                    new[i].y
                ),
                select: Some(Item::Sprite(i)),
                look: None,
                revert: Revert::Entry(Part::Sprites, change),
            },
            EntryChange::Removed { old: o, .. } => Line {
                mark: "−",
                color: theme::ERROR,
                text: format!(
                    "{} at ({}, {})",
                    names::sprite(old[o].id),
                    old[o].x,
                    old[o].y
                ),
                select: None,
                look: head.sprite_bounds(o),
                revert: Revert::Entry(Part::Sprites, change),
            },
            EntryChange::Changed { old: o, new: i, .. } => {
                let (a, b) = (&old[o], &new[i]);
                let what = if (a.x, a.y) != (b.x, b.y) {
                    format!("({}, {}) → ({}, {})", a.x, a.y, b.x, b.y)
                } else {
                    "its settings".to_string()
                };
                Line {
                    mark: "~",
                    color: theme::ACCENT,
                    text: format!("{}: {what}", names::sprite(b.id)),
                    select: Some(Item::Sprite(i)),
                    look: None,
                    revert: Revert::Entry(Part::Sprites, change),
                }
            }
        };
        lines.push(line);
    }
    for (changed, text, revert) in [
        (d.header, "The level's settings (header)", Revert::Header),
        (d.entrance, "The main entrance", Revert::Entrance),
        (
            d.settings,
            "Lunar Magic's settings or the size",
            Revert::Settings,
        ),
        (
            d.other,
            "Entrances, palette, graphics, or animation",
            Revert::Other,
        ),
    ] {
        if changed {
            lines.push(Line {
                mark: "~",
                color: theme::ACCENT,
                text: text.to_string(),
                select: None,
                look: None,
                revert,
            });
        }
    }
    lines
}

/// The edits that take one line's change back.
fn revert_edits(head: &Head, level: &Level, revert: &Revert) -> Vec<Edit> {
    let old = &head.level;
    match revert {
        Revert::Entry(part, change) => diff::revert(old, level, *part, *change),
        Revert::Header => vec![Edit::SetHeader(old.header)],
        Revert::Entrance => vec![Edit::SetEntrance(old.entrance)],
        Revert::Settings => vec![Edit::SetSettings(old.settings), Edit::SetSize(old.size)],
        Revert::Other => Vec::new(),
    }
}

/// The Changes tab: what differs from the last commit, each with a way
/// back.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(number) = app.current_number() else {
        return;
    };
    let workspace = app.workspace().cloned();
    let Some(open) = app.open_mut(number) else {
        return;
    };
    if open.head.is_none() && open.no_head.is_none() {
        let Some(workspace) = workspace else { return };
        match Head::read(open.document.path(), &workspace, number) {
            Ok(head) => open.head = Some(head),
            Err(why) => open.no_head = Some(why),
        }
    }
    if let Some(why) = &open.no_head {
        let text = match why {
            NoHead::NotInGit => {
                "The project is not in a git repository, so there is no commit to compare with."
                    .to_string()
            }
            NoHead::NotCommitted => {
                "This level's file has not been committed yet: all of it is new.".to_string()
            }
            NoHead::Unreadable(e) => format!("The committed file does not read: {e}"),
        };
        ui.label(RichText::new(text).color(theme::MUTED));
        if ui.button("Look again").clicked() {
            open.no_head = None;
        }
        return;
    }
    let Some(head) = &mut open.head else { return };
    head.update(&open.document);
    let level = open.document.level().clone();
    let lines = lines(head, &level);
    ui.horizontal(|ui| {
        let count = lines.len();
        let text = match count {
            0 => "No changes since the last commit".to_string(),
            1 => "1 change since the last commit".to_string(),
            n => format!("{n} changes since the last commit"),
        };
        ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
    });
    if ui.small_button("Read the last commit again").clicked() {
        open.head = None;
    }
    ui.label(
        RichText::new("Green was added, amber changed, red removed; the canvas marks them while this tab is open.")
            .small()
            .color(theme::MUTED),
    );
    if open.head.is_none() {
        return;
    }
    let mut select = None;
    let mut look = None;
    let mut revert = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, line) in lines.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(line.mark)
                            .color(line.color)
                            .monospace()
                            .strong(),
                    );
                    let row = ui
                        .add(
                            egui::Label::new(&line.text)
                                .sense(egui::Sense::click())
                                .truncate(),
                        )
                        .on_hover_text(&line.text);
                    if row.clicked() {
                        select = line.select;
                        look = line.look;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can = !matches!(line.revert, Revert::Other);
                        if ui
                            .add_enabled(can, egui::Button::new("Revert").small())
                            .on_disabled_hover_text("Revert these in the source pane, or with git")
                            .clicked()
                        {
                            revert = Some(i);
                        }
                    });
                });
            }
        });
    if let Some(item) = select {
        open.selection = vec![item];
        open.focus = true;
    }
    if let Some(area) = look
        && let Some(camera) = &mut open.camera
    {
        camera.offset = area.center().to_vec2() - open.canvas.size() / camera.zoom / 2.0;
    }
    if let Some(i) = revert
        && let Some(head) = &open.head
    {
        let edits = revert_edits(head, &level, &lines[i].revert);
        let label = format!("Revert: {}", lines[i].text);
        if app.apply(&label, edits)
            && let Some(open) = app.current_mut()
        {
            open.selection.clear();
        }
    }
}

/// Marks the changes on the canvas: added green, changed amber (with
/// where it was), removed red where it was.
pub fn draw(
    painter: &egui::Painter,
    head: &Head,
    level: &Level,
    geometry: &crate::selection::Geometry,
    to_screen: impl Fn(egui::Rect) -> egui::Rect,
) {
    let stroke = |c| egui::Stroke::new(2.0, c);
    let dashed = |r: egui::Rect, c| {
        let points = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        painter.extend(egui::Shape::dashed_line(&points, stroke(c), 5.0, 4.0));
    };
    let fill = |c: egui::Color32| c.gamma_multiply(0.18);
    let d = head.diff();
    for layer in [ObjectLayer::One, ObjectLayer::Two] {
        if layer == ObjectLayer::Two && !matches!(level.layer2, Layer2::Objects(_)) {
            continue;
        }
        for &change in d.objects(layer) {
            match change {
                EntryChange::Added(i) => {
                    if let Some(b) = geometry.bounds(Item::object(layer, i)) {
                        let r = to_screen(b);
                        painter.rect_filled(r, 2, fill(theme::OK));
                        painter.rect_stroke(r, 2, stroke(theme::OK), egui::StrokeKind::Outside);
                    }
                }
                EntryChange::Removed { old, .. } => {
                    if let Some(b) = head.bounds(layer, old) {
                        let r = to_screen(b);
                        painter.rect_filled(r, 2, fill(theme::ERROR));
                        dashed(r, theme::ERROR);
                    }
                }
                EntryChange::Changed { old, new, .. } => {
                    let now = geometry.bounds(Item::object(layer, new));
                    let was = head.bounds(layer, old);
                    if let Some(b) = now {
                        painter.rect_stroke(
                            to_screen(b),
                            2,
                            stroke(theme::ACCENT),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if let (Some(a), Some(b)) = (was, now)
                        && a != b
                    {
                        dashed(to_screen(a), theme::ERROR);
                        if a.min != b.min {
                            painter.arrow(
                                to_screen(a).center(),
                                to_screen(b).center() - to_screen(a).center(),
                                stroke(theme::ACCENT),
                            );
                        }
                    }
                }
            }
        }
    }
    for &change in &d.sprites {
        match change {
            EntryChange::Added(i) => {
                if let Some(b) = geometry.bounds(Item::Sprite(i)) {
                    painter.rect_stroke(
                        to_screen(b),
                        2,
                        stroke(theme::OK),
                        egui::StrokeKind::Outside,
                    );
                }
            }
            EntryChange::Removed { old, .. } => {
                if let Some(b) = head.sprite_bounds(old) {
                    dashed(to_screen(b), theme::ERROR);
                }
            }
            EntryChange::Changed { old, new, .. } => {
                if let Some(b) = geometry.bounds(Item::Sprite(new)) {
                    painter.rect_stroke(
                        to_screen(b),
                        2,
                        stroke(theme::ACCENT),
                        egui::StrokeKind::Outside,
                    );
                    if let Some(a) = head.sprite_bounds(old)
                        && a.min != b.min
                    {
                        dashed(to_screen(a), theme::ERROR);
                    }
                }
            }
        }
    }
}
