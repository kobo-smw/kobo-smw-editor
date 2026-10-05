//! A level edited in memory builds and renders as edited
//! (`kobo_core::edit`), without the file being saved.

mod common;

use std::fs;
use std::sync::Arc;

use common::temp::TempDir;
use kobo_core::edit::{Edit, LevelDocument, ObjectLayer, Workspace};
use kobo_core::import;
use kobo_core::operation::Operation;
use kobo_core::render::{RenderOptions, Sprites};

#[test]
fn an_edit_in_memory_renders_without_a_save() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-workspace");
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    fs::create_dir_all(dir.join("levels")).unwrap();
    let path = dir.join("levels/105.toml");
    fs::write(&path, level.to_toml(&Default::default())).unwrap();
    fs::write(
        dir.join("kobo.toml"),
        "format = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n",
    )
    .unwrap();

    let mut document = LevelDocument::open(&path).unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
    };
    let render = |workspace: &Workspace| {
        workspace
            .preview(0x105, options, &Operation::default())
            .unwrap()
            .render
            .level
    };
    let before = render(&workspace);
    let ledge = before
        .objects
        .owner_at(&before.tiles, ObjectLayer::One, 65, 22);
    assert_eq!(ledge.map(|o| o.index), Some(10));

    // Object 10 is the ground ledge at (60, 20).
    let remove = Edit::RemoveObject {
        layer: ObjectLayer::One,
        index: 10,
    };
    document.apply("Delete object", &[remove]).unwrap();
    workspace.set_level(0x105, document.level());
    let after = render(&workspace);
    assert_eq!(after.tiles.tile_at(65, 22), 0x25, "the ledge is gone");
    assert_eq!(
        after
            .objects
            .owner_at(&after.tiles, ObjectLayer::One, 65, 22),
        None
    );
    // The bush that stood on it (object 11) is object 10 now.
    let bush = after
        .objects
        .owner_at(&after.tiles, ObjectLayer::One, 67, 19);
    assert_eq!(bush.map(|o| o.index), Some(10));
    assert!(document.is_modified(), "nothing was saved");
}

#[test]
fn a_level_is_added_from_the_clean_rom_or_a_copy() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-add-level");
    fs::write(dir.join("kobo.toml"), "# My hack.\nformat = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    assert_eq!(workspace.levels().count(), 0);

    let level = workspace.clean_level(0x105).unwrap();
    let path = workspace.add_level(0x105, &level).unwrap();
    assert_eq!(path, dir.join("levels/105.toml"));
    assert_eq!(workspace.levels().collect::<Vec<_>>(), [0x105]);
    let manifest = fs::read_to_string(dir.join("kobo.toml")).unwrap();
    assert!(manifest.starts_with("# My hack.\n"), "{manifest}");
    assert!(
        manifest.contains("0x105 = \"levels/105.toml\""),
        "{manifest}"
    );
    assert!(matches!(
        workspace.add_level(0x105, &level),
        Err(kobo_core::edit::WorkspaceError::HasLevel(0x105))
    ));

    // A copy draws as the original does. The secondary entrances into
    // the original stay its own.
    let copy = kobo_core::edit::copy_of(&level);
    assert!(!level.entrances.is_empty() && copy.entrances.is_empty());
    workspace.add_level(0x106, &copy).unwrap();
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
    };
    let draw = |n| {
        workspace
            .preview(n, options, &Operation::default())
            .unwrap()
            .render
            .level
            .tiles
    };
    let (a, b) = (draw(0x105), draw(0x106));
    assert_eq!((a.low, a.high), (b.low, b.high));
}

#[test]
fn objects_are_pictured_as_the_level_draws_them() {
    use kobo_core::level::objects::Object;
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-previews");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let level = workspace.clean_level(0x105).unwrap();
    workspace.add_level(0x105, &level).unwrap();
    let object = |number, settings| Object::Standard {
        number,
        x: 0,
        y: 0,
        settings,
    };
    // A coin, and a ground ledge 3 tiles by 2.
    let objects = [object(0x05, 0x00), object(0x14, 0x12)];
    let pictures = kobo_core::edit::object_previews(
        &workspace,
        0x105,
        &level,
        &objects,
        [0, 0, 0],
        &Operation::default(),
    )
    .unwrap();
    let sizes: Vec<_> = pictures
        .iter()
        .map(|p| p.as_ref().map(|p| (p.width, p.height)))
        .collect();
    assert_eq!(sizes, [Some((16, 16)), Some((48, 32))]);
    // Drawn, not left as the background.
    let coin = pictures[0].as_ref().unwrap();
    assert!(coin.pixels.iter().any(|&p| p != [0, 0, 0]));
}

