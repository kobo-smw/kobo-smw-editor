//! The palette: objects and sprites to add to the level. Choosing one
//! puts the canvas in placing mode, where each click places one, until
//! Escape or a right click.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText};
use kobo_core::edit::{self, Workspace};
use kobo_core::image::RgbImage;
use kobo_core::level::objects::Object;
use kobo_core::names;
use kobo_core::operation::Operation;
use kobo_core::source::level::Level;

use crate::app::App;
use crate::theme;

/// What a click on the canvas places.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Placing {
    /// An object, as a template: its place is where it is put.
    Object(Object),
    Sprite(u8),
    /// A Map16 tile, placed directly (`edit::map16_object`).
    Map16(u16),
}

impl Placing {
    pub fn name(&self, tileset: u8) -> String {
        match self {
            Placing::Object(object) => names::object(object, tileset)
                .unwrap_or("Object")
                .to_string(),
            Placing::Sprite(id) => names::sprite(*id).to_string(),
            Placing::Map16(tile) => format!("Map16 tile {tile:03X}"),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    #[default]
    Objects,
    Extended,
    Sprites,
    Map16,
}

#[derive(Default)]
pub struct PaletteState {
    kind: Kind,
    filter: String,
    /// The Map16 page shown, and its picture: for which level's picture
    /// (its request), which page.
    map16_page: u16,
    map16_sheet: Option<((u64, u16), egui::TextureHandle)>,
    thumbnails: Thumbnails,
}

/// A list's level, tileset (sprite set for sprites), and kind.
type ListKey = (u16, u8, Kind);
/// A picture on the GPU and its size.
type Thumbnail = (egui::TextureHandle, egui::Vec2);

/// The pictures of what the palette lists as the open level draws them,
/// made on a worker thread (`edit::object_previews`,
/// `edit::sprite_previews`), kept by level, tileset (the sprite set for
/// sprites), and list.
#[derive(Default)]
struct Thumbnails {
    cache: HashMap<ListKey, Vec<Option<Thumbnail>>>,
    pending: Option<Pending>,
    /// Why the last pictures could not be drawn.
    error: Option<String>,
}

struct Pending {
    key: (u16, u8, Kind),
    operation: Operation,
    result: Receiver<Result<Vec<Option<RgbImage>>, String>>,
}

impl Thumbnails {
    /// The picture of entry `i` of the list `key` names.
    fn get(&self, key: (u16, u8, Kind), i: usize) -> Option<&(egui::TextureHandle, egui::Vec2)> {
        self.cache.get(&key)?.get(i)?.as_ref()
    }

    /// Takes pictures that are in, and asks for `key`'s if they are not
    /// there or on their way.
    fn update(
        &mut self,
        ctx: &egui::Context,
        key: (u16, u8, Kind),
        start: impl FnOnce() -> Option<(Workspace, Level, Vec<Placing>)>,
    ) {
        if let Some(pending) = &self.pending
            && let Ok(result) = pending.result.try_recv()
        {
            let pending = self.pending.take().expect("checked");
            self.error = result.as_ref().err().cloned();
            let pictures = result
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(i, picture)| {
                    let picture = picture?;
                    let size = [picture.width as usize, picture.height as usize];
                    let rgb: Vec<u8> = picture.pixels.iter().flatten().copied().collect();
                    let image = egui::ColorImage::from_rgb(size, &rgb);
                    let name = format!("thumbnail-{:?}-{i}", pending.key);
                    let texture = ctx.load_texture(name, image, egui::TextureOptions::NEAREST);
                    Some((texture, egui::vec2(size[0] as f32, size[1] as f32)))
                })
                .collect();
            self.cache.insert(pending.key, pictures);
        }
        let asked = self.pending.as_ref().map(|p| p.key);
        if self.cache.contains_key(&key) || asked == Some(key) {
            return;
        }
        if let Some(pending) = self.pending.take() {
            pending.operation.cancel();
        }
        let Some((workspace, level, entries)) = start() else {
            return;
        };
        let (send, result) = mpsc::channel();
        let operation = Operation::default();
        let control = operation.clone();
        let ctx = ctx.clone();
        let background = [
            theme::PANEL_RAISED.r(),
            theme::PANEL_RAISED.g(),
            theme::PANEL_RAISED.b(),
        ];
        std::thread::spawn(move || {
            let objects: Vec<Object> = entries
                .iter()
                .filter_map(|p| match p {
                    Placing::Object(object) => Some(object.clone()),
                    Placing::Sprite(_) | Placing::Map16(_) => None,
                })
                .collect();
            let sprites: Vec<u8> = entries
                .iter()
                .filter_map(|p| match p {
                    Placing::Sprite(id) => Some(*id),
                    Placing::Object(_) | Placing::Map16(_) => None,
                })
                .collect();
            let pictures = if key.2 == Kind::Sprites {
                edit::sprite_previews(&workspace, key.0, &level, &sprites, &control)
            } else {
                edit::object_previews(&workspace, key.0, &level, &objects, background, &control)
            };
            let _ = send.send(pictures.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.pending = Some(Pending {
            key,
            operation,
            result,
        });
    }

    /// Forgets the pictures, after the project changed.
    pub fn clear(&mut self) {
        self.cache.clear();
        if let Some(pending) = self.pending.take() {
            pending.operation.cancel();
        }
    }
}

impl PaletteState {
    /// Lists `kind`.
    pub fn show(&mut self, kind: Kind) {
        self.kind = kind;
    }

    /// Whether thumbnails are being drawn.
    pub fn busy(&self) -> bool {
        self.thumbnails.pending.is_some()
    }

    /// Forgets the thumbnails, after the project changed.
    pub fn forget_pictures(&mut self) {
        self.thumbnails.clear();
    }
}

/// The numbers Lunar Magic's own objects use, which are placed by other
/// means than a palette.
const LUNAR_MAGIC_OBJECTS: std::ops::RangeInclusive<u8> = 0x22..=0x2D;

fn entries(kind: Kind, tileset: u8) -> Vec<(Placing, u8, &'static str)> {
    let used = |name: &&str| *name != "Unused";
    match kind {
        Kind::Objects => (0x01..=0x3Fu8)
            .filter(|n| !LUNAR_MAGIC_OBJECTS.contains(n))
            .filter_map(|n| {
                let name = names::standard_object(n, tileset).filter(used)?;
                let object = Object::Standard {
                    number: n,
                    x: 0,
                    y: 0,
                    settings: 0,
                };
                Some((Placing::Object(object), n, name))
            })
            .collect(),
        Kind::Extended => (0x04..=0xFFu8)
            .filter_map(|n| {
                let name = Some(names::extended_object(n)).filter(used)?;
                let object = Object::Extended {
                    number: n,
                    x: 0,
                    y: 0,
                };
                Some((Placing::Object(object), n, name))
            })
            .collect(),
        Kind::Map16 => Vec::new(),
        Kind::Sprites => (0x00..=0xFFu8)
            .filter_map(|n| {
                let name = Some(names::sprite(n)).filter(used)?;
                Some((Placing::Sprite(n), n, name))
            })
            .collect(),
    }
}

/// The size a thumbnail is shown at: whole multiples of its pixels where
/// it fits, within a box beside the name.
fn thumbnail_size(size: egui::Vec2) -> egui::Vec2 {
    let fit = (THUMBNAIL.x / size.x).min(THUMBNAIL.y / size.y);
    let scale = if fit >= 1.0 {
        fit.floor().min(2.0)
    } else {
        fit
    };
    size * scale
}

const THUMBNAIL: egui::Vec2 = egui::vec2(48.0, 32.0);

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = app.current() else {
        ui.label(RichText::new("Open a level to add to it.").color(theme::MUTED));
        return;
    };
    let tileset = open.document.level().header.object_tileset;
    let number = open.number;
    let level = open.document.level().clone();
    let workspace = app.workspace().cloned();
    let state = &mut app.palette;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.kind, Kind::Objects, "Objects");
        ui.selectable_value(&mut state.kind, Kind::Extended, "Extended");
        ui.selectable_value(&mut state.kind, Kind::Sprites, "Sprites");
        ui.selectable_value(&mut state.kind, Kind::Map16, "Map16");
    });
    if state.kind != Kind::Map16 {
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Find by name or number")
                .desired_width(f32::INFINITY),
        );
    }
    if let Some(placing) = &app.placing {
        ui.label(
            RichText::new(format!(
                "Placing {}: click the level; Esc stops.",
                placing.name(tileset)
            ))
            .small()
            .color(theme::ACCENT),
        );
    } else {
        ui.label(
            RichText::new("Choose one, then click the level to place it.")
                .small()
                .color(theme::MUTED),
        );
    }
    if matches!(level.layer2, kobo_core::source::level::Layer2::Objects(_))
        && state.kind != Kind::Sprites
    {
        ui.horizontal(|ui| {
            ui.label(RichText::new("On").color(theme::MUTED));
            ui.selectable_value(&mut app.place_layer, edit::ObjectLayer::One, "Layer 1");
            ui.selectable_value(&mut app.place_layer, edit::ObjectLayer::Two, "Layer 2");
        });
    }
    let state = &mut app.palette;
    let kind = state.kind;
    if kind == Kind::Map16 {
        map16(app, ui, number);
        return;
    }
    let state = &mut app.palette;
    let all = entries(kind, tileset);
    let set = if kind == Kind::Sprites {
        level.header.sprite_tileset
    } else {
        tileset
    };
    let key = (number, set, kind);
    state.thumbnails.update(ui.ctx(), key, || {
        let entries = all.iter().map(|(placing, _, _)| placing.clone()).collect();
        Some((workspace?, level, entries))
    });
    let drawing = state
        .thumbnails
        .pending
        .as_ref()
        .is_some_and(|p| p.key == key);
    if drawing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("Drawing them as this level would…")
                    .small()
                    .color(theme::MUTED),
            );
        });
    }
    if let Some(error) = &state.thumbnails.error {
        ui.label(
            RichText::new(format!("No pictures: {error}"))
                .small()
                .color(theme::ERROR),
        );
    }
    let filter = state.filter.trim().to_lowercase();
    let list: Vec<_> = all
        .into_iter()
        .enumerate()
        .filter(|(_, (_, n, name))| {
            filter.is_empty()
                || name.to_lowercase().contains(&filter)
                || format!("{n:02x}").contains(&filter)
        })
        .collect();
    let mut chosen = None;
    let placing_now = app.placing.clone();
    let thumbnails = &mut app.palette.thumbnails;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                for (i, (placing, n, name)) in list {
                    let selected = placing_now.as_ref() == Some(&placing);
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &format!("{n:02X}  "),
                        0.0,
                        egui::TextFormat::simple(egui::FontId::monospace(12.5), theme::ACCENT),
                    );
                    job.append(
                        name,
                        0.0,
                        egui::TextFormat::simple(egui::FontId::proportional(13.0), theme::TEXT),
                    );
                    let clicked = {
                        ui.horizontal(|ui| {
                            let (rect, image) =
                                ui.allocate_exact_size(THUMBNAIL, egui::Sense::click());
                            if let Some((texture, size)) = thumbnails.get(key, i) {
                                let shown = egui::Rect::from_center_size(
                                    rect.center(),
                                    thumbnail_size(*size),
                                );
                                ui.painter().image(
                                    texture.id(),
                                    shown,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            }
                            let label = ui.selectable_label(selected, job);
                            image.clicked() || label.clicked()
                        })
                        .inner
                    };
                    if clicked {
                        chosen = Some(if selected { None } else { Some(placing) });
                    }
                }
            });
        });
    if let Some(choice) = chosen {
        app.placing = choice;
    }
}

