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
        hidden_layers: 0,
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
        hidden_layers: 0,
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
        hidden_layers: 0,
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

#[test]
fn a_hidden_layer_is_left_out_of_the_picture() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let draw = |hidden_layers| {
        kobo_core::render::render_level(
            &clean,
            0x105,
            RenderOptions {
                sprites: Sprites::Hidden,
                player: false,
                hidden_layers,
            },
        )
        .unwrap()
    };
    let whole = draw(0);
    let without_background = draw(2);
    assert_eq!(whole.image.width, without_background.image.width);
    assert_ne!(whole.image.pixels, without_background.image.pixels);
    // The level's own tiles are the same: only the picture changes.
    assert_eq!(whole.level.tiles.low, without_background.level.tiles.low);
}

#[test]
fn the_games_backgrounds_are_listed_and_pictured() {
    use kobo_core::addr::SnesAddr;
    use kobo_core::source::level::Layer2;

    let Some(clean) = common::vanilla() else {
        return;
    };
    let backgrounds = kobo_core::level::game_backgrounds(&clean);
    assert_eq!(backgrounds.len(), 17);
    assert_eq!(backgrounds[0].0, SnesAddr::new(0x0C_D900));
    assert!(backgrounds[0].1.contains(&0x105));
    for (addr, _) in &backgrounds {
        assert!(
            kobo_core::names::game_background(*addr).is_some(),
            "{addr} has no name"
        );
    }

    let dir = TempDir::new("edit-background-previews");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    let level = workspace.clean_level(0x105).unwrap();
    workspace.add_level(0x105, &level).unwrap();
    let picture = |addr: SnesAddr| {
        kobo_core::edit::background_preview(
            &workspace,
            0x105,
            &level,
            &Layer2::VanillaBackground(addr),
            &Operation::default(),
        )
        .unwrap()
    };
    let hills = picture(backgrounds[0].0);
    let other = picture(backgrounds[1].0);
    assert_eq!((hills.width, other.width), (512, 512));
    assert_ne!(hills.pixels, other.pixels);
    // Only the copy had the other background: the level is as it was.
    assert_eq!(workspace.level(0x105), Some(&level));
}

#[test]
fn a_search_finds_every_one_of_a_kind() {
    use kobo_core::edit::find::{self, Entry};

    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-find");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let mut workspace = Workspace::open(&dir, Arc::new(clean))
        .unwrap()
        .without_cache();
    for number in [0x105, 0x106] {
        let level = workspace.clean_level(number).unwrap();
        workspace.add_level(number, &level).unwrap();
    }
    let level = workspace.level(0x105).unwrap().clone();
    let id = level.sprites.list[0].id;
    let query = find::query_for(&level, Entry::Sprite(0)).unwrap();
    assert_eq!(query, format!("sprite {id:02X}"));
    let found = find::find(&workspace, &query);
    assert!(!found.is_empty());
    for f in &found {
        let Entry::Sprite(i) = f.entry else {
            panic!("{query} found {f:?}");
        };
        assert_eq!(workspace.level(f.level).unwrap().sprites.list[i].id, id);
    }
    // One level's alone.
    let here = find::find_in(0x105, &level, &query);
    assert_eq!(
        here,
        found
            .iter()
            .filter(|f| f.level == 0x105)
            .cloned()
            .collect::<Vec<_>>()
    );
    // Every screen exit, of both levels.
    let exits = find::find(&workspace, "exit");
    assert!(exits.iter().any(|f| f.level == 0x105));
    assert!(exits.iter().all(|f| f.kind.starts_with("exit")));
}

#[test]
fn each_palette_setting_gives_its_colours() {
    use kobo_core::palette::{PaletteSetting, setting_colors};

    let Some(clean) = common::vanilla() else {
        return;
    };
    let bg = |v| setting_colors(&clean, PaletteSetting::Background, v).unwrap();
    assert_eq!(bg(0).len(), 12);
    assert_ne!(bg(0), bg(1));
    // Level 105's sky, back area colour 2.
    let back = setting_colors(&clean, PaletteSetting::BackArea, 2).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].to_rgb8(), [0x00, 0x63, 0xBD]);
}

#[test]
fn the_screens_a_level_uses_are_counted_from_its_load() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-screens");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let clean = Arc::new(clean);
    let workspace = Workspace::open(&dir, clean.clone()).unwrap();
    for number in [0x105u16, 0x106, 0x13B, 0x01E] {
        let level = workspace.clean_level(number).unwrap();
        let loaded = kobo_core::expand::expand_level(&clean, number).unwrap();
        let used = kobo_core::edit::screens_used(&loaded, &level);
        // The game's levels end where what they hold ends.
        assert_eq!(used, level.header.screens, "level {number:03X}");
    }
}

