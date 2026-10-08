//! The overworld window: the project's overworld
//! (`edit::OverworldDocument`), drawn as a player on the chosen map sees
//! it, from a build of the project on a worker thread
//! (`render::render_overworld`). Layer 1's 16x16 tiles are drawn on with a
//! brush, a right click picks the brush and the level tile under it, and
//! the level tile chosen has its name edited.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui::{self, Color32, RichText, Sense, TextureHandle, TextureOptions};
use kobo_core::edit::Workspace;
use kobo_core::overworld::{
    Event, EventBlock, LAYER1_SIZE, LAYER2_SIZE, Overworld, Start, event_vram, layer1_index,
    layer1_place, layer2_index, layer2_place,
};
use kobo_core::render::OVERWORLD_SIDE;
use kobo_core::source::overworld::{name_text, parse_name};

use crate::app::App;
use crate::theme;

/// What a picture is of: the overworld's changes so far, and the map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub generation: u64,
    pub submap: u8,
    /// The last event passed, all those before it passed too.
    pub passed: Option<u8>,
}

/// Events `0` to `last` as the game's bits for them.
fn passed_bits(last: Option<u8>) -> [u8; 0x0F] {
    let mut bits = [0; 0x0F];
    if let Some(last) = last {
        for e in 0..=usize::from(last.min(0x77)) {
            bits[e / 8] |= 0x80 >> (e % 8);
        }
    }
    bits
}

/// Draws pictures of builds of the project on a thread of its own, the
/// newest request only.
struct Worker {
    jobs: Sender<(Key, Workspace)>,
    done: Receiver<(Key, Result<egui::ColorImage, String>)>,
}

impl Worker {
    fn new(ctx: egui::Context) -> Self {
        let (jobs, inbox) = mpsc::channel::<(Key, Workspace)>();
        let (outbox, done) = mpsc::channel();
        thread::Builder::new()
            .name("kobo-overworld".into())
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
                            let passed = passed_bits(key.passed);
                            kobo_core::render::render_overworld_passed(&rom, key.submap, &passed)
                                .map_err(|e| e.to_string())
                        })
                        .map(|img| {
                            egui::ColorImage::from_rgb(
                                [img.width as usize, img.height as usize],
                                &img.pixels.iter().flatten().copied().collect::<Vec<u8>>(),
                            )
                        });
                    if outbox.send((key, result)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the overworld thread starts");
        Self { jobs, done }
    }
}

/// The window's state.
#[derive(Default)]
pub struct OverworldEditor {
    pub open: bool,
    /// The map shown: 0 the main map, 1-6 the submaps.
    pub submap: u8,
    /// The layer drawn on: 1 (16x16 tiles) or 2 (8x8 tiles).
    pub layer: u8,
    /// The layer 1 tile drawn with, its page in the high byte.
    pub brush: u16,
    /// The layer 2 tile drawn with: the number in the low byte, the
    /// properties in the high.
    pub brush2: u16,
    /// Pixels a picture pixel takes.
    zoom: u8,
    picture: Option<(Key, TextureHandle)>,
    /// Why the newest picture could not be drawn.
    failed: Option<(Key, String)>,
    worker: Option<Worker>,
    requested: Option<Key>,
    /// The tile the stroke under way last drew, if one is.
    stroke: Option<usize>,
    /// The tile chosen, by map, x, and y, and its name as being edited.
    chosen: Option<(u8, u8, u8)>,
    name: String,
    /// The 8x8 tile chosen, by map, x, and y.
    chosen2: Option<(u8, u8, u8)>,
    /// Whether an event is shown, and which: the map with it and those
    /// before it passed, its tiles outlined, and layer 2 drawn in its
    /// blocks.
    pub show_event: bool,
    pub event: u8,
}

impl OverworldEditor {
    /// Whether the newest picture asked for is drawn, or failed.
    pub fn drawn(&self) -> bool {
        self.requested.is_some_and(|key| {
            self.picture.as_ref().is_some_and(|(k, _)| *k == key)
                || self.failed.as_ref().is_some_and(|(k, _)| *k == key)
        })
    }

