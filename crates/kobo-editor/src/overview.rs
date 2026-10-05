//! The overview: every level of the project as a picture card, drawn one
//! after another on a worker thread from a build of the project, each
//! around where the player starts. A card opens its level.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use kobo_core::edit::Workspace;
use kobo_core::image::RgbImage;
use kobo_core::names;
use kobo_core::operation::Operation;
use kobo_core::render::{RenderOptions, Sprites};

use crate::app::App;
use crate::theme;

/// What a card shows of its level: three screens wide, the level's
/// height, around the start; then a third of the size.
const SHOWN: u32 = 768;
const SHRINK: u32 = 3;

/// A level's card picture, or why it does not draw.
type Drawn = (u16, Result<RgbImage, String>);

#[derive(Default)]
pub struct Overview {
    pub open: bool,
    pictures: HashMap<u16, egui::TextureHandle>,
    failed: HashMap<u16, String>,
    worker: Option<(Operation, Receiver<Drawn>)>,
    filter: String,
}

impl Overview {
    /// Forgets the pictures, after the project changed.
    pub fn forget(&mut self) {
        if let Some((operation, _)) = self.worker.take() {
            operation.cancel();
        }
        self.pictures.clear();
        self.failed.clear();
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    /// Forgets one level's picture, after it changed: the next look draws
    /// it again.
    pub fn changed(&mut self, number: u16) {
        self.pictures.remove(&number);
        self.failed.remove(&number);
    }
}

/// A third of `image`'s part around `x`: three screens of it, whole
/// pixels averaged in threes.
fn card_picture(image: &RgbImage, x: u32) -> RgbImage {
    let width = SHOWN.min(image.width);
    let left = x.saturating_sub(width / 3).min(image.width - width);
    let (w, h) = (width / SHRINK, image.height / SHRINK);
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..SHRINK {
                for dx in 0..SHRINK {
                    let p = image.pixels
                        [((y * SHRINK + dy) * image.width + left + x * SHRINK + dx) as usize];
                    for c in 0..3 {
                        sum[c] += u32::from(p[c]);
                    }
                }
            }
            let n = SHRINK * SHRINK;
            out.pixels[(y * w + x) as usize] = sum.map(|s| (s / n) as u8);
        }
    }
    out
}

/// Draws every level of `workspace` not drawn yet, one after another.
fn start(overview: &mut Overview, workspace: Workspace, ctx: egui::Context) {
    let levels: Vec<u16> = workspace
        .levels()
        .filter(|n| !overview.pictures.contains_key(n) && !overview.failed.contains_key(n))
        .collect();
    if levels.is_empty() {
        return;
    }
    let (send, receive) = mpsc::channel();
    let operation = Operation::default();
    let control = operation.clone();
    std::thread::spawn(move || {
        let (rom, left_out) = match workspace.build_leaving_out(None) {
            Ok(built) => built,
            Err(e) => {
                for number in levels {
                    let _ = send.send((number, Err(e.to_string())));
                }
                ctx.request_repaint();
                return;
            }
        };
        let options = RenderOptions {
            sprites: Sprites::Hidden,
            player: false,
            hidden_layers: 0,
        };
        for number in levels {
            if control.check().is_err() {
                break;
            }
            if let Some((_, why)) = left_out.iter().find(|(n, _)| *n == number) {
                let _ = send.send((number, Err(format!("does not build: {why}"))));
                continue;
            }
            let picture =
                kobo_core::render::render_level_with_control(&rom, number, options, &control)
                    .map(|render| {
                        let x = u32::from(render.level.ram.u16(kobo_core::ram::PLAYER_X));
                        card_picture(&render.image, x)
                    })
                    .map_err(|e| e.to_string());
            if send.send((number, picture)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    overview.worker = Some((operation, receive));
}

/// The overview, in place of the canvas.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    // Take what the worker drew.
    let mut finished = false;
    if let Some((_, receive)) = &app.overview.worker {
        loop {
            match receive.try_recv() {
                Ok((number, Ok(image))) => {
                    let size = [image.width as usize, image.height as usize];
                    let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
                    let texture = ctx.load_texture(
                        format!("overview-{number:03X}"),
                        egui::ColorImage::from_rgb(size, &rgb),
                        egui::TextureOptions::LINEAR,
                    );
                    app.overview.pictures.insert(number, texture);
                }
                Ok((number, Err(e))) => {
                    app.overview.failed.insert(number, e);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
    }
    if finished {
        app.overview.worker = None;
    }
    if app.overview.worker.is_none() {
        app.sync_levels();
        if let Some(workspace) = app.workspace().cloned() {
            start(&mut app.overview, workspace, ctx.clone());
        }
    }

    let Some(workspace) = app.workspace() else {
        return;
    };
    let levels: Vec<u16> = workspace.levels().collect();
    let total = levels.len();
    let drawn = app.overview.pictures.len() + app.overview.failed.len();
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
                                        app.overview.pictures.get(&number),
                                        app.overview.failed.get(&number),
                                    ) {
                                        (Some(texture), _) => {
                                            let size = texture.size_vec2();
                                            let scale = (card_width / size.x).max(height / size.y);
                                            let shown = size * scale;
                                            // Cover the card, keeping the ground: the bottom shows.
                                            let uv_w = (card_width / shown.x).min(1.0);
                                            let uv_h = (height / shown.y).min(1.0);
                                            let uv = egui::Rect::from_min_max(
                                                egui::pos2((1.0 - uv_w) / 2.0, 1.0 - uv_h),
                                                egui::pos2((1.0 + uv_w) / 2.0, 1.0),
                                            );
                                            ui.painter().image(
                                                texture.id(),
                                                rect,
                                                uv,
                                                egui::Color32::WHITE,
                                            );
                                        }
                                        (None, Some(error)) => {
                                            ui.painter().rect_filled(rect, 0, theme::BACKGROUND);
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "does not draw",
                                                egui::FontId::proportional(13.0),
                                                theme::ERROR,
                                            );
                                            let _ = error;
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
                        let response = match app.overview.failed.get(&number) {
                            Some(error) => response.on_hover_text(error),
                            None => response,
                        };
                        if response.clicked() {
                            open_level = Some(number);
                        }
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
}
