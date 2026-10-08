//! The editor driven as a user would, without a display: open a level,
//! click and drag on the canvas, undo, save, and follow an outside edit.
//! They need the vanilla ROM, and skip as the library's ROM tests do
//! when it is not configured.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use kobo_core::edit::{self, Edit, ObjectLayer};
use kobo_core::tiers::{self, Tier};
use kobo_core::{Rom, RomIdentity, config, import};

use crate::app::{App, Startup};
use crate::selection::Item;

/// The vanilla ROM, or `None` after saying the test skips; a failure if
/// the ROM tier is required (`KOBO_REQUIRE_ROM`, `KOBO_REQUIRE_ALL`).
fn vanilla() -> Option<Rom> {
    let Ok(path) = config::vanilla_rom_path() else {
        assert!(
            !Tier::Rom.required(),
            "the ROM tier is required, and no vanilla ROM is configured"
        );
        eprintln!("skipping: no vanilla ROM is configured");
        if let Some(log) = std::env::var_os(tiers::SKIP_LOG_ENV_VAR) {
            use std::io::Write;
            let name = std::thread::current().name().unwrap_or("?").to_string();
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
            {
                let _ = writeln!(
                    file,
                    "{name}\t{}\tno vanilla ROM is configured",
                    Tier::Rom.name()
                );
            }
        }
        return None;
    };
    let rom = Rom::load(&path).expect("the configured ROM loads");
    assert_eq!(rom.identify(), RomIdentity::VanillaUsa);
    Some(rom)
}

/// A project holding vanilla level 105, in a scratch folder.
struct Project(PathBuf);

impl Project {
    fn new(clean: &Rom, name: &str) -> Self {
        Self::with_level(clean, name, 0x105)
    }

    fn with_level(clean: &Rom, name: &str, number: u16) -> Self {
        let dir =
            std::env::temp_dir().join(format!("kobo-test-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("levels")).unwrap();
        let (level, _) = import::read_level(clean, number).unwrap();
        std::fs::write(
            dir.join(format!("levels/{number:03X}.toml")),
            level.to_toml(&Default::default()),
        )
        .unwrap();
        let manifest =
            format!("format = 1\n\n[levels]\n0x{number:03X} = \"levels/{number:03X}.toml\"\n");
        std::fs::write(dir.join("kobo.toml"), manifest).unwrap();
        Self(dir)
    }

    fn level_file(&self) -> PathBuf {
        self.0.join("levels/105.toml")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(project: &Path) -> Harness<'static, App> {
    harness_for(project, 0x105)
}

fn harness_for(project: &Path, level: u16) -> Harness<'static, App> {
    let startup = Startup {
        project: Some(project.to_path_buf()),
        level: Some(level),
        ..Default::default()
    };
    Harness::builder()
        .with_size([1600.0, 1400.0])
        .build_eframe(move |cc| App::new(cc, startup))
}

/// Steps the window until `done`, as the picture comes from another
/// thread and file changes from the system.
fn wait_for(harness: &mut Harness<'static, App>, what: &str, done: impl Fn(&App) -> bool) {
    let started = Instant::now();
    while !done(harness.state()) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "waited too long for {what}: {:?}",
            harness.state().current().map(|o| (
                o.render_error.clone(),
                o.requested,
                o.shown,
                o.document.undo_label().map(str::to_owned),
                harness.state().status().map(str::to_owned),
            ))
        );
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    // Once more, for what the change leads to on screen.
    harness.step();
}

fn drawn(app: &App) -> bool {
    app.current()
        .is_some_and(|o| o.geometry.is_some() && o.up_to_date())
}

/// Where a level tile's middle is on screen.
fn tile_on_screen(app: &App, x: i32, y: i32) -> Pos2 {
    let open = app.current().unwrap();
    let camera = open.camera.expect("the canvas has been shown");
    let at = Pos2::new(x as f32 * 16.0 + 8.0, y as f32 * 16.0 + 8.0);
    camera.to_screen(open.canvas, at)
}

fn object_place(app: &App, index: usize) -> Option<(u16, u16)> {
    edit::object_position(&app.current()?.document.level().layer1[index])
}

fn press(harness: &mut Harness<'static, App>, at: Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

/// Clicks the tile, and comes back with the object it selected.
fn click_tile(harness: &mut Harness<'static, App>, x: i32, y: i32) {
    let at = tile_on_screen(harness.state(), x, y);
    harness.hover_at(at);
    harness.step();
    press(harness, at, true);
    harness.step();
    press(harness, at, false);
    harness.step();
}

/// Brings tile (`x`, `y`) into view, wherever the camera started.
fn look_at(harness: &mut Harness<'static, App>, x: i32, y: i32) {
    let open = harness.state_mut().current_mut().unwrap();
    open.selection = vec![Item::object(ObjectLayer::One, 0)];
    let camera = open.camera.as_mut().unwrap();
    camera.offset.x = (x as f32 * 16.0 - 100.0).max(0.0);
    let _ = y;
    harness.step();
    harness.step();
}

#[test]
fn clicking_a_tile_selects_the_object_that_drew_it() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "click");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);

    // Object 10, the ground ledge at (60, 20), 12 by 5.
    click_tile(&mut harness, 65, 22);
    let selection = harness.state().current().unwrap().selection.clone();
    assert_eq!(selection, [Item::object(ObjectLayer::One, 10)]);
    // The sky is nothing, and clicking it clears the selection.
    click_tile(&mut harness, 65, 2);
    assert!(harness.state().current().unwrap().selection.is_empty());
}

