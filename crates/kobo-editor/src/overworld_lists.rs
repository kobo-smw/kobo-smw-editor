//! The overworld window's lists: the sprite list (`OverworldSprites`, 13
//! slots of a sprite and its place in pixels), the reveal list (the layer
//! 1 tiles events turn into others), the crushed tiles, and the further
//! tiles of the event shown. Each change is the overworld as changed, for
//! `edit::OverworldDocument::change`.

use eframe::egui::{self, DragValue, Grid, RichText};
use kobo_core::overworld::{
    EventBlock, ExtraTile, Overworld, TABLES, event_vram, layer1_index, layer1_place, layer2_index,
};

use crate::inspector::named_picker;
use crate::theme;

/// A change of the overworld: its undo label, and the overworld after it.
pub struct Edited {
    pub label: String,
    pub to: Box<Overworld>,
}

fn edited(label: impl Into<String>, to: Overworld) -> Option<Edited> {
    Some(Edited {
        label: label.into(),
        to: Box::new(to),
    })
}

/// The sprite list's slots: a sprite number, then its x and y in pixels.
const SPRITE_SLOTS: usize = 13;
const SPRITE_BYTES: usize = 5;

/// The sprite list, the tile chosen (by map, x, and y) offered as a place
/// on the main map.
pub fn sprites(
    ui: &mut egui::Ui,
    overworld: &Overworld,
    chosen: Option<(u8, u8, u8)>,
) -> Option<Edited> {
    let table = TABLES.iter().position(|t| t.name == "sprites")?;
    let bytes = &overworld.tables[table];
    if bytes.len() < SPRITE_SLOTS * SPRITE_BYTES {
        return None;
    }
    let mut out = None;
    let mut set = |slot: usize, kind: u8, x: u16, y: u16, label: &str| {
        let mut to = overworld.clone();
        let at = SPRITE_BYTES * slot;
        let row = &mut to.tables[table][at..at + SPRITE_BYTES];
        row[0] = kind;
        row[1..3].copy_from_slice(&x.to_le_bytes());
        row[3..5].copy_from_slice(&y.to_le_bytes());
        out = edited(label, to);
    };
    ui.label(
        RichText::new("Most sprites show on the main map alone; places are in pixels.")
            .small()
            .color(theme::MUTED),
    );
    Grid::new("overworld-sprites")
        .num_columns(5)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for slot in 0..SPRITE_SLOTS {
                let at = SPRITE_BYTES * slot;
                let kind = bytes[at];
                let x = u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]);
                let y = u16::from_le_bytes([bytes[at + 3], bytes[at + 4]]);
                ui.label(format!("{slot:X}"));
                let (_, picked) = named_picker(
                    ui,
                    &format!("overworld-sprite-{slot}"),
                    kind,
                    0x00..=0x0A,
                    kobo_core::names::overworld_sprite,
                );
                if let Some(kind) = picked {
                    set(slot, kind, x, y, "Change an overworld sprite");
                }
                let (mut nx, mut ny) = (x, y);
                let rx = ui.add(DragValue::new(&mut nx).hexadecimal(4, false, true));
                let ry = ui.add(DragValue::new(&mut ny).hexadecimal(4, false, true));
                if (rx.changed() || ry.changed()) && (nx, ny) != (x, y) {
                    set(slot, kind, nx, ny, "Move an overworld sprite");
                }
                match chosen {
                    Some((0, tx, ty)) => {
                        let here = (u16::from(tx) * 16, u16::from(ty) * 16);
                        if ui
                            .add_enabled(here != (x, y), egui::Button::new("Here").small())
                            .on_hover_text("To the tile chosen on the main map")
                            .clicked()
                        {
                            set(slot, kind, here.0, here.1, "Move an overworld sprite");
                        }
                    }
                    _ => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    out
}

/// The reveal list: the layer 1 tile an event's layer 1 tile is, and the
/// one it turns into.
pub fn reveal(ui: &mut egui::Ui, overworld: &Overworld) -> Option<Edited> {
    let mut out = None;
    Grid::new("overworld-reveal")
        .num_columns(3)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label("From");
            ui.label("To");
            ui.end_row();
            for (i, &(from, to)) in overworld.events.reveal.iter().enumerate() {
                ui.label(format!("{i:X}"));
                let (mut f, mut t) = (from, to);
                let rf = ui.add(DragValue::new(&mut f).hexadecimal(2, false, true));
                let rt = ui.add(DragValue::new(&mut t).hexadecimal(2, false, true));
                if (rf.changed() || rt.changed()) && (f, t) != (from, to) {
                    let mut next = overworld.clone();
                    next.events.reveal[i] = (f, t);
                    out = edited("Change the reveal list", next);
                }
                ui.end_row();
            }
        });
    out
}

