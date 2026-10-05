//! The editor driven as a user would, without a display: open a level,
//! click and drag on the canvas, undo, save, and follow an outside edit.
//! They need the vanilla ROM, and skip as the library's ROM tests do
//! when it is not configured.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use kobo_core::edit::{self, ObjectLayer};
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
    harness.get_by_label("Outline").click();
    harness.state_mut().outline_filter = "exit".to_string();
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

    harness.get_by_label("Changes").click();
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
