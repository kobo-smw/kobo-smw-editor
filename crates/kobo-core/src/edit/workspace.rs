//! A project open for editing: what it builds into, and its levels'
//! pictures from that build.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::build::{self, BuildError, Cache, Project};
use crate::operation::Operation;
use crate::render::{self, LevelRender, RenderError, RenderOptions};
use crate::rom::Rom;
use crate::source::level::Level;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Build(#[from] BuildError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("the project has no level {0:03X}")]
    NoLevel(u16),
    #[error("the project has level {0:03X} already")]
    HasLevel(u16),
    #[error(transparent)]
    Import(Box<crate::import::ImportError>),
}

impl From<crate::import::ImportError> for WorkspaceError {
    fn from(e: crate::import::ImportError) -> Self {
        Self::Import(Box::new(e))
    }
}

/// A project in memory, with the clean ROM it builds onto. Cloning one is
/// cheap, so a worker thread can build and render a copy while the
/// editor changes the original.
#[derive(Clone)]
pub struct Workspace {
    clean: Arc<Rom>,
    project: Arc<Project>,
    cache: Option<Cache>,
}

/// A level's picture, and the ROM it was rendered from.
pub struct Preview {
    pub rom: Arc<Rom>,
    pub render: LevelRender,
}

impl Workspace {
    /// Loads the project in `dir`, to build onto `clean` with the user's
    /// stage cache.
    pub fn open(dir: &Path, clean: Arc<Rom>) -> Result<Self, WorkspaceError> {
        Ok(Self {
            clean,
            project: Arc::new(Project::load(dir)?),
            cache: Cache::user(),
        })
    }

    /// Builds without a stage cache, for tests.
    pub fn without_cache(mut self) -> Self {
        self.cache = None;
        self
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn clean(&self) -> &Arc<Rom> {
        &self.clean
    }

    /// Loads the project from its folder again, after a file other than
    /// an open level's changed, keeping the levels in `keep` as they are
    /// in memory.
    pub fn reload(&mut self, keep: &[(u16, Level)]) -> Result<(), WorkspaceError> {
        let mut project = Project::load(&self.project.root)?;
        for (number, level) in keep {
            set(&mut project, *number, level.clone());
        }
        self.project = Arc::new(project);
        Ok(())
    }

    /// The level numbers the project defines, in order.
    pub fn levels(&self) -> impl Iterator<Item = u16> + '_ {
        self.project.manifest.levels.keys().copied()
    }

    /// The file a level is defined in.
    pub fn level_path(&self, number: u16) -> Option<PathBuf> {
        let file = self.project.manifest.levels.get(&number)?;
        Some(self.project.root.join(file))
    }

    /// The level as the workspace has it.
    pub fn level(&self, number: u16) -> Option<&Level> {
        self.project
            .levels
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, level)| level)
    }

    /// A level as the clean ROM has it, which is what a level the project
    /// does not list builds as.
    pub fn clean_level(&self, number: u16) -> Result<Level, WorkspaceError> {
        Ok(crate::import::read_level(&self.clean, number)?.0)
    }

    /// Adds level `number` to the project, as `level`: its file is
    /// written and the manifest lists it ([`crate::import::add_level`]).
    /// The rest of the project in memory stays as it is. Returns the
    /// level's file.
    pub fn add_level(&mut self, number: u16, level: &Level) -> Result<PathBuf, WorkspaceError> {
        if self.project.manifest.levels.contains_key(&number) {
            return Err(WorkspaceError::HasLevel(number));
        }
        let path = crate::import::add_level(&self.project.root, number, level)?;
        let project = Arc::make_mut(&mut self.project);
        let file = path
            .strip_prefix(&project.root)
            .map_or_else(|_| path.clone(), Path::to_path_buf);
        project.manifest.levels.insert(number, file);
        set(project, number, level.clone());
        Ok(path)
    }

    /// Puts the level's state in the editor in the project, for the next
    /// build.
    pub fn set_level(&mut self, number: u16, level: &Level) {
        if self.level(number) != Some(level) {
            set(Arc::make_mut(&mut self.project), number, level.clone());
        }
    }

    /// Builds the project as it is in memory.
    pub fn build(&self) -> Result<Rom, WorkspaceError> {
        Ok(build::build_cached(
            &self.clean,
            &self.project,
            self.cache.as_ref(),
        )?)
    }

    /// Builds the project and renders `level` from the build, under
    /// `operation`, which can cancel the render.
    pub fn preview(
        &self,
        level: u16,
        options: RenderOptions,
        operation: &Operation,
    ) -> Result<Preview, WorkspaceError> {
        if !self.project.manifest.levels.contains_key(&level) {
            return Err(WorkspaceError::NoLevel(level));
        }
        let rom = Arc::new(self.build()?);
        operation.check().map_err(RenderError::from)?;
        let render = render::render_level_with_control(&rom, level, options, operation)?;
        Ok(Preview { rom, render })
    }
}

fn set(project: &mut Project, number: u16, level: Level) {
    match project.levels.iter_mut().find(|(n, _)| *n == number) {
        Some((_, old)) => *old = level,
        None => project.levels.push((number, level)),
    }
}
