//! A project open for editing: what it builds into, and its levels'
//! pictures from that build.

use std::collections::BTreeMap;
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
    /// Levels whose build failed, why, and what they held then: shared by
    /// every copy, so that a build leaves them out at once until they
    /// change ([`Workspace::build_leaving_out`]).
    broken: Arc<std::sync::Mutex<BTreeMap<u16, (String, Level)>>>,
}

/// A level's picture, and the ROM it was rendered from.
pub struct Preview {
    pub rom: Arc<Rom>,
    pub render: LevelRender,
    /// Other levels whose build failed, and why: the build the picture
    /// comes from has the clean ROM's in their place.
    pub left_out: Vec<(u16, String)>,
}

impl Workspace {
    /// Loads the project in `dir`, to build onto `clean` with the user's
    /// stage cache.
    pub fn open(dir: &Path, clean: Arc<Rom>) -> Result<Self, WorkspaceError> {
        Ok(Self {
            clean,
            project: Arc::new(Project::load(dir)?),
            cache: Cache::user(),
            broken: Default::default(),
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

    /// A secondary entrance number for a new entrance into level `level`:
    /// one the clean ROM's tables do not use, nor any level of the
    /// project, and in the game's format (Kobo's builds write entrances
    /// so up to `1FF`) one whose bit 8 is the level's, which the number
    /// gives. `None` when the level's half of the 512 is full.
    pub fn free_entrance(&self, level: u16) -> Option<u16> {
        let clean = crate::level::read_entrances(&self.clean).ok()?;
        let format = crate::level::LevelFormat::of(&self.clean);
        let taken: std::collections::BTreeSet<u16> = self
            .project
            .levels
            .iter()
            .flat_map(|(_, l)| l.entrances.iter().map(|e| e.id))
            .collect();
        let bank = level & 0x100;
        (bank..bank + 0x100).find(|&id| {
            !taken.contains(&id)
                && clean
                    .get(usize::from(id))
                    .is_some_and(|bytes| !bytes.in_use(format))
        })
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

    /// Takes level `number` out of the project: its file is deleted and
    /// the manifest no longer lists it, so it builds as the clean ROM has
    /// it ([`crate::import::remove_level`]).
    pub fn remove_level(&mut self, number: u16) -> Result<PathBuf, WorkspaceError> {
        if !self.project.manifest.levels.contains_key(&number) {
            return Err(WorkspaceError::NoLevel(number));
        }
        let path = crate::import::remove_level(&self.project.root, number)?;
        let project = Arc::make_mut(&mut self.project);
        project.manifest.levels.remove(&number);
        project.levels.retain(|(n, _)| *n != number);
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

    /// Builds the project as it is in memory, telling `report` of each
    /// stage as it goes.
    pub fn build_reporting(
        &self,
        report: &mut dyn FnMut(build::Stage, build::StageEvent),
    ) -> Result<Rom, WorkspaceError> {
        Ok(build::build_reporting(
            &self.clean,
            &self.project,
            self.cache.as_ref(),
            report,
        )?)
    }

    /// Builds the project, leaving out each level whose build fails (but
    /// `keep`, whose failure fails the build), as the clean ROM has it;
    /// with the levels left out, and why.
    pub fn build_leaving_out(
        &self,
        keep: Option<u16>,
    ) -> Result<(Rom, Vec<(u16, String)>), WorkspaceError> {
        let mut copy = self.clone();
        let mut left_out: Vec<(u16, String)> = Vec::new();
        let leave_out = |copy: &mut Workspace, level: u16| {
            let project = Arc::make_mut(&mut copy.project);
            project.levels.retain(|(n, _)| *n != level);
            project.manifest.levels.remove(&level);
        };
        // What failed before and has not changed since fails again.
        let known: Vec<(u16, String)> = {
            let broken = self.broken.lock().expect("no build panics holding it");
            broken
                .iter()
                .filter(|(n, (_, held))| Some(**n) != keep && self.level(**n) == Some(held))
                .map(|(n, (why, _))| (*n, why.clone()))
                .collect()
        };
        for (level, why) in known {
            leave_out(&mut copy, level);
            left_out.push((level, why));
        }
        loop {
            match copy.build() {
                Ok(rom) => return Ok((rom, left_out)),
                Err(WorkspaceError::Build(BuildError::Level { level, message }))
                    if Some(level) != keep && !left_out.iter().any(|(n, _)| *n == level) =>
                {
                    if let Some(held) = self.level(level) {
                        let mut broken = self.broken.lock().expect("no build panics holding it");
                        broken.insert(level, (message.clone(), held.clone()));
                    }
                    left_out.push((level, message));
                    leave_out(&mut copy, level);
                }
                Err(e) => return Err(e),
            }
        }
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
        // Another level that does not build is left out, as the clean
        // ROM's, so that this one still draws.
        let (rom, left_out) = self.build_leaving_out(Some(level))?;
        let rom = Arc::new(rom);
        operation.check().map_err(RenderError::from)?;
        let render = render::render_level_with_control(&rom, level, options, operation)?;
        Ok(Preview {
            rom,
            render,
            left_out,
        })
    }
}

fn set(project: &mut Project, number: u16, level: Level) {
    match project.levels.iter_mut().find(|(n, _)| *n == number) {
        Some((_, old)) => *old = level,
        None => project.levels.push((number, level)),
    }
}
