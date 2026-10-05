//! The editor's colours: dark panels around the level, amber for Kobo,
//! cyan for the selection, magenta for sprites.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x16, 0x18, 0x1d);
pub const PANEL: Color32 = Color32::from_rgb(0x1e, 0x21, 0x28);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(0x25, 0x29, 0x31);
pub const CANVAS: Color32 = Color32::from_rgb(0x0d, 0x0e, 0x11);
pub const LINE: Color32 = Color32::from_rgb(0x32, 0x37, 0x43);
pub const TEXT: Color32 = Color32::from_rgb(0xd9, 0xdc, 0xe3);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x91, 0xa0);
pub const ACCENT: Color32 = Color32::from_rgb(0xf2, 0xb5, 0x44);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x5a, 0x45, 0x1c);
pub const SELECTION: Color32 = Color32::from_rgb(0x43, 0xc6, 0xff);
pub const SPRITE: Color32 = Color32::from_rgb(0xff, 0x7d, 0xe9);
pub const ON_SELECTION: Color32 = Color32::from_rgb(0x00, 0x20, 0x2e);
pub const HEADER: Color32 = Color32::from_rgb(0x7d, 0xd3, 0xfc);
pub const WARNING: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x3c);
pub const ERROR: Color32 = Color32::from_rgb(0xef, 0x6b, 0x6b);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = PANEL_RAISED;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTION.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, SELECTION);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(5);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(5);
    visuals.widgets.active.corner_radius = CornerRadius::same(5);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
}

/// The top and bottom bars.
pub fn bar_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .inner_margin(Margin::symmetric(10, 6))
        .stroke(Stroke::new(1.0, LINE))
}

/// The side panels.
pub fn side_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .inner_margin(Margin::symmetric(10, 6))
        .stroke(Stroke::new(1.0, LINE))
}
