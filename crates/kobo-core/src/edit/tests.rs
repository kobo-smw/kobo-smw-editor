use super::*;
use crate::level::objects::Object;
use crate::source::level::Sprite;

const LEVEL: &str = r#"[header]
screens = 2
mode = 0x00  # Horizontal, background
tileset = 0x0  # Normal 1
sprite_tileset = 0x0  # Forest
music = 0  # Overworld
time = 2
bg_palette = 0
fg_palette = 0
sprite_palette = 0
back_area = 0
item_memory = 0
vertical_scroll = 0
layer3_priority = false

[entrance]
screen = 0
x = 0
y = 0
action = 0
midway_screen = 0
fg_position = 0
bg_position = 0
layer2_scroll = 0
layer3 = 0
no_yoshi_intro = false
vertical_position = false

[layer1]
objects = [
    # The floor.
    { obj = 0x21, x = 0, y = 24, length = 31 },  # Long ground ledge
    { obj = 0x05, x = 3, y = 20, height = 1, width = 5 },  # Coin
    # Over the gap.
    { ext = 0x2D, x = 10, y = 18 },  # Turn block, coin
]

[layer2]
background = 0x0CD900

[sprites]
memory = 0
list = [
    { id = 0x0F, x = 20, y = 23 },  # Goomba
]
"#;

fn document() -> LevelDocument {
    LevelDocument::from_text("105.toml", LEVEL.to_string()).unwrap()
}

fn object(document: &LevelDocument, index: usize) -> &Object {
    &document.level().layer1[index]
}

fn comment_before(document: &LevelDocument, index: usize) -> &[String] {
    document
        .comments()
        .before(&crate::source::entry_place("layer1.objects", index))
}

#[test]
fn opening_changes_nothing() {
    let document = document();
    assert!(!document.is_modified());
    assert_eq!(document.text(), LEVEL);
}

#[test]
fn a_file_in_another_layout_is_not_modified() {
    let text = LEVEL.replace("screens = 2", "screens   =   2");
    let document = LevelDocument::from_text("105.toml", text).unwrap();
    assert!(!document.is_modified());
}

#[test]
fn moving_an_object_is_one_undo_step() {
    let mut document = document();
    let moved = object_at(object(&document, 1), 5, 21);
    let edit = Edit::ReplaceObject {
        layer: ObjectLayer::One,
        index: 1,
        object: moved.clone(),
    };
    document.apply("Move object", &[edit]).unwrap();
    assert_eq!(object(&document, 1), &moved);
    assert!(document.is_modified());
    assert!(document.text().contains("{ obj = 0x05, x = 5, y = 21,"));
    assert_eq!(document.undo_label(), Some("Move object"));

    assert!(document.undo());
    assert_eq!(document.text(), LEVEL);
    assert!(!document.is_modified());
    assert_eq!(document.redo_label(), Some("Move object"));
    assert!(document.redo());
    assert_eq!(object(&document, 1), &moved);
    assert!(!document.redo());
}

#[test]
fn a_failed_edit_changes_nothing() {
    let mut document = document();
    let inside = object_at(object(&document, 1), 6, 6);
    let outside = object_at(object(&document, 1), 32, 6);
    let edits = [
        Edit::ReplaceObject {
            layer: ObjectLayer::One,
            index: 1,
            object: inside,
        },
        Edit::ReplaceObject {
            layer: ObjectLayer::One,
            index: 1,
            object: outside,
        },
    ];
    let error = document.apply("Move", &edits).unwrap_err();
    assert!(matches!(
        error,
        EditError::Outside {
            x: 32,
            width: 32,
            height: 27,
            ..
        }
    ));
    assert_eq!(document.text(), LEVEL);
    assert_eq!(document.undo_label(), None);
}

#[test]
fn an_entry_past_the_edge_comes_back_but_goes_no_further() {
    // Vanilla 108 has ledges below its last screen; this one is past the
    // right edge, at column 40 of 32.
    let text = LEVEL.replace("x = 10, y = 18 }", "x = 40, y = 18 }");
    let mut document = LevelDocument::from_text("105.toml", text).unwrap();
    let block = object(&document, 2).clone();
    let place = |x, y| Edit::ReplaceObject {
        layer: ObjectLayer::One,
        index: 2,
        object: object_at(&block, x, y),
    };
    let further = [place(41, 18)];
    assert!(matches!(
        document.apply("Move", &further),
        Err(EditError::Outside { x: 41, .. })
    ));
    assert_eq!(
        past_edge(document.level()),
        [find::Entry::Object(ObjectLayer::One, 2)]
    );
    for (x, y) in [(40, 18), (39, 20), (12, 18)] {
        let edit = [place(x, y)];
        document.apply("Move", &edit).unwrap();
    }
    assert!(past_edge(document.level()).is_empty());
    // Back inside, it is held to the level like any other.
    let out_again = [place(33, 18)];
    assert!(document.apply("Move", &out_again).is_err());
}

