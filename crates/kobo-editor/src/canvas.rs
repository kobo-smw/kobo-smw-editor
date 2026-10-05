//! The level canvas: the picture, what is selected, and the mouse and
//! keys that select and move things.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Vec2,
};
use kobo_core::edit::{self, Edit, ObjectLayer};
use kobo_core::entrance::{self, MainEntranceTables, SeparateMidway};
use kobo_core::expand::ObjectRef;
use kobo_core::level::objects::Object;
use kobo_core::source::level::{Level, Sprite};

use crate::app::{App, OpenLevel, Pending, SpriteView};
use crate::palette::Placing;
use crate::selection::{self, Geometry, Item, objects};
use crate::theme;

const TILE: f32 = 16.0;
/// Zoom steps the camera keeps to: whole pixels look even at these.
const ZOOMS: [f32; 8] = [0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

/// Where the canvas looks: `offset` is the level pixel at its top left.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub zoom: f32,
    pub offset: Vec2,
    /// Zoom to fit the level's height (or a vertical level's width) on
    /// the next frame.
    pub fit_height: bool,
}

impl Camera {
    pub fn zoom_by(&mut self, factor: f32) {
        let want = self.zoom * factor;
        self.zoom = if factor > 1.0 {
            ZOOMS
                .into_iter()
                .find(|&z| z >= want - 0.01)
                .unwrap_or(ZOOMS[7])
        } else {
            ZOOMS
                .into_iter()
                .rev()
                .find(|&z| z <= want + 0.01)
                .unwrap_or(ZOOMS[0])
        };
    }

    fn fit(&mut self, level: Vec2, view: Vec2, vertical: bool) {
        let fits = |z: f32| {
            if vertical {
                level.x * z <= view.x
            } else {
                level.y * z <= view.y
            }
        };
        // A level too tall (or wide) to fit at full size is seen at full
        // size, a part at a time.
        self.zoom = ZOOMS
            .into_iter()
            .rev()
            .find(|&z| fits(z))
            .unwrap_or(1.0)
            .max(1.0);
        self.fit_height = false;
    }

    pub fn to_screen(self, canvas: Rect, at: Pos2) -> Pos2 {
        canvas.min + (at.to_vec2() - self.offset) * self.zoom
    }

    fn to_level(self, canvas: Rect, at: Pos2) -> Pos2 {
        ((at - canvas.min) / self.zoom + self.offset).to_pos2()
    }

    fn rect_to_screen(self, canvas: Rect, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(canvas, r.min), self.to_screen(canvas, r.max))
    }

    /// Keeps some of the level in view, and a level smaller than the
    /// view in its middle.
    fn clamp(&mut self, level: Vec2, view: Vec2) {
        let seen = view / self.zoom;
        let margin = 64.0;
        let axis = |offset: f32, level: f32, seen: f32| {
            if level <= seen {
                (level - seen) / 2.0
            } else {
                offset.clamp(-margin, level - seen + margin)
            }
        };
        self.offset = Vec2::new(
            axis(self.offset.x, level.x, seen.x),
            axis(self.offset.y, level.y, seen.y),
        );
    }

    /// Centres `area` (level pixels) in the view.
    fn centre_on(&mut self, area: Rect, view: Vec2) {
        self.offset = area.center().to_vec2() - view / self.zoom / 2.0;
    }
}

/// A drag on the canvas in progress.
#[derive(Clone, Debug)]
pub enum Drag {
    /// Moving the selection by whole tiles.
    Move {
        from: Pos2,
        items: Vec<Item>,
        delta: (i32, i32),
        /// Ctrl was held when it began: drop copies, not the items.
        copy: bool,
    },
    /// Selecting what a rectangle meets.
    Marquee { from: Pos2, to: Pos2, add: bool },
    /// A screen exit's label taken to another screen.
    Exit { index: usize, screen: u8 },
    /// An entrance's marker taken to another place: the settings nearest
    /// it, less how far the marker stood from where its settings' tables
    /// put it (an action, a pipe say, moves the player on).
    Entry {
        kind: crate::preview::EntryKind,
        at: Pos2,
        offset: Vec2,
    },
    /// Changing an object's size by its handle, in whole tiles.
    Resize {
        object: ObjectRef,
        from: Pos2,
        delta: (i32, i32),
    },
}

/// The selected object whose size can be dragged, with its box: one
/// standard object with a width, a height, or a length.
fn resizable(open: &OpenLevel, geometry: &Geometry) -> Option<(ObjectRef, Rect)> {
    let [Item::Object(o)] = open.selection[..] else {
        return None;
    };
    let object = objects(open.document.level(), o.layer)?.get(o.index)?;
    let sized = match object {
        Object::Standard {
            number, settings, ..
        } => edit::setting_fields(*number, *settings)
            .iter()
            .any(|f| matches!(f.name, "width" | "height" | "length")),
        _ => edit::map16_object_parts(object).is_some(),
    };
    sized.then(|| Some((o, geometry.bounds(Item::Object(o))?)))?
}

/// The edit that changes an object's size by `delta` tiles: its width or
/// length across, its height down.
fn resize_edit(level: &Level, o: ObjectRef, (dx, dy): (i32, i32)) -> Option<Edit> {
    let object = objects(level, o.layer)?.get(o.index)?;
    if let Some((_, w, h)) = edit::map16_object_parts(object) {
        let w = (i32::from(w) + dx).clamp(1, 16) as u16;
        let h = (i32::from(h) + dy).clamp(1, 16) as u16;
        let changed = edit::map16_object_sized(object, w, h);
        return (changed != *object).then_some(Edit::ReplaceObject {
            layer: o.layer,
            index: o.index,
            object: changed,
        });
    }
    let Object::Standard {
        number, settings, ..
    } = object
    else {
        return None;
    };
    let mut new = *settings;
    for field in edit::setting_fields(*number, *settings) {
        let d = match field.name {
            "width" | "length" => dx,
            "height" => dy,
            _ => continue,
        };
        let value = (i32::from(field.value) + d).clamp(i32::from(field.min), i32::from(field.max));
        new = edit::with_setting(*number, new, field.name, value as u16);
    }
    let mut changed = object.clone();
    if let Object::Standard { settings, .. } = &mut changed {
        *settings = new;
    }
    (changed != *object).then_some(Edit::ReplaceObject {
        layer: o.layer,
        index: o.index,
        object: changed,
    })
}

/// The handle's square on screen, at the box's bottom right corner.
fn handle_rect(camera: &Camera, canvas: Rect, bounds: Rect) -> Rect {
    Rect::from_center_size(camera.to_screen(canvas, bounds.max), Vec2::splat(10.0))
}

/// What is under the mouse, for the status bar.
#[derive(Clone, Debug, Default)]
pub struct Hover {
    pub x: i32,
    pub y: i32,
    pub screen: i32,
    pub tile: Option<u16>,
    pub owner: Option<String>,
}

fn hover_id() -> egui::Id {
    egui::Id::new("canvas-hover")
}

/// What the canvas last found under the mouse.
pub fn last_hover(ctx: &egui::Context) -> Option<Hover> {
    ctx.data(|d| d.get_temp::<Option<Hover>>(hover_id()))
        .flatten()
}

/// The edits that move `items` by `delta` tiles, and the items as they
/// will be: a sprite moved to another screen moves in the list too, to
/// stay where the game's loader reaches it.
pub fn move_edits(level: &Level, items: &[Item], (dx, dy): (i32, i32)) -> (Vec<Edit>, Vec<Item>) {
    let shift = |v: u16, d: i32| (i32::from(v) + d).clamp(0, i32::from(u16::MAX)) as u16;
    let mut edits = Vec::new();
    let mut sprites = Vec::new();
    for &item in items {
        match item {
            Item::Object(o) => {
                let Some(object) = objects(level, o.layer).and_then(|l| l.get(o.index)) else {
                    continue;
                };
                let Some((x, y)) = edit::object_position(object) else {
                    continue;
                };
                edits.push(Edit::ReplaceObject {
                    layer: o.layer,
                    index: o.index,
                    object: edit::object_at(object, shift(x, dx), shift(y, dy)),
                });
            }
            Item::Sprite(i) => {
                if let Some(sprite) = level.sprites.list.get(i) {
                    let mut sprite = sprite.clone();
                    sprite.x = shift(sprite.x, dx);
                    sprite.y = shift(sprite.y, dy);
                    sprites.push((i, sprite));
                }
            }
        }
    }
    let mut moved: Vec<Item> = items
        .iter()
        .copied()
        .filter(|i| matches!(i, Item::Object(_)))
        .collect();
    if let Ok((sprite_edits, at)) = edit::move_sprites(level, &sprites) {
        edits.extend(sprite_edits);
        moved.extend(at.into_iter().map(Item::Sprite));
    }
    (edits, moved)
}

