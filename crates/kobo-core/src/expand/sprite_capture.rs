//! Drawing a level's sprites by running the game's sprite engine.
//!
//! The work is a nest of passes, each starting from a snapshot of RAM:
//!
//! - a *column* per camera position that puts some sprite entry where the
//!   ROM's sprite loader looks, starting from the loaded level;
//! - *spawn rounds* within it, each calling the loader once and starting
//!   where the last one left off, after a first round in which the
//!   sprites of the columns the game loads before this one hold their
//!   slots;
//! - a *spot* pass per place a round put sprites, starting from that
//!   round's spawn, which runs the level loop until the sprite has drawn;
//! - a *baseline* pass per spot, again from the loaded level, whose
//!   objects are what the level draws without the sprite;
//! - and a *slotless* pass per column for the entries that took no slot.
//!
//! Every frame is followed by the ROM's NMI handler, so a pass's video
//! memory holds the tiles its sprite had uploaded. The capture runs on
//! video memory of its own, which each spot starts from the level's.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::diagnostics::{Diagnostic, Pass};
use super::load_flags::LoadFlags;
use super::machine::{Call, Machine};
use super::oam::{self, SCREEN_H, SCREEN_W};
use super::{ExpandError, LoadedLevel, routines};
use crate::cpu::CpuError;
use crate::operation::{Operation, Stage};
use crate::ram::{self, Ram};
use crate::rom::Rom;
use crate::sprites::{SpriteEntry, SpriteList};
use crate::video::{
    CapturedSprite, Character, DynamicObjects, OBJECT_CHARACTER_LEN, SpriteObject, SpriteScene,
    UndrawnSprite,
};

/// Most frames a sprite pass runs waiting for the sprite to initialise.
const SPRITE_FRAMES: usize = 8;
/// Most frames a sprite pass runs for a sprite that has drawn nothing yet.
pub const LATE_SPRITE_FRAMES: usize = 160;
/// Frames the slotless pass runs.
const SLOTLESS_FRAMES: usize = 2;
/// Most times the sprite loader is called for one column.
const SPAWN_ROUNDS: usize = 8;
/// Most times a sprite pass moves the camera after a sprite.
const CAMERA_MOVES: usize = 3;
/// How far behind the loading column sprites are still alive when the
/// camera scrolls there: the loader works `$120` pixels ahead of a camera
/// moving right or down, and most sprites erase themselves `$40` behind
/// it (`SubOffscreen`). The same holds the other way round (`$30` ahead,
/// `$130` behind), a column more or less.
const LIVE_BEHIND_LOADER: i32 = 0x160;
/// The columns the game loads on entering a level (`CODE_02AC5C`), from
/// left to right or top to bottom: from `$60` pixels before the camera,
/// 32 of them.
const ENTRY_COLUMNS: std::ops::Range<i32> = -0x60..0x1A0;
/// A cluster sprite the game draws at its position minus layer 2's, both
/// in eight bits, so that it rides on layer 2 and repeats every 256
/// pixels along it. Its OAM objects are fixed: cluster slot
/// `slots.start + n` draws object `first_object + n`.
struct Layer2Cluster {
    number: u8,
    slots: std::ops::Range<u32>,
    first_object: usize,
}

/// The castle candle flame (`CODE_02FA16`) is the only one in the game.
const LAYER2_CLUSTERS: [Layer2Cluster; 1] = [Layer2Cluster {
    number: 5,
    slots: 0..4,
    first_object: 124,
}];

/// Sprite slot statuses a loader leaves a sprite in: initialising, and
/// alive or carryable. (The handlers of slotless sprites scribble on the
/// status table; `E6` leaves a 7.)
const STATUS_FREE: u8 = 0;
const STATUS_INIT: u8 = 1;
const STATUS_ALIVE: std::ops::RangeInclusive<u8> = 0x08..=0x0B;

/// The level's extent in pixels, for keeping the camera inside it.
#[derive(Clone, Copy)]
struct Bounds {
    width: i32,
    height: i32,
}

impl Bounds {
    fn clamp_x(self, x: i32) -> i32 {
        x.clamp(0, (self.width - SCREEN_W).max(0))
    }

    fn clamp_y(self, y: i32) -> i32 {
        y.clamp(0, (self.height - SCREEN_H).max(0))
    }

