//! The inspector: what is selected, or the level's header when nothing
//! is, as fields that edit it.

use eframe::egui::{self, DragValue, Grid, Response, RichText};
use kobo_core::edit::{self, Edit, ObjectLayer};
use kobo_core::entrance::LevelSettings;
use kobo_core::level::objects::Object;
use kobo_core::level::objects::ScreenExit;
use kobo_core::level::{LevelMode, PrimaryHeader, SecondaryHeader};
use kobo_core::names;
use kobo_core::source::level::{Entrance, Level, Sprite};

use crate::app::App;
use crate::canvas;
use crate::selection::{self, Item, objects};
use crate::theme;

/// One change a field asks for: an undo step of its own, or, while its
/// value is being dragged, part of the step the drag began.
struct Change {
    widget: egui::Id,
    dragging: bool,
    label: String,
    edits: Vec<Edit>,
    /// What to select after, when the change gives it a new number.
    select: Option<Item>,
}

impl Change {
    fn new(response: &Response, label: impl Into<String>, edits: Vec<Edit>) -> Self {
        Self {
            widget: response.id,
            dragging: response.dragged() && !response.drag_started(),
            label: label.into(),
            edits,
            select: None,
        }
    }

    fn selecting(mut self, item: Item) -> Self {
        self.select = Some(item);
        self
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = app.current() else {
        return;
    };
    let level = open.document.level().clone();
    let number = open.number;
    let selection = open.selection.clone();
    let diagnostics = open.diagnostics.clone();
    let mut change: Option<Change> = None;
    let mut delete = false;
    let mut level_action = None;
    let mut copy_to = app.copy_to;
    let taken: Vec<bool> = (0..0x200).map(|n| app.has_level(n)).collect();
    let free_entrance = app.workspace().and_then(|w| w.free_entrance(number));
    // Propose the first free number after this level.
    if taken[usize::from(copy_to) & 0x1FF] {
        copy_to = (1..0x200u16)
            .map(|d| (number + d) & 0x1FF)
            .find(|&n| !taken[usize::from(n)])
            .unwrap_or(copy_to);
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match selection[..] {
                [] => {
                    header(ui, &level, &mut change);
                    entrances(ui, &level, free_entrance, &mut change);
                    level_action = copy_level(ui, &mut copy_to, &taken);
                }
                [Item::Object(o)] => object(ui, &level, number, o, &mut change),
                [Item::Sprite(i)] => sprite(ui, &level, i, &mut change),
                _ => {
                    heading(ui, &format!("{} selected", selection.len()), "");
                    ui.label(
                        RichText::new("Drag them on the canvas, or use the arrow keys, to move them together.")
                            .color(theme::MUTED),
                    );
                }
            }
            if !selection.is_empty() {
                ui.add_space(8.0);
                delete = ui.button("Delete").on_hover_text("Delete (Del)").clicked();
            }
            ui.add_space(12.0);
            section(ui, "Diagnostics");
            if diagnostics.is_empty() {
                ui.label(RichText::new("None: the level loaded and drew in full.").color(theme::MUTED));
            } else {
                for line in &diagnostics {
                    ui.label(RichText::new(line).color(theme::WARNING));
                }
            }
        });

    app.copy_to = copy_to;
    match level_action {
        Some(LevelAction::Copy) => app.add_level(copy_to, &edit::copy_of(&level)),
        Some(LevelAction::Empty) => {
            // The open level's header and settings, and the secondary
            // entrances the game's own level there has, which lead to it.
            let mut empty = edit::empty_of(&level);
            empty.entrances = app
                .workspace()
                .and_then(|w| w.clean_level(copy_to).ok())
                .map(|l| l.entrances)
                .unwrap_or_default();
            app.add_level(copy_to, &empty);
        }
        Some(LevelAction::Remove) => app.removing = Some(number),
        None => {}
    }
    if delete {
        let label = if selection.len() == 1 {
            "Delete".to_string()
        } else {
            format!("Delete {} items", selection.len())
        };
        if app.apply(&label, canvas::delete_edits(&selection))
            && let Some(open) = app.current_mut()
        {
            open.selection.clear();
        }
    }
    if let Some(change) = change {
        let applied = if change.dragging && app.editing == Some(change.widget) {
            app.amend(change.edits);
            true
        } else if app.apply(&change.label, change.edits) {
            app.editing = Some(change.widget);
            true
        } else {
            false
        };
        if applied
            && let Some(item) = change.select
            && let Some(open) = app.current_mut()
        {
            open.selection = vec![item];
        }
    }
}

fn heading(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).size(16.0).strong());
    if !detail.is_empty() {
        ui.label(RichText::new(detail).monospace().color(theme::ACCENT));
    }
    ui.add_space(6.0);
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .small()
            .color(theme::MUTED),
    );
    ui.separator();
}

