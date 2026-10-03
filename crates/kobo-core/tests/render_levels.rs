//! Pixel-level regressions using synthetic tiles, plus optional vanilla
//! ROM checks for level capture and rendering.

mod common;

use kobo_core::expand::{
    GRID_LEN, LAYER2_TILEMAP_LEN, LevelTiles, LoadedLevel, SCREEN_COLS, SCREEN_ROWS,
};
use kobo_core::gfx::Tile8;
use kobo_core::level::{LevelMode, PrimaryHeader};
use kobo_core::map16::{BG_TILE_COUNT, Map16Tile, Tile8Ref};
use kobo_core::palette::{Color15, Palette};
use kobo_core::ram;
use kobo_core::render::{self, LayerTiles};
use kobo_core::video::{Layer3, LevelScene, Screen};
use std::collections::HashMap;

fn solid_tile(index: u16) -> Map16Tile {
    let r = Tile8Ref(index);
    Map16Tile {
        top_left: r,
        bottom_left: r,
        top_right: r,
        bottom_right: r,
    }
}

/// A three-screen level whose screen setup is the vanilla one (layer 2
/// on the subscreen, added to the backdrop) with a green back area
/// colour, tile 1 solid red and tile 2 solid blue.
fn scene() -> (LoadedLevel, LayerTiles, Palette) {
    let tiles = LevelTiles {
        level: 0x105,
        header: PrimaryHeader::from_bytes([0; 5]),
        level_mode: LevelMode(0),
        object_tileset: 0,
        vertical: false,
        screens: 3,
        rows: SCREEN_ROWS,
        low: vec![0; GRID_LEN],
        high: vec![0; GRID_LEN],
        map16: HashMap::from([(0, solid_tile(0)), (0x200, solid_tile(1))]),
        pipe_map16: None,
        bg_map16: vec![solid_tile(2); BG_TILE_COUNT],
        bg_map16_at: None,
        layer2_tilemap: Some((vec![0; LAYER2_TILEMAP_LEN], vec![0; LAYER2_TILEMAP_LEN])),
        layer2_screen_len: SCREEN_COLS * SCREEN_ROWS,
    };
    let loaded = LoadedLevel {
        tiles,
        video: Default::default(),
        scene: LevelScene {
            screen: Screen::vanilla(Color15::from_rgb5(0, 31, 0)),
            ..Default::default()
        },
        ram: Default::default(),
        diagnostics: vec![],
    };
    let mut gfx = LayerTiles::blank();
    gfx.tiles[1] = Tile8 {
        pixels: [[1; 8]; 8],
    };
    gfx.tiles[2] = Tile8 {
        pixels: [[2; 8]; 8],
    };
    let mut palette = Palette::default();
    palette.set(0, 1, Color15::from_rgb5(31, 0, 0));
    palette.set(0, 2, Color15::from_rgb5(0, 0, 31));
    (loaded, gfx, palette)
}

#[test]
fn overlapping_map16_numbers_keep_foreground_and_background_art_separate() {
    let (mut loaded, mut gfx, palette) = scene();
    loaded.tiles.high[0] = 2; // Foreground tile $200 over background tile $200.
    gfx.tiles[1].pixels[0][0] = 0; // Transparent pixels reveal the background.
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[1], [255, 0, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
    assert_eq!(image.pixels[512], [0, 0, 255]); // Background repeats.
    assert_eq!((image.width, image.height), (768, 432));
}

/// A level whose layer 2 is objects: tile 1 is solid blue and 2 is solid
/// red, with map16 knowing both.
fn object_scene(level_mode: LevelMode) -> (LoadedLevel, LayerTiles, Palette) {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.level_mode = level_mode;
    loaded.tiles.layer2_tilemap = None;
    loaded.tiles.bg_map16.clear();
    loaded.tiles.map16.insert(1, solid_tile(2)); // blue
    loaded.tiles.map16.insert(2, solid_tile(1)); // red
    (loaded, gfx, palette)
}

#[test]
fn layer2_objects_draw_under_layer_1_from_their_own_screens() {
    let (mut loaded, mut gfx, palette) = object_scene(LevelMode(0x01));
    let back = [0, 255, 0];
    // Screen 0 of the horizontal layer 2 buffer starts at plane offset $1B00.
    loaded.tiles.low[0x1B00] = 1;
    loaded.tiles.low[0x1B00 + SCREEN_COLS * SCREEN_ROWS + 5] = 1; // screen 1, x = 5
    loaded.tiles.low[0] = 2;
    gfx.tiles[1].pixels[0][0] = 0; // Layer 2 shows through layer 1's hole.
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[1], [255, 0, 0]);
    assert_eq!(image.pixels[16], back);
    assert_eq!(image.pixels[(16 + 5) * 16], [0, 0, 255]);
    // The same bytes are not layer 2 objects in a background tilemap mode.
    loaded.tiles.level_mode = LevelMode(0x00);
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], back);
}

