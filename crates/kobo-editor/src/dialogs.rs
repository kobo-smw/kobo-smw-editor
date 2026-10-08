//! The system's file dialogs, run off the window's thread so that the
//! window goes on answering the desktop while one is open (a dialog that
//! blocks it has the system call the editor not responding). What was
//! chosen comes back on a later frame, and goes where it was asked for.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use rfd::AsyncFileDialog;

use crate::app::App;

/// What a dialog's choice is for.
pub enum Purpose {
    /// The clean ROM.
    CleanRom,
    /// A project to open from the start screen.
    OpenProject,
    /// A project to switch to, once unsaved edits are dealt with.
    SwitchProject,
    /// A hack to make a new project from.
    Hack,
    /// The folder for a new project.
    NewFolder(crate::start::NewProject),
    /// A Lunar Magic level to import.
    Mwl,
    /// Where to save the open level's picture.
    Picture,
    /// An indexed PNG to draw a graphics file from.
    GraphicsImport(kobo_core::edit::GraphicsFile),
    /// Where to save a graphics file as an indexed PNG.
    GraphicsExport(kobo_core::edit::GraphicsFile),
}

/// What a dialog chooses.
pub enum Pick {
    File,
    Folder,
    Save,
}

#[derive(Default)]
pub struct Dialogs {
    pending: Option<(Purpose, Receiver<Option<PathBuf>>)>,
}

impl Dialogs {
    /// Whether a dialog is open.
    pub fn open(&self) -> bool {
        self.pending.is_some()
    }
}

/// Opens `dialog`, unless one is open already.
pub fn ask(app: &mut App, purpose: Purpose, pick: Pick, dialog: AsyncFileDialog) {
    // The tests open nothing.
    if app.dialogs.open() || cfg!(test) {
        return;
    }
    let (send, receive) = mpsc::channel();
    let ctx = app.ctx().clone();
    std::thread::spawn(move || {
        let chosen = pollster::block_on(async move {
            match pick {
                Pick::File => dialog.pick_file().await,
                Pick::Folder => dialog.pick_folder().await,
                Pick::Save => dialog.save_file().await,
            }
        });
        let _ = send.send(chosen.map(|file| file.path().to_path_buf()));
        ctx.request_repaint();
    });
    app.dialogs.pending = Some((purpose, receive));
}

/// Takes a dialog's choice once it is made.
pub fn poll(app: &mut App) {
    let Some((_, receive)) = &app.dialogs.pending else {
        return;
    };
    let chosen = match receive.try_recv() {
        Ok(chosen) => chosen,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => None,
    };
    let (purpose, _) = app.dialogs.pending.take().expect("checked");
    let Some(path) = chosen else { return };
    match purpose {
        Purpose::CleanRom => crate::start::choose_rom(app, &path),
        Purpose::OpenProject => {
            let ctx = app.ctx().clone();
            crate::start::switch(app, &ctx, Some(path));
        }
        Purpose::SwitchProject => app.switch_project(Some(path)),
        Purpose::Hack => crate::start::hack_chosen(app, path),
        Purpose::NewFolder(new) => crate::start::folder_chosen(app, new, path),
        Purpose::Mwl => app.import_mwl(&path),
        Purpose::Picture => app.save_picture(&path),
        Purpose::GraphicsImport(file) => app.import_graphics(file, &path),
        Purpose::GraphicsExport(file) => app.export_graphics(file, &path),
    }
}
