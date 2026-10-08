//! The Map16 window: the project's foreground Map16 tiles
//! (`edit::Map16Document`), a page at a time, edited as the open level
//! sees them: its object tileset decides which definition of a tile kept
//! per tileset is shown, and its graphics and palette draw them. A tile is
//! its four 8x8 tiles (each a character of the level's graphics, a palette
//! row, flips, and priority) and the tile it acts like. Pictures here are
//! drawn from the document at once; the level is built again behind them.

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions};
use kobo_core::edit::TileChange;
use kobo_core::image::RgbImage;
use kobo_core::map16::Tile8Ref;
use kobo_core::palette::Palette;
use kobo_core::render::{LayerTiles, draw_map16_tile, draw_tile_ref};
use kobo_core::source::map16::Map16Entry;

use crate::app::App;
use crate::theme;

/// The window's state.
#[derive(Default)]
pub struct Map16Editor {
    pub open: bool,
    pub page: u8,
    /// The tile chosen, and its quarter, (x, y) in 0..2.
    pub tile: Option<u16>,
    pub quarter: (usize, usize),
    /// The page's picture and the 8x8 tiles', with what each was drawn
    /// for.
    sheet: Option<(SheetKey, TextureHandle)>,
    characters: Option<(SheetKey, TextureHandle)>,
}

/// What a picture was drawn from: the document's state, the level's
/// picture, and the page or palette row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SheetKey {
    generation: u64,
    shown: u64,
    which: u16,
}

impl Map16Editor {
    /// Opens the window at `tile`.
    pub fn show_tile(&mut self, tile: u16) {
        self.open = true;
        self.page = (tile >> 8) as u8;
        self.tile = Some(tile);
    }

    /// Forgets the pictures, for another project.
    pub fn forget(&mut self) {
        self.sheet = None;
        self.characters = None;
        self.tile = None;
    }
}

const BACKGROUND: [u8; 3] = [
    theme::BACKGROUND.r(),
    theme::BACKGROUND.g(),
    theme::BACKGROUND.b(),
];

fn texture(ctx: &egui::Context, name: &str, image: &RgbImage) -> TextureHandle {
    let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
    let image = egui::ColorImage::from_rgb([image.width as usize, image.height as usize], &rgb);
    ctx.load_texture(name, image, TextureOptions::NEAREST)
}

/// The 1024 8x8 tiles of the level's layers, 16 to a row, in palette row
/// `row`.
fn character_sheet(tiles: &LayerTiles, palette: &Palette, row: u8) -> RgbImage {
    let mut image = RgbImage::new(16 * 8, 64 * 8);
    image.pixels.fill(BACKGROUND);
    for n in 0..0x400u16 {
        let r = Tile8Ref::new(n, row, false, false, false);
        draw_tile_ref(
            &mut image,
            u32::from(n % 16) * 8,
            u32::from(n / 16) * 8,
            r,
            tiles,
            palette,
        );
    }
    image
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.map16_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Map16")
        .open(&mut open)
        .default_width(820.0)
        .resizable(false)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.map16_editor.open = false;
    }
}

/// A change the window asks for: its label, the tile, and whether a value
/// is being dragged.
type Change = (String, TileChange, bool);

fn show(app: &mut App, ui: &mut egui::Ui) {
    let mut state = std::mem::take(&mut app.map16_editor);
    let (change, place) = contents(app, &mut state, ui);
    app.map16_editor = state;
    if let Some((label, change, dragging)) = change {
        app.apply_map16(&label, change, dragging);
    }
    if let Some(tile) = place {
        app.placing = Some(crate::palette::Placing::Map16(tile));
    }
}

