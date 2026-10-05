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
        let dir =
            std::env::temp_dir().join(format!("kobo-test-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("levels")).unwrap();
        let (level, _) = import::read_level(clean, 0x105).unwrap();
        std::fs::write(
            dir.join("levels/105.toml"),
            level.to_toml(&Default::default()),
        )
        .unwrap();
        std::fs::write(
            dir.join("kobo.toml"),
            "format = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n",
        )
        .unwrap();
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
    let startup = Startup {
        project: Some(project.to_path_buf()),
        level: Some(0x105),
        ..Default::default()
    };
    Harness::builder()
        .with_size([1600.0, 940.0])
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
            harness
                .state()
                .current()
                .and_then(|o| o.render_error.clone())
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
    harness.get_by_label("Copy").click();
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
