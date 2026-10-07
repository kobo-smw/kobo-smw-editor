//! Small pictures of levels, for the level list and the overview: three
//! screens around where the player starts, at a third of the size, drawn
//! on a worker thread from a build of the project as they are wanted,
//! those wanted first drawn first.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use kobo_core::edit::Workspace;
use kobo_core::image::RgbImage;
use kobo_core::operation::Operation;
use kobo_core::render::{RenderOptions, Sprites};

/// What a picture shows of its level: three screens wide, the level's
/// height, around the start; then a third of the size.
const SHOWN: u32 = 768;
const SHRINK: u32 = 3;

/// How long after a level changed its picture is drawn again, so that a
/// drag does not have it drawn at every step.
const SETTLE: Duration = Duration::from_millis(1500);

/// A level's picture, or why it does not draw.
type Drawn = (u16, Result<RgbImage, String>);

#[derive(Default)]
pub struct Thumbnails {
    /// Pictures, and whether the level changed since.
    pictures: HashMap<u16, (egui::TextureHandle, bool)>,
    failed: HashMap<u16, String>,
    /// The levels wanted, first first: the worker takes from the front.
    queue: Arc<Mutex<Vec<u16>>>,
    /// The levels wanted this frame, so far.
    wanted: Vec<u16>,
    worker: Option<(Operation, Receiver<Drawn>)>,
    changed_at: Option<Instant>,
}

impl Thumbnails {
    /// Forgets every picture, after the project changed.
    pub fn forget(&mut self) {
        self.stop();
        self.pictures.clear();
        self.failed.clear();
    }

    /// Level `number` changed: its picture stays until it is drawn again.
    pub fn changed(&mut self, number: u16) {
        // The worker's build is of the level as it was.
        self.stop();
        self.failed.remove(&number);
        if let Some((_, stale)) = self.pictures.get_mut(&number) {
            *stale = true;
        }
        self.changed_at = Some(Instant::now());
    }