#[test]
fn dragging_moves_by_whole_tiles_and_undo_puts_it_back() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "drag");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);

    let from = tile_on_screen(harness.state(), 65, 22);
    let to = tile_on_screen(harness.state(), 68, 21);
    harness.hover_at(from);
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=8 {
        harness.hover_at(from + (to - from) * (i as f32 / 8.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();

    assert_eq!(object_place(harness.state(), 10), Some((63, 19)));
    let open = harness.state().current().unwrap();
    assert!(open.document.is_modified());
    assert_eq!(open.document.undo_label(), Some("Move object"));
    wait_for(&mut harness, "the moved picture", drawn);
    // The picture now has the ledge where it was moved.
    let geometry = harness
        .state()
        .current()
        .unwrap()
        .geometry
        .as_ref()
        .unwrap();
    let owner = geometry
        .loaded
        .objects
        .owner_at(&geometry.loaded.tiles, ObjectLayer::One, 74, 20);
    assert_eq!(owner.map(|o| o.index), Some(10));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.step();
    assert_eq!(object_place(harness.state(), 10), Some((60, 20)));
    assert!(!harness.state().current().unwrap().document.is_modified());
}

#[test]
fn delete_and_save_write_the_file() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "delete");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    let before = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .layer1
        .len();
    harness.key_press(Key::Delete);
    harness.step();
    let open = harness.state().current().unwrap();
    assert_eq!(open.document.level().layer1.len(), before - 1);
    assert!(open.selection.is_empty());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.step();
    let saved = std::fs::read_to_string(project.level_file()).unwrap();
    assert!(!saved.contains("{ obj = 0x14, x = 60, y = 20,"));
    assert!(!harness.state().current().unwrap().document.is_modified());
}

#[test]
fn an_edit_on_disk_is_followed() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "outside");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);

    let path = project.level_file();
    let text = std::fs::read_to_string(&path).unwrap();
    let moved = text.replace(
        "{ obj = 0x14, x = 60, y = 20,",
        "{ obj = 0x14, x = 61, y = 20,",
    );
    assert_ne!(text, moved);
    std::fs::write(&path, moved).unwrap();
    wait_for(&mut harness, "the change on disk", |app| {
        object_place(app, 10) == Some((61, 20))
    });
    let open = harness.state().current().unwrap();
    assert!(
        !open.document.is_modified(),
        "the file and the editor agree"
    );
    assert_eq!(open.document.undo_label(), Some("Change on disk"));
    wait_for(&mut harness, "the picture of it", drawn);
}

#[test]
fn placing_puts_objects_last_and_sprites_in_screen_order() {
    use crate::palette::Placing;
    use kobo_core::level::objects::Object;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "place");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);

    let objects = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .layer1
        .len();
    let coins = Object::Standard {
        number: 0x05,
        x: 0,
        y: 0,
        settings: 0,
    };
    harness.state_mut().placing = Some(Placing::Object(coins));
    click_tile(&mut harness, 66, 15);
    let open = harness.state().current().unwrap();
    let level = open.document.level();
    assert_eq!(level.layer1.len(), objects + 1);
    assert_eq!(
        edit::object_position(&level.layer1[objects]),
        Some((66, 15))
    );
    assert_eq!(open.selection, [Item::object(ObjectLayer::One, objects)]);

    // A Goomba on screen 4, among the Rexes of screens 3 to 5.
    harness.state_mut().placing = Some(Placing::Sprite(0x0F));
    click_tile(&mut harness, 70, 18);
    let open = harness.state().current().unwrap();
    let list = &open.document.level().sprites.list;
    let Some(&[Item::Sprite(index)]) = Some(&open.selection[..]) else {
        panic!("the new sprite is selected: {:?}", open.selection);
    };
    assert_eq!(
        (list[index].id, list[index].x, list[index].y),
        (0x0F, 70, 18)
    );
    let screens: Vec<u16> = list.iter().map(|s| s.x / 16).collect();
    assert!(
        screens.is_sorted(),
        "the list stays in screen order: {screens:?}"
    );
    assert!(
        harness.state().placing.is_some(),
        "placing goes on until Esc"
    );
    harness.key_press(Key::Escape);
    harness.step();
    assert!(harness.state().placing.is_none());
    wait_for(&mut harness, "the picture with both", drawn);
}

#[test]
fn the_handle_resizes_an_object() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "resize");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);

    // The ground ledge's box ends at the corner of tile (71, 24); its
    // handle sits on that corner.
    let corner = |app: &App| {
        let open = app.current().unwrap();
        let camera = open.camera.unwrap();
        camera.to_screen(open.canvas, Pos2::new(72.0 * 16.0, 25.0 * 16.0))
    };
    let from = corner(harness.state());
    let to = from
        + egui::vec2(2.0 * 16.0, -16.0) * harness.state().current().unwrap().camera.unwrap().zoom;
    harness.hover_at(from);
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=8 {
        harness.hover_at(from + (to - from) * (i as f32 / 8.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();

    let open = harness.state().current().unwrap();
    let text = open.document.text();
    assert!(
        text.contains("{ obj = 0x14, x = 60, y = 20, height = 4, width = 14 }"),
        "{}",
        text.lines()
            .find(|l| l.contains("obj = 0x14, x = 60"))
            .unwrap_or("")
    );
    assert_eq!(open.document.undo_label(), Some("Resize object"));
    assert_eq!(
        object_place(harness.state(), 10),
        Some((60, 20)),
        "it did not move"
    );
}

#[test]
fn levels_are_added_by_copy_or_from_the_games_own() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "add");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);

    // With nothing selected, the level panel copies the level.
    harness.state_mut().copy_to = 0x106;
    harness.step();
    harness.get_by_label("Copy this one there").click();
    harness.step();
    wait_for(&mut harness, "the copy", |app| {
        app.current_number() == Some(0x106) && drawn(app)
    });
    assert!(project.0.join("levels/106.toml").exists());
    let manifest = std::fs::read_to_string(project.0.join("kobo.toml")).unwrap();
    assert!(
        manifest.contains("0x106 = \"levels/106.toml\""),
        "{manifest}"
    );
    let copy = harness.state().current().unwrap().document.level();
    assert!(
        copy.entrances.is_empty(),
        "the entrances stay the original's"
    );

    // A level the project leaves as the game's is added once confirmed.
    harness.state_mut().adding = Some(0x107);
    // The dialog fades in before it takes clicks.
    harness.run_steps(20);
    harness.get_by_label("Add to the project").click();
    harness.step();
    wait_for(&mut harness, "level 107", |app| {
        app.current_number() == Some(0x107) && drawn(app)
    });
    assert!(project.0.join("levels/107.toml").exists());
}