    /// The camera that centres a 16x16 sprite at (`x`, `y`).
    fn centre(self, (x, y): (i32, i32)) -> (i32, i32) {
        (
            self.clamp_x(x + 8 - SCREEN_W / 2),
            self.clamp_y(y + 8 - SCREEN_H / 2),
        )
    }
}

/// The level loop, run from a restored state with the camera held still
/// and the player parked out of the way.
struct LevelLoop<'r> {
    machine: Machine<'r>,
    camera: (i32, i32),
    /// `OBSEL` and the object sizes it selects.
    object_select: u8,
    sizes: [(i32, i32); 2],
    /// VRAM as level preparation left it, which the scene is drawn from.
    level_vram: Vec<u8>,
    /// Frames run since the counter was last reset.
    frames: usize,
    /// The last frame's OAM image as the game drew it.
    drawn: Vec<u8>,
}

impl LevelLoop<'_> {
    fn ram(&mut self) -> &mut Ram {
        &mut self.machine.bus.ram
    }

    fn restore(&mut self, snapshot: &Ram) {
        self.ram().clone_from(snapshot);
    }

    /// Puts video memory back to the level's, so that a pass shows what
    /// it uploaded itself and nothing of the pass before it.
    fn reset_video(&mut self) {
        self.machine.bus.vram.clone_from(&self.level_vram);
    }

    /// The characters of `objects` that the passes since
    /// [`LevelLoop::reset_video`] have changed, in address order.
    fn uploaded_characters(&self, objects: &[SpriteObject]) -> Vec<Character> {
        let vram = &self.machine.bus.vram;
        let mut addresses = BTreeSet::new();
        for object in objects {
            let (width, height) = self.sizes[object.large as usize];
            for row in 0..height as usize / 8 {
                for column in 0..width as usize / 8 {
                    addresses.insert(object.character_address(self.object_select, column, row));
                }
            }
        }
        addresses
            .into_iter()
            .filter_map(|at| {
                let data = vram.get(at..at + OBJECT_CHARACTER_LEN)?;
                (data != &self.level_vram[at..at + OBJECT_CHARACTER_LEN]).then(|| Character {
                    address: at as u16,
                    data: data.try_into().expect("sliced to length"),
                })
            })
            .collect()
    }

    /// Holds the camera at `camera` and parks Mario just off the left
    /// edge, where most sprites expect to meet him (a Banzai Bill erases
    /// itself if he is to its right) and where his own objects stay out
    /// of OAM.
    fn place_camera(&mut self, camera: (i32, i32)) {
        self.camera = camera;
        let ram = self.ram();
        for addr in [ram::LAYER1_X, ram::NEXT_LAYER1_X] {
            ram.set_u16(addr, camera.0 as u16);
        }
        for addr in [ram::LAYER1_Y, ram::NEXT_LAYER1_Y] {
            ram.set_u16(addr, camera.1 as u16);
        }
        self.park_player();
    }

    /// Mario does not interact with tiles (parked inside a wall he would
    /// be crushed, and his death locks every sprite), so he is put back
    /// before every frame instead of being left to fall. At the level's
    /// left edge this puts him on screen `$FF`, where the game never has
    /// him; keeping him inside the level instead puts him on the screen,
    /// and his objects into the capture (vanilla `117`).
    fn park_player(&mut self) {
        let (x, y) = (self.camera.0 - 64, self.camera.1 + SCREEN_H / 2 - 16);
        let ram = self.ram();
        ram.set_u16(ram::PLAYER_X, x as u16);
        ram.set_u16(ram::PLAYER_Y, y as u16);
        ram.set_u8(ram::PLAYER_X_SPEED, 0);
        ram.set_u8(ram::PLAYER_Y_SPEED, 0);
        ram.set_u8(ram::PLAYER_NO_TILE_INTERACTION, 1);
    }

    /// Scroll commands move the camera (autoscroll) or tie it to layer 2,
    /// away from what is being captured.
    fn stop_scrolling(&mut self) {
        self.ram().set_u8(ram::LAYER1_SCROLL_CMD, 0);
        self.ram().set_u8(ram::LAYER2_SCROLL_CMD, 0);
    }

    /// Runs one frame of the level loop and returns what it drew, in
    /// screen coordinates.
    fn frame(&mut self) -> Result<Vec<SpriteObject>, CpuError> {
        self.park_player();
        let frame = oam::draw_frame(&mut self.machine)?;
        self.frames += 1;
        self.drawn = frame.drawn;
        let (image, first) = frame.uploaded;
        Ok(oam::screen_objects(&image, first, self.sizes))
    }

    fn slots(&self) -> std::ops::Range<u32> {
        0..self.machine.bus.ram.map().sprite_slots()
    }

    fn status(&self, slot: u32) -> u8 {
        self.machine.bus.ram.u8_at(ram::SPRITE_STATUS, slot)
    }

    /// Frees every sprite slot but those in `keep`.
    fn free_slots(&mut self, keep: &[u32]) {
        for slot in self.slots().filter(|slot| !keep.contains(slot)) {
            self.ram().set_u8_at(ram::SPRITE_STATUS, slot, STATUS_FREE);
        }
    }

    /// Position of the sprite in `slot`, in level pixels.
    fn position(&self, slot: u32) -> (i32, i32) {
        let ram = &self.machine.bus.ram;
        let at =
            |low, high| i16::from_le_bytes([ram.u8_at(low, slot), ram.u8_at(high, slot)]) as i32;
        (
            at(ram::SPRITE_X_LOW, ram::SPRITE_X_HIGH),
            at(ram::SPRITE_Y_LOW, ram::SPRITE_Y_HIGH),
        )
    }
}

