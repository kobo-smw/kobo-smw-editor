//! Building and rendering the open level on a worker thread, so the
//! window stays responsive. A newer request cancels the one in flight;
//! only the newest result is ever shown.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use kobo_core::edit::Workspace;
use kobo_core::expand::{self, LoadedLevel};
use kobo_core::operation::Operation;
use kobo_core::render::RenderOptions;
use kobo_core::video::SpriteScene;

/// A level's picture as the worker made it.
pub struct Rendered {
    pub image: egui::ColorImage,
    pub loaded: LoadedLevel,
    pub sprites: Option<SpriteScene>,
    /// One line per distinct thing a pass gave up on.
    pub diagnostics: Vec<String>,
    /// The sprites whose pass gave up, by tile, and why.
    pub failed_sprites: Vec<(i32, i32, String)>,
    /// Other levels left out of the build for the picture, and why.
    pub left_out: Vec<(u16, String)>,
    /// Where the player enters the level, each way he can.
    pub entries: Vec<Entry>,
    pub took: Duration,
}

/// A way into the level, and where the player stands, in level pixels.
#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: EntryKind,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Main,
    /// The midway entrance as the game makes it: the main entrance's
    /// place on the midway screen.
    Midway,
    Secondary(u16),
}

/// A request's outcome, by the request's number.
pub struct Done {
    pub id: u64,
    pub result: Result<Rendered, String>,
}

struct Job {
    id: u64,
    workspace: Workspace,
    level: u16,
    options: RenderOptions,
    operation: Operation,
}

/// What the worker sends back: a picture, and later the secondary
/// entrances' places, which take a load each.
enum Message {
    Done(Box<Done>),
    Entries(u64, Vec<Entry>),
}

pub struct Previewer {
    jobs: Sender<Job>,
    done: Receiver<Message>,
    /// The secondary entrances of the newest picture, once found.
    entries: Option<(u64, Vec<Entry>)>,
    /// The newest request, and what can cancel it.
    current: Option<(u64, Operation)>,
    next: u64,
}

impl Previewer {
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel();
        thread::Builder::new()
            .name("kobo-preview".into())
            .spawn(move || {
                while let Ok(mut job) = inbox.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = inbox.try_recv() {
                        job = newer;
                    }
                    let (result, later) = run(&job);
                    if outbox
                        .send(Message::Done(Box::new(Done { id: job.id, result })))
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                    // The entrances only while nothing newer waits.
                    if let Some((rom, loaded)) = later {
                        let entries = secondary_entries(&job, &rom, &loaded);
                        if job.operation.check().is_ok() {
                            let _ = outbox.send(Message::Entries(job.id, entries));
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .expect("the preview thread starts");
        Self {
            jobs,
            done,
            entries: None,
            current: None,
            next: 1,
        }
    }

    /// Asks for `level` of `workspace` as it is now, cancelling the
    /// request before. Returns the request's number.
    pub fn request(&mut self, workspace: &Workspace, level: u16, options: RenderOptions) -> u64 {
        if let Some((_, operation)) = self.current.take() {
            operation.cancel();
        }
        let id = self.next;
        self.next += 1;
        let operation = Operation::default();
        self.current = Some((id, operation.clone()));
        let job = Job {
            id,
            workspace: workspace.clone(),
            level,
            options,
            operation,
        };
        // The worker outlives the window's state, so this only fails at
        // exit.
        let _ = self.jobs.send(job);
        id
    }

    /// Whether the newest request has not come back yet.
    pub fn busy(&self) -> bool {
        self.current.is_some()
    }

    /// The newest request's result, once it is in; older ones are
    /// dropped.
    pub fn poll(&mut self) -> Option<Done> {
        let mut newest = None;
        while let Ok(message) = self.done.try_recv() {
            match message {
                Message::Done(done) => {
                    if self.current.as_ref().is_some_and(|(id, _)| *id == done.id) {
                        self.current = None;
                        newest = Some(*done);
                    }
                }
                Message::Entries(id, entries) => self.entries = Some((id, entries)),
            }
        }
        newest
    }

    /// The secondary entrances found for request `id`, once they are.
    pub fn take_entries(&mut self) -> Option<(u64, Vec<Entry>)> {
        self.entries.take()
    }
}

/// A picture, and what finding the secondary entrances later needs.
type Ran = (
    Result<Rendered, String>,
    Option<(std::sync::Arc<kobo_core::Rom>, LoadedLevel)>,
);

fn run(job: &Job) -> Ran {
    let started = Instant::now();
    let preview = match job
        .workspace
        .preview(job.level, job.options, &job.operation)
    {
        Ok(preview) => preview,
        Err(e) => return (Err(e.to_string()), None),
    };
    let render = preview.render;
    let entries = entries(&job.workspace, job.level, &render.level);
    let image = &render.image;
    let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
    let size = [image.width as usize, image.height as usize];
    let later = (preview.rom, render.level.clone());
    let left_out = preview.left_out;
    let rendered = Rendered {
        image: egui::ColorImage::from_rgb(size, &rgb),
        diagnostics: expand::summarize(&render.diagnostics),
        failed_sprites: render
            .diagnostics
            .iter()
            .filter_map(|d| match d {
                expand::Diagnostic::Cpu {
                    pass: expand::Pass::Sprite { x, y, .. },
                    ..
                } => Some((*x, *y, d.to_string())),
                _ => None,
            })
            .collect(),
        loaded: render.level,
        sprites: render.sprites,
        entries,
        left_out,
        took: started.elapsed(),
    };
    (Ok(rendered), Some(later))
}

/// Where the player enters at once: the main entrance as the load left
/// him, and the midway on its screen.
fn entries(workspace: &Workspace, number: u16, loaded: &LoadedLevel) -> Vec<Entry> {
    use kobo_core::ram;
    let (x, y) = (
        i32::from(loaded.ram.u16(ram::PLAYER_X)),
        i32::from(loaded.ram.u16(ram::PLAYER_Y)),
    );
    let mut entries = vec![Entry {
        kind: EntryKind::Main,
        x,
        y,
    }];
    let Some(level) = workspace.level(number) else {
        return entries;
    };
    if !loaded.tiles.vertical {
        let screen = i32::from(level.entrance.midway_screen)
            | if level.settings.midway.screen_high {
                0x10
            } else {
                0
            };
        if level.settings.midway.separate.is_none() && screen > 0 {
            entries.push(Entry {
                kind: EntryKind::Midway,
                x: screen * 256 + x % 256,
                y,
            });
        }
    }
    entries
}

/// Each secondary entrance into the level, by loading it through each.
/// One that does not load is left out.
fn secondary_entries(job: &Job, rom: &kobo_core::Rom, _loaded: &LoadedLevel) -> Vec<Entry> {
    let Some(level) = job.workspace.level(job.level) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entrance in &level.entrances {
        if job.operation.check().is_err() {
            break;
        }
        if let Ok((x, y)) = kobo_core::expand::secondary_entry(rom, job.level, entrance.id) {
            found.push(Entry {
                kind: EntryKind::Secondary(entrance.id),
                x: i32::from(x),
                y: i32::from(y),
            });
        }
    }
    found
}