#[test]
fn layer_2_objects_are_selected_and_placed_where_layer_2_shows() {
    use crate::palette::Placing;
    use kobo_core::level::objects::Object;
    use kobo_core::source::level::Layer2;

    let Some(clean) = vanilla() else { return };
    let project = Project::with_level(&clean, "layer2", 0x00E);
    let mut harness = harness_for(&project.0, 0x00E);
    wait_for(&mut harness, "the picture", drawn);

    // Layer 2's first object, at its tile (3, 19).
    let at = {
        let open = harness.state().current().unwrap();
        let geometry = open.geometry.as_ref().unwrap();
        let offset = geometry.layer_offset(ObjectLayer::Two);
        let camera = open.camera.unwrap();
        camera.to_screen(
            open.canvas,
            Pos2::new(3.0 * 16.0 + 8.0, 19.0 * 16.0 + 8.0) + offset,
        )
    };
    harness.hover_at(at);
    harness.step();
    press(&mut harness, at, true);
    harness.step();
    press(&mut harness, at, false);
    harness.step();
    let selection = harness.state().current().unwrap().selection.clone();
    assert_eq!(selection, [Item::object(ObjectLayer::Two, 0)]);

    let layer2 = |app: &App| match &app.current().unwrap().document.level().layer2 {
        Layer2::Objects(list) => list.clone(),
        _ => panic!("level 00E has layer 2 objects"),
    };
    let before = layer2(harness.state()).len();
    harness.state_mut().place_layer = ObjectLayer::Two;
    harness.state_mut().placing = Some(Placing::Object(Object::Standard {
        number: 0x05,
        x: 0,
        y: 0,
        settings: 0,
    }));
    harness.hover_at(at);
    harness.step();
    press(&mut harness, at, true);
    harness.step();
    press(&mut harness, at, false);
    harness.step();
    let after = layer2(harness.state());
    assert_eq!(after.len(), before + 1);
    assert_eq!(edit::object_position(&after[before]), Some((3, 19)));
}

#[test]
fn entrances_are_added_with_a_free_number_and_removed() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "entrances");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let before = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .entrances
        .clone();

    harness.get_by_label("Add an entrance").click();
    harness.step();
    let after = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .entrances
        .clone();
    assert_eq!(after.len(), before.len() + 1);
    let new = after.last().unwrap().id;
    assert!(before.iter().all(|e| e.id != new) && new >> 8 == 1);
    wait_for(&mut harness, "the picture with it", drawn);

    harness.get_by_label(&format!("Entrance {new:03X}")).click();
    harness.run_steps(10);
    harness.get_by_label("Remove this entrance").click();
    harness.step();
    let entrances = &harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .entrances;
    assert_eq!(*entrances, before);
}

#[test]
fn the_outline_selects_what_the_canvas_cannot_and_keys_reorder() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "outline");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.get_by_label("Objects").click();
    harness.state_mut().outline.filter = "exit".to_string();
    harness.state_mut().outline.drawing_order = true;
    harness.run_steps(3);
    // Level 105's screen exit has no place on the canvas.
    harness
        .get_by_label_contains("screen 07 → level 1CB")
        .click();
    harness.step();
    let exit = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .layer1
        .len()
        - 1;
    let selection = harness.state().current().unwrap().selection.clone();
    assert_eq!(selection, [Item::object(ObjectLayer::One, exit)]);

    // Ctrl+[ sends it one back in the list, and the selection follows.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::OpenBracket);
    harness.step();
    let open = harness.state().current().unwrap();
    assert_eq!(open.selection, [Item::object(ObjectLayer::One, exit - 1)]);
    assert!(matches!(
        open.document.level().layer1[exit - 1],
        kobo_core::level::objects::Object::ScreenExit(_)
    ));
    assert_eq!(open.document.undo_label(), Some("Send object backward"));
}

#[test]
fn copying_pastes_where_the_mouse_is_and_duplicating_steps_aside() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "clipboard");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    let count = |app: &App| app.current().unwrap().document.level().layer1.len();
    let before = count(harness.state());

    harness.event(egui::Event::Copy);
    harness.step();
    assert_eq!(harness.state().status(), Some("Copied 1 object"));
    let marker = "Kobo: 1 object".to_string();

    // Paste with the mouse at tile (64, 10): the copy's top left goes there.
    let at = tile_on_screen(harness.state(), 64, 10);
    harness.hover_at(at);
    harness.step();
    harness.event(egui::Event::Paste(marker));
    harness.step();
    let open = harness.state().current().unwrap();
    assert_eq!(count(harness.state()), before + 1);
    assert_eq!(object_place(harness.state(), before), Some((64, 10)));
    assert_eq!(open.selection, [Item::object(ObjectLayer::One, before)]);
    assert_eq!(open.document.undo_label(), Some("Paste 1 object"));

    // Text copied elsewhere is not pasted as objects.
    harness.event(egui::Event::Paste("hello".to_string()));
    harness.step();
    assert_eq!(count(harness.state()), before + 1);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::D);
    harness.step();
    assert_eq!(count(harness.state()), before + 2);
    assert_eq!(object_place(harness.state(), before + 1), Some((65, 11)));
}

#[test]
fn closing_a_project_with_edits_asks_first() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "switch");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    assert_eq!(
        harness.state().start.recent.first(),
        Some(&project.0.canonicalize().unwrap()),
        "opening a project remembers it"
    );
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    harness.key_press(Key::Delete);
    harness.step();

    harness.state_mut().switch_project(None);
    harness.run_steps(20);
    assert!(
        harness.state().workspace().is_some(),
        "not before the user says"
    );
    harness.get_by_label("Cancel").click();
    harness.run_steps(5);
    assert!(harness.state().workspace().is_some());

    harness.state_mut().switch_project(None);
    harness.run_steps(20);
    harness.get_by_label("Don't save").click();
    harness.run_steps(5);
    assert!(
        harness.state().workspace().is_none(),
        "back at the start screen"
    );
    let saved = std::fs::read_to_string(project.level_file()).unwrap();
    assert!(
        saved.contains("{ obj = 0x14, x = 60, y = 20,"),
        "nothing was saved"
    );
}

