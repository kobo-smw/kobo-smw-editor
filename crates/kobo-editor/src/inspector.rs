//! The inspector: what is selected, or the level's header when nothing
//! is, as fields that edit it.

use eframe::egui::{self, DragValue, Grid, Response, RichText};
use kobo_core::edit::{self, Edit, ObjectLayer};
use kobo_core::entrance::{Camera, EntranceSettings, LevelSettings};
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
    // Where a selected screen exit leads: the level, and the entrance.
    let exit_leads = match selection[..] {
        [Item::Object(o)] => objects(&level, o.layer)
            .and_then(|l| l.get(o.index))
            .and_then(|object| match object {
                Object::ScreenExit(exit) => app.exit_leads(number, *exit),
                _ => None,
            }),
        _ => None,
    };
    let mut go_to = false;
    let mut select_only = None;
    // The game's palette for the level, to start one of its own from.
    let game_palette = app
        .workspace()
        .and_then(|w| kobo_core::palette::game_palette(w.clean(), &level.header).ok());
    let mut show_table: Option<&'static str> = None;
    let strips = app.workspace().map(|w| palette_strips(w.clean()));
    // How each layer 2 scroll setting moves layer 2.
    let scroll_names = app.workspace().map(|w| {
        std::array::from_fn(|s| kobo_core::level::layer2_scroll(w.clean(), s as u8).unwrap_or("?"))
    });
    // What each layer 3 setting does in the level's tileset.
    let layer3_names = app.workspace().map(|w| {
        std::array::from_fn(|s| {
            kobo_core::level::layer3_setting(w.clean(), level.header.object_tileset, s as u8)
                .unwrap_or("?")
        })
    });
    // From the picture of the level as it is, not an older one.
    let screens_used = open
        .geometry
        .as_ref()
        .filter(|g| g.level == level)
        .map(|g| edit::screens_used(&g.loaded, &level));
    // Propose the first free number after this level.
    if taken[usize::from(copy_to) & 0x1FF] {
        copy_to = (1..0x200u16)
            .map(|d| (number + d) & 0x1FF)
            .find(|&n| !taken[usize::from(n)])
            .unwrap_or(copy_to);
    }

    if let [item] = selection[..] {
        picture_of(app, ui, item);
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match selection[..] {
                [] => {
                    header(ui, &level, strips.as_ref(), screens_used, &mut change, |ui| {
                        crate::backgrounds::row(app, ui);
                    });
                    entrances(ui, &level, free_entrance, layer3_names, scroll_names, &mut change, |ui, change| {
                        graphics_and_palette(
                            ui,
                            &level,
                            game_palette.as_ref(),
                            change,
                            &mut show_table,
                        );
                    });
                    level_action = copy_level(ui, &mut copy_to, &taken);
                }
                [Item::Object(o)] => {
                    object(ui, &level, number, o, &mut change, exit_leads, &mut go_to)
                }
                [Item::Sprite(i)] => sprite(ui, &level, i, &mut change),
                _ => {
                    let sprites = selection
                        .iter()
                        .filter(|i| matches!(i, Item::Sprite(_)))
                        .count();
                    let others = selection.len() - sprites;
                    let count = |n: usize, one: &str| match n {
                        1 => format!("1 {one}"),
                        n => format!("{n} {one}s"),
                    };
                    let detail = match (others, sprites) {
                        (0, s) => count(s, "sprite"),
                        (o, 0) => count(o, "object"),
                        (o, s) => format!("{}, {}", count(o, "object"), count(s, "sprite")),
                    };
                    heading(ui, &format!("{} selected", selection.len()), &detail);
                    ui.label(
                        RichText::new("Drag them on the canvas, or use the arrow keys, to move them together; Ctrl+drag copies them. A click here selects one alone.")
                            .color(theme::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                        for &item in &selection {
                            let name = selection::describe(&level, item);
                            let place = match item {
                                Item::Object(o) => objects(&level, o.layer)
                                    .and_then(|l| l.get(o.index))
                                    .and_then(edit::object_position),
                                Item::Sprite(i) => level.sprites.list.get(i).map(|s| (s.x, s.y)),
                            };
                            let place = place.map_or_else(String::new, |(x, y)| format!("{x}, {y}"));
                            let row = egui::Button::selectable(false, RichText::new(name).color(crate::app::color_for(item)))
                                .right_text(RichText::new(place).size(12.0).color(theme::MUTED));
                            if ui.add(row).clicked() {
                                select_only = Some(item);
                            }
                        }
                    });
                }
            }
            if !selection.is_empty() {
                ui.add_space(8.0);
                delete = ui.button("Delete").on_hover_text("Delete (Del)").clicked();
            }
            ui.add_space(12.0);
            section(ui, "Diagnostics");
            let unreached = edit::unreached_sprites(&level);
            if !unreached.is_empty() {
                let n = unreached.len();
                let text = if n == 1 {
                    "1 sprite comes after a sprite on a later screen, and the game never loads it.".to_string()
                } else {
                    format!("{n} sprites come after sprites on later screens, and the game never loads them.")
                };
                ui.label(RichText::new(text).color(theme::WARNING));
                if ui.button("Put the sprites in screen order").clicked() {
                    change = Some(Change {
                        widget: egui::Id::new("sort-sprites"),
                        dragging: false,
                        label: "Put the sprites in screen order".into(),
                        edits: edit::sort_sprites(&level),
                        select: None,
                    });
                }
            }
            if diagnostics.is_empty() && unreached.is_empty() {
                ui.label(RichText::new("None: the level loaded and drew in full.").color(theme::MUTED));
            } else {
                for line in &diagnostics {
                    ui.label(RichText::new(line).color(theme::WARNING));
                }
            }
        });

    app.copy_to = copy_to;
    if let Some(item) = select_only
        && let Some(open) = app.current_mut()
    {
        open.selection = vec![item];
        open.focus = true;
    }
    if let Some(table) = show_table {
        app.view.source = true;
        if let Some(open) = app.current_mut() {
            open.source.show_table = Some(table);
        }
    }
    if go_to && let Some(leads) = exit_leads {
        app.follow_exit(leads);
    }
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

