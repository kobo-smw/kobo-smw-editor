//! The project window: what the project holds, what its build makes of
//! it, and where each tool the build runs comes from. It shows; the
//! manifest (`kobo.toml`) is where these are set.

use eframe::egui::{self, Grid, RichText};
use kobo_core::build::{self, Project};
use kobo_core::tools::{Origin, Tool, ToolError};

use crate::app::App;
use crate::theme;

/// A count of things, in words.
fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// Where a tool comes from, as the build would find it now, without
/// downloading anything.
fn tool_line(project: &Project, tool: Tool) -> (String, egui::Color32) {
    let located = match tool {
        Tool::Pixi => tool.locate_version_offline(project.manifest.pixi_version.as_deref()),
        _ => tool.locate_offline(),
    };
    match located {
        Ok(located) => match &located.origin {
            Origin::Pinned { release, .. } => (format!("Kobo's build, {release}"), theme::OK),
            Origin::Configured(setting) => (
                format!("{} ({setting})", located.path.display()),
                theme::WARNING,
            ),
        },
        Err(ToolError::NotCached { .. }) => (
            "Kobo's build, downloaded at the first build".to_string(),
            theme::MUTED,
        ),
        Err(e) => (e.to_string(), theme::ERROR),
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.project_open {
        return;
    }
    let Some(workspace) = app.workspace().cloned() else {
        app.project_open = false;
        return;
    };
    let project = workspace.project();
    let m = &project.manifest;
    let mut open = true;
    let mut reveal = None;
    egui::Window::new("Project")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(600.0)
        .default_height(640.0)
        .default_pos([290.0, 72.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(project.root.display().to_string()).monospace());
                    if ui.small_button("Show the folder").clicked() {
                        reveal = Some(project.root.clone());
                    }
                });
                ui.label(
                    RichText::new("Set in kobo.toml; the editor shows what it says.")
                        .small()
                        .color(theme::MUTED),
                );
                ui.add_space(6.0);
                Grid::new("project")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Clean ROM");
                        ui.label(format!(
                            "Super Mario World (USA), SHA-1 {}…",
                            &workspace.clean().sha1_hex()[..12]
                        ));
                        ui.end_row();

                        ui.label("Cartridge");
                        let mut cart = if m.sa1 {
                            "SA-1, through SA-1 Pack".to_string()
                        } else {
                            "LoROM, as the game's".to_string()
                        };
                        if let Some(size) = m.rom_size {
                            cart += &format!(", expanded to {} KiB", size / 1024);
                        }
                        ui.label(cart);
                        ui.end_row();

                        ui.label("Levels");
                        ui.label(format!(
                            "{} of 512; the rest build as the game has them",
                            m.levels.len()
                        ));
                        ui.end_row();

                        ui.label("Map16");
                        ui.label(format!(
                            "{}, {} of the game's changed, {} of tilesets, {} of backgrounds",
                            count(project.map16.len(), "page", "pages"),
                            count(project.map16_game.len(), "page", "pages"),
                            count(project.map16_tileset.len(), "file", "files"),
                            count(project.map16_bg.len(), "page", "pages"),
                        ));
                        ui.end_row();

                        ui.label("Graphics");
                        let mut graphics = format!(
                            "{} changed, {}",
                            count(project.gfx.len(), "GFX file", "GFX files"),
                            count(project.exgfx.len(), "ExGFX file", "ExGFX files"),
                        );
                        if m.four_bpp || project.lunar_magic_graphics() {
                            graphics += "; stored as Lunar Magic stores them";
                        }
                        if m.lz3 {
                            graphics += ", LC_LZ3";
                        }
                        ui.label(graphics);
                        ui.end_row();

                        ui.label("ExAnimation");
                        ui.label(if project.animation_global.is_some() {
                            "a global list, and the levels' own"
                        } else if project.exanimation() {
                            "the levels' own"
                        } else {
                            "none"
                        });
                        ui.end_row();

                        ui.label("Lunar Magic's layout");
                        ui.label(if project.lunar_magic_layout() {
                            "the build installs Kobo's code for it"
                        } else {
                            "not needed: the build keeps the game's"
                        });
                        ui.end_row();

                        for (label, list) in [
                            ("Patches before the tools", &m.early_patches),
                            ("Patches after the tools", &m.late_patches),
                        ] {
                            ui.label(label);
                            if list.is_empty() {
                                ui.label(RichText::new("none").color(theme::MUTED));
                            } else {
                                ui.vertical(|ui| {
                                    for patch in list {
                                        ui.label(
                                            RichText::new(patch.display().to_string()).monospace(),
                                        );
                                    }
                                });
                            }
                            ui.end_row();
                        }

                        for (label, folder) in [
                            ("PIXI's files", &m.pixi),
                            ("GPS's files", &m.gps),
                            ("UberASM Tool's files", &m.uberasm),
                            ("AddmusicK's files", &m.music),
                        ] {
                            ui.label(label);
                            match folder {
                                Some(folder) => ui
                                    .label(RichText::new(folder.display().to_string()).monospace()),
                                None => ui.label(RichText::new("none").color(theme::MUTED)),
                            };
                            ui.end_row();
                        }
                        if m.pixi_compiled.is_some() {
                            ui.label("PIXI");
                            ui.label("a hack's sprites, carried as compiled code");
                            ui.end_row();
                        }
                    });

                ui.add_space(10.0);
                ui.label(
                    RichText::new("THE TOOLS ITS BUILD RUNS")
                        .small()
                        .color(theme::MUTED),
                );
                let tools = build::tools(project);
                if tools.is_empty() {
                    ui.label(
                        RichText::new("None: the build writes everything itself.")
                            .color(theme::MUTED),
                    );
                }
                Grid::new("tools")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for tool in tools {
                            ui.label(tool.name());
                            let (text, color) = tool_line(project, tool);
                            ui.label(RichText::new(text).color(color));
                            ui.end_row();
                        }
                    });
                let warnings = build::warnings(project);
                if !warnings.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("WARNINGS").small().color(theme::MUTED));
                    for warning in warnings {
                        ui.label(RichText::new(warning).color(theme::WARNING));
                    }
                }
            });
        });
    app.project_open = open;
    if let Some(path) = reveal {
        crate::start::reveal(app, &path);
    }
}