#[test]
fn building_writes_the_rom_and_a_patch_with_unsaved_edits() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "build");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    harness.key_press(Key::Delete);
    harness.step();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
    harness.step();
    assert!(harness.state().build.open, "the build window opens");
    wait_for(&mut harness, "the build", |app| app.build.succeeded());
    assert_eq!(
        harness.state().build.stages_done(),
        kobo_core::build::Stage::ALL.len()
    );
    let built = Rom::load(project.0.join("build.sfc")).unwrap();
    let patch = std::fs::read(project.0.join("build.bps")).unwrap();
    let patched = kobo_core::bps::apply_to_rom(&patch, &clean).unwrap();
    assert_eq!(patched.data, built.data());
    // The deleted ledge is not in the build: its tile is the blank one.
    let loaded = kobo_core::expand::expand_level(&built, 0x105).unwrap();
    assert_eq!(loaded.tiles.tile_at(65, 22), 0x25);
}

#[test]
fn entrances_are_marked_where_the_loader_puts_the_player() {
    use crate::preview::EntryKind;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "entries");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the secondary entrances", |app| {
        app.current().is_some_and(|o| {
            o.entries
                .iter()
                .any(|e| e.kind == EntryKind::Secondary(0x1CB))
        })
    });
    let open = harness.state().current().unwrap();
    let place = |kind| {
        open.entries
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| (e.x, e.y))
    };
    // Level 105 starts at its left, on the ground; entrance 1CB is on
    // screen 8, out of the diagonal pipe; the midway is on screen 9.
    assert_eq!(place(EntryKind::Main), Some((16, 352)));
    assert_eq!(place(EntryKind::Secondary(0x1CB)), Some((2072, 306)));
    assert_eq!(place(EntryKind::Midway).map(|(x, _)| x / 256), Some(9));
}

#[test]
fn changes_since_the_last_commit_are_listed_and_taken_back() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "changes");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&project.0)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "Start"]);

    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    // Delete the ground ledge, and move the bush that stood on it.
    click_tile(&mut harness, 65, 22);
    harness.key_press(Key::Delete);
    harness.step();
    click_tile(&mut harness, 67, 19);
    harness.key_press(Key::ArrowRight);
    harness.step();

    harness.get_by_label("Git").click();
    harness.run_steps(2);
    harness
        .get_by_label("Changes since the last commit")
        .click();
    harness.run_steps(3);
    let count = |app: &App| app.current().unwrap().head.as_ref().map(|h| h.diff().len());
    assert_eq!(count(harness.state()), Some(2));
    harness
        .get_by_label_contains("Ground ledge at (60, 20)")
        .click();
    harness.step();
    let revert = harness
        .get_all_by_label("Revert")
        .next()
        .expect("a revert button");
    revert.click();
    harness.run_steps(3);
    assert_eq!(count(harness.state()), Some(1), "the ledge is back");
    let level = harness.state().current().unwrap().document.level().clone();
    assert!(level.layer1.iter().any(|o| matches!(
        o,
        kobo_core::level::objects::Object::Standard {
            number: 0x14,
            x: 60,
            y: 20,
            ..
        }
    )));
}

#[test]
fn the_command_palette_finds_and_runs_by_name() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "commands");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run_steps(3);
    assert!(harness.state().commands.open);
    // The palette's field has the keyboard.
    harness.event(egui::Event::Text("add coin".into()));
    harness.run_steps(3);
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    assert!(!harness.state().commands.open);
    let placing = harness.state().placing.clone();
    assert_eq!(
        placing,
        Some(crate::palette::Placing::Object(
            kobo_core::level::objects::Object::Standard {
                number: 0x05,
                x: 0,
                y: 0,
                settings: 0,
            }
        )),
        "the best match for `add coin` is object 05, Coin"
    );
}

#[test]
fn a_map16_tile_is_placed_directly_and_drawn() {
    use crate::palette::Placing;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "map16");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    harness.state_mut().placing = Some(Placing::Map16(0x130));
    click_tile(&mut harness, 64, 10);
    let objects = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .layer1
        .len();
    let placed = harness.state().current().unwrap().document.level().layer1[objects - 1].clone();
    assert_eq!(
        kobo_core::edit::map16_object_parts(&placed),
        Some((0x130, 1, 1))
    );
    wait_for(&mut harness, "the picture with it", drawn);
    let open = harness.state().current().unwrap();
    if let Some(error) = &open.render_error {
        panic!("{error}");
    }
    let loaded = &open.geometry.as_ref().unwrap().loaded;
    assert_eq!(loaded.tiles.tile_at(64, 10), 0x130);
    let owner = loaded
        .objects
        .owner_at(&loaded.tiles, ObjectLayer::One, 64, 10);
    assert_eq!(owner.map(|o| o.index), Some(objects - 1));
}

