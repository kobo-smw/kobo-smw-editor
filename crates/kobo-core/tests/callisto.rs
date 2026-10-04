//! Importing a Callisto project (`import::import_callisto`) and building it:
//! a synthetic project whose patch includes Callisto's header and a file by
//! its path from the root, with a full Map16 export and a graphics folder.

mod common;

use std::fs;
use std::path::Path;

use kobo_core::build::{self, Project};
use kobo_core::import::{self, CallistoOptions};
use kobo_core::map16::{GameTables, Map16Tile};
use kobo_core::tools::Tool;
use kobo_core::{Rom, SnesAddr, gfx};

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A full Map16 export of `clean`'s tables, with `edit` applied.
fn map16_export(clean: &Rom, edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let sections: [(usize, usize); 8] = [
        (0x70, 0x1_0000 * 8),
        (0x78, 0x8000 * 2),
        (0x80, 0),
        (0x88, 0),
        (0x90, 0),
        (0x98, 15 * 0x1000),
        (0xA0, 0x100),
        (0xA8, 0x40),
    ];
    let mut data = vec![0u8; 0xB0];
    data[..4].copy_from_slice(b"LM16");
    let mut at = Vec::new();
    for (table, len) in sections {
        let offset = data.len();
        data[table..table + 4].copy_from_slice(&(offset as u32).to_le_bytes());
        data[table + 4..table + 8].copy_from_slice(&(len as u32).to_le_bytes());
        data.resize(offset + len, 0);
        at.push(offset);
    }
    // What tiles act like: pages 0 and 1 themselves, the rest $130.
    for tile in 0..0x8000usize {
        let acts: u16 = if tile < 0x200 { tile as u16 } else { 0x130 };
        data[at[1] + 2 * tile..at[1] + 2 * tile + 2].copy_from_slice(&acts.to_le_bytes());
    }
    let game = GameTables::read(clean).unwrap();
    for tileset in 0..15u8 {
        for tile in 0..0x200u16 {
            let def = clean.read(game.address(tileset, tile), 8).unwrap();
            let o = at[5] + tileset as usize * 0x1000 + tile as usize * 8;
            data[o..o + 8].copy_from_slice(def);
        }
    }
    for tile in 0..0x200usize {
        let def = clean
            .read(SnesAddr::new(0x0D9100 + 8 * tile as u32), 8)
            .unwrap();
        let o = at[0] + (0x8000 + tile) * 8;
        data[o..o + 8].copy_from_slice(def);
    }
    let pipes = clean.read(SnesAddr::new(0x0D8AB0), 0xC0).unwrap().to_vec();
    let page1 = (0x133..=0x13Au16)
        .flat_map(|t| clean.read(game.address(0, t), 8).unwrap().to_vec())
        .collect::<Vec<_>>();
    // MAP16AppTable's order: set 0, page 1's own, sets 2 and 3.
    data[at[6]..at[6] + 0x40].copy_from_slice(&pipes[..0x40]);
    data[at[6] + 0x40..at[6] + 0x80].copy_from_slice(&page1);
    data[at[6] + 0x80..at[6] + 0x100].copy_from_slice(&pipes[0x40..]);
    let diagonal = clean.read(SnesAddr::new(0x0D8A70), 0x40).unwrap();
    data[at[7]..at[7] + 0x40].copy_from_slice(diagonal);
    edit(&mut data);
    data
}

#[test]
fn a_callisto_project_imports_and_builds() {
    let Some(clean) = common::vanilla() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("kobo-callisto-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let source = base.join("hack");
    let config = source.join("tools").join("callisto");
    write(
        &config.join("project.toml"),
        "[settings]\nproject_root = \"../../\"\n",
    );
    write(
        &config.join("build.toml"),
        "[orders]\nbuild_order = [\"Graphics\", \"Map16\", \"Patches\", \"Levels\"]\n\
         [resources]\npatches = [\"patches/mark.asm\"]\n\
         callisto_header = \"shared/header.asm\"\nmap16 = \"export/all.map16\"\n\
         [output]\noutput_rom = \"build/hack.smc\"\n",
    );
    write(&source.join("shared/header.asm"), "!marker_value = $5A\n");
    write(&source.join("shared/lib.asm"), "db !marker_value\n");
    write(
        &source.join("patches/mark.asm"),
        "incsrc \"callisto.asm\"\n\
         if not(defined(\"CALLISTO_ASSEMBLING\"))\n  error \"no header\"\nendif\n\
         org $0EFFF0\n%incsrc_file(\"shared/lib.asm\")\n",
    );
    // Tile $200 defined and acting like $25 on page 2.
    let map16 = map16_export(&clean, |d| {
        let defs = 0xB0;
        d[defs + 0x200 * 8..defs + 0x200 * 8 + 8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let acts = defs + 0x1_0000 * 8;
        d[acts + 0x400..acts + 0x402].copy_from_slice(&0x25u16.to_le_bytes());
    });
    write(&source.join("export/all.map16"), &map16);
    // GFX00 as Lunar Magic exports it, with a pixel changed; GFX01 as the
    // game has it.
    let reader = gfx::GfxReader::new(&clean).unwrap();
    let mut gfx00 = reader.read(0).unwrap().to_lm_export();
    gfx00[0] ^= 0x80;
    write(&source.join("build/Graphics/GFX00.bin"), &gfx00);
    write(
        &source.join("build/Graphics/GFX01.bin"),
        reader.read(1).unwrap().to_lm_export(),
    );
    // What a project never holds.
    write(&source.join("build/hack.smc"), [0u8; 16]);
    write(&source.join("tools/flips/flips.exe"), [0u8; 16]);
    write(&source.join("notes.txt"), "kept");

    let dir = base.join("project");
    let report =
        import::import_callisto(&source, &clean, &dir, &CallistoOptions::default()).unwrap();
    assert!(report.levels.is_empty(), "{report:?}");
    assert_eq!(report.map16, vec![2]);
    let manifest = fs::read_to_string(dir.join("kobo.toml")).unwrap();
    assert!(
        manifest.contains("[callisto]\nheader = \"shared/header.asm\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("early = [\"patches/mark.asm\"]"),
        "{manifest}"
    );
    assert!(dir.join("notes.txt").is_file());
    assert!(dir.join("graphics/GFX00.png").is_file());
    assert!(!dir.join("graphics/GFX01.png").exists());
    for gone in ["build", "tools", "export/all.map16"] {
        assert!(!dir.join(gone).exists(), "{gone} was copied");
    }
    let page = fs::read_to_string(dir.join("map16/02.toml")).unwrap();
    assert!(page.contains("0x200 = { acts = 0x025"), "{page}");

    // The build needs Asar for the patch.
    if common::tool(Tool::Asar, "KOBO_REQUIRE_ASAR").is_none() {
        let _ = fs::remove_dir_all(&base);
        return;
    }
    let project = Project::load(&dir).unwrap();
    let built = build::build(&clean, &project).unwrap();
    assert_eq!(built.read_u8(SnesAddr::new(0x0EFFF0)).unwrap(), 0x5A);
    let tile = kobo_core::map16::pages::PAGE_GROUPS[0]
        .definition(&built, 0x200)
        .unwrap()
        .unwrap();
    assert_eq!(
        Map16Tile::from_bytes(built.read(tile, 8).unwrap().try_into().unwrap()),
        Map16Tile::from_bytes([1, 2, 3, 4, 5, 6, 7, 8])
    );
    let _ = fs::remove_dir_all(&base);
}
