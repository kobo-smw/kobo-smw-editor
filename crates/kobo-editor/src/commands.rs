//! The command palette (Ctrl+K): every action by name, found by typing a
//! few letters of it. Opening a level by its number, and placing any
//! object or sprite by its name, go through it too. And the keyboard's
//! shortcuts, listed (F1).

use eframe::egui::{self, Key, RichText};
use kobo_core::level::objects::Object;
use kobo_core::names;

use crate::app::{App, LeftTab, SpriteView};
use crate::palette::Placing;
use crate::theme;

#[derive(Default)]
pub struct CommandState {
    pub open: bool,
    query: String,
    selected: usize,
    pub shortcuts: bool,
}

#[derive(Clone, Debug)]
enum Command {
    Save,
    Undo,
    Redo,
    Build,
    OpenLevel(u16),
    Tab(LeftTab),
    Source,
    Grid,
    Screens,
    Entrances,
    Sprites(SpriteView),
    Player,
    ZoomIn,
    ZoomOut,
    Fit,
    SelectAll,
    Deselect,
    Place(Placing),
    Shortcuts,
    CloseProject,
    Overview,
    /// Back to the level before, or with `true` forward again.
    Back(bool),
    /// Shows or hides a layer, by its screen designation bit.
    Layer(u8),
    Background,
}

/// Every command there is now: its name, its keys, and what it does.
fn commands(app: &App) -> Vec<(String, &'static str, Command)> {
    let mut list: Vec<(String, &'static str, Command)> = vec![
        ("Save every changed level".into(), "Ctrl+S", Command::Save),
        ("Undo".into(), "Ctrl+Z", Command::Undo),
        ("Redo".into(), "Ctrl+Shift+Z", Command::Redo),
        ("Build the project".into(), "Ctrl+B", Command::Build),
        (
            "Back to the level shown before".into(),
            "Alt+Left",
            Command::Back(false),
        ),
        (
            "Forward to the next level".into(),
            "Alt+Right",
            Command::Back(true),
        ),
        (
            "Show the level list".into(),
            "",
            Command::Tab(LeftTab::Levels),
        ),
        (
            "Show the palette (add objects and sprites)".into(),
            "",
            Command::Tab(LeftTab::Add),
        ),
        (
            "Show the outline".into(),
            "",
            Command::Tab(LeftTab::Outline),
        ),
        (
            "Show changes since the last commit".into(),
            "",
            Command::Tab(LeftTab::Changes),
        ),
        (
            "Find objects and sprites in every level".into(),
            "Ctrl+Shift+F",
            Command::Tab(LeftTab::Find),
        ),
        ("Show or hide the source pane".into(), "", Command::Source),
        ("Show or hide the grid".into(), "G", Command::Grid),
        (
            "Show or hide screen boundaries".into(),
            "",
            Command::Screens,
        ),
        (
            "Show or hide entrance markers".into(),
            "",
            Command::Entrances,
        ),
        (
            "Sprites: drawn".into(),
            "",
            Command::Sprites(SpriteView::Drawn),
        ),
        (
            "Sprites: as their numbers".into(),
            "",
            Command::Sprites(SpriteView::Markers),
        ),
        (
            "Sprites: hidden".into(),
            "",
            Command::Sprites(SpriteView::Hidden),
        ),
        ("Show or hide the player".into(), "", Command::Player),
        ("Show or hide layer 1".into(), "", Command::Layer(1)),
        ("Show or hide layer 2".into(), "", Command::Layer(2)),
        ("Show or hide layer 3".into(), "", Command::Layer(4)),
        ("Zoom in".into(), "Ctrl+wheel", Command::ZoomIn),
        ("Zoom out".into(), "Ctrl+wheel", Command::ZoomOut),
        ("Zoom to fit the level".into(), "", Command::Fit),
        ("Select everything".into(), "Ctrl+A", Command::SelectAll),
        ("Select nothing".into(), "Esc", Command::Deselect),
        ("Keyboard shortcuts".into(), "F1", Command::Shortcuts),
        ("All levels as pictures".into(), "", Command::Overview),
        ("Close the project".into(), "", Command::CloseProject),
    ];
    if let Some(workspace) = app.workspace() {
        for number in workspace.levels() {
            list.push((
                format!("Open level {number:03X}"),
                "",
                Command::OpenLevel(number),
            ));
        }
    }
    if let Some(open) = app.current() {
        if crate::backgrounds::applies(open.document.level()) {
            list.push((
                "Choose the level's background".into(),
                "",
                Command::Background,
            ));
        }
        let tileset = open.document.level().header.object_tileset;
        for n in (0x01..=0x3Fu8).filter(|n| !(0x22..=0x2D).contains(n)) {
            if let Some(name) = names::standard_object(n, tileset).filter(|n| *n != "Unused") {
                let object = Object::Standard {
                    number: n,
                    x: 0,
                    y: 0,
                    settings: 0,
                };
                list.push((
                    format!("Add object {n:02X}: {name}"),
                    "",
                    Command::Place(Placing::Object(object)),
                ));
            }
        }
        for n in 0x04..=0xFFu8 {
            let name = names::extended_object(n);
            if name != "Unused" {
                let object = Object::Extended {
                    number: n,
                    x: 0,
                    y: 0,
                };
                list.push((
                    format!("Add extended object {n:02X}: {name}"),
                    "",
                    Command::Place(Placing::Object(object)),
                ));
            }
        }
        for n in 0x00..=0xFFu8 {
            let name = names::sprite(n);
            if name != "Unused" {
                list.push((
                    format!("Add sprite {n:02X}: {name}"),
                    "",
                    Command::Place(Placing::Sprite(n)),
                ));
            }
        }
    }
    list
}

