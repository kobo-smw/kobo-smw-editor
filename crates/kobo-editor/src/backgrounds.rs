//! Choosing layer 2's background, for a level mode that draws one: one of
//! the game's (`level::game_backgrounds`), or the one the level had of its
//! own, each pictured as the open level would draw it
//! (`edit::background_preview`), one after another on a worker thread.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use kobo_core::addr::SnesAddr;
use kobo_core::edit::{self, Edit, Workspace};
use kobo_core::image::RgbImage;
use kobo_core::level::Layer2Kind;
use kobo_core::operation::Operation;
use kobo_core::source::level::{Layer2, Level, Sprites};

use crate::app::App;
use crate::theme;

/// A background to choose.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Choice {
    /// One of the game's, by its address.
    Game(SnesAddr),
    /// The level's own, as it had it when the window opened.
    Own,
}

type Drawn = (Choice, Result<RgbImage, String>);

#[derive(Default)]
pub struct Backgrounds {
    /// The window of every background is open.
    pub open: bool,
    /// The game's backgrounds and the levels that use each, read once.
    game: Option<Vec<(SnesAddr, Vec<u16>)>>,
    /// The level's own background, kept so that choosing it again is one
    /// click after choosing one of the game's.
    own: Option<(u16, Layer2)>,
    /// What the pictures are of: the level and the settings that change
    /// how its background draws.
    key: u64,
    pictures: HashMap<Choice, Result<egui::TextureHandle, String>>,
    worker: Option<(Operation, Receiver<Drawn>)>,
}

/// Whether the level's mode draws a background, which can be chosen.
pub fn applies(level: &Level) -> bool {
    level.header.level_mode.layer2() == Layer2Kind::Background
        && matches!(
            level.layer2,
            Layer2::VanillaBackground(_) | Layer2::Background(_)
        )
}

fn current(level: &Level) -> Option<Choice> {
    match level.layer2 {
        Layer2::VanillaBackground(addr) => Some(Choice::Game(addr)),
        Layer2::Background(_) => Some(Choice::Own),
        _ => None,
    }
}

/// What a background's picture depends on: everything of the level but
/// what the picture leaves out or sets itself.
fn key(number: u16, level: &Level) -> u64 {
    let mut stripped = Level {
        layer1: Vec::new(),
        layer2: Layer2::None,
        sprites: Sprites::default(),
        entrances: Vec::new(),
        ..level.clone()
    };
    stripped.header.screens = 0;
    stripped.entrance.entrance_screen = 0;
    let mut hasher = DefaultHasher::new();
    number.hash(&mut hasher);
    format!("{stripped:?}").hash(&mut hasher);
    hasher.finish()
}

/// Half the size, each pixel the average of four.
fn halve(image: &RgbImage) -> RgbImage {
    let (w, h) = (image.width / 2, image.height / 2);
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = image.pixels[((y * 2 + dy) * image.width + x * 2 + dx) as usize];
                for c in 0..3 {
                    sum[c] += u32::from(p[c]);
                }
            }
            out.pixels[(y * w + x) as usize] = sum.map(|s| (s / 4) as u8);
        }
    }
    out
}

impl Backgrounds {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    /// The game's backgrounds, with the levels that use each.
    fn game(&mut self, workspace: &Workspace) -> &[(SnesAddr, Vec<u16>)] {
        self.game
            .get_or_insert_with(|| kobo_core::level::game_backgrounds(workspace.clean()))
    }

    /// Every choice for the level: its own first, if it had one.
    fn choices(&mut self, workspace: &Workspace, number: u16) -> Vec<Choice> {
        let own = self.own.as_ref().is_some_and(|(n, _)| *n == number);
        own.then_some(Choice::Own)
            .into_iter()
            .chain(self.game(workspace).iter().map(|(a, _)| Choice::Game(*a)))
            .collect()
    }

