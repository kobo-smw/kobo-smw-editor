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
    pub took: Duration,
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

pub struct Previewer {
    jobs: Sender<Job>,
    done: Receiver<Done>,
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
                    let result = run(&job);
                    if outbox.send(Done { id: job.id, result }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the preview thread starts");
        Self {
            jobs,
            done,
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
        while let Ok(done) = self.done.try_recv() {
            if self.current.as_ref().is_some_and(|(id, _)| *id == done.id) {
                self.current = None;
                newest = Some(done);
            }
        }
        newest
    }
}

fn run(job: &Job) -> Result<Rendered, String> {
    let started = Instant::now();
    let preview = job
        .workspace
        .preview(job.level, job.options, &job.operation)
        .map_err(|e| e.to_string())?;
    let render = preview.render;
    let image = &render.image;
    let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
    let size = [image.width as usize, image.height as usize];
    Ok(Rendered {
        image: egui::ColorImage::from_rgb(size, &rgb),
        diagnostics: expand::summarize(&render.diagnostics),
        loaded: render.level,
        sprites: render.sprites,
        took: started.elapsed(),
    })
}
