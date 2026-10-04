//! Callisto projects (`github.com/Underrout/callisto`), read for import
//! (`import::import_callisto`). Callisto builds a hack by running Lunar
//! Magic's command line and the tools in an order its configuration gives;
//! this reads that configuration, from its documentation (v0.6.2), as data.
//!
//! The configuration is the `.toml` files next to `callisto.exe`, merged:
//! a table may be split across files. `[variables]` (in `variables.toml`,
//! or in the one file when there is one) are substituted into every string
//! as `{name}`, `{{` and `}}` standing for braces. `[settings]
//! project_root` is relative to the configuration's folder, and every other
//! path to the project root. Profiles (`profiles/`) and the user's own
//! configuration are not read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CallistoError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Toml { path: PathBuf, message: String },
    #[error("no Callisto configuration (`.toml` files with `[orders] build_order`) under {0}")]
    NotFound(PathBuf),
    #[error("variable `{0}` is not defined, or defined in terms of itself")]
    Variable(String),
    #[error("`{key}` is not {expected}")]
    Type { key: String, expected: &'static str },
}

/// A tool Callisto runs by name from the build order (`[tools.generic.X]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericTool {
    pub name: String,
    /// The tool's folder, under the project root.
    pub directory: PathBuf,
    pub executable: String,
}