/// A whole-number field; returns its response and whether it changed.
fn number_field(ui: &mut egui::Ui, value: &mut u16, min: u16, max: u16, hex: bool) -> Response {
    let mut field = DragValue::new(value).range(min..=max).speed(0.1);
    if hex {
        field = field.hexadecimal(2, false, true);
    }
    ui.add(field)
}

fn object(
    ui: &mut egui::Ui,
    level: &Level,
    number: u16,
    o: kobo_core::expand::ObjectRef,
    change: &mut Option<Change>,
) {
    let Some(object) = objects(level, o.layer).and_then(|l| l.get(o.index)) else {
        return;
    };
    let name = selection::describe(level, Item::Object(o));
    let layer = match o.layer {
        ObjectLayer::One => "layer 1",
        ObjectLayer::Two => "layer 2",
    };
    let kind = match object {
        Object::Standard { number, .. } => format!("object {number:02X}"),
        Object::Extended { number, .. } => format!("extended object {number:02X}"),
        Object::ScreenExit(_) => "screen exit".to_string(),
        Object::Lunar { number, .. } => format!("Lunar Magic object {number:02X}"),
        Object::Unplaced(_) => "unplaced object".to_string(),
    };
    heading(ui, &name, &format!("{kind} · {layer} · #{}", o.index));
    let (width, height) = edit::layer_size(level, o.layer);
    let replace = |object: Object| Edit::ReplaceObject {
        layer: o.layer,
        index: o.index,
        object,
    };
    Grid::new("object")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            if let Some((x, y)) = edit::object_position(object) {
                let (mut nx, mut ny) = (x, y);
                ui.label("X");
                let r = number_field(ui, &mut nx, 0, width.saturating_sub(1), false);
                if r.changed() {
                    *change = Some(Change::new(
                        &r,
                        "Move object",
                        vec![replace(edit::object_at(object, nx, y))],
                    ));
                }
                ui.end_row();
                ui.label("Y");
                let r = number_field(ui, &mut ny, 0, height.saturating_sub(1), false);
                if r.changed() {
                    *change = Some(Change::new(
                        &r,
                        "Move object",
                        vec![replace(edit::object_at(object, x, ny))],
                    ));
                }
                ui.end_row();
            }
            match object {
                Object::Standard {
                    number: n,
                    settings,
                    ..
                } => {
                    for field in edit::setting_fields(*n, *settings) {
                        let mut value = field.value;
                        ui.label(capitalised(field.name));
                        let r = number_field(
                            ui,
                            &mut value,
                            field.min,
                            field.max,
                            field.name == "settings",
                        );
                        if r.changed() {
                            let settings = edit::with_setting(*n, *settings, field.name, value);
                            let mut changed = object.clone();
                            if let Object::Standard { settings: s, .. } = &mut changed {
                                *s = settings;
                            }
                            *change = Some(Change::new(
                                &r,
                                format!("Change {}", field.name),
                                vec![replace(changed)],
                            ));
                        }
                        ui.end_row();
                    }
                }
                Object::Extended { number: n, x, y } => {
                    let mut value = u16::from(*n);
                    ui.label("Number");
                    let r = number_field(ui, &mut value, 0x04, 0xFF, true);
                    if r.changed() {
                        let changed = Object::Extended {
                            number: value as u8,
                            x: *x,
                            y: *y,
                        };
                        *change = Some(Change::new(&r, "Change object", vec![replace(changed)]));
                    }
                    ui.end_row();
                }
                Object::ScreenExit(exit) => {
                    exit_fields(ui, number, *exit, &replace, change);
                }
                Object::Lunar { .. } if edit::map16_object_parts(object).is_some() => {
                    let (tile, w, h) = edit::map16_object_parts(object).expect("checked");
                    let (x, y) = edit::object_position(object).unwrap_or((0, 0));
                    ui.label("Tile");
                    let mut t = tile;
                    let r = ui.add(
                        DragValue::new(&mut t)
                            .range(0..=0x7FFF)
                            .speed(0.1)
                            .hexadecimal(3, false, true),
                    );
                    if r.changed() {
                        let changed = edit::map16_object_sized(&edit::map16_object(t, x, y), w, h);
                        *change =
                            Some(Change::new(&r, "Change Map16 tile", vec![replace(changed)]));
                    }
                    ui.end_row();
                    for (label, value, is_width) in [("Width", w, true), ("Height", h, false)] {
                        ui.label(label);
                        let mut v = value;
                        let r = number_field(ui, &mut v, 1, 16, false);
                        if r.changed() {
                            let (nw, nh) = if is_width { (v, h) } else { (w, v) };
                            let changed = edit::map16_object_sized(object, nw, nh);
                            *change =
                                Some(Change::new(&r, "Resize object", vec![replace(changed)]));
                        }
                        ui.end_row();
                    }
                }
                Object::Lunar { data, .. } => {
                    ui.label("Data");
                    let (r, bytes) = hex_field(ui, ("lunar", o.index), data);
                    if let Some(bytes) = bytes {
                        let mut changed = object.clone();
                        if let Object::Lunar { data, .. } = &mut changed {
                            *data = bytes;
                        }
                        *change = Some(Change::new(
                            &r,
                            "Change object data",
                            vec![replace(changed)],
                        ));
                    }
                    ui.end_row();
                }
                Object::Unplaced(bytes) => {
                    ui.label("Bytes");
                    let (r, new) = hex_field(ui, ("unplaced", o.index), bytes);
                    if let Some(new) = new {
                        let changed = Object::Unplaced(new);
                        *change = Some(Change::new(
                            &r,
                            "Change object bytes",
                            vec![replace(changed)],
                        ));
                    }
                    ui.end_row();
                }
            }
        });
}

