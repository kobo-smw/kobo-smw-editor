//! The level list: each overworld level with the sublevels its exits
//! lead to (`edit::reach`), the levels nothing reaches by an exit, and
//! the game's own levels the project does not have. The game's unused
//! level numbers, which all hold its "TEST" level, are left out unless
//! asked for. One search finds levels by number, name, or tileset, and
//! what every level holds (`edit::find`).

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Align2, FontId, Rect, RichText, Sense, Vec2};
use kobo_core::edit::find::{Entry, Found};
use kobo_core::edit::reach::{Placeholder, Reach};

use crate::app::App;
use crate::selection::Item;
use crate::theme;

/// A row's height, and its picture's size.
const ROW: f32 = 30.0;
const THUMB: Vec2 = egui::vec2(46.0, 26.0);
/// How far a sublevel is set in.
const INDENT: f32 = 16.0;
/// The most entries found that are listed.
const MOST_FOUND: usize = 2000;

#[derive(Default)]
pub struct LevelsState {
    pub filter: String,
    /// Put the cursor in the search field on the next frame.
    pub focus_search: bool,
    /// Overworld levels whose sublevels are shown.
    expanded: BTreeSet<u16>,
    /// The level whose group was last opened for it.
    followed: Option<u16>,
    /// Scroll the list to the level shown, on the next frame.
    scroll_to_current: bool,
    /// Show the game's levels the project does not have.
    others: bool,
    /// Show the levels that hold the game's placeholder.
    unused: bool,
    /// What the list is made from, until the project changes.
    known: Option<Known>,
    /// The game's placeholder level, read once.
    placeholder: Option<Option<Placeholder>>,
}

impl LevelsState {
    /// The project changed: its levels are looked at again.
    pub fn changed(&mut self) {
        self.known = None;
    }
}

/// What the list shows of a level.
struct Info {
    listed: bool,
    unused: bool,
    screens: u8,
    vertical: bool,
    tileset: String,
}

/// The levels, and how they are reached.
struct Known {
    info: BTreeMap<u16, Info>,
    /// The project's levels, but the unused ones.
    reach: Reach,
    /// The game's own levels, but the unused ones.
    others: Vec<u16>,
    unused: Vec<u16>,
}

fn know(app: &mut App) -> Option<Known> {
    if app.levels.placeholder.is_none() {
        let found = app.workspace().and_then(|w| Placeholder::of(w.clean()));
        app.levels.placeholder = Some(found);
    }
    let placeholder = app.levels.placeholder.clone().flatten();
    let workspace = app.workspace()?;
    let mut info = BTreeMap::new();
    for number in 0..0x200u16 {
        let listed = workspace.level(number).is_some();
        let level = match workspace.level(number) {
            Some(level) => level.clone(),
            None => match workspace.clean_level(number) {
                Ok(level) => level,
                Err(_) => continue,
            },
        };
        let unused = placeholder.as_ref().is_some_and(|p| p.is(&level));
        info.insert(
            number,
            Info {
                listed,
                unused,
                screens: level.header.screens,
                vertical: level.header.level_mode.layer1_vertical(),
                tileset: kobo_core::names::object_tileset(level.header.object_tileset)
                    .unwrap_or("?")
                    .to_string(),
            },
        );
    }
    let listed: Vec<u16> = info
        .iter()
        .filter(|(_, i)| i.listed && !i.unused)
        .map(|(&n, _)| n)
        .collect();
    let reach = Reach::of(workspace, &listed);
    let others = info
        .iter()
        .filter(|(_, i)| !i.listed && !i.unused)
        .map(|(&n, _)| n)
        .collect();
    let unused = info
        .iter()
        .filter(|(_, i)| i.unused)
        .map(|(&n, _)| n)
        .collect();
    Some(Known {
        info,
        reach,
        others,
        unused,
    })
}

/// A row of the list.
enum Row {
    Heading(String),
    /// A heading that shows or hides what follows it.
    Toggle(String, Toggle, bool),
    Level {
        number: u16,
        sublevel: bool,
        /// Whether its sublevels are shown, when it has any.
        expanded: Option<bool>,
    },
    Found(usize),
}

#[derive(Clone, Copy)]
enum Toggle {
    Others,
    Unused,
}