#[test]
fn sublevels_are_grouped_under_the_overworld_level_that_reaches_them() {
    use kobo_core::edit::reach::{Exits, Placeholder, Reach};
    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-reach");
    fs::write(dir.join("kobo.toml"), "format = 1\n").unwrap();
    let clean = Arc::new(clean);
    let workspace = Workspace::open(&dir, clean.clone()).unwrap();
    // The game's unused numbers share the TEST level; real levels do not.
    let placeholder = Placeholder::of(&clean).unwrap();
    let unused: Vec<u16> = (0..0x200)
        .filter(|&n| placeholder.is(&workspace.clean_level(n).unwrap()))
        .collect();
    assert_eq!(unused.len(), 277);
    assert!(unused.contains(&0x025) && !unused.contains(&0x105));
    let listed: Vec<u16> = (0..0x200).filter(|n| !unused.contains(n)).collect();
    let exits = Exits::of(&workspace);
    let reach = Reach::of(&exits, &listed);
    // The overworld levels are those its level tiles enter: 000 has a
    // translevel number, 0, which is none, and the game's code alone
    // enters it (the bonus game), as it does the credits' rooms.
    assert!(exits.overworld.contains(&0x105) && !exits.overworld.contains(&0x000));
    assert!(reach.groups.iter().all(|g| g.level != 0x000));
    assert!(reach.unreached.contains(&0x000));
    // Yoshi's Island 1's pipe leads to 1CB; the Front Door's rooms are
    // its sublevels.
    assert_eq!(reach.group_of(0x1CB), Some(0x105));
    let front_door = reach.groups.iter().find(|g| g.level == 0x10D).unwrap();
    assert!(front_door.sublevels.len() >= 10, "{front_door:?}");
    // The credits' rooms are reached by the game's code, not an exit.
    assert!(reach.unreached.contains(&0x093));
}

#[test]
fn map16_edits_build_unsaved_and_save_into_the_page_files() {
    use kobo_core::edit::{Map16Document, TileChange};
    use kobo_core::map16::{Map16Tile, Tile8Ref};
    use kobo_core::source::map16::Map16Entry;

    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-map16");
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    fs::create_dir_all(dir.join("levels")).unwrap();
    fs::write(
        dir.join("levels/105.toml"),
        level.to_toml(&Default::default()),
    )
    .unwrap();
    fs::write(
        dir.join("kobo.toml"),
        "format = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n",
    )
    .unwrap();
    let tileset = level.header.object_tileset;
    let clean = Arc::new(clean);
    let mut map16 = Map16Document::open(&dir, &clean).unwrap();
    let mut workspace = Workspace::open(&dir, clean.clone())
        .unwrap()
        .without_cache();
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
        hidden_layers: 0,
    };
    let shown = |workspace: &Workspace, tile: u16| {
        let render = workspace
            .preview(0x105, options, &Operation::default())
            .unwrap()
            .render;
        render.level.tiles.foreground_map16()[usize::from(tile)]
    };

    // A tile of page 0, the game's, and one of page 2, Lunar Magic's.
    let quarter = |n| Tile8Ref::new(n, 3, false, true, false);
    let gfx = Map16Tile {
        top_left: quarter(0x40),
        bottom_left: quarter(0x41),
        top_right: quarter(0x42),
        bottom_right: quarter(0x43),
    };
    let changes = [0x045, 0x210].map(|tile| TileChange {
        tile,
        tileset,
        entry: Map16Entry { gfx, acts: 0x130 },
    });
    map16.apply("Edit Map16 tiles", &changes).unwrap();
    workspace.set_map16(&map16);
    assert_eq!(shown(&workspace, 0x045), Some(gfx));
    assert_eq!(shown(&workspace, 0x210), Some(gfx));
    assert!(map16.is_modified());

    let written = map16.save().unwrap();
    assert_eq!(written.len(), 3, "two page files and the manifest");
    assert!(!map16.is_modified());
    let reopened = Map16Document::open(&dir, &clean).unwrap();
    assert_eq!(reopened.entry(0x045, tileset).gfx, gfx);
    assert_eq!(reopened.entry(0x210, tileset).acts, 0x130);
    let from_disk = Workspace::open(&dir, clean).unwrap().without_cache();
    assert_eq!(shown(&from_disk, 0x210), Some(gfx));
}