/// The crushed tiles: an event, and the layer 1 tile it crushes, the tile
/// chosen offered.
pub fn crush(
    ui: &mut egui::Ui,
    overworld: &Overworld,
    chosen: Option<(u8, u8, u8)>,
) -> Option<Edited> {
    let mut out = None;
    Grid::new("overworld-crush")
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label("Event");
            ui.label("Tile");
            ui.end_row();
            for (i, c) in overworld.events.crush.iter().enumerate() {
                ui.label(format!("{i:X}"));
                let mut event = c.event;
                if ui
                    .add(
                        DragValue::new(&mut event)
                            .range(0..=0x77)
                            .hexadecimal(2, false, true),
                    )
                    .changed()
                    && event != c.event
                {
                    let mut next = overworld.clone();
                    next.events.crush[i].event = event;
                    out = edited("Change a crushed tile's event", next);
                }
                let (m, x, y) = layer1_place(usize::from(c.place));
                ui.label(format!(
                    "{} {x},{y}",
                    if m == 0 { "main" } else { "submaps" }
                ));
                if let Some((cm, cx, cy)) = chosen {
                    let place = layer1_index(cm, cx, cy) as u16;
                    if ui
                        .add_enabled(place != c.place, egui::Button::new("Here").small())
                        .on_hover_text("The tile chosen on the map")
                        .clicked()
                    {
                        let mut next = overworld.clone();
                        next.events.crush[i].place = place;
                        next.events.crush[i].vram = event_vram(cx, cy);
                        out = edited("Move a crushed tile", next);
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    out
}

/// The further tiles of event `e`, in the order it makes them: removed one
/// by one, a layer 1 tile added at the 16x16 tile chosen, of the brush, and
/// a layer 2 block at the 8x8 tile chosen (`chosen2`), of the tiles there.
pub fn extras(
    ui: &mut egui::Ui,
    overworld: &Overworld,
    e: u8,
    chosen: Option<(u8, u8, u8)>,
    chosen2: Option<(u8, u8, u8)>,
    brush: u16,
) -> Option<Edited> {
    let mut out = None;
    let event = &overworld.event_list()[usize::from(e)];
    let mut with = |label: &str, extras: Vec<ExtraTile>| {
        let mut next = overworld.clone();
        let mut to = event.clone();
        to.extras = extras;
        // A change the event tile data cannot hold is left out.
        if next.set_event(usize::from(e), to).is_ok() {
            out = edited(label, next);
        }
    };
    for (i, extra) in event.extras.iter().enumerate() {
        ui.horizontal(|ui| {
            let text = match extra {
                ExtraTile::Layer1 { place, tile } => {
                    let (m, x, y) = layer1_place(usize::from(*place));
                    format!(
                        "Layer 1 tile {tile:03X} at {} {x},{y}",
                        if m == 0 { "main" } else { "submaps" }
                    )
                }
                ExtraTile::Layer2(block) => {
                    format!(
                        "Layer 2 block of {} tiles at {:04X}",
                        block.tiles.len(),
                        block.place
                    )
                }
            };
            ui.label(text);
            if ui.small_button("Remove").clicked() {
                let mut extras = event.extras.clone();
                extras.remove(i);
                with("Remove a further tile", extras);
            }
        });
    }
    if let Some((m, x, y)) = chosen
        && ui
            .button(format!("Add layer 1 tile {brush:03X} here"))
            .on_hover_text("A further tile: the brush's tile set outright at the tile chosen when the event is made")
            .clicked()
    {
        let mut extras = event.extras.clone();
        extras.push(ExtraTile::Layer1 {
            place: layer1_index(m, x, y) as u16,
            tile: brush,
        });
        with("Add a further tile", extras);
    }
    if let Some((m, x, y)) = chosen2 {
        ui.horizontal(|ui| {
            for side in [2usize, 6] {
                if ui
                    .button(format!("Add a {side}x{side} further block here"))
                    .on_hover_text(
                        "A further layer 2 block from the 8x8 tile chosen, of the tiles there now",
                    )
                    .clicked()
                {
                    let mut block = EventBlock {
                        place: (layer2_index(m, x, y) * 2) as u16,
                        tiles: vec![0; side * side],
                    };
                    let offsets = block.offsets();
                    for (tile, offset) in block.tiles.iter_mut().zip(offsets) {
                        *tile = overworld.layer2[usize::from(offset) / 2];
                    }
                    let mut extras = event.extras.clone();
                    extras.push(ExtraTile::Layer2(block));
                    with("Add a further block", extras);
                }
            }
        });
    }
    out
}