    pub fn forget(&mut self) {
        self.picture = None;
        self.failed = None;
        self.requested = None;
        self.stroke = None;
        self.chosen = None;
        self.chosen2 = None;
    }
}

/// The level a translevel enters, as the layout numbers them: its own
/// number up to `$24`, `$101` on from `$25`.
fn level_of(translevel: u8) -> u16 {
    if translevel < 0x25 {
        u16::from(translevel)
    } else {
        0x101 + u16::from(translevel - 0x25)
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.overworld_editor.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Overworld")
        .open(&mut open)
        .default_width(560.0)
        .default_height(680.0)
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.overworld_editor.open = false;
    }
}

/// What the window asks of the document.
enum Change {
    /// A tile of layer 1 or 2, by its index in the layer's tiles.
    Draw {
        layer: u8,
        index: usize,
        tile: u16,
        amend: bool,
    },
    Rename {
        translevel: u8,
        name: [u8; kobo_core::overworld::NAME_TILES],
    },
    /// A layer 1 place's translevel and direction byte.
    Level {
        index: usize,
        translevel: u8,
        directions: u8,
    },
    /// The event passing a translevel makes.
    Event { translevel: u8, event: u8 },
    /// A translevel's settings at a new game.
    Flags { translevel: u8, flags: u8 },
    /// Where a new game puts Mario (0) or Luigi (1).
    Start { player: usize, start: Start },
    /// Event `event` as `to`.
    Event2 {
        label: &'static str,
        event: u8,
        to: Box<Event>,
        amend: bool,
    },
    /// The path reveal speed: the event path fade off and paths revealed
    /// at it, or the game's fade.
    RevealSpeed(Option<u8>),
    /// Lunar Magic's FG1-2 merge on or off.
    Merge(bool),
    /// The overworld as changed by one of its lists.
    Edited(crate::overworld_lists::Edited),
    /// The overworld's ExAnimation as `to`.
    Animation {
        label: String,
        to: Option<Box<kobo_core::exanimation::OverworldAnimation>>,
    },
}

impl Change {
    fn apply(self, app: &mut App) {
        match self {
            Change::Draw {
                layer,
                index,
                tile,
                amend,
            } => app.change_overworld("Draw on the overworld", amend, |ow| {
                if layer == 1 {
                    ow.layer1[index] = tile;
                } else {
                    ow.layer2[index] = tile;
                }
            }),
            Change::Rename { translevel, name } => {
                app.change_overworld("Rename a level", false, |ow| {
                    if let Some(slot) = ow.names.get_mut(usize::from(translevel)) {
                        *slot = name;
                    }
                })
            }
            Change::Level {
                index,
                translevel,
                directions,
            } => app.change_overworld("Change a level tile", false, |ow| {
                ow.translevels[index] = translevel;
                ow.directions[index] = directions;
            }),
            Change::Event { translevel, event } => {
                app.change_overworld("Change a level's event", false, |ow| {
                    if let Some(slot) = ow.level_events.get_mut(usize::from(translevel)) {
                        *slot = event;
                    }
                })
            }
            Change::Flags { translevel, flags } => {
                app.change_overworld("Change a level's settings", false, |ow| {
                    if let Some(slot) = ow.level_flags.get_mut(usize::from(translevel)) {
                        *slot = flags;
                    }
                })
            }
            Change::Start { player, start } => {
                app.change_overworld("Move the start", false, |ow| ow.start[player] = start)
            }
            Change::Event2 {
                label,
                event,
                to,
                amend,
            } => app.change_overworld(label, amend, |ow| {
                // An event that no longer fits the event tile data is left
                // as it was.
                let _ = ow.set_event(usize::from(event), *to);
            }),
            Change::RevealSpeed(speed) => {
                app.change_overworld_settings("Change the path reveal speed", |c| {
                    c.reveal_speed = speed;
                })
            }
            Change::Merge(on) => app.change_overworld(
                if on {
                    "Merge FG1-2 into SP3-4"
                } else {
                    "Unmerge FG1-2 from SP3-4"
                },
                false,
                |ow| ow.merge_fg = on,
            ),
            Change::Edited(edited) => {
                let to = *edited.to;
                app.change_overworld(&edited.label, false, |ow| *ow = to)
            }
            Change::Animation { label, to } => {
                app.change_overworld(&label, false, |ow| ow.animation = to.map(|a| *a))
            }
        }
    }
}

