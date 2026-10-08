//! The layer 3 window: the open level's layer 3 tilemap
//! (`edit::layer3`), drawn with the level's own layer 3 tiles and colours,
//! and drawn on a tile at a time. A level without one is given one, which
//! its graphics list then loads (`T`). The rows the file does not reach,
//! or that stay under the status bar, are dimmed.

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions};
use kobo_core::edit::layer3::Tilemap;
use kobo_core::gfx::Bpp;
use kobo_core::map16::Tile8Ref;
use kobo_core::palette::Palette;
use kobo_core::render::character_pixel;

use crate::app::App;
use crate::theme;

/// Where layer 3's tiles are in VRAM, in bytes (word `$4000`).
const CHARACTERS: usize = 0x8000;
/// The tiles there, 2bpp.
const CHARACTER_COUNT: u16 = 0x200;

/// The window's state.
#[derive(Default)]
pub struct Layer3Editor {
    pub open: bool,
    /// The word drawn with.
    pub brush: Tile8Ref,
    /// The cell the stroke under way last drew, if one is.
    stroke: Option<usize>,
    map: Option<(Key, TextureHandle)>,
    characters: Option<(Key, TextureHandle)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    generation: u64,
    shown: u64,
    which: u16,
}

impl Layer3Editor {
    pub fn forget(&mut self) {
        self.stroke = None;
        self.map = None;
        self.characters = None;
    }
}

fn colour(palette: &Palette, group: u8, c: u8) -> [u8; 3] {
    palette.colors[usize::from(group % 8) * 4 + usize::from(c & 3)].to_rgb8()
}

