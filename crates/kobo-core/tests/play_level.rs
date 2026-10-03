//! `expand::play_level` runs a level's frames: the game's own code in
//! play, which the contact probe and later checks rely on. Needs the
//! vanilla ROM.

mod common;

use kobo_core::{expand, ram};

#[test]
fn levels_play_frames() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    // A horizontal and a vertical level.
    for level in [0x105, 0x0D3] {
        let mut calls = Vec::new();
        let after = expand::play_level(&clean, level, 60, |frame, ram| {
            calls.push(frame);
            if frame == 0 {
                // Lift the player 16 pixels, into the air.
                ram.set_u16(ram::PLAYER_Y, ram.u16(ram::PLAYER_Y) - 0x10);
            }
        })
        .unwrap();
        assert_eq!(calls, (0..60).collect::<Vec<_>>(), "level {level:03X}");
        // The game ran: the player fell from where the first frame put
        // them, or at least moved.
        let before = expand::play_level(&clean, level, 0, |_, _| {}).unwrap();
        assert_ne!(
            after.u16(ram::PLAYER_Y),
            before.u16(ram::PLAYER_Y) - 0x10,
            "level {level:03X}"
        );
    }
}

/// `expand::play_game_loop` runs frames through the game loop and the NMI:
/// the camera follows a player walked to the right, and the NMI streams the
/// columns that come into view into layer 1's tilemap.
#[test]
fn the_game_loop_scrolls_and_streams_columns() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let mut start = None;
    let mut cameras = Vec::new();
    let mut tilemaps = Vec::new();
    expand::play_game_loop(
        &clean,
        0x105,
        90,
        |frame, ram| {
            let x = *start.get_or_insert(ram.u16(ram::PLAYER_X));
            ram.set_u16(ram::PLAYER_X, x + 3 * frame as u16);
        },
        |_, played| {
            cameras.push(played.ram.u16(ram::LAYER1_X));
            // Layer 1's tilemap, where vanilla puts it.
            tilemaps.push(played.vram[0x4000..0x6000].to_vec());
        },
    )
    .unwrap();
    assert_eq!(cameras[0], 0);
    assert!(
        cameras[89] > 0x80,
        "the camera stayed at {:04X}",
        cameras[89]
    );
    assert_ne!(tilemaps[0], tilemaps[89], "no column was streamed");
}
