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
