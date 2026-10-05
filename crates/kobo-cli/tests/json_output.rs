//! `--json` output, which scripts such as `tools/corpus-sweep` read
//! instead of the sentences the commands print otherwise.

use std::path::Path;
use std::process::Command;

fn kobo(args: &[&str], rom: &Path) -> (Option<i32>, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_kobo"))
        .args(args)
        .arg("-r")
        .arg(rom)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e}): {stdout}"));
    (output.status.code(), value)
}

/// A 512 KiB LoROM image with a valid header.
fn image(dir: &Path) -> std::path::PathBuf {
    let mut bytes = vec![0xFF; 0x8_0000];
    bytes[0x7FC0..0x7FD5].copy_from_slice(b"KOBO JSON TEST       ");
    bytes[0x7FD5] = 0x20;
    bytes[0x7FD7] = 0x09;
    bytes[0x7FD8] = 0x00;
    let mut rom = kobo_core::Rom::from_bytes(bytes).unwrap();
    rom.fix_checksum().unwrap();
    let path = dir.join("image.sfc");
    rom.save(&path).unwrap();
    path
}

#[test]
fn rom_info_and_a_failed_build_print_json() {
    let dir = std::env::temp_dir().join(format!("kobo-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    let rom = image(&dir);

    let (code, info) = kobo(&["rom", "info", "--json"], &rom);
    assert_eq!(code, Some(0));
    assert_eq!(info["size"], 0x8_0000);
    assert_eq!(info["checksum"]["ok"], true);
    assert_eq!(info["sha1"].as_str().unwrap().len(), 40);
    assert!(info["lunar_magic"].is_null());

    std::fs::write(
        dir.join("project/kobo.toml"),
        "format = 1\n\n[levels]\n0x105 = \"missing.toml\"\n",
    )
    .unwrap();
    let project = dir.join("project");
    let out = dir.join("out.sfc");
    let (code, failed) = kobo(
        &[
            "build",
            project.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--json",
        ],
        &rom,
    );
    assert_eq!(code, Some(1));
    assert_eq!(failed["ok"], false);
    assert!(
        failed["error"].as_str().unwrap().contains("missing.toml"),
        "{failed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