fn show(app: &mut App, ui: &mut egui::Ui) {
    if let Err(e) = app.overworld_document() {
        ui.label(RichText::new(e).color(theme::ERROR));
        return;
    }
    let mut state = std::mem::take(&mut app.overworld_editor);
    if state.zoom == 0 {
        state.zoom = 1;
    }
    if state.layer == 0 {
        state.layer = 1;
    }
    let worker = state
        .worker
        .get_or_insert_with(|| Worker::new(ui.ctx().clone()));
    while let Ok((key, result)) = worker.done.try_recv() {
        match result {
            Ok(image) => {
                let texture = ui
                    .ctx()
                    .load_texture("overworld", image, TextureOptions::NEAREST);
                state.picture = Some((key, texture));
                state.failed = None;
            }
            Err(e) => state.failed = Some((key, e)),
        }
    }
    let key = Key {
        generation: app.overworld_generation(),
        submap: state.submap,
        passed: state.show_event.then_some(state.event),
    };
    if state.requested != Some(key)
        && let Some(workspace) = app.workspace_copy()
    {
        let _ = worker.jobs.send((key, workspace));
        state.requested = Some(key);
    }
    let change = contents(app, &mut state, key, ui);
    app.overworld_editor = state;
    if let Some(change) = change {
        change.apply(app);
    }
}

/// The tile at (`x`, `y`) of a layer's grid on `map`, by its index in the
/// layer's tiles.
fn index_of(layer: u8, map: u8, x: u8, y: u8) -> usize {
    if layer == 1 {
        layer1_index(map, x, y)
    } else {
        layer2_index(map, x, y)
    }
}

fn tile_of(overworld: &Overworld, layer: u8, index: usize) -> u16 {
    if layer == 1 {
        overworld.layer1[index]
    } else {
        overworld.layer2[index]
    }
}