/// Draws the level's sprites the way the game does. For each camera
/// position that puts a sprite entry's column where the ROM's sprite
/// loader looks, it restores the loaded level and runs that loader (so
/// custom sprite tools' loaders and extension bytes apply). Every spot
/// the loader put sprites at then gets a pass of its own: the other slots
/// are cleared, the camera is centred on the sprite, and the level loop
/// runs until the sprite has left its initialisation state and drawn
/// (sprites that stay hidden at first, like a Podoboo under the lava,
/// get up to [`LATE_SPRITE_FRAMES`]). OAM is read back in level
/// coordinates, less whatever a pass without the sprite draws from the
/// same camera, along with the characters the pass uploaded for them
/// (`SpriteScene::dynamic`). Entries that filled no slot (shooters,
/// generators, scroll commands, cluster sprite spawners) share one more
/// pass from the loader's camera. A pass the CPU core gives up on draws
/// nothing and is listed in `diagnostics`; what draws nothing is listed
/// in `undrawn` for the caller to mark.
///
/// Boss arenas draw their sprites in `LevelScene::boss` instead and get an empty
/// scene here.
pub fn capture_sprites(
    rom: &Rom,
    level: &LoadedLevel,
    list: &SpriteList,
) -> Result<SpriteScene, ExpandError> {
    capture_controlled(rom, level, list, None)
}

/// Captures sprites with a shared operation budget and cancellation.
pub fn capture_sprites_with_control(
    rom: &Rom,
    level: &LoadedLevel,
    list: &SpriteList,
    operation: &Operation,
) -> Result<SpriteScene, ExpandError> {
    let scene = capture_controlled(rom, level, list, Some(operation))?;
    operation.stage(Stage::Finished)?;
    Ok(scene)
}

pub(crate) fn capture_controlled(
    rom: &Rom,
    level: &LoadedLevel,
    list: &SpriteList,
    operation: Option<&Operation>,
) -> Result<SpriteScene, ExpandError> {
    if let Some(op) = operation {
        op.check()?;
    }
    if level.scene.boss.is_some() {
        return Ok(SpriteScene {
            object_select: level.video.object_select,
            ..Default::default()
        });
    }
    let mut capture = SpriteCapture::new(rom, level, list);
    capture.level_loop.machine.bus.operation = operation.cloned();
    // Columns go in camera order, so the scene does not depend on the
    // order of the level's sprite list.
    let mut columns: BTreeMap<(i32, i32), Vec<&SpriteEntry>> = BTreeMap::new();
    // An entry outside the level (Lunar Magic keeps what a hack left on
    // screens past its width) has no column the camera can reach, and
    // the ROM's loader has never been asked for one: the level's own
    // data ends where it finds sprites there.
    for entry in list.sprites.iter().filter(|entry| capture.inside(entry)) {
        columns
            .entry(capture.loader_camera(entry))
            .or_default()
            .push(entry);
    }
    let cameras: Vec<_> = columns.keys().copied().collect();
    let total = columns.len();
    for (completed, (camera, entries)) in columns.into_iter().enumerate() {
        if let Some(op) = operation {
            op.stage(Stage::Sprites { completed, total })?;
        }
        let earlier = capture.loaded_before(camera, &cameras);
        capture.capture_column(camera, &earlier, &entries);
    }
    if let Some(op) = operation {
        op.stage(Stage::Sprites {
            completed: total,
            total,
        })?;
    }
    Ok(capture.finish())
}