#[test]
fn vertical_modes_read_layer2_objects_from_the_vertical_buffer() {
    let (mut loaded, gfx, palette) = object_scene(LevelMode(0x07));
    loaded.scene.screen.fixed_color = Color15(0);
    loaded.tiles.vertical = true;
    loaded.tiles.screens = 2;
    // Screen 1, right half, row 0, column 0: level position (16, 16).
    loaded.tiles.low[0x1C00 + 0x200 + 0x100] = 1;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!((image.width, image.height), (512, 512));
    let at = |x: usize, y: usize| image.pixels[y * 16 * 512 + x * 16];
    assert_eq!(at(16, 16), [0, 0, 255]);
    assert_eq!(at(0, 0), [0; 3]);
    // Modes 3 and 4 pair a vertical layer 1 with a horizontal layer 2.
    loaded.tiles.level_mode = LevelMode(0x03);
    loaded.tiles.low[0x1B00 + SCREEN_COLS] = 1; // row 1, column 0
    let image = render::level_image(&loaded, &gfx, &palette);
    let at = |x: usize, y: usize| image.pixels[y * 16 * 512 + x * 16];
    assert_eq!(at(16, 16), [0; 3]);
    assert_eq!(at(0, 1), [0, 0, 255]);
}

#[test]
fn object_tileset_three_moves_layer2_palettes_up_four_rows() {
    let (mut loaded, gfx, mut palette) = object_scene(LevelMode(0x02));
    loaded.tiles.object_tileset = 3;
    palette.set(4, 2, Color15::from_rgb5(0, 31, 0));
    loaded.tiles.low[0x1B00] = 1;
    loaded.tiles.low[1] = 1; // Layer 1 keeps its own rows.
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [0, 255, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
}

#[test]
fn priority_bits_interleave_layers_like_mode_1() {
    // Level mode 2 puts layer 2 on the main screen with layer 1.
    let (mut loaded, gfx, palette) = object_scene(LevelMode(0x02));
    loaded.scene.screen.main = 0x17;
    loaded.scene.screen.sub = 0x00;
    let r = Tile8Ref::new(2, 0, true, false, false);
    loaded.tiles.map16.insert(
        3,
        Map16Tile {
            top_left: r,
            bottom_left: r,
            top_right: r,
            bottom_right: r,
        },
    );
    loaded.tiles.low[0x1B00] = 3; // High-priority blue layer 2 tile...
    loaded.tiles.low[0] = 2; // ...over a low-priority red layer 1 tile.
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    loaded.tiles.low[0x1B00] = 1; // Equal priorities put layer 1 in front.
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [255, 0, 0]);
}

#[test]
fn special_modes_do_not_draw_unused_background_buffers() {
    let (mut loaded, gfx, palette) = scene();
    let back = [0, 255, 0];
    for mode in [0x09, 0x0B, 0x0F, 0x10] {
        loaded.tiles.level_mode = LevelMode(mode);
        let image = render::level_image(&loaded, &gfx, &palette);
        assert!(image.pixels.iter().all(|&p| p == back), "mode {mode:02X}");
        // Suppressing the background must still leave foreground art visible.
        loaded.tiles.high[0] = 2;
        let image = render::level_image(&loaded, &gfx, &palette);
        assert_eq!(image.pixels[0], [255, 0, 0], "mode {mode:02X}");
        assert_eq!(image.pixels[16], back, "mode {mode:02X}");
        loaded.tiles.high[0] = 0;
    }
}