/// Draws an 8x8 layer 3 tile into `pixels` (`width` wide) at (`x`, `y`).
fn draw(
    pixels: &mut [Color32],
    width: usize,
    x: usize,
    y: usize,
    r: Tile8Ref,
    vram: &[u8],
    palette: &Palette,
) {
    let start = CHARACTERS + usize::from(r.tile() % CHARACTER_COUNT) * 16;
    for py in 0..8 {
        for px in 0..8 {
            let sx = if r.flip_x() { 7 - px } else { px };
            let sy = if r.flip_y() { 7 - py } else { py };
            let c = character_pixel(vram, start, Bpp::Two, sx, sy);
            if c != 0 {
                let [red, g, b] = colour(palette, r.palette(), c);
                pixels[(y + py) * width + x + px] = Color32::from_rgb(red, g, b);
            }
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.layer3_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Layer 3")
        .open(&mut open)
        .default_width(980.0)
        .resizable(false)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.layer3_editor.open = false;
    }
}

enum Action {
    Draw(Vec<usize>, bool),
    StrokeEnded,
}

fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = app.current() else {
        ui.label(RichText::new("Open a level to draw on its layer 3.").color(theme::MUTED));
        return;
    };
    let Some(geometry) = &open.geometry else {
        ui.label(RichText::new("Once the level is drawn.").color(theme::MUTED));
        return;
    };
    let tilemap = Tilemap::of(open.document.level());
    let vram = geometry.loaded.video.vram.clone();
    let palette = geometry.loaded.video.palette();
    let shown = open.shown;
    let Some(tilemap) = tilemap else {
        ui.label("The level has no layer 3 tilemap of its own: its layer 3 is the game's, as its header's layer 3 setting draws it.");
        if ui
            .button("Give the level a layer 3 tilemap")
            .on_hover_text("A new ExGFX file of blank tiles under the status bar, which the level's graphics list loads")
            .clicked()
        {
            app.give_tilemap();
        }
        return;
    };
    let generation = app.layer3_generation();
    let words = match app.tilemap_document(tilemap.file, tilemap.bytes()) {
        Ok(document) => document.words().to_vec(),
        Err(e) => {
            ui.label(RichText::new(e).color(theme::ERROR));
            return;
        }
    };
    let in_project = app
        .open_tilemap(tilemap.file)
        .is_some_and(|d| d.in_project());
    let mut state = std::mem::take(&mut app.layer3_editor);
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("ExGFX{:X}", tilemap.file))
                .size(16.0)
                .strong(),
        );
        let detail = format!(
            "{} bytes from ${:04X}{}",
            tilemap.bytes(),
            tilemap.base(),
            if in_project {
                ""
            } else {
                " · new, added to the project when saved"
            }
        );
        ui.label(RichText::new(detail).small().color(theme::MUTED));
    });
    ui.horizontal_top(|ui| {
        // The tilemap, 64 tiles wide, as tall as the file reaches.
        let rows = if tilemap.base() == 0x5800 || tilemap.bytes() > 0x1000 {
            64
        } else {
            32
        };
        let (width, height) = (64 * 8, rows * 8);
        let key = Key {
            generation,
            shown,
            which: 0,
        };
        if state.map.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut pixels = vec![theme::CANVAS; width * height];
            for y in 0..rows {
                for x in 0..64 {
                    let at = (x * 8, y * 8);
                    match tilemap.index_at(x, y, words.len()) {
                        Some(i) => draw(
                            &mut pixels,
                            width,
                            at.0,
                            at.1,
                            Tile8Ref(words[i]),
                            &vram,
                            &palette,
                        ),
                        None => {
                            for py in 0..8 {
                                for px in 0..8 {
                                    pixels[(at.1 + py) * width + at.0 + px] =
                                        Color32::from_gray(24);
                                }
                            }
                        }
                    }
                }
            }
            let image = egui::ColorImage::new([width, height], pixels);
            state.map = Some((
                key,
                ui.ctx()
                    .load_texture("layer3-map", image, TextureOptions::NEAREST),
            ));
        }
        let (_, map) = state.map.as_ref().expect("drawn above");
        let scale = 1.25;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width as f32, height as f32) * scale,
            Sense::click_and_drag(),
        );
        ui.painter().image(
            map.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let cell = 8.0 * scale;
        let cell_at = |at: egui::Pos2| {
            let local = (at - rect.min) / cell;
            (local.x >= 0.0 && local.y >= 0.0)
                .then(|| tilemap.index_at(local.x as usize, local.y as usize, words.len()))
                .flatten()
        };
        if let Some(pos) = response.hover_pos() {
            let local = (pos - rect.min) / cell;
            let text = match cell_at(pos) {
                Some(i) => format!(
                    "({}, {}) · word {i:03X} · tile {:03X}",
                    local.x as usize,
                    local.y as usize,
                    Tile8Ref(words[i]).tile()
                ),
                None => format!(
                    "({}, {}) · the file does not load here",
                    local.x as usize, local.y as usize
                ),
            };
            response.clone().on_hover_text_at_pointer(text);
        }
        let pointer = response.interact_pointer_pos().and_then(cell_at);
        if response.secondary_clicked()
            && let Some(i) = pointer
        {
            state.brush = Tile8Ref(words[i]);
        }
        let primary = ui.input(|i| i.pointer.primary_down());
        if (response.drag_started() || response.clicked() || (response.dragged() && primary))
            && let Some(i) = pointer
            && ui.input(|i| !i.pointer.secondary_down())
            && state.stroke != Some(i)
        {
            action = Some(Action::Draw(vec![i], state.stroke.is_some()));
            state.stroke = Some(i);
        }
        if (response.drag_stopped() || response.clicked()) && state.stroke.is_some() {
            state.stroke = None;
            if !matches!(action, Some(Action::Draw(..))) {
                action = Some(Action::StrokeEnded);
            }
        }

        ui.vertical(|ui| {
            ui.set_width(300.0);
            let brush = state.brush;
            ui.label(RichText::new(format!("Drawing tile {:03X}", brush.tile())).strong());
            egui::Grid::new("layer3-brush")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Palette");
                    let mut p = u16::from(brush.palette());
                    ui.add(egui::DragValue::new(&mut p).range(0..=7).speed(0.05));
                    ui.end_row();
                    ui.label("Flip");
                    let (mut x, mut y) = (brush.flip_x(), brush.flip_y());
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut x, "X");
                        ui.checkbox(&mut y, "Y");
                    });
                    ui.end_row();
                    ui.label("Priority");
                    let mut priority = brush.priority();
                    ui.checkbox(&mut priority, "In front");
                    ui.end_row();
                    state.brush = Tile8Ref::new(brush.tile(), p as u8, priority, x, y);
                });
            ui.label(
                RichText::new(
                    "Layer 3's tiles: a click chooses one; a right click on the map takes its word",
                )
                .small()
                .color(theme::MUTED),
            );
            let key = Key {
                generation,
                shown,
                which: u16::from(state.brush.palette()) + 1,
            };
            if state.characters.as_ref().is_none_or(|(k, _)| *k != key) {
                let (w, h) = (16 * 8, usize::from(CHARACTER_COUNT) / 16 * 8);
                let mut pixels = vec![theme::CANVAS; w * h];
                for n in 0..CHARACTER_COUNT {
                    let r = Tile8Ref::new(n, state.brush.palette(), false, false, false);
                    draw(
                        &mut pixels,
                        w,
                        usize::from(n % 16) * 8,
                        usize::from(n / 16) * 8,
                        r,
                        &vram,
                        &palette,
                    );
                }
                let image = egui::ColorImage::new([w, h], pixels);
                state.characters = Some((
                    key,
                    ui.ctx()
                        .load_texture("layer3-characters", image, TextureOptions::NEAREST),
                ));
            }
            let (_, characters) = state.characters.as_ref().expect("drawn above");
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .id_salt("layer3-characters")
                .show(ui, |ui| {
                    let scale = 2.0;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(128.0, f32::from(CHARACTER_COUNT) / 16.0 * 8.0) * scale,
                        Sense::click(),
                    );
                    ui.painter().image(
                        characters.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    let cell = 8.0 * scale;
                    let n = state.brush.tile() % CHARACTER_COUNT;
                    ui.painter().rect_stroke(
                        egui::Rect::from_min_size(
                            rect.min + egui::vec2(f32::from(n % 16), f32::from(n / 16)) * cell,
                            egui::vec2(cell, cell),
                        ),
                        0,
                        egui::Stroke::new(1.5, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                    if response.clicked()
                        && let Some(at) = response.interact_pointer_pos()
                    {
                        let local = (at - rect.min) / cell;
                        let n = (local.y as u16) * 16 + (local.x as u16).min(15);
                        let b = state.brush;
                        state.brush =
                            Tile8Ref::new(n, b.palette(), b.priority(), b.flip_x(), b.flip_y());
                    }
                });
        });
    });
    let brush = state.brush;
    let ended = state.stroke.is_none();
    app.layer3_editor = state;
    match action {
        Some(Action::Draw(cells, stroke)) => {
            let cells: Vec<(usize, Tile8Ref)> = cells.into_iter().map(|i| (i, brush)).collect();
            app.draw_tilemap(tilemap.file, &cells, stroke);
            if ended {
                app.tilemap_changed(tilemap.file);
            }
        }
        Some(Action::StrokeEnded) => app.tilemap_changed(tilemap.file),
        None => {}
    }
}
