//! `cargo xtask tiers`, and the report of what skipped.

use std::collections::BTreeMap;
use std::path::Path;

use kobo_core::tiers::{self, Tier};

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => {
            table();
            Ok(())
        }
        [flag, name] if flag == "--list" => {
            let tier = Tier::ALL
                .into_iter()
                .find(|t| t.name() == name)
                .ok_or_else(|| format!("no tier `{name}`; `cargo xtask tiers` lists them"))?;
            match tier.resolve().map_err(|e| e.to_string())? {
                Some((paths, _)) => {
                    for p in paths {
                        println!("{}", p.display());
                    }
                    Ok(())
                }
                None => Err(format!(
                    "{} is not set: {} or `{}`",
                    tier.name(),
                    tier.env_var(),
                    tier.key()
                )),
            }
        }
        _ => Err("usage: cargo xtask tiers [--list TIER]".into()),
    }
}

fn table() {
    println!("Test tiers (docs/testing.md); a tier not set skips its tests.");
    if std::env::var_os(tiers::REQUIRE_ALL_ENV_VAR).is_some() {
        println!(
            "{} is set: every tier is required.",
            tiers::REQUIRE_ALL_ENV_VAR
        );
    }
    println!();
    let width = Tier::ALL.iter().map(|t| t.name().len()).max().unwrap_or(0);
    for tier in Tier::ALL {
        let required = if tier.required() { " (required)" } else { "" };
        let state = match tier.resolve() {
            Ok(Some((paths, origin))) => {
                let missing: Vec<_> = paths.iter().filter(|p| !p.exists()).collect();
                let what = match paths.as_slice() {
                    [one] => one.display().to_string(),
                    many => format!("{} entries", many.len()),
                };
                if missing.is_empty() {
                    format!("{what}, from {origin}")
                } else {
                    format!(
                        "{what}, from {origin}; MISSING: {}",
                        missing
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ok(None) => format!("not set ({} or `{}`)", tier.env_var(), tier.key()),
            Err(e) => format!("INVALID: {e}"),
        };
        println!("  {:width$}  {state}{required}", tier.name());
        println!("  {:width$}    {}", "", tier.describe());
    }
    match tiers::full_render() {
        Ok(true) => println!(
            "\nThe checks of every level run ({}).",
            tiers::FULL_RENDER_ENV_VAR
        ),
        Ok(false) => println!(
            "\nThe checks of every level are off ({}=1 or `tests.full_render`).",
            tiers::FULL_RENDER_ENV_VAR
        ),
        Err(e) => println!("\n{e}"),
    }
}

/// Prints the skips the tests appended to `log`, by tier.
pub fn report_skips(log: &Path) {
    let Ok(text) = std::fs::read_to_string(log) else {
        eprintln!("\nNo test skipped.");
        return;
    };
    let mut by_tier: BTreeMap<&str, (Vec<&str>, &str)> = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, '\t');
        let (Some(test), Some(tier), why) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let entry = by_tier
            .entry(tier)
            .or_insert((Vec::new(), why.unwrap_or("")));
        if !entry.0.contains(&test) {
            entry.0.push(test);
        }
    }
    eprintln!("\nSkipped tests, by tier (set the tier, or KOBO_REQUIRE_<TIER> to fail instead):");
    for (tier, (tests, why)) in by_tier {
        eprintln!("  {tier}: {} tests ({why})", tests.len());
        for test in tests.iter().take(6) {
            eprintln!("      {test}");
        }
        if tests.len() > 6 {
            eprintln!("      and {} more", tests.len() - 6);
        }
    }
}