#[test]
fn tall_background_uses_its_own_screen_stride() {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.layer2_screen_len = 0x200;
    loaded.tiles.bg_map16[1] = solid_tile(1);
    let (low, _) = loaded.tiles.layer2_tilemap.as_mut().unwrap();
    low[0x200] = 1;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[256], [255, 0, 0]);
    assert_eq!(image.pixels[512], [0, 0, 255]);
}

#[test]
fn layer2_is_drawn_where_the_entry_camera_put_it() {
    // Layer 2 sits 18 pixels above layer 1 (a ghost house's vertical
    // scroll) and, with the camera two screens in, half a screen behind
    // horizontally (half-speed parallax).
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.screens = 4;
    loaded.tiles.bg_map16[1] = solid_tile(1);
    let (low, _) = loaded.tiles.layer2_tilemap.as_mut().unwrap();
    low[SCREEN_COLS] = 1; // screen 0, row 1, column 0 is red
    loaded.scene.camera = [0, 192];
    loaded.scene.layer2_position = [0, 174];
    assert_eq!(loaded.scene.layer2_offset(), [0, 18]);
    let image = render::level_image(&loaded, &gfx, &palette);
    let at = |x: usize, y: usize| image.pixels[y * 1024 + x];
    assert_eq!(at(0, 33), [0, 0, 255]);
    assert_eq!(at(0, 34), [255, 0, 0]);
    assert_eq!(at(0, 49), [255, 0, 0]);
    assert_eq!(at(0, 50), [0, 0, 255]);
    assert_eq!(at(16, 40), [0, 0, 255]);
    // The top 18 pixels show the buffer's last rows wrapped around.
    assert_eq!(at(0, 0), [0, 0, 255]);
    loaded.scene.camera = [852, 192];
    loaded.scene.layer2_position = [426, 174];
    assert_eq!(loaded.scene.layer2_offset(), [426, 18]);
    let image = render::level_image(&loaded, &gfx, &palette);
    let at = |x: usize, y: usize| image.pixels[y * 1024 + x];
    assert_eq!(at(426, 40), [255, 0, 0]);
    assert_eq!(at(441, 40), [255, 0, 0]);
    assert_eq!(at(442, 40), [0, 0, 255]);
    assert_eq!(at(425, 40), [0, 0, 255]);
    assert_eq!(at(426 + 512, 40), [255, 0, 0]); // repeats every two screens
    // Layer 2 objects move with the same offset.
    let (mut loaded, gfx, palette) = object_scene(LevelMode(0x01));
    loaded.tiles.low[0x1B00] = 1;
    loaded.scene.camera = [0, 0];
    loaded.scene.layer2_position = [0, 8];
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[8 * 768], [0, 255, 0]);
    assert_eq!(image.pixels[7 * 768], [0, 0, 255]);
}

#[test]
fn background_indices_above_511_do_not_alias_lower_tiles() {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.bg_map16.resize(0x400, solid_tile(2));
    loaded.tiles.bg_map16[0x201] = solid_tile(1);
    let (lo, hi) = loaded.tiles.layer2_tilemap.as_mut().unwrap();
    lo[0] = 1;
    hi[0] = 2;
    assert_eq!(loaded.tiles.layer2_bg_tile(0, 0, 0), Some(0x401));
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [255, 0, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
}

#[test]
fn subscreen_layer2_only_shows_through_the_backdrop() {
    // Level mode 1 keeps layer 2 on the subscreen: even a high-priority
    // layer 2 tile stays behind low-priority layer 1, and where layer 1
    // is transparent the backdrop math shows layer 2 at full strength.
    let (mut loaded, gfx, palette) = object_scene(LevelMode(0x01));
    let r = Tile8Ref::new(2, 0, true, false, false);
    loaded.tiles.map16.insert(
        3,
        Map16Tile {
            top_left: r,
            bottom_left: r,
            top_right: r,
            bottom_right: r,
        },
    );
    loaded.tiles.low[0x1B00] = 3;
    loaded.tiles.low[0] = 2;
    loaded.tiles.low[0x1B00 + 1] = 3;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[0], [255, 0, 0]);
    assert_eq!(image.pixels[16], [0, 0, 255]);
    // Half-brightness modes ($0C/$0D) halve the backdrop's sum with layer
    // 2, but not the fixed colour where layer 2 is transparent.
    loaded.scene.screen.color_math = 0x70;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[16], Color15::from_rgb5(0, 0, 15).to_rgb8());
    assert_eq!(image.pixels[32], [0, 255, 0]);
}

