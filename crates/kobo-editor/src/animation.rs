//! The inspector's ExAnimation section: which of the game's and Lunar
//! Magic's animations run in the level, and its list's slots (type,
//! trigger, frames, where they go, and the frames' words), each change an
//! `Edit::SetAnimation`. `source::animation` documents what the fields
//! mean; a list a build would refuse is refused here too, with why.

use eframe::egui::{self, DragValue, Grid, Response, RichText};
use kobo_core::edit::Edit;
use kobo_core::exanimation::{FIRST_ALT_FILE, Kind, List, SLOTS, Settings, Slot, second_set};
use kobo_core::names;
use kobo_core::source::level::Level;

use crate::inspector::named_picker;
use crate::theme;

/// What the section asks for: the widget, a label, and the edit.
pub type Asked = (Response, String, Edit);

fn set(level: &Level, list: Option<List>) -> Edit {
    Edit::SetAnimation {
        settings: level.animation_settings,
        list: list.map(Box::new),
    }
}

pub fn section(ui: &mut egui::Ui, number: u16, level: &Level) -> Option<Asked> {
    let mut asked = None;
    // The settings: what a build gives the level until one is changed.
    let settings = Settings(
        level
            .animation_settings
            .unwrap_or(Settings::default_for(number).0),
    );
    ui.horizontal_wrapped(|ui| {
        for (flag, text, tip) in [
            (
                Settings::NO_GAME_TILES,
                "Game's tiles",
                "The game's own animated tiles (coins, question blocks, water)",
            ),
            (
                Settings::NO_GAME_COLOURS,
                "Game's colours",
                "The game's flashing colour 64",
            ),
            (
                Settings::NO_LEVEL,
                "Level's list",
                "This level's ExAnimation, below",
            ),
            (
                Settings::NO_GLOBAL,
                "Global list",
                "The project's global ExAnimation",
            ),
        ] {
            let mut on = settings.on(flag);
            let r = ui.checkbox(&mut on, text).on_hover_text(tip);
            if r.changed() {
                let byte = if on {
                    settings.0 & !flag
                } else {
                    settings.0 | flag
                };
                let edit = Edit::SetAnimation {
                    settings: Some(byte),
                    list: level.animation.clone().map(Box::new),
                };
                asked = Some((
                    r,
                    format!("Turn {}", if on { "on" } else { "off" }) + " " + &text.to_lowercase(),
                    edit,
                ));
            }
        }
    });
    let Some(list) = &level.animation else {
        let r = ui.button("Give the level an ExAnimation list");
        if r.clicked() {
            asked = Some((
                r,
                "Add an ExAnimation list".into(),
                set(level, Some(List::default())),
            ));
        }
        return asked;
    };

    Grid::new("animation-list")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Alternative file").on_hover_text("The uncompressed ExGFX file (60-63) that slots marked alt take their frames from");
            let mut file = u16::from(list.alt_file) + FIRST_ALT_FILE;
            let r = ui.add(
                DragValue::new(&mut file)
                    .range(FIRST_ALT_FILE..=FIRST_ALT_FILE + 3)
                    .speed(0.05)
                    .hexadecimal(2, false, true),
            );
            if r.changed() {
                let new = List {
                    alt_file: (file - FIRST_ALT_FILE) as u8,
                    ..list.clone()
                };
                asked = Some((r, "Change the alternative file".into(), set(level, Some(new))));
            }
            ui.end_row();
            for (text, tip, value, which) in [
                ("Custom kept", "The custom triggers (bits 0 to F) that keep their state when the level loads; the rest are cleared", list.custom_keep, 0),
                ("Custom set", "The custom triggers the level's load sets", list.custom_set, 1),
            ] {
                ui.label(text).on_hover_text(tip);
                let mut v = value;
                let r = ui.add(DragValue::new(&mut v).speed(1.0).hexadecimal(4, false, true));
                if r.changed() {
                    let mut new = list.clone();
                    if which == 0 {
                        new.custom_keep = v;
                    } else {
                        new.custom_set = v;
                    }
                    asked = Some((r, format!("Change {}", text.to_lowercase()), set(level, Some(new))));
                }
                ui.end_row();
            }
        });

    for (&n, slot) in &list.slots {
        let kind = names::exanimation_type(slot.kind).unwrap_or("?");
        let trigger = names::exanimation_trigger(slot.trigger).unwrap_or("?");
        egui::CollapsingHeader::new(format!("Slot {n:02X} · {kind}"))
            .id_salt(("animation-slot", n))
            .show(ui, |ui| {
                ui.label(RichText::new(trigger).small().color(theme::MUTED));
                if let Some(a) = slot_fields(ui, n, slot) {
                    let (r, label, new_slot) = a;
                    let mut new = list.clone();
                    new.slots.insert(n, new_slot);
                    asked = Some((r, label, set(level, Some(new))));
                }
                let r = ui.small_button("Remove the slot");
                if r.clicked() {
                    let mut new = list.clone();
                    new.slots.remove(&n);
                    new.count = new.slots_used();
                    asked = Some((
                        r,
                        format!("Remove ExAnimation slot {n:02X}"),
                        set(level, Some(new)),
                    ));
                }
            });
    }
    ui.horizontal(|ui| {
        let free = (0..SLOTS as u8).find(|n| !list.slots.contains_key(n));
        let r = ui.add_enabled(free.is_some(), egui::Button::new("Add a slot"));
        if r.clicked()
            && let Some(n) = free
        {
            let mut new = list.clone();
            // One 8x8 tile at the start of layer 1's tiles, two frames of
            // RAM: a slot to change from.
            new.slots.insert(
                n,
                Slot {
                    kind: 0x01,
                    trigger: 0x00,
                    frames_less_one: 1,
                    dest: 0x0000,
                    frames: vec![0xAD00, 0xAD20],
                },
            );
            new.count = new.count.max(new.slots_used());
            asked = Some((
                r,
                format!("Add ExAnimation slot {n:02X}"),
                set(level, Some(new)),
            ));
        }
        let r = ui.button("No list");
        if r.clicked() {
            asked = Some((r, "Remove the ExAnimation list".into(), set(level, None)));
        }
    });
    if let Some(why) = kobo_core::exanimation::refusal(list) {
        ui.label(RichText::new(format!("A build refuses this list: {why}.")).color(theme::WARNING));
    }
    asked
}