fn sprite(ui: &mut egui::Ui, level: &Level, index: usize, change: &mut Option<Change>) {
    let Some(sprite) = level.sprites.list.get(index) else {
        return;
    };
    heading(
        ui,
        names::sprite(sprite.id),
        &format!("sprite {:02X} · #{index}", sprite.id),
    );
    let (width, height) = edit::layer_size(level, ObjectLayer::One);
    let replace = |sprite: Sprite| Edit::ReplaceSprite { index, sprite };
    Grid::new("sprite")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            let mut id = u16::from(sprite.id);
            ui.label("Number");
            let r = number_field(ui, &mut id, 0, 0xFF, true);
            if r.changed() {
                let changed = Sprite {
                    id: id as u8,
                    ..sprite.clone()
                };
                *change = Some(Change::new(&r, "Change sprite", vec![replace(changed)]));
            }
            ui.end_row();
            let (mut x, mut y) = (sprite.x, sprite.y);
            // A move to another screen moves the sprite in the list too.
            let moved = |r: &Response, changed: Sprite| {
                let (edits, at) = edit::move_sprites(level, &[(index, changed)]).ok()?;
                Some(Change::new(r, "Move sprite", edits).selecting(Item::Sprite(at[0])))
            };
            ui.label("X");
            let r = number_field(ui, &mut x, 0, width.saturating_sub(1), false);
            if r.changed() {
                *change = moved(
                    &r,
                    Sprite {
                        x,
                        ..sprite.clone()
                    },
                );
            }
            ui.end_row();
            ui.label("Y");
            let r = number_field(ui, &mut y, 0, height.saturating_sub(1), false);
            if r.changed() {
                *change = moved(
                    &r,
                    Sprite {
                        y,
                        ..sprite.clone()
                    },
                );
            }
            ui.end_row();
            let mut bits = u16::from(sprite.extra_bits);
            ui.label("Extra bits");
            let r = number_field(ui, &mut bits, 0, 3, false);
            if r.changed() {
                let changed = Sprite {
                    extra_bits: bits as u8,
                    ..sprite.clone()
                };
                *change = Some(Change::new(&r, "Change sprite", vec![replace(changed)]));
            }
            ui.end_row();
            if !sprite.extension.is_empty() {
                ui.label("Extension")
                    .on_hover_text("The bytes a tool's sprite takes after the game's three: as many as PIXI's list gives it");
                let (r, bytes) = hex_field(ui, ("extension", index), &sprite.extension);
                if let Some(extension) = bytes {
                    let changed = Sprite {
                        extension,
                        ..sprite.clone()
                    };
                    *change = Some(Change::new(&r, "Change sprite", vec![replace(changed)]));
                }
                ui.end_row();
            }
        });
}