#[test]
fn a_screen_exit_is_dragged_to_another_screen() {
    use kobo_core::level::objects::Object;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "exit-drag");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    // Bring screen 7, where level 105's exit is, into view.
    {
        let open = harness.state_mut().current_mut().unwrap();
        let camera = open.camera.as_mut().unwrap();
        camera.offset.x = 7.0 * 256.0;
        camera.offset.y = 0.0;
    }
    harness.run_steps(2);
    // The exit's label is a little below the screen's top left.
    let (from, to) = {
        let open = harness.state().current().unwrap();
        let camera = open.camera.unwrap();
        let top = camera.to_screen(open.canvas, Pos2::new(7.0 * 256.0, 0.0));
        let label = Pos2::new(top.x + 30.0, top.y.max(open.canvas.top()) + 34.0);
        let target = camera.to_screen(open.canvas, Pos2::new(9.0 * 256.0 + 40.0, 100.0));
        (label, target)
    };
    harness.hover_at(from);
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=10 {
        harness.hover_at(from + (to - from) * (i as f32 / 10.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();
    let level = harness.state().current().unwrap().document.level().clone();
    let screens: Vec<u8> = level
        .layer1
        .iter()
        .filter_map(|o| match o {
            Object::ScreenExit(exit) => Some(exit.screen),
            _ => None,
        })
        .collect();
    assert_eq!(screens, [9]);
}

#[test]
fn the_start_is_dragged_to_the_nearest_place_its_settings_allow() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "start-drag");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    {
        let open = harness.state_mut().current_mut().unwrap();
        let camera = open.camera.as_mut().unwrap();
        camera.offset = egui::Vec2::ZERO;
    }
    harness.run_steps(2);
    // From the start's pole, at (16, 352) and 8 to the right, to near
    // screen 1's X setting 1 (0x80) and Y setting 4 (0xA0).
    let (from, to) = {
        let open = harness.state().current().unwrap();
        let camera = open.camera.unwrap();
        (
            camera.to_screen(open.canvas, Pos2::new(24.0, 360.0)),
            camera.to_screen(
                open.canvas,
                Pos2::new(256.0 + 0x82 as f32 + 8.0, 0xA3 as f32),
            ),
        )
    };
    harness.hover_at(from);
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=10 {
        harness.hover_at(from + (to - from) * (i as f32 / 10.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();
    let e = harness.state().current().unwrap().document.level().entrance;
    assert_eq!((e.entrance_screen, e.entrance_x, e.entrance_y), (1, 1, 4));
    assert_eq!(
        harness.state().current().unwrap().document.undo_label(),
        Some("Move the start")
    );
}

#[test]
fn a_level_is_taken_out_of_the_project_once_confirmed() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "remove");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().removing = Some(0x105);
    harness.run_steps(20);
    harness.get_by_label("Take it out").click();
    harness.run_steps(3);
    assert!(!project.level_file().exists());
    assert!(!harness.state().has_level(0x105));
    let manifest = std::fs::read_to_string(project.0.join("kobo.toml")).unwrap();
    assert!(!manifest.contains("0x105"), "{manifest}");
}

#[test]
fn a_secondary_entrance_is_dragged_keeping_where_its_action_puts_the_player() {
    use crate::preview::EntryKind;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "entrance-drag");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the secondary entrances", |app| {
        app.current().is_some_and(|o| {
            o.entries
                .iter()
                .any(|e| e.kind == EntryKind::Secondary(0x1CB))
        })
    });
    {
        let open = harness.state_mut().current_mut().unwrap();
        let camera = open.camera.as_mut().unwrap();
        camera.offset = egui::vec2(2000.0, 150.0);
    }
    harness.run_steps(2);
    // Entrance 1CB stands at (2072, 306): screen 8, X setting 0, Y setting
    // 9 (2064, 304) and its pipe's 8 and 2 more. To screen 9's X setting 1.
    let (from, to) = {
        let open = harness.state().current().unwrap();
        let camera = open.camera.unwrap();
        (
            camera.to_screen(open.canvas, Pos2::new(2072.0 + 8.0, 306.0 + 8.0)),
            camera.to_screen(
                open.canvas,
                Pos2::new((9 * 256 + 0x80 + 8 + 8) as f32, 306.0 + 8.0),
            ),
        )
    };
    harness.hover_at(from);
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=10 {
        harness.hover_at(from + (to - from) * (i as f32 / 10.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();
    let level = harness.state().current().unwrap().document.level().clone();
    let e = level.entrances.iter().find(|e| e.id == 0x1CB).unwrap();
    assert_eq!((e.screen, e.x, e.y), (9, 1, 9));
}

#[test]
fn entrances_move_by_the_settings_that_place_them() {
    use crate::canvas::{Placement, entry_placement, move_entry};
    use crate::preview::EntryKind;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "placements");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let mut level = harness.state().current().unwrap().document.level().clone();

    // The game's midway entrance moves by its screen alone.
    assert_eq!(
        entry_placement(&level, EntryKind::Midway),
        Some(Placement::Screen(9))
    );
    let (_, edits) = move_entry(&level, EntryKind::Midway, Placement::Screen(0x14)).unwrap();
    let mut header = level.entrance;
    header.midway_screen = 4;
    let mut settings = level.settings;
    settings.midway.screen_high = true;
    assert_eq!(
        edits,
        [Edit::SetEntrance(header), Edit::SetSettings(settings)]
    );
    assert!(move_entry(&level, EntryKind::Midway, Placement::Screen(9)).is_none());

    // Position method 2 splits the tile between the settings' X and Y and
    // the method's high bits.
    level.settings.tile_position = Some((0, 1));
    let Some(Placement::Tiles(_, x, y)) = entry_placement(&level, EntryKind::Main) else {
        panic!("the start is placed by tile");
    };
    assert_eq!(
        (x, y),
        (
            u16::from(level.entrance.entrance_x),
            16 | u16::from(level.entrance.entrance_y)
        )
    );
    let (_, edits) = move_entry(&level, EntryKind::Main, Placement::Tiles(2, 12, 30)).unwrap();
    let mut header = level.entrance;
    (header.entrance_screen, header.entrance_x, header.entrance_y) = (2, 4, 14);
    let mut settings = level.settings;
    settings.tile_position = Some((1, 1));
    assert_eq!(
        edits,
        [Edit::SetEntrance(header), Edit::SetSettings(settings)]
    );
}

#[test]
fn a_background_is_chosen_from_pictures_of_them_all() {
    use egui_kittest::kittest::Queryable;
    use kobo_core::source::level::Layer2;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "backgrounds");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().backgrounds.open = true;
    harness.run_steps(2);
    wait_for(&mut harness, "every background's picture", |app| {
        !app.backgrounds.busy()
    });
    harness.get_by_label("Use Big clouds (0CDD44)").click();
    harness.run_steps(2);
    let level = harness.state().current().unwrap().document.level().clone();
    assert_eq!(
        level.layer2,
        Layer2::VanillaBackground(kobo_core::addr::SnesAddr::new(0x0C_DD44))
    );
    // One undo step takes it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.step();
    let level = harness.state().current().unwrap().document.level().clone();
    assert_eq!(
        level.layer2,
        Layer2::VanillaBackground(kobo_core::addr::SnesAddr::new(0x0C_D900))
    );
}

