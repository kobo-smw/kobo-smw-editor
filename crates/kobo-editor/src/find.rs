//! Finding objects and sprites in every level of the project
//! (`edit::find`), for the level list's search; kept for the last query
//! until the project changes.

use kobo_core::edit::find::{self, Found};

use crate::app::App;

#[derive(Default)]
pub struct FindState {
    /// What the last search found, and for which query; `None` once the
    /// project changed.
    found: Option<(String, Vec<Found>)>,
}

impl FindState {
    /// Forgets what was found, after the project changed.
    pub fn changed(&mut self) {
        self.found = None;
    }
}

/// What `query` finds in every level of the project.
pub fn found<'a>(app: &'a mut App, query: &str) -> &'a [Found] {
    let stale = app.find.found.as_ref().is_none_or(|(q, _)| q != query);
    if stale && let Some(workspace) = app.workspace() {
        let found = find::find(workspace, query);
        app.find.found = Some((query.to_string(), found));
    }
    app.find.found.as_ref().map_or(&[], |(_, found)| found)
}
