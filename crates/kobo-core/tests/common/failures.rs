//! What a corpus test found wrong, collected across every hack and level
//! and reported once, against the known failures in
//! `tests/fixtures/known_failures.toml`.
//!
//! A test records each failure under the hack's headerless SHA-1 and the
//! level (or none, for a failure of the whole ROM) with
//! [`Failures::fail`], and calls [`Failures::finish`] last. That panics
//! on a failure the list does not hold, and on a listed failure of this
//! test that did not happen on a hack the test ran on, so the list
//! shrinks as gaps are fixed. Known failures that still fail are printed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;

use kobo_core::Rom;

/// `f`'s result, or the message it panicked with: for a check written as
/// assertions, run once per level so that one level's failure is
/// recorded ([`Failures::fail`]) and the rest still run. The panic is not
/// printed as it happens; its message is the failure.
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    use std::cell::Cell;
    use std::panic::{self, AssertUnwindSafe};
    thread_local!(static QUIET: Cell<bool> = const { Cell::new(false) });
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                default(info);
            }
        }));
    });
    QUIET.with(|q| q.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    QUIET.with(|q| q.set(false));
    result.map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panicked".into())
    })
}

/// The list, keyed by test, then hack SHA-1.
#[derive(Debug, Default)]
pub struct Known {
    entries: BTreeMap<String, BTreeMap<String, KnownEntry>>,
}

#[derive(Debug)]
struct KnownEntry {
    name: String,
    /// `None`: the whole hack (every level, or the ROM itself).
    levels: Option<BTreeSet<u16>>,
}

impl Known {
    /// The checked-in list.
    pub fn load() -> Known {
        Known::parse(include_str!("../fixtures/known_failures.toml"))
    }

    /// Parses the list: one `[[failure]]` table per hack and test.
    pub fn parse(text: &str) -> Known {
        let doc: toml::Table = toml::from_str(text).expect("known_failures.toml must parse");
        let mut known = Known::default();
        let entries = doc.get("failure").and_then(|f| f.as_array());
        for entry in entries.into_iter().flatten() {
            let field = |key: &str| {
                entry
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| panic!("known failure without `{key}`: {entry}"))
            };
            assert!(
                entry.get("reason").and_then(|v| v.as_str()).is_some(),
                "known failure without a `reason`: {entry}"
            );
            let levels = match entry.get("levels") {
                None => None,
                Some(toml::Value::String(s)) if s == "all" => None,
                Some(toml::Value::Array(list)) => Some(
                    list.iter()
                        .map(|l| {
                            let s = l.as_str().expect("a level is a hex string");
                            u16::from_str_radix(s, 16).expect("a level is a hex string")
                        })
                        .collect(),
                ),
                Some(other) => panic!("`levels` is \"all\" or a list: {other}"),
            };
            for test in field("tests").split(',').map(str::trim) {
                let old = known.entries.entry(test.to_owned()).or_default().insert(
                    field("sha1").to_owned(),
                    KnownEntry {
                        name: field("hack").to_owned(),
                        levels: levels.clone(),
                    },
                );
                assert!(old.is_none(), "{} listed twice for {test}", field("hack"));
            }
        }
        known
    }

    fn expects(&self, test: &str, sha1: &str, level: Option<u16>) -> bool {
        let Some(entry) = self.entries.get(test).and_then(|t| t.get(sha1)) else {
            return false;
        };
        match (&entry.levels, level) {
            (None, _) => true,
            (Some(levels), Some(level)) => levels.contains(&level),
            (Some(_), None) => false,
        }
    }
}

/// One test's failures over the corpus. Shared between threads.
pub struct Failures {
    test: String,
    known: Known,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Each hack's display name, by SHA-1, once checked.
    checked: BTreeMap<String, String>,
    /// (SHA-1, level) to its messages.
    found: BTreeMap<(String, Option<u16>), Vec<String>>,
}

