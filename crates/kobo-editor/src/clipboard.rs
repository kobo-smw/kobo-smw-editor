//! Copying, cutting, pasting, and duplicating objects and sprites, within
//! a level or from one to another.
//!
//! The editor keeps what was copied itself; the system clipboard gets a
//! line saying what, so that pasting (which comes as text from the
//! system) knows the copy is the editor's. Screen exits and objects with
//! no place are not copied: an exit belongs to its screen, and a second
//! one for the same screen would only shadow the first.

use eframe::egui;
use kobo_core::edit::{self, Edit, ObjectLayer};
use kobo_core::level::objects::Object;
use kobo_core::source::level::{Level, Sprite};

use crate::app::App;
use crate::selection::{Item, objects};

/// What was copied, with positions relative to the top left of it all.
#[derive(Clone, Default, Debug)]
pub struct Clipboard {
    objects: Vec<(ObjectLayer, Object, (u16, u16))>,
    sprites: Vec<(Sprite, (u16, u16))>,
    /// Where the top left of it was, for a paste with no place to go.
    origin: (u16, u16),
    /// What the system clipboard was given, to know a paste is ours.
    marker: String,
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.sprites.is_empty()
    }

    /// The selected objects and sprites of `level`. Returns how many
    /// items were left out (those with no place).
    fn take(level: &Level, selection: &[Item]) -> (Self, usize) {
        let mut placed_objects = Vec::new();
        let mut placed_sprites = Vec::new();
        let mut skipped = 0;
        // In list order, so that pasting keeps what draws over what.
        let mut items = selection.to_vec();
        items.sort_by_key(|item| match *item {
            Item::Object(o) => (0, o.layer == ObjectLayer::Two, o.index),
            Item::Sprite(i) => (1, false, i),
        });
        for item in items {
            match item {
                Item::Object(o) => {
                    let Some(object) = objects(level, o.layer).and_then(|l| l.get(o.index)) else {
                        continue;
                    };
                    match edit::object_position(object) {
                        Some(at) => placed_objects.push((o.layer, object.clone(), at)),
                        None => skipped += 1,
                    }
                }
                Item::Sprite(i) => {
                    if let Some(sprite) = level.sprites.list.get(i) {
                        placed_sprites.push((sprite.clone(), (sprite.x, sprite.y)));
                    }
                }
            }
        }
        let places = placed_objects
            .iter()
            .map(|(_, _, at)| *at)
            .chain(placed_sprites.iter().map(|(_, at)| *at));
        let origin = places.fold(None, |min: Option<(u16, u16)>, (x, y)| {
            Some(min.map_or((x, y), |(mx, my)| (mx.min(x), my.min(y))))
        });
        let origin = origin.unwrap_or((0, 0));
        let relative = |(x, y): (u16, u16)| (x - origin.0, y - origin.1);
        let clipboard = Self {
            objects: placed_objects
                .into_iter()
                .map(|(layer, object, at)| (layer, object, relative(at)))
                .collect(),
            sprites: placed_sprites
                .into_iter()
                .map(|(sprite, at)| (sprite, relative(at)))
                .collect(),
            origin,
            marker: String::new(),
        };
        (clipboard, skipped)
    }

    fn describe(&self) -> String {
        let count = |n: usize, one: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {one}s"),
        };
        match (self.objects.len(), self.sprites.len()) {
            (o, 0) => count(o, "object"),
            (0, s) => count(s, "sprite"),
            (o, s) => format!("{} and {}", count(o, "object"), count(s, "sprite")),
        }
    }

    /// The edits that put the copy into `level` with its top left at
    /// `at`, and the items it becomes. Objects go last in their lists, on
    /// the layer they came from if the level has it, else layer 1; sprites
    /// go where the loader reaches them.
    fn paste(&self, level: &Level, (ax, ay): (u16, u16)) -> (Vec<Edit>, Vec<Item>) {
        let mut scratch = level.clone();
        let mut comments = Default::default();
        let mut edits = Vec::new();
        let mut items = Vec::new();
        let has_layer2 = objects(level, ObjectLayer::Two).is_some();
        for (layer, object, (dx, dy)) in &self.objects {
            let layer = if has_layer2 { *layer } else { ObjectLayer::One };
            let index = objects(&scratch, layer).map_or(0, Vec::len);
            let edit = Edit::InsertObject {
                layer,
                index,
                object: edit::object_at(object, ax.saturating_add(*dx), ay.saturating_add(*dy)),
            };
            if edit.apply(&mut scratch, &mut comments).is_ok() {
                edits.push(edit);
                items.push(Item::object(layer, index));
            }
        }
        for (sprite, (dx, dy)) in &self.sprites {
            let placed = Sprite {
                x: ax.saturating_add(*dx),
                y: ay.saturating_add(*dy),
                ..sprite.clone()
            };
            let (edit, index) = edit::insert_sprite(&scratch, placed);
            if edit.apply(&mut scratch, &mut comments).is_ok() {
                // Sprites already pasted after this one move on by one.
                for item in &mut items {
                    if let Item::Sprite(i) = item
                        && *i >= index
                    {
                        *i += 1;
                    }
                }
                edits.push(edit);
                items.push(Item::Sprite(index));
            }
        }
        (edits, items)
    }
}