/// The edits that delete `items`: later entries first, so the indices of
/// the rest hold.
pub fn delete_edits(items: &[Item]) -> Vec<Edit> {
    let mut items = items.to_vec();
    items.sort_by_key(|item| match *item {
        Item::Object(o) => (0, o.layer == ObjectLayer::Two, std::cmp::Reverse(o.index)),
        Item::Sprite(i) => (1, false, std::cmp::Reverse(i)),
    });
    items
        .into_iter()
        .map(|item| match item {
            Item::Object(o) => Edit::RemoveObject {
                layer: o.layer,
                index: o.index,
            },
            Item::Sprite(index) => Edit::RemoveSprite { index },
        })
        .collect()
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        format!("{n} {one}s")
    }
}

fn label_for(items: &[Item], verb: &str) -> String {
    let what = match items {
        [Item::Object(_)] => "object".to_string(),
        [Item::Sprite(_)] => "sprite".to_string(),
        _ => plural(items.len(), "item"),
    };
    format!("{verb} {what}")
}

/// Where an object moves in its list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// Drawn last, over everything.
    Front,
    Forward,
    Backward,
    /// Drawn first, under everything.
    Back,
}

/// The edit that moves an object in its list, its label, and the object
/// where it ends up; `None` where it is already.
pub fn reorder(level: &Level, o: ObjectRef, order: Order) -> Option<(&'static str, Edit, Item)> {
    let last = objects(level, o.layer)?.len().checked_sub(1)?;
    let (to, label) = match order {
        Order::Front => (last, "Bring object to front"),
        Order::Forward => ((o.index + 1).min(last), "Bring object forward"),
        Order::Backward => (o.index.saturating_sub(1), "Send object backward"),
        Order::Back => (0, "Send object to back"),
    };
    (to != o.index).then_some((
        label,
        Edit::ReorderObject {
            layer: o.layer,
            from: o.index,
            to,
        },
        Item::object(o.layer, to),
    ))
}

/// Every object and sprite of `level` in drawing order: layer 1's
/// objects, layer 2's, then the sprites.
fn drawing_order(level: &Level) -> Vec<Item> {
    let mut order: Vec<Item> = (0..level.layer1.len())
        .map(|i| Item::object(ObjectLayer::One, i))
        .collect();
    if let Some(list) = objects(level, ObjectLayer::Two) {
        order.extend((0..list.len()).map(|i| Item::object(ObjectLayer::Two, i)));
    }
    order.extend((0..level.sprites.list.len()).map(Item::Sprite));
    order
}

/// The keys that act on the selection, when no text field has them.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    let Some(open) = app.current() else { return };
    let selection = open.selection.clone();
    let order = ctx.input_mut(|i| {
        let command = egui::Modifiers::COMMAND;
        let shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        let mut key = |m, k| i.consume_shortcut(&egui::KeyboardShortcut::new(m, k));
        if key(shift, Key::CloseBracket) {
            Some(Order::Front)
        } else if key(shift, Key::OpenBracket) {
            Some(Order::Back)
        } else if key(command, Key::CloseBracket) {
            Some(Order::Forward)
        } else if key(command, Key::OpenBracket) {
            Some(Order::Backward)
        } else {
            None
        }
    });
    if let (Some(order), [Item::Object(o)]) = (order, &selection[..])
        && let Some(open) = app.current()
        && let Some((label, edit, item)) = reorder(open.document.level(), *o, order)
        && app.apply(label, vec![edit])
        && let Some(open) = app.current_mut()
    {
        open.selection = vec![item];
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::G)) {
        app.view.grid = !app.view.grid;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F))
        && let Some(open) = app.current_mut()
    {
        open.focus = true;
        open.centre = true;
    }
    // Tab and Shift+Tab: the next or the one before in drawing order,
    // which reaches what draws behind something else.
    let step = ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::SHIFT, Key::Tab) {
            Some(false)
        } else if i.consume_key(egui::Modifiers::NONE, Key::Tab) {
            Some(true)
        } else {
            None
        }
    });
    if step.is_some() {
        // Not egui's move to the next widget, which would put the
        // keyboard in a text field.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }
    if let Some(forward) = step
        && let Some(open) = app.current_mut()
    {
        let order = drawing_order(open.document.level());
        if !order.is_empty() {
            let at = selection
                .first()
                .and_then(|item| order.iter().position(|i| i == item));
            let next = match (at, forward) {
                (None, true) => 0,
                (None, false) => order.len() - 1,
                (Some(at), true) => (at + 1) % order.len(),
                (Some(at), false) => (at + order.len() - 1) % order.len(),
            };
            open.selection = vec![order[next]];
            open.focus = true;
        }
    }
    let (delete, escape, all, nudge) = ctx.input_mut(|i| {
        let step = if i.modifiers.shift { 16 } else { 1 };
        let mut nudge = (0, 0);
        for (key, d) in [
            (Key::ArrowLeft, (-step, 0)),
            (Key::ArrowRight, (step, 0)),
            (Key::ArrowUp, (0, -step)),
            (Key::ArrowDown, (0, step)),
        ] {
            if i.key_pressed(key) {
                nudge = (nudge.0 + d.0, nudge.1 + d.1);
            }
        }
        (
            i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            i.key_pressed(Key::Escape),
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::A,
            )),
            nudge,
        )
    });
    if delete && !selection.is_empty() {
        let label = label_for(&selection, "Delete");
        if app.apply(&label, delete_edits(&selection))
            && let Some(open) = app.current_mut()
        {
            open.selection.clear();
        }
    }
    if escape {
        if app.placing.is_some() {
            app.placing = None;
        } else if let Some(open) = app.current_mut() {
            open.selection.clear();
            open.drag = None;
        }
    }
    if all && let Some(open) = app.current_mut() {
        let level = open.document.level();
        let mut items: Vec<Item> = (0..level.layer1.len())
            .map(|i| Item::object(ObjectLayer::One, i))
            .collect();
        items.extend((0..level.sprites.list.len()).map(Item::Sprite));
        open.selection = items;
    }
    if nudge != (0, 0) && !selection.is_empty() {
        let Some(open) = app.current() else { return };
        let (edits, moved) = move_edits(open.document.level(), &selection, nudge);
        if app.apply(&label_for(&selection, "Move"), edits)
            && let Some(open) = app.current_mut()
        {
            open.selection = moved;
        }
    }
}

/// A move the mouse finished, to apply once the canvas is drawn.
struct Finished {
    label: String,
    edits: Vec<Edit>,
    /// What moved, as the picture on screen knows it.
    items: Vec<Item>,
    /// The same, as the document will have them.
    moved: Vec<Item>,
    delta: (i32, i32),
}

/// Draws the canvas and handles the mouse on it.
pub fn show(app: &mut App, ui: &mut egui::Ui) -> Option<Hover> {
    let view = app.view;
    let size = ui.available_size() - Vec2::new(0.0, MINIMAP_HEIGHT);
    let (response, painter) =
        ui.allocate_painter(size.max(Vec2::splat(64.0)), Sense::click_and_drag());
    let hover = show_canvas(app, ui, response, painter, view);
    minimap(app, ui);
    hover
}

/// The strip under the canvas: the whole level, small.
const MINIMAP_HEIGHT: f32 = 64.0;