impl Failures {
    /// Failures of the test named `test` in the list.
    pub fn new(test: &str) -> Failures {
        Failures::with_known(test, Known::load())
    }

    pub fn with_known(test: &str, known: Known) -> Failures {
        Failures {
            test: test.to_owned(),
            known,
            state: Mutex::new(State::default()),
        }
    }

    /// Records that the test ran on `rom`, found at `path`: a listed
    /// failure of a hack never checked is not reported as fixed.
    pub fn checked(&self, path: &Path, rom: &Rom) {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut state = self.state.lock().unwrap();
        state.checked.insert(rom.sha1_hex(), name);
    }

    /// Records a failure of `rom` (at `level`, or of the whole ROM).
    pub fn fail(&self, rom: &Rom, level: Option<u16>, message: impl Into<String>) {
        let mut state = self.state.lock().unwrap();
        state
            .found
            .entry((rom.sha1_hex(), level))
            .or_default()
            .push(message.into());
    }

    /// Whether anything failed, known or not.
    pub fn any(&self) -> bool {
        !self.state.lock().unwrap().found.is_empty()
    }

    /// Reports the test's failures: panics on any the list does not hold,
    /// and on a listed failure of a checked hack that did not happen.
    pub fn finish(self) {
        let state = self.state.into_inner().unwrap();
        let name = |sha: &str| {
            state
                .checked
                .get(sha)
                .cloned()
                .unwrap_or_else(|| sha.to_owned())
        };
        let place = |sha: &str, level: Option<u16>| match level {
            Some(level) => format!("{} level {level:03X}", name(sha)),
            None => name(sha),
        };
        let mut unexpected = String::new();
        let mut known = 0;
        // Each hack's known failures, in a line: the places, and the first
        // message.
        let mut known_by_hack: BTreeMap<&str, (Vec<String>, &str)> = BTreeMap::new();
        for ((sha, level), messages) in &state.found {
            if self.known.expects(&self.test, sha, *level) {
                known += 1;
                let entry = known_by_hack
                    .entry(sha)
                    .or_insert_with(|| (Vec::new(), &messages[0]));
                entry
                    .0
                    .push(level.map_or_else(|| "the ROM".into(), |l| format!("{l:03X}")));
                continue;
            }
            for message in messages {
                let _ = writeln!(unexpected, "{}: {message}", place(sha, *level));
            }
        }
        let mut fixed = String::new();
        if let Some(listed) = self.known.entries.get(&self.test) {
            for (sha, entry) in listed {
                if !state.checked.contains_key(sha) {
                    continue;
                }
                let failed_at: BTreeSet<Option<u16>> = state
                    .found
                    .keys()
                    .filter(|(s, _)| s == sha)
                    .map(|(_, l)| *l)
                    .collect();
                match &entry.levels {
                    None if failed_at.is_empty() => {
                        let _ = writeln!(fixed, "{}: every level", entry.name);
                    }
                    None => {}
                    Some(levels) => {
                        for level in levels {
                            if !failed_at.contains(&Some(*level)) {
                                let _ = writeln!(fixed, "{} level {level:03X}", entry.name);
                            }
                        }
                    }
                }
            }
        }
        for (sha, (places, first)) in &known_by_hack {
            let shown = places
                .iter()
                .take(32)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let more = places.len().saturating_sub(32);
            eprintln!(
                "known failure: {}: {}{} ({first})",
                name(sha),
                shown,
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            );
        }
        if known > 0 {
            eprintln!("{known} known failures of {} still fail", self.test);
        }
        assert!(
            unexpected.is_empty() && fixed.is_empty(),
            "{}{}",
            if unexpected.is_empty() {
                String::new()
            } else {
                format!(
                    "{} found failures not in known_failures.toml:\n{unexpected}",
                    self.test
                )
            },
            if fixed.is_empty() {
                String::new()
            } else {
                format!(
                    "{} passes where known_failures.toml says it fails; take these out of it:\n{fixed}",
                    self.test
                )
            },
        );
    }
}