#[test]
fn comments_follow_their_entries() {
    let mut document = document();
    let coins = object(&document, 2).clone();
    let floor = object(&document, 0).clone();

    let insert = Edit::InsertObject {
        layer: ObjectLayer::One,
        index: 1,
        object: Object::Extended {
            number: 0x41,
            x: 1,
            y: 1,
        },
    };
    document.apply("Add", &[insert]).unwrap();
    assert_eq!(comment_before(&document, 0), ["# The floor."]);
    assert_eq!(comment_before(&document, 1), [] as [String; 0]);
    assert_eq!(object(&document, 3), &coins);
    assert_eq!(comment_before(&document, 3), ["# Over the gap."]);

    let reorder = Edit::ReorderObject {
        layer: ObjectLayer::One,
        from: 0,
        to: 3,
    };
    document.apply("Reorder", &[reorder]).unwrap();
    assert_eq!(object(&document, 3), &floor);
    assert_eq!(comment_before(&document, 3), ["# The floor."]);
    assert_eq!(object(&document, 2), &coins);
    assert_eq!(comment_before(&document, 2), ["# Over the gap."]);
}

#[test]
fn a_removed_entrys_comments_stay_where_it_was() {
    let mut document = document();
    let remove = Edit::RemoveObject {
        layer: ObjectLayer::One,
        index: 0,
    };
    document.apply("Delete", &[remove]).unwrap();
    assert_eq!(comment_before(&document, 0), ["# The floor."]);
    assert_eq!(comment_before(&document, 1), ["# Over the gap."]);

    let remove_last = Edit::RemoveObject {
        layer: ObjectLayer::One,
        index: 1,
    };
    document.apply("Delete", &[remove_last]).unwrap();
    assert_eq!(
        document.comments().before("layer1.objects[]"),
        ["# Over the gap."]
    );
    assert!(document.text().contains("    # Over the gap.\n]"));
}

#[test]
fn sprites_are_edited_like_objects() {
    let mut document = document();
    let sprite = Sprite {
        id: 0xAB,
        x: 25,
        y: 22,
        extra_bits: 0,
        extension: Vec::new(),
    };
    let edits = [
        Edit::InsertSprite {
            index: 1,
            sprite: sprite.clone(),
        },
        Edit::RemoveSprite { index: 0 },
    ];
    document.apply("Sprites", &edits).unwrap();
    assert_eq!(document.level().sprites.list, [sprite]);
    let past_end = Edit::RemoveSprite { index: 1 };
    assert!(matches!(
        document.apply("Delete", &[past_end]),
        Err(EditError::NoEntry {
            index: 1,
            len: 1,
            ..
        })
    ));
}

#[test]
fn layer_2_objects_need_a_mode_with_them() {
    let mut document = document();
    let insert = Edit::InsertObject {
        layer: ObjectLayer::Two,
        index: 0,
        object: Object::Extended {
            number: 0x41,
            x: 1,
            y: 1,
        },
    };
    assert!(matches!(
        document.apply("Add", &[insert]),
        Err(EditError::NoLayer2)
    ));
}

#[test]
fn source_text_is_an_undo_step_and_must_parse() {
    let mut document = document();
    let text = LEVEL.replace("x = 3, y = 20", "x = 4, y = 20");
    document.set_text("Edit source", &text).unwrap();
    assert_eq!(object_position(object(&document, 1)), Some((4, 20)));
    assert!(document.set_text("Edit source", "[header").is_err());
    assert_eq!(object_position(object(&document, 1)), Some((4, 20)));
    assert!(document.undo());
    assert_eq!(document.text(), LEVEL);
}

