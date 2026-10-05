//! Find: every object and sprite of the project's levels that a search
//! names (`edit::find`), by level; choosing one opens its level with it
//! selected.

use eframe::egui::{self, RichText, TextStyle};
use kobo_core::edit::find::{self, Entry, Found};

use crate::app::App;
use crate::selection::Item;
use crate::theme;

/// The most entries listed.
const MOST: usize = 2000;

#[derive(Default)]
pub struct FindState {
    pub query: String,
    /// What the last search found, and for which query; `None` once the
    /// project changed.
    found: Option<(String, Vec<Found>)>,
}

impl FindState {
    /// Forgets what was found, after the project changed.
    pub fn changed(&mut self) {
        self.found = None;
    }
}

enum Row<'a> {
    Level(u16, usize),
    Entry(&'a Found),
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.find.query)
            .hint_text("Find in every level: goomba, sprite 0F, exit…")
            .desired_width(f32::INFINITY),
    );
    if app.find.query.is_empty() && !field.has_focus() && app.find.found.is_none() {
        field.request_focus();
    }
    let query = app.find.query.trim().to_string();
    let stale = app.find.found.as_ref().is_none_or(|(q, _)| *q != query);
    if stale && let Some(workspace) = app.workspace() {
        let found = find::find(workspace, &query);
        app.find.found = Some((query.clone(), found));
    }
    let Some((_, found)) = &app.find.found else {
        return;
    };
    if query.is_empty() {
        ui.label(
            RichText::new("Objects and sprites of every level in the project, by name or number.")
                .small()
                .color(theme::MUTED),
        );
        return;
    }
    let levels = {
        let mut levels: Vec<u16> = found.iter().map(|f| f.level).collect();
        levels.dedup();
        levels.len()
    };
    let summary = match (found.len(), levels) {
        (0, _) => "Nothing found".to_string(),
        (1, _) => "1 found".to_string(),
        (n, 1) => format!("{n} found, in one level"),
        (n, l) => format!("{n} found, in {l} levels"),
    };
    ui.label(RichText::new(summary).small().color(theme::MUTED));
    let mut rows = Vec::new();
    for (i, f) in found.iter().take(MOST).enumerate() {
        if i == 0 || found[i - 1].level != f.level {
            let count = found.iter().filter(|g| g.level == f.level).count();
            rows.push(Row::Level(f.level, count));
        }
        rows.push(Row::Entry(f));
    }
    let height = ui.text_style_height(&TextStyle::Body) + 6.0;
    let current = app.current_number();
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, height, rows.len(), |ui, range| {
            for row in &rows[range] {
                match row {
                    Row::Level(level, count) => {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), height),
                            egui::Sense::hover(),
                        );
                        let color = if current == Some(*level) {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        };
                        ui.painter().text(
                            rect.left_bottom() + egui::vec2(4.0, -4.0),
                            egui::Align2::LEFT_BOTTOM,
                            format!("LEVEL {level:03X} · {count}"),
                            egui::FontId::proportional(10.5),
                            color,
                        );
                    }
                    Row::Entry(f) => {
                        let mut job = egui::text::LayoutJob::default();
                        job.append(
                            &format!("{} ", f.kind.rsplit(' ').next().unwrap_or_default()),
                            0.0,
                            egui::TextFormat::simple(egui::FontId::monospace(11.5), theme::MUTED),
                        );
                        job.append(
                            &f.name,
                            0.0,
                            egui::TextFormat::simple(egui::FontId::proportional(13.0), theme::TEXT),
                        );
                        let at = f.at.map_or_else(String::new, |(x, y)| format!("{x}, {y}"));
                        let response = ui.add(
                            egui::Button::selectable(false, job)
                                .right_text(RichText::new(at).size(12.0).color(theme::MUTED))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .min_size(egui::vec2(ui.available_width(), height)),
                        );
                        if response.clicked() {
                            chosen = Some((f.level, f.entry));
                        }
                    }
                }
            }
        });
    if found.len() > MOST {
        ui.label(
            RichText::new(format!("The first {MOST} shown"))
                .small()
                .color(theme::MUTED),
        );
    }
    if let Some((level, entry)) = chosen {
        app.open_level(level);
        if let Some(open) = app.current_mut()
            && open.number == level
        {
            open.selection = vec![match entry {
                Entry::Object(layer, index) => Item::object(layer, index),
                Entry::Sprite(index) => Item::Sprite(index),
            }];
            open.focus = true;
        }
    }
}
