//! The layer 3 screens window: the title screen's layer 3 and the
//! overworld's border (`overworld::Overworld::title` and `border`, which
//! Lunar Magic's overworld editor edits), drawn as the title screen's load
//! or the overworld's leaves layer 3 in a build of the project, on a
//! thread of its own, and drawn on a tile at a time through the overworld
//! document.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions};
use kobo_core::edit::Workspace;
use kobo_core::gfx::Bpp;
use kobo_core::map16::Tile8Ref;
use kobo_core::palette::Color15;
use kobo_core::render::character_pixel;

use crate::app::App;
use crate::theme;

/// Which screen: the title screen's, or the overworld's border.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    #[default]
    Title,
    Border,
}

/// Layer 3's tilemap shown: 32 columns, the 28 rows that show.
const COLUMNS: usize = 32;
const ROWS: usize = 28;
/// The tiles drawn from, 2bpp, from layer 3's character base.
const CHARACTERS: u16 = 0x200;

/// What a build's load leaves of layer 3: video memory, colours, and where
/// layer 3's characters start (bytes).
struct Loaded {
    vram: Vec<u8>,
    cgram: Vec<u8>,
    characters: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    generation: u64,
    screen: Screen,
}

struct Worker {
    jobs: Sender<(Key, Workspace)>,
    done: Receiver<(Key, Result<Loaded, String>)>,
}

impl Worker {
    fn new(ctx: egui::Context) -> Self {
        let (jobs, inbox) = mpsc::channel::<(Key, Workspace)>();
        let (outbox, done) = mpsc::channel();
        thread::Builder::new()
            .name("kobo-screens".into())
            .spawn(move || {
                while let Ok(mut job) = inbox.recv() {
                    while let Ok(newer) = inbox.try_recv() {
                        job = newer;
                    }
                    let (key, workspace) = job;
                    let result = workspace
                        .build()
                        .map_err(|e| e.to_string())
                        .and_then(|rom| {
                            let loaded = match key.screen {
                                Screen::Title => kobo_core::expand::load_title(&rom),
                                Screen::Border => kobo_core::expand::load_overworld(&rom),
                            }
                            .map_err(|e| e.to_string())?;
                            Ok(Loaded {
                                characters: usize::from(loaded.bg_character_base[2]),
                                vram: loaded.vram,
                                cgram: loaded.cgram,
                            })
                        });
                    if outbox.send((key, result)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the screens thread starts");
        Self { jobs, done }
    }
}

#[derive(Default)]
pub struct ScreensEditor {
    pub open: bool,
    pub screen: Screen,
    pub brush: Tile8Ref,
    loaded: Option<(Key, Loaded)>,
    texture: Option<(Key, TextureHandle)>,
    sheet: Option<(Key, u8, TextureHandle)>,
    failed: Option<(Key, String)>,
    requested: Option<Key>,
    worker: Option<Worker>,
    stroke: Option<usize>,
}

impl ScreensEditor {
    /// Whether the newest picture asked for is drawn, or failed.
    pub fn drawn(&self) -> bool {
        self.requested.is_some_and(|key| {
            self.loaded.as_ref().is_some_and(|(k, _)| *k == key)
                || self.failed.as_ref().is_some_and(|(k, _)| *k == key)
        })
    }
}

/// An 8x8 layer 3 tile's pixels into `pixels` (`width` wide) at (`x`, `y`).
fn draw_tile(pixels: &mut [Color32], width: usize, x: usize, y: usize, r: Tile8Ref, l: &Loaded) {
    let start = l.characters + usize::from(r.tile() % CHARACTERS) * 16;
    for py in 0..8 {
        for px in 0..8 {
            let sx = if r.flip_x() { 7 - px } else { px };
            let sy = if r.flip_y() { 7 - py } else { py };
            let c = character_pixel(&l.vram, start, Bpp::Two, sx, sy);
            if c != 0 {
                let i = 2 * (usize::from(r.palette() % 8) * 4 + usize::from(c));
                let colour = Color15(u16::from_le_bytes([l.cgram[i], l.cgram[i + 1]]));
                let [red, g, b] = colour.to_rgb8();
                pixels[(y + py) * width + x + px] = Color32::from_rgb(red, g, b);
            }
        }
    }
}

/// Layer 3's tilemap word at a cell, as the load left it (word `$5000` on).
fn word_at(l: &Loaded, cell: usize) -> u16 {
    let at = 0xA000 + 2 * cell;
    u16::from_le_bytes([l.vram[at], l.vram[at + 1]])
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.screens_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Layer 3 screens")
        .open(&mut open)
        .default_width(760.0)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.screens_editor.open = false;
    }
}

fn show(app: &mut App, ui: &mut egui::Ui) {
    if let Err(e) = app.overworld_document() {
        ui.label(RichText::new(e).color(theme::ERROR));
        return;
    }
    let mut state = std::mem::take(&mut app.screens_editor);
    let worker = state
        .worker
        .get_or_insert_with(|| Worker::new(ui.ctx().clone()));
    while let Ok((key, result)) = worker.done.try_recv() {
        match result {
            Ok(loaded) => {
                state.loaded = Some((key, loaded));
                state.failed = None;
            }
            Err(e) => state.failed = Some((key, e)),
        }
    }
    let key = Key {
        generation: app.overworld_generation(),
        screen: state.screen,
    };
    if state.requested != Some(key)
        && let Some(workspace) = app.workspace_copy()
    {
        let _ = worker.jobs.send((key, workspace));
        state.requested = Some(key);
    }
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.screen, Screen::Title, "Title screen")
            .on_hover_text("The title screen's layer 3: the logo and what is on it");
        ui.selectable_value(&mut state.screen, Screen::Border, "Overworld border")
            .on_hover_text("The overworld's border on layer 3");
    });
    ui.label(
        RichText::new(
            "Drawn as the build's load leaves layer 3. A click draws the brush's word; a right \
             click takes the word under it.",
        )
        .small()
        .color(theme::MUTED),
    );
    if let Some((failed, e)) = &state.failed
        && *failed == key
    {
        ui.label(RichText::new(e).color(theme::ERROR));
    }
    let mut draw: Option<(usize, bool)> = None;
    if let Some((shown_value, loaded_value)) = state.loaded.take() {
        let shown = &shown_value;
        let loaded = &loaded_value;
        let (width, height) = (COLUMNS * 8, ROWS * 8);
        if state.texture.as_ref().is_none_or(|(k, _)| k != shown) {
            let mut pixels = vec![Color32::BLACK; width * height];
            for cell in 0..COLUMNS * ROWS {
                let r = Tile8Ref(word_at(loaded, cell));
                draw_tile(
                    &mut pixels,
                    width,
                    cell % COLUMNS * 8,
                    cell / COLUMNS * 8,
                    r,
                    loaded,
                );
            }
            let image = egui::ColorImage::new([width, height], pixels);
            state.texture = Some((
                *shown,
                ui.ctx()
                    .load_texture("layer3-screen", image, TextureOptions::NEAREST),
            ));
        }
        let scale = 2.0;
        ui.horizontal_top(|ui| {
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(width as f32, height as f32) * scale,
                Sense::click_and_drag(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "The layer 3 screen")
            });
            let (_, texture) = state.texture.as_ref().expect("drawn above");
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            if *shown != key {
                ui.painter()
                    .rect_filled(rect, 0, Color32::from_black_alpha(90));
            }
            let cell_at = |at: egui::Pos2| {
                let local = (at - rect.min) / (8.0 * scale);
                (local.x >= 0.0
                    && local.y >= 0.0
                    && (local.x as usize) < COLUMNS
                    && (local.y as usize) < ROWS)
                    .then(|| local.y as usize * COLUMNS + local.x as usize)
            };
            if let Some(cell) = response.hover_pos().and_then(cell_at) {
                response.clone().on_hover_text_at_pointer(format!(
                    "({}, {}) · {:04X}",
                    cell % COLUMNS,
                    cell / COLUMNS,
                    word_at(loaded, cell)
                ));
            }
            let pointer = response.interact_pointer_pos().and_then(cell_at);
            if response.secondary_clicked()
                && let Some(cell) = pointer
            {
                state.brush = Tile8Ref(word_at(loaded, cell));
            }
            let primary = ui.input(|i| i.pointer.primary_down());
            if (response.drag_started() || response.clicked() || (response.dragged() && primary))
                && let Some(cell) = pointer
                && state.stroke != Some(cell)
            {
                draw = Some((cell, state.stroke.is_some()));
                state.stroke = Some(cell);
            }
            if response.drag_stopped() || response.clicked() {
                state.stroke = None;
            }
            ui.vertical(|ui| brush_panel(ui, &mut state, loaded, *shown));
        });
        state.loaded = Some((shown_value, loaded_value));
    } else {
        ui.label(RichText::new("Drawing…").color(theme::MUTED));
    }
    let screen = state.screen;
    let word = state.brush.0;
    app.screens_editor = state;
    if let Some((cell, amend)) = draw {
        let label = match screen {
            Screen::Title => "Draw on the title screen",
            Screen::Border => "Draw on the overworld border",
        };
        app.change_overworld(label, amend, |ow| {
            let map = match screen {
                Screen::Title => &mut ow.title,
                Screen::Border => &mut ow.border,
            };
            if let Some(slot) = map.cells.get_mut(cell) {
                *slot = Some(word);
            }
        });
    }
}