/// What the list's menu asks for of a level.
enum ListAction {
    Play(u8),
    Remove,
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    if app.levels.known.is_none() {
        app.levels.known = know(app);
    }
    // The group of the level shown opens, once, when the level changes.
    let current = app.current_number();
    if current != app.levels.followed {
        app.levels.followed = current;
        if let Some(known) = &app.levels.known
            && let Some(group) = current.and_then(|n| known.reach.group_of(n))
        {
            app.levels.expanded.insert(group);
        }
        app.levels.scroll_to_current = true;
    }
    let field = ui.add(
        egui::TextEdit::singleline(&mut app.levels.filter)
            .hint_text("Find a level, or what levels hold: 105, castle, goomba…")
            .desired_width(f32::INFINITY),
    );
    if std::mem::take(&mut app.levels.focus_search) {
        field.request_focus();
    }
    ui.add_space(4.0);
    let filter = app.levels.filter.trim().to_lowercase();
    let found: Vec<Found> = if filter.is_empty() {
        Vec::new()
    } else {
        crate::find::found(app, &filter).to_vec()
    };
    let Some(known) = app.levels.known.take() else {
        return;
    };
    let rows = rows(app, &known, &filter, &found);
    let mut clicked = None;
    let mut menu = None;
    let mut toggled = None;
    let mut expand = None;
    let mut chosen = None;
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("level-list")
        .auto_shrink([false, false]);
    if std::mem::take(&mut app.levels.scroll_to_current)
        && let Some(row) = rows
            .iter()
            .position(|r| matches!(r, Row::Level { number, .. } if Some(*number) == current))
    {
        // A third of the way down, so what comes before shows.
        let spacing = ui.spacing().item_spacing.y;
        let offset = row as f32 * (ROW + spacing) - ui.available_height() / 3.0;
        scroll = scroll.vertical_scroll_offset(offset.max(0.0));
    }
    scroll.show_rows(ui, ROW, rows.len(), |ui, range| {
        for row in &rows[range] {
            match row {
                Row::Heading(text) => {
                    heading(ui, text, None);
                }
                Row::Toggle(text, toggle, open) => {
                    if heading(ui, text, Some(*open)).clicked() {
                        toggled = Some(*toggle);
                    }
                }
                Row::Level {
                    number,
                    sublevel,
                    expanded,
                } => {
                    let out = level_row(app, ui, &known, *number, *sublevel, *expanded);
                    if out.clicked {
                        clicked = Some(*number);
                    }
                    if out.expander {
                        expand = Some(*number);
                    }
                    if out.menu.is_some() {
                        menu = out.menu.map(|a| (*number, a));
                    }
                }
                Row::Found(i) => {
                    if found_row(ui, &found[*i]) {
                        chosen = Some((found[*i].level, found[*i].entry));
                    }
                }
            }
        }
    });
    if found.len() > MOST_FOUND {
        ui.label(
            RichText::new(format!("The first {MOST_FOUND} found shown"))
                .small()
                .color(theme::MUTED),
        );
    }
    app.levels.known = Some(known);
    if app.thumbnails.waiting() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    match toggled {
        Some(Toggle::Others) => app.levels.others ^= true,
        Some(Toggle::Unused) => app.levels.unused ^= true,
        None => {}
    }
    if let Some(number) = expand
        && !app.levels.expanded.remove(&number)
    {
        app.levels.expanded.insert(number);
    }
    if let Some(number) = clicked {
        if app.has_level(number) {
            app.open_level(number);
        } else {
            app.adding = Some(number);
        }
    }
    match menu {
        Some((number, ListAction::Play(powerup))) => {
            crate::play::from_level_start(app, number, powerup);
        }
        Some((number, ListAction::Remove)) => app.removing = Some(number),
        None => {}
    }
    if let Some((level, entry)) = chosen {
        app.open_level(level);
        if let Some(open) = app.current_mut()
            && open.number == level
        {
            open.selection = vec![match entry {
                Entry::Object(layer, index) => Item::object(layer, index),
                Entry::Sprite(index) => Item::Sprite(index),
            }];
            open.focus = true;
        }
    }
}