/// The whole level, small, with the view's rectangle on it; a click or a
/// drag there brings the view to that place.
fn minimap(app: &mut App, ui: &mut egui::Ui) {
    let (response, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width(), MINIMAP_HEIGHT),
        Sense::click_and_drag(),
    );
    let strip = response.rect;
    painter.rect_filled(strip, CornerRadius::ZERO, theme::PANEL);
    painter.line_segment(
        [strip.left_top(), strip.right_top()],
        Stroke::new(1.0, theme::LINE),
    );
    let Some(number) = app.current_number() else {
        return;
    };
    let Some(open) = app.open_mut(number) else {
        return;
    };
    let (Some(picture), Some(camera)) = (&open.picture, &mut open.camera) else {
        return;
    };
    let room = strip.shrink2(Vec2::new(16.0, 8.0));
    let scale = (room.height() / picture.size.y).min(room.width() / picture.size.x);
    let shown = Rect::from_center_size(room.center(), picture.size * scale);
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(picture.small.id(), shown, uv, Color32::WHITE);
    painter.rect_stroke(
        shown,
        CornerRadius::ZERO,
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Outside,
    );
    let seen = Rect::from_min_size(camera.offset.to_pos2(), open.canvas.size() / camera.zoom);
    let seen_on_strip = Rect::from_min_max(
        shown.min + seen.min.to_vec2() * scale,
        shown.min + seen.max.to_vec2() * scale,
    )
    .intersect(shown.expand(2.0));
    painter.rect_stroke(
        seen_on_strip,
        CornerRadius::same(2),
        Stroke::new(2.0, theme::ACCENT),
        StrokeKind::Outside,
    );
    if (response.clicked() || response.dragged())
        && let Some(at) = response.interact_pointer_pos()
    {
        let level = (at - shown.min) / scale;
        camera.offset = level - seen.size() / 2.0;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn show_canvas(
    app: &mut App,
    ui: &mut egui::Ui,
    response: egui::Response,
    painter: egui::Painter,
    view: crate::app::View,
) -> Option<Hover> {
    let canvas = response.rect;
    painter.rect_filled(canvas, CornerRadius::ZERO, theme::CANVAS);
    let Some(number) = app.current_number() else {
        painter.text(
            canvas.center(),
            Align2::CENTER_CENTER,
            "Choose a level",
            FontId::proportional(16.0),
            theme::MUTED,
        );
        return None;
    };
    let sprites_on = view.sprites != SpriteView::Hidden;
    let placing = app.placing.clone();
    let tables = app.entrance_tables;
    let mut place_at: Option<(u16, u16)> = None;
    let place_layer = app.place_layer;
    let mut stop_placing = false;
    let mut finished: Option<Finished> = None;
    let mut context_edit: Option<(String, Vec<Edit>)> = None;
    let mut find_query: Option<String> = None;
    let mut copy_drop: Option<(Vec<Item>, (i32, i32))> = None;
    let mut play: Option<kobo_core::playtest::Start> = None;
    let app_powerup = app.play.powerup;
    // A screen exit's label double-clicked: go where it leads.
    let mut follow: Option<kobo_core::level::objects::ScreenExit> = None;
    let mut clip: Option<(crate::clipboard::Action, Option<(u16, u16)>)> = None;
    let can_paste = !app.clipboard.is_empty();
    let mut hover = None;
    {
        let open = app.open_mut(number)?;
        open.canvas = canvas;
        let Some(geometry) = &open.geometry else {
            let text = match &open.render_error {
                Some(e) => format!("This level does not draw: {e}"),
                None => "Drawing…".to_string(),
            };
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(16.0),
                theme::MUTED,
            );
            return None;
        };
        let level_size = geometry.size();
        let vertical = geometry.loaded.tiles.vertical;
        let camera = open.camera.get_or_insert_with(|| {
            let mut camera = Camera {
                zoom: 2.0,
                offset: Vec2::ZERO,
                fit_height: false,
            };
            camera.fit(level_size, canvas.size(), vertical);
            // The level is seen first where the player starts: a third of
            // the way in along it. Without that, from its bottom, where
            // the ground is.
            let seen = canvas.size() / camera.zoom;
            camera.offset.y = level_size.y - seen.y;
            if let Some(start) = open
                .entries
                .iter()
                .find(|e| e.kind == crate::preview::EntryKind::Main)
            {
                let (x, y) = (start.x as f32, start.y as f32);
                if vertical {
                    camera.offset.y = y - seen.y * 2.0 / 3.0;
                } else {
                    camera.offset.x = x - seen.x / 3.0;
                    // A level taller than the view: where the player is.
                    if level_size.y > seen.y {
                        camera.offset.y = y - seen.y / 2.0;
                    }
                }
            }
            // The first view stays inside the level.
            let most = (level_size - seen).max(Vec2::ZERO);
            camera.offset = camera.offset.clamp(Vec2::ZERO, most);
            camera
        });
        if camera.fit_height {
            camera.fit(level_size, canvas.size(), vertical);
        }
        if let Some(kind) = open.look_at
            && let Some(entry) = open.entries.iter().find(|e| e.kind == kind)
        {
            open.look_at = None;
            let seen = canvas.size() / camera.zoom;
            let at = Vec2::new(entry.x as f32, entry.y as f32);
            let most = (level_size - seen).max(Vec2::ZERO);
            camera.offset = (at - seen / 2.0).clamp(Vec2::ZERO, most);
        }
        if open.focus {
            open.focus = false;
            let centre = std::mem::take(&mut open.centre);
            let bounds = open
                .selection
                .iter()
                .filter_map(|&item| geometry.bounds(item))
                .reduce(|a, b| a.union(b));
            // Only what is out of view is brought into it.
            let seen = Rect::from_min_size(camera.offset.to_pos2(), canvas.size() / camera.zoom);
            if let Some(bounds) = bounds
                && (centre || !seen.contains_rect(bounds))
            {
                camera.centre_on(bounds, canvas.size());
            }
        }

        // Zoom about the mouse, and scroll.
        if response.hovered() {
            let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if zoom != 1.0
                && let Some(at) = response.hover_pos()
            {
                let before = camera.to_level(canvas, at);
                camera.zoom_by(if zoom > 1.0 { 1.25 } else { 0.8 });
                let after = camera.to_level(canvas, at);
                camera.offset += before - after;
            } else if scroll != Vec2::ZERO {
                camera.offset -= scroll / camera.zoom;
            }
        }
        let space = ui.input(|i| i.key_down(Key::Space));
        if response.dragged_by(PointerButton::Middle)
            || (space && response.dragged_by(PointerButton::Primary))
        {
            camera.offset -= response.drag_delta() / camera.zoom;
        }
        camera.clamp(level_size, canvas.size());
        let camera = *camera;

        let pointer = response.hover_pos().map(|p| camera.to_level(canvas, p));
        if let Some(at) = pointer
            && ui.input(|i| i.key_pressed(Key::F5))
        {
            play = Some(crate::play::start_at(number, (at.x, at.y), app_powerup));
        }
        let pills = exit_pills(&painter, canvas, &camera, open, geometry);
        let on_pill = |at: Pos2| pills.iter().find(|p| p.rect.contains(at)).map(|p| p.item);
        let on_exit = response.hover_pos().and_then(on_pill).is_some();
        let hovered_item = match response.hover_pos().and_then(on_pill) {
            Some(item) => Some(item),
            None => pointer.and_then(|p| geometry.item_at(p, sprites_on)),
        };
        if response.double_clicked()
            && let Some(Item::Object(o)) = response.hover_pos().and_then(on_pill)
            && let Some(Object::ScreenExit(exit)) = open.document.level().layer1.get(o.index)
        {
            follow = Some(*exit);
        }
        if response.secondary_clicked() {
            open.menu_at = pointer;
            // The menu acts on what was right-clicked.
            if let Some(item) = hovered_item
                && !open.selection.contains(&item)
                && placing.is_none()
            {
                open.selection = vec![item];
            }
        }

        // Placing from the palette: each click puts one down.
        if placing.is_some() && !space {
            if response.clicked_by(PointerButton::Primary)
                && let Some(at) = pointer
            {
                let layer = match placing {
                    Some(Placing::Object(_) | Placing::Map16(_)) => place_layer,
                    _ => ObjectLayer::One,
                };
                place_at = geometry.tile_at(layer, at);
            }
            if response.clicked_by(PointerButton::Secondary) {
                stop_placing = true;
            }
        }

        // Selecting and moving with the primary button.
        let shift = ui.input(|i| i.modifiers.shift);
        if !space && placing.is_none() {
            if response.clicked_by(PointerButton::Primary) {
                match hovered_item {
                    Some(item) if shift => {
                        if let Some(i) = open.selection.iter().position(|&s| s == item) {
                            open.selection.remove(i);
                        } else {
                            open.selection.push(item);
                        }
                    }
                    Some(item) => open.selection = vec![item],
                    None if !shift => open.selection.clear(),
                    None => {}
                }
            }
            let handle = resizable(open, geometry);
            if response.drag_started_by(PointerButton::Primary)
                && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            {
                let from = camera.to_level(canvas, origin);
                let pill = on_pill(origin);
                // An entrance's marker, placed by the game's tables.
                let on_entry = if view.entrances
                    && let Some(tables) = tables
                {
                    open.entries
                        .iter()
                        .find(|e| start_rect(e).contains(from))
                        .and_then(|e| {
                            let placement = entry_placement(open.document.level(), e.kind)?;
                            let (px, py) = placement.position(tables, vertical);
                            let offset = Vec2::new((e.x - px) as f32, (e.y - py) as f32);
                            Some((e.kind, offset))
                        })
                } else {
                    None
                };
                let on_handle = handle.filter(|(_, bounds)| {
                    handle_rect(&camera, canvas, *bounds)
                        .expand(3.0)
                        .contains(origin)
                });
                match geometry.item_at(from, sprites_on) {
                    _ if on_entry.is_some() => {
                        let (kind, offset) = on_entry.expect("checked");
                        open.drag = Some(Drag::Entry {
                            kind,
                            at: from,
                            offset,
                        });
                    }
                    _ if pill.is_some() => {
                        if let Some(Item::Object(o)) = pill
                            && let Some(Object::ScreenExit(exit)) =
                                open.document.level().layer1.get(o.index)
                        {
                            open.selection = vec![Item::object(ObjectLayer::One, o.index)];
                            open.drag = Some(Drag::Exit {
                                index: o.index,
                                screen: exit.screen,
                            });
                        }
                    }
                    _ if on_handle.is_some() => {
                        let (object, _) = on_handle.expect("checked");
                        open.drag = Some(Drag::Resize {
                            object,
                            from,
                            delta: (0, 0),
                        });
                    }
                    Some(item) => {
                        if !open.selection.contains(&item) {
                            if !shift {
                                open.selection.clear();
                            }
                            open.selection.push(item);
                        }
                        open.drag = Some(Drag::Move {
                            from,
                            items: open.selection.clone(),
                            delta: (0, 0),
                            copy: ui.input(|i| i.modifiers.command),
                        });
                    }
                    None => {
                        open.drag = Some(Drag::Marquee {
                            from,
                            to: from,
                            add: shift,
                        })
                    }
                }
            }
            if response.dragged_by(PointerButton::Primary)
                && let Some(at) = pointer
            {
                match &mut open.drag {
                    Some(Drag::Move { from, delta, .. }) => {
                        let d = (at - *from) / TILE;
                        *delta = (d.x.round() as i32, d.y.round() as i32);
                    }
                    Some(Drag::Marquee { to, .. }) => *to = at,
                    Some(Drag::Resize { from, delta, .. }) => {
                        let d = (at - *from) / TILE;
                        *delta = (d.x.round() as i32, d.y.round() as i32);
                    }
                    Some(Drag::Entry { at: entry, .. }) => *entry = at,
                    Some(Drag::Exit { screen, .. }) => {
                        let along = if vertical { at.y } else { at.x };
                        *screen = (along / 256.0).floor().clamp(0.0, 31.0) as u8;
                    }
                    None => {}
                }
            }
            if response.drag_stopped() {
                match open.drag.take() {
                    Some(Drag::Move {
                        items,
                        delta,
                        copy: true,
                        ..
                    }) if delta != (0, 0) => copy_drop = Some((items, delta)),
                    Some(Drag::Move { items, delta, .. }) if delta != (0, 0) => {
                        let (edits, moved) = move_edits(open.document.level(), &items, delta);
                        finished = Some(Finished {
                            label: label_for(&items, "Move"),
                            edits,
                            items,
                            moved,
                            delta,
                        });
                    }
                    Some(Drag::Entry { kind, at, offset }) => {
                        let level = open.document.level();
                        if let Some(tables) = tables
                            && let Some(placement) = entry_placement(level, kind)
                        {
                            let target = at - Vec2::new(8.0, 0.0) - offset;
                            let target = (target.x as i32, target.y as i32);
                            let placement = placement.nearest(tables, target, vertical);
                            if let Some(edit) = move_entry(level, kind, placement) {
                                context_edit = Some(edit);
                            }
                        }
                    }
                    Some(Drag::Exit { index, screen }) => {
                        let level = open.document.level();
                        let taken = exits(level).any(|(i, e)| i != index && e.screen == screen);
                        if let Some(Object::ScreenExit(exit)) = level.layer1.get(index)
                            && exit.screen != screen
                            && !taken
                        {
                            let mut moved = *exit;
                            moved.screen = screen;
                            context_edit = Some((
                                "Move screen exit".to_string(),
                                vec![Edit::ReplaceObject {
                                    layer: ObjectLayer::One,
                                    index,
                                    object: Object::ScreenExit(moved),
                                }],
                            ));
                        }
                    }
                    Some(Drag::Resize { object, delta, .. }) => {
                        if let Some(edit) = resize_edit(open.document.level(), object, delta) {
                            context_edit = Some(("Resize object".to_string(), vec![edit]));
                        }
                    }
                    Some(Drag::Marquee { from, to, add }) => {
                        let area = Rect::from_two_pos(from, to);
                        let found = geometry.items_in(area, sprites_on);
                        if !add {
                            open.selection.clear();
                        }
                        for item in found {
                            if !open.selection.contains(&item) {
                                open.selection.push(item);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // A menu on what was right-clicked.
        if placing.is_none() {
            response.context_menu(|ui| {
                clip = clipboard_menu(ui, open, can_paste);
                context_menu(
                    ui,
                    open,
                    number,
                    &mut context_edit,
                    &mut find_query,
                    &mut play,
                );
            });
        }

        let hovered_item = hovered_item.filter(|_| placing.is_none());
        let selected = |item| open.selection.contains(&item);
        let pill_shapes: Vec<(Rect, std::sync::Arc<egui::Galley>, bool)> = pills
            .into_iter()
            .map(|p| {
                (
                    p.rect,
                    p.galley,
                    selected(p.item) || hovered_item == Some(p.item),
                )
            })
            .collect();
        draw(
            &painter,
            canvas,
            &camera,
            open,
            geometry_ref(open),
            view,
            hovered_item,
        );
        for (rect, galley, lit) in pill_shapes {
            let fill = if lit {
                theme::SELECTION
            } else {
                Color32::from_rgb(0x2b, 0x5d, 0x3a)
            };
            let text = if lit {
                theme::ON_SELECTION
            } else {
                Color32::WHITE
            };
            painter.rect_filled(rect, CornerRadius::same(4), fill);
            painter.galley(rect.min + Vec2::new(6.0, 3.0), galley, text);
        }
        // An entrance being moved: where its settings would put it.
        if let (Some(Drag::Entry { kind, at, offset }), Some(tables)) = (&open.drag, tables)
            && let Some(placement) = entry_placement(open.document.level(), *kind)
        {
            let target = *at - Vec2::new(8.0, 0.0) - *offset;
            let placement = placement.nearest(tables, (target.x as i32, target.y as i32), vertical);
            let (px, py) = placement.position(tables, vertical);
            let entry = crate::preview::Entry {
                kind: *kind,
                x: px + offset.x as i32,
                y: py + offset.y as i32,
            };
            draw_entries(&painter, canvas, &camera, std::slice::from_ref(&entry));
            let r = camera.rect_to_screen(canvas, start_rect(&entry));
            painter.text(
                r.center_bottom() + Vec2::new(0.0, 6.0),
                Align2::CENTER_TOP,
                placement.describe(),
                FontId::monospace(11.0),
                theme::OK,
            );
        }
        if let (Some(placing), Some(at)) = (&placing, pointer) {
            let layer = match placing {
                Placing::Object(_) | Placing::Map16(_) => place_layer,
                Placing::Sprite(_) => ObjectLayer::One,
            };
            let offset = geometry_ref(open).layer_offset(layer);
            let local = at - offset;
            let tile = selection::tile_rect(
                (local.x / TILE).floor() as i32,
                (local.y / TILE).floor() as i32,
            )
            .translate(offset);
            let r = camera.rect_to_screen(canvas, tile);
            painter.rect(
                r,
                CornerRadius::same(2),
                theme::ACCENT.gamma_multiply(0.3),
                Stroke::new(2.0, theme::ACCENT),
                StrokeKind::Outside,
            );
            let name = placing.name(open.document.level().header.object_tileset);
            let galley = painter.layout_no_wrap(name, FontId::monospace(11.0), theme::ON_SELECTION);
            let at = r.left_top() - Vec2::new(0.0, 20.0);
            let label = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(5.0, 2.0));
            painter.rect_filled(label, CornerRadius::same(3), theme::ACCENT);
            painter.galley(at, galley, theme::ON_SELECTION);
        }

        // What is under the mouse, named beside it, while nothing is being
        // dragged or placed.
        if let Some(item) = hovered_item
            && open.drag.is_none()
            && placing.is_none()
        {
            let level = open.document.level();
            let name = selection::describe(level, item);
            let place = match item {
                Item::Object(o) => objects(level, o.layer)
                    .and_then(|l| l.get(o.index))
                    .and_then(edit::object_position),
                Item::Sprite(i) => level.sprites.list.get(i).map(|s| (s.x, s.y)),
            };
            let what = match item {
                Item::Object(o) if o.layer == ObjectLayer::Two => {
                    format!("layer 2 object {}", o.index)
                }
                Item::Object(o) => format!("object {}", o.index),
                Item::Sprite(i) => format!("sprite {i}"),
            };
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(RichText::new(name).strong());
                let at = place.map_or_else(String::new, |(x, y)| format!(" at ({x}, {y})"));
                ui.label(
                    RichText::new(format!("{what}{at}"))
                        .small()
                        .color(theme::MUTED),
                );
                if on_exit {
                    ui.label(
                        RichText::new("Drag to another screen; double-click to go where it leads")
                            .small()
                            .color(theme::MUTED),
                    );
                }
            });
        }
        if let Some(at) = pointer {
            let (x, y) = ((at.x / TILE).floor() as i32, (at.y / TILE).floor() as i32);
            let geometry = geometry_ref(open);
            let (w, h) = geometry.loaded.tiles.size();
            let inside = x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h;
            hover = Some(Hover {
                x,
                y,
                screen: if vertical { y / 16 } else { x / 16 },
                tile: inside.then(|| geometry.loaded.tiles.tile_at(x as usize, y as usize)),
                owner: open
                    .failed_sprites
                    .iter()
                    .find(|(fx, fy, _)| (*fx, *fy) == (x, y))
                    .map(|(_, _, why)| format!("Not drawn: {why}"))
                    .or_else(|| {
                        hovered_item.map(|item| {
                            let name = selection::describe(&geometry.level, item);
                            match item {
                                Item::Object(o) => format!("{name} (object {})", o.index),
                                Item::Sprite(i) => format!("{name} (sprite {i})"),
                            }
                        })
                    }),
            });
        }
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(hover_id(), hover.clone()));

    if let Some(Finished {
        label,
        edits,
        items,
        moved,
        delta,
    }) = finished
        && app.apply(&label, edits)
        && let Some(open) = app.open_mut(number)
    {
        open.selection = moved;
        open.pending = Some(Pending {
            items,
            delta,
            request: open.requested,
        });
    }
    if let Some((label, edits)) = context_edit {
        app.apply(&label, edits);
    }
    if let Some(exit) = follow
        && let Some(leads) = app.exit_leads(number, exit)
    {
        app.follow_exit(leads);
    }
    if let Some((items, delta)) = copy_drop {
        crate::clipboard::copy_by(app, &items, delta);
    }
    if let Some(start) = play {
        crate::play::start(app, start);
    }
    if let Some(query) = find_query {
        app.find.query = query;
        app.left = crate::app::LeftTab::Find;
    }
    if let Some((action, at)) = clip {
        let ctx = ui.ctx().clone();
        crate::clipboard::run(app, &ctx, action, None, at);
    }
    if stop_placing {
        app.placing = None;
    }
    if let (Some(placing), Some((x, y))) = (placing, place_at) {
        place(app, number, &placing, place_layer, x, y);
    }
    hover
}

/// The level's screen exits, by their index in layer 1's list.
fn exits(
    level: &Level,
) -> impl Iterator<Item = (usize, kobo_core::level::objects::ScreenExit)> + '_ {
    level
        .layer1
        .iter()
        .enumerate()
        .filter_map(|(i, o)| match o {
            Object::ScreenExit(exit) => Some((i, *exit)),
            _ => None,
        })
}

/// A screen exit's label on the canvas, at the top of its screen.
struct ExitPill {
    item: Item,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
}

fn exit_pills(
    painter: &egui::Painter,
    canvas: Rect,
    camera: &Camera,
    open: &OpenLevel,
    geometry: &Geometry,
) -> Vec<ExitPill> {
    let level = open.document.level();
    let vertical = geometry.loaded.tiles.vertical;
    exits(level)
        .map(|(index, exit)| {
            let target = edit::ExitTarget::of(exit, open.number);
            let text = if target.secondary {
                format!("EXIT → entrance {:03X}", target.destination)
            } else {
                format!("EXIT → level {:03X}", target.destination)
            };
            let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), Color32::WHITE);
            let start = f32::from(exit.screen) * 256.0;
            let corner = if vertical {
                Pos2::new(0.0, start)
            } else {
                Pos2::new(start, 0.0)
            };
            let mut at = camera.to_screen(canvas, corner);
            // Below the screen's label, and in view along the other axis.
            if vertical {
                at.x = at.x.max(canvas.left()) + 6.0;
                at.y += 26.0;
            } else {
                at.x += 6.0;
                at.y = at.y.max(canvas.top()) + 26.0;
            }
            let rect = Rect::from_min_size(at, galley.size() + Vec2::new(12.0, 6.0));
            ExitPill {
                item: Item::object(ObjectLayer::One, index),
                rect,
                galley,
            }
        })
        .collect()
}

