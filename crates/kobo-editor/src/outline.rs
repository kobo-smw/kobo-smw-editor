//! The outline (the Objects tab): every object and sprite of the level,
//! along the level as the player meets them, screen by screen, or in data
//! order, which is drawing order. It reaches what the canvas cannot
//! click: a screen exit, Lunar Magic's settings objects, an object hidden
//! behind another.

use eframe::egui::{self, Align, RichText, TextStyle};
use kobo_core::edit::{self, ExitTarget, ObjectLayer};
use kobo_core::level::objects::Object;
use kobo_core::names;
use kobo_core::source::level::{Layer2, Level};

use crate::app::App;
use crate::preview::EntryKind;
use crate::selection::Item;
use crate::theme;

/// What the outline shows, and how.
#[derive(Default)]
pub struct OutlineState {
    pub filter: String,
    /// In drawing order rather than along the level.
    pub drawing_order: bool,
    pub show: Show,
}

/// Which entries the outline lists.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Show {
    #[default]
    All,
    Objects,
    Sprites,
}

/// One line of the outline.
enum Row {
    Heading(String),
    Item(Item, String, String),
}

/// What an object is and where, for its line.
fn object_line(level: &Level, number: u16, object: &Object) -> (String, String) {
    let tileset = level.header.object_tileset;
    let name = names::object(object, tileset)
        .unwrap_or("Object")
        .to_string();
    let place = match object {
        Object::ScreenExit(exit) => {
            let target = ExitTarget::of(*exit, number);
            let to = if target.secondary {
                "entrance"
            } else {
                "level"
            };
            format!(
                "screen {:02X} → {to} {:03X}",
                exit.screen, target.destination
            )
        }
        _ => match edit::object_position(object) {
            Some((x, y)) => format!("{x}, {y}"),
            None => "no place".to_string(),
        },
    };
    let id = match object {
        Object::Standard { number, .. } => format!("{number:02X} "),
        Object::Extended { number, .. } => format!("E{number:02X} "),
        Object::Lunar { number, .. } => format!("L{number:02X} "),
        Object::ScreenExit(_) | Object::Unplaced(_) => String::new(),
    };
    (format!("{id}{name}"), place)
}

/// Every entry in data order, by list.
fn drawing_rows(level: &Level, number: u16, show: Show) -> Vec<Row> {
    let mut rows = Vec::new();
    if show != Show::Sprites {
        rows.push(Row::Heading(format!("Layer 1 · {}", level.layer1.len())));
        for (i, object) in level.layer1.iter().enumerate() {
            let (name, place) = object_line(level, number, object);
            rows.push(Row::Item(Item::object(ObjectLayer::One, i), name, place));
        }
        if let Layer2::Objects(list) = &level.layer2 {
            rows.push(Row::Heading(format!("Layer 2 · {}", list.len())));
            for (i, object) in list.iter().enumerate() {
                let (name, place) = object_line(level, number, object);
                rows.push(Row::Item(Item::object(ObjectLayer::Two, i), name, place));
            }
        }
    }
    if show != Show::Objects {
        rows.push(Row::Heading(format!(
            "Sprites · {}",
            level.sprites.list.len()
        )));
        for (i, sprite) in level.sprites.list.iter().enumerate() {
            rows.push(Row::Item(
                Item::Sprite(i),
                sprite_name(sprite.id),
                format!("{}, {}", sprite.x, sprite.y),
            ));
        }
    }
    rows
}

fn sprite_name(id: u8) -> String {
    format!("{id:02X} {}", names::sprite(id))
}

/// Where an entry is along the level: its screen, and its place along
/// and across.
type Place = (u16, i32, i32);

