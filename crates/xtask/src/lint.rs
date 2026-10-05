//! `cargo xtask lint-tools`: the scripts in `tools/` checked as CI checks
//! them. `bash -n` and Python's parser always; shellcheck and `luac -p`
//! when installed, and shellcheck required with `--strict` or in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::root;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Shell,
    Python,
    Lua,
}

/// Every script under `tools/`, by its extension or its `#!` line.
fn scripts() -> Vec<(PathBuf, Kind)> {
    let mut out = Vec::new();
    let mut stack = vec![root().join("tools")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "__pycache__") {
                    stack.push(path);
                }
                continue;
            }
            if let Some(kind) = kind(&path) {
                out.push((path, kind));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn kind(path: &Path) -> Option<Kind> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sh") => return Some(Kind::Shell),
        Some("py") => return Some(Kind::Python),
        Some("lua") => return Some(Kind::Lua),
        Some(_) => return None,
        None => {}
    }
    let text = std::fs::read(path).ok()?;
    let first = text.split(|&b| b == b'\n').next()?;
    let first = String::from_utf8_lossy(first);
    if !first.starts_with("#!") {
        return None;
    }
    if first.contains("bash") || first.ends_with("/sh") || first.contains("env sh") {
        Some(Kind::Shell)
    } else if first.contains("python") {
        Some(Kind::Python)
    } else {
        None
    }
}

fn found(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn run(strict: bool) -> Result<(), String> {
    eprintln!("==> lint-tools");
    let scripts = scripts();
    let shellcheck = found("shellcheck");
    let luac = ["luac", "luac5.4", "luac5.3"]
        .into_iter()
        .find(|p| Command::new(p).arg("-v").output().is_ok());
    let python = ["python3", "python"].into_iter().find(|p| found(p));
    if !shellcheck && (strict || std::env::var_os("CI").is_some()) {
        return Err("shellcheck is not installed".into());
    }
    let mut failed = Vec::new();
    let mut check = |path: &Path, cmd: &mut Command| {
        let out = cmd.output();
        match out {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                eprint!("{}", String::from_utf8_lossy(&out.stdout));
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
                failed.push(path.to_path_buf());
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failed.push(path.to_path_buf());
            }
        }
    };
    for (path, kind) in &scripts {
        match kind {
            Kind::Shell => {
                check(path, Command::new("bash").arg("-n").arg(path));
                if shellcheck {
                    check(
                        path,
                        Command::new("shellcheck")
                            .args(["--severity=warning", "--external-sources"])
                            .arg(path),
                    );
                }
            }
            Kind::Python => {
                if let Some(python) = python {
                    check(
                        path,
                        Command::new(python)
                            .args(["-c", "import ast, sys; ast.parse(open(sys.argv[1], encoding='utf-8').read(), sys.argv[1])"])
                            .arg(path),
                    );
                }
            }
            Kind::Lua => {
                if let Some(luac) = luac {
                    check(path, Command::new(luac).args(["-p"]).arg(path));
                }
            }
        }
    }
    let count = |k| scripts.iter().filter(|(_, kind)| *kind == k).count();
    eprintln!(
        "    {} shell ({}), {} Python{}, {} Lua{}",
        count(Kind::Shell),
        if shellcheck {
            "bash -n, shellcheck"
        } else {
            "bash -n only: shellcheck is not installed"
        },
        count(Kind::Python),
        if python.is_some() {
            ""
        } else {
            " (not checked: no Python)"
        },
        count(Kind::Lua),
        if luac.is_some() {
            ""
        } else {
            " (not checked: no luac)"
        },
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "lint-tools: {}",
            failed
                .iter()
                .map(|p| p.strip_prefix(root()).unwrap_or(p).display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}