struct SpriteCapture<'a, 'r> {
    level_loop: LevelLoop<'r>,
    vertical: bool,
    /// Where the level is entered, along the axis it scrolls on.
    entry: i32,
    list: &'a SpriteList,
    bounds: Bounds,
    load_flags: LoadFlags,
    /// The loaded level with its sprite tables emptied, which every
    /// column and every baseline starts from.
    loaded_level: Ram,
    scene: SpriteScene,
    /// Objects already in the scene.
    seen: HashSet<SpriteObject>,
    /// Indices of the list entries a spawned sprite has been matched to.
    matched: HashSet<usize>,
    /// Sprites already captured, by number and spawn position: the loader
    /// fills the whole column whichever of its entries the camera was
    /// chosen for.
    captured: HashSet<(u8, i32, i32)>,
    /// Tile positions the loader has put a sprite at.
    spawned_at: HashSet<(i32, i32)>,
}

impl<'a, 'r> SpriteCapture<'a, 'r> {
    fn new(rom: &'r Rom, level: &LoadedLevel, list: &'a SpriteList) -> Self {
        let mut machine = Machine::new(rom, level.tiles.level);
        machine.bus.ram = level.ram.clone();
        machine.bus.vram.clone_from(&level.video.vram);
        let load_flags = LoadFlags::detect(&mut machine.bus);
        // Start from empty sprite tables: the entrance screen's sprites
        // were spawned during level preparation, and each pass respawns
        // what it needs from the level data.
        let ram = &mut machine.bus.ram;
        ram.fill(ram::SPRITE_STATUS, ram.map().sprite_slots(), 0);
        ram.fill(ram::CLUSTER_NUMBER, ram::CLUSTER_SLOTS, 0);
        load_flags.fill(ram, 0);
        ram.set_u8(ram::LAYER1_SCROLL_DIR, 1);
        ram.set_u8(ram::HORIZ_SCROLL_SETTING, 0);
        ram.set_u8(ram::VERT_SCROLL_SETTING, 0);
        ram.set_u8(ram::GAME_MODE, 0x14);
        let loaded_level = ram.clone();
        let (w, h) = level.tiles.size();
        Self {
            level_loop: LevelLoop {
                machine,
                camera: (0, 0),
                object_select: level.video.object_select,
                sizes: oam::object_sizes(level.video.object_select),
                level_vram: level.video.vram.clone(),
                frames: 0,
                drawn: level.ram.bytes(ram::OAM, oam::OAM_LEN),
            },
            vertical: level.tiles.vertical,
            entry: level.scene.camera[level.tiles.vertical as usize] as i16 as i32,
            list,
            bounds: Bounds {
                width: w as i32 * 16,
                height: h as i32 * 16,
            },
            load_flags,
            loaded_level,
            scene: SpriteScene {
                object_select: level.video.object_select,
                ..Default::default()
            },
            seen: HashSet::new(),
            matched: HashSet::new(),
            captured: HashSet::new(),
            spawned_at: HashSet::new(),
        }
    }

    fn finish(mut self) -> SpriteScene {
        let report = &self.level_loop.machine.bus.unsupported;
        if !report.is_empty() {
            self.scene
                .diagnostics
                .push(Diagnostic::Unsupported(report.clone()));
        }
        let mut marked = HashSet::new();
        self.scene.undrawn.retain(|entry| marked.insert(*entry));
        self.scene
    }

    /// Entry position in level pixels.
    fn entry_position(&self, entry: &SpriteEntry) -> (i32, i32) {
        let (x, y) = entry.tile_position(self.vertical);
        (x as i32 * 16, y as i32 * 16)
    }

    /// Whether the entry's position is inside the level.
    fn inside(&self, entry: &SpriteEntry) -> bool {
        let (x, y) = self.entry_position(entry);
        x < self.bounds.width && y < self.bounds.height
    }

    /// Camera position whose loading column is the entry's, keeping the
    /// sprite inside the screen on the other axis.
    fn loader_camera(&self, entry: &SpriteEntry) -> (i32, i32) {
        let (x, y) = self.entry_position(entry);
        if self.vertical {
            (self.bounds.clamp_x(x - SCREEN_W / 2), y)
        } else {
            (x, self.bounds.clamp_y(y - SCREEN_H / 2))
        }
    }