#[test]
fn the_main_entrance_takes_a_camera_from_the_player() {
    use egui_kittest::kittest::Queryable;
    use kobo_core::entrance::Camera;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "camera");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.get_by_label("Main entrance").click();
    harness.run_steps(2);
    harness.get_by_value("the game's positions").click();
    harness.run_steps(2);
    harness.get_by_label("from the player").click();
    harness.run_steps(2);
    let level = harness.state().current().unwrap().document.level().clone();
    assert_eq!(level.settings.relative, Some(false));
    let e = level.entrance;
    assert_eq!(
        Camera::from_bits(e.fg_position, e.bg_position, level.settings.relative, false),
        Camera::Relative(0)
    );
    assert_eq!(
        harness.state().current().unwrap().document.undo_label(),
        Some("Change the main entrance")
    );
}

#[test]
fn a_sprite_is_changed_to_another_by_name() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "picker");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().current_mut().unwrap().selection = vec![Item::Sprite(0)];
    harness.run_steps(2);
    let before = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .sprites
        .list[0]
        .clone();
    let name = format!("{:02X}  {}", before.id, kobo_core::names::sprite(before.id));
    harness.get_by_label_contains(&name).click();
    harness.run_steps(2);
    // The popup's search field has the focus.
    harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.is_focused())
        .expect("the search field has the focus")
        .type_text("buzzy");
    harness.run_steps(2);
    harness.get_by_label_contains("Buzzy Beetle").click();
    harness.run_steps(2);
    let after = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .sprites
        .list[0]
        .clone();
    assert_eq!(after.id, 0x11);
    assert_eq!((after.x, after.y), (before.x, before.y));
}

#[test]
fn a_screen_exit_goes_to_the_level_it_leads_to() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "go-to");
    // Level 105's pipe leads to entrance 1CB, which is into 105 itself;
    // an exit to level 106 leads there instead.
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let level = harness.state().current().unwrap().document.level().clone();
    let index = level
        .layer1
        .iter()
        .position(|o| matches!(o, kobo_core::level::objects::Object::ScreenExit(_)))
        .expect("level 105 has a screen exit");
    harness.state_mut().current_mut().unwrap().selection =
        vec![Item::object(ObjectLayer::One, index)];
    harness.run_steps(2);
    let workspace = harness.state().workspace().unwrap().clone();
    let target = match &level.layer1[index] {
        kobo_core::level::objects::Object::ScreenExit(exit) => edit::ExitTarget::of(*exit, 0x105),
        _ => unreachable!(),
    };
    let to = if target.secondary {
        workspace.entrance_level(target.destination).unwrap()
    } else {
        target.destination
    };
    harness
        .get_by_label(&format!("Go to level {to:03X}"))
        .click();
    harness.run_steps(2);
    // The project lists 105 alone: the other is offered to add first.
    assert_eq!(harness.state().adding, Some(to));
    harness.get_by_label("Add to the project").click();
    harness.run_steps(2);
    assert_eq!(harness.state().current_number(), Some(to));
    wait_for(&mut harness, "the entrance in view", |app| {
        app.current()
            .is_some_and(|o| o.look_at.is_none() && o.camera.is_some())
    });
}

#[test]
fn back_and_forward_go_between_the_levels_shown() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "history");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let level = harness
        .state()
        .workspace()
        .unwrap()
        .clean_level(0x106)
        .unwrap();
    harness.state_mut().add_level(0x106, &level);
    harness.run_steps(2);
    assert_eq!(harness.state().current_number(), Some(0x106));
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    harness.step();
    assert_eq!(harness.state().current_number(), Some(0x105));
    assert!(!harness.state().can_go_back(false));
    harness.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    harness.step();
    assert_eq!(harness.state().current_number(), Some(0x106));
    // Nothing selected moved: the arrows were taken for going back.
    assert!(
        harness
            .state()
            .current()
            .unwrap()
            .document
            .undo_label()
            .is_none()
    );
}

#[test]
fn find_lists_what_every_level_has_and_opens_one() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "find");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let level = harness
        .state()
        .workspace()
        .unwrap()
        .clean_level(0x106)
        .unwrap();
    harness.state_mut().add_level(0x106, &level);
    harness.run_steps(2);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    harness.run_steps(2);
    harness
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.is_focused())
        .expect("the search field has the focus")
        .type_text("dragon coin");
    harness.run_steps(2);
    let found = kobo_core::edit::find::find(harness.state().workspace().unwrap(), "dragon coin");
    assert!(found.iter().any(|f| f.level == 0x105));
    assert!(found.iter().any(|f| f.level == 0x106));
    assert!(found.iter().all(|f| f.name == "Dragon coin"));
    // The first, in 105, opens 105 with it selected.
    let first = &found[0];
    assert_eq!(first.level, 0x105);
    harness
        .get_all_by_label_contains("Dragon coin")
        .next()
        .expect("the results list them")
        .click();
    harness.run_steps(2);
    let open = harness.state().current().unwrap();
    assert_eq!(open.number, 0x105);
    let kobo_core::edit::find::Entry::Object(layer, index) = first.entry else {
        panic!("a Dragon coin is an object");
    };
    assert_eq!(open.selection, [Item::object(layer, index)]);
}