/// How well `query` matches `name`: every word of the query found in the
/// name, the earlier and the more at word starts the better; `None` if a
/// word is not there.
fn score(name: &str, query: &str) -> Option<i32> {
    let name = name.to_lowercase();
    let mut score = 0;
    for word in query.to_lowercase().split_whitespace() {
        let at = name.find(word)?;
        let starts_word = at == 0 || !name.as_bytes()[at - 1].is_ascii_alphanumeric();
        score += if starts_word { 100 } else { 40 } - (at as i32).min(60);
    }
    // Shorter names first among equals: "Coin" before "Coin, used".
    Some(score * 10 - name.len() as i32)
}

fn run(app: &mut App, command: Command) {
    let view = &mut app.view;
    match command {
        Command::Save => app.save_all_levels(),
        Command::Undo => app.undo_step(false),
        Command::Redo => app.undo_step(true),
        Command::Build => crate::build::start(app),
        Command::OpenLevel(number) => app.open_level(number),
        Command::Tab(tab) => app.left = tab,
        Command::Source => view.source = !view.source,
        Command::Grid => view.grid = !view.grid,
        Command::Screens => view.screens = !view.screens,
        Command::Entrances => view.entrances = !view.entrances,
        Command::Sprites(sprites) => {
            view.sprites = sprites;
            app.redraw();
        }
        Command::Layer(bit) => {
            view.hidden_layers ^= bit;
            app.redraw();
        }
        Command::Background => app.backgrounds.open = true,
        Command::Back(forward) => app.go_back(forward),
        Command::Player => {
            view.player = !view.player;
            app.redraw();
        }
        Command::ZoomIn | Command::ZoomOut | Command::Fit => {
            if let Some(camera) = app.current_mut().and_then(|o| o.camera.as_mut()) {
                match command {
                    Command::ZoomIn => camera.zoom_by(2.0),
                    Command::ZoomOut => camera.zoom_by(0.5),
                    _ => camera.fit_height = true,
                }
            }
        }
        Command::SelectAll => {
            if let Some(open) = app.current_mut() {
                let level = open.document.level();
                let mut items: Vec<_> = (0..level.layer1.len())
                    .map(|i| crate::selection::Item::object(kobo_core::edit::ObjectLayer::One, i))
                    .collect();
                items.extend((0..level.sprites.list.len()).map(crate::selection::Item::Sprite));
                open.selection = items;
            }
        }
        Command::Deselect => {
            if let Some(open) = app.current_mut() {
                open.selection.clear();
            }
        }
        Command::Place(placing) => {
            app.placing = Some(placing);
            app.left = LeftTab::Add;
        }
        Command::Shortcuts => app.commands.shortcuts = true,
        Command::CloseProject => app.switch_project(None),
        Command::Overview => app.overview.open = true,
    }
}

/// Ctrl+K and F1, whatever has the keyboard.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    let (palette, help) = ctx.input_mut(|i| {
        (
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                Key::K,
            )),
            i.consume_key(egui::Modifiers::NONE, Key::F1),
        )
    });
    if palette && app.workspace().is_some() {
        app.commands.open = !app.commands.open;
        app.commands.query.clear();
        app.commands.selected = 0;
    }
    if help {
        app.commands.shortcuts = !app.commands.shortcuts;
    }
}