/// Puts what the palette chose at tile (`x`, `y`): an object last in
/// layer 1's list, so it draws over the rest; a sprite where the loader
/// reaches it.
fn place(app: &mut App, number: u16, placing: &Placing, layer: ObjectLayer, x: u16, y: u16) {
    let Some(open) = app.open_mut(number) else {
        return;
    };
    let level = open.document.level();
    let (label, edits, item) = match placing {
        Placing::Object(template) => {
            let index = objects(level, layer).map_or(0, Vec::len);
            let edit = Edit::InsertObject {
                layer,
                index,
                object: edit::object_at(template, x, y),
            };
            ("Add object", vec![edit], Item::object(layer, index))
        }
        Placing::Map16(tile) => {
            let index = objects(level, layer).map_or(0, Vec::len);
            let edit = Edit::InsertObject {
                layer,
                index,
                object: edit::map16_object(*tile, x, y),
            };
            ("Add Map16 tile", vec![edit], Item::object(layer, index))
        }
        Placing::Sprite(id) => {
            let sprite = Sprite {
                id: *id,
                x,
                y,
                extra_bits: 0,
                extension: Vec::new(),
            };
            let (edit, index) = edit::insert_sprite(level, sprite);
            ("Add sprite", vec![edit], Item::Sprite(index))
        }
    };
    if app.apply(label, edits)
        && let Some(open) = app.open_mut(number)
    {
        open.selection = vec![item];
    }
}