fn contents(app: &App, state: &mut OverworldEditor, key: Key, ui: &mut egui::Ui) -> Option<Change> {
    let document = app.open_overworld()?;
    let overworld = document.overworld();
    let map = u8::from(state.submap != 0);
    let mut change = None;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("overworld-map")
            .selected_text(kobo_core::names::submap(state.submap).unwrap_or("?"))
            .show_ui(ui, |ui| {
                for submap in 0..=6 {
                    let name = kobo_core::names::submap(submap).unwrap_or("?");
                    ui.selectable_value(&mut state.submap, submap, name);
                }
            });
        ui.separator();
        ui.selectable_value(&mut state.layer, 1, "Layer 1")
            .on_hover_text("16x16 tiles: paths, level tiles, and what is walked on");
        ui.selectable_value(&mut state.layer, 2, "Layer 2")
            .on_hover_text("8x8 tiles: the picture behind");
        ui.separator();
        ui.label("Brush");
        if state.layer == 1 {
            ui.add(
                egui::DragValue::new(&mut state.brush)
                    .range(0..=0x1FF)
                    .hexadecimal(3, false, true),
            )
            .on_hover_text("The layer 1 tile drawn with; right-click a tile to pick it");
        } else {
            ui.add(
                egui::DragValue::new(&mut state.brush2)
                    .range(0..=0xFFFF)
                    .hexadecimal(4, false, true),
            )
            .on_hover_text(
                "The layer 2 tile drawn with: properties, then the number; right-click a tile to pick it",
            );
        }
        ui.separator();
        ui.selectable_value(&mut state.zoom, 1, "1x");
        ui.selectable_value(&mut state.zoom, 2, "2x");
    });
    let changes = document.changes();
    let note = if changes.is_empty() {
        "Left-drag draws tiles; right-click picks a tile and chooses its level tile.".to_owned()
    } else {
        format!(
            "The project changes the overworld: {} layer 1 rows, {} layer 2 rows, {} level tiles, {} names.",
            changes.layer1.len(),
            changes.layer2.len(),
            changes.levels.len(),
            changes.names.len()
        )
    };
    ui.label(RichText::new(note).small().color(theme::MUTED));
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.show_event, "Event")
            .on_hover_text("The map with an event and those before it passed, the event's tiles outlined; layer 2 is then drawn in its blocks");
        if state.show_event {
            ui.add(egui::DragValue::new(&mut state.event).range(0..=0x77).hexadecimal(2, false, true));
            let event = &overworld.event_list()[usize::from(state.event)];
            ui.label(
                RichText::new(format!(
                    "{} layer 2 blocks{}",
                    event.blocks.len(),
                    if event.layer1 == (0, 0) { ", no layer 1 tile" } else { ", a layer 1 tile" }
                ))
                .small()
                .color(theme::MUTED),
            );
        }
    });
    let event = state
        .show_event
        .then(|| overworld.event_list().swap_remove(usize::from(state.event)));
    if let Some((failed, e)) = &state.failed
        && *failed == key
    {
        ui.label(RichText::new(e).color(theme::ERROR));
    }
    let zoom = f32::from(state.zoom);
    let side = OVERWORLD_SIDE as f32 * zoom;
    // A grid cell of the layer drawn on.
    let (cell, cells) = if state.layer == 1 {
        (16.0 * zoom, LAYER1_SIZE)
    } else {
        (8.0 * zoom, LAYER2_SIZE)
    };
    let mut hovered = None;
    egui::ScrollArea::both().max_height(560.0).show(ui, |ui| {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "The overworld's map")
        });
        match &state.picture {
            Some((shown, texture)) => {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                if *shown != key {
                    ui.painter()
                        .rect_filled(rect, 0, Color32::from_black_alpha(40));
                }
            }
            None => {
                ui.painter().rect_filled(rect, 0, theme::PANEL);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drawing the overworld…",
                    egui::FontId::proportional(14.0),
                    theme::MUTED,
                );
            }
        }
        let at = |pos: egui::Pos2, cell: f32, cells: u8| -> Option<(u8, u8)> {
            let d = pos - rect.min;
            let (x, y) = ((d.x / cell).floor(), (d.y / cell).floor());
            (x >= 0.0 && y >= 0.0 && x < f32::from(cells) && y < f32::from(cells))
                .then_some((x as u8, y as u8))
        };
        let outline = |x: u8, y: u8, cell: f32, stroke: egui::Stroke| {
            let r = egui::Rect::from_min_size(
                rect.min + egui::vec2(f32::from(x) * cell, f32::from(y) * cell),
                egui::vec2(cell, cell),
            );
            ui.painter()
                .rect_stroke(r, 0, stroke, egui::StrokeKind::Inside);
        };
        if let Some(pos) = response.hover_pos() {
            if let Some((x, y)) = at(pos, cell, cells) {
                outline(x, y, cell, egui::Stroke::new(1.0, Color32::WHITE));
            }
            hovered = at(pos, 16.0 * zoom, LAYER1_SIZE);
        }
        if let Some((m, x, y)) = state.chosen
            && m == map
        {
            outline(x, y, 16.0 * zoom, egui::Stroke::new(2.0, theme::ACCENT));
        }
        if let Some(event) = &event {
            for block in &event.blocks {
                for offset in block.offsets() {
                    let (m, x, y) = layer2_place(offset);
                    if m == map {
                        outline(x, y, 8.0 * zoom, egui::Stroke::new(1.0, theme::SPRITE));
                    }
                }
            }
            if event.layer1 != (0, 0) {
                let (m, x, y) = layer1_place(usize::from(event.layer1.0));
                if m == map {
                    outline(x, y, 16.0 * zoom, egui::Stroke::new(2.0, theme::OK));
                }
            }
        }
        let primary = ui.input(|i| i.pointer.primary_down());
        if primary && (response.is_pointer_button_down_on() || response.dragged()) {
            if let Some((x, y)) = response
                .interact_pointer_pos()
                .and_then(|p| at(p, cell, cells))
            {
                let index = index_of(state.layer, map, x, y);
                if state.stroke != Some(index) {
                    let amend = state.stroke.is_some();
                    change = match (&event, state.layer) {
                        // Layer 2 with an event shown: the event's block
                        // there, if it has one.
                        (Some(event), 2) => {
                            let offset = (index * 2) as u16;
                            let at = event.blocks.iter().enumerate().find_map(|(b, block)| {
                                let t = block.offsets().iter().position(|&o| o == offset)?;
                                Some((b, t))
                            });
                            at.map(|(b, t)| {
                                let mut to = event.clone();
                                to.blocks[b].tiles[t] = state.brush2;
                                Change::Event2 {
                                    label: "Draw in an event",
                                    event: state.event,
                                    to: Box::new(to),
                                    amend,
                                }
                            })
                        }
                        _ => Some(Change::Draw {
                            layer: state.layer,
                            index,
                            tile: if state.layer == 1 {
                                state.brush
                            } else {
                                state.brush2
                            },
                            amend,
                        }),
                    };
                    state.stroke = Some(index);
                }
            }
        } else {
            state.stroke = None;
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            if let Some((x, y)) = at(pos, cell, cells) {
                let picked = tile_of(overworld, state.layer, index_of(state.layer, map, x, y));
                if state.layer == 1 {
                    state.brush = picked;
                } else {
                    state.brush2 = picked;
                }
            }
            if let Some((x, y)) = at(pos, 8.0 * zoom, LAYER2_SIZE) {
                state.chosen2 = Some((map, x, y));
            }
            if let Some((x, y)) = at(pos, 16.0 * zoom, LAYER1_SIZE) {
                state.chosen = Some((map, x, y));
                let t = overworld.translevels[layer1_index(map, x, y)];
                state.name = overworld
                    .names
                    .get(usize::from(t))
                    .map(name_text)
                    .unwrap_or_default();
            }
        }
    });
    let shown = hovered.map(|(x, y)| (map, x, y)).or(state.chosen);
    if let Some((m, x, y)) = shown {
        let index = layer1_index(m, x, y);
        let t = overworld.translevels[index];
        let mut line = format!("{x}, {y} · layer 1 tile {:03X}", overworld.layer1[index]);
        if t != 0 {
            let name = overworld
                .names
                .get(usize::from(t))
                .map(name_text)
                .unwrap_or_default();
            line += &format!(
                " · level {:03X} (translevel {t:02X}) · {}",
                level_of(t),
                name.trim_end()
            );
        }
        ui.label(RichText::new(line).monospace());
    }
    if let Some(event) = &event {
        ui.horizontal(|ui| {
            let e = state.event;
            if let Some((m, x, y)) = state.chosen2 {
                let place = (layer2_index(m, x, y) * 2) as u16;
                let inside = event
                    .blocks
                    .iter()
                    .position(|b| b.offsets().contains(&place));
                for side in [2usize, 6] {
                    let mut block = EventBlock {
                        place,
                        tiles: vec![0; side * side],
                    };
                    let offsets = block.offsets();
                    for (tile, offset) in block.tiles.iter_mut().zip(offsets) {
                        *tile = overworld.layer2[usize::from(offset) / 2];
                    }
                    if ui
                        .button(format!("Add a {side}x{side} block"))
                        .on_hover_text(format!("A block of event {e:02X} from the chosen 8x8 tile, of the tiles there now"))
                        .clicked()
                    {
                        let mut to = event.clone();
                        to.blocks.push(block);
                        change = Some(Change::Event2 {
                            label: "Add an event block",
                            event: e,
                            to: Box::new(to),
                            amend: false,
                        });
                    }
                }
                if let Some(b) = inside
                    && ui.button("Remove the block").clicked()
                {
                    let mut to = event.clone();
                    to.blocks.remove(b);
                    change = Some(Change::Event2 {
                        label: "Remove an event block",
                        event: e,
                        to: Box::new(to),
                        amend: false,
                    });
                }
            }
            if let Some((m, x, y)) = state.chosen {
                let here = (layer1_index(m, x, y) as u16, event_vram(x, y));
                if ui
                    .add_enabled(event.layer1 != here, egui::Button::new("Its layer 1 tile here"))
                    .on_hover_text(format!("The layer 1 tile event {e:02X} turns into another, by the reveal list"))
                    .clicked()
                {
                    let mut to = event.clone();
                    to.layer1 = here;
                    change = Some(Change::Event2 {
                        label: "Move an event's layer 1 tile",
                        event: e,
                        to: Box::new(to),
                        amend: false,
                    });
                }
            }
            if event.layer1 != (0, 0) && ui.button("No layer 1 tile").clicked() {
                let mut to = event.clone();
                to.layer1 = (0, 0);
                change = Some(Change::Event2 {
                    label: "Take an event's layer 1 tile away",
                    event: e,
                    to: Box::new(to),
                    amend: false,
                });
            }
            ui.label(RichText::new("Further tiles").strong());
            if let Some(edited) =
                crate::overworld_lists::extras(
                    ui,
                    overworld,
                    e,
                    state.chosen,
                    state.chosen2,
                    state.brush,
                )
            {
                change = Some(Change::Edited(edited));
            }
        });
    }
    if let Some((m, x, y)) = state.chosen {
        let index = layer1_index(m, x, y);
        let (t, d) = (overworld.translevels[index], overworld.directions[index]);
        ui.horizontal(|ui| {
            ui.label(format!("The tile at {x}, {y}:"));
            let (mut translevel, mut directions) = (t, d);
            ui.label("translevel");
            let a = ui
                .add(
                    egui::DragValue::new(&mut translevel)
                        .range(0..=0x5F)
                        .hexadecimal(2, false, true),
                )
                .on_hover_text("The level the tile enters (0 for none): up to 24 its own number, 101 on from 25");
            ui.label("directions");
            let b = ui
                .add(egui::DragValue::new(&mut directions).hexadecimal(2, false, true))
                .on_hover_text("The tile's direction byte, which the game copies from the translevel's when it numbers them");
            if (a.changed() || b.changed()) && (translevel, directions) != (t, d) {
                change = Some(Change::Level {
                    index,
                    translevel,
                    directions,
                });
            }
        });
        ui.horizontal(|ui| {
            for (player, who) in ["Mario", "Luigi"].iter().enumerate() {
                let here = Start {
                    submap: state.submap,
                    x: u16::from(x) * 16 + 8,
                    y: u16::from(y) * 16 + 8,
                };
                let starts = overworld.start[player] == here;
                if ui
                    .add_enabled(!starts, egui::Button::new(format!("{who} starts here")))
                    .on_hover_text("Where a new game puts the player: this tile, on the map shown")
                    .on_disabled_hover_text(format!("A new game puts {who} here"))
                    .clicked()
                {
                    change = Some(Change::Start {
                        player,
                        start: here,
                    });
                }
            }
        });
        if t != 0 {
            ui.horizontal(|ui| {
                let was = overworld.level_events[usize::from(t)];
                let mut event = was;
                ui.label(format!("Passing level {:03X} makes event", level_of(t)));
                let response = ui
                    .add(
                        egui::DragValue::new(&mut event)
                            .range(0..=0x77)
                            .hexadecimal(2, false, true),
                    )
                    .on_hover_text("Its secret exit makes the next one");
                if response.changed() && event != was {
                    change = Some(Change::Event {
                        translevel: t,
                        event,
                    });
                }
            });
            ui.horizontal(|ui| {
                let was = overworld.level_flags[usize::from(t)];
                let mut flags = was;
                ui.label("At a new game: directions");
                let mut directions = flags & 0x0F;
                ui.add(
                    egui::DragValue::new(&mut directions)
                        .range(0..=0x0F)
                        .hexadecimal(1, false, true),
                )
                .on_hover_text("The directions open from the tile at a new game (bits 0 to 3)");
                flags = flags & !0x0F | directions;
                let mut save = flags & kobo_core::overworld::FLAG_SAVE != 0;
                ui.checkbox(&mut save, "save prompt").on_hover_text(
                    "The save prompt comes up when the level is passed (Lunar Magic's flag)",
                );
                let mut no_entry = flags & kobo_core::overworld::FLAG_NO_ENTRY != 0;
                ui.checkbox(&mut no_entry, "no entry once passed")
                    .on_hover_text(
                        "The level cannot be entered once it is passed (Lunar Magic's flag)",
                    );
                flags = flags
                    & !(kobo_core::overworld::FLAG_SAVE | kobo_core::overworld::FLAG_NO_ENTRY)
                    | if save {
                        kobo_core::overworld::FLAG_SAVE
                    } else {
                        0
                    }
                    | if no_entry {
                        kobo_core::overworld::FLAG_NO_ENTRY
                    } else {
                        0
                    };
                if flags != was {
                    change = Some(Change::Flags {
                        translevel: t,
                        flags,
                    });
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!("Level {:03X}'s name", level_of(t)));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.name)
                        .desired_width(220.0)
                        .font(egui::TextStyle::Monospace),
                );
                let commit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                match parse_name("name", &state.name) {
                    Ok(name) => {
                        if commit && overworld.names.get(usize::from(t)) != Some(&name) {
                            change = Some(Change::Rename {
                                translevel: t,
                                name,
                            });
                        }
                    }
                    Err(e) => {
                        ui.label(RichText::new(e.to_string()).small().color(theme::ERROR));
                    }
                }
            });
        }
    }
    if let Some(c) = settings(ui, state.submap, overworld, document.changes()) {
        change = Some(c);
    }
    for (title, salt) in [
        ("Sprites", "overworld-sprites-header"),
        ("Reveal list", "overworld-reveal-header"),
        ("Crushed tiles", "overworld-crush-header"),
    ] {
        egui::CollapsingHeader::new(title)
            .id_salt(salt)
            .show(ui, |ui| {
                let edited = match title {
                    "Sprites" => crate::overworld_lists::sprites(ui, overworld, state.chosen),
                    "Reveal list" => crate::overworld_lists::reveal(ui, overworld),
                    _ => crate::overworld_lists::crush(ui, overworld, state.chosen),
                };
                if let Some(edited) = edited {
                    change = Some(Change::Edited(edited));
                }
            });
    }
    change
}