    /// The loader cameras among `cameras` whose sprites hold slots when
    /// the game first loads the column at `camera`, in the order it loaded
    /// them, for a player who comes from the entrance and leaves every
    /// sprite where it is: the columns before this one on entering the
    /// level, and beyond those the columns the camera has just scrolled
    /// past, whose sprites have not yet fallen far enough behind to erase
    /// themselves.
    fn loaded_before(&self, camera: (i32, i32), cameras: &[(i32, i32)]) -> Vec<(i32, i32)> {
        let along = |camera: (i32, i32)| if self.vertical { camera.1 } else { camera.0 };
        let at = along(camera);
        let entry = ENTRY_COLUMNS.start + self.entry..ENTRY_COLUMNS.end + self.entry;
        let (live, backwards) = if entry.contains(&at) {
            (entry.start..at, false)
        } else if at < entry.start {
            (at + 1..at + 1 + LIVE_BEHIND_LOADER, true)
        } else {
            (at - LIVE_BEHIND_LOADER..at, false)
        };
        let mut earlier: Vec<_> = cameras
            .iter()
            .copied()
            .filter(|&other| live.contains(&along(other)))
            .collect();
        if backwards {
            earlier.reverse();
        }
        earlier
    }

    fn diagnose(&mut self, pass: Pass, error: CpuError) {
        self.scene.diagnostics.push(Diagnostic::Cpu { pass, error });
    }

    /// Adds what a pass drew from `camera` to the scene, in level
    /// coordinates: with the level's objects, or as a capture of its own
    /// if the pass uploaded characters for it. A pass for one entry
    /// (`entry`: its sprite number and tile position) is also kept as
    /// that entry's capture.
    fn keep(&mut self, camera: (i32, i32), drawn: Drawn, entry: Option<(u8, (i32, i32))>) {
        let translated: Vec<_> = drawn
            .objects
            .iter()
            .map(|object| object.translated(camera.0, camera.1))
            .collect();
        if let Some((id, (x, y))) = entry {
            self.scene.captures.push(CapturedSprite {
                x,
                y,
                id,
                objects: translated.clone(),
            });
        }
        let objects: Vec<_> = translated
            .into_iter()
            .filter(|object| self.seen.insert(*object))
            .collect();
        if drawn.characters.is_empty() {
            self.scene.objects.extend(objects);
        } else if !objects.is_empty() {
            self.scene.dynamic.push(DynamicObjects {
                objects,
                characters: drawn.characters,
            });
        }
    }