fn geometry_ref(open: &OpenLevel) -> &Geometry {
    open.geometry.as_ref().expect("checked above")
}

/// The menu's clipboard part: what it asked for, and where a paste goes
/// (the tile the menu was opened on).
fn clipboard_menu(
    ui: &mut egui::Ui,
    open: &OpenLevel,
    can_paste: bool,
) -> Option<(crate::clipboard::Action, Option<(u16, u16)>)> {
    use crate::clipboard::Action;
    let selected = !open.selection.is_empty();
    let at = open
        .menu_at
        .filter(|p| p.x >= 0.0 && p.y >= 0.0)
        .map(|p| ((p.x / TILE) as u16, (p.y / TILE) as u16));
    let mut chosen = None;
    for (text, keys, action, enabled) in [
        ("Cut", "Ctrl+X", Action::Cut, selected),
        ("Copy", "Ctrl+C", Action::Copy, selected),
        ("Paste here", "Ctrl+V", Action::Paste, can_paste),
        ("Duplicate", "Ctrl+D", Action::Duplicate, selected),
    ] {
        let button = egui::Button::new(text).shortcut_text(keys);
        if ui.add_enabled(enabled, button).clicked() {
            chosen = Some((action, at));
            ui.close();
        }
    }
    ui.separator();
    chosen
}