/// What is selected, cut from the level's picture with a tile around it,
/// at a whole scale that fits the panel.
fn picture_of(app: &App, ui: &mut egui::Ui, item: Item) {
    let Some(open) = app.current() else { return };
    let (Some(picture), Some(geometry)) = (&open.picture, &open.geometry) else {
        return;
    };
    let Some(bounds) = geometry.bounds(item) else {
        return;
    };
    // A screen exit's bounds are its whole screen: not a picture of it.
    if bounds.width() > 512.0 || bounds.height() > 512.0 {
        return;
    }
    let source = bounds
        .expand(16.0)
        .intersect(egui::Rect::from_min_size(egui::Pos2::ZERO, picture.size));
    let room = egui::vec2(ui.available_width() - 8.0, 120.0);
    let scale = (room / source.size()).min_elem().floor().clamp(1.0, 4.0);
    let size = source.size() * scale;
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size.y),
        egui::Sense::hover(),
    );
    let target = egui::Rect::from_min_size(rect.min, size);
    ui.painter().rect_filled(target, 0, theme::CANVAS);
    picture.draw(ui.painter(), source, target, egui::Color32::WHITE);
    let selected = egui::Rect::from_min_size(
        target.min + (bounds.min - source.min) * scale,
        bounds.size() * scale,
    );
    ui.painter().rect_stroke(
        selected,
        0,
        egui::Stroke::new(1.0, crate::app::color_for(item)),
        egui::StrokeKind::Outside,
    );
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
    exit_leads: Option<(u16, Option<u16>)>,
    go_to: &mut bool,
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
                    ui.label("Object");
                    let tileset = level.header.object_tileset;
                    let (r, picked) = named_picker(ui, "object", *n, 0x01..=0x3F, |n| {
                        // 22-2D are not the game's (docs/smw.md).
                        if (0x22..=0x2D).contains(&n) {
                            Some("Unused")
                        } else {
                            names::standard_object(n, tileset)
                        }
                    });
                    if let Some(number) = picked {
                        let mut changed = object.clone();
                        if let Object::Standard { number: m, .. } = &mut changed {
                            *m = number;
                        }
                        *change = Some(Change::new(&r, "Change object", vec![replace(changed)]));
                    }
                    ui.end_row();
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
                    ui.label("Object");
                    let (r, picked) = named_picker(ui, "extended", *n, 0x04..=0xFF, |n| {
                        Some(names::extended_object(n))
                    });
                    if let Some(number) = picked {
                        let changed = Object::Extended {
                            number,
                            x: *x,
                            y: *y,
                        };
                        *change = Some(Change::new(&r, "Change object", vec![replace(changed)]));
                    }
                    ui.end_row();
                }
                Object::ScreenExit(exit) => {
                    exit_fields(ui, number, *exit, &replace, change);
                    ui.label("");
                    match exit_leads {
                        Some((level, _)) => {
                            *go_to = ui
                                .button(format!("Go to level {level:03X}"))
                                .on_hover_text("Open the level it leads to, where it comes in")
                                .clicked();
                        }
                        None => {
                            ui.label(
                                RichText::new("No level has that entrance.").color(theme::WARNING),
                            );
                        }
                    }
                    ui.end_row();
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
            ui.label("Sprite");
            let (r, picked) = named_picker(ui, "sprite", sprite.id, 0..=0xFF, |n| {
                Some(names::sprite(n))
            });
            if let Some(id) = picked {
                let changed = Sprite {
                    id,
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

/// The colours each value of each palette setting gives, in
/// `PaletteSetting` order: background, foreground, sprite, back area.
type Strips = [[Vec<egui::Color32>; 8]; 4];

fn palette_strips(rom: &kobo_core::Rom) -> Strips {
    use kobo_core::palette::{PaletteSetting, setting_colors};
    let settings = [
        PaletteSetting::Background,
        PaletteSetting::Foreground,
        PaletteSetting::Sprite,
        PaletteSetting::BackArea,
    ];
    settings.map(|setting| {
        std::array::from_fn(|value| {
            setting_colors(rom, setting, value as u8)
                .unwrap_or_default()
                .iter()
                .map(|c| {
                    let [r, g, b] = c.to_rgb8();
                    egui::Color32::from_rgb(r, g, b)
                })
                .collect()
        })
    })
}

/// A row of colours.
fn strip(ui: &mut egui::Ui, colors: &[egui::Color32]) {
    let cell = egui::vec2(if colors.len() == 1 { 24.0 } else { 9.0 }, 14.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(cell.x * colors.len() as f32, cell.y),
        egui::Sense::hover(),
    );
    for (i, color) in colors.iter().enumerate() {
        let at = rect.min + egui::vec2(i as f32 * cell.x, 0.0);
        ui.painter()
            .rect_filled(egui::Rect::from_min_size(at, cell), 0, *color);
    }
}

/// A palette setting (0 to 7), each value shown with its colours; the
/// response and the value chosen.
fn palette_field(
    ui: &mut egui::Ui,
    id: &str,
    value: u8,
    colors: Option<&[Vec<egui::Color32>; 8]>,
) -> (Response, u8) {
    ui.horizontal(|ui| {
        let mut chosen = value;
        let mut r = egui::ComboBox::from_id_salt(id)
            .selected_text(format!("{value}"))
            .width(44.0)
            .show_ui(ui, |ui| {
                for v in 0..8u8 {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut chosen, v, format!("{v}"));
                        if let Some(colors) = colors {
                            strip(ui, &colors[usize::from(v)]);
                        }
                    });
                }
            })
            .response;
        if let Some(colors) = colors {
            strip(ui, &colors[usize::from(value & 7)]);
        }
        if chosen != value {
            r.mark_changed();
        }
        (r, chosen)
    })
    .inner
}