#[test]
fn reloading_follows_the_file_unless_edited() {
    let dir = std::env::temp_dir().join(format!("kobo-test-edit-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("105.toml");
    std::fs::write(&path, LEVEL).unwrap();
    let mut document = LevelDocument::open(&path).unwrap();
    assert_eq!(document.reload().unwrap(), Reload::Unchanged);

    let outside = LEVEL.replace("x = 10, y = 18", "x = 11, y = 18");
    std::fs::write(&path, &outside).unwrap();
    assert_eq!(document.reload().unwrap(), Reload::Reloaded);
    assert_eq!(object_position(object(&document, 2)), Some((11, 18)));
    assert!(!document.is_modified());

    let edit = Edit::RemoveSprite { index: 0 };
    document.apply("Delete sprite", &[edit]).unwrap();
    let again = outside.replace("x = 11, y = 18", "x = 12, y = 18");
    std::fs::write(&path, &again).unwrap();
    let Reload::Conflict(text) = document.reload().unwrap() else {
        panic!("an edited document should not take the file's change");
    };
    assert_eq!(text, again);
    document.keep_over(text);
    assert!(document.is_modified());
    document.save().unwrap();
    assert!(!document.is_modified());
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("x = 11, y = 18") && !saved.contains("id = 0x0F"));
    assert_eq!(document.reload().unwrap(), Reload::Unchanged);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn objects_without_a_place_keep_none() {
    let exit = Object::Unplaced(vec![0x00, 0x60, 0x00]);
    assert_eq!(object_position(&exit), None);
    assert_eq!(object_at(&exit, 3, 3), exit);
}

#[test]
fn entries_are_found_by_line() {
    let document = document();
    let line = |list, index| entry_line(document.text(), list, index);
    let text: Vec<&str> = document.text().lines().collect();
    assert!(text[line("layer1.objects", 0).unwrap()].contains("obj = 0x21"));
    assert!(text[line("layer1.objects", 2).unwrap()].contains("ext = 0x2D"));
    assert_eq!(line("layer1.objects", 3), None);
    assert!(text[line(SPRITE_LIST, 0).unwrap()].contains("id = 0x0F"));
}

#[test]
fn setting_fields_read_and_write_the_byte() {
    // Coin 05: height and width, each one less than the tiles.
    let fields = setting_fields(0x05, 0x04);
    assert_eq!(
        fields.iter().map(|f| (f.name, f.value)).collect::<Vec<_>>(),
        [("height", 1), ("width", 5)]
    );
    assert_eq!(with_setting(0x05, 0x04, "height", 3), 0x24);
    assert_eq!(with_setting(0x05, 0x04, "width", 40), 0x0F);
    // Ledge edge 13: height and a type in the low nibble.
    assert_eq!(with_setting(0x13, 0x3B, "type", 13), 0x3D);
    // Vertical pipe end 10: a type in the high nibble and a width.
    assert_eq!(with_setting(0x10, 0x21, "type", 5), 0x51);
    // Long ground ledge 21: a length in the whole byte.
    assert_eq!(setting_fields(0x21, 0xBF)[0].value, 192);
    assert_eq!(with_setting(0x21, 0xBF, "length", 256), 0xFF);
    // A field the object has not leaves the byte alone.
    assert_eq!(with_setting(0x21, 0xBF, "width", 3), 0xBF);
}

#[test]
fn amending_keeps_one_undo_step() {
    let mut document = document();
    let at = |x| Edit::ReplaceObject {
        layer: ObjectLayer::One,
        index: 1,
        object: object_at(object(&document, 1), x, 20),
    };
    let first = at(4);
    document.apply("Move object", &[first]).unwrap();
    for x in 5..9 {
        let edit = Edit::ReplaceObject {
            layer: ObjectLayer::One,
            index: 1,
            object: object_at(object(&document, 1), x, 20),
        };
        document.amend(&[edit]).unwrap();
    }
    assert_eq!(object_position(object(&document, 1)), Some((8, 20)));
    assert!(document.undo());
    assert_eq!(document.text(), LEVEL);
    assert!(!document.undo());
}

fn sprite(id: u8, x: u16, y: u16) -> Sprite {
    Sprite {
        id,
        x,
        y,
        extra_bits: 0,
        extension: Vec::new(),
    }
}

/// A two-screen level with a sprite on each screen, in the loader's order.
fn sprites_document() -> LevelDocument {
    let text = LEVEL.replace(
        "    { id = 0x0F, x = 20, y = 23 },  # Goomba\n",
        "    { id = 0x0F, x = 5, y = 23 },  # Goomba\n    # The second.\n    { id = 0x0F, x = 20, y = 23 },  # Goomba\n",
    );
    LevelDocument::from_text("105.toml", text).unwrap()
}

#[test]
fn a_new_sprite_goes_where_the_loader_reaches_it() {
    let document = sprites_document();
    let (edit, index) = insert_sprite(document.level(), sprite(0xAB, 10, 20));
    assert_eq!(index, 1, "after screen 0's sprite, before screen 1's");
    assert_eq!(
        edit,
        Edit::InsertSprite {
            index: 1,
            sprite: sprite(0xAB, 10, 20)
        }
    );
    let (_, index) = insert_sprite(document.level(), sprite(0xAB, 31, 20));
    assert_eq!(index, 2);
    let (_, index) = insert_sprite(document.level(), sprite(0xAB, 0, 20));
    assert_eq!(index, 1, "after the others on its screen");
}

#[test]
fn a_sprite_moved_to_another_screen_moves_in_the_list() {
    let mut document = sprites_document();
    // The first sprite to screen 1, past the second.
    let moves = [(0, sprite(0x0F, 25, 23))];
    let (edits, at) = move_sprites(document.level(), &moves).unwrap();
    assert_eq!(at, [1]);
    document.apply("Move sprite", &edits).unwrap();
    let list = &document.level().sprites.list;
    assert_eq!((list[0].x, list[1].x), (20, 25));
    // The comment before the second sprite stayed with it.
    assert_eq!(
        document.comments().before("sprites.list[0]"),
        ["# The second."]
    );

    // Moving both together keeps the order and says where each went.
    let moves = [(0, sprite(0x0F, 3, 23)), (1, sprite(0x0F, 2, 23))];
    let (edits, at) = move_sprites(document.level(), &moves).unwrap();
    document.apply("Move sprites", &edits).unwrap();
    let list = &document.level().sprites.list;
    assert_eq!(at.iter().map(|&i| list[i].x).collect::<Vec<_>>(), [3, 2]);
    // Neither had to move in the list to keep it in screen order.
    assert_eq!(edits.len(), 2);
}

#[test]
fn an_exit_keeps_the_games_format_while_it_can() {
    use crate::level::objects::ScreenExit;
    // Level 105's exit to level 1CB, in the game's format: bit 8 is the
    // level's.
    let exit = ScreenExit {
        screen: 7,
        flags: 0,
        destination: 0xCB,
    };
    let target = ExitTarget::of(exit, 0x105);
    assert_eq!(target.destination, 0x1CB);
    assert_eq!(target.exit(0x105, false), exit);
    // To level 0CB, in the other bank: only Lunar Magic's format says it.
    let other = ExitTarget {
        destination: 0x0CB,
        ..target
    };
    let lunar = other.exit(0x105, false);
    assert_ne!(lunar.flags & ScreenExit::LUNAR_MAGIC, 0);
    assert_eq!(ExitTarget::of(lunar, 0x105), other);
    // One already in Lunar Magic's format stays so.
    let kept = target.exit(0x105, true);
    assert_ne!(kept.flags & ScreenExit::LUNAR_MAGIC, 0);
    assert_eq!(ExitTarget::of(kept, 0x105), target);
}

#[test]
fn every_change_is_found_and_taken_back_one_at_a_time() {
    use super::diff::{self, Part};
    let old = document().level().clone();
    let mut document = document();
    let coin = Object::Extended {
        number: 0x41,
        x: 2,
        y: 2,
    };
    let moved = object_at(object(&document, 1), 7, 20);
    let edits = [
        Edit::ReplaceObject {
            layer: ObjectLayer::One,
            index: 1,
            object: moved,
        },
        Edit::RemoveObject {
            layer: ObjectLayer::One,
            index: 2,
        },
        Edit::InsertObject {
            layer: ObjectLayer::One,
            index: 0,
            object: coin,
        },
        Edit::InsertSprite {
            index: 1,
            sprite: sprite(0xAB, 25, 22),
        },
    ];
    document.apply("Edits", &edits).unwrap();
    let found = diff::diff(&old, document.level());
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(!found.header);

    // Each change back, diffing again after each.
    for _ in 0..8 {
        let found = diff::diff(&old, document.level());
        let next = found
            .layer1
            .first()
            .map(|&c| (Part::Objects(ObjectLayer::One), c))
            .or_else(|| found.sprites.first().map(|&c| (Part::Sprites, c)));
        let Some((part, change)) = next else { break };
        let edits = diff::revert(&old, document.level(), part, change);
        document.apply("Revert", &edits).unwrap();
    }
    assert_eq!(document.level(), &old);

    // An object moved in the list is changed, and goes back.
    let reorder = Edit::ReorderObject {
        layer: ObjectLayer::One,
        from: 0,
        to: 2,
    };
    document.apply("Reorder", &[reorder]).unwrap();
    for _ in 0..4 {
        let found = diff::diff(&old, document.level());
        let Some(&change) = found.layer1.first() else {
            break;
        };
        let edits = diff::revert(
            &old,
            document.level(),
            Part::Objects(ObjectLayer::One),
            change,
        );
        document.apply("Revert", &edits).unwrap();
    }
    assert_eq!(document.level(), &old);
}

#[test]
fn direct_map16_objects_say_their_tile_and_size() {
    let low = map16_object(0x1F3, 4, 5);
    assert!(matches!(low, Object::Lunar { number: 0x27, .. }));
    assert_eq!(map16_object_parts(&low), Some((0x1F3, 1, 1)));
    let high = map16_object(0x4512, 0, 0);
    assert!(matches!(high, Object::Lunar { number: 0x29, .. }));
    assert_eq!(map16_object_parts(&high), Some((0x4512, 1, 1)));
    let big = map16_object_sized(&low, 3, 2);
    assert_eq!(map16_object_parts(&big), Some((0x1F3, 3, 2)));
    // Object 23: a tile of page 1, its low byte in the fourth byte.
    let page1 = Object::Lunar {
        number: 0x23,
        x: 0,
        y: 0,
        data: vec![0x10, 0x30],
    };
    assert_eq!(map16_object_parts(&page1), Some((0x130, 1, 2)));
    // Encoded and decoded as any object is.
    let bytes = crate::level::objects::encode(
        [0; 5],
        std::slice::from_ref(&low),
        crate::level::objects::Layout::Horizontal,
        crate::level::objects::Jumps::Vanilla,
    )
    .unwrap();
    let back = crate::level::objects::decode(
        &bytes,
        crate::level::objects::Layout::Horizontal,
        crate::level::objects::Jumps::Vanilla,
    )
    .unwrap();
    assert_eq!(back.objects, [low]);
}

#[test]
fn sprites_past_a_later_screens_are_found_and_sorted() {
    let mut document = sprites_document();
    // Screen 1's sprite first, then screen 0's: the loader never reaches
    // the second.
    let reorder = Edit::ReorderSprite { from: 1, to: 0 };
    document.apply("Reorder", &[reorder]).unwrap();
    assert_eq!(unreached_sprites(document.level()), [1]);
    let edits = sort_sprites(document.level());
    document.apply("Sort", &edits).unwrap();
    assert!(unreached_sprites(document.level()).is_empty());
    let list = &document.level().sprites.list;
    assert_eq!((list[0].x, list[1].x), (5, 20));
    // The comment before screen 1's sprite went with it.
    assert_eq!(
        document.comments().before("sprites.list[1]"),
        ["# The second."]
    );
}

#[test]
fn an_animation_list_a_build_refuses_is_refused() {
    use crate::exanimation::{List, Slot};

    let mut document = document();
    let mut list = List::default();
    list.slots.insert(
        0,
        Slot {
            kind: 0x01,
            trigger: 0x00,
            frames_less_one: 1,
            dest: 0x2000,
            frames: vec![0xAD00, 0xAD20],
        },
    );
    list.count = 1;
    let good = Edit::SetAnimation {
        settings: Some(0x40),
        list: Some(Box::new(list.clone())),
    };
    document.apply("Animate", &[good]).unwrap();
    assert_eq!(document.level().animation.as_ref(), Some(&list));
    assert_eq!(document.level().animation_settings, Some(0x40));
    // Three frames where its type takes two: a build would refuse it.
    list.slots.get_mut(&0).unwrap().frames.push(0xAD40);
    let bad = Edit::SetAnimation {
        settings: None,
        list: Some(Box::new(list)),
    };
    assert!(matches!(
        document.apply("Animate", &[bad]),
        Err(EditError::Animation(_))
    ));
}