    /// The entry a freshly loaded sprite at (`x`, `y`) came from. Only the
    /// low nibbles and the screen number are compared: the vanilla loader
    /// leaves the entry's extra bits in the high byte of the other axis
    /// for the sprite's initialisation to collect (a goal tape starts out
    /// 1280 pixels down).
    fn entry_for(&mut self, (x, y): (i32, i32)) -> Option<&'a SpriteEntry> {
        let vertical = self.vertical;
        let (index, entry) = self
            .list
            .sprites
            .iter()
            .enumerate()
            .find(|(index, entry)| {
                let (ex, ey) = entry.tile_position(vertical);
                let (ex, ey) = (ex as i32, ey as i32);
                let screen = if vertical {
                    ey / 16 == y >> 8
                } else {
                    ex / 16 == x >> 8
                };
                screen
                    && ex % 16 == (x >> 4) & 15
                    && ey % 16 == (y >> 4) & 15
                    && !self.matched.contains(index)
            })?;
        self.matched.insert(index);
        Some(entry)
    }

    /// What the level draws by itself after `frames` frames at `camera`.
    /// Every load flag is set: the level loop runs the sprite loader too,
    /// and a baseline that spawned the sprite would subtract it. Video
    /// memory is left as it was found.
    fn baseline(
        &mut self,
        camera: (i32, i32),
        frames: usize,
    ) -> Result<HashSet<SpriteObject>, CpuError> {
        self.level_loop.restore(&self.loaded_level);
        self.load_flags.fill(self.level_loop.ram(), 1);
        self.level_loop.stop_scrolling();
        self.level_loop.place_camera(camera);
        let counted = self.level_loop.frames;
        let vram = self.level_loop.machine.bus.vram.clone();
        let mut objects = Ok(Vec::new());
        for _ in 0..frames {
            objects = self.level_loop.frame();
            if objects.is_err() {
                break;
            }
        }
        self.level_loop.frames = counted;
        self.level_loop.machine.bus.vram = vram;
        Ok(objects?.into_iter().collect())
    }

    /// Captures everything the loader spawns with the camera at `camera`,
    /// then the column's entries that took no slot. Some sprites look at
    /// their slot (the Yoshi's House birds take their colour from it, a
    /// line-guided rope its length), so the column is loaded first with
    /// the sprites of the columns in `earlier` holding theirs.
    fn capture_column(
        &mut self,
        camera: (i32, i32),
        earlier: &[(i32, i32)],
        entries: &[&SpriteEntry],
    ) {
        // A loader that fails here fails again below, and is reported there.
        if !earlier.is_empty() && self.hold_slots(earlier).is_ok() {
            self.level_loop.place_camera(camera);
            let _ = self.spawn_round();
        }
        self.level_loop.restore(&self.loaded_level);
        self.level_loop.place_camera(camera);
        // The loader stops after a scroll sprite and skips sprites it has
        // no free slot for; the game calls it again every other frame.
        // Each round here captures what it spawned and frees the slots.
        for _ in 0..SPAWN_ROUNDS {
            match self.spawn_round() {
                Ok(true) => {}
                Ok(false) => break,
                // Whatever this column holds that crashes the loader
                // leaves its entries as markers.
                Err(error) => {
                    self.diagnose(Pass::SpriteLoader { camera }, error);
                    self.level_loop.restore(&self.loaded_level);
                    self.level_loop.place_camera(camera);
                    break;
                }
            }
        }
        self.capture_slotless(camera, entries);
    }

    /// The ROM's sprite loader, for the column at the camera. It reads its
    /// slot tables through the data bank its bank 2 callers set.
    fn load_column(&mut self) -> Result<(), CpuError> {
        let loader = Call::jsr(routines::SPAWN_SPRITES).data_bank(0x02);
        self.level_loop.machine.try_call(loader)
    }

    /// Starts over from the loaded level with the slots taken that the
    /// sprites of the columns at `cameras` get, loaded in that order.
    /// Nothing else of those columns is kept (cluster sprites, a
    /// generator, a scroll command), and nothing is in the slots to run.
    fn hold_slots(&mut self, cameras: &[(i32, i32)]) -> Result<(), CpuError> {
        self.level_loop.restore(&self.loaded_level);
        for &camera in cameras {
            self.level_loop.place_camera(camera);
            self.level_loop.stop_scrolling();
            self.load_column()?;
        }
        let held: Vec<u32> = (self.level_loop.slots())
            .filter(|&slot| self.level_loop.status(slot) != STATUS_FREE)
            .collect();
        self.level_loop.restore(&self.loaded_level);
        for slot in held {
            let ram = self.level_loop.ram();
            ram.set_u8_at(ram::SPRITE_STATUS, slot, *STATUS_ALIVE.start());
        }
        Ok(())
    }

    /// Calls the sprite loader once and captures each spot it filled, of
    /// the slots that were free. False once a call loads nothing new.
    fn spawn_round(&mut self) -> Result<bool, CpuError> {
        let camera = self.level_loop.camera;
        self.level_loop.stop_scrolling();
        let flags_before = self.load_flags.read(self.level_loop.ram());
        let held: Vec<u32> = (self.level_loop.slots())
            .filter(|&slot| self.level_loop.status(slot) != STATUS_FREE)
            .collect();
        self.load_column()?;
        // A scroll sprite in this column has just installed its command.
        self.level_loop.stop_scrolling();
        let after_loader = self.level_loop.ram().clone();
        let mut fresh = flags_before != self.load_flags.read(self.level_loop.ram());
        // Sprites sharing a spot stay together: the flying platform
        // (`9C`) carries and draws the Hammer Bro (`9B`) placed on it.
        let mut spots: BTreeMap<(i32, i32), (Vec<u32>, u8)> = BTreeMap::new();
        for slot in self.level_loop.slots() {
            let status = self.level_loop.status(slot);
            if held.contains(&slot) || (status != STATUS_INIT && !STATUS_ALIVE.contains(&status)) {
                continue;
            }
            let (x, y) = self.level_loop.position(slot);
            let number = self.level_loop.ram().u8_at(ram::SPRITE_NUMBER, slot);
            if !self.captured.insert((number, x, y)) {
                continue;
            }
            fresh = true;
            let (spot, id) = match self.entry_for((x, y)) {
                Some(entry) => (self.entry_position(entry), entry.id),
                None => ((x, y), number),
            };
            self.spawned_at
                .insert((spot.0.div_euclid(16), spot.1.div_euclid(16)));
            spots.entry(spot).or_insert((Vec::new(), id)).0.push(slot);
        }
        // The level loop must not load anything else.
        self.load_flags.fill(self.level_loop.ram(), 1);
        self.level_loop.ram().set_u8(ram::SPRITE_GENERATOR, 0);
        let spawned = self.level_loop.ram().clone();
        for (spot, (slots, id)) in spots {
            self.level_loop.restore(&spawned);
            self.capture_spot(spot, &slots, id);
        }
        self.level_loop.restore(&after_loader);
        self.level_loop.free_slots(&[]);
        self.level_loop.place_camera(camera);
        Ok(fresh)
    }

    /// Captures the sprites in `slots`, which the loader put at `spot`,
    /// from the spawned state the level loop has been restored to.
    fn capture_spot(&mut self, spot: (i32, i32), slots: &[u32], id: u8) {
        self.level_loop.free_slots(slots);
        self.level_loop.place_camera(self.bounds.centre(spot));
        self.level_loop.reset_video();
        self.level_loop.frames = 0;
        let tile = (spot.0.div_euclid(16), spot.1.div_euclid(16));
        // A sprite whose code crashes the CPU (or a level whose own
        // per-frame code does) gets a marker.
        let drawn = self.draw_spot(slots).unwrap_or_else(|error| {
            let (x, y) = tile;
            self.diagnose(Pass::Sprite { id, x, y }, error);
            Drawn::default()
        });
        let nothing = drawn.objects.is_empty();
        self.keep(self.level_loop.camera, drawn, Some((id, tile)));
        if nothing {
            self.scene.undrawn.push(UndrawnSprite {
                x: tile.0.max(0) as usize,
                y: tile.1.max(0) as usize,
                id,
            });
        }
    }

    /// Runs the level loop until the sprites in `slots` have drawn, and
    /// returns their objects in screen coordinates of the camera the loop
    /// ends on. Initialisation takes the first frame and drawing the
    /// second; some sprites wait a few more frames before they first
    /// appear, and one that has drawn nothing by then gets frames for as
    /// long as it lives.
    fn draw_spot(&mut self, slots: &[u32]) -> Result<Drawn, CpuError> {
        let mut follower = Follower {
            slot: slots[0],
            moves: 0,
        };
        let mut objects = Vec::new();
        while self.level_loop.frames < SPRITE_FRAMES {
            objects = self.level_loop.frame()?;
            let moved = follower.follow(&mut self.level_loop, self.bounds);
            let ready = slots
                .iter()
                .all(|&slot| self.level_loop.status(slot) != STATUS_INIT);
            if !moved && self.level_loop.frames >= 2 && ready {
                break;
            }
        }
        let resume = self.level_loop.ram().clone();
        let (camera, frames) = (self.level_loop.camera, self.level_loop.frames);
        let level_draws = self.baseline(camera, frames)?;
        let mut drawn = Drawn::own(&self.level_loop, objects, &level_draws);
        if drawn.objects.is_empty() {
            // Hidden so far: keep going while the sprite lives.
            self.level_loop.restore(&resume);
            while drawn.objects.is_empty()
                && self.level_loop.frames < LATE_SPRITE_FRAMES
                && slots
                    .iter()
                    .any(|&slot| self.level_loop.status(slot) != STATUS_FREE)
            {
                let objects = self.level_loop.frame()?;
                if !follower.follow(&mut self.level_loop, self.bounds) {
                    drawn = Drawn::own(&self.level_loop, objects, &level_draws);
                }
            }
        }
        Ok(drawn)
    }

    /// The pass shared by the column's entries that took no sprite slot:
    /// shooters, generators, scroll commands, and the spawners of cluster
    /// sprites. Runs from wherever the spawn rounds left the level loop.
    fn capture_slotless(&mut self, camera: (i32, i32), entries: &[&SpriteEntry]) {
        let vertical = self.vertical;
        let slotless: Vec<_> = entries
            .iter()
            .map(|entry| (entry.tile_position(vertical), entry.id))
            .filter(|((x, y), _)| !self.spawned_at.contains(&(*x as i32, *y as i32)))
            .collect();
        if slotless.is_empty() {
            return;
        }
        self.load_flags.fill(self.level_loop.ram(), 1);
        self.level_loop.reset_video();
        self.level_loop.frames = 0;
        // A frame the CPU core gives up on draws nothing; the pass goes on
        // and reports the first such error.
        let mut failure = None;
        let mut objects = Vec::new();
        for _ in 0..SLOTLESS_FRAMES {
            objects = self.level_loop.frame().unwrap_or_else(|error| {
                failure.get_or_insert(error);
                Vec::new()
            });
        }
        let riding = self.take_layer2_clusters(&mut objects);
        let level_draws = self
            .baseline(camera, SLOTLESS_FRAMES)
            .unwrap_or_else(|error| {
                failure.get_or_insert(error);
                HashSet::new()
            });
        if let Some(error) = failure {
            self.diagnose(Pass::Slotless { camera }, error);
        }
        let drawn = Drawn::own(&self.level_loop, objects, &level_draws);
        let nothing = drawn.objects.is_empty();
        self.keep(camera, drawn, None);
        if nothing && !riding {
            self.scene
                .undrawn
                .extend(
                    slotless
                        .into_iter()
                        .map(|((x, y), id)| UndrawnSprite { x, y, id }),
                );
        }
    }

    /// Moves the objects of layer-2-riding cluster sprites (see
    /// [`LAYER2_CLUSTERS`]) that the level loop has just drawn out of
    /// `objects` and into the scene's layer 2 objects, positioned on that
    /// layer. True if there were any.
    fn take_layer2_clusters(&mut self, objects: &mut Vec<SpriteObject>) -> bool {
        let ram = &self.level_loop.machine.bus.ram;
        // Fixed objects are where the game drew them, not where the end
        // of the frame may have moved them to.
        let image = &self.level_loop.drawn;
        let mut found = false;
        for cluster in &LAYER2_CLUSTERS {
            for slot in cluster.slots.clone() {
                if ram.u8_at(ram::CLUSTER_NUMBER, slot) != cluster.number {
                    continue;
                }
                found = true;
                let object = cluster.first_object + (slot - cluster.slots.start) as usize;
                let (tile, attr) = (image[object * 4 + 2], image[object * 4 + 3]);
                let high = image[oam::OAM_OBJECTS * 4 + object / 4] >> (2 * (object % 4));
                let riding = SpriteObject {
                    x: ram.u8_at(ram::CLUSTER_X_LOW, slot) as i32,
                    y: ram.u8_at(ram::CLUSTER_Y_LOW, slot) as i32,
                    tile,
                    attr,
                    large: high & 2 != 0,
                };
                let placed = &mut self.scene.layer2_objects;
                if !placed.iter().any(|o| (o.x, o.y) == (riding.x, riding.y)) {
                    placed.push(riding);
                }
                objects.retain(|o| (o.tile, o.attr) != (tile, attr));
            }
        }
        found
    }
}