fn header(ui: &mut egui::Ui, level: &Level, change: &mut Option<Change>) {
    heading(ui, "Level", "nothing selected: the level's settings");
    let h = level.header;
    let mut set = |r: &Response, label: &str, header: PrimaryHeader| {
        if r.changed() {
            *change = Some(Change::new(r, label, vec![Edit::SetHeader(header)]));
        }
    };
    Grid::new("header")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            let mut screens = u16::from(h.screens);
            ui.label("Screens");
            let r = number_field(ui, &mut screens, 1, 32, false);
            set(
                &r,
                "Change screens",
                PrimaryHeader {
                    screens: screens as u8,
                    ..h
                },
            );
            ui.end_row();

            ui.label("Mode");
            let mut mode = h.level_mode.0;
            let r = choice(ui, "mode", &mut mode, 0..=0x1F, |m| LevelMode(m).name());
            set(
                &r,
                "Change level mode",
                PrimaryHeader {
                    level_mode: LevelMode(mode),
                    ..h
                },
            );
            ui.end_row();

            ui.label("Tileset");
            let mut tileset = h.object_tileset;
            let r = choice(ui, "tileset", &mut tileset, 0..=0xF, names::object_tileset);
            set(
                &r,
                "Change tileset",
                PrimaryHeader {
                    object_tileset: tileset,
                    ..h
                },
            );
            ui.end_row();

            ui.label("Sprite set");
            let mut sprites = h.sprite_tileset;
            let r = choice(
                ui,
                "sprite-set",
                &mut sprites,
                0..=0xF,
                names::sprite_tileset,
            );
            set(
                &r,
                "Change sprite set",
                PrimaryHeader {
                    sprite_tileset: sprites,
                    ..h
                },
            );
            ui.end_row();

            ui.label("Music");
            let mut music = h.music;
            let r = choice(ui, "music", &mut music, 0..=7, names::music);
            set(&r, "Change music", PrimaryHeader { music, ..h });
            ui.end_row();

            ui.label("Time");
            let mut time = h.time;
            let r = choice(ui, "time", &mut time, 0..=3, |t| {
                ["0", "200", "300", "400"].get(usize::from(t)).copied()
            });
            set(&r, "Change time", PrimaryHeader { time, ..h });
            ui.end_row();

            for (label, value, field) in [
                ("BG palette", h.bg_palette, 0),
                ("FG palette", h.fg_palette, 1),
                ("Sprite palette", h.sprite_palette, 2),
                ("Back area", h.back_area, 3),
            ] {
                ui.label(label);
                let mut v = u16::from(value);
                let r = number_field(ui, &mut v, 0, 7, false);
                let v = v as u8;
                let header = match field {
                    0 => PrimaryHeader { bg_palette: v, ..h },
                    1 => PrimaryHeader { fg_palette: v, ..h },
                    2 => PrimaryHeader {
                        sprite_palette: v,
                        ..h
                    },
                    _ => PrimaryHeader { back_area: v, ..h },
                };
                set(&r, &format!("Change {}", label.to_lowercase()), header);
                ui.end_row();
            }
            for (label, value, field) in [
                ("Item memory", h.item_memory, 0),
                ("Vertical scroll", h.vertical_scroll, 1),
            ] {
                ui.label(label);
                let mut v = u16::from(value);
                let r = number_field(ui, &mut v, 0, 3, false);
                let v = v as u8;
                let header = match field {
                    0 => PrimaryHeader {
                        item_memory: v,
                        ..h
                    },
                    _ => PrimaryHeader {
                        vertical_scroll: v,
                        ..h
                    },
                };
                set(&r, &format!("Change {}", label.to_lowercase()), header);
                ui.end_row();
            }
            ui.label("Layer 3 priority");
            let mut priority = h.layer3_priority;
            let r = ui.checkbox(&mut priority, "");
            set(
                &r,
                "Change layer 3 priority",
                PrimaryHeader {
                    layer3_priority: priority,
                    ..h
                },
            );
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label(
        RichText::new(format!(
            "{} objects on layer 1, {} sprites.",
            level.layer1.len(),
            level.sprites.list.len()
        ))
        .color(theme::MUTED),
    );
}

/// A drop-down of numbered settings with their names.
fn choice(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut u8,
    range: std::ops::RangeInclusive<u8>,
    name: impl Fn(u8) -> Option<&'static str>,
) -> Response {
    let text = |v: u8| match name(v) {
        Some(n) => format!("{v:02X}  {n}"),
        None => format!("{v:02X}"),
    };
    let before = *value;
    let mut response = egui::ComboBox::from_id_salt(id)
        .selected_text(text(*value))
        .width(200.0)
        .show_ui(ui, |ui| {
            for v in range {
                ui.selectable_value(value, v, text(v));
            }
        })
        .response;
    if *value != before {
        response.mark_changed();
    }
    response
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Bytes as hex to edit, the same count as before; the new bytes once the
/// field is left or Enter pressed, if they read as that many.
fn hex_field(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    bytes: &[u8],
) -> (Response, Option<Vec<u8>>) {
    let id = ui.id().with(id);
    let shown = hex_bytes(bytes);
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .font(egui::TextStyle::Monospace)
            .desired_width(180.0),
    );
    let parsed: Option<Vec<u8>> = text
        .split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).ok())
        .collect::<Option<Vec<u8>>>()
        .filter(|new| new.len() == bytes.len());
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    let done = response.lost_focus();
    let mut response = response;
    if parsed.is_none() && text != shown {
        response = response.on_hover_text(format!("{} bytes in hex, such as 00 1F", bytes.len()));
    }
    let new = parsed.filter(|new| done && new.as_slice() != bytes);
    if new.is_some() {
        response.mark_changed();
    }
    (response, new)
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A screen exit's fields: its screen, where it leads, and how. It stays
/// in the format it is in while the game's can say where it leads.
fn exit_fields(
    ui: &mut egui::Ui,
    number: u16,
    exit: ScreenExit,
    replace: &dyn Fn(Object) -> Edit,
    change: &mut Option<Change>,
) {
    let target = edit::ExitTarget::of(exit, number);
    let lunar = exit.flags & ScreenExit::LUNAR_MAGIC != 0;
    let mut set = |r: &Response, target: edit::ExitTarget| {
        if r.changed() {
            let exit = target.exit(number, lunar);
            *change = Some(Change::new(
                r,
                "Change screen exit",
                vec![replace(Object::ScreenExit(exit))],
            ));
        }
    };
    let mut screen = u16::from(target.screen);
    ui.label("Screen");
    let r = number_field(ui, &mut screen, 0, 0x1F, true);
    set(
        &r,
        edit::ExitTarget {
            screen: screen as u8,
            ..target
        },
    );
    ui.end_row();

    let mut secondary = target.secondary;
    ui.label("Leads to");
    let r = ui
        .horizontal(|ui| {
            let a = ui.selectable_value(&mut secondary, false, "Level");
            let b = ui.selectable_value(&mut secondary, true, "Entrance");
            a.union(b)
        })
        .inner;
    set(
        &r,
        edit::ExitTarget {
            secondary,
            ..target
        },
    );
    ui.end_row();

    let max = if target.secondary { 0x1FFF } else { 0x1FF };
    let mut destination = target.destination;
    ui.label(if target.secondary {
        "Entrance"
    } else {
        "Level"
    });
    let r = ui.add(
        DragValue::new(&mut destination)
            .range(0..=max)
            .speed(0.1)
            .hexadecimal(3, false, true),
    );
    set(
        &r,
        edit::ExitTarget {
            destination,
            ..target
        },
    );
    ui.end_row();

    let mut water = target.water;
    ui.label(if target.secondary {
        "Water"
    } else {
        "To midway"
    });
    let r = ui.checkbox(&mut water, "");
    set(&r, edit::ExitTarget { water, ..target });
    ui.end_row();

    ui.label("Format");
    ui.label(RichText::new(if lunar { "Lunar Magic's" } else { "the game's" }).color(theme::MUTED));
    ui.end_row();
}