fn context_menu(
    ui: &mut egui::Ui,
    open: &mut OpenLevel,
    number: u16,
    edit: &mut Option<(String, Vec<Edit>)>,
    find: &mut Option<String>,
    play: &mut Option<kobo_core::playtest::Start>,
) {
    // The game, from where the menu was opened.
    if let Some(at) = open.menu_at {
        ui.menu_button("Play from here", |ui| {
            for (i, name) in crate::play::POWERUPS.iter().enumerate() {
                if ui.button(*name).clicked() {
                    *play = Some(crate::play::start_at(number, (at.x, at.y), i as u8));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Build the project to start here, and open it (F5 where the mouse is)");
        ui.separator();
    }
    // A screen exit for the screen the menu was opened on, if it has none.
    if let Some(at) = open.menu_at {
        let vertical = open.document.level().header.level_mode.layer1_vertical();
        let along = if vertical { at.y } else { at.x };
        let screen = (along / 256.0).floor();
        let level = open.document.level();
        let has_exit = exits(level).any(|(_, exit)| f32::from(exit.screen) == screen);
        if (0.0..32.0).contains(&screen) && !has_exit {
            let screen = screen as u8;
            if ui
                .button(format!("Add screen exit on screen {screen:02X}"))
                .clicked()
            {
                let exit = edit::ExitTarget {
                    screen,
                    destination: number,
                    secondary: false,
                    water: false,
                }
                .exit(number, false);
                let index = level.layer1.len();
                *edit = Some((
                    "Add screen exit".to_string(),
                    vec![Edit::InsertObject {
                        layer: ObjectLayer::One,
                        index,
                        object: Object::ScreenExit(exit),
                    }],
                ));
                open.selection = vec![Item::object(ObjectLayer::One, index)];
                ui.close();
            }
            ui.separator();
        }
    }
    if open.selection.is_empty() {
        ui.label("Nothing selected");
        return;
    }
    let selection = open.selection.clone();
    if let [item] = selection[..] {
        let entry = match item {
            Item::Object(o) => kobo_core::edit::find::Entry::Object(o.layer, o.index),
            Item::Sprite(i) => kobo_core::edit::find::Entry::Sprite(i),
        };
        if let Some(query) = kobo_core::edit::find::query_for(open.document.level(), entry) {
            if ui.button("Select every one like it").clicked() {
                let found = kobo_core::edit::find::find_in(number, open.document.level(), &query);
                open.selection = found
                    .iter()
                    .map(|f| match f.entry {
                        kobo_core::edit::find::Entry::Object(layer, index) => {
                            Item::object(layer, index)
                        }
                        kobo_core::edit::find::Entry::Sprite(index) => Item::Sprite(index),
                    })
                    .collect();
                ui.close();
            }
            if ui
                .button("Find every one in the project")
                .on_hover_text(format!("Find: {query}"))
                .clicked()
            {
                *find = Some(query);
                ui.close();
            }
        }
    }
    if ui.button("Delete").clicked() {
        *edit = Some((label_for(&selection, "Delete"), delete_edits(&selection)));
        open.selection.clear();
        ui.close();
    }
    if let [Item::Object(o)] = selection[..] {
        ui.separator();
        for (text, keys, order) in [
            ("Bring to front", "Ctrl+Shift+]", Order::Front),
            ("Bring forward", "Ctrl+]", Order::Forward),
            ("Send backward", "Ctrl+[", Order::Backward),
            ("Send to back", "Ctrl+Shift+[", Order::Back),
        ] {
            let change = reorder(open.document.level(), o, order);
            let button = egui::Button::new(text).shortcut_text(keys);
            if ui.add_enabled(change.is_some(), button).clicked()
                && let Some((label, reordering, item)) = change
            {
                *edit = Some((label.to_string(), vec![reordering]));
                open.selection = vec![item];
                ui.close();
            }
        }
    }
}

fn dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    painter.extend(Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
}

fn draw(
    painter: &egui::Painter,
    canvas: Rect,
    camera: &Camera,
    open: &OpenLevel,
    geometry: &Geometry,
    view: crate::app::View,
    hovered: Option<Item>,
) {
    let painter = painter.with_clip_rect(canvas);
    let seen = Rect::from_min_size(camera.offset.to_pos2(), canvas.size() / camera.zoom);
    if let Some(picture) = &open.picture {
        let source = seen.intersect(Rect::from_min_size(Pos2::ZERO, picture.size));
        if source.is_positive() {
            let target = camera.rect_to_screen(canvas, source);
            picture.draw(&painter, source, target, Color32::WHITE);
        }
    }
    let level_size = geometry.size();
    let level_rect = camera.rect_to_screen(canvas, Rect::from_min_size(Pos2::ZERO, level_size));
    painter.rect_stroke(
        level_rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Outside,
    );

    if view.grid && camera.zoom >= 1.0 {
        let stroke = Stroke::new(1.0, Color32::from_white_alpha(28));
        let first = (seen.min / TILE).floor() * TILE;
        let mut x = first.x.max(0.0);
        while x <= seen.max.x.min(level_size.x) {
            let sx = camera.to_screen(canvas, Pos2::new(x, 0.0)).x;
            painter.line_segment(
                [
                    Pos2::new(sx, level_rect.top()),
                    Pos2::new(sx, level_rect.bottom()),
                ],
                stroke,
            );
            x += TILE;
        }
        let mut y = first.y.max(0.0);
        while y <= seen.max.y.min(level_size.y) {
            let sy = camera.to_screen(canvas, Pos2::new(0.0, y)).y;
            painter.line_segment(
                [
                    Pos2::new(level_rect.left(), sy),
                    Pos2::new(level_rect.right(), sy),
                ],
                stroke,
            );
            y += TILE;
        }
    }

    if view.screens {
        let vertical = geometry.loaded.tiles.vertical;
        let stroke = Stroke::new(1.5, Color32::from_white_alpha(110));
        let span = if vertical { level_size.y } else { level_size.x };
        let mut at = 0.0;
        let mut screen = 0;
        while at < span {
            let (a, b, label_at) = if vertical {
                let y = camera.to_screen(canvas, Pos2::new(0.0, at)).y;
                (
                    Pos2::new(level_rect.left(), y),
                    Pos2::new(level_rect.right(), y),
                    Pos2::new(level_rect.left().max(canvas.left()) + 6.0, y + 6.0),
                )
            } else {
                let x = camera.to_screen(canvas, Pos2::new(at, 0.0)).x;
                (
                    Pos2::new(x, level_rect.top()),
                    Pos2::new(x, level_rect.bottom()),
                    Pos2::new(x + 6.0, level_rect.top().max(canvas.top()) + 6.0),
                )
            };
            if screen > 0 {
                dashed(&painter, a, b, stroke);
            }
            let text = format!("SCREEN {screen:02X}");
            let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), Color32::WHITE);
            let r = Rect::from_min_size(label_at, galley.size()).expand(3.0);
            painter.rect_filled(r, CornerRadius::same(3), Color32::from_black_alpha(170));
            painter.galley(label_at, galley, Color32::WHITE);
            at += 256.0;
            screen += 1;
        }
    }

    let moving: Option<(&[Item], (i32, i32))> = match (&open.drag, &open.pending) {
        (Some(Drag::Move { items, delta, .. }), _) => Some((items, *delta)),
        (_, Some(p)) => Some((&p.items, p.delta)),
        _ => None,
    };

    // The selection: each tile an object drew, and its box.
    for &item in &open.selection {
        // A finished move's items have their new numbers already, which
        // the old picture does not know; its outlines are drawn below.
        if open.pending.is_some() || moving.is_some_and(|(items, _)| items.contains(&item)) {
            continue;
        }
        outline(&painter, canvas, camera, geometry, item, (0, 0), true);
    }
    if let Some(item) = hovered
        && !open.selection.contains(&item)
        && open.drag.is_none()
        && let Some(bounds) = geometry.bounds(item)
    {
        painter.rect_stroke(
            camera.rect_to_screen(canvas, bounds),
            CornerRadius::same(2),
            Stroke::new(1.0, Color32::from_white_alpha(200)),
            StrokeKind::Outside,
        );
    }

    // What is being moved, drawn where it goes over the picture it left.
    if let Some((items, delta)) = moving
        && let Some(picture) = &open.picture
    {
        let shift = Vec2::new(delta.0 as f32 * TILE, delta.1 as f32 * TILE);
        let pieces: Vec<Rect> = items
            .iter()
            .flat_map(|&item| match item {
                Item::Object(o) => geometry.object_tiles(o),
                Item::Sprite(_) => geometry.bounds(item).into_iter().collect(),
            })
            .collect();
        for r in &pieces {
            painter.rect_filled(
                camera.rect_to_screen(canvas, *r),
                CornerRadius::ZERO,
                Color32::from_black_alpha(150),
            );
        }
        for r in &pieces {
            let target = camera.rect_to_screen(canvas, r.translate(shift));
            picture.draw(&painter, *r, target, Color32::from_white_alpha(235));
        }
        for &item in items {
            outline(&painter, canvas, camera, geometry, item, delta, false);
        }
    }

    if let Some((_, bounds)) = resizable(open, geometry)
        && open.pending.is_none()
    {
        let shown = match &open.drag {
            Some(Drag::Resize { delta, .. }) => {
                let max = bounds.max + Vec2::new(delta.0 as f32, delta.1 as f32) * TILE;
                let r = Rect::from_min_max(bounds.min, max.max(bounds.min + Vec2::splat(TILE)));
                painter.rect_stroke(
                    camera.rect_to_screen(canvas, r),
                    CornerRadius::same(2),
                    Stroke::new(2.0, theme::ACCENT),
                    StrokeKind::Outside,
                );
                r
            }
            _ => bounds,
        };
        let handle = handle_rect(camera, canvas, shown);
        painter.rect(
            handle,
            CornerRadius::same(2),
            Color32::WHITE,
            Stroke::new(2.0, theme::SELECTION),
            StrokeKind::Inside,
        );
    }

    if view.entrances {
        draw_entries(&painter, canvas, camera, &open.entries);
    }
    // A screen exit being taken to another screen: that screen, marked.
    if let Some(Drag::Exit { index, screen }) = &open.drag {
        let size = geometry.size();
        let start = f32::from(*screen) * 256.0;
        let column = if geometry.loaded.tiles.vertical {
            Rect::from_min_size(Pos2::new(0.0, start), Vec2::new(size.x, 256.0))
        } else {
            Rect::from_min_size(Pos2::new(start, 0.0), Vec2::new(256.0, size.y))
        };
        let r = camera.rect_to_screen(canvas, column);
        painter.rect(
            r,
            CornerRadius::ZERO,
            theme::ACCENT.gamma_multiply(0.12),
            Stroke::new(2.0, theme::ACCENT),
            StrokeKind::Inside,
        );
        let taken = exits(open.document.level()).any(|(i, e)| i != *index && e.screen == *screen);
        let text = if taken {
            format!("Screen {screen:02X} has an exit")
        } else {
            format!("To screen {screen:02X}")
        };
        painter.text(
            r.center_top() + Vec2::new(0.0, 48.0),
            Align2::CENTER_TOP,
            text,
            FontId::proportional(14.0),
            theme::ACCENT,
        );
    }
    // Sprites the game's loader never reaches, marked where they are.
    for i in edit::unreached_sprites(open.document.level()) {
        if let Some(bounds) = geometry.bounds(Item::Sprite(i)) {
            let r = camera.rect_to_screen(canvas, bounds);
            painter.rect_stroke(
                r,
                CornerRadius::same(2),
                Stroke::new(2.0, theme::WARNING),
                StrokeKind::Outside,
            );
            let badge = Pos2::new(r.right(), r.top());
            painter.circle_filled(badge, 7.0, theme::WARNING);
            painter.text(
                badge,
                Align2::CENTER_CENTER,
                "!",
                FontId::monospace(11.0),
                Color32::BLACK,
            );
        }
    }
    for (x, y, _) in &open.failed_sprites {
        let tile = camera.rect_to_screen(canvas, selection::tile_rect(*x, *y));
        painter.rect_stroke(
            tile,
            CornerRadius::same(2),
            Stroke::new(2.0, theme::ERROR),
            StrokeKind::Outside,
        );
        let badge = Pos2::new(tile.right(), tile.top());
        painter.circle_filled(badge, 7.0, theme::ERROR);
        painter.text(
            badge,
            Align2::CENTER_CENTER,
            "!",
            FontId::monospace(11.0),
            Color32::WHITE,
        );
    }
    if view.changes
        && let Some(head) = &open.head
    {
        crate::changes::draw(&painter, head, open.document.level(), geometry, |r| {
            camera.rect_to_screen(canvas, r)
        });
    }

    if let Some(Drag::Marquee { from, to, .. }) = &open.drag {
        let r = camera.rect_to_screen(canvas, Rect::from_two_pos(*from, *to));
        painter.rect(
            r,
            CornerRadius::ZERO,
            theme::SELECTION.gamma_multiply(0.15),
            Stroke::new(1.0, theme::SELECTION),
            StrokeKind::Inside,
        );
    }

    // A label on a single selection.
    if let [item] = open.selection[..]
        && let Some(bounds) = geometry.bounds(item)
    {
        let delta = moving
            .filter(|(items, _)| items.contains(&item))
            .map_or((0, 0), |(_, d)| d);
        let shift = Vec2::new(delta.0 as f32 * TILE, delta.1 as f32 * TILE);
        let level = open.document.level();
        let name = selection::describe(level, item);
        let place = match item {
            Item::Object(o) => objects(level, o.layer)
                .and_then(|l| l.get(o.index))
                .and_then(edit::object_position),
            Item::Sprite(i) => level.sprites.list.get(i).map(|s| (s.x, s.y)),
        };
        let text = match place {
            Some((x, y)) => format!("{name} · ({x}, {y})"),
            None => name,
        };
        let at = camera.to_screen(canvas, bounds.min + shift) - Vec2::new(0.0, 20.0);
        let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), theme::ON_SELECTION);
        let r = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(5.0, 2.0));
        painter.rect_filled(r, CornerRadius::same(3), crate::app::color_for(item));
        painter.galley(at, galley, theme::ON_SELECTION);
    }
}

