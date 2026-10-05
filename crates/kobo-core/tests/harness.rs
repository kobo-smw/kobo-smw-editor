//! The test harness's own helpers (`tests/common`): the known failures a
//! corpus test is checked against, catching a level's failure, and scratch
//! folders. No ROM needed.

mod common;

use std::path::Path;

use common::failures::{Failures, Known, catch};
use common::temp::TempDir;
use kobo_core::Rom;

fn rom(tag: u8) -> Rom {
    let mut data = common::synthetic_base().data().to_vec();
    data[0] = tag;
    Rom::from_headerless(data).unwrap()
}

fn known(a: &Rom, b: &Rom) -> Known {
    Known::parse(&format!(
        r#"
[[failure]]
hack = "a"
sha1 = "{}"
tests = "t::one, t::two"
levels = ["105", "1FF"]
reason = "two levels"

[[failure]]
hack = "b"
sha1 = "{}"
tests = "t::one"
levels = "all"
reason = "the whole hack"
"#,
        a.sha1_hex(),
        b.sha1_hex()
    ))
}

#[test]
fn known_failures_pass_and_new_ones_fail() {
    let (a, b, c) = (rom(1), rom(2), rom(3));
    let failures = Failures::with_known("t::one", known(&a, &b));
    for rom in [&a, &b, &c] {
        failures.checked(Path::new("x.smc"), rom);
    }
    failures.fail(&a, Some(0x105), "known");
    failures.fail(&a, Some(0x1FF), "known");
    failures.fail(&b, Some(0x000), "any level of b");
    failures.fail(&b, None, "and b as a whole");
    failures.finish();

    let failures = Failures::with_known("t::one", known(&a, &b));
    failures.checked(Path::new("c.smc"), &c);
    failures.fail(&c, Some(0x105), "new");
    let e = catch(|| failures.finish()).unwrap_err();
    assert!(
        e.contains("not in known_failures.toml") && e.contains("c.smc level 105: new"),
        "{e}"
    );
}

#[test]
fn a_known_failure_that_passes_must_come_off_the_list() {
    let (a, b) = (rom(1), rom(2));
    // a's level 1FF passes.
    let failures = Failures::with_known("t::two", known(&a, &b));
    failures.checked(Path::new("a.smc"), &a);
    failures.fail(&a, Some(0x105), "known");
    let e = catch(|| failures.finish()).unwrap_err();
    assert!(
        e.contains("take these out") && e.contains("a level 1FF"),
        "{e}"
    );

    // b, listed for every level, passes entirely.
    let failures = Failures::with_known("t::one", known(&a, &b));
    failures.checked(Path::new("b.smc"), &b);
    failures.fail(&a, Some(0x105), "known");
    failures.fail(&a, Some(0x1FF), "known");
    failures.checked(Path::new("a.smc"), &a);
    let e = catch(|| failures.finish()).unwrap_err();
    assert!(e.contains("b: every level"), "{e}");

    // A listed hack the test never ran on says nothing.
    let failures = Failures::with_known("t::one", known(&a, &b));
    failures.finish();
}

#[test]
fn a_listed_level_does_not_cover_the_whole_rom() {
    let (a, b) = (rom(1), rom(2));
    let failures = Failures::with_known("t::two", known(&a, &b));
    failures.checked(Path::new("a.smc"), &a);
    failures.fail(&a, Some(0x105), "known");
    failures.fail(&a, Some(0x1FF), "known");
    failures.fail(&a, None, "the ROM");
    assert!(catch(|| failures.finish()).is_err());
}

#[test]
fn the_checked_in_list_parses() {
    let _ = Known::load();
}

#[test]
fn catch_gives_the_panic_message() {
    assert_eq!(catch(|| 7), Ok(7));
    let e = catch(|| assert_eq!(1, 2, "level {:03X}", 0x105)).unwrap_err();
    assert!(e.contains("level 105"), "{e}");
}

#[test]
fn a_scratch_folder_goes_away_with_its_test() {
    let path = {
        let dir = TempDir::new("harness");
        std::fs::write(dir.join("rom.smc"), b"data").unwrap();
        dir.to_path_buf()
    };
    assert!(!path.exists());
    let path = {
        let dir = TempDir::unmade("harness");
        assert!(!dir.exists());
        dir.to_path_buf()
    };
    assert!(!path.exists());
    // On a panic too, unless KOBO_KEEP_TEMP is set.
    let mut kept = None;
    let _ = catch(|| {
        let dir = TempDir::new("harness-panic");
        kept = Some(dir.to_path_buf());
        panic!("a test fails");
    });
    let kept = kept.unwrap();
    assert_eq!(
        kept.exists(),
        std::env::var_os(common::temp::KEEP_ENV_VAR).is_some()
    );
}

/// `tools/kobo_config.py`, which the scripts in `tools/` find their
/// settings with, resolves every setting as `kobo_core::tiers` does, on
/// this machine's configuration and on a corpus of folders.
#[test]
fn the_scripts_find_settings_as_the_library_does() {
    use kobo_core::tiers::Tier;
    use std::process::Command;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/kobo_config.py");
    let python = |name: &str, corpus: Option<&str>| {
        let mut cmd = Command::new("python3");
        cmd.arg(&script).arg(name);
        if let Some(c) = corpus {
            cmd.env("KOBO_LM_ROMS", c);
        }
        cmd.output().ok()
    };
    if python("rom", None).is_none_or(|o| String::from_utf8_lossy(&o.stderr).contains("tomllib")) {
        eprintln!("skipping: no Python 3.11 to run tools/kobo_config.py");
        return;
    }
    let tiers = Tier::ALL
        .into_iter()
        .filter(|t| !matches!(t, Tier::Tool(_)));
    for tier in tiers {
        let out = python(tier.name(), None).unwrap();
        let theirs: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        let ours: Vec<String> = match tier.resolve() {
            Ok(Some((paths, _))) => paths.iter().map(|p| p.display().to_string()).collect(),
            _ => Vec::new(),
        };
        assert_eq!(theirs, ours, "{}", tier.name());
    }
    // Folders of ROMs and patches, as the corpus is laid out.
    let dir = TempDir::new("harness-corpus");
    for f in ["a/x.smc", "a/x.bps", "a/y.bps", "a/z.SFC", "b/p.bps"] {
        std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
        std::fs::write(dir.join(f), b"").unwrap();
    }
    let corpus = std::env::join_paths([dir.join("a"), dir.join("b"), dir.join("one.smc")]).unwrap();
    let corpus = corpus.to_str().unwrap();
    let out = python("lm_roms", Some(corpus)).unwrap();
    let theirs: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    let ours: Vec<String> =
        kobo_core::tiers::expand_roms(&std::env::split_paths(corpus).collect::<Vec<_>>())
            .unwrap()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
    assert_eq!(theirs, ours);
    assert_eq!(ours.len(), 5);
}