/// What a pass drew of its own, in screen coordinates, and the characters
/// it uploaded to draw it with.
#[derive(Default)]
struct Drawn {
    objects: Vec<SpriteObject>,
    characters: Vec<Character>,
}

impl Drawn {
    /// The objects of the frame `level_loop` has just run that are not
    /// among `level_draws`, what the level draws without the sprite.
    fn own(
        level_loop: &LevelLoop,
        objects: Vec<SpriteObject>,
        level_draws: &HashSet<SpriteObject>,
    ) -> Self {
        let objects: Vec<_> = objects
            .into_iter()
            .filter(|object| !level_draws.contains(object))
            .collect();
        let characters = level_loop.uploaded_characters(&objects);
        Self {
            objects,
            characters,
        }
    }
}

/// Keeps the camera on a sprite that leaves the screen: some start by
/// moving a long way from their entry (a line-guided chainsaw goes 320
/// pixels left).
struct Follower {
    slot: u32,
    moves: usize,
}

impl Follower {
    /// Re-centres the camera if the sprite has left the screen. True if
    /// the camera moved, in which case the frame just drawn does not show
    /// the sprite.
    fn follow(&mut self, level_loop: &mut LevelLoop, bounds: Bounds) -> bool {
        let (x, y) = level_loop.position(self.slot);
        let on_screen = (0..SCREEN_W).contains(&(x + 8 - level_loop.camera.0))
            && (0..SCREEN_H).contains(&(y + 8 - level_loop.camera.1));
        let moved = !on_screen
            && self.moves < CAMERA_MOVES
            && level_loop.status(self.slot) != STATUS_FREE
            && bounds.centre((x, y)) != level_loop.camera;
        if moved {
            self.moves += 1;
            level_loop.place_camera(bounds.centre((x, y)));
        }
        moved
    }
}