#[test]
fn a_new_entrance_takes_a_free_number_and_builds() {
    use kobo_core::source::level::Entrance;
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-entrance");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let level = workspace.clean_level(0x105).unwrap();
    workspace.add_level(0x105, &level).unwrap();

    let id = workspace.free_entrance(0x105).unwrap();
    assert_eq!(id >> 8, 1, "in the level's half");
    assert!(level.entrances.iter().all(|e| e.id != id));
    let entrance = Entrance {
        id,
        screen: 3,
        x: 2,
        y: 9,
        action: 0,
        fg_position: 2,
        bg_position: 2,
        settings: Default::default(),
    };
    let mut document = LevelDocument::open(dir.join("levels/105.toml")).unwrap();
    let insert = Edit::InsertEntrance {
        index: document.level().entrances.len(),
        entrance,
    };
    document.apply("Add entrance", &[insert]).unwrap();
    workspace.set_level(0x105, document.level());
    assert_ne!(workspace.free_entrance(0x105), Some(id), "taken now");

    let rom = workspace.build().unwrap();
    let format = kobo_core::level::LevelFormat::of(&rom);
    let bytes = kobo_core::level::read_entrances(&rom).unwrap()[usize::from(id)];
    assert!(bytes.in_use(format));
    assert_eq!(bytes.destination(id, format), 0x105);
}

#[test]
fn sprites_are_pictured_as_the_level_draws_them() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-sprite-previews");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let level = workspace.clean_level(0x105).unwrap();
    workspace.add_level(0x105, &level).unwrap();
    // A Goomba, a Rex (the level's sprite set has it), and a generator,
    // which draws nothing.
    let started = std::time::Instant::now();
    let pictures = kobo_core::edit::sprite_previews(
        &workspace,
        0x105,
        &level,
        &[0x0F, 0xAB, 0xCB],
        &Operation::default(),
    )
    .unwrap();
    eprintln!("three sprites in {:?}", started.elapsed());
    let sizes: Vec<_> = pictures
        .iter()
        .map(|p| p.as_ref().map(|p| (p.width, p.height)))
        .collect();
    assert!(
        sizes[0].is_some_and(|(w, h)| w >= 16 && h >= 16),
        "{sizes:?}"
    );
    assert!(
        sizes[1].is_some_and(|(_, h)| h > 16),
        "a Rex is taller than a tile: {sizes:?}"
    );
    assert_eq!(sizes[2], None);
}

#[test]
fn a_level_that_does_not_build_is_left_out_of_another_ones_picture() {
    use kobo_core::level::objects::Object;
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-left-out");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let good = workspace.clean_level(0x105).unwrap();
    workspace.add_level(0x105, &good).unwrap();
    // Level 106 with an object past its last screen, which no build
    // writes.
    let mut bad = workspace.clean_level(0x106).unwrap();
    bad.layer1.push(Object::Extended {
        number: 0x41,
        x: 600,
        y: 5,
    });
    workspace
        .add_level(0x106, &kobo_core::edit::copy_of(&bad))
        .unwrap();
    workspace.set_level(0x106, &bad);
    assert!(workspace.build().is_err());

    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
    };
    let preview = workspace
        .preview(0x105, options, &Operation::default())
        .unwrap();
    assert_eq!(
        preview.left_out.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        [0x106]
    );
    // Again, left out at once: the reason is remembered.
    let again = workspace
        .preview(0x105, options, &Operation::default())
        .unwrap();
    assert_eq!(again.left_out.len(), 1);
    // Its own picture fails as it should.
    assert!(
        workspace
            .preview(0x106, options, &Operation::default())
            .is_err()
    );
}

#[test]
fn the_main_entrance_tables_place_the_player_as_the_load_does() {
    use kobo_core::entrance::MainEntranceTables;
    let Some(clean) = common::vanilla() else {
        return;
    };
    let tables = MainEntranceTables::read(&clean).unwrap();
    // Level 105: screen 0, X setting 0, Y setting 11, which the load puts at
    // (16, 352) (the editor's start marker test has the same).
    assert_eq!(tables.position(0, 0, 11, false), (16, 352));
    assert_eq!(tables.nearest(16, 352, false), (0, 0, 11));
    // X 0x75 is setting 5 (0x70), Y 0x95 setting 4 (0xA0).
    assert_eq!(tables.nearest(3 * 256 + 0x75, 0x95, false), (3, 5, 4));
}

#[test]
fn a_level_taken_out_builds_as_the_games_own() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-remove-level");
    fs::write(dir.join("kobo.toml"), "# Mine.\nformat = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let level = workspace.clean_level(0x105).unwrap();
    workspace
        .add_level(0x105, &kobo_core::edit::empty_of(&level))
        .unwrap();
    assert!(workspace.level(0x105).unwrap().layer1.is_empty());
    let path = workspace.remove_level(0x105).unwrap();
    assert!(!path.exists());
    assert_eq!(workspace.levels().count(), 0);
    let manifest = fs::read_to_string(dir.join("kobo.toml")).unwrap();
    assert!(
        manifest.starts_with("# Mine.\n") && !manifest.contains("0x105"),
        "{manifest}"
    );
    assert!(workspace.remove_level(0x105).is_err());
}