#[test]
fn oversized_vertical_level_renders_only_the_captured_grid() {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.vertical = true;
    loaded.tiles.layer2_tilemap = None; // Only the grid matters here.
    loaded.tiles.screens = 32; // 32 * $200 exceeds the $3800-byte grid planes.
    let last = GRID_LEN - 1;
    loaded.tiles.high[last] = 2;
    let back = [0, 255, 0];
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!((image.width, image.height), (512, 7168));
    assert_eq!(image.pixels[0], back);
    assert_eq!(image.pixels.last(), Some(&[255, 0, 0]));
    assert_eq!(loaded.tiles.screens, 32); // Retain the loader's value for inspection.
}

#[test]
fn vertical_level_background_spans_the_width_and_tiles_downward() {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.vertical = true;
    loaded.tiles.screens = 4;
    loaded.tiles.rows = 16;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!((image.width, image.height), (512, 1024));
    // Background tile $200 (blue) everywhere: both background screens sit
    // side by side, and the 27 rows repeat below.
    assert_eq!(image.pixels[0], [0, 0, 255]);
    assert_eq!(image.pixels[511], [0, 0, 255]);
    assert_eq!(image.pixels[27 * 16 * 512], [0, 0, 255]);
    assert_eq!(image.pixels[1023 * 512 + 511], [0, 0, 255]);
}

#[test]
fn expanded_height_lays_screens_out_with_a_taller_stride() {
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.rows = 40;
    loaded.tiles.screens = 2;
    loaded.tiles.layer2_tilemap = None;
    loaded.tiles.high[40 * 16 + 30 * 16 + 3] = 2; // screen 1, x = 3, y = 30: tile $200 (red)
    assert_eq!(loaded.tiles.size(), (32, 40));
    assert_eq!(loaded.tiles.tile_at(19, 30), 0x200);
    assert_eq!(loaded.tiles.tile(1, 3, 30), 0x200);
    let back = [0, 255, 0];
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!((image.width, image.height), (512, 640));
    assert_eq!(image.pixels[30 * 16 * 512 + 19 * 16], [255, 0, 0]);
    assert_eq!(image.pixels[30 * 16 * 512 + 3 * 16], back);
}