/// A slot's fields; the slot as changed, with its frames refitted.
fn slot_fields(ui: &mut egui::Ui, n: u8, slot: &Slot) -> Option<(Response, String, Slot)> {
    let mut asked = None;
    let refit = |changed: Slot| {
        let mut changed = changed;
        changed.refit_frames(slot);
        changed
    };
    Grid::new(("animation-slot-fields", n))
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label("Type");
            let (r, picked) = named_picker(ui, &format!("animation-type-{n}"), slot.kind, 0x01..=0x1B, names::exanimation_type);
            if let Some(kind) = picked {
                asked = Some((r, "Change an ExAnimation type".into(), refit(Slot { kind, ..slot.clone() })));
            }
            ui.end_row();
            ui.label("Trigger");
            let (r, picked) = named_picker(ui, &format!("animation-trigger-{n}"), slot.trigger, 0x00..=0x4F, names::exanimation_trigger);
            if let Some(trigger) = picked {
                asked = Some((r, "Change an ExAnimation trigger".into(), refit(Slot { trigger, ..slot.clone() })));
            }
            ui.end_row();
            let kind = Kind::of(slot.kind);
            let (count_name, count_tip) = if kind == Some(Kind::Rotation) {
                ("Delay", "How many frames it waits between turns")
            } else {
                ("Frames", "How many frames it has")
            };
            ui.label(count_name).on_hover_text(count_tip);
            let mut count = u16::from(slot.frames_less_one) + 1;
            let r = ui.add(DragValue::new(&mut count).range(1..=256).speed(0.1));
            if r.changed() {
                let changed = Slot {
                    frames_less_one: (count - 1) as u8,
                    ..slot.clone()
                };
                asked = Some((r, format!("Change an ExAnimation {}", count_name.to_lowercase()), refit(changed)));
            }
            ui.end_row();
            match kind {
                Some(Kind::Tiles) => {
                    ui.label("VRAM").on_hover_text("The word address its tiles go to");
                    let mut vram = slot.dest & 0x7FFF;
                    let r = ui.add(DragValue::new(&mut vram).range(0..=0x7FFF).speed(1.0).hexadecimal(4, false, true));
                    if r.changed() {
                        let changed = Slot { dest: slot.dest & 0x8000 | vram, ..slot.clone() };
                        asked = Some((r, "Change where an ExAnimation goes".into(), changed));
                    }
                    ui.end_row();
                }
                _ => {
                    ui.label("Colour").on_hover_text("The first colour it changes, 00 to FF");
                    let mut colour = slot.dest & 0xFF;
                    let r = ui.add(DragValue::new(&mut colour).range(0..=0xFF).speed(0.1).hexadecimal(2, false, true));
                    if r.changed() {
                        let changed = Slot { dest: slot.dest & 0xFF00 | colour, ..slot.clone() };
                        asked = Some((r, "Change an ExAnimation's colour".into(), changed));
                    }
                    ui.end_row();
                    ui.label("Colours");
                    let mut colours = slot.colours();
                    let r = ui.add(DragValue::new(&mut colours).range(1..=128).speed(0.1));
                    if r.changed() {
                        let changed = Slot { dest: slot.dest & 0x80FF | (colours - 1) << 8, ..slot.clone() };
                        asked = Some((r, "Change an ExAnimation's colours".into(), changed));
                    }
                    ui.end_row();
                }
            }
            if kind != Some(Kind::Rotation) {
                ui.label("Alt").on_hover_text("Its frames are offsets into the alternative file, not RAM addresses");
                let mut alt = slot.alternative();
                let r = ui.checkbox(&mut alt, "From the alternative file");
                if r.changed() {
                    let changed = Slot { dest: slot.dest & 0x7FFF | u16::from(alt) << 15, ..slot.clone() };
                    asked = Some((r, "Change where an ExAnimation's frames are".into(), changed));
                }
                ui.end_row();
                let per_set = usize::from(slot.frames_less_one) + 1;
                let sets = if second_set(slot.trigger) { 2 } else { 1 };
                for set in 0..sets.min(slot.frames.len() / per_set.max(1)) {
                    let (name, tip) = if set == 0 {
                        ("Frame words", "A word per frame: a RAM address in bank 7E, an offset into the alternative file, or a colour")
                    } else {
                        ("Triggered", "The frames while the trigger is on")
                    };
                    ui.label(name).on_hover_text(tip);
                    let words = &slot.frames[set * per_set..(set + 1) * per_set];
                    if let Some((r, new)) = words_field(ui, (n, set), words) {
                        let mut frames = slot.frames.clone();
                        frames[set * per_set..(set + 1) * per_set].copy_from_slice(&new);
                        asked = Some((r, "Change an ExAnimation's frames".into(), Slot { frames, ..slot.clone() }));
                    }
                    ui.end_row();
                }
            }
        });
    asked
}

/// Words as hex to edit, the same count as before; the new words once the
/// field is left or Enter pressed, if they read as that many.
fn words_field(ui: &mut egui::Ui, id: (u8, usize), words: &[u16]) -> Option<(Response, Vec<u16>)> {
    let id = ui.id().with(("animation-words", id));
    let shown = words
        .iter()
        .map(|w| format!("{w:04X}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let mut response = ui.add(
        egui::TextEdit::multiline(&mut text)
            .font(egui::TextStyle::Monospace)
            .desired_rows(1)
            .desired_width(180.0),
    );
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    let parsed: Option<Vec<u16>> = text
        .split_whitespace()
        .map(|w| u16::from_str_radix(w, 16).ok())
        .collect::<Option<Vec<u16>>>()
        .filter(|new| new.len() == words.len());
    if parsed.is_none() && text != shown {
        response =
            response.on_hover_text(format!("{} words in hex, such as AD00 AD20", words.len()));
    }
    let new = parsed.filter(|new| response.lost_focus() && new.as_slice() != words)?;
    response.mark_changed();
    Some((response, new))
}
