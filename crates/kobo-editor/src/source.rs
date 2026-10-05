//! The source pane: the level file as saving would write it, with the
//! selected entry's line marked. Typing in it edits the level as soon as
//! the text reads as one.

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align, Color32, FontId, RichText};
use kobo_core::edit::{self, SPRITE_LIST};

use crate::app::App;
use crate::selection::Item;
use crate::theme;

#[derive(Default)]
pub struct SourceState {
    /// The text in the pane, which can differ from the document's while
    /// it is being typed.
    buffer: String,
    /// The document's text the buffer last took.
    synced: String,
    /// The buffer does not read as a level.
    error: Option<String>,
    /// An "Edit source" undo step is open, which more typing amends.
    typing: bool,
    /// The selection the pane last scrolled to.
    scrolled_to: Option<Item>,
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(number) = app.current_number() else {
        return;
    };
    let mut redraw = false;
    {
        let Some(open) = app.open_mut(number) else {
            return;
        };
        let name = open.file_name();
        let state = &mut open.source;
        let text = open.document.text();
        let id = egui::Id::new(("source", number));
        let focused = ui.ctx().memory(|m| m.has_focus(id));
        if text != state.synced && !focused {
            state.buffer = text.to_string();
            state.synced = text.to_string();
            state.error = None;
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).strong());
            ui.label(
                RichText::new("edits apply as you type; outside edits show here")
                    .small()
                    .color(theme::MUTED),
            );
        });
        if let Some(error) = &state.error {
            ui.colored_label(theme::ERROR, error);
        }

        let selected = match open.selection[..] {
            [Item::Object(o)] => {
                edit::entry_line(&state.buffer, edit::object_list(o.layer), o.index)
            }
            [Item::Sprite(i)] => edit::entry_line(&state.buffer, SPRITE_LIST, i),
            _ => None,
        };
        let scroll = open.selection.first().copied() != state.scrolled_to;
        state.scrolled_to = open.selection.first().copied();

        // One entry to a line, as the file has them: the pane scrolls
        // sideways rather than wrap.
        let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job = highlight(buffer.as_str(), selected);
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let output = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let output = egui::TextEdit::multiline(&mut state.buffer)
                    .id(id)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .frame(egui::Frame::NONE)
                    .layouter(&mut layouter)
                    .show(ui);
                if scroll
                    && let Some(line) = selected
                    && let Some(row) = output.galley.rows.get(line)
                {
                    let rect = row.rect().translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
                output
            })
            .inner;

        let response = &output.response.response;
        if response.changed() {
            let applied = if state.typing {
                open.document.amend_text(&state.buffer)
            } else {
                open.document.set_text("Edit source", &state.buffer)
            };
            match applied {
                Ok(()) => {
                    state.typing = true;
                    state.error = None;
                    state.synced = open.document.text().to_string();
                    redraw = true;
                }
                Err(e) => state.error = Some(e.to_string()),
            }
        }
        if response.lost_focus() {
            state.typing = false;
            if state.error.is_none() {
                state.buffer = open.document.text().to_string();
                state.synced = state.buffer.clone();
            }
        }
    }
    if redraw {
        app.request_preview(number);
    }
}

/// Kobo's level format coloured: table headers, numbers in hex, the
/// comments, and the line of the selected entry.
fn highlight(text: &str, selected: Option<usize>) -> LayoutJob {
    let font = FontId::monospace(12.5);
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(font.clone(), theme::TEXT);
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let background = if Some(n) == selected {
            theme::SELECTION.gamma_multiply(0.22)
        } else {
            Color32::TRANSPARENT
        };
        let format = |color: Color32| TextFormat {
            background,
            ..TextFormat::simple(font.clone(), color)
        };
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            job.append(line, 0.0, format(theme::HEADER));
            continue;
        }
        let (code, comment) = match find_comment(line) {
            Some(at) => line.split_at(at),
            None => (line, ""),
        };
        let mut rest = code;
        while !rest.is_empty() {
            match rest.find("0x") {
                Some(at) => {
                    job.append(&rest[..at], 0.0, format(plain.color));
                    let len = 2 + rest[at + 2..]
                        .chars()
                        .take_while(char::is_ascii_hexdigit)
                        .count();
                    job.append(&rest[at..at + len], 0.0, format(theme::ACCENT));
                    rest = &rest[at + len..];
                }
                None => {
                    job.append(rest, 0.0, format(plain.color));
                    rest = "";
                }
            }
        }
        if !comment.is_empty() {
            job.append(comment, 0.0, format(theme::MUTED));
        }
    }
    job
}

/// Where a line's comment starts, outside a string.
fn find_comment(line: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}
