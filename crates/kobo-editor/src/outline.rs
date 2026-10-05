//! The outline: every object and sprite of the level in data order,
//! which is drawing order. It reaches what the canvas cannot click: a
//! screen exit, Lunar Magic's settings objects, an object hidden behind
//! another.

use eframe::egui::{self, Align, RichText, TextStyle};
use kobo_core::edit::{self, ExitTarget, ObjectLayer};
use kobo_core::level::objects::Object;
use kobo_core::names;
use kobo_core::source::level::{Layer2, Level};

use crate::app::App;
use crate::selection::Item;
use crate::theme;

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

fn rows(level: &Level, number: u16) -> Vec<Row> {
    let mut rows = vec![Row::Heading(format!("Layer 1 · {}", level.layer1.len()))];
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
    rows.push(Row::Heading(format!(
        "Sprites · {}",
        level.sprites.list.len()
    )));
    for (i, sprite) in level.sprites.list.iter().enumerate() {
        let name = format!("{:02X} {}", sprite.id, names::sprite(sprite.id));
        rows.push(Row::Item(
            Item::Sprite(i),
            name,
            format!("{}, {}", sprite.x, sprite.y),
        ));
    }
    rows
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(number) = app.current_number() else {
        return;
    };
    let shift = ui.input(|i| i.modifiers.shift);
    ui.add(
        egui::TextEdit::singleline(&mut app.outline_filter)
            .hint_text("Find by name, number, or place")
            .desired_width(f32::INFINITY),
    );
    let filter = app.outline_filter.trim().to_lowercase();
    let Some(open) = app.open_mut(number) else {
        return;
    };
    let rows: Vec<Row> = rows(open.document.level(), number)
        .into_iter()
        .filter(|row| match row {
            Row::Heading(_) => true,
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
    ui.label(
        RichText::new("In drawing order: later ones draw over earlier ones. Ctrl+[ and Ctrl+] move the selected object back and forth.")
            .small()
            .color(theme::MUTED),
    );
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
    let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
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
                        ui.add_sized(
                            [ui.available_width(), height],
                            egui::Label::new(
                                RichText::new(text.to_uppercase())
                                    .small()
                                    .color(theme::MUTED),
                            ),
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
                        job.append(
                            &format!("{index:>3} "),
                            0.0,
                            egui::TextFormat::simple(mono, theme::MUTED),
                        );
                        job.append(
                            name,
                            0.0,
                            egui::TextFormat::simple(body.clone(), theme::TEXT),
                        );
                        job.append(
                            &format!("  {place}"),
                            0.0,
                            egui::TextFormat::simple(body, theme::MUTED),
                        );
                        let response = ui.add_sized(
                            [ui.available_width(), height],
                            egui::Button::selectable(selected, job)
                                .wrap_mode(egui::TextWrapMode::Truncate),
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
