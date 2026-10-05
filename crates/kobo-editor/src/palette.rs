//! The palette: objects and sprites to add to the level. Choosing one
//! puts the canvas in placing mode, where each click places one, until
//! Escape or a right click.

use eframe::egui::{self, RichText};
use kobo_core::level::objects::Object;
use kobo_core::names;

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

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = app.current() else {
        ui.label(RichText::new("Open a level to add to it.").color(theme::MUTED));
        return;
    };
    let tileset = open.document.level().header.object_tileset;
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
    let filter = state.filter.trim().to_lowercase();
    let list: Vec<_> = entries(state.kind, tileset)
        .into_iter()
        .filter(|(_, n, name)| {
            filter.is_empty()
                || name.to_lowercase().contains(&filter)
                || format!("{n:02x}").contains(&filter)
        })
        .collect();
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                for (placing, n, name) in list {
                    let selected = app.placing.as_ref() == Some(&placing);
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
                    if ui.selectable_label(selected, job).clicked() {
                        chosen = Some(if selected { None } else { Some(placing) });
                    }
                }
            });
        });
    if let Some(choice) = chosen {
        app.placing = choice;
    }
}