/// The level's header and sprite settings, with `more` rows after them.
fn header(
    ui: &mut egui::Ui,
    level: &Level,
    strips: Option<&Strips>,
    screens_used: Option<u8>,
    change: &mut Option<Change>,
    more: impl FnOnce(&mut egui::Ui),
) {
    heading(ui, "Level", "");
    let h = level.header;
    let mut set = |r: &Response, label: &str, header: PrimaryHeader| {
        if r.changed() {
            *change = Some(Change::new(r, label, vec![Edit::SetHeader(header)]));
        }
    };
    let mut sprite_change = None;
    Grid::new("header")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            let mut screens = u16::from(h.screens);
            ui.label("Screens");
            ui.horizontal(|ui| {
                let r = number_field(ui, &mut screens, 1, 32, false);
                set(
                    &r,
                    "Change screens",
                    PrimaryHeader {
                        screens: screens as u8,
                        ..h
                    },
                );
                if let Some(used) = screens_used.filter(|&n| n != h.screens) {
                    let r = ui
                        .small_button(format!("Fit: {used}"))
                        .on_hover_text("As many as its objects and sprites reach");
                    if r.clicked() {
                        set(
                            &r,
                            "Fit the screens to the level",
                            PrimaryHeader { screens: used, ..h },
                        );
                    }
                }
            });
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
                // A timer at 0 never counts down (`UpdateStatusBar`).
                ["no limit", "200", "300", "400"]
                    .get(usize::from(t))
                    .copied()
            });
            set(&r, "Change time", PrimaryHeader { time, ..h });
            ui.end_row();

            for (label, value, field) in [
                ("BG palette", h.bg_palette, 0),
                ("FG palette", h.fg_palette, 1),
                ("Sprite palette", h.sprite_palette, 2),
                ("Back area", h.back_area, 3),
            ] {
                let label_response = ui.label(label);
                if level.palette.is_some() {
                    label_response.on_hover_text("The level's own palette takes its place");
                }
                let (r, v) = palette_field(ui, label, value, strips.map(|s| &s[field]));
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

            let s = &level.sprites;
            let (mut memory, mut buoyancy, mut no_layer2) =
                (u16::from(s.memory), s.buoyancy, s.buoyancy_no_layer2);
            let mut sprites_changed = |r: &Response, label: &str, (m, b, n): (u16, bool, bool)| {
                if r.changed() {
                    sprite_change = Some(Change::new(
                        r,
                        label,
                        vec![Edit::SetSpriteSettings {
                            memory: m as u8,
                            buoyancy: b,
                            buoyancy_no_layer2: n,
                        }],
                    ));
                }
            };
            ui.label("Sprite memory").on_hover_text(
                "Which slots the level's sprites take, and how many can be alive at once",
            );
            let r = number_field(ui, &mut memory, 0, 31, true);
            sprites_changed(&r, "Change sprite memory", (memory, buoyancy, no_layer2));
            ui.end_row();
            ui.label("Sprites in water");
            let r = ui
                .checkbox(&mut buoyancy, "swim and sink")
                .on_hover_text("Sprites feel the water and lava tiles they are in (buoyancy)");
            sprites_changed(&r, "Change buoyancy", (memory, buoyancy, no_layer2));
            ui.end_row();
            ui.label("");
            let r = ui
                .add_enabled(
                    buoyancy,
                    egui::Checkbox::new(&mut no_layer2, "but not on layer 2"),
                )
                .on_hover_text("With buoyancy, sprites leave layer 2's tiles alone");
            sprites_changed(&r, "Change buoyancy", (memory, buoyancy, no_layer2));
            ui.end_row();
            more(ui);
        });
    if sprite_change.is_some() {
        *change = sprite_change;
    }
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