/// The brush: its tile, chosen from layer 3's tiles, palette, flips, and
/// priority.
fn brush_panel(ui: &mut egui::Ui, state: &mut ScreensEditor, loaded: &Loaded, key: Key) {
    let brush = state.brush;
    ui.label(RichText::new(format!("Drawing {:04X}", brush.0)).strong());
    egui::Grid::new("screens-brush")
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
    let palette = state.brush.palette();
    if state
        .sheet
        .as_ref()
        .is_none_or(|(k, p, _)| *k != key || *p != palette)
    {
        let (w, h) = (16 * 8, usize::from(CHARACTERS) / 16 * 8);
        let mut pixels = vec![theme::CANVAS; w * h];
        for n in 0..CHARACTERS {
            let r = Tile8Ref::new(n, palette, false, false, false);
            draw_tile(
                &mut pixels,
                w,
                usize::from(n % 16) * 8,
                usize::from(n / 16) * 8,
                r,
                loaded,
            );
        }
        let image = egui::ColorImage::new([w, h], pixels);
        state.sheet = Some((
            key,
            palette,
            ui.ctx()
                .load_texture("screens-characters", image, TextureOptions::NEAREST),
        ));
    }
    let Some((_, _, sheet)) = &state.sheet else {
        return;
    };
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .id_salt("screens-characters")
        .show(ui, |ui| {
            let scale = 2.0;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(128.0, f32::from(CHARACTERS) / 16.0 * 8.0) * scale,
                Sense::click(),
            );
            ui.painter().image(
                sheet.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let cell = 8.0 * scale;
            let n = state.brush.tile() % CHARACTERS;
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
                state.brush = Tile8Ref::new(n, b.palette(), b.priority(), b.flip_x(), b.flip_y());
            }
        });
}
