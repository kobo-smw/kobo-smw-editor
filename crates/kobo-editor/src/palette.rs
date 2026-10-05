//! The palette: objects and sprites to add to the level. Choosing one
//! puts the canvas in placing mode, where each click places one, until
//! Escape or a right click.

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
}

impl Placing {
    pub fn name(&self, tileset: u8) -> String {
        match self {
            Placing::Object(object) => names::object(object, tileset)
                .unwrap_or("Object")
                .to_string(),
            Placing::Sprite(id) => names::sprite(*id).to_string(),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Kind {
    #[default]
    Objects,
    Extended,
    Sprites,
}

#[derive(Default)]
pub struct PaletteState {
    kind: Kind,
    filter: String,
    thumbnails: Thumbnails,
}

/// The objects' pictures as the open level draws them, made on a worker
/// thread (`edit::object_previews`) for one level, tileset, and list at a
/// time.
#[derive(Default)]
struct Thumbnails {
    /// What the pictures are of: level, tileset, list.
    key: Option<(u16, u8, Kind)>,
    pictures: Vec<Option<(egui::TextureHandle, egui::Vec2)>>,
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
    /// The picture of entry `i`, asking for the list's when they are not
    /// for this level and list yet.
    fn get(
        &mut self,
        app_key: (u16, u8, Kind),
        i: usize,
    ) -> Option<&(egui::TextureHandle, egui::Vec2)> {
        if self.key != Some(app_key) {
            return None;
        }
        self.pictures.get(i)?.as_ref()
    }

    fn update(
        &mut self,
        ctx: &egui::Context,
        key: (u16, u8, Kind),
        start: impl FnOnce() -> Option<(Workspace, Level, Vec<Object>)>,
    ) {
        if let Some(pending) = &self.pending
            && let Ok(result) = pending.result.try_recv()
        {
            let pending = self.pending.take().expect("checked");
            self.error = result.as_ref().err().cloned();
            if let Ok(pictures) = result {
                self.pictures = pictures
                    .into_iter()
                    .enumerate()
                    .map(|(i, picture)| {
                        let picture = picture?;
                        let size = [picture.width as usize, picture.height as usize];
                        let rgb: Vec<u8> = picture.pixels.iter().flatten().copied().collect();
                        let image = egui::ColorImage::from_rgb(size, &rgb);
                        let texture = ctx.load_texture(
                            format!("thumbnail-{i}"),
                            image,
                            egui::TextureOptions::NEAREST,
                        );
                        Some((texture, egui::vec2(size[0] as f32, size[1] as f32)))
                    })
                    .collect();
            } else {
                self.pictures.clear();
            }
            self.key = Some(pending.key);
        }
        let asked = self.pending.as_ref().map(|p| p.key);
        // Sprites have no pictures yet.
        if key.2 == Kind::Sprites || self.key == Some(key) || asked == Some(key) {
            return;
        }
        if let Some(pending) = self.pending.take() {
            pending.operation.cancel();
        }
        let Some((workspace, level, objects)) = start() else {
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
            let pictures =
                edit::object_previews(&workspace, key.0, &level, &objects, background, &control)
                    .map_err(|e| e.to_string());
            let _ = send.send(pictures);
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
        self.key = None;
        self.pictures.clear();
    }
}

impl PaletteState {
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
    });
    ui.add(
        egui::TextEdit::singleline(&mut state.filter)
            .hint_text("Find by name or number")
            .desired_width(f32::INFINITY),
    );
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
    let kind = state.kind;
    let all = entries(kind, tileset);
    let key = (number, tileset, kind);
    state.thumbnails.update(ui.ctx(), key, || {
        let objects = all
            .iter()
            .filter_map(|(placing, _, _)| match placing {
                Placing::Object(object) => Some(object.clone()),
                Placing::Sprite(_) => None,
            })
            .collect();
        Some((workspace?, level, objects))
    });
    let drawing = state.thumbnails.pending.is_some() && kind != Kind::Sprites;
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
                    let clicked = if kind == Kind::Sprites {
                        ui.selectable_label(selected, job).clicked()
                    } else {
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