    /// Takes the pictures that are in, and draws those of `wanted` that are
    /// not, for level `number` as it is now.
    fn update(
        &mut self,
        ctx: &egui::Context,
        workspace: &Workspace,
        number: u16,
        level: &Level,
        wanted: Vec<Choice>,
    ) {
        if let Layer2::Background(_) = level.layer2 {
            self.own = Some((number, level.layer2.clone()));
        } else if self.own.as_ref().is_some_and(|(n, _)| *n != number) {
            self.own = None;
        }
        let key = key(number, level);
        if key != self.key {
            if let Some((operation, _)) = self.worker.take() {
                operation.cancel();
            }
            self.pictures.clear();
            self.key = key;
        }
        let mut finished = false;
        if let Some((_, receive)) = &self.worker {
            loop {
                match receive.try_recv() {
                    Ok((choice, picture)) => {
                        let picture = picture.map(|image| {
                            let size = [image.width as usize, image.height as usize];
                            let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
                            ctx.load_texture(
                                format!("background-{choice:?}"),
                                egui::ColorImage::from_rgb(size, &rgb),
                                egui::TextureOptions::LINEAR,
                            )
                        });
                        self.pictures.insert(choice, picture);
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
            self.worker = None;
        }
        if self.worker.is_some() {
            return;
        }
        let missing: Vec<(Choice, Layer2)> = wanted
            .into_iter()
            .filter(|c| !self.pictures.contains_key(c))
            .filter_map(|c| match c {
                Choice::Game(addr) => Some((c, Layer2::VanillaBackground(addr))),
                Choice::Own => self.own.as_ref().map(|(_, own)| (c, own.clone())),
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        let (send, receive) = mpsc::channel();
        let operation = Operation::default();
        let control = operation.clone();
        let (workspace, level, ctx) = (workspace.clone(), level.clone(), ctx.clone());
        std::thread::spawn(move || {
            for (choice, layer2) in missing {
                if control.check().is_err() {
                    break;
                }
                let picture =
                    edit::background_preview(&workspace, number, &level, &layer2, &control)
                        .map(|image| halve(&image))
                        .map_err(|e| e.to_string());
                if send.send((choice, picture)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        self.worker = Some((operation, receive));
    }

    /// A background's picture in `rect`, its bottom kept, or what stands
    /// in for one.
    fn paint(&self, ui: &egui::Ui, rect: egui::Rect, choice: Choice) {
        let painter = ui.painter();
        match self.pictures.get(&choice) {
            Some(Ok(texture)) => {
                let size = texture.size_vec2();
                let scale = (rect.width() / size.x).max(rect.height() / size.y);
                let uv_w = (rect.width() / (size.x * scale)).min(1.0);
                let uv_h = (rect.height() / (size.y * scale)).min(1.0);
                let uv =
                    egui::Rect::from_min_max(egui::pos2(0.0, 1.0 - uv_h), egui::pos2(uv_w, 1.0));
                painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
            }
            Some(Err(_)) => {
                painter.rect_filled(rect, 4, theme::BACKGROUND);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "does not draw",
                    egui::FontId::proportional(12.0),
                    theme::ERROR,
                );
            }
            None => {
                painter.rect_filled(rect, 4, theme::BACKGROUND);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "drawing…",
                    egui::FontId::proportional(12.0),
                    theme::MUTED,
                );
            }
        }
    }

    fn name(&self, choice: Choice) -> String {
        match choice {
            Choice::Game(addr) => match kobo_core::names::game_background(addr) {
                Some(name) => format!("{name} ({:06X})", addr.raw()),
                None => format!("The game's {:06X}", addr.raw()),
            },
            Choice::Own => "The level's own".to_string(),
        }
    }

    /// Who uses a background, to say under its picture.
    fn used_by(&self, choice: Choice) -> String {
        match choice {
            Choice::Game(addr) => {
                let levels = self
                    .game
                    .as_ref()
                    .and_then(|g| g.iter().find(|(a, _)| *a == addr))
                    .map(|(_, l)| l.as_slice())
                    .unwrap_or_default();
                let first: Vec<String> =
                    levels.iter().take(4).map(|n| format!("{n:03X}")).collect();
                match levels.len() {
                    0 => "no level of the game".to_string(),
                    n if n <= 4 => format!("levels {}", first.join(", ")),
                    n => format!("levels {}, and {} more", first.join(", "), n - 4),
                }
            }
            Choice::Own => "drawn for this level, in its file".to_string(),
        }
    }
}

/// The inspector's row: the level's background, which opens the window
/// to choose another.
pub fn row(app: &mut App, ui: &mut egui::Ui) {
    let Some(workspace) = app.workspace().cloned() else {
        return;
    };
    let Some(open) = app.current() else {
        return;
    };
    let (number, level) = (open.number, open.document.level().clone());
    if !applies(&level) {
        return;
    }
    let Some(choice) = current(&level) else {
        return;
    };
    let backgrounds = &mut app.backgrounds;
    let wanted = if backgrounds.open {
        backgrounds.choices(&workspace, number)
    } else {
        vec![choice]
    };
    backgrounds.update(ui.ctx(), &workspace, number, &level, wanted);
    ui.label("Background");
    ui.vertical(|ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(200.0, 64.0), egui::Sense::click());
        backgrounds.paint(ui, rect, choice);
        let stroke = if response.hovered() {
            theme::ACCENT
        } else {
            theme::LINE
        };
        ui.painter().rect_stroke(
            rect,
            4,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Outside,
        );
        let response = response
            .on_hover_text("Choose another background")
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.label(
            RichText::new(backgrounds.name(choice))
                .small()
                .color(theme::MUTED),
        );
        if response.clicked() {
            backgrounds.open = true;
        }
    });
    ui.end_row();
}

/// The window of every background, each as the level would draw it.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.backgrounds.open {
        return;
    }
    let Some(workspace) = app.workspace().cloned() else {
        return;
    };
    let Some(open) = app.current() else {
        return;
    };
    let (number, level) = (open.number, open.document.level().clone());
    if !applies(&level) {
        app.backgrounds.open = false;
        return;
    }
    let chosen = current(&level);
    let backgrounds = &mut app.backgrounds;
    let choices = backgrounds.choices(&workspace, number);
    backgrounds.update(ctx, &workspace, number, &level, choices.clone());
    let mut open = true;
    let mut pick = None;
    egui::Window::new(format!("Background of level {number:03X}"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(680.0)
        .default_height(560.0)
        .default_pos([290.0, 72.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Each as this level draws it, with its tileset and palette. Choosing one \
                     changes the level at once; undo takes it back.",
                )
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(6.0);
            let card = egui::vec2(200.0, 112.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let columns = ((ui.available_width() + 8.0) / (card.x + 8.0))
                        .floor()
                        .max(1.0) as usize;
                    for row in choices.chunks(columns) {
                        ui.horizontal(|ui| {
                            for &choice in row {
                                ui.vertical(|ui| {
                                    ui.set_width(card.x);
                                    let (rect, response) =
                                        ui.allocate_exact_size(card, egui::Sense::click());
                                    backgrounds.paint(ui, rect, choice);
                                    let stroke = if chosen == Some(choice) {
                                        egui::Stroke::new(2.0, theme::SELECTION)
                                    } else if response.hovered() {
                                        egui::Stroke::new(1.0, theme::ACCENT)
                                    } else {
                                        egui::Stroke::new(1.0, theme::LINE)
                                    };
                                    ui.painter().rect_stroke(
                                        rect,
                                        4,
                                        stroke,
                                        egui::StrokeKind::Outside,
                                    );
                                    let name = backgrounds.name(choice);
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!("Use {name}"),
                                        )
                                    });
                                    let response =
                                        response.on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if response.clicked() && chosen != Some(choice) {
                                        pick = Some(choice);
                                    }
                                    ui.label(RichText::new(name).strong());
                                    ui.label(
                                        RichText::new(backgrounds.used_by(choice))
                                            .small()
                                            .color(theme::MUTED),
                                    );
                                });
                                ui.add_space(8.0);
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
        });
    app.backgrounds.open = open;
    let layer2 = match pick {
        Some(Choice::Game(addr)) => Some(Layer2::VanillaBackground(addr)),
        Some(Choice::Own) => app.backgrounds.own.as_ref().map(|(_, own)| own.clone()),
        None => None,
    };
    if let Some(layer2) = layer2 {
        app.apply("Change the background", vec![Edit::SetLayer2(layer2)]);
    }
}