fn contents(
    app: &App,
    state: &mut Map16Editor,
    ui: &mut egui::Ui,
) -> (Option<Change>, Option<u16>) {
    let Some(map16) = app.map16() else {
        let why = app
            .map16_error()
            .map_or("The project's Map16 files did not open.".to_string(), |e| {
                format!("The project's Map16 files did not open: {e}")
            });
        ui.label(RichText::new(why).color(theme::ERROR));
        return (None, None);
    };
    let generation = app.map16_generation();
    let Some(open) = app.current() else {
        ui.label(
            RichText::new("Open a level: its tileset, graphics, and palette draw the tiles.")
                .color(theme::MUTED),
        );
        return (None, None);
    };
    let Some(geometry) = &open.geometry else {
        ui.label(RichText::new("Once the level is drawn.").color(theme::MUTED));
        return (None, None);
    };
    let tileset = open.document.level().header.object_tileset;
    let shown = open.shown;
    let tiles = LayerTiles::from_vram(&geometry.loaded.video.vram);
    let palette = geometry.loaded.video.palette();

    // The page, and the tiles on it as the document has them.
    let page = state.page;
    let entries: Vec<Map16Entry> = (0..0x100)
        .map(|i| map16.entry(u16::from(page) << 8 | i, tileset))
        .collect();
    let key = SheetKey {
        generation,
        shown,
        which: u16::from(page),
    };
    if state.sheet.as_ref().is_none_or(|(k, _)| *k != key) {
        let mut image = RgbImage::new(256, 256);
        image.pixels.fill(BACKGROUND);
        for (i, entry) in entries.iter().enumerate() {
            let (x, y) = ((i % 16) as u32 * 16, (i / 16) as u32 * 16);
            draw_map16_tile(&mut image, x, y, &entry.gfx, &tiles, &palette);
        }
        state.sheet = Some((key, texture(ui.ctx(), "map16-editor-page", &image)));
    }
    let selected = state.tile.filter(|t| t >> 8 == u16::from(page));
    let quarter_ref = selected.map(|t| {
        entries[usize::from(t & 0xFF)]
            .gfx
            .quadrant(state.quarter.0, state.quarter.1)
    });
    let row = quarter_ref.map_or(0, Tile8Ref::palette);
    let key = SheetKey {
        generation,
        shown,
        which: u16::from(row),
    };
    if state.characters.as_ref().is_none_or(|(k, _)| *k != key) {
        let image = character_sheet(&tiles, &palette, row);
        state.characters = Some((key, texture(ui.ctx(), "map16-editor-characters", &image)));
    }

    let mut change: Option<Change> = None;
    let mut place = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Object tileset {tileset:X}")).strong());
        if let Some(name) = kobo_core::names::object_tileset(tileset) {
            ui.label(RichText::new(name).color(theme::MUTED));
        }
        ui.separator();
        ui.label(RichText::new("Page").color(theme::MUTED));
        if ui.small_button("◀").clicked() {
            state.page = state.page.saturating_sub(1);
        }
        let mut p = u16::from(state.page);
        ui.add(
            egui::DragValue::new(&mut p)
                .range(0..=0x7F)
                .speed(0.1)
                .hexadecimal(2, false, true),
        );
        state.page = p as u8;
        if ui.small_button("▶").clicked() {
            state.page = (state.page + 1).min(0x7F);
        }
    });
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        // The page: a click chooses a tile, a double click places it.
        let (_, sheet) = state.sheet.as_ref().expect("drawn above");
        let scale = 2.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(256.0, 256.0) * scale, Sense::click());
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(sheet.id(), rect, uv, Color32::WHITE);
        let cell = 16.0 * scale;
        let tile_at = |at: egui::Pos2| {
            let local = (at - rect.min) / cell;
            (local.x >= 0.0 && local.y >= 0.0 && local.x < 16.0 && local.y < 16.0)
                .then(|| u16::from(page) << 8 | (local.y as u16) << 4 | local.x as u16)
        };
        let cell_rect = |tile: u16| {
            let i = tile & 0xFF;
            egui::Rect::from_min_size(
                rect.min + egui::vec2(f32::from(i % 16), f32::from(i / 16)) * cell,
                egui::vec2(cell, cell),
            )
        };
        for i in 0..0x100u16 {
            let tile = u16::from(page) << 8 | i;
            if map16.is_changed(tile, tileset) {
                let r = cell_rect(tile);
                ui.painter().circle_filled(r.right_top() + egui::vec2(-4.0, 4.0), 2.5, theme::ACCENT);
            }
        }
        if let Some(tile) = selected {
            ui.painter().rect_stroke(
                cell_rect(tile),
                0,
                egui::Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(tile) = response.hover_pos().and_then(tile_at) {
            ui.painter().rect_stroke(
                cell_rect(tile),
                0,
                egui::Stroke::new(1.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            response
                .clone()
                .on_hover_text_at_pointer(format!("Map16 tile {tile:03X} (double-click to place it)"));
        }
        if let Some(tile) = response.interact_pointer_pos().and_then(tile_at) {
            if response.clicked() {
                state.tile = Some(tile);
            }
            if response.double_clicked() {
                place = Some(tile);
            }
        }

        ui.vertical(|ui| {
            ui.set_width(250.0);
            let Some(tile) = selected else {
                ui.label(RichText::new("Choose a tile.").color(theme::MUTED));
                return;
            };
            let entry = entries[usize::from(tile & 0xFF)];
            ui.label(RichText::new(format!("Tile {tile:03X}")).size(16.0).strong());
            let shared = map16.shown_in(tile, tileset);
            let note = if shared.len() == kobo_core::map16::TILESET_COUNT as usize {
                "The same in every object tileset.".to_string()
            } else if shared.len() == 1 {
                format!("Object tileset {tileset:X}'s own.")
            } else {
                let list: Vec<String> = shared.iter().map(|t| format!("{t:X}")).collect();
                format!("Object tilesets {} share it.", list.join(", "))
            };
            ui.label(RichText::new(note).small().color(theme::MUTED));
            ui.add_space(6.0);

            // The tile large, its quarters to choose from.
            let zoom = 6.0;
            let (big, response) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0) * zoom, Sense::click());
            let i = tile & 0xFF;
            let uv = egui::Rect::from_min_size(
                egui::pos2(f32::from(i % 16) / 16.0, f32::from(i / 16) / 16.0),
                egui::vec2(1.0 / 16.0, 1.0 / 16.0),
            );
            ui.painter().image(sheet.id(), big, uv, Color32::WHITE);
            let (qx, qy) = state.quarter;
            let quarter = egui::Rect::from_min_size(
                big.min + egui::vec2(qx as f32, qy as f32) * 8.0 * zoom,
                egui::vec2(8.0, 8.0) * zoom,
            );
            ui.painter().rect_stroke(
                quarter,
                0,
                egui::Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            if response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                let local = (at - big.min) / (8.0 * zoom);
                state.quarter = ((local.x as usize).min(1), (local.y as usize).min(1));
            }
            ui.add_space(6.0);

            let r = entry.gfx.quadrant(state.quarter.0, state.quarter.1);
            let set_quarter = |r: Tile8Ref| {
                let mut gfx = entry.gfx;
                match state.quarter {
                    (0, 0) => gfx.top_left = r,
                    (0, _) => gfx.bottom_left = r,
                    (_, 0) => gfx.top_right = r,
                    _ => gfx.bottom_right = r,
                }
                Map16Entry { gfx, ..entry }
            };
            let tile_change = |entry: Map16Entry| TileChange {
                tile,
                tileset,
                entry,
            };
            egui::Grid::new("map16-quarter")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("8x8 tile");
                    let mut n = r.tile();
                    let response = ui.add(
                        egui::DragValue::new(&mut n)
                            .range(0..=0x3FF)
                            .speed(0.1)
                            .hexadecimal(3, false, true),
                    );
                    if response.changed() {
                        let new = Tile8Ref::new(n, r.palette(), r.priority(), r.flip_x(), r.flip_y());
                        let dragging = response.dragged() && !response.drag_started();
                        change = Some(("Change Map16 8x8 tile".into(), tile_change(set_quarter(new)), dragging));
                    }
                    ui.end_row();
                    ui.label("Palette");
                    let mut p = u16::from(r.palette());
                    let response = ui.add(egui::DragValue::new(&mut p).range(0..=7).speed(0.05));
                    if response.changed() {
                        let new = Tile8Ref::new(r.tile(), p as u8, r.priority(), r.flip_x(), r.flip_y());
                        let dragging = response.dragged() && !response.drag_started();
                        change = Some(("Change Map16 palette".into(), tile_change(set_quarter(new)), dragging));
                    }
                    ui.end_row();
                    ui.label("Flip");
                    ui.horizontal(|ui| {
                        let (mut x, mut y) = (r.flip_x(), r.flip_y());
                        let fx = ui.checkbox(&mut x, "X").changed();
                        let fy = ui.checkbox(&mut y, "Y").changed();
                        if fx || fy {
                            let new = Tile8Ref::new(r.tile(), r.palette(), r.priority(), x, y);
                            change = Some(("Flip Map16 8x8 tile".into(), tile_change(set_quarter(new)), false));
                        }
                    });
                    ui.end_row();
                    ui.label("Priority");
                    let mut priority = r.priority();
                    if ui
                        .checkbox(&mut priority, "In front of sprites")
                        .changed()
                    {
                        let new = Tile8Ref::new(r.tile(), r.palette(), priority, r.flip_x(), r.flip_y());
                        change = Some(("Change Map16 priority".into(), tile_change(set_quarter(new)), false));
                    }
                    ui.end_row();
                    ui.label("Acts like").on_hover_text(
                        "The tile whose behaviour this one has: itself for most of pages 0 and 1, 130 (cement) for an empty tile past them",
                    );
                    let mut acts = entry.acts;
                    let response = ui.add(
                        egui::DragValue::new(&mut acts)
                            .range(0..=0x7FFF)
                            .speed(0.1)
                            .hexadecimal(3, false, true),
                    );
                    if response.changed() {
                        let dragging = response.dragged() && !response.drag_started();
                        change = Some((
                            "Change what a Map16 tile acts like".into(),
                            tile_change(Map16Entry { acts, ..entry }),
                            dragging,
                        ));
                    }
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button("Place it")
                    .on_hover_text("Place this tile with each click on the level")
                    .clicked()
                {
                    place = Some(tile);
                }
                let changed = map16.is_changed(tile, tileset);
                let back = ui
                    .add_enabled(changed, egui::Button::new("Back to the game's"))
                    .on_hover_text(if tile < 0x200 {
                        "Put the tile back as the game has it"
                    } else {
                        "Empty the tile, as a new page has it"
                    })
                    .on_disabled_hover_text("The project does not change this tile");
                if back.clicked() {
                    let clean = map16.clean_entry(tile, tileset);
                    change = Some(("Put a Map16 tile back".into(), tile_change(clean), false));
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("The level's 8x8 tiles, in palette {row}: a click sets the quarter's"))
                    .small()
                    .color(theme::MUTED),
            );
            let (_, characters) = state.characters.as_ref().expect("drawn above");
            egui::ScrollArea::vertical()
                .max_height(256.0)
                .id_salt("map16-characters")
                .show(ui, |ui| {
                    let scale = 1.75;
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(128.0, 512.0) * scale, Sense::click());
                    ui.painter().image(
                        characters.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    let cell = 8.0 * scale;
                    let at_cell = |n: u16| {
                        egui::Rect::from_min_size(
                            rect.min + egui::vec2(f32::from(n % 16), f32::from(n / 16)) * cell,
                            egui::vec2(cell, cell),
                        )
                    };
                    ui.painter().rect_stroke(
                        at_cell(r.tile()),
                        0,
                        egui::Stroke::new(1.5, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                    let char_at = |at: egui::Pos2| {
                        let local = (at - rect.min) / cell;
                        (local.x >= 0.0 && local.y >= 0.0 && local.x < 16.0 && local.y < 64.0)
                            .then(|| (local.y as u16) * 16 + local.x as u16)
                    };
                    if let Some(n) = response.hover_pos().and_then(char_at) {
                        response.clone().on_hover_text_at_pointer(format!("8x8 tile {n:03X}"));
                    }
                    if response.clicked()
                        && let Some(n) = response.interact_pointer_pos().and_then(char_at)
                    {
                        let new = Tile8Ref::new(n, r.palette(), r.priority(), r.flip_x(), r.flip_y());
                        change = Some(("Change Map16 8x8 tile".into(), tile_change(set_quarter(new)), false));
                    }
                });
        });
    });

    (change, place)
}