/// How an entrance's settings place the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Placement {
    /// Position method 1, the game's tables: a screen, an X setting, and a
    /// Y setting.
    Tables(u8, u8, u8),
    /// Position method 2: a screen and a tile.
    Tiles(u8, u16, u16),
    /// The game's own midway entrance: the main entrance's place on a
    /// screen.
    Screen(u8),
}

impl Placement {
    /// Where the player stands, in level pixels (a screen's left edge, for
    /// `Screen`).
    fn position(self, tables: MainEntranceTables, vertical: bool) -> (i32, i32) {
        let (x, y) = match self {
            Self::Tables(screen, x, y) => tables.position(screen, x, y, vertical),
            Self::Tiles(screen, x, y) => entrance::tile_place(screen, x, y, vertical),
            Self::Screen(screen) => (u32::from(screen) * 256, 0),
        };
        (x as i32, y as i32)
    }

    /// The placement of the same kind nearest (`px`, `py`).
    fn nearest(self, tables: MainEntranceTables, (px, py): (i32, i32), vertical: bool) -> Self {
        match self {
            Self::Tables(..) => {
                let (screen, x, y) = tables.nearest(px, py, vertical);
                Self::Tables(screen, x, y)
            }
            Self::Tiles(..) => {
                let (screen, x, y) = entrance::nearest_tile(px, py, vertical);
                Self::Tiles(screen, x, y)
            }
            // Screen 0 is no midway point at all.
            Self::Screen(_) => Self::Screen((px + 128).div_euclid(256).clamp(1, 31) as u8),
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Tables(screen, x, y) => format!("screen {screen:02X}, X {x}, Y {y}"),
            Self::Tiles(screen, x, y) => format!("screen {screen:02X}, tile {x}, {y}"),
            Self::Screen(screen) => format!("screen {screen:02X}"),
        }
    }
}

