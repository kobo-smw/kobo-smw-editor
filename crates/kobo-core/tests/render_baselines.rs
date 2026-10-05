//! Every level's picture against the committed baseline
//! (`fixtures/render_hashes/`, `tests/common/render_hashes.rs` has the
//! format): the vanilla ROM's, and, with `KOBO_SA1_REFERENCE`, the SA-1
//! reference ROM's (docs/sa1.md). A change that should leave every picture
//! alone must pass as it is; one that changes pictures on purpose writes
//! the baseline again (`cargo xtask baseline`) and the diff shows which.

mod common;

use common::render_hashes;
use kobo_core::Rom;
use kobo_core::tiers::Tier;

/// The baseline's ROM SHA-1 (its header) and lines.
fn baseline(text: &str) -> (Option<&str>, Vec<String>) {
    let sha = text
        .lines()
        .find_map(|l| l.strip_prefix("# sha1 "))
        .map(str::trim);
    let lines = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_owned)
        .collect();
    (sha, lines)
}

fn check(rom: &Rom, name: &str, text: &str) {
    let (sha, want) = baseline(text);
    assert_eq!(
        sha,
        Some(rom.sha1_hex().as_str()),
        "{name}: the baseline is of another ROM"
    );
    let levels: Vec<u16> = (0..0x200).collect();
    let got = render_hashes::lines(rom, &levels);
    let differ = render_hashes::differ(&want, &got);
    assert!(
        differ.is_empty(),
        "{name}: {} levels' pictures differ from the baseline: {}\n\
         If the change is meant to change them, `cargo xtask baseline` writes it again.",
        differ.len(),
        differ.join(" ")
    );
}

#[test]
fn vanilla_pictures_are_the_baseline() {
    let Some(rom) = common::vanilla() else { return };
    check(
        &rom,
        "vanilla",
        include_str!("fixtures/render_hashes/vanilla.txt"),
    );
}

#[test]
fn sa1_reference_pictures_are_the_baseline() {
    let Some(path) = common::tier_path(Tier::Sa1Reference) else {
        return;
    };
    let rom = Rom::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    check(
        &rom,
        "the SA-1 reference ROM",
        include_str!("fixtures/render_hashes/sa1_reference.txt"),
    );
}