/// What the clipboard keys or the canvas's menu asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Copy,
    Cut,
    Paste,
    Duplicate,
}

/// Takes the clipboard's keys: Ctrl+C, Ctrl+X, Ctrl+V, and Ctrl+D.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    let mut actions = Vec::new();
    let mut pasted_text = None;
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Copy => {
                actions.push(Action::Copy);
                false
            }
            egui::Event::Cut => {
                actions.push(Action::Cut);
                false
            }
            egui::Event::Paste(text) => {
                pasted_text = Some(text.clone());
                actions.push(Action::Paste);
                false
            }
            _ => true,
        });
        if i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND,
            egui::Key::D,
        )) {
            actions.push(Action::Duplicate);
        }
    });
    for action in actions {
        run(app, ctx, action, pasted_text.as_deref(), None);
    }
}

/// Copies, cuts, duplicates, or pastes: a paste at tile `at` if given,
/// else where the mouse is. `pasted` is the system clipboard's text for
/// a paste from the keys, which must be the editor's own.
pub fn run(
    app: &mut App,
    ctx: &egui::Context,
    action: Action,
    pasted: Option<&str>,
    at: Option<(u16, u16)>,
) {
    let Some(open) = app.current() else { return };
    let level = open.document.level();
    let selection = open.selection.clone();
    match action {
        Action::Copy | Action::Cut | Action::Duplicate => {
            if selection.is_empty() {
                return;
            }
            let (mut clipboard, skipped) = Clipboard::take(level, &selection);
            if clipboard.is_empty() {
                app.say("Nothing to copy: screen exits and objects with no place are not copied.");
                return;
            }
            let what = clipboard.describe();
            if action == Action::Duplicate {
                let at = (clipboard.origin.0 + 1, clipboard.origin.1 + 1);
                paste_at(app, &clipboard, at, &format!("Duplicate {what}"));
                return;
            }
            clipboard.marker = format!("Kobo: {what}");
            ctx.copy_text(clipboard.marker.clone());
            let note = if skipped > 0 {
                format!(" ({skipped} with no place left out)")
            } else {
                String::new()
            };
            if action == Action::Cut {
                let label = format!("Cut {what}");
                if app.apply(&label, crate::canvas::delete_edits(&selection))
                    && let Some(open) = app.current_mut()
                {
                    open.selection.clear();
                }
                app.say(format!("Cut {what}{note}"));
            } else {
                app.say(format!("Copied {what}{note}"));
            }
            app.clipboard = clipboard;
        }
        Action::Paste => {
            let clipboard = app.clipboard.clone();
            // Text from elsewhere is not the editor's to paste.
            if clipboard.is_empty() || pasted.is_some_and(|t| t.trim() != clipboard.marker) {
                return;
            }
            let at = at
                .or_else(|| {
                    crate::canvas::last_hover(ctx)
                        .filter(|h| h.x >= 0 && h.y >= 0)
                        .map(|h| (h.x as u16, h.y as u16))
                })
                .unwrap_or((clipboard.origin.0 + 1, clipboard.origin.1 + 1));
            paste_at(
                app,
                &clipboard,
                at,
                &format!("Paste {}", clipboard.describe()),
            );
        }
    }
}

/// Copies `items` of the open level `delta` tiles away, as a drag with
/// Ctrl held drops them: the copies selected, the originals where they
/// were.
pub fn copy_by(app: &mut App, items: &[Item], (dx, dy): (i32, i32)) {
    let Some(open) = app.current() else { return };
    let (clipboard, _) = Clipboard::take(open.document.level(), items);
    if clipboard.is_empty() {
        return;
    }
    let shift = |v: u16, d: i32| (i32::from(v) + d).clamp(0, i32::from(u16::MAX)) as u16;
    let at = (shift(clipboard.origin.0, dx), shift(clipboard.origin.1, dy));
    let label = format!("Copy {}", clipboard.describe());
    paste_at(app, &clipboard, at, &label);
}

fn paste_at(app: &mut App, clipboard: &Clipboard, at: (u16, u16), label: &str) {
    let Some(open) = app.current() else { return };
    let (edits, items) = clipboard.paste(open.document.level(), at);
    if edits.is_empty() {
        app.say(format!("{label}: it does not fit there"));
        return;
    }
    if app.apply(label, edits)
        && let Some(open) = app.current_mut()
    {
        open.selection = items;
    }
}