/// A numbered field of a value: its label, what it holds, its largest
/// value, and the value with it changed.
type Field<T> = (&'static str, u8, u16, fn(T, u8) -> T);
/// A flag of Lunar Magic's level settings, likewise.
type Flag = (&'static str, bool, fn(&mut LevelSettings, bool));

/// The main entrance and midway, Lunar Magic's settings the editor
/// shows, and the secondary entrances that lead into the level.
fn entrances(
    ui: &mut egui::Ui,
    level: &Level,
    free_entrance: Option<u16>,
    change: &mut Option<Change>,
) {
    let e = level.entrance;
    let mut set = |r: &Response, label: &str, entrance: SecondaryHeader| {
        if r.changed() {
            *change = Some(Change::new(r, label, vec![Edit::SetEntrance(entrance)]));
        }
    };
    egui::CollapsingHeader::new(RichText::new("Main entrance").strong())
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("entrance")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    let fields: [Field<SecondaryHeader>; 8] = [
                        ("Screen", e.entrance_screen, 0x1F, |h, v| SecondaryHeader {
                            entrance_screen: v,
                            ..h
                        }),
                        ("X", e.entrance_x, 7, |h, v| SecondaryHeader {
                            entrance_x: v,
                            ..h
                        }),
                        ("Y", e.entrance_y, 15, |h, v| SecondaryHeader {
                            entrance_y: v,
                            ..h
                        }),
                        ("Action", e.entrance_action, 7, |h, v| SecondaryHeader {
                            entrance_action: v,
                            ..h
                        }),
                        ("FG position", e.fg_position, 3, |h, v| SecondaryHeader {
                            fg_position: v,
                            ..h
                        }),
                        ("BG position", e.bg_position, 3, |h, v| SecondaryHeader {
                            bg_position: v,
                            ..h
                        }),
                        ("Layer 2 scroll", e.layer2_scroll, 15, |h, v| {
                            SecondaryHeader {
                                layer2_scroll: v,
                                ..h
                            }
                        }),
                        ("Layer 3", e.layer3, 3, |h, v| SecondaryHeader {
                            layer3: v,
                            ..h
                        }),
                    ];
                    for (label, value, max, with) in fields {
                        ui.label(label);
                        let mut v = u16::from(value);
                        let r = number_field(ui, &mut v, 0, max, false);
                        set(
                            &r,
                            &format!("Change entrance {}", label.to_lowercase()),
                            with(e, v as u8),
                        );
                        ui.end_row();
                    }
                    ui.label("No Yoshi intro");
                    let mut skip = e.no_yoshi_intro;
                    let r = ui.checkbox(&mut skip, "");
                    set(
                        &r,
                        "Change entrance",
                        SecondaryHeader {
                            no_yoshi_intro: skip,
                            ..e
                        },
                    );
                    ui.end_row();
                    ui.label("Vertical position");
                    let mut vertical = e.vertical_position;
                    let r = ui.checkbox(&mut vertical, "");
                    set(
                        &r,
                        "Change entrance",
                        SecondaryHeader {
                            vertical_position: vertical,
                            ..e
                        },
                    );
                    ui.end_row();
                });
        });

    lunar_settings(ui, level, change);

    section(ui, "Secondary entrances here");
    let add = ui
        .add_enabled(
            free_entrance.is_some(),
            egui::Button::new("Add an entrance"),
        )
        .on_hover_text("A secondary entrance into this level, for a screen exit to lead to")
        .on_disabled_hover_text("Every entrance number in this level's half is taken.");
    if add.clicked()
        && let Some(id) = free_entrance
    {
        let entrance = Entrance {
            id,
            screen: 0,
            x: 0,
            y: 0,
            action: 0,
            fg_position: level.entrance.fg_position,
            bg_position: level.entrance.bg_position,
            settings: Default::default(),
        };
        let index = level.entrances.len();
        *change = Some(Change::new(
            &add,
            format!("Add entrance {id:03X}"),
            vec![Edit::InsertEntrance { index, entrance }],
        ));
    }
    for (index, entrance) in level.entrances.iter().enumerate() {
        let title = format!("Entrance {:03X}", entrance.id);
        egui::CollapsingHeader::new(title)
            .id_salt(("entrance", index))
            .show(ui, |ui| {
                Grid::new(("entrance-fields", index))
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        let e = *entrance;
                        let fields: [Field<Entrance>; 6] = [
                            ("Screen", e.screen, 0x1F, |e, v| Entrance { screen: v, ..e }),
                            ("X", e.x, 7, |e, v| Entrance { x: v, ..e }),
                            ("Y", e.y, 15, |e, v| Entrance { y: v, ..e }),
                            ("Action", e.action, 7, |e, v| Entrance { action: v, ..e }),
                            ("FG position", e.fg_position, 3, |e, v| Entrance {
                                fg_position: v,
                                ..e
                            }),
                            ("BG position", e.bg_position, 3, |e, v| Entrance {
                                bg_position: v,
                                ..e
                            }),
                        ];
                        for (label, value, max, with) in fields {
                            ui.label(label);
                            let mut v = u16::from(value);
                            let r = number_field(ui, &mut v, 0, max, false);
                            if r.changed() {
                                let edit = Edit::ReplaceEntrance {
                                    index,
                                    entrance: with(e, v as u8),
                                };
                                *change = Some(Change::new(&r, "Change entrance", vec![edit]));
                            }
                            ui.end_row();
                        }
                    });
                let remove = ui.button("Remove this entrance");
                if remove.clicked() {
                    *change = Some(Change::new(
                        &remove,
                        format!("Remove entrance {:03X}", entrance.id),
                        vec![Edit::RemoveEntrance { index }],
                    ));
                }
            });
    }
}