/// The rows: with no search, the levels as they are reached; with one,
/// the levels it names and what it finds in them.
fn rows(app: &App, known: &Known, filter: &str, found: &[Found]) -> Vec<Row> {
    let state = &app.levels;
    let mut rows = Vec::new();
    let level = |number, sublevel| Row::Level {
        number,
        sublevel,
        expanded: None,
    };
    if !filter.is_empty() {
        let matches = |n: &u16| {
            let info = &known.info[n];
            format!("{n:03x}").contains(filter)
                || info.tileset.to_lowercase().contains(filter)
                || app
                    .level_name(*n)
                    .is_some_and(|name| name.to_lowercase().contains(filter))
        };
        let levels: Vec<u16> = known
            .info
            .iter()
            .filter(|(_, i)| (i.listed || state.others) && (!i.unused || state.unused))
            .map(|(&n, _)| n)
            .filter(matches)
            .collect();
        if !levels.is_empty() {
            rows.push(Row::Heading(format!("Levels · {}", levels.len())));
            rows.extend(levels.into_iter().map(|n| level(n, false)));
        }
        let mut last = None;
        for (i, f) in found.iter().take(MOST_FOUND).enumerate() {
            if last != Some(f.level) {
                last = Some(f.level);
                let count = found.iter().filter(|g| g.level == f.level).count();
                let name = app.level_name(f.level).unwrap_or_default();
                rows.push(Row::Heading(
                    format!("In {:03X} {name} · {count}", f.level)
                        .trim()
                        .to_string(),
                ));
            }
            rows.push(Row::Found(i));
        }
        if rows.is_empty() {
            rows.push(Row::Heading("Nothing found".into()));
        }
        return rows;
    }
    for group in &known.reach.groups {
        let open = state.expanded.contains(&group.level);
        rows.push(Row::Level {
            number: group.level,
            sublevel: false,
            expanded: (!group.sublevels.is_empty()).then_some(open),
        });
        if open {
            rows.extend(group.sublevels.iter().map(|&n| level(n, true)));
        }
    }
    if !known.reach.unreached.is_empty() {
        rows.push(Row::Heading("Not reached by an exit".into()));
        rows.extend(known.reach.unreached.iter().map(|&n| level(n, false)));
    }
    if !known.others.is_empty() {
        rows.push(Row::Toggle(
            format!(
                "The game's own, not in the project · {}",
                known.others.len()
            ),
            Toggle::Others,
            state.others,
        ));
        if state.others {
            rows.extend(known.others.iter().map(|&n| level(n, false)));
        }
    }
    if !known.unused.is_empty() {
        rows.push(Row::Toggle(
            format!("Unused: the game's TEST level · {}", known.unused.len()),
            Toggle::Unused,
            state.unused,
        ));
        if state.unused {
            rows.extend(known.unused.iter().map(|&n| level(n, false)));
        }
    }
    rows
}

/// A heading row; with `open`, one that shows or hides what follows.
fn heading(ui: &mut egui::Ui, text: &str, open: Option<bool>) -> egui::Response {
    let sense = if open.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), sense);
    let color = if response.hovered() && open.is_some() {
        theme::TEXT
    } else {
        theme::MUTED
    };
    let mut x = rect.left() + 4.0;
    if let Some(open) = open {
        ui.painter().text(
            egui::pos2(x, rect.bottom() - 7.0),
            Align2::LEFT_BOTTOM,
            if open { "⏷" } else { "⏵" },
            FontId::proportional(10.5),
            color,
        );
        x += 14.0;
    }
    ui.painter().text(
        egui::pos2(x, rect.bottom() - 7.0),
        Align2::LEFT_BOTTOM,
        text.to_uppercase(),
        FontId::proportional(10.5),
        color,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, true, text));
    response
}

struct RowOut {
    clicked: bool,
    expander: bool,
    menu: Option<ListAction>,
}