#[test]
fn sprite_objects_respect_layer_priorities() {
    use kobo_core::video::{Character, DynamicObjects, SpriteObject, SpriteScene};
    let (mut loaded, gfx, palette) = scene();
    loaded.tiles.layer2_tilemap = None;
    loaded.tiles.screens = 1;
    // Layer 1: tile 1 (red) low priority at (0, 0), high priority at (1, 0).
    loaded.tiles.map16.insert(3, solid_tile(1 | 0x2000)); // priority bit set
    loaded.tiles.low[0] = 1;
    loaded.tiles.low[1] = 3;
    loaded.tiles.map16.insert(1, solid_tile(1));
    // Sprite VRAM: object character 0 is solid colour 1 (16x16 in OBSEL 3).
    let mut vram = vec![0u8; 0x10000];
    for tile in [0usize, 1, 16, 17] {
        for row in 0..8 {
            vram[0xC000 + tile * 32 + row * 2] = 0xFF;
        }
    }
    let mut pal = palette.clone();
    pal.set(8, 1, kobo_core::palette::Color15::from_rgb5(0, 31, 0));
    let scene = SpriteScene {
        objects: vec![
            SpriteObject {
                x: 0,
                y: 0,
                tile: 0,
                attr: 0x20,
                large: true,
            },
            SpriteObject {
                x: 16,
                y: 0,
                tile: 0,
                attr: 0x20,
                large: true,
            },
        ],
        object_select: 0x03,
        ..Default::default()
    };
    let mut layers = render::level_layers(&loaded, &gfx);
    render::draw_sprite_scene(&mut layers, &scene, [0, 0], &vram);
    let img = render::compose_level(&loaded, &layers, &pal);
    assert_eq!(img.pixels[0], [0, 255, 0]); // priority 2 beats low-priority layer 1
    assert_eq!(img.pixels[16], [255, 0, 0]); // but not high-priority layer 1

    // A capture that uploaded its own characters is drawn with them, over
    // the red tile; character 2 is empty in VRAM, for everyone else.
    let small = SpriteObject {
        x: 0,
        y: 0,
        tile: 2,
        attr: 0x20,
        large: false,
    };
    let uploaded = SpriteScene {
        objects: vec![small.translated(0, 8)],
        dynamic: vec![DynamicObjects {
            objects: vec![small],
            characters: vec![Character {
                address: 0xC000 + 2 * 32,
                data: vram[0xC000..0xC020].try_into().unwrap(),
            }],
        }],
        ..scene.clone()
    };
    let mut layers = render::level_layers(&loaded, &gfx);
    render::draw_sprite_scene(&mut layers, &uploaded, [0, 0], &vram);
    let img = render::compose_level(&loaded, &layers, &pal);
    assert_eq!(img.pixels[0], [0, 255, 0]);
    assert_eq!(img.pixels[8 * 256], [255, 0, 0]);

    // An object riding on layer 2 shows wherever that layer is drawn,
    // once per 256 pixels: at x = 40 + 16 - 256k with the layer 16 right.
    let riding = SpriteScene {
        objects: vec![],
        layer2_objects: vec![SpriteObject {
            x: 40,
            y: 64,
            tile: 0,
            attr: 0x20,
            large: true,
        }],
        ..scene
    };
    loaded.tiles.screens = 2;
    let mut layers = render::level_layers(&loaded, &gfx);
    render::draw_sprite_scene(&mut layers, &riding, [16, 8], &vram);
    pal.set(8, 1, kobo_core::palette::Color15::from_rgb5(31, 31, 0));
    let img = render::compose_level(&loaded, &layers, &pal);
    let at = |x: usize, y: usize| img.pixels[y * 512 + x];
    assert_eq!(at(56, 72), [255, 255, 0]);
    assert_eq!(at(56 + 256, 72), [255, 255, 0]);
    assert_ne!(at(40, 64), [255, 255, 0]);
}

#[test]
fn boss_preparation_does_not_replace_the_level_dimensions() {
    let Some(rom) = common::vanilla() else { return };
    let loaded = kobo_core::expand::expand_level(&rom, 0x1C7).unwrap();
    assert_eq!(loaded.tiles.screens, 1);
    assert_eq!(loaded.tiles.size(), (16, 27));
    // The raw dump still includes the boss routine's overwritten byte.
    assert_eq!(loaded.ram.u8(ram::SCREENS), 0xFF);
    let image = render::level_image(
        &loaded,
        &LayerTiles::from_vram(&loaded.video.vram),
        &loaded.video.palette(),
    );
    assert_eq!((image.width, image.height), (256, 224));
    assert_eq!(image.pixels[0], [0; 3]);
    assert!(image.pixels[192 * 256..].iter().any(|&p| p != [0; 3]));
}

#[test]
fn grand_poo_world_background_validation() {
    for (_, rom) in common::lunar_magic_roms() {
        if rom.sha1_hex() != "390583d5faa0cc02e0c4f414f7638228661b2dc9" {
            continue;
        }
        // An unused slot with no background table of its own: the upload
        // reads definitions from bank 0, as the game does.
        let unused = kobo_core::expand::expand_level(&rom, 0x09F).unwrap();
        assert!(unused.tiles.layer2_tilemap.is_some());
        let objects = kobo_core::expand::expand_level(&rom, 0x00E).unwrap();
        assert!(objects.tiles.layer2_tilemap.is_none());
        let background = kobo_core::expand::expand_level(&rom, 0x046).unwrap();
        assert_eq!(background.tiles.layer2_bg_rows(), 32);
        assert!(background.tiles.bg_map16.len() > 0x350);
    }
}