/// What the level panel's last section asked for.
enum LevelAction {
    Copy,
    Empty,
    Remove,
}

/// Making a level at a number the project does not list yet, a copy of
/// this one (without its secondary entrances, which stay its own) or an
/// empty one; and taking this one out of the project.
fn copy_level(ui: &mut egui::Ui, to: &mut u16, taken: &[bool]) -> Option<LevelAction> {
    section(ui, "Other levels");
    let mut action = None;
    let free = !taken[usize::from(*to)];
    ui.horizontal(|ui| {
        ui.label("Level");
        ui.add(
            DragValue::new(to)
                .range(0..=0x1FF)
                .speed(0.1)
                .hexadecimal(3, false, true),
        );
        if ui
            .add_enabled(free, egui::Button::new("Copy this one there"))
            .on_disabled_hover_text("The project has that level already.")
            .clicked()
        {
            action = Some(LevelAction::Copy);
        }
    });
    if ui
        .add_enabled(free, egui::Button::new("Start an empty level there"))
        .on_hover_text("With this level's settings, and nothing in it")
        .on_disabled_hover_text("The project has that level already.")
        .clicked()
    {
        action = Some(LevelAction::Empty);
    }
    ui.add_space(4.0);
    if ui
        .button("Take this level out of the project…")
        .on_hover_text("It then builds as the game has it")
        .clicked()
    {
        action = Some(LevelAction::Remove);
    }
    action
}

/// A field of a midway entrance of its own, as [`Field`] is of others.
type MidwayField = (
    &'static str,
    u16,
    u16,
    fn(kobo_core::entrance::MidwayEntrance, u16) -> kobo_core::entrance::MidwayEntrance,
);
type MidwayFlag = (
    &'static str,
    bool,
    fn(kobo_core::entrance::MidwayEntrance, bool) -> kobo_core::entrance::MidwayEntrance,
);

/// A level size as the panel names it.
fn size_name(mode: u8) -> String {
    let (height, screens) = kobo_core::level::size::SIZES[usize::from(mode)];
    let rows = height / 16;
    if mode == 0 {
        format!("{rows} rows, {screens} screens: the game's")
    } else {
        format!("{rows} rows, {screens} screens")
    }
}

