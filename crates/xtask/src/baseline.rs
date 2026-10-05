//! `cargo xtask baseline`: writes the picture hashes of every level of the
//! vanilla ROM, and of the SA-1 reference ROM when that tier is set, to
//! `tests/fixtures/render_hashes/`, which `tests/render_baselines.rs`
//! checks, and says which levels changed.

use std::path::Path;
use std::process::Command;

use kobo_core::Rom;
use kobo_core::tiers::Tier;

use crate::{cargo, root, run};

pub fn run_task(args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: cargo xtask baseline".into());
    }
    run(
        "build render_hashes",
        cargo().args([
            "build",
            "--release",
            "--package",
            "kobo-core",
            "--example",
            "render_hashes",
        ]),
    )?;
    let mut written = 0;
    for (tier, file) in [
        (Tier::Rom, "vanilla.txt"),
        (Tier::Sa1Reference, "sa1_reference.txt"),
    ] {
        let Some(path) = tier.path().map_err(|e| e.to_string())? else {
            eprintln!(
                "{}: not set ({} or `{}`), its baseline left as it is",
                tier.name(),
                tier.env_var(),
                tier.key()
            );
            continue;
        };
        write(&path, file)?;
        written += 1;
    }
    if written == 0 {
        return Err("no ROM to make a baseline of".into());
    }
    Ok(())
}

fn write(rom_path: &Path, file: &str) -> Result<(), String> {
    let rom = Rom::load(rom_path).map_err(|e| format!("{}: {e}", rom_path.display()))?;
    let exe = root()
        .join("target/release/examples/render_hashes")
        .with_extension(std::env::consts::EXE_EXTENSION);
    eprintln!("==> render_hashes {}", rom_path.display());
    let out = Command::new(&exe)
        .arg(rom_path)
        .output()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "render_hashes failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let lines = String::from_utf8_lossy(&out.stdout).into_owned();
    let target = root()
        .join("crates/kobo-core/tests/fixtures/render_hashes")
        .join(file);
    let old = std::fs::read_to_string(&target).unwrap_or_default();
    let old: Vec<&str> = old.lines().filter(|l| !l.starts_with('#')).collect();
    let changed: Vec<&str> = lines
        .lines()
        .filter(|l| !old.contains(l))
        .filter_map(|l| l.get(..3))
        .collect();
    let text = format!(
        "# Every level's picture hashes (tests/common/render_hashes.rs), written by\n\
         # `cargo xtask baseline` and checked by tests/render_baselines.rs.\n\
         # sha1 {}\n{lines}",
        rom.sha1_hex()
    );
    std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&target, text).map_err(|e| e.to_string())?;
    if old.is_empty() {
        eprintln!("    {file}: written");
    } else if changed.is_empty() {
        eprintln!("    {file}: unchanged");
    } else {
        let more = changed.len().saturating_sub(48);
        eprintln!(
            "    {file}: {} levels changed: {}{}",
            changed.len(),
            changed[..changed.len() - more].join(" "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        );
    }
    Ok(())
}