/// A number chosen by name: a button with the number and its name, which
/// opens a list of every one, found by typing. Returns the button and the
/// number chosen, if one was.
fn named_picker(
    ui: &mut egui::Ui,
    id: &str,
    value: u8,
    range: std::ops::RangeInclusive<u8>,
    name: impl Fn(u8) -> Option<&'static str>,
) -> (Response, Option<u8>) {
    let text = |v: u8| match name(v) {
        Some(n) => format!("{v:02X}  {n}"),
        None => format!("{v:02X}"),
    };
    // Left-aligned, as a field reads: the empty right text takes the
    // space after the name.
    let button = ui.add(
        egui::Button::new(text(value))
            .right_text("")
            .truncate()
            .min_size(egui::vec2(200.0, 0.0)),
    );
    let filter_id = ui.make_persistent_id(("picker-filter", id));
    let mut picked = None;
    egui::Popup::from_toggle_button_response(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(280.0);
            let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
            let field = ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("Find by name or number")
                    .desired_width(f32::INFINITY),
            );
            if !field.has_focus() && filter.is_empty() {
                field.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            let filter = filter.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for v in range {
                        let label = text(v);
                        let unused = name(v) == Some("Unused");
                        if !filter.is_empty() && !label.to_lowercase().contains(&filter)
                            || unused && v != value
                        {
                            continue;
                        }
                        let r = ui.selectable_label(v == value, label);
                        if v == value && filter.is_empty() && button.clicked() {
                            r.scroll_to_me(Some(egui::Align::Center));
                        }
                        if r.clicked() {
                            picked = Some(v);
                            ui.data_mut(|d| d.remove_temp::<String>(filter_id));
                            ui.close();
                        }
                    }
                });
        });
    (button, picked.filter(|&v| v != value))
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

/// Where an entrance puts the player and the camera, as its fields show
/// them: a screen, X and Y as table settings or (`by_tile`) as tiles, and
/// the camera.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Placed {
    screen: u8,
    x: u16,
    y: u16,
    by_tile: bool,
    camera: Camera,
}

impl Placed {
    /// From an entrance's settings: the low bits of X and Y, and method
    /// 2's high bits if it places by tile.
    fn new(screen: u8, x: u8, y: u8, tile: Option<(u8, u8)>, camera: Camera) -> Self {
        let (x, y) = (u16::from(x), u16::from(y));
        match tile {
            Some((xh, yh)) => Self {
                screen,
                x: u16::from(xh) << 3 | x & 7,
                y: u16::from(yh) << 4 | y & 15,
                by_tile: true,
                camera,
            },
            None => Self {
                screen,
                x,
                y,
                by_tile: false,
                camera,
            },
        }
    }

    /// The settings' X and Y, and method 2's high bits.
    fn split(self) -> (u8, u8, Option<(u8, u8)>) {
        if self.by_tile {
            (
                (self.x & 7) as u8,
                (self.y & 15) as u8,
                Some(((self.x >> 3) as u8, (self.y >> 4) as u8)),
            )
        } else {
            (self.x as u8, self.y as u8, None)
        }
    }
}

