//! Kobo's development tasks, run as `cargo xtask <task>` (the alias is in
//! `.cargo/config.toml`). Only cargo is needed; each task says what else it
//! uses when it is missing.
//!
//! - `tiers [--list TIER]`: what this machine has for each test tier
//!   ([`kobo_core::tiers`]), or one tier's paths, a line each, for scripts.
//! - `verify [--strict] [--require-all] [--full] [-- TEST ARGS]`:
//!   formatting, clippy, the scripts in `tools/`, what this machine has
//!   for each tier, then the tests, and a report of every test that
//!   skipped, by tier. `--strict` requires every tier this machine has set
//!   (`KOBO_REQUIRE_<TIER>`), so a test that still skips fails;
//!   `--require-all` requires every tier (`KOBO_REQUIRE_ALL`); `--full`
//!   turns on the checks of every level (`KOBO_FULL_RENDER`).
//! - `lint-tools`: `bash -n` and shellcheck on the shell scripts in
//!   `tools/`, Python's parser on the Python ones, `luac -p` on the Lua.
//! - `skips FILE`: the skips a test run appended to `KOBO_SKIP_LOG`, by
//!   tier (`verify` prints them itself).
//! - `baseline`: the picture hashes of every level of the vanilla ROM
//!   and the SA-1 reference ROM, written to
//!   `tests/fixtures/render_hashes/` for `tests/render_baselines.rs`.

mod baseline;
mod lint;
mod tiers;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (task, rest) = match args.split_first() {
        Some((task, rest)) => (task.as_str(), rest),
        None => ("help", &[][..]),
    };
    let result = match task {
        "tiers" => tiers::run(rest),
        "verify" => verify(rest),
        "baseline" => baseline::run_task(rest),
        "skips" => match rest {
            [log] => {
                tiers::report_skips(Path::new(log));
                Ok(())
            }
            _ => Err("usage: cargo xtask skips KOBO_SKIP_LOG-file".into()),
        },
        "lint-tools" => lint::run(rest.iter().any(|a| a == "--strict")),
        "help" | "-h" | "--help" => {
            println!("{}", HELP.trim());
            Ok(())
        }
        other => Err(format!("unknown task `{other}`\n\n{}", HELP.trim())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

const HELP: &str = "
cargo xtask <task>

  tiers [--list TIER]       what this machine has for each test tier
  verify [--strict] [--require-all] [--full] [-- TEST ARGS]
                            fmt, clippy, lint-tools, the tests, and what skipped
  lint-tools [--strict]     syntax-check the scripts in tools/ (and shellcheck them)
  baseline                  write every level's picture hashes for render_baselines
  skips FILE                what the tests that wrote FILE (KOBO_SKIP_LOG) skipped, by tier

docs/testing.md has what each tier checks.
";

/// The workspace's root folder.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// Runs `cmd` in the workspace's root, failing on a non-zero exit.
pub fn run(what: &str, cmd: &mut Command) -> Result<(), String> {
    eprintln!("==> {what}");
    let status = cmd
        .current_dir(root())
        .status()
        .map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

pub fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn verify(args: &[String]) -> Result<(), String> {
    let (ours, test_args) = match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };
    let flag = |f: &str| ours.iter().any(|a| a == f);
    for a in ours {
        if !matches!(a.as_str(), "--strict" | "--require-all" | "--full") {
            return Err(format!("verify: unknown option `{a}`"));
        }
    }
    let (strict, full) = (flag("--strict"), flag("--full"));
    let require_all = flag("--require-all");
    run(
        "cargo fmt --check",
        cargo().args(["fmt", "--all", "--check"]),
    )?;
    run(
        "cargo clippy",
        cargo().args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
    )?;
    lint::run(strict || require_all)?;
    tiers::run(&[])?;
    let log = root().join("target").join("kobo-skips.txt");
    let _ = std::fs::remove_file(&log);
    let mut test = cargo();
    // A dev build: kobo-core is optimised there (Cargo.toml), and keeps its
    // debug assertions and overflow checks, which a release build drops.
    test.args(["test", "--workspace", "--no-fail-fast"]);
    if !test_args.is_empty() {
        test.arg("--").args(test_args);
    }
    test.env(kobo_core::tiers::SKIP_LOG_ENV_VAR, &log);
    if strict {
        for tier in kobo_core::tiers::Tier::ALL {
            if tier.resolve().is_ok_and(|r| r.is_some()) {
                test.env(tier.require_var(), "1");
            }
        }
    }
    if require_all {
        test.env(kobo_core::tiers::REQUIRE_ALL_ENV_VAR, "1");
    }
    if full {
        test.env(kobo_core::tiers::FULL_RENDER_ENV_VAR, "1");
    }
    let result = run("cargo test", &mut test);
    tiers::report_skips(&log);
    result
}