/// The midway screen, 0 to 31.
fn midway_screen(level: &Level) -> u8 {
    level.entrance.midway_screen
        | if level.settings.midway.screen_high {
            0x10
        } else {
            0
        }
}

/// How an entrance's settings place the player, for one the canvas can
/// move.
pub(crate) fn entry_placement(level: &Level, kind: crate::preview::EntryKind) -> Option<Placement> {
    use crate::preview::EntryKind;
    let tiles = |screen, x: u8, y: u8, (x_high, y_high): (u8, u8)| {
        Placement::Tiles(
            screen,
            u16::from(x_high) << 3 | u16::from(x & 7),
            u16::from(y_high) << 4 | u16::from(y & 15),
        )
    };
    match kind {
        EntryKind::Main => {
            let e = level.entrance;
            Some(match level.settings.tile_position {
                Some(high) => tiles(e.entrance_screen, e.entrance_x, e.entrance_y, high),
                None => Placement::Tables(e.entrance_screen, e.entrance_x, e.entrance_y),
            })
        }
        EntryKind::Secondary(id) => {
            let e = level.entrances.iter().find(|e| e.id == id)?;
            Some(match e.settings.tile_position {
                Some(high) => tiles(e.screen, e.x, e.y, high),
                None => Placement::Tables(e.screen, e.x, e.y),
            })
        }
        EntryKind::Midway => match level.settings.midway.separate {
            None => Some(Placement::Screen(midway_screen(level))),
            Some(SeparateMidway::Entrance(m)) => {
                Some(Placement::Tiles(midway_screen(level), u16::from(m.x), m.y))
            }
            Some(SeparateMidway::Redirect(_)) => None,
        },
    }
}

/// The edits that give an entrance a new placement, and their label.
pub(crate) fn move_entry(
    level: &Level,
    kind: crate::preview::EntryKind,
    placement: Placement,
) -> Option<(String, Vec<Edit>)> {
    use crate::preview::EntryKind;
    if entry_placement(level, kind) == Some(placement) {
        return None;
    }
    // The settings' X, Y, and method 2's high bits, for a placement.
    let split = |x: u16, y: u16| {
        (
            (x & 7) as u8,
            (y & 15) as u8,
            ((x >> 3) as u8, (y >> 4) as u8),
        )
    };
    let mut edits = Vec::new();
    let mut header = level.entrance;
    let mut settings = level.settings;
    let label = match (kind, placement) {
        (EntryKind::Main, Placement::Tables(screen, x, y)) => {
            (header.entrance_screen, header.entrance_x, header.entrance_y) = (screen, x, y);
            "Move the start".to_string()
        }
        (EntryKind::Main, Placement::Tiles(screen, x, y)) => {
            let (x, y, high) = split(x, y);
            (header.entrance_screen, header.entrance_x, header.entrance_y) = (screen, x, y);
            settings.tile_position = Some(high);
            "Move the start".to_string()
        }
        (EntryKind::Secondary(id), Placement::Tables(..) | Placement::Tiles(..)) => {
            let index = level.entrances.iter().position(|e| e.id == id)?;
            let mut entrance = level.entrances[index];
            match placement {
                Placement::Tiles(screen, x, y) => {
                    let (x, y, high) = split(x, y);
                    (entrance.screen, entrance.x, entrance.y) = (screen, x, y);
                    entrance.settings.tile_position = Some(high);
                }
                Placement::Tables(screen, x, y) => {
                    (entrance.screen, entrance.x, entrance.y) = (screen, x, y);
                }
                Placement::Screen(_) => return None,
            }
            edits.push(Edit::ReplaceEntrance { index, entrance });
            format!("Move entrance {id:03X}")
        }
        (EntryKind::Midway, Placement::Screen(screen) | Placement::Tiles(screen, ..)) => {
            header.midway_screen = screen & 0x0F;
            settings.midway.screen_high = screen & 0x10 != 0;
            if let (Placement::Tiles(_, x, y), Some(SeparateMidway::Entrance(m))) =
                (placement, &mut settings.midway.separate)
            {
                (m.x, m.y) = (x as u8, y);
            }
            "Move the midway entrance".to_string()
        }
        _ => return None,
    };
    if header != level.entrance {
        edits.push(Edit::SetEntrance(header));
    }
    if settings != level.settings {
        edits.push(Edit::SetSettings(settings));
    }
    (!edits.is_empty()).then_some((label, edits))
}

/// Where the start marker can be taken hold of, in level pixels: the flag
/// and the player under it.
fn start_rect(entry: &crate::preview::Entry) -> Rect {
    Rect::from_min_max(
        Pos2::new(entry.x as f32 - 4.0, entry.y as f32 - 24.0),
        Pos2::new(entry.x as f32 + 40.0, entry.y as f32 + 32.0),
    )
}

/// A flag where the player enters each way, labelled.
fn draw_entries(
    painter: &egui::Painter,
    canvas: Rect,
    camera: &Camera,
    entries: &[crate::preview::Entry],
) {
    use crate::preview::EntryKind;
    for entry in entries {
        let (text, color) = match entry.kind {
            EntryKind::Main => ("START".to_string(), theme::OK),
            EntryKind::Midway => ("MIDWAY".to_string(), theme::ACCENT),
            EntryKind::Secondary(id) => (format!("ENTRANCE {id:03X}"), theme::HEADER),
        };
        // The player's feet are 32 pixels below his position.
        let foot = camera.to_screen(
            canvas,
            Pos2::new(entry.x as f32 + 8.0, entry.y as f32 + 32.0),
        );
        let top = camera.to_screen(
            canvas,
            Pos2::new(entry.x as f32 + 8.0, entry.y as f32 - 8.0),
        );
        painter.line_segment([foot, top], Stroke::new(2.0, color));
        painter.circle_filled(foot, 3.0, color);
        let galley = painter.layout_no_wrap(text, FontId::monospace(10.5), theme::ON_SELECTION);
        let at = top + Vec2::new(0.0, -galley.size().y - 2.0);
        let r = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(4.0, 1.0));
        painter.rect_filled(r, CornerRadius::same(3), color);
        painter.galley(at, galley, theme::ON_SELECTION);
    }
}

/// An item's tiles and box, `delta` tiles from where it is.
fn outline(
    painter: &egui::Painter,
    canvas: Rect,
    camera: &Camera,
    geometry: &Geometry,
    item: Item,
    delta: (i32, i32),
    fill: bool,
) {
    let shift = Vec2::new(delta.0 as f32 * TILE, delta.1 as f32 * TILE);
    let color = crate::app::color_for(item);
    if fill && let Item::Object(o) = item {
        for r in geometry.object_tiles(o) {
            painter.rect_filled(
                camera.rect_to_screen(canvas, r.translate(shift)),
                CornerRadius::ZERO,
                color.gamma_multiply(0.18),
            );
        }
    }
    if let Some(bounds) = geometry.bounds(item) {
        painter.rect_stroke(
            camera.rect_to_screen(canvas, bounds.translate(shift)),
            CornerRadius::same(2),
            Stroke::new(2.0, color),
            StrokeKind::Outside,
        );
    }
}