    fn stop(&mut self) {
        if let Some((operation, _)) = self.worker.take() {
            operation.cancel();
        }
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn picture(&self, number: u16) -> Option<&egui::TextureHandle> {
        self.pictures.get(&number).map(|(texture, _)| texture)
    }

    pub fn failed(&self, number: u16) -> Option<&str> {
        self.failed.get(&number).map(String::as_str)
    }

    /// Whether level `number` has its picture as it is, or is known not to
    /// draw.
    pub fn done(&self, number: u16) -> bool {
        self.failed.contains_key(&number) || self.pictures.get(&number).is_some_and(|(_, s)| !s)
    }

    /// Asks for level `number`'s picture this frame, after those asked
    /// for before it.
    pub fn want(&mut self, number: u16) {
        if !self.done(number) && !self.wanted.contains(&number) {
            self.wanted.push(number);
        }
    }

    /// Asks for level `number`'s picture before any other.
    pub fn want_first(&mut self, number: u16) {
        if !self.done(number) {
            self.wanted.retain(|&n| n != number);
            self.wanted.insert(0, number);
        }
    }

    /// Whether a picture asked for this frame is not in yet.
    pub fn waiting(&self) -> bool {
        self.wanted.iter().any(|&n| !self.done(n))
    }

    /// Takes what the worker drew, hands it this frame's wants, and
    /// starts one when something is wanted and none runs.
    pub fn poll(&mut self, ctx: &egui::Context, workspace: Option<&Workspace>) {
        let mut finished = false;
        if let Some((_, receive)) = &self.worker {
            loop {
                match receive.try_recv() {
                    Ok((number, Ok(image))) => {
                        let size = [image.width as usize, image.height as usize];
                        let rgb: Vec<u8> = image.pixels.iter().flatten().copied().collect();
                        let texture = ctx.load_texture(
                            format!("thumbnail-{number:03X}"),
                            egui::ColorImage::from_rgb(size, &rgb),
                            egui::TextureOptions::LINEAR,
                        );
                        self.pictures.insert(number, (texture, false));
                    }
                    Ok((number, Err(e))) => {
                        self.pictures.remove(&number);
                        self.failed.insert(number, e);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished {
            self.worker = None;
        }
        let wanted = std::mem::take(&mut self.wanted);
        if let Ok(mut queue) = self.queue.lock() {
            *queue = wanted.iter().copied().filter(|&n| !self.done(n)).collect();
        }
        let settled = self.changed_at.is_none_or(|at| at.elapsed() >= SETTLE);
        if !settled && wanted.iter().any(|n| !self.done(*n)) {
            ctx.request_repaint_after(SETTLE);
        }
        if self.worker.is_none()
            && settled
            && !wanted.is_empty()
            && let Some(workspace) = workspace
        {
            self.start(workspace.clone(), ctx.clone());
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Draws the levels wanted, one after another, until none are.
    fn start(&mut self, workspace: Workspace, ctx: egui::Context) {
        let (send, receive) = mpsc::channel();
        let operation = Operation::default();
        let control = operation.clone();
        let queue = self.queue.clone();
        std::thread::spawn(move || {
            let take = || {
                queue
                    .lock()
                    .ok()
                    .and_then(|mut q| (!q.is_empty()).then(|| q.remove(0)))
            };
            let (rom, left_out) = match workspace.build_leaving_out(None) {
                Ok(built) => built,
                Err(e) => {
                    while let Some(number) = take() {
                        let _ = send.send((number, Err(e.to_string())));
                    }
                    ctx.request_repaint();
                    return;
                }
            };
            let options = RenderOptions {
                sprites: Sprites::Hidden,
                player: false,
                hidden_layers: 0,
            };
            while let Some(number) = take() {
                if control.check().is_err() {
                    break;
                }
                if let Some((_, why)) = left_out.iter().find(|(n, _)| *n == number) {
                    let _ = send.send((number, Err(format!("does not build: {why}"))));
                    continue;
                }
                let picture =
                    kobo_core::render::render_level_with_control(&rom, number, options, &control)
                        .map(|render| {
                            let x = u32::from(render.level.ram.u16(kobo_core::ram::PLAYER_X));
                            picture(&render.image, x)
                        })
                        .map_err(|e| e.to_string());
                if control.check().is_err() || send.send((number, picture)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        self.worker = Some((operation, receive));
    }
}

/// A third of `image`'s part around `x`: three screens of it, whole
/// pixels averaged in threes.
fn picture(image: &RgbImage, x: u32) -> RgbImage {
    let width = SHOWN.min(image.width);
    let left = x.saturating_sub(width / 3).min(image.width - width);
    let (w, h) = (width / SHRINK, image.height / SHRINK);
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..SHRINK {
                for dx in 0..SHRINK {
                    let p = image.pixels
                        [((y * SHRINK + dy) * image.width + left + x * SHRINK + dx) as usize];
                    for c in 0..3 {
                        sum[c] += u32::from(p[c]);
                    }
                }
            }
            let n = SHRINK * SHRINK;
            out.pixels[(y * w + x) as usize] = sum.map(|s| (s / n) as u8);
        }
    }
    out
}

/// Paints `texture` to cover `rect`, keeping the bottom of the level,
/// where the ground is.
pub fn paint_cover(painter: &egui::Painter, texture: &egui::TextureHandle, rect: egui::Rect) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).max(rect.height() / size.y);
    let shown = size * scale;
    let uv_w = (rect.width() / shown.x).min(1.0);
    let uv_h = (rect.height() / shown.y).min(1.0);
    let uv = egui::Rect::from_min_max(
        egui::pos2((1.0 - uv_w) / 2.0, 1.0 - uv_h),
        egui::pos2((1.0 + uv_w) / 2.0, 1.0),
    );
    painter.image(texture.id(), rect, uv, egui::Color32::WHITE);
}