/// The fields of an entrance's place and camera, as rows of a grid; the
/// response of the one that changed, and the new place.
fn placed_fields(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
    placed: Placed,
    vertical: bool,
) -> Option<(Response, Placed)> {
    let mut new = placed;
    let mut changed: Option<Response> = None;
    let mut note = |r: Response| {
        if r.changed() {
            changed = Some(r);
        }
    };
    ui.label("Position");
    let mut by_tile = new.by_tile;
    let mut r = egui::ComboBox::from_id_salt(("position", id))
        .selected_text(if by_tile {
            "by tile"
        } else {
            "the game's places"
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut by_tile, false, "the game's places")
                .on_hover_text("A screen, and X and Y settings that pick from the game's tables");
            ui.selectable_value(&mut by_tile, true, "by tile")
                .on_hover_text("Lunar Magic's position method 2: any tile of the screen");
        })
        .response;
    if by_tile != new.by_tile {
        r.mark_changed();
        new.by_tile = by_tile;
        // The settings' bits carry over; the high ones start clear.
        (new.x, new.y) = (new.x & 7, new.y & 15);
    }
    note(r);
    ui.end_row();

    let (x_max, y_max) = match (new.by_tile, vertical) {
        (false, _) => (7, 15),
        (true, false) => (15, 1023),
        (true, true) => (31, 15),
    };
    let mut screen = u16::from(new.screen);
    ui.label("Screen");
    note(number_field(ui, &mut screen, 0, 0x1F, false));
    new.screen = screen as u8;
    ui.end_row();
    ui.label(if new.by_tile { "Tile X" } else { "X" });
    note(number_field(ui, &mut new.x, 0, x_max, false));
    ui.end_row();
    ui.label(if new.by_tile { "Tile Y" } else { "Y" });
    note(number_field(ui, &mut new.y, 0, y_max, false));
    ui.end_row();

    if let Some((r, camera)) = camera_fields(ui, id, new.camera) {
        new.camera = camera;
        note(r);
    }
    changed.filter(|_| new != placed).map(|r| (r, new))
}

/// The fields of an entrance's camera, as rows of a grid; the response of
/// the one that changed, and the new camera.
fn camera_fields(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
    camera: Camera,
) -> Option<(Response, Camera)> {
    let mut new = camera;
    let mut changed: Option<Response> = None;
    let mut note = |r: Response| {
        if r.changed() {
            changed = Some(r);
        }
    };
    ui.label("Camera");
    let relative = matches!(new, Camera::Relative(_));
    let mut chosen = relative;
    let mut r = egui::ComboBox::from_id_salt(("camera", id))
        .selected_text(if relative {
            "from the player"
        } else {
            "the game's positions"
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, false, "the game's positions")
                .on_hover_text("Layers 1 and 2 start at one of four heights each");
            ui.selectable_value(&mut chosen, true, "from the player")
                .on_hover_text("Lunar Magic's: layer 1 starts a number of rows from the player");
        })
        .response;
    if chosen != relative {
        r.mark_changed();
        new = if chosen {
            Camera::Relative(0)
        } else {
            Camera::Positions { fg: 0, bg: 0 }
        };
    }
    note(r);
    ui.end_row();
    match &mut new {
        Camera::Positions { fg, bg } => {
            for (label, value) in [("FG position", fg), ("BG position", bg)] {
                ui.label(label);
                let mut v = u16::from(*value);
                note(number_field(ui, &mut v, 0, 3, false));
                *value = v as u8;
                ui.end_row();
            }
        }
        Camera::Relative(rows) => {
            ui.label("Rows");
            let r = ui
                .add(DragValue::new(rows).range(-16..=15).speed(0.1))
                .on_hover_text("Layer 1's top, in rows from the player: up is negative");
            note(r);
            ui.end_row();
        }
    }
    changed.filter(|_| new != camera).map(|r| (r, new))
}

/// A flag of Lunar Magic's level settings: its label, what it does, what
/// it holds, and the settings with it changed.
type Flag = (
    &'static str,
    &'static str,
    bool,
    fn(&mut LevelSettings, bool),
);

