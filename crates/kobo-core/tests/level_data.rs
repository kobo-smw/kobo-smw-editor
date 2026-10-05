//! Level object data, sprite lists, and background tilemaps must decode
//! and encode again to the same objects, sprites, and tiles, on every
//! level of the vanilla ROM and of the hacks in `KOBO_LM_ROMS`. Where Lunar Magic stored the data in a RATS
//! block, the decoded length must fit the block. ROMs locked by their
//! authors are skipped: they add objects past the level's last screen
//! that no screen jump can reach again.

mod common;

use common::failures::{Failures, catch};

use kobo_core::compress::rle1;
use kobo_core::import;
use kobo_core::level::objects::{self, Layout, ObjectData};
use kobo_core::level::{self, Layer2, LevelFormat};
use kobo_core::source::level::{Comments, Level};
use kobo_core::{Rom, SnesAddr, gfx, rats, sprites};

#[derive(Default, Debug)]
struct Counts {
    /// Object lists and sprite lists checked.
    lists: usize,
    /// Of those, the ones that came out byte for byte as stored.
    identical: usize,
    /// Distinct background tilemaps checked.
    backgrounds: usize,
}

fn rats_len(rom: &Rom, addr: SnesAddr) -> Option<usize> {
    let pc = rom.pc(addr).ok()?.as_usize();
    rats::tag_at(rom.data(), pc.checked_sub(rats::TAG_LEN)?)
}

fn check_objects(
    name: &str,
    rom: &Rom,
    what: &str,
    addr: SnesAddr,
    data: &ObjectData,
    layout: Layout,
    counts: &mut Counts,
) {
    let jumps = LevelFormat::of(rom).jumps;
    let bytes = objects::encode(data.header, &data.objects, layout, jumps)
        .unwrap_or_else(|e| panic!("{name} {what}: {e}"));
    let again = objects::decode(&bytes, layout, jumps).unwrap();
    assert_eq!(again.objects, data.objects, "{name} {what}");
    assert_eq!(again.len, bytes.len(), "{name} {what}");
    // Kobo never writes more than the data needs: it only drops screen
    // jumps that change nothing, or merges one with a new-screen bit.
    assert!(bytes.len() <= data.len, "{name} {what}");
    if let Some(len) = rats_len(rom, addr) {
        assert!(
            data.len <= len,
            "{name} {what}: {} bytes in a {len}-byte block",
            data.len
        );
    }
    counts.lists += 1;
    counts.identical += (bytes[..] == rom.read(addr, data.len).unwrap()[..]) as usize;
}

/// Every level of `rom`, each level's failure recorded in `failures`, or
/// on the first, with none, a panic.
fn check_rom(name: &str, rom: &Rom, failures: Option<&Failures>) -> Counts {
    let mut counts = Counts::default();
    let mut backgrounds = std::collections::HashSet::new();
    for n in 0..level::LEVEL_COUNT {
        let Some(failures) = failures else {
            check_level(name, rom, n, &mut counts, &mut backgrounds);
            continue;
        };
        if let Err(e) = catch(|| check_level(name, rom, n, &mut counts, &mut backgrounds)) {
            failures.fail(rom, Some(n), e);
        }
    }
    counts
}

fn check_level(
    name: &str,
    rom: &Rom,
    n: u16,
    counts: &mut Counts,
    backgrounds: &mut std::collections::HashSet<SnesAddr>,
) {
    let sizes = sprites::pixi_size_table(rom).unwrap();
    {
        let what = |layer: &str| format!("level {n:03X} {layer}");
        let objects =
            level::read_objects(rom, n).unwrap_or_else(|e| panic!("{name} level {n:03X}: {e}"));
        let mode = objects.header().level_mode;
        let vertical = |v| {
            if v {
                Layout::Vertical
            } else {
                Layout::Horizontal
            }
        };
        let layer1 = level::layer1_ptr(rom, n).unwrap();
        let layout1 = vertical(mode.layer1_vertical());
        check_objects(
            name,
            rom,
            &what("layer 1"),
            layer1,
            &objects.layer1,
            layout1,
            counts,
        );
        if let Layer2::Objects(data) = &objects.layer2 {
            let (level::Layer2Data::Objects(addr) | level::Layer2Data::Tilemap(addr)) =
                level::layer2_ptr(rom, n).unwrap();
            let layout = vertical(mode.layer2() == level::Layer2Kind::VerticalObjects);
            check_objects(name, rom, &what("layer 2"), addr, data, layout, counts);
        }

        if let Some(bg) = level::read_background(rom, n).unwrap()
            && backgrounds.insert(bg.address)
        {
            let packed = rle1::compress(&bg.data).unwrap();
            assert_eq!(rle1::decompress(&packed).unwrap().data, bg.data);
            // Kobo's encoding is the shortest; Nintendo's is not.
            assert!(
                packed.len() <= bg.stream_len,
                "{name} {} background",
                what("")
            );
            counts.backgrounds += 1;
        }

        // And through the text format, as an import writes and a build
        // reads it.
        let (source, _) = import::read_level(rom, n).unwrap();
        let text = source.to_toml(&Comments::default());
        let (again, _) =
            Level::from_toml(&text).unwrap_or_else(|e| panic!("{name} {} as text: {e}", what("")));
        assert_eq!(again, source, "{name} {} as text", what(""));

        let addr = level::sprite_ptr(rom, n).unwrap();
        let list = sprites::read_sprites_at(rom, addr)
            .unwrap_or_else(|e| panic!("{name} {} sprites: {e}", what("")));
        let bytes = sprites::encode(list.header, &list.sprites, sizes)
            .unwrap_or_else(|e| panic!("{name} {} sprites: {e}", what("")));
        let again = sprites::decode(&bytes, sizes)
            .unwrap_or_else(|e| panic!("{name} {} sprites: {e}", what("")));
        assert_eq!(again.header, list.header, "{name} {} sprites", what(""));
        assert_eq!(again.sprites, list.sprites, "{name} {} sprites", what(""));
        assert_eq!(again.len, bytes.len(), "{name} {} sprites", what(""));
        if let Some(len) = rats_len(rom, addr) {
            assert!(list.len <= len, "{name} {} sprites", what(""));
        }
        counts.lists += 1;
        counts.identical += (bytes[..] == rom.read(addr, list.len).unwrap()[..]) as usize;
    }
}

#[test]
fn vanilla_levels_round_trip() {
    let Some(rom) = common::vanilla() else { return };
    let counts = check_rom("vanilla", &rom, None);
    eprintln!("vanilla: {} backgrounds", counts.backgrounds);
    // 538 object lists and 512 sprite lists. Eighteen object lists have
    // a screen jump that changes nothing, or one a new-screen bit does.
    assert_eq!(counts.lists, 538 + 512);
    assert_eq!(counts.identical, counts.lists - 18, "{counts:?}");
}

#[test]
fn lunar_magic_levels_round_trip() {
    let failures = Failures::new("level_data::lunar_magic_levels_round_trip");
    for (path, rom) in common::lunar_magic_roms() {
        let name = path.file_name().unwrap().to_string_lossy();
        if gfx::is_locked(&rom) {
            eprintln!("{name}: locked, skipped");
            continue;
        }
        failures.checked(&path, &rom);
        let counts = check_rom(&name, &rom, Some(&failures));
        eprintln!(
            "{name}: {} of {} lists byte for byte, {} backgrounds",
            counts.identical, counts.lists, counts.backgrounds
        );
    }
    failures.finish();
}
