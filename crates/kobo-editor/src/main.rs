//! The Kobo level editor: a window over `kobo_core`, which does
//! everything it shows and every change it makes (docs/step-3.md).

mod app;
mod canvas;
mod clipboard;
mod inspector;
mod outline;
mod palette;
mod picture;
mod preview;
mod selection;
mod source;
mod start;
mod theme;
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
    /// Save a picture of the window once the level is drawn, and quit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// The window's size, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1600x940")]
    size: String,
}

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
            .with_inner_size([width, height])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    let startup = app::Startup {
        project: args.project,
        level: args.level,
        select: args.select,
        source: args.source,
        palette: args.palette,
        screenshot: args.screenshot,
    };
    eframe::run_native(
        "Kobo",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}