#[test]
fn a_level_takes_a_palette_and_graphics_list_of_its_own() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "palette");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let before = harness.state().current().unwrap().image.clone();
    harness.get_by_label("Graphics and palette").click();
    harness.run_steps(2);
    harness
        .get_by_label("Give the level a palette of its own")
        .click();
    harness.run_steps(2);
    harness
        .get_by_label("Give the level a list of its own")
        .click();
    harness.run_steps(2);
    let level = harness.state().current().unwrap().document.level().clone();
    let palette = level.palette.expect("a palette of its own");
    assert_eq!(
        level.graphics,
        Some(kobo_core::exgfx::GraphicsList::DEFAULT)
    );
    // The game's colours for it, so the level looks as it did.
    let clean_rom = harness.state().workspace().unwrap().clean().clone();
    assert_eq!(
        palette,
        kobo_core::palette::game_palette(&clean_rom, &level.header).unwrap()
    );
    wait_for(&mut harness, "the picture with them", |app| {
        app.current()
            .is_some_and(|o| o.shown == o.requested && o.render_error.is_none())
    });
    assert_eq!(harness.state().current().unwrap().image, before);
    // Two steps, each undone on its own.
    assert_eq!(
        harness.state().current().unwrap().document.undo_label(),
        Some("Add a graphics list")
    );
}

#[test]
fn play_from_here_builds_a_rom_that_starts_there() {
    let Some(clean) = vanilla() else { return };
    if kobo_core::tools::Tool::Asar.locate_offline().is_err() {
        return;
    }
    let project = Project::new(&clean, "play");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    // An unsaved edit is in the build too: a ledge goes.
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    harness.key_press(Key::Delete);
    harness.step();
    let start = crate::play::start_at(
        harness.state().current().unwrap(),
        (70.0 * 16.0, 19.0 * 16.0),
        2,
    );
    assert_eq!((start.x, start.y), (70, 18));
    crate::play::start(harness.state_mut(), start);
    wait_for(&mut harness, "the build to play", |app| !app.play.busy());
    let message = harness.state().status().unwrap_or_default().to_string();
    assert!(message.starts_with("Playing level 105"), "{message}");
    // Into the user's cache, not the project's folder.
    assert!(!project.0.join("play.sfc").exists());
    let path = kobo_core::playtest::rom_path(&project.0).unwrap();
    let rom = Rom::load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        rom.read_u8(kobo_core::addr::SnesAddr::new(0x0096BE))
            .unwrap(),
        0x22
    );
    let loaded = kobo_core::expand::expand_level(&rom, 0x105).unwrap();
    assert_eq!(loaded.tiles.tile_at(65, 22), 0x25);
    // The project's files are as they were: the edit is not saved.
    assert!(harness.state().is_modified(0x105));

    // From the level's start, found by loading it: level 105's player
    // starts on tile (1, 22).
    crate::play::from_level_start(harness.state_mut(), 0x105, 0);
    wait_for(&mut harness, "the build to play", |app| !app.play.busy());
    let message = harness.state().status().unwrap_or_default().to_string();
    assert!(
        message.starts_with("Playing level 105 from (1, 22) as Small Mario"),
        "{message}"
    );
}

#[test]
fn the_project_window_says_what_the_project_holds() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "project-window");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().project_open = true;
    harness.run_steps(2);
    harness.get_by_label_contains("1 of 512");
    harness.get_by_label("not needed: the build keeps the game's");
}

#[test]
fn dragging_with_ctrl_held_copies() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "copy-drag");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    let before = harness
        .state()
        .current()
        .unwrap()
        .document
        .level()
        .layer1
        .len();

    let from = tile_on_screen(harness.state(), 65, 22);
    let to = tile_on_screen(harness.state(), 68, 21);
    harness.hover_at(from);
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.step();
    press(&mut harness, from, true);
    harness.step();
    for i in 1..=8 {
        harness.hover_at(from + (to - from) * (i as f32 / 8.0));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.step();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.step();

    let open = harness.state().current().unwrap();
    // The ledge stays, and a copy of it is three across and one up.
    assert_eq!(open.document.level().layer1.len(), before + 1);
    assert_eq!(object_place(harness.state(), 10), Some((60, 20)));
    let [Item::Object(copy)] = open.selection[..] else {
        panic!("the copy is selected: {:?}", open.selection);
    };
    assert_eq!(object_place(harness.state(), copy.index), Some((63, 19)));
    assert_eq!(open.document.undo_label(), Some("Copy 1 object"));
}

#[test]
fn several_selected_are_listed_and_one_chosen_alone() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "several");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().current_mut().unwrap().selection =
        vec![Item::object(ObjectLayer::One, 10), Item::Sprite(0)];
    harness.run_steps(2);
    harness.get_by_label("1 object, 1 sprite");
    harness.get_by_label_contains("Ground ledge").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().current().unwrap().selection,
        [Item::object(ObjectLayer::One, 10)]
    );
}

#[test]
fn tab_steps_through_the_level_in_drawing_order() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "tab");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    harness.state_mut().current_mut().unwrap().selection = vec![Item::object(ObjectLayer::One, 10)];
    harness.step();
    harness.key_press(Key::Tab);
    harness.step();
    assert_eq!(
        harness.state().current().unwrap().selection,
        [Item::object(ObjectLayer::One, 11)]
    );
    for _ in 0..2 {
        harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        harness.step();
    }
    assert_eq!(
        harness.state().current().unwrap().selection,
        [Item::object(ObjectLayer::One, 9)]
    );
}

#[test]
fn f_centres_the_view_on_the_selection() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "centre");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let open = harness.state_mut().current_mut().unwrap();
    open.selection = vec![Item::object(ObjectLayer::One, 10)];
    open.camera.as_mut().unwrap().offset.x = 0.0;
    harness.step();
    harness.key_press(Key::F);
    harness.run_steps(2);
    // The ledge at (60, 20), 12 wide, centred on pixel 1056: the view
    // starts left of it by less than its middle.
    let camera = harness.state().current().unwrap().camera.unwrap();
    assert!(
        camera.offset.x > 500.0 && camera.offset.x < 1056.0,
        "the view starts at {}",
        camera.offset.x
    );
}