/// The main entrance and midway, Lunar Magic's settings the editor
/// shows, `before_secondary`'s sections, and the secondary entrances that
/// lead into the level.
fn entrances(
    ui: &mut egui::Ui,
    level: &Level,
    free_entrance: Option<u16>,
    layer3_names: Option<[&'static str; 4]>,
    scroll_names: Option<[&'static str; 16]>,
    change: &mut Option<Change>,
    before_secondary: impl FnOnce(&mut egui::Ui, &mut Option<Change>),
) {
    let e = level.entrance;
    let mut set = |r: &Response, label: &str, entrance: SecondaryHeader| {
        if r.changed() {
            *change = Some(Change::new(r, label, vec![Edit::SetEntrance(entrance)]));
        }
    };
    let vertical = level.header.level_mode.layer1_vertical();
    let mut placed_change = None;
    egui::CollapsingHeader::new(RichText::new("Main entrance").strong())
        .default_open(false)
        .show(ui, |ui| {
            Grid::new("entrance")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    let placed = Placed::new(
                        e.entrance_screen,
                        e.entrance_x,
                        e.entrance_y,
                        level.settings.tile_position,
                        Camera::from_bits(
                            e.fg_position,
                            e.bg_position,
                            level.settings.relative,
                            false,
                        ),
                    );
                    placed_change = placed_fields(ui, "main", placed, vertical);
                    ui.label("Action");
                    let mut action = e.entrance_action;
                    let r = choice(
                        ui,
                        "main-action",
                        &mut action,
                        0..=7,
                        names::entrance_action,
                    );
                    set(
                        &r,
                        "Change entrance action",
                        SecondaryHeader {
                            entrance_action: action,
                            ..e
                        },
                    );
                    ui.end_row();
                    ui.label("Layer 2 scroll").on_hover_text(
                        "How layer 2 moves as layer 1 does; with Lunar Magic's separate \
                         vertical setting, how it moves across",
                    );
                    let mut scroll = e.layer2_scroll;
                    let separate = level.settings.layer2_vertical_scroll.is_some();
                    let r = choice(ui, "layer2-scroll", &mut scroll, 0..=15, |v| {
                        scroll_names
                            .filter(|_| !separate)
                            .map(|names| names[usize::from(v & 15)])
                    });
                    set(
                        &r,
                        "Change layer 2 scroll",
                        SecondaryHeader {
                            layer2_scroll: scroll,
                            ..e
                        },
                    );
                    ui.end_row();
                    ui.label("Layer 3").on_hover_text(
                        "What the game's table for the level's tileset makes of each setting",
                    );
                    let mut layer3 = e.layer3;
                    let r = choice(ui, "layer3", &mut layer3, 0..=3, |v| {
                        layer3_names.map(|names| names[usize::from(v & 3)])
                    });
                    set(&r, "Change layer 3", SecondaryHeader { layer3, ..e });
                    ui.end_row();
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

    if let Some((r, placed)) = placed_change {
        let (x, y, tile_position) = placed.split();
        let (fg_position, bg_position, relative) = placed.camera.to_bits(false);
        let header = SecondaryHeader {
            entrance_screen: placed.screen,
            entrance_x: x,
            entrance_y: y,
            fg_position,
            bg_position,
            ..e
        };
        let settings = LevelSettings {
            tile_position,
            relative,
            ..level.settings
        };
        let mut edits = Vec::new();
        if header != e {
            edits.push(Edit::SetEntrance(header));
        }
        if settings != level.settings {
            edits.push(Edit::SetSettings(settings));
        }
        *change = Some(Change::new(&r, "Change the main entrance", edits));
    }

    lunar_settings(ui, level, change);
    before_secondary(ui, change);

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
                        let s = e.settings;
                        let placed = Placed::new(
                            e.screen,
                            e.x,
                            e.y,
                            s.tile_position,
                            Camera::from_bits(e.fg_position, e.bg_position, s.relative, true),
                        );
                        if let Some((r, placed)) =
                            placed_fields(ui, ("secondary", index), placed, vertical)
                        {
                            let (x, y, tile_position) = placed.split();
                            let (fg_position, bg_position, relative) = placed.camera.to_bits(true);
                            let entrance = Entrance {
                                screen: placed.screen,
                                x,
                                y,
                                fg_position,
                                bg_position,
                                settings: EntranceSettings {
                                    tile_position,
                                    relative,
                                    ..s
                                },
                                ..e
                            };
                            let edit = Edit::ReplaceEntrance { index, entrance };
                            *change = Some(Change::new(&r, "Change entrance", vec![edit]));
                        }
                        ui.label("Action");
                        let mut action = e.action;
                        let r = choice(
                            ui,
                            &format!("action-{index}"),
                            &mut action,
                            0..=7,
                            names::entrance_action,
                        );
                        if r.changed() {
                            let edit = Edit::ReplaceEntrance {
                                index,
                                entrance: Entrance { action, ..e },
                            };
                            *change = Some(Change::new(&r, "Change entrance", vec![edit]));
                        }
                        ui.end_row();
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

/// Lunar Magic's graphics list and palette for the level, and its
/// ExAnimation, which only its file edits.
fn graphics_and_palette(
    ui: &mut egui::Ui,
    level: &Level,
    game: Option<&kobo_core::palette::CustomPalette>,
    change: &mut Option<Change>,
    show_table: &mut Option<&'static str>,
) {
    use kobo_core::exgfx::{self, GraphicsList};
    use kobo_core::palette::Color15;

    egui::CollapsingHeader::new(RichText::new("Graphics and palette").strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.label(RichText::new("GRAPHICS").small().color(theme::MUTED));
            match level.graphics {
                None => {
                    ui.label(RichText::new("The tilesets' own files.").color(theme::MUTED));
                    let r = ui.button("Give the level a list of its own");
                    if r.clicked() {
                        *change = Some(Change::new(
                            &r,
                            "Add a graphics list",
                            vec![Edit::SetGraphics(Some(GraphicsList::DEFAULT))],
                        ));
                    }
                }
                Some(list) => {
                    let set = |r: &Response, list: GraphicsList| {
                        Change::new(r, "Change the graphics list", vec![Edit::SetGraphics(Some(list))])
                    };
                    let an2 = list.0[exgfx::slot::AN2];
                    for (bit, text, tip) in [
                        (exgfx::BYPASS, "Its files replace the tilesets'", "Super GFX bypass: the slots below load in place of the tilesets' files"),
                        (exgfx::LAYER3_FILES, "Layer 3's files too", "LG1 to LG4 load layer 3's graphics"),
                        (exgfx::LAYER3_TILEMAP, "Layer 3's tilemap too", "LT3 loads layer 3's tilemap"),
                    ] {
                        let mut on = an2 & bit != 0;
                        let r = ui.checkbox(&mut on, text).on_hover_text(tip);
                        if r.changed() {
                            let mut new = list;
                            new.0[exgfx::slot::AN2] = if on { an2 | bit } else { an2 & !bit };
                            *change = Some(set(&r, new));
                        }
                    }
                    Grid::new("graphics-slots")
                        .num_columns(4)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for (i, name) in GraphicsList::SLOTS.iter().enumerate() {
                                ui.label(RichText::new(*name).monospace());
                                let word = list.0[i];
                                let mut file = list.file(i);
                                let r = ui
                                    .add(
                                        DragValue::new(&mut file)
                                            .range(0..=exgfx::EXGFX_LAST)
                                            .speed(0.1)
                                            .hexadecimal(3, false, true),
                                    )
                                    .on_hover_text("A GFX file (00-7E), ExGFX (80-FFF), or 7F for none");
                                if r.changed() {
                                    let mut new = list;
                                    new.0[i] = word & 0xF000 | file;
                                    *change = Some(set(&r, new));
                                }
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        let r = ui.button("No list: the tilesets' files");
                        if r.clicked() {
                            *change = Some(Change::new(&r, "Remove the graphics list", vec![Edit::SetGraphics(None)]));
                        }
                        if ui.button("Show in the file").clicked() {
                            *show_table = Some("graphics");
                        }
                    });
                }
            }

            ui.add_space(6.0);
            ui.label(RichText::new("PALETTE").small().color(theme::MUTED));
            match &level.palette {
                None => {
                    ui.label(
                        RichText::new("The game's, by the header's BG, FG, and sprite palettes.")
                            .color(theme::MUTED),
                    );
                    let r = ui
                        .add_enabled(game.is_some(), egui::Button::new("Give the level a palette of its own"))
                        .on_hover_text("Starting from the game's colours for it");
                    if r.clicked()
                        && let Some(custom) = game
                    {
                        *change = Some(Change::new(
                            &r,
                            "Add a palette",
                            vec![Edit::SetPalette(Some(Box::new(custom.clone())))],
                        ));
                    }
                }
                Some(custom) => {
                    let chosen_id = ui.make_persistent_id("palette-chosen");
                    let mut chosen: Option<usize> = ui.data(|d| d.get_temp(chosen_id)).flatten();
                    let cell = 12.0;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(cell * 16.0, cell * 16.0),
                        egui::Sense::click(),
                    );
                    let painter = ui.painter_at(rect);
                    for (i, color) in custom.palette.colors.iter().enumerate() {
                        let [r, g, b] = color.to_rgb8();
                        let at = rect.min + egui::vec2((i % 16) as f32 * cell, (i / 16) as f32 * cell);
                        let r_cell = egui::Rect::from_min_size(at, egui::vec2(cell, cell));
                        painter.rect_filled(r_cell, 0, egui::Color32::from_rgb(r, g, b));
                        if chosen == Some(i) {
                            painter.rect_stroke(r_cell, 0, egui::Stroke::new(2.0, egui::Color32::WHITE), egui::StrokeKind::Inside);
                        }
                    }
                    let index_at = |p: egui::Pos2| {
                        let d = (p - rect.min) / cell;
                        (d.x >= 0.0 && d.y >= 0.0 && d.x < 16.0 && d.y < 16.0)
                            .then(|| d.y as usize * 16 + d.x as usize)
                    };
                    if let Some(i) = response.hover_pos().and_then(index_at) {
                        let [r, g, b] = custom.palette.colors[i].to_rgb8();
                        response.clone().on_hover_text(format!(
                            "Row {:X}, colour {:X}: #{r:02X}{g:02X}{b:02X}",
                            i / 16,
                            i % 16
                        ));
                    }
                    if response.clicked() {
                        chosen = response.interact_pointer_pos().and_then(index_at);
                        ui.data_mut(|d| d.insert_temp(chosen_id, chosen));
                    }
                    let pressed_at = ui.input(|i| i.pointer.press_start_time()).unwrap_or_default();
                    let dragging = ui.input(|i| i.pointer.any_down());
                    let mut edit_color = |ui: &mut egui::Ui, label: &str, color: Color15, index: Option<usize>| {
                        ui.label(label);
                        let [r, g, b] = color.to_rgb8();
                        let mut c = egui::Color32::from_rgb(r, g, b);
                        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
                            let new = Color15::from_rgb5(c.r() >> 3, c.g() >> 3, c.b() >> 3);
                            if new != color {
                                let mut changed = custom.clone();
                                match index {
                                    Some(i) => changed.palette.colors[i] = new,
                                    None => changed.back_area = new,
                                }
                                *change = Some(Change {
                                    widget: egui::Id::new(("palette", index, pressed_at.to_bits())),
                                    dragging,
                                    label: "Change a colour".into(),
                                    edits: vec![Edit::SetPalette(Some(Box::new(changed)))],
                                    select: None,
                                });
                            }
                        }
                    };
                    match chosen {
                        Some(i) => edit_color(
                            ui,
                            &format!("Row {:X}, colour {:X}", i / 16, i % 16),
                            custom.palette.colors[i],
                            Some(i),
                        ),
                        None => {
                            ui.label(RichText::new("Click a colour to change it.").small().color(theme::MUTED));
                            ui.collapsing("Back area colour", |ui| {
                                edit_color(ui, "", custom.back_area, None);
                            });
                        }
                    }
                    ui.horizontal(|ui| {
                        let r = ui.button("Back to the game's");
                        if r.clicked() {
                            *change = Some(Change::new(&r, "Remove the palette", vec![Edit::SetPalette(None)]));
                        }
                        if ui.button("Show in the file").clicked() {
                            *show_table = Some("palette");
                        }
                    });
                }
            }

            ui.add_space(6.0);
            ui.label(RichText::new("ANIMATION").small().color(theme::MUTED));
            match &level.animation {
                None => {
                    ui.label(RichText::new("No ExAnimation of its own.").color(theme::MUTED));
                }
                Some(list) => {
                    ui.label(format!("ExAnimation in {} slots, edited in the file.", list.count));
                    if ui.button("Show in the file").clicked() {
                        *show_table = Some("animation");
                    }
                }
            }
        });
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
                        (
                            "Slippery",
                            "The level is slippery from the main entrance on",
                            settings.slippery,
                            |s, v| s.slippery = v,
                        ),
                        (
                            "Water",
                            "The player swims from the main entrance on",
                            settings.water,
                            |s, v| s.water = v,
                        ),
                        (
                            "Face left",
                            "The player enters facing left",
                            settings.face_left,
                            |s, v| s.face_left = v,
                        ),
                        (
                            "Smart spawning",
                            "Lunar Magic's sprite loader spawns by the range below",
                            settings.smart_spawn,
                            |s, v| s.smart_spawn = v,
                        ),
                        (
                            "Lunar Magic sets screens",
                            "Lunar Magic sets the level's screen count from its objects when \
                             it saves the level; Kobo builds the count the header gives",
                            settings.auto_screens,
                            |s, v| s.auto_screens = v,
                        ),
                    ];
                    for (label, tip, value, with) in flags {
                        ui.label(label).on_hover_text(tip);
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
                    ui.label("Background starts")
                        .on_hover_text("Where layer 2 starts, up and down, when the level is entered");
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
                            let fields: [MidwayField; 2] = [
                                ("Its X tile", u16::from(m.x), 31, |m, v| MidwayEntrance { x: v as u8, ..m }),
                                ("Its Y tile", m.y, 1023, |m, v| MidwayEntrance { y: v, ..m }),
                            ];
                            for (label, value, max, change_to) in fields {
                                ui.label(label);
                                let mut v = value;
                                let r = number_field(ui, &mut v, 0, max, false);
                                set(&r, "Change midway entrance", with(change_to(m, v)), change);
                                ui.end_row();
                            }
                            ui.label("Its action");
                            let mut action = m.action;
                            let r = choice(ui, "midway-action", &mut action, 0..=7, names::entrance_action);
                            set(&r, "Change midway entrance", with(MidwayEntrance { action, ..m }), change);
                            ui.end_row();
                            let camera = Camera::from_bits(m.fg_position, m.bg_position, m.relative, false);
                            if let Some((r, camera)) = camera_fields(ui, "midway", camera) {
                                let (fg_position, bg_position, relative) = camera.to_bits(false);
                                let m = MidwayEntrance { fg_position, bg_position, relative, ..m };
                                set(&r, "Change midway entrance", with(m), change);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_by_tile_splits_into_the_settings_bits() {
        let camera = Camera::Relative(-3);
        let placed = Placed::new(4, 5, 9, Some((1, 2)), camera);
        assert_eq!((placed.x, placed.y), (13, 41));
        assert_eq!(placed.split(), (5, 9, Some((1, 2))));
        let placed = Placed::new(4, 5, 9, None, camera);
        assert_eq!(placed.split(), (5, 9, None));
    }
}