/// The Map16 tab: a page of the level's own Map16 tiles, as its load
/// resolved them (pages 0 to 3, and those its tiles use), to place
/// directly as Lunar Magic's direct Map16 objects.
fn map16(app: &mut App, ui: &mut egui::Ui, number: u16) {
    let placing = app.placing.clone();
    let (open, state) = app.level_and_palette(number);
    let Some(open) = open else { return };
    let Some(geometry) = &open.geometry else {
        ui.label(RichText::new("Once the level is drawn.").color(theme::MUTED));
        return;
    };
    let tiles = &geometry.loaded.tiles;
    let definitions = tiles.foreground_map16();
    let pages: Vec<u16> = (0..definitions.len() / 0x100)
        .filter(|p| {
            definitions[p * 0x100..(p + 1) * 0x100]
                .iter()
                .any(Option::is_some)
        })
        .map(|p| p as u16)
        .collect();
    let shown = open.shown;
    if !pages.contains(&state.map16_page) {
        state.map16_page = pages.first().copied().unwrap_or(0);
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Page").color(theme::MUTED));
        for &page in &pages {
            ui.selectable_value(&mut state.map16_page, page, format!("{page:02X}"));
        }
    });
    let page = state.map16_page;
    let key = (shown, page);
    if state.map16_sheet.as_ref().is_none_or(|(k, _)| *k != key) {
        let start = usize::from(page) * 0x100;
        let layer_tiles = kobo_core::render::LayerTiles::from_vram(&geometry.loaded.video.vram);
        let palette = geometry.loaded.video.palette();
        let background = [
            theme::BACKGROUND.r(),
            theme::BACKGROUND.g(),
            theme::BACKGROUND.b(),
        ];
        let sheet = kobo_core::render::map16_sheet(
            &definitions[start..(start + 0x100).min(definitions.len())],
            &layer_tiles,
            &palette,
            background,
            16,
        );
        let rgb: Vec<u8> = sheet.pixels.iter().flatten().copied().collect();
        let image = egui::ColorImage::from_rgb([sheet.width as usize, sheet.height as usize], &rgb);
        let texture = ui
            .ctx()
            .load_texture("map16-page", image, egui::TextureOptions::NEAREST);
        state.map16_sheet = Some((key, texture));
    }
    ui.label(
        RichText::new(
            "Placed as Lunar Magic's direct Map16 objects; a build installs Kobo's code for them.",
        )
        .small()
        .color(theme::MUTED),
    );
    let Some((_, texture)) = &state.map16_sheet else {
        return;
    };
    let scale = ((ui.available_width() - 4.0) / 256.0).clamp(1.0, 2.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(256.0, 256.0) * scale, egui::Sense::click());
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter()
        .image(texture.id(), rect, uv, egui::Color32::WHITE);
    let cell = 16.0 * scale;
    let tile_at = |at: egui::Pos2| {
        let local = (at - rect.min) / cell;
        (local.x >= 0.0 && local.y >= 0.0 && local.x < 16.0 && local.y < 16.0)
            .then(|| page * 0x100 + local.y as u16 * 16 + local.x as u16)
    };
    let mark = |tile: u16, color: egui::Color32| {
        let i = tile - page * 0x100;
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2(f32::from(i % 16), f32::from(i / 16)) * cell,
            egui::vec2(cell, cell),
        );
        ui.painter().rect_stroke(
            r,
            0,
            egui::Stroke::new(2.0, color),
            egui::StrokeKind::Inside,
        );
    };
    if let Some(Placing::Map16(tile)) = &placing
        && tile >> 8 == page
    {
        mark(*tile, theme::ACCENT);
    }
    if let Some(tile) = response.hover_pos().and_then(tile_at) {
        mark(tile, egui::Color32::WHITE);
        let defined = definitions
            .get(usize::from(tile))
            .is_some_and(Option::is_some);
        let text = if defined {
            format!("Map16 tile {tile:03X}")
        } else {
            format!("Map16 tile {tile:03X}: not defined")
        };
        response.clone().on_hover_text_at_pointer(text);
    }
    if response.clicked()
        && let Some(tile) = response.interact_pointer_pos().and_then(tile_at)
    {
        app.placing = Some(Placing::Map16(tile));
    }
}
