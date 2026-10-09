//! The Kobo level editor: a window over `kobo_core`, which does
//! everything it shows and every change it makes (docs/editor.md).

mod animation;
mod app;
mod backgrounds;
mod build;
mod canvas;
mod changes;
mod clipboard;
mod commands;
mod dialogs;
mod find;
mod graphics;
mod inspector;
mod layer3;
mod levels;
mod map16;
mod outline;
mod overview;
mod overworld;
mod overworld_lists;
mod palette;
mod palettes;
mod picture;
mod play;
mod preview;
mod project;
mod ram_watch;
mod screens;
mod selection;
mod source;
mod start;
mod theme;
mod thumbnails;
mod watch;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use clap::Parser;
use eframe::egui;

#[derive(Parser)]
#[command(name = "kobo-editor", version, about = "The Kobo level editor")]
struct Args {
    /// The project's folder.
    project: Option<PathBuf>,
    /// The level to open first, in hex.
    #[arg(long, value_parser = parse_hex)]
    level: Option<u16>,
    /// Layer 1 objects to select once the level is open, by index.
    #[arg(long, value_delimiter = ',')]
    select: Vec<usize>,
    /// Show the level file beside the canvas.
    #[arg(long)]
    source: bool,
    /// Show the palette of things to add rather than the level list.
    #[arg(long)]
    palette: bool,
    /// The left panel's tab (levels, objects, add, sprites, map16), or a window
    /// (changes, overview, backgrounds, project, map16-editor, graphics, palettes, layer3,
    /// overworld, screens).
    #[arg(long)]
    tab: Option<String>,
    /// Build the project once the level is open.
    #[arg(long)]
    build: bool,
    /// Save a picture of the window once the level is drawn, and quit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// The window's size, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1600x940")]
    size: String,
}

/// The window's icon: Kobo's amber square with a dark K, drawn here
/// rather than kept as a file.
fn icon() -> egui::IconData {
    const SIZE: usize = 64;
    // The K, on a 16 by 16 grid of 4-pixel cells.
    const K: [&str; 16] = [
        "................",
        "................",
        "...##.....##....",
        "...##....##.....",
        "...##...##......",
        "...##..##.......",
        "...##.##........",
        "...####.........",
        "...#####........",
        "...##.###.......",
        "...##..###......",
        "...##...###.....",
        "...##....###....",
        "...##.....###...",
        "................",
        "................",
    ];
    let amber = [0xF2, 0xB5, 0x44, 0xFF];
    let dark = [0x16, 0x18, 0x1D, 0xFF];
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Rounded corners, 10 pixels in.
            let corner = |c: usize| {
                if c < 10 {
                    10 - c
                } else if c >= SIZE - 10 {
                    c + 11 - SIZE
                } else {
                    0
                }
            };
            let (cx, cy) = (corner(x), corner(y));
            let outside = cx * cx + cy * cy > 100;
            let ink = K[y / 4].as_bytes()[x / 4] == b'#';
            rgba.extend(if outside {
                [0, 0, 0, 0]
            } else if ink {
                dark
            } else {
                amber
            });
        }
    }
    egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}

/// Whether the window draws without vsync and paces its own frames: on
/// Linux, where a compositor shows every frame whole anyway.
const PACE: bool = cfg!(target_os = "linux");

fn parse_hex(text: &str) -> Result<u16, String> {
    u16::from_str_radix(text.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let (width, height) = args
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((1600.0, 940.0));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kobo")
            .with_icon(icon())
            .with_inner_size([width, height])
            .with_min_inner_size([800.0, 500.0]),
        // With vsync, a frame drawn while the window is covered (by the
        // emulator Play opens) waits on Wayland until it is shown again,
        // as the compositor sends no frame callbacks to a hidden window,
        // and the desktop calls the editor not responding meanwhile. The
        // editor paces its frames itself instead (`Startup::pace`).
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: !PACE,
            ..Default::default()
        },
        ..Default::default()
    };
    let startup = app::Startup {
        project: args.project,
        level: args.level,
        select: args.select,
        source: args.source,
        palette: args.palette,
        tab: args.tab,
        build: args.build,
        screenshot: args.screenshot,
        pace: PACE,
    };
    eframe::run_native(
        "Kobo",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}