impl GenericTool {
    /// Which of the toolchain's programs this is, by its executable.
    pub fn kind(&self) -> Option<ToolKind> {
        let exe = self.executable.to_ascii_lowercase();
        let exe = exe.trim_end_matches(".exe");
        Some(match exe {
            "pixi" => ToolKind::Pixi,
            "gps" => ToolKind::Gps,
            "uberasmtool" => ToolKind::UberAsm,
            "addmusick" => ToolKind::AddmusicK,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Pixi,
    Gps,
    UberAsm,
    AddmusicK,
}

/// What a Callisto project's configuration says, paths under its root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    /// The configuration's folder.
    pub config: PathBuf,
    pub build_order: Vec<String>,
    /// `[resources] initial_patch`: a BPS patch of the clean ROM.
    pub initial_patch: Option<PathBuf>,
    /// `[resources] patches`, in order.
    pub patches: Vec<PathBuf>,
    /// `[resources] modules`.
    pub modules: Vec<PathBuf>,
    /// `[resources] callisto_header`, which Callisto's generated
    /// `callisto.asm` includes.
    pub callisto_header: Option<PathBuf>,
    pub tools: Vec<GenericTool>,
    /// `[resources]` exports: `levels` (a folder of MWL files), `map16`,
    /// `shared_palettes`, `overworld`, `titlescreen`, `credits`,
    /// `global_exanimation`, by key.
    pub resources: BTreeMap<String, PathBuf>,
    /// `[settings] use_text_map16_format`.
    pub text_map16: bool,
    /// `[output] output_rom`, next to which Lunar Magic keeps the
    /// `Graphics` and `ExGraphics` folders.
    pub output_rom: Option<PathBuf>,
}

impl Project {
    /// Finds the configuration under `dir` (the folder itself, or one up
    /// to three levels down) and reads it.
    pub fn find(dir: &Path) -> Result<Self, CallistoError> {
        let config = find_config(dir, 3)?.ok_or_else(|| CallistoError::NotFound(dir.into()))?;
        Self::read(&config)
    }

    /// Reads the configuration in folder `config`.
    pub fn read(config: &Path) -> Result<Self, CallistoError> {
        let table = merged(config)?;
        let variables = variables(&table)?;
        let expand = |s: &str| expand(s, &variables);
        let get = |path: &[&str]| -> Option<&toml::Value> {
            let mut value = table.get(path[0])?;
            for key in &path[1..] {
                value = value.get(key)?;
            }
            Some(value)
        };
        let string = |path: &[&str]| -> Result<Option<String>, CallistoError> {
            match get(path) {
                None => Ok(None),
                Some(toml::Value::String(s)) => Ok(Some(expand(s)?)),
                Some(_) => Err(CallistoError::Type {
                    key: path.join("."),
                    expected: "a string",
                }),
            }
        };
        let strings = |path: &[&str]| -> Result<Vec<String>, CallistoError> {
            match get(path) {
                None => Ok(Vec::new()),
                Some(toml::Value::Array(items)) => items
                    .iter()
                    .map(|item| match item {
                        toml::Value::String(s) => expand(s),
                        _ => Err(CallistoError::Type {
                            key: path.join("."),
                            expected: "a list of strings",
                        }),
                    })
                    .collect(),
                Some(_) => Err(CallistoError::Type {
                    key: path.join("."),
                    expected: "a list of strings",
                }),
            }
        };
        let root_setting = string(&["settings", "project_root"])?.unwrap_or_else(|| ".".into());
        let root = normalise(&config.join(native(&root_setting)));
        let path = |s: String| PathBuf::from(native(&s));
        let mut tools = Vec::new();
        if let Some(toml::Value::Table(generic)) = get(&["tools", "generic"]) {
            for (name, tool) in generic {
                let field = |key: &str| -> Result<Option<String>, CallistoError> {
                    match tool.get(key) {
                        None => Ok(None),
                        Some(toml::Value::String(s)) => Ok(Some(expand(s)?)),
                        Some(_) => Err(CallistoError::Type {
                            key: format!("tools.generic.{name}.{key}"),
                            expected: "a string",
                        }),
                    }
                };
                tools.push(GenericTool {
                    name: name.clone(),
                    directory: path(field("directory")?.unwrap_or_default()),
                    executable: field("executable")?.unwrap_or_default(),
                });
            }
        }
        let mut resources = BTreeMap::new();
        for key in [
            "levels",
            "map16",
            "shared_palettes",
            "overworld",
            "titlescreen",
            "credits",
            "global_exanimation",
        ] {
            if let Some(value) = string(&["resources", key])? {
                resources.insert(key.to_owned(), path(value));
            }
        }
        Ok(Self {
            root,
            config: config.to_path_buf(),
            build_order: strings(&["orders", "build_order"])?,
            initial_patch: string(&["resources", "initial_patch"])?.map(path),
            patches: strings(&["resources", "patches"])?
                .into_iter()
                .map(path)
                .collect(),
            modules: strings(&["resources", "modules"])?
                .into_iter()
                .map(path)
                .collect(),
            callisto_header: string(&["resources", "callisto_header"])?.map(path),
            tools,
            resources,
            text_map16: matches!(
                get(&["settings", "use_text_map16_format"]),
                Some(toml::Value::Boolean(true))
            ),
            output_rom: string(&["output", "output_rom"])?.map(path),
        })
    }

    /// The tool of `kind` the build order runs, if any.
    pub fn tool(&self, kind: ToolKind) -> Option<&GenericTool> {
        self.tools
            .iter()
            .find(|t| t.kind() == Some(kind) && self.build_order.contains(&t.name))
    }

    /// Whether the build order has `step`.
    pub fn builds(&self, step: &str) -> bool {
        self.build_order.iter().any(|s| s == step)
    }
}

/// A folder under `dir`, down to `depth` levels, whose `.toml` files hold
/// `[orders] build_order`.
fn find_config(dir: &Path, depth: usize) -> Result<Option<PathBuf>, CallistoError> {
    let io = |source| CallistoError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut folders = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(io)?
        .collect::<Result<_, _>>()
        .map_err(io)?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "profiles"
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                folders.push(path);
            }
        } else if path.extension().is_some_and(|e| e == "toml")
            && fs::read_to_string(&path)
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .is_some_and(|t| t.get("orders").and_then(|o| o.get("build_order")).is_some())
        {
            return Ok(Some(dir.to_path_buf()));
        }
    }
    if depth == 0 {
        return Ok(None);
    }
    for folder in folders {
        if let Some(found) = find_config(&folder, depth - 1)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Every `.toml` file in `dir`, merged table by table.
fn merged(dir: &Path) -> Result<toml::Table, CallistoError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| CallistoError::Io { path, source }
    };
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(io(dir))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    let mut out = toml::Table::new();
    for file in files {
        let text = fs::read_to_string(&file).map_err(io(&file))?;
        let table: toml::Table =
            text.parse()
                .map_err(|e: toml::de::Error| CallistoError::Toml {
                    path: file.clone(),
                    message: e.message().to_owned(),
                })?;
        merge(&mut out, table);
    }
    Ok(out)
}

