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
    LAYER1_SIZE, LAYER2_SIZE, Overworld, Start, layer1_index, layer2_index,
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
                            kobo_core::render::render_overworld(&rom, key.submap)
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
    /// Where a new game puts Mario (0) or Luigi (1).
    Start { player: usize, start: Start },
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
            Change::Start { player, start } => {
                app.change_overworld("Move the start", false, |ow| ow.start[player] = start)
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
        let primary = ui.input(|i| i.pointer.primary_down());
        if primary && (response.is_pointer_button_down_on() || response.dragged()) {
            if let Some((x, y)) = response
                .interact_pointer_pos()
                .and_then(|p| at(p, cell, cells))
            {
                let index = index_of(state.layer, map, x, y);
                if state.stroke != Some(index) {
                    change = Some(Change::Draw {
                        layer: state.layer,
                        index,
                        tile: if state.layer == 1 {
                            state.brush
                        } else {
                            state.brush2
                        },
                        amend: state.stroke.is_some(),
                    });
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
    change
}
