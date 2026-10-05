//! Templates for new projects (`kobo new --template <name>`): widely used
//! baseroms, made into a project on the user's machine. A template is a
//! recipe (`templates/*.toml`, compiled in): where to download the baserom's
//! own release, its SHA-256, the steps its setup takes before a first build,
//! and how Kobo builds it. Kobo carries nothing of the baserom itself:
//! most baseroms have no licence, their resources staying their authors'
//! (docs/build.md, "No base"), so the user's machine fetches the release
//! from where its authors publish it, checked against the recipe, and the
//! import (`import::import_callisto`) turns it into the user's project.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use thiserror::Error;

use crate::import::{self, ImportError, Report};
use crate::rom::Rom;
use crate::tools::ToolError;
use crate::tools::pinned::PinnedBuild;

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("no template `{0}`; `kobo new --list` lists them")]
    Unknown(String),
    #[error(transparent)]
    Fetch(#[from] ToolError),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("setting the template up: {0} is not in its release")]
    Setup(PathBuf),
    #[error(transparent)]
    Import(#[from] ImportError),
}

/// A template's recipe.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// What the baserom is called.
    pub title: String,
    pub version: String,
    pub homepage: String,
    /// Who holds the rights to what it has, and where its credits are.
    pub rights: String,
    /// The release, downloaded from `url` + `build.file`.
    pub url: String,
    pub build: PinnedBuild,
    /// What the baserom's own setup does before its first build.
    #[serde(default)]
    pub setup: Vec<Setup>,
    /// Files and folders of the release a project leaves out: scripts for
    /// the baserom's own build, say.
    #[serde(default)]
    pub exclude: Vec<PathBuf>,
    /// The PIXI version the baserom's sprites are written for, if not the
    /// one Kobo pins by default.
    pub pixi: Option<String>,
}

/// One step of a baserom's setup: a file of its release copied to
/// another place in it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub copy: PathBuf,
    pub to: PathBuf,
}

const RECIPES: &[(&str, &str)] = &[("rhr", include_str!("templates/rhr.toml"))];

/// Every template, by name.
pub fn templates() -> &'static BTreeMap<String, Template> {
    static TEMPLATES: OnceLock<BTreeMap<String, Template>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        RECIPES
            .iter()
            .map(|(name, text)| {
                let recipe: Template =
                    toml::from_str(text).unwrap_or_else(|e| panic!("templates/{name}.toml: {e}"));
                ((*name).to_owned(), recipe)
            })
            .collect()
    })
}

/// The template called `name`.
pub fn template(name: &str) -> Result<&'static Template, TemplateError> {
    templates()
        .get(name)
        .ok_or_else(|| TemplateError::Unknown(name.to_owned()))
}

impl Template {
    /// The release's folder, downloaded and unpacked into the tool cache
    /// if it is not there yet (never with `KOBO_OFFLINE`).
    pub fn fetch(&self) -> Result<PathBuf, TemplateError> {
        Ok(crate::tools::pinned::fetch_cached(
            &format!("{}{}", self.url, self.build.file),
            &self.build,
        )?)
    }

    /// Makes `dir` a project from the template, against the clean ROM.
    pub fn create(&self, clean: &Rom, dir: &Path) -> Result<Report, TemplateError> {
        let release = self.fetch()?;
        // The setup's steps change the release, which the cache keeps as
        // it came: they are taken on a copy.
        let work = std::env::temp_dir().join(format!(
            "kobo-template-{}-{}",
            std::process::id(),
            self.build.root
        ));
        let _ = fs::remove_dir_all(&work);
        let result = (|| {
            copy_tree(&release, &work)?;
            for step in &self.setup {
                let from = work.join(&step.copy);
                if !from.is_file() {
                    return Err(TemplateError::Setup(step.copy.clone()));
                }
                let to = work.join(&step.to);
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent).map_err(|source| TemplateError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::copy(&from, &to).map_err(|source| TemplateError::Io {
                    path: to.clone(),
                    source,
                })?;
            }
            let options = import::CallistoOptions {
                pixi_version: self.pixi.clone(),
                origin: Some(format!(
                    "{} {} ({}), from {}{}",
                    self.title, self.version, self.homepage, self.url, self.build.file
                )),
                rights: Some(self.rights.clone()),
                exclude: self.exclude.clone(),
            };
            Ok(import::import_callisto(&work, clean, dir, &options)?)
        })();
        let _ = fs::remove_dir_all(&work);
        result
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), TemplateError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| TemplateError::Io { path, source }
    };
    fs::create_dir_all(to).map_err(io(to))?;
    for entry in fs::read_dir(from).map_err(io(from))? {
        let entry = entry.map_err(io(from))?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            fs::copy(&path, &target).map_err(io(&path))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recipe_reads() {
        assert!(!templates().is_empty());
        for (name, recipe) in templates() {
            assert!(recipe.url.ends_with('/'), "{name}: url must be a folder");
            assert_eq!(recipe.build.sha256.len(), 64, "{name}: sha256");
        }
    }
}