/// Lunar Magic's settings for the level: its size, how its layers start,
/// its sprites' spawning, and the midway entrance.
fn lunar_settings(ui: &mut egui::Ui, level: &Level, change: &mut Option<Change>) {
    use kobo_core::entrance::{Background, MidwayEntrance, SeparateMidway};
    use kobo_core::level::size::LevelSize;

    let settings = level.settings;
    let header = level.entrance;
    egui::CollapsingHeader::new(RichText::new("Lunar Magic's settings").strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                RichText::new(
                    "A build that has any of these other than the game's way installs Kobo's code for Lunar Magic's layout.",
                )
                .small()
                .color(theme::MUTED),
            );
            let set = |r: &Response, label: &str, s: LevelSettings, change: &mut Option<Change>| {
                if r.changed() {
                    *change = Some(Change::new(r, label, vec![Edit::SetSettings(s)]));
                }
            };
            Grid::new("lunar-settings")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    // The size: a horizontal level's height.
                    let size = level.size;
                    ui.label("Size");
                    if level.header.level_mode.layer1_vertical() {
                        ui.label(RichText::new("a vertical level takes none").color(theme::MUTED));
                    } else {
                        let mut mode = size.mode;
                        let before = mode;
                        let mut r = egui::ComboBox::from_id_salt("size")
                            .selected_text(size_name(mode))
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for m in 0..32u8 {
                                    ui.selectable_value(&mut mode, m, size_name(m));
                                }
                            })
                            .response;
                        if mode != before {
                            r.mark_changed();
                            let edit = Edit::SetSize(LevelSize { mode, ..size });
                            *change = Some(Change::new(&r, "Change level size", vec![edit]));
                        }
                    }
                    ui.end_row();
                    ui.label("Last row whole");
                    let mut bottom = size.bottom_row;
                    let r = ui
                        .checkbox(&mut bottom, "")
                        .on_hover_text("The camera goes low enough to show the level's last row whole");
                    if r.changed() {
                        let edit = Edit::SetSize(LevelSize {
                            bottom_row: bottom,
                            ..size
                        });
                        *change = Some(Change::new(&r, "Change level size", vec![edit]));
                    }
                    ui.end_row();

                    let flags: [Flag; 5] = [
                        ("Slippery", settings.slippery, |s, v| s.slippery = v),
                        ("Water", settings.water, |s, v| s.water = v),
                        ("Face left", settings.face_left, |s, v| s.face_left = v),
                        ("Smart spawning", settings.smart_spawn, |s, v| s.smart_spawn = v),
                        ("Screens set themselves", settings.auto_screens, |s, v| {
                            s.auto_screens = v
                        }),
                    ];
                    for (label, value, with) in flags {
                        ui.label(label);
                        let mut v = value;
                        let r = ui.checkbox(&mut v, "");
                        let mut changed = settings;
                        with(&mut changed, v);
                        set(&r, &format!("Change {}", label.to_lowercase()), changed, change);
                        ui.end_row();
                    }
                    ui.label("Spawn range");
                    let mut range = u16::from(settings.spawn_range);
                    let r = number_field(ui, &mut range, 0, 3, false);
                    set(
                        &r,
                        "Change spawn range",
                        LevelSettings {
                            spawn_range: range as u8,
                            ..settings
                        },
                        change,
                    );
                    ui.end_row();

                    // Layer 2's vertical scroll, apart from its horizontal.
                    ui.label("Layer 2 vertical");
                    ui.horizontal(|ui| {
                        let mut own = settings.layer2_vertical_scroll.is_some();
                        let r = ui.checkbox(&mut own, "its own");
                        set(
                            &r,
                            "Change layer 2 scroll",
                            LevelSettings {
                                layer2_vertical_scroll: own.then_some(0),
                                layer2_horizontal_high: false,
                                ..settings
                            },
                            change,
                        );
                        if let Some(value) = settings.layer2_vertical_scroll {
                            let mut v = u16::from(value);
                            let r = number_field(ui, &mut v, 0, 31, false);
                            set(
                                &r,
                                "Change layer 2 scroll",
                                LevelSettings {
                                    layer2_vertical_scroll: Some(v as u8),
                                    ..settings
                                },
                                change,
                            );
                        }
                    });
                    ui.end_row();

                    // Where the background starts.
                    ui.label("Background");
                    ui.horizontal(|ui| {
                        let kind = match settings.background {
                            Background::Height(_) => 0,
                            Background::Offset(_) => 1,
                            Background::Absolute => 2,
                        };
                        let mut chosen = kind;
                        let names = ["shows its last row", "rows from layer 1", "at the top"];
                        let mut r = egui::ComboBox::from_id_salt("background")
                            .selected_text(names[kind])
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut chosen, i, *name);
                                }
                            })
                            .response;
                        if chosen != kind {
                            r.mark_changed();
                            let background = match chosen {
                                0 => Background::Height(27),
                                1 => Background::Offset(0),
                                _ => Background::Absolute,
                            };
                            set(&r, "Change background position", LevelSettings { background, ..settings }, change);
                        }
                        match settings.background {
                            Background::Height(rows) => {
                                let mut v = u16::from(rows);
                                let r = number_field(ui, &mut v, 1, 32, false)
                                    .on_hover_text("The background's height in rows");
                                set(&r, "Change background position", LevelSettings { background: Background::Height(v as u8), ..settings }, change);
                            }
                            Background::Offset(rows) => {
                                let mut v = i32::from(rows);
                                let r = ui.add(DragValue::new(&mut v).range(-15..=15).speed(0.1));
                                set(&r, "Change background position", LevelSettings { background: Background::Offset(v as i8), ..settings }, change);
                            }
                            Background::Absolute => {}
                        }
                    });
                    ui.end_row();

                    // The midway entrance: its screen, and its own settings.
                    ui.label("Midway screen");
                    let screen = u16::from(header.midway_screen)
                        | if settings.midway.screen_high { 0x10 } else { 0 };
                    let mut v = screen;
                    let r = number_field(ui, &mut v, 0, 31, true);
                    if r.changed() {
                        let mut s = settings;
                        s.midway.screen_high = v & 0x10 != 0;
                        let entrance = SecondaryHeader {
                            midway_screen: (v & 0x0F) as u8,
                            ..header
                        };
                        *change = Some(Change::new(
                            &r,
                            "Change midway screen",
                            vec![Edit::SetEntrance(entrance), Edit::SetSettings(s)],
                        ));
                    }
                    ui.end_row();

                    ui.label("Midway entrance");
                    let kind = match settings.midway.separate {
                        None => 0,
                        Some(SeparateMidway::Entrance(_)) => 1,
                        Some(SeparateMidway::Redirect(_)) => 2,
                    };
                    let mut chosen = kind;
                    let names = ["as the main one", "its own", "another level's"];
                    let mut r = egui::ComboBox::from_id_salt("midway")
                        .selected_text(names[kind])
                        .show_ui(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                ui.selectable_value(&mut chosen, i, *name);
                            }
                        })
                        .response;
                    if chosen != kind {
                        r.mark_changed();
                        let mut s = settings;
                        s.midway.separate = match chosen {
                            1 => Some(SeparateMidway::Entrance(MidwayEntrance {
                                slippery: settings.slippery,
                                water: settings.water,
                                action: header.entrance_action,
                                x: 0,
                                y: 0,
                                fg_position: header.fg_position,
                                bg_position: header.bg_position,
                                relative: None,
                                face_left: settings.face_left,
                            })),
                            2 => Some(SeparateMidway::Redirect(0)),
                            _ => None,
                        };
                        set(&r, "Change midway entrance", s, change);
                    }
                    ui.end_row();
                    match settings.midway.separate {
                        Some(SeparateMidway::Redirect(to)) => {
                            ui.label("Its level");
                            let mut v = to;
                            let r = ui.add(DragValue::new(&mut v).range(0..=0x1FF).speed(0.1).hexadecimal(3, false, true));
                            let mut s = settings;
                            s.midway.separate = Some(SeparateMidway::Redirect(v));
                            set(&r, "Change midway entrance", s, change);
                            ui.end_row();
                        }
                        Some(SeparateMidway::Entrance(m)) => {
                            let with = |m: MidwayEntrance| {
                                let mut s = settings;
                                s.midway.separate = Some(SeparateMidway::Entrance(m));
                                s
                            };
                            let fields: [MidwayField; 5] = [
                                ("Its X tile", u16::from(m.x), 31, |m, v| MidwayEntrance { x: v as u8, ..m }),
                                ("Its Y tile", m.y, 1023, |m, v| MidwayEntrance { y: v, ..m }),
                                ("Its action", u16::from(m.action), 7, |m, v| MidwayEntrance { action: v as u8, ..m }),
                                ("Its FG position", u16::from(m.fg_position), 3, |m, v| MidwayEntrance { fg_position: v as u8, ..m }),
                                ("Its BG position", u16::from(m.bg_position), 3, |m, v| MidwayEntrance { bg_position: v as u8, ..m }),
                            ];
                            for (label, value, max, change_to) in fields {
                                ui.label(label);
                                let mut v = value;
                                let r = number_field(ui, &mut v, 0, max, false);
                                set(&r, "Change midway entrance", with(change_to(m, v)), change);
                                ui.end_row();
                            }
                            let flags: [MidwayFlag; 3] = [
                                ("Its water", m.water, |m, v| MidwayEntrance { water: v, ..m }),
                                ("Slippery there", m.slippery, |m, v| MidwayEntrance { slippery: v, ..m }),
                                ("Faces left", m.face_left, |m, v| MidwayEntrance { face_left: v, ..m }),
                            ];
                            for (label, value, change_to) in flags {
                                ui.label(label);
                                let mut v = value;
                                let r = ui.checkbox(&mut v, "");
                                set(&r, "Change midway entrance", with(change_to(m, v)), change);
                                ui.end_row();
                            }
                        }
                        None => {}
                    }
                });
        });
}