#[test]
fn boss_arenas_capture_mode_switches_and_object_art() {
    let Some(rom) = common::vanilla() else { return };
    for (level, starts) in [
        (0x096, vec![0, 36]),
        (0x0CC, vec![0, 45, 174]),
        (0x0D9, vec![0, 36, 174]),
        (0x1C7, vec![0]),
    ] {
        let loaded = kobo_core::expand::expand_level(&rom, level).unwrap();
        let scene = loaded.scene.boss.as_ref().unwrap();
        assert_eq!(
            scene.bands.iter().map(|b| b.start).collect::<Vec<_>>(),
            starts,
            "level {level:03X}"
        );
        assert!(scene.bands.iter().any(|b| b.layer.mode & 7 == 7));
        assert!(!scene.objects.is_empty());
        // The game's window setup: BG1 and objects masked inside window
        // 1, colour math prevented outside it.
        assert_eq!(scene.window.masks, [0x11, 0x00]);
        assert_eq!(scene.window.select, [0x02, 0x00, 0x32]);
        // Keep the frame counter from before the drawing pass.
        assert_eq!(
            loaded.ram.u8(ram::TRUE_FRAME),
            kobo_core::expand::ENTRY_FRAME_COUNTER
        );
    }
}

/// The scene plus a layer 3: a solid 2bpp tile (colour 1 of palette row 1,
/// green) at tilemap column 2, row 24, shown with the tide's scroll
/// behaviour from a camera resting at the bottom of the level.
fn layer3_scene(priority: bool) -> (LoadedLevel, LayerTiles, Palette) {
    let (mut loaded, gfx, mut palette) = scene();
    loaded.video.vram = vec![0; 0x10000];
    for row in 0..8 {
        loaded.video.vram[0x8000 + 16 + row * 2] = 0xFF;
    }
    let word: u16 = 0x0001 | (1 << 10) | if priority { 0x2000 } else { 0 };
    let at = 0xA000 + (24 * 32 + 2) * 2;
    loaded.video.vram[at..at + 2].copy_from_slice(&word.to_le_bytes());
    palette.set(0, 5, Color15::from_rgb5(0, 31, 0));
    loaded.scene.layer3 = Some(Layer3 {
        position: [0, 64],
        camera: [0, 192],
        scroll_per_16: [16, 0],
        tilemap: 0x53,
        character_base: 0x8000,
        bg_mode: 1,
    });
    loaded.scene.screen.color_math = 0x20;
    (loaded, gfx, palette)
}

#[test]
fn layer3_is_placed_where_the_entry_screen_shows_it() {
    let (mut loaded, gfx, palette) = layer3_scene(false);
    loaded.tiles.layer2_tilemap = None;
    loaded.tiles.bg_map16.clear();
    loaded.scene.screen.fixed_color = Color15(0);
    let back = [0, 0, 0];
    let image = render::level_image(&loaded, &gfx, &palette);
    let at = |x: usize, y: usize| image.pixels[y * 768 + x];
    // Tilemap row 24 sits 128 pixels below the scroll position of 64, so
    // 128 pixels below the camera's top edge at 192.
    assert_eq!(at(16, 320), [0, 255, 0]);
    assert_eq!(at(23, 327), [0, 255, 0]);
    assert_eq!(at(16, 319), back);
    assert_eq!(at(24, 320), back);
    assert_eq!(at(16 + 256, 320), back); // Scrolls with the level: no repeat.
    // A fixed layer repeats the entry screen across the level.
    loaded.scene.layer3.as_mut().unwrap().scroll_per_16 = [0, 0];
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[320 * 768 + 16 + 256], [0, 255, 0]);
}