fn level_row(
    app: &mut App,
    ui: &mut egui::Ui,
    known: &Known,
    number: u16,
    sublevel: bool,
    expanded: Option<bool>,
) -> RowOut {
    let info = &known.info[&number];
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::click());
    let selected = app.current_number() == Some(number);
    let modified = app.is_modified(number);
    let name = app.level_name(number).map(str::to_string);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 4, theme::SELECTION.gamma_multiply(0.25));
    } else if response.hovered() {
        painter.rect_filled(rect, 4, theme::PANEL_RAISED);
    }
    let mut x = rect.left() + 2.0 + if sublevel { INDENT } else { 0.0 };
    let middle = rect.center().y;
    // The expander, for an overworld level with sublevels.
    let expander = Rect::from_center_size(egui::pos2(x + 6.0, middle), egui::vec2(14.0, ROW));
    let mut on_expander = false;
    if let Some(open) = expanded {
        on_expander = response
            .interact_pointer_pos()
            .is_some_and(|p| expander.contains(p));
        let hovered = response.hover_pos().is_some_and(|p| expander.contains(p));
        painter.text(
            expander.center(),
            Align2::CENTER_CENTER,
            if open { "⏷" } else { "⏵" },
            FontId::proportional(11.0),
            if hovered { theme::TEXT } else { theme::MUTED },
        );
    }
    if !sublevel {
        x += 14.0;
    }
    // The level's picture, once drawn.
    let thumb = Rect::from_min_size(egui::pos2(x, middle - THUMB.y / 2.0), THUMB);
    if ui.is_rect_visible(rect) {
        app.thumbnails.want(number);
    }
    match app.thumbnails.picture(number) {
        Some(texture) => crate::thumbnails::paint_cover(&painter, texture, thumb),
        None => {
            painter.rect_filled(thumb, 2, theme::BACKGROUND);
        }
    }
    painter.rect_stroke(
        thumb,
        2,
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    x = thumb.right() + 8.0;
    // Its number, name, and size.
    let number_text = painter.text(
        egui::pos2(x, middle),
        Align2::LEFT_CENTER,
        format!("{number:03X}"),
        FontId::monospace(12.5),
        if info.listed {
            theme::TEXT
        } else {
            theme::MUTED
        },
    );
    x = number_text.right() + 8.0;
    let size = format!("{} {}", info.screens, if info.vertical { "↕" } else { "↔" });
    let size_rect = painter.text(
        egui::pos2(rect.right() - 6.0, middle),
        Align2::RIGHT_CENTER,
        &size,
        FontId::proportional(11.5),
        theme::MUTED,
    );
    let mut right = size_rect.left() - 6.0;
    if modified {
        let dot = painter.text(
            egui::pos2(right, middle),
            Align2::RIGHT_CENTER,
            "●",
            FontId::proportional(11.0),
            theme::ACCENT,
        );
        right = dot.left() - 4.0;
    }
    let (text, color) = match &name {
        Some(name) => (name.as_str(), theme::TEXT),
        None => (info.tileset.as_str(), theme::MUTED),
    };
    let galley = painter.layout(
        text.to_string(),
        FontId::proportional(12.5),
        color,
        f32::INFINITY,
    );
    let room = (right - x).max(0.0);
    let clip = Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(x + room, rect.bottom()),
    );
    painter.with_clip_rect(clip).galley(
        egui::pos2(x, middle - galley.size().y / 2.0),
        galley,
        color,
    );

    let label = match &name {
        Some(name) => format!("{number:03X} {name}"),
        None => format!("{number:03X} {}", info.tileset),
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
    });
    if response.hovered() {
        app.thumbnails.want_first(number);
    }
    let response = response.on_hover_ui(|ui| tooltip(app, ui, known, number));
    let listed = info.listed;
    let mut menu = None;
    let mut open = false;
    response.context_menu(|ui| {
        if ui
            .button(if listed {
                "Open"
            } else {
                "Add to the project…"
            })
            .clicked()
        {
            open = true;
            ui.close();
        }
        if listed {
            ui.menu_button("Play from its start", |ui| {
                for (i, name) in crate::play::POWERUPS.iter().enumerate() {
                    if ui.button(*name).clicked() {
                        menu = Some(ListAction::Play(i as u8));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Take out of the project…").clicked() {
                menu = Some(ListAction::Remove);
                ui.close();
            }
        }
    });
    RowOut {
        clicked: (response.clicked() && !on_expander) || open,
        expander: response.clicked() && on_expander,
        menu,
    }
}

/// A level's picture and what it is, on hovering its row.
fn tooltip(app: &App, ui: &mut egui::Ui, known: &Known, number: u16) {
    let info = &known.info[&number];
    let width = 256.0;
    match (
        app.thumbnails.picture(number),
        app.thumbnails.failed(number),
    ) {
        (Some(texture), _) => {
            let size = texture.size_vec2();
            let shown = size * (width / size.x);
            ui.image((texture.id(), shown));
        }
        (None, Some(why)) => {
            ui.label(RichText::new(format!("Does not draw: {why}")).color(theme::ERROR));
        }
        (None, None) => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 144.0), Sense::hover());
            ui.painter().rect_filled(rect, 4, theme::BACKGROUND);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Drawing…",
                FontId::proportional(12.0),
                theme::MUTED,
            );
        }
    }
    let name = app.level_name(number).unwrap_or_default();
    ui.label(RichText::new(format!("{number:03X} {name}").trim().to_string()).strong());
    let size = match (info.vertical, info.screens) {
        (true, n) => format!("vertical, {n} screens tall"),
        (false, 1) => "1 screen wide".to_string(),
        (false, n) => format!("{n} screens wide"),
    };
    ui.label(RichText::new(format!("{}, {size}", info.tileset)).color(theme::MUTED));
    if let Some(group) = known.reach.group_of(number).filter(|&g| g != number) {
        let name = app.level_name(group).unwrap_or_default();
        ui.label(
            RichText::new(
                format!("A sublevel of {group:03X} {name}")
                    .trim()
                    .to_string(),
            )
            .color(theme::MUTED),
        );
    }
    if !info.listed {
        ui.label(
            RichText::new("Not in the project: it builds as the game has it. Choose it to add it.")
                .color(theme::MUTED),
        );
    }
}

/// An entry found in a level; whether it was chosen.
fn found_row(ui: &mut egui::Ui, f: &Found) -> bool {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{} ", f.kind.rsplit(' ').next().unwrap_or_default()),
        0.0,
        egui::TextFormat::simple(FontId::monospace(11.5), theme::MUTED),
    );
    job.append(
        &f.name,
        0.0,
        egui::TextFormat::simple(FontId::proportional(13.0), theme::TEXT),
    );
    let at = f.at.map_or_else(String::new, |(x, y)| format!("{x}, {y}"));
    ui.add(
        egui::Button::selectable(false, job)
            .right_text(RichText::new(at).size(12.0).color(theme::MUTED))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .min_size(egui::vec2(ui.available_width(), ROW)),
    )
    .clicked()
}
