//! The shared palettes window: the game's colour tables
//! (`source::palettes::TABLES`), as the project changes them
//! (`edit::PalettesDocument`). Every level that has no palette of its own
//! draws from them, by its header's palette settings, which the window
//! marks for the open level.

use eframe::egui::{self, Color32, RichText, Sense};
use kobo_core::palette::Color15;
use kobo_core::source::palettes::{SharedTable, TABLES};

use crate::app::App;
use crate::theme;

/// The window's state.
#[derive(Default)]
pub struct PalettesEditor {
    pub open: bool,
    /// The table shown, by its index in `TABLES`.
    pub table: usize,
    /// The colour chosen, by its number among all of them.
    pub colour: Option<u16>,
}

fn color32(c: Color15) -> Color32 {
    let [r, g, b] = c.to_rgb8();
    Color32::from_rgb(r, g, b)
}

/// The table's name as a heading: `overworld_hud` as "Overworld hud".
fn title(table: &SharedTable) -> String {
    let words = table.name.replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.palettes_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Shared palettes")
        .open(&mut open)
        .default_width(640.0)
        .resizable(false)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.palettes_editor.open = false;
    }
}

/// A change the window asks for: the colour, its new value, and whether a
/// value is being dragged.
type Change = (u16, Color15, bool);

fn show(app: &mut App, ui: &mut egui::Ui) {
    if let Err(e) = app.palettes_document() {
        ui.label(RichText::new(e).color(theme::ERROR));
        return;
    }
    let mut state = std::mem::take(&mut app.palettes_editor);
    let change = contents(app, &mut state, ui);
    app.palettes_editor = state;
    if let Some((n, colour, dragging)) = change {
        app.set_shared_colour(n, colour, dragging);
    }
}

fn contents(app: &App, state: &mut PalettesEditor, ui: &mut egui::Ui) -> Option<Change> {
    let document = app.palettes()?;
    // The palettes the open level's header picks, to mark.
    let header = app.current().map(|o| o.document.level().header);
    let uses_own = app
        .current()
        .is_some_and(|o| o.document.level().palette.is_some());
    let mut change: Option<Change> = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(170.0);
            for (i, table) in TABLES.iter().enumerate() {
                let changed = (table.first..table.first + table.len)
                    .filter(|&n| document.is_changed(n))
                    .count();
                let text = if changed > 0 {
                    format!("{}  ·{changed}", title(table))
                } else {
                    title(table)
                };
                if ui
                    .add(egui::Button::selectable(state.table == i, text))
                    .on_hover_text(format!("{} at ${:06X}, {} colours", table.label, table.addr().raw(), table.len))
                    .clicked()
                {
                    state.table = i;
                    state.colour = None;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let table = &TABLES[state.table];
            ui.label(RichText::new(title(table)).size(16.0).strong());
            ui.label(
                RichText::new(format!("{} · ${:06X} · {} colours", table.label, table.addr().raw(), table.len))
                    .small()
                    .color(theme::MUTED),
            );
            if let Some(header) = header {
                let picked = match table.name {
                    "background" => Some(header.bg_palette),
                    "foreground" => Some(header.fg_palette),
                    "sprite" => Some(header.sprite_palette),
                    "back_area" => Some(header.back_area),
                    _ => None,
                };
                if let Some(picked) = picked {
                    let note = if uses_own {
                        format!("The open level's header picks {picked}, but it has a palette of its own.")
                    } else {
                        format!("The open level draws with {picked}, outlined.")
                    };
                    ui.label(RichText::new(note).small().color(theme::MUTED));
                }
            }
            ui.add_space(6.0);
            // One line per row of a palette, or 16 colours to a line.
            let per_line = table.layout.map_or(16, |(per_row, _, _)| per_row);
            let per_palette = table.layout.map_or(1, |(per_row, rows, _)| per_row * rows);
            let marked = header.and_then(|h| match table.name {
                "background" => Some(h.bg_palette),
                "foreground" => Some(h.fg_palette),
                "sprite" => Some(h.sprite_palette),
                "back_area" => Some(h.back_area),
                _ => None,
            });
            let cell = 22.0;
            for line in 0..table.len.div_ceil(per_line) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if let Some((_, rows, _)) = table.layout {
                        let label = format!("{}.{}", line / rows, line % rows);
                        ui.add_sized([28.0, cell], egui::Label::new(RichText::new(label).small().color(theme::MUTED)));
                    }
                    for i in line * per_line..((line + 1) * per_line).min(table.len) {
                        let n = table.first + i;
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(cell, cell), Sense::click());
                        ui.painter().rect_filled(rect, 2, color32(document.colour(n)));
                        let palette = i / per_palette;
                        if marked.is_some_and(|m| u16::from(m) == palette) {
                            ui.painter().rect_stroke(rect, 2, egui::Stroke::new(1.0, theme::MUTED), egui::StrokeKind::Outside);
                        }
                        if document.is_changed(n) {
                            ui.painter().circle_filled(rect.right_top() + egui::vec2(-4.0, 4.0), 2.5, theme::ACCENT);
                        }
                        if state.colour == Some(n) {
                            ui.painter().rect_stroke(rect, 2, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
                        }
                        let tip = match table.place(i) {
                            Some(place) => format!("Colour {i:02X}: {place}"),
                            None => format!("Colour {i:02X}"),
                        };
                        if response.on_hover_text(tip).clicked() {
                            state.colour = Some(n);
                        }
                    }
                });
            }
            ui.add_space(8.0);
            let Some(n) = state.colour.filter(|n| (table.first..table.first + table.len).contains(n)) else {
                ui.label(RichText::new("Choose a colour.").color(theme::MUTED));
                return;
            };
            let colour = document.colour(n);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), Sense::hover());
                ui.painter().rect_filled(rect, 4, color32(colour));
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "#{:02X}{:02X}{:02X}",
                            colour.r() * 8,
                            colour.g() * 8,
                            colour.b() * 8
                        ))
                        .monospace(),
                    );
                    ui.horizontal(|ui| {
                        let (mut r, mut g, mut b) = (colour.r(), colour.g(), colour.b());
                        let mut any = None;
                        for (name, value) in [("R", &mut r), ("G", &mut g), ("B", &mut b)] {
                            ui.label(name);
                            let response = ui.add(egui::DragValue::new(value).range(0..=31).speed(0.1));
                            if response.changed() {
                                any = Some(response.dragged() && !response.drag_started());
                            }
                        }
                        if let Some(dragging) = any {
                            change = Some((n, Color15::from_rgb5(r, g, b), dragging));
                        }
                    });
                });
            });
            let back = ui
                .add_enabled(document.is_changed(n), egui::Button::new("Back to the game's"))
                .on_disabled_hover_text("The project does not change this colour");
            if back.clicked() {
                change = Some((n, document.clean_colour(n), false));
            }
        });
    });
    change
}
