//! The overview: every level of the project as a picture card
//! (`thumbnails`), around where the player starts. A card opens its
//! level.

use eframe::egui::{self, RichText};
use kobo_core::names;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct Overview {
    pub open: bool,
    filter: String,
}

/// The overview, in place of the canvas.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(workspace) = app.workspace() else {
        return;
    };
    let levels: Vec<u16> = workspace.levels().collect();
    let total = levels.len();
    for &number in &levels {
        app.thumbnails.want(number);
    }
    let drawn = levels.iter().filter(|&&n| app.thumbnails.done(n)).count();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new("All levels").size(18.0).strong());
        ui.label(RichText::new(format!("{total} in the project")).color(theme::MUTED));
        if drawn < total {
            ui.spinner();
            ui.label(RichText::new(format!("drawing {drawn} of {total}")).color(theme::MUTED));
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.overview.filter)
                .hint_text("Find: 105, castle, rope…")
                .desired_width(200.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Back to the level").clicked() {
                app.overview.open = false;
            }
        });
    });
    let filter = app.overview.filter.trim().to_lowercase();
    let mut open_level = None;
    let mut play = None;
    let card_width = 300.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let columns = ((ui.available_width() - 24.0) / (card_width + 12.0))
                .floor()
                .max(1.0) as usize;
            let workspace = app.workspace().expect("checked");
            let cards: Vec<u16> = levels
                .iter()
                .copied()
                .filter(|&n| {
                    let level = workspace.level(n);
                    let tileset = level
                        .and_then(|l| names::object_tileset(l.header.object_tileset))
                        .unwrap_or_default()
                        .to_lowercase();
                    filter.is_empty()
                        || format!("{n:03x}").contains(&filter)
                        || tileset.contains(&filter)
                        || app
                            .level_name(n)
                            .is_some_and(|name| name.to_lowercase().contains(&filter))
                })
                .collect();
            for row in cards.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    for &number in row {
                        let level = workspace.level(number);
                        let modified = app.is_modified(number);
                        let frame = egui::Frame::NONE
                            .fill(theme::PANEL_RAISED)
                            .corner_radius(8)
                            .stroke(egui::Stroke::new(
                                1.0,
                                if app.current_number() == Some(number) {
                                    theme::SELECTION
                                } else {
                                    theme::LINE
                                },
                            ))
                            .inner_margin(0);
                        let response = frame
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(card_width);
                                    let height = 100.0;
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(card_width, height),
                                        egui::Sense::hover(),
                                    );
                                    match (
                                        app.thumbnails.picture(number),
                                        app.thumbnails.failed(number),
                                    ) {
                                        (Some(texture), _) => {
                                            crate::thumbnails::paint_cover(
                                                ui.painter(),
                                                texture,
                                                rect,
                                            );
                                        }
                                        (None, Some(_)) => {
                                            ui.painter().rect_filled(rect, 0, theme::BACKGROUND);
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "does not draw",
                                                egui::FontId::proportional(13.0),
                                                theme::ERROR,
                                            );
                                        }
                                        (None, None) => {
                                            ui.painter().rect_filled(rect, 0, theme::BACKGROUND);
                                        }
                                    }
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(format!("{number:03X}"))
                                                .monospace()
                                                .strong()
                                                .color(theme::ACCENT),
                                        );
                                        if let Some(name) = app.level_name(number) {
                                            ui.label(RichText::new(name).color(theme::TEXT));
                                        } else if let Some(level) = level {
                                            let tileset =
                                                names::object_tileset(level.header.object_tileset)
                                                    .unwrap_or("?");
                                            ui.label(RichText::new(tileset).color(theme::TEXT));
                                        }
                                        if let Some(level) = level {
                                            let screens = match level.header.screens {
                                                1 => "1 screen".to_string(),
                                                n => format!("{n} screens"),
                                            };
                                            ui.label(
                                                RichText::new(screens).small().color(theme::MUTED),
                                            );
                                        }
                                        if modified {
                                            ui.label(
                                                RichText::new("● changed")
                                                    .small()
                                                    .color(theme::ACCENT),
                                            );
                                        }
                                    });
                                    ui.add_space(4.0);
                                })
                            })
                            .response
                            .interact(egui::Sense::click());
                        let response = match app.thumbnails.failed(number) {
                            Some(error) => response.on_hover_text(error),
                            None => response,
                        };
                        if response.clicked() {
                            open_level = Some(number);
                        }
                        response.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                open_level = Some(number);
                                ui.close();
                            }
                            ui.menu_button("Play from its start", |ui| {
                                for (i, name) in crate::play::POWERUPS.iter().enumerate() {
                                    if ui.button(*name).clicked() {
                                        play = Some((number, i as u8));
                                        ui.close();
                                    }
                                }
                            });
                        });
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.add_space(12.0);
                    }
                });
                ui.add_space(12.0);
            }
        });
    if let Some(number) = open_level {
        app.overview.open = false;
        app.open_level(number);
    }
    if let Some((number, powerup)) = play {
        crate::play::from_level_start(app, number, powerup);
    }
}
