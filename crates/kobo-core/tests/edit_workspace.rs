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