#[test]
fn layer3_stacks_by_priority_and_blends_with_the_subscreen() {
    let (blue, green, red) = ([0, 0, 255], [0, 255, 0], [255, 0, 0]);
    let at = 320 * 768 + 16;
    // Layer 2 (blue) is on the subscreen, so layer 3 covers it...
    let (loaded, gfx, palette) = layer3_scene(false);
    assert_eq!(
        render::level_image(&loaded, &gfx, &palette).pixels[at],
        green
    );
    assert_eq!(
        render::level_image(&loaded, &gfx, &palette).pixels[at + 8],
        blue
    );
    // ...unless colour math adds the two, as the vanilla setting does.
    let mut loaded = loaded;
    loaded.scene.screen.color_math = 0x24;
    assert_eq!(
        render::level_image(&loaded, &gfx, &palette).pixels[at],
        [0, 255, 255]
    );
    // Low-priority layer 1 (red, tile position (1, 20)) covers low-priority layer 3.
    loaded.tiles.high[20 * 16 + 1] = 2;
    assert_eq!(render::level_image(&loaded, &gfx, &palette).pixels[at], red);
    // A high-priority layer 3 tile stays behind it until the BG3 priority
    // bit lifts it in front of everything.
    let (mut loaded, gfx, palette) = layer3_scene(true);
    loaded.tiles.high[20 * 16 + 1] = 2;
    assert_eq!(render::level_image(&loaded, &gfx, &palette).pixels[at], red);
    loaded.scene.layer3.as_mut().unwrap().bg_mode = 0x09;
    assert_eq!(
        render::level_image(&loaded, &gfx, &palette).pixels[at],
        green
    );
    // Level mode $0E puts everything else on the subscreen and adds it to
    // layer 3, so the layer 1 tile shows through the layer 3 pixel.
    let (mut loaded, gfx, palette) = layer3_scene(false);
    loaded.tiles.high[20 * 16 + 1] = 2;
    loaded.scene.screen.main = 0x04;
    loaded.scene.screen.sub = 0x13;
    loaded.scene.screen.color_math = 0x24;
    let image = render::level_image(&loaded, &gfx, &palette);
    assert_eq!(image.pixels[at], [255, 255, 0]);
    assert_eq!(image.pixels[at + 8], red);
    assert_eq!(image.pixels[at + 16], blue);
}

/// The one-call entry point draws sprites and the player unless told not
/// to, and marks every sprite entry when asked for markers.
#[test]
fn render_level_honours_its_options() {
    use render::{RenderOptions, Sprites, render_level};
    let Some(rom) = common::vanilla() else { return };
    let full = render_level(&rom, 0x105, RenderOptions::default()).unwrap();
    let operation = kobo_core::operation::Operation::default();
    let controlled =
        render::render_level_with_control(&rom, 0x105, RenderOptions::default(), &operation)
            .unwrap();
    assert_eq!(controlled.image, full.image);
    assert_eq!(
        operation.progress().stage,
        kobo_core::operation::Stage::Finished
    );
    assert!(operation.progress().instructions > 0);
    assert!(full.diagnostics.is_empty());
    let (w, h) = full.level.tiles.size();
    assert_eq!(
        (full.image.width, full.image.height),
        (w as u32 * 16, h as u32 * 16)
    );
    let with = |sprites, player| {
        render_level(&rom, 0x105, RenderOptions { sprites, player })
            .unwrap()
            .image
    };
    let bare = with(Sprites::Hidden, false);
    assert_ne!(with(Sprites::Hidden, true), bare, "the player");
    assert_ne!(with(Sprites::Drawn, false), bare, "the sprites");
    assert_ne!(with(Sprites::Markers, false), bare, "the markers");
    assert_ne!(with(Sprites::Markers, false), with(Sprites::Drawn, false));
    assert_eq!(with(Sprites::Drawn, true), full.image);
    // A boss arena is one fixed screen whatever the options.
    let arena = render_level(&rom, 0x1C7, RenderOptions::default()).unwrap();
    assert_eq!((arena.image.width, arena.image.height), (256, 224));
}