/// The overworld's options and the shown submap's ExAnimation.
fn settings(
    ui: &mut egui::Ui,
    submap: u8,
    overworld: &Overworld,
    changes: &kobo_core::overworld::Changes,
) -> Option<Change> {
    use kobo_core::exanimation::{List, OverworldAnimation};
    let mut change = None;
    egui::CollapsingHeader::new("Options")
        .id_salt("overworld-options")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut fade_off = changes.reveal_speed.is_some();
                let r = ui.checkbox(&mut fade_off, "Reveal paths at a speed").on_hover_text(
                    "Lunar Magic's option: the event path fade off, and each step of a path revealed at the speed given (a step every 0x40)",
                );
                if r.changed() {
                    change = Some(Change::RevealSpeed(fade_off.then_some(0x10)));
                }
                if let Some(speed) = changes.reveal_speed {
                    let mut value = speed;
                    let r = ui.add(egui::DragValue::new(&mut value).range(1..=0x40).hexadecimal(2, false, true));
                    if r.changed() && value != speed {
                        change = Some(Change::RevealSpeed(Some(value)));
                    }
                }
            });
            let mut merged = overworld.merge_fg;
            let r = ui.checkbox(&mut merged, "Merge FG1-2 into SP3-4").on_hover_text(
                "Lunar Magic's option: layers 1 and 2 take their first 256 tiles from SP3-4, which then hold FG1-2's files (the graphics lists say which), leaving two more FG slots",
            );
            if r.changed() {
                change = Some(Change::Merge(merged));
            }
        });
    let name = kobo_core::names::submap(submap).unwrap_or("?");
    egui::CollapsingHeader::new(format!("ExAnimation: {name}"))
        .id_salt("overworld-animation")
        .show(ui, |ui| {
            let Some(animation) = &overworld.animation else {
                let r = ui.button("Install Lunar Magic's overworld ExAnimation").on_hover_text(
                    "Each submap's animations of tiles and colours, and the overworld's global ones",
                );
                if r.clicked() {
                    change = Some(Change::Animation {
                        label: "Install the overworld's ExAnimation".into(),
                        to: Some(Box::default()),
                    });
                }
                return;
            };
            let n = usize::from(submap);
            let byte = animation.settings[n];
            ui.horizontal_wrapped(|ui| {
                for (flag, text, tip) in [
                    (0x40u8, "Game's tiles", "The game's own animated tiles (water, waterfalls)"),
                    (0x80, "Level dots' colours", "The level dots' flashing colours 6D and 7D"),
                    (0x20, "Submap's list", "This submap's ExAnimation, below"),
                    (0x10, "Global list", "The overworld's global ExAnimation"),
                ] {
                    let mut on = byte & flag == 0;
                    let r = ui.checkbox(&mut on, text).on_hover_text(tip);
                    if r.changed() {
                        let mut to = animation.clone();
                        to.settings[n] = if on { byte & !flag } else { byte | flag };
                        change = Some(Change::Animation {
                            label: format!("Turn {} {}", if on { "on" } else { "off" }, text.to_lowercase()),
                            to: Some(Box::new(to)),
                        });
                    }
                }
            });
            match &animation.submaps[n] {
                None => {
                    if ui.button(format!("Give {name} an ExAnimation list")).clicked() {
                        let mut to = animation.clone();
                        to.submaps[n] = Some(List::default());
                        change = Some(Change::Animation {
                            label: "Add an ExAnimation list".into(),
                            to: Some(Box::new(to)),
                        });
                    }
                }
                Some(list) => {
                    if let Some((_, label, new)) =
                        crate::animation::list_fields(ui, &format!("submap-{n}"), list, true)
                    {
                        let mut to = animation.clone();
                        to.submaps[n] = new;
                        change = Some(Change::Animation {
                            label,
                            to: Some(Box::new(to)),
                        });
                    }
                }
            }
            ui.separator();
            ui.label(RichText::new("The overworld's global list").strong());
            match &animation.global {
                None => {
                    if ui.button("Give the overworld a global list").clicked() {
                        let to = OverworldAnimation {
                            global: Some(List::default()),
                            ..animation.clone()
                        };
                        change = Some(Change::Animation {
                            label: "Add the overworld's global ExAnimation".into(),
                            to: Some(Box::new(to)),
                        });
                    }
                }
                Some(list) => {
                    if let Some((_, label, new)) =
                        crate::animation::list_fields(ui, "overworld-global", list, true)
                    {
                        let to = OverworldAnimation {
                            global: new,
                            ..animation.clone()
                        };
                        change = Some(Change::Animation {
                            label,
                            to: Some(Box::new(to)),
                        });
                    }
                }
            }
        });
    change
}