/// Every entry along the level, as the player meets it: by column (in a
/// vertical level by row, from where the player starts), under a heading
/// for each screen. What has no place comes last.
fn along_rows(level: &Level, number: u16, show: Show, upward: bool) -> Vec<Row> {
    let vertical = level.header.level_mode.layer1_vertical();
    let mut entries: Vec<(Option<Place>, Row)> = Vec::new();
    let along = |x: u16, y: u16| {
        if vertical {
            let y = if upward { -i32::from(y) } else { i32::from(y) };
            (y, i32::from(x))
        } else {
            (i32::from(x), i32::from(y))
        }
    };
    let screen_of = |x: u16, y: u16| if vertical { y / 16 } else { x / 16 };
    if show != Show::Sprites {
        let layer2: &[Object] = match &level.layer2 {
            Layer2::Objects(list) => list,
            _ => &[],
        };
        for (layer, list) in [
            (ObjectLayer::One, &level.layer1[..]),
            (ObjectLayer::Two, layer2),
        ] {
            for (i, object) in list.iter().enumerate() {
                let (name, place) = object_line(level, number, object);
                let at = match object {
                    Object::ScreenExit(exit) => {
                        let start = u16::from(exit.screen) * 16;
                        let (x, y) = if vertical { (0, start) } else { (start, 0) };
                        let (a, b) = along(x, y);
                        Some((u16::from(exit.screen), a, b))
                    }
                    _ => edit::object_position(object).map(|(x, y)| {
                        let (a, b) = along(x, y);
                        (screen_of(x, y), a, b)
                    }),
                };
                entries.push((at, Row::Item(Item::object(layer, i), name, place)));
            }
        }
    }
    if show != Show::Objects {
        for (i, sprite) in level.sprites.list.iter().enumerate() {
            let (x, y) = (sprite.x, sprite.y);
            let (a, b) = along(x, y);
            entries.push((
                Some((screen_of(x, y), a, b)),
                Row::Item(Item::Sprite(i), sprite_name(sprite.id), format!("{x}, {y}")),
            ));
        }
    }
    entries.sort_by_key(|(at, _)| match at {
        Some((_, a, b)) => (0, *a, *b),
        None => (1, 0, 0),
    });
    let mut rows = Vec::new();
    let mut screen = None;
    for (at, row) in entries {
        let heading = match at {
            Some((s, ..)) => Some(s),
            None => Some(u16::MAX),
        };
        if heading != screen {
            screen = heading;
            rows.push(Row::Heading(match at {
                Some((s, ..)) => format!("Screen {s:02X}"),
                None => "No place".to_string(),
            }));
        }
        rows.push(row);
    }
    rows
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(number) = app.current_number() else {
        return;
    };
    let shift = ui.input(|i| i.modifiers.shift);
    ui.add(
        egui::TextEdit::singleline(&mut app.outline.filter)
            .hint_text("Find by name, number, or place")
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        let state = &mut app.outline;
        ui.selectable_value(&mut state.show, Show::All, "All");
        ui.selectable_value(&mut state.show, Show::Objects, "Objects");
        ui.selectable_value(&mut state.show, Show::Sprites, "Sprites");
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.selectable_value(&mut state.drawing_order, true, "Drawing order")
                .on_hover_text("In the level file's order, which is the order they draw in: later ones draw over earlier ones. Ctrl+[ and Ctrl+] move the selected object back and forth.");
            ui.selectable_value(&mut state.drawing_order, false, "Along")
                .on_hover_text("Along the level, as the player meets them, screen by screen");
        });
    });
    let filter = app.outline.filter.trim().to_lowercase();
    let (show, drawing_order) = (app.outline.show, app.outline.drawing_order);
    let Some(open) = app.open_mut(number) else {
        return;
    };
    // A vertical level is read from where the player starts: from the
    // bottom up when that is in its lower half.
    let upward = match (
        &open.geometry,
        open.entries.iter().find(|e| e.kind == EntryKind::Main),
    ) {
        (Some(geometry), Some(start)) => start.y as f32 > geometry.size().y / 2.0,
        _ => true,
    };
    let level = open.document.level();
    let rows = if drawing_order {
        drawing_rows(level, number, show)
    } else {
        along_rows(level, number, show, upward)
    };
    let rows: Vec<Row> = rows
        .into_iter()
        .filter(|row| match row {
            Row::Heading(_) => filter.is_empty(),
            Row::Item(item, name, place) => {
                let index = match item {
                    Item::Object(o) => o.index,
                    Item::Sprite(i) => *i,
                };
                filter.is_empty()
                    || name.to_lowercase().contains(&filter)
                    || place.to_lowercase().contains(&filter)
                    || index.to_string() == filter
            }
        })
        .collect();
    let first = open.selection.first().copied();
    let scroll_to = (first != open.outline_scrolled_to)
        .then(|| {
            first.and_then(|item| {
                rows.iter()
                    .position(|r| matches!(r, Row::Item(i, ..) if *i == item))
            })
        })
        .flatten();
    open.outline_scrolled_to = first;

    let height = ui.text_style_height(&TextStyle::Body) + 6.0;
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("outline")
        .auto_shrink([false, false]);
    if let Some(row) = scroll_to {
        // Put the row a third of the way down, so what comes before shows.
        let offset = (row as f32 * height - ui.available_height() / 3.0).max(0.0);
        scroll = scroll.vertical_scroll_offset(offset);
    }
    let mut clicked = None;
    scroll.show_rows(ui, height, rows.len(), |ui, range| {
        ui.with_layout(egui::Layout::top_down_justified(Align::Min), |ui| {
            for row in &rows[range] {
                match row {
                    Row::Heading(text) => {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            rect.left_bottom() + egui::vec2(4.0, -4.0),
                            egui::Align2::LEFT_BOTTOM,
                            text.to_uppercase(),
                            egui::FontId::proportional(10.5),
                            theme::MUTED,
                        );
                    }
                    Row::Item(item, name, place) => {
                        let selected = open.selection.contains(item);
                        let index = match item {
                            Item::Object(o) => o.index,
                            Item::Sprite(i) => *i,
                        };
                        let mut job = egui::text::LayoutJob::default();
                        let mono = egui::FontId::monospace(11.5);
                        let body = egui::FontId::proportional(13.0);
                        // Objects and sprites in the canvas's colours.
                        job.append(
                            "■ ",
                            0.0,
                            egui::TextFormat::simple(
                                egui::FontId::proportional(9.0),
                                crate::app::color_for(*item),
                            ),
                        );
                        job.append(
                            &format!("{index:>3} "),
                            0.0,
                            egui::TextFormat::simple(mono, theme::MUTED),
                        );
                        job.append(name, 0.0, egui::TextFormat::simple(body, theme::TEXT));
                        let response = ui.add(
                            egui::Button::selectable(selected, job)
                                .right_text(RichText::new(place).size(12.0).color(theme::MUTED))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .min_size(egui::vec2(ui.available_width(), height)),
                        );
                        if response.clicked() {
                            clicked = Some(*item);
                        }
                    }
                }
            }
        });
    });
    if let Some(item) = clicked {
        if shift {
            match open.selection.iter().position(|&s| s == item) {
                Some(i) => {
                    open.selection.remove(i);
                }
                None => open.selection.push(item),
            }
        } else {
            open.selection = vec![item];
        }
        // The list stays where it is; the canvas comes to what was chosen.
        open.outline_scrolled_to = open.selection.first().copied();
        open.focus = true;
    }
}