#[test]
fn a_graphics_file_drawn_in_builds_unsaved_and_saves_into_the_project() {
    use kobo_core::edit::{GraphicsDocument, GraphicsFile};
    use kobo_core::render::LayerTiles;

    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-graphics");
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    fs::create_dir_all(dir.join("levels")).unwrap();
    fs::write(
        dir.join("levels/105.toml"),
        level.to_toml(&Default::default()),
    )
    .unwrap();
    fs::write(
        dir.join("kobo.toml"),
        "format = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n",
    )
    .unwrap();
    let fg1 = kobo_core::gfx::object_tileset_files(&clean, level.header.object_tileset).unwrap()[0];
    let clean = Arc::new(clean);
    let mut workspace = Workspace::open(&dir, clean.clone())
        .unwrap()
        .without_cache();
    let mut graphics =
        GraphicsDocument::open(workspace.project(), &clean, GraphicsFile::Gfx(fg1)).unwrap();
    assert!(!graphics.in_project());
    // Tile 10 of the file, which FG1 loads at VRAM tile 10, in colour 1.
    let tile: Vec<(u32, u32)> = (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, 8 + y)))
        .collect();
    graphics.paint("Draw", &tile, 1).unwrap();
    workspace.set_graphics(&graphics).unwrap();
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
        hidden_layers: 0,
    };
    let vram_tile = |workspace: &Workspace| {
        let render = workspace
            .preview(0x105, options, &Operation::default())
            .unwrap()
            .render;
        *LayerTiles::from_vram(&render.level.video.vram).get(0x10)
    };
    let drawn = vram_tile(&workspace);
    assert!(
        drawn.pixels.iter().flatten().all(|&p| p & 7 == 1),
        "{drawn:?}"
    );

    let written = graphics.save(&dir).unwrap();
    assert_eq!(written.len(), 2, "the image and the manifest");
    assert!(!graphics.is_modified());
    let manifest = fs::read_to_string(dir.join("kobo.toml")).unwrap();
    assert!(
        manifest.contains(&format!("graphics/GFX{fg1:02X}.png")),
        "{manifest}"
    );
    let from_disk = Workspace::open(&dir, clean).unwrap().without_cache();
    assert_eq!(vram_tile(&from_disk), drawn);
}

#[test]
fn a_layer3_tilemap_given_to_a_level_builds_unsaved_and_saves_into_the_project() {
    use kobo_core::edit::TilemapDocument;
    use kobo_core::edit::layer3::{self, Tilemap};
    use kobo_core::map16::Tile8Ref;

    let Some(clean) = common::vanilla() else {
        return;
    };
    let dir = TempDir::new("edit-layer3");
    let (level, _) = import::read_level(&clean, 0x105).unwrap();
    fs::create_dir_all(dir.join("levels")).unwrap();
    let path = dir.join("levels/105.toml");
    fs::write(&path, level.to_toml(&Default::default())).unwrap();
    fs::write(
        dir.join("kobo.toml"),
        "format = 1\n\n[levels]\n0x105 = \"levels/105.toml\"\n",
    )
    .unwrap();
    let clean = Arc::new(clean);
    let mut workspace = Workspace::open(&dir, clean.clone())
        .unwrap()
        .without_cache();
    let mut document = LevelDocument::open(&path).unwrap();
    let file = layer3::free_file(workspace.project()).unwrap();
    assert_eq!(file, 0x80);
    let list = layer3::with_tilemap(document.level(), file);
    document
        .apply("Give layer 3 a tilemap", &[Edit::SetGraphics(Some(list))])
        .unwrap();
    let tilemap = Tilemap::of(document.level()).unwrap();
    let mut words = TilemapDocument::open(workspace.project(), file, tilemap.bytes()).unwrap();
    assert!(!words.in_project());
    // The first word that loads: under the status bar, row 5.
    let tile = Tile8Ref::new(0x12, 3, true, true, false);
    words.set("Draw", &[(tilemap.skip(), tile)]);
    workspace.set_level(0x105, document.level());
    workspace.set_tilemap(&words);
    let options = RenderOptions {
        sprites: Sprites::Hidden,
        player: false,
        hidden_layers: 0,
    };
    let vram_word = |workspace: &Workspace, at: u16| {
        let render = workspace
            .preview(0x105, options, &Operation::default())
            .unwrap()
            .render;
        let vram = &render.level.video.vram;
        let i = usize::from(at) * 2;
        u16::from_le_bytes([vram[i], vram[i + 1]])
    };
    assert_eq!(vram_word(&workspace, 0x50A0), tile.0);
    assert_eq!(vram_word(&workspace, 0x50A1), layer3::BLANK);

    document.save().unwrap();
    let written = words.save(&dir).unwrap();
    assert_eq!(written.len(), 2, "the file and the manifest");
    let from_disk = Workspace::open(&dir, clean).unwrap().without_cache();
    assert_eq!(vram_word(&from_disk, 0x50A0), tile.0);
}