fn merge(into: &mut toml::Table, from: toml::Table) {
    for (key, value) in from {
        match (into.get_mut(&key), value) {
            (Some(toml::Value::Table(a)), toml::Value::Table(b)) => merge(a, b),
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

/// `[variables]`, each expanded in terms of the others.
fn variables(table: &toml::Table) -> Result<BTreeMap<String, String>, CallistoError> {
    let mut raw = BTreeMap::new();
    if let Some(toml::Value::Table(vars)) = table.get("variables") {
        for (name, value) in vars {
            let toml::Value::String(s) = value else {
                return Err(CallistoError::Type {
                    key: format!("variables.{name}"),
                    expected: "a string",
                });
            };
            raw.insert(name.clone(), s.clone());
        }
    }
    let mut out = BTreeMap::new();
    for name in raw.keys() {
        out.insert(name.clone(), resolve(name, &raw, &mut Vec::new())?);
    }
    Ok(out)
}

fn resolve(
    name: &str,
    raw: &BTreeMap<String, String>,
    stack: &mut Vec<String>,
) -> Result<String, CallistoError> {
    if stack.iter().any(|s| s == name) {
        return Err(CallistoError::Variable(name.to_owned()));
    }
    let value = raw
        .get(name)
        .ok_or_else(|| CallistoError::Variable(name.to_owned()))?;
    stack.push(name.to_owned());
    let mut lookup = |n: &str| resolve(n, raw, stack);
    let out = substitute(value, &mut lookup);
    stack.pop();
    out
}

/// `text` with every `{name}` replaced by its variable.
fn expand(text: &str, variables: &BTreeMap<String, String>) -> Result<String, CallistoError> {
    substitute(text, &mut |name| {
        variables
            .get(name)
            .cloned()
            .ok_or_else(|| CallistoError::Variable(name.to_owned()))
    })
}

fn substitute(
    text: &str,
    lookup: &mut dyn FnMut(&str) -> Result<String, CallistoError>,
) -> Result<String, CallistoError> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
        } else if let Some(stripped) = tail.strip_prefix('{')
            && let Some(end) = stripped.find('}')
        {
            out.push_str(&lookup(&stripped[..end])?);
            rest = &stripped[end + 1..];
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// A configuration's path, with Windows separators made `/`.
fn native(path: &str) -> String {
    path.replace('\\', "/")
}

/// `path` without `.` and with each `..` taking the folder before it.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_expand_in_terms_of_each_other() {
        let raw: BTreeMap<String, String> = [
            ("a", "x"),
            ("b", "{a}/y"),
            ("c", "{{b}} {b}"),
            ("loop", "{loop}"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let table: toml::Table = "[variables]".parse().unwrap();
        assert!(variables(&table).unwrap().is_empty());
        assert_eq!(resolve("c", &raw, &mut Vec::new()).unwrap(), "{b} x/y");
        assert!(resolve("loop", &raw, &mut Vec::new()).is_err());
        assert!(resolve("missing", &raw, &mut Vec::new()).is_err());
    }

    #[test]
    fn a_configuration_split_across_files_reads_as_one() {
        let dir = std::env::temp_dir().join(format!("kobo-callisto-{}", std::process::id()));
        let config = dir.join("tools").join("Callisto");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("variables.toml"),
            "[variables]\nname = \"hack\"\npatches_folder = \"resources/patches\"\n",
        )
        .unwrap();
        fs::write(
            config.join("project.toml"),
            "[settings]\nproject_root = \"../../\"\n[output]\noutput_rom = \"build/{name}.smc\"\n",
        )
        .unwrap();
        fs::write(
            config.join("build.toml"),
            "[orders]\nbuild_order = [\"Patches\", \"PIXI\"]\n\
             [resources]\npatches = [\"{patches_folder}/a.asm\", \"{patches_folder}\\\\b.asm\"]\n",
        )
        .unwrap();
        fs::write(
            config.join("tools.toml"),
            "[tools.generic.PIXI]\ndirectory = \"tools/pixi\"\nexecutable = \"pixi.exe\"\n\
             [tools.generic.GPS]\ndirectory = \"tools/gps\"\nexecutable = \"gps.exe\"\n\
             [resources]\nlevels = \"export/levels\"\n",
        )
        .unwrap();
        let project = Project::find(&dir).unwrap();
        assert_eq!(project.config, config);
        assert_eq!(project.root, dir);
        assert_eq!(project.output_rom, Some(PathBuf::from("build/hack.smc")));
        assert_eq!(
            project.patches,
            [
                PathBuf::from("resources/patches/a.asm"),
                PathBuf::from("resources/patches/b.asm")
            ]
        );
        assert_eq!(project.resources["levels"], PathBuf::from("export/levels"));
        assert_eq!(
            project.tool(ToolKind::Pixi).map(|t| &t.directory),
            Some(&PathBuf::from("tools/pixi"))
        );
        // GPS is configured but not in the build order.
        assert!(project.tool(ToolKind::Gps).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