#[test]
fn a_level_goes_back_to_its_saved_file_as_one_step() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "back-to-saved");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let saved = harness.state().current().unwrap().document.level().clone();
    look_at(&mut harness, 60, 20);
    click_tile(&mut harness, 65, 22);
    harness.key_press(Key::Delete);
    harness.step();
    assert!(harness.state().is_modified(0x105));
    harness.state_mut().back_to_saved();
    harness.step();
    let open = harness.state().current().unwrap();
    assert_eq!(open.document.level(), &saved);
    assert!(!harness.state().is_modified(0x105));
    // Undo brings the edit back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.step();
    assert!(harness.state().is_modified(0x105));
}

#[test]
fn what_an_emulator_writes_beside_a_build_is_not_followed() {
    use crate::app::matters;
    for file in [
        "levels/105.toml",
        "kobo.toml",
        "map16/01.toml",
        "gfx/ExGFX80.png",
    ] {
        assert!(matters(Path::new(file)), "{file}");
    }
    for file in [
        "play.sfc",
        "play.srm",
        "build.sav",
        "play.state1",
        "play.000",
        "play.mss",
        ".git/index",
        "levels/105.toml~",
    ] {
        assert!(!matters(Path::new(file)), "{file}");
    }
}

#[test]
fn an_import_says_what_the_project_does_not_carry() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "import-report");
    // A hack that changes a byte of the game's code, which no import
    // carries.
    let mut hack = Rom::from_bytes(clean.data().to_vec()).unwrap();
    let at = kobo_core::addr::SnesAddr::new(0x00A1DA);
    let byte = hack.read(at, 1).unwrap()[0];
    hack.write_u8(at, byte ^ 0xFF).unwrap();
    let dir = project.0.join("imported");
    let report = import::import_rom(&hack, &clean, &dir, false).unwrap();
    let report = crate::start::ImportReport::new("hack.sfc".into(), true, &report);
    assert!(report.left_out.is_some());
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the level", drawn);
    harness.state_mut().start.report = Some(report);
    harness.run_steps(2);
    use egui_kittest::kittest::Queryable;
    harness.get_by_label_contains("locked by its author");
    harness.get_by_label_contains("outside its levels");
}

#[test]
fn a_mouse_wheel_scrolls_along_the_level() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "wheel");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let open = harness.state_mut().current_mut().unwrap();
    open.camera.as_mut().unwrap().offset = egui::vec2(400.0, 0.0);
    let middle = open.canvas.center();
    harness.hover_at(middle);
    harness.step();
    let offset =
        |harness: &Harness<'static, App>| harness.state().current().unwrap().camera.unwrap().offset;
    let before = offset(&harness);
    let wheel = |harness: &mut Harness<'static, App>, modifiers| {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -3.0),
            modifiers,
            phase: egui::TouchPhase::Move,
        });
        harness.run_steps(20);
    };
    // Down the wheel goes on through a horizontal level.
    wheel(&mut harness, Modifiers::NONE);
    let after = offset(&harness);
    assert!(after.x > before.x, "{before:?} to {after:?}");
    assert_eq!(after.y, before.y);
    // With Shift, back up the level's rows: none to go back past here,
    // and the view stays where it was along it.
    wheel(&mut harness, Modifiers::SHIFT);
    assert_eq!(offset(&harness).x, after.x);
}

#[test]
fn the_level_list_groups_sublevels_or_lists_by_number_and_leaves_unused_levels_out() {
    use egui_kittest::kittest::Queryable;

    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "level-list");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    for number in [0x1CB, 0x025] {
        let level = harness
            .state()
            .workspace()
            .unwrap()
            .clean_level(number)
            .unwrap();
        harness.state_mut().add_level(number, &level);
    }
    harness.state_mut().open_level(0x105);
    // The status bar names what was added.
    harness.state_mut().say("");
    harness.run_steps(3);
    // 105's pipe leads to 1CB, listed under it; 025 is the game's TEST
    // level, left out until asked for.
    harness.get_by_label_contains("105 Yoshi's Island 1");
    harness.get_by_label_contains("1CB ");
    assert!(harness.query_by_label_contains("025 ").is_none());
    harness
        .get_by_label_contains("Unused: the game's TEST level")
        .click();
    harness.run_steps(2);
    harness.get_by_label_contains("025 ");
    // Folding 105 hides its sublevel.
    let row = harness.get_by_label_contains("105 Yoshi's Island 1");
    let rect = row.rect();
    harness.hover_at(egui::pos2(rect.left() + 8.0, rect.center().y));
    harness.step();
    press(
        &mut harness,
        egui::pos2(rect.left() + 8.0, rect.center().y),
        true,
    );
    harness.step();
    press(
        &mut harness,
        egui::pos2(rect.left() + 8.0, rect.center().y),
        false,
    );
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("1CB ").is_none());
    // By number, every level shows, with nothing folded.
    harness.get_by_label("By number").click();
    harness.run_steps(2);
    harness.get_by_label_contains("1CB ");
    assert!(
        harness
            .query_by_label_contains("Not reached by an exit")
            .is_none()
    );
}

#[test]
fn play_from_a_click_in_the_ground_starts_on_it() {
    let Some(clean) = vanilla() else { return };
    let project = Project::new(&clean, "play-start");
    let mut harness = harness(&project.0);
    wait_for(&mut harness, "the picture", drawn);
    let open = harness.state().current().unwrap();
    let at = |x: f32, y: f32| (x * 16.0 + 8.0, y * 16.0 + 8.0);
    // Inside the ground ledge whose top is row 20: on top of it, his feet
    // in row 19 and his top in 18.
    let start = crate::play::start_at(open, at(65.0, 22.0), 0);
    assert_eq!((start.x, start.y), (65, 18));
    // In the air, where it was clicked.
    let start = crate::play::start_at(open, at(65.0, 12.0), 0);
    assert_eq!((start.x, start.y), (65, 11));
}