/// The palette's window, when open.
pub fn window(app: &mut App, ctx: &egui::Context) {
    shortcuts(app, ctx);
    if !app.commands.open {
        return;
    }
    let all = commands(app);
    let query = app.commands.query.trim().to_string();
    let mut found: Vec<(i32, usize)> = all
        .iter()
        .enumerate()
        .filter(|(_, (name, ..))| {
            // Without a query, the general commands, not every object.
            !query.is_empty() || !(name.starts_with("Add ") || name.starts_with("Open level"))
        })
        .map(|(i, (name, ..))| (score(name, &query).unwrap_or(i32::MIN), i))
        .filter(|(s, _)| query.is_empty() || *s > i32::MIN)
        .collect();
    if !query.is_empty() {
        found.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
    }
    found.truncate(14);
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    let state = &mut app.commands;
    if down {
        state.selected = (state.selected + 1).min(found.len().saturating_sub(1));
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    state.selected = state.selected.min(found.len().saturating_sub(1));
    let mut chosen = enter
        .then(|| found.get(state.selected).map(|&(_, i)| i))
        .flatten();
    egui::Modal::new(egui::Id::new("command-palette")).show(ctx, |ui| {
        ui.set_width(520.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Type a command, a level number, or something to add")
                .desired_width(f32::INFINITY),
        );
        field.request_focus();
        if field.changed() {
            state.selected = 0;
        }
        ui.add_space(4.0);
        for (row, &(_, i)) in found.iter().enumerate() {
            let (name, keys, _) = &all[i];
            ui.horizontal(|ui| {
                let response = ui.selectable_label(row == state.selected, name);
                if response.clicked() {
                    chosen = Some(i);
                }
                if !keys.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(*keys).monospace().small().color(theme::MUTED));
                    });
                }
            });
        }
        if found.is_empty() {
            ui.label(RichText::new("Nothing by that name").color(theme::MUTED));
        }
    });
    if escape {
        app.commands.open = false;
    }
    if let Some(i) = chosen {
        app.commands.open = false;
        let command = all[i].2.clone();
        run(app, command);
    }
}

/// The keyboard's shortcuts, in a window of their own.
fn shortcuts(app: &mut App, ctx: &egui::Context) {
    let mut open = app.commands.shortcuts;
    egui::Window::new("Keyboard shortcuts")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("shortcuts")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for (keys, what) in [
                        ("Ctrl+K", "Find any command, level, or thing to add"),
                        ("Ctrl+S", "Save every changed level"),
                        ("Ctrl+Z, Ctrl+Shift+Z", "Undo, redo"),
                        ("Ctrl+B", "Build"),
                        ("Ctrl+Shift+F", "Find in every level"),
                        ("Click, Shift+click", "Select, add to the selection"),
                        ("Drag", "Move the selection, or select what a box meets"),
                        ("Arrow keys (Shift)", "Move the selection a tile (a screen)"),
                        ("Delete", "Delete the selection"),
                        (
                            "Ctrl+C, Ctrl+X, Ctrl+V",
                            "Copy, cut, paste where the mouse is",
                        ),
                        ("Ctrl+D", "Duplicate"),
                        ("Ctrl+A, Esc", "Select everything, nothing"),
                        (
                            "Ctrl+[, Ctrl+]",
                            "Send backward, bring forward (Shift: to the back, front)",
                        ),
                        ("Wheel, middle drag, Space+drag", "Scroll and pan"),
                        ("Ctrl+wheel", "Zoom"),
                        ("Right click", "The menu for what is under the mouse"),
                        ("Double-click an exit", "Go to the level it leads to"),
                        (
                            "Alt+Left, Alt+Right",
                            "Back to the level before, forward again",
                        ),
                        ("F1", "These"),
                    ] {
                        ui.label(RichText::new(keys).monospace().color(theme::ACCENT));
                        ui.label(what);
                        ui.end_row();
                    }
                });
        });
    app.commands.shortcuts = open;
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn words_match_anywhere_and_word_starts_win() {
        assert!(score("Add object 05: Coin", "coin").is_some());
        assert!(score("Add object 05: Coin", "add coin").is_some());
        assert!(score("Add object 05: Coin", "goomba").is_none());
        let coin = score("Add object 05: Coin", "coin").unwrap();
        let used = score("Add object 2B: Coin, used", "coin").unwrap();
        assert!(coin > used, "the shorter name first");
        let start = score("Open level 105", "level").unwrap();
        let middle = score("Show or hide the grid", "rid").unwrap();
        assert!(start > middle);
    }
}
