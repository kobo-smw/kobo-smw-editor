//! The graphics window: the files the open level loads
//! (`edit::graphics::level_files`), or any of the game's, drawn in a row of
//! the level's palette and drawn in (`edit::GraphicsDocument`). The pencil
//! draws with the left button and picks a colour with the right; the fill
//! fills an area of one colour within its 8x8 tile. A stroke is one undo
//! step, and the level is built and drawn again when it ends.

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions};
use kobo_core::edit::GraphicsFile;
use kobo_core::palette::Palette;

use crate::app::App;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tool {
    #[default]
    Pencil,
    Fill,
}

/// The window's state.
#[derive(Default)]
pub struct GraphicsEditor {
    pub open: bool,
    pub file: Option<GraphicsFile>,
    /// The palette row drawn in: 0 to 15, or for a 2bpp file the group
    /// of four colours, 0 to 7.
    pub row: u8,
    /// The colour drawn with.
    pub colour: u8,
    pub tool: Tool,
    /// The file to open by number.
    other: u16,
    /// The pixel the stroke under way last drew, if one is.
    stroke: Option<(u32, u32)>,
    /// The file's picture, and what it was drawn for.
    sheet: Option<(Key, TextureHandle)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    file: GraphicsFile,
    generation: u64,
    shown: u64,
    row: u8,
}

impl GraphicsEditor {
    pub fn forget(&mut self) {
        self.file = None;
        self.sheet = None;
        self.stroke = None;
    }

    fn choose(&mut self, file: GraphicsFile, slot: Option<&str>) {
        if self.file != Some(file) {
            self.file = Some(file);
            self.stroke = None;
            // Sprites draw in the second half of the palette.
            if let Some(slot) = slot {
                self.row = if slot.starts_with("SP") { 9 } else { 2 };
            }
        }
    }
}

/// The colours a file's pixels are drawn in: a 16-colour row, or for a
/// 2bpp file one of the first 32 colours' groups of four.
fn colours(palette: &Palette, row: u8, colors: usize) -> Vec<Color32> {
    (0..colors)
        .map(|c| {
            let index = if colors == 4 {
                usize::from(row % 8) * 4 + c
            } else {
                usize::from(row % 16) * 16 + c
            };
            let [r, g, b] = palette.colors[index].to_rgb8();
            Color32::from_rgb(r, g, b)
        })
        .collect()
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.graphics_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Graphics")
        .open(&mut open)
        .default_width(760.0)
        .resizable(false)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.graphics_editor.open = false;
    }
}

/// What the window asks of the app, once it has drawn.
enum Action {
    Paint(Vec<(u32, u32)>, bool),
    Fill(u32, u32),
    StrokeEnded,
}

fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = app.current() else {
        ui.label(
            RichText::new("Open a level: its graphics files are listed here, in its palette.")
                .color(theme::MUTED),
        );
        return;
    };
    let Some(geometry) = &open.geometry else {
        ui.label(RichText::new("Once the level is drawn.").color(theme::MUTED));
        return;
    };
    let palette = geometry.loaded.video.palette();
    let shown = open.shown;
    let level = open.document.level().clone();
    let files = match app.workspace() {
        Some(workspace) => kobo_core::edit::graphics::level_files(&level, workspace.clean()),
        None => Vec::new(),
    };
    let mut state = std::mem::take(&mut app.graphics_editor);
    if state.file.is_none()
        && let Some(&(slot, file)) = files.first()
    {
        state.choose(file, Some(slot));
    }
    let mut action = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(150.0);
            ui.label(
                RichText::new("THE LEVEL'S FILES")
                    .small()
                    .color(theme::MUTED),
            );
            for &(slot, file) in &files {
                let chosen = state.file == Some(file);
                let row = egui::Button::selectable(chosen, format!("{slot}  {file}"));
                if ui.add(row).clicked() {
                    state.choose(file, Some(slot));
                }
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("ANY OF THE GAME'S")
                    .small()
                    .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.label("GFX");
                ui.add(
                    egui::DragValue::new(&mut state.other)
                        .range(0..=0x33)
                        .speed(0.1)
                        .hexadecimal(2, false, true),
                );
                if ui.button("Open").clicked() {
                    state.choose(GraphicsFile::Gfx(state.other as u8), None);
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(file) = state.file else {
                ui.label(RichText::new("The level loads no file.").color(theme::MUTED));
                return;
            };
            let generation = app.graphics_generation();
            let document = match app.graphics_document(file) {
                Ok(document) => document,
                Err(e) => {
                    ui.label(RichText::new(e).color(theme::ERROR));
                    return;
                }
            };
            let colors = document.colors();
            let image = document.image();
            let modified = document.is_modified();
            let in_project = document.in_project();
            ui.horizontal(|ui| {
                ui.label(RichText::new(file.to_string()).size(16.0).strong());
                let detail = format!(
                    "{} tiles · {colors} colours{}",
                    document.tiles(),
                    if in_project {
                        ""
                    } else {
                        " · the game's, added to the project when saved"
                    }
                );
                ui.label(RichText::new(detail).small().color(theme::MUTED));
                if modified {
                    ui.label(RichText::new("● modified").small().color(theme::ACCENT));
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Palette").color(theme::MUTED));
                let most = if colors == 4 { 7 } else { 15 };
                let mut row = u16::from(state.row);
                ui.add(egui::DragValue::new(&mut row).range(0..=most).speed(0.05));
                state.row = row as u8;
                ui.separator();
                ui.selectable_value(&mut state.tool, Tool::Pencil, "✏ Pencil")
                    .on_hover_text("Draw with the left button; the right picks a colour");
                ui.selectable_value(&mut state.tool, Tool::Fill, "Fill")
                    .on_hover_text("Fill an area of one colour, within its 8x8 tile");
            });
            let swatches = colours(&palette, state.row, colors);
            state.colour = state.colour.min(colors as u8 - 1);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, &colour) in swatches.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
                    ui.painter().rect_filled(rect, 2, colour);
                    if usize::from(state.colour) == i {
                        ui.painter().rect_stroke(
                            rect,
                            2,
                            egui::Stroke::new(2.0, Color32::WHITE),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.on_hover_text(format!("Colour {i}")).clicked() {
                        state.colour = i as u8;
                    }
                }
            });
            ui.add_space(4.0);

            let key = Key {
                file,
                generation,
                shown,
                row: state.row,
            };
            if state.sheet.as_ref().is_none_or(|(k, _)| *k != key) {
                let pixels: Vec<Color32> = image
                    .pixels
                    .iter()
                    .map(|&p| {
                        if p == 0 {
                            theme::CANVAS
                        } else {
                            swatches
                                .get(usize::from(p))
                                .copied()
                                .unwrap_or(Color32::MAGENTA)
                        }
                    })
                    .collect();
                let picture =
                    egui::ColorImage::new([image.width as usize, image.height as usize], pixels);
                let texture =
                    ui.ctx()
                        .load_texture("graphics-sheet", picture, TextureOptions::NEAREST);
                state.sheet = Some((key, texture));
            }
            let (_, texture) = state.sheet.as_ref().expect("drawn above");
            let (width, height) = (image.width, image.height);
            let scale = 4.0;
            egui::ScrollArea::vertical()
                .max_height(560.0)
                .id_salt("graphics-sheet")
                .show(ui, |ui| {
                    let size = egui::vec2(width as f32, height as f32) * scale;
                    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    // The 8x8 grid, faint.
                    let grid = egui::Stroke::new(1.0, Color32::from_white_alpha(18));
                    for x in (8..width).step_by(8) {
                        let at = rect.left() + x as f32 * scale;
                        ui.painter().vline(at, rect.y_range(), grid);
                    }
                    for y in (8..height).step_by(8) {
                        let at = rect.top() + y as f32 * scale;
                        ui.painter().hline(rect.x_range(), at, grid);
                    }
                    let pixel_at = |at: egui::Pos2| {
                        let local = (at - rect.min) / scale;
                        (local.x >= 0.0
                            && local.y >= 0.0
                            && local.x < width as f32
                            && local.y < height as f32)
                            .then_some((local.x as u32, local.y as u32))
                    };
                    let hovered = response.hover_pos().and_then(pixel_at);
                    if let Some((x, y)) = hovered {
                        let tile = (y / 8) * 16 + x / 8;
                        response.clone().on_hover_text_at_pointer(format!(
                            "Tile {tile:02X} · ({x}, {y}) · colour {}",
                            image.pixels[(y * width + x) as usize]
                        ));
                    }
                    let pointer = response.interact_pointer_pos().and_then(pixel_at);
                    if response.secondary_clicked()
                        && let Some((x, y)) = pointer
                    {
                        state.colour = image.pixels[(y * width + x) as usize];
                    }
                    match state.tool {
                        Tool::Pencil => {
                            let primary = ui.input(|i| i.pointer.primary_down());
                            if (response.drag_started()
                                || response.clicked()
                                || (response.dragged() && primary))
                                && let Some(at) = pointer
                                && ui.input(|i| !i.pointer.secondary_down())
                            {
                                let line = match state.stroke {
                                    Some(from) => line(from, at),
                                    None => vec![at],
                                };
                                action = Some(Action::Paint(line, state.stroke.is_some()));
                                state.stroke = Some(at);
                            }
                            if (response.drag_stopped() || response.clicked())
                                && state.stroke.is_some()
                            {
                                state.stroke = None;
                                if !matches!(action, Some(Action::Paint(..))) {
                                    action = Some(Action::StrokeEnded);
                                }
                            }
                        }
                        Tool::Fill => {
                            if response.clicked()
                                && let Some((x, y)) = pointer
                            {
                                action = Some(Action::Fill(x, y));
                            }
                        }
                    }
                });
        });
    });
    let file = state.file;
    let colour = state.colour;
    let ended = state.stroke.is_none();
    app.graphics_editor = state;
    let Some(file) = file else { return };
    match action {
        Some(Action::Paint(pixels, stroke)) => {
            app.paint(file, &pixels, colour, stroke);
            if ended {
                app.graphics_changed(file);
            }
        }
        Some(Action::Fill(x, y)) => app.fill(file, x, y, colour),
        Some(Action::StrokeEnded) => app.graphics_changed(file),
        None => {}
    }
}

/// The pixels from `a` to `b`, both ends among them, so that a fast stroke
/// leaves no gaps.
fn line(a: (u32, u32), b: (u32, u32)) -> Vec<(u32, u32)> {
    let (x0, y0) = (a.0 as i32, a.1 as i32);
    let (x1, y1) = (b.0 as i32, b.1 as i32);
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    (0..=steps)
        .map(|i| {
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            (x as u32, y as u32)
        })
        .collect()
}