/// The dragon coin's highlight is animated by the NMI, even when the
/// caller hides Mario and every sprite. Its loaded palette value is magenta.
#[test]
fn dragon_coins_use_the_roms_animated_palette() {
    use kobo_core::addr::SnesAddr;
    let Some(rom) = common::vanilla() else { return };
    let options = render::RenderOptions {
        sprites: render::Sprites::Hidden,
        player: false,
    };
    let rendered = render::render_level(&rom, 0x105, options).unwrap();
    assert!(rendered.diagnostics.is_empty());
    let yellow = rendered.level.video.palette().get(6, 4);
    let animation: Vec<_> = rom
        .read(SnesAddr::new(0x00B60C), 16)
        .unwrap()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| Color15(u16::from_le_bytes(b)))
        .collect();
    assert!(
        animation.contains(&yellow),
        "highlight {yellow:?} is not a flashing yellow"
    );
    // First dragon coin in Yoshi's Island 1: both halves must contain
    // the animated highlight, and neither may retain the loaded magenta.
    let width = rendered.image.width;
    for top in [256, 272] {
        let pixels: Vec<_> = (top..top + 16)
            .flat_map(|y| (272..288).map(move |x| (y * width + x) as usize))
            .map(|i| rendered.image.pixels[i])
            .collect();
        assert!(pixels.contains(&yellow.to_rgb8()));
        assert!(!pixels.contains(&Color15(0x7C3F).to_rgb8()));
    }

    // A patch may replace the animation colours. Follow its ROM code,
    // rather than substituting a hard-coded yellow in the renderer.
    let mut data = rom.data().to_vec();
    let start = rom.pc(SnesAddr::new(0x00B60C)).unwrap().as_usize();
    let custom = Color15::from_rgb5(3, 17, 9);
    for bytes in data[start..start + 16].as_chunks_mut::<2>().0 {
        bytes.copy_from_slice(&custom.0.to_le_bytes());
    }
    let patched = kobo_core::Rom::from_bytes(data).unwrap();
    let rendered = render::render_level(&patched, 0x105, options).unwrap();
    assert!(rendered.diagnostics.is_empty());
    assert_eq!(rendered.level.video.palette().get(6, 4), custom);
}

#[test]
fn cancellation_and_sprite_budget_exhaustion_are_fatal() {
    use kobo_core::operation::{Operation, OperationError, Stage};
    use render::{RenderError, RenderOptions, render_level_with_control};
    // Cancellation is checked before even trying to parse level pointers.
    let mut data = vec![0; 0x8000];
    data[0x7FD5] = 0x20;
    let fake = kobo_core::Rom::from_bytes(data).unwrap();
    let cancelled = Operation::default();
    cancelled.cancel();
    assert!(render_level_with_control(&fake, 0x105, RenderOptions::default(), &cancelled).is_err());

    let Some(rom) = common::vanilla() else { return };
    let load = Operation::default();
    kobo_core::expand::expand_level_with_control(&rom, 0x105, &load).unwrap();
    // Enough to finish loading, but not one instruction of sprite capture.
    let budget = load.progress().instructions;
    let operation = Operation::new(Some(budget));
    assert!(
        matches!(render_level_with_control(&rom, 0x105, RenderOptions::default(), &operation),
        Err(RenderError::Expand(kobo_core::expand::ExpandError::Operation(OperationError::Budget { limit }))) if limit == budget)
    );
    assert_ne!(operation.progress().stage, Stage::Finished);
}

#[test]
fn unsupported_hardware_reaches_successful_render_diagnostics() {
    use kobo_core::{Rom, SnesAddr, cpu::access::AccessKind, expand::Diagnostic};
    let Some(rom) = common::vanilla() else { return };
    // Replace the NMI with a synthetic unsupported read and a valid return.
    // Work on an in-memory copy; no original ROM bytes are changed on disk.
    let nmi = SnesAddr::new(rom.read_u16(SnesAddr::new(0x00FFEA)).unwrap() as u32);
    let offset = rom.pc(nmi).unwrap().as_usize();
    let mut data = rom.data().to_vec();
    data[offset..offset + 4].copy_from_slice(&[0xAD, 0x37, 0x21, 0x40]);
    let modified = Rom::from_bytes(data).unwrap();
    let rendered = render::render_level(&modified, 0x105, Default::default()).unwrap();
    let reports: Vec<_> = rendered
        .diagnostics
        .iter()
        .filter_map(|d| match d {
            Diagnostic::Unsupported(report) => Some(report),
            _ => None,
        })
        .collect();
    // Both loading (player frames) and sprite capture run the handler.
    assert_eq!(reports.len(), 2);
    for report in reports {
        assert!(
            report
                .accesses
                .iter()
                .any(|a| a.address == 0x2137 && a.kind == AccessKind::Read && a.count > 0)
        );
    }
}
