//! Which object drew each tile of the grid, found by watching the loader.
//!
//! Only the ROM's loader knows what an object draws: a slope's tiles, a
//! pipe's, a patch's custom object. So the load is watched instead: the
//! bus logs the loader's data reads in the level's object data and its
//! writes to the tile planes, and a tile written after an object's bytes
//! were read, and before the next object's, is that object's. Nothing
//! but addresses is used, so this works the same on any ROM and keeps to
//! the clean room.
//!
//! A tile's owner is the last object that wrote it, the one that shows.
//! An object's footprint is every tile it wrote.

use super::tiles::{GRID_LEN, Layer2Objects, LevelTiles};
use crate::cpu::watch::{AccessLog, Watched};
use crate::level::objects::ObjectLayer;
use crate::level::{self, Layer2, Layer2Data};
use crate::ram::{self, RamMap};
use crate::rom::Rom;

/// One object of a level's lists, by its index there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjectRef {
    pub layer: ObjectLayer,
    pub index: usize,
}

/// The objects that drew the grid's tiles.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct ObjectMap {
    /// For each offset in the tile planes, the object that wrote it last.
    owners: Vec<Option<ObjectRef>>,
    /// Each object's tiles by layer and index, as plane offsets in
    /// ascending order.
    footprints: [Vec<Vec<usize>>; 2],
}

fn slot(layer: ObjectLayer) -> usize {
    match layer {
        ObjectLayer::One => 0,
        ObjectLayer::Two => 1,
    }
}

impl ObjectMap {
    /// Whether the load was not watched, or drew nothing from objects.
    pub fn is_empty(&self) -> bool {
        self.footprints.iter().all(Vec::is_empty)
    }

    /// The object that last wrote the tile at a plane offset.
    pub fn owner(&self, offset: usize) -> Option<ObjectRef> {
        self.owners.get(offset).copied().flatten()
    }

    /// The object that shows at a tile of a layer, by level position.
    pub fn owner_at(
        &self,
        tiles: &LevelTiles,
        layer: ObjectLayer,
        x: usize,
        y: usize,
    ) -> Option<ObjectRef> {
        let offset = match layer {
            ObjectLayer::One => {
                let (width, height) = tiles.size();
                (x < width && y < height).then(|| tiles.offset(x, y))?
            }
            ObjectLayer::Two => tiles.layer2_objects()?.offset(x, y)?,
        };
        self.owner(offset).filter(|o| o.layer == layer)
    }

    /// The plane offsets of every tile an object wrote.
    pub fn footprint(&self, object: ObjectRef) -> &[usize] {
        self.footprints[slot(object.layer)]
            .get(object.index)
            .map_or(&[], Vec::as_slice)
    }

    /// The level positions of every tile an object wrote.
    pub fn tiles(&self, tiles: &LevelTiles, object: ObjectRef) -> Vec<(usize, usize)> {
        let layer2 = tiles.layer2_objects();
        self.footprint(object)
            .iter()
            .filter_map(|&offset| match object.layer {
                ObjectLayer::One => tiles.position(offset),
                ObjectLayer::Two => layer2?.position(offset),
            })
            .collect()
    }

    /// The smallest rectangle around an object's tiles, as (x, y, width,
    /// height) in tiles; `None` for an object that wrote none.
    pub fn bounds(&self, tiles: &LevelTiles, object: ObjectRef) -> Option<[usize; 4]> {
        let placed = self.tiles(tiles, object);
        let (x0, x1) = (
            placed.iter().map(|p| p.0).min()?,
            placed.iter().map(|p| p.0).max()?,
        );
        let (y0, y1) = (
            placed.iter().map(|p| p.1).min()?,
            placed.iter().map(|p| p.1).max()?,
        );
        Some([x0, y0, x1 - x0 + 1, y1 - y0 + 1])
    }
}

/// A level's object data streams, by where they are on the bus.
pub(super) struct ObjectWatch {
    /// Per watched read range: the layer, and where each object starts.
    streams: Vec<(ObjectLayer, Vec<usize>, usize)>,
}

impl ObjectWatch {
    /// The watch for `level`'s load, and the log to put on the bus; `None`
    /// when its object data cannot be read, which the load itself then
    /// reports.
    pub fn new(rom: &Rom, level: u16) -> Option<(Self, AccessLog)> {
        let objects = level::read_objects(rom, level).ok()?;
        let mut streams = Vec::new();
        let mut reads = Vec::new();
        let start = level::layer1_ptr(rom, level).ok()?.raw();
        reads.push(start..start + objects.layer1.len as u32);
        streams.push((ObjectLayer::One, objects.layer1.starts, objects.layer1.len));
        if let (Layer2::Objects(data), Ok(Layer2Data::Objects(at))) =
            (objects.layer2, level::layer2_ptr(rom, level))
        {
            reads.push(at.raw()..at.raw() + data.len as u32);
            streams.push((ObjectLayer::Two, data.starts, data.len));
        }
        let map = RamMap::of(rom);
        let plane = |addr| {
            let at = map.resolve(addr);
            at..at + GRID_LEN as u32
        };
        let writes = vec![plane(ram::TILES_LOW), plane(ram::TILES_HIGH)];
        Some((Self { streams }, AccessLog::new(reads, writes)))
    }

    /// Reads the log of the load.
    pub fn finish(self, log: &AccessLog) -> ObjectMap {
        let mut map = ObjectMap {
            owners: vec![None; GRID_LEN],
            footprints: [Vec::new(), Vec::new()],
        };
        for (layer, starts, _) in &self.streams {
            map.footprints[slot(*layer)] = vec![Vec::new(); starts.len()];
        }
        let mut current: Option<ObjectRef> = None;
        for event in &log.events {
            match *event {
                Watched::Read { range, offset } => {
                    let (layer, starts, len) = &self.streams[range];
                    let offset = offset as usize;
                    // The header and the terminator belong to no object.
                    current = (offset + 1 < *len)
                        .then(|| starts.partition_point(|&s| s <= offset).checked_sub(1))
                        .flatten()
                        .map(|index| ObjectRef {
                            layer: *layer,
                            index,
                        });
                }
                Watched::Write { offset, .. } => {
                    let Some(object) = current else { continue };
                    let offset = offset as usize;
                    // A tile's two planes are written apart; one entry each.
                    if map.owners[offset] != Some(object) {
                        map.owners[offset] = Some(object);
                        map.footprints[slot(object.layer)][object.index].push(offset);
                    }
                }
            }
        }
        for footprint in map.footprints.iter_mut().flatten() {
            footprint.sort_unstable();
            footprint.dedup();
        }
        map
    }
}

impl LevelTiles {
    /// The level position of a layer 1 plane offset, the reverse of
    /// [`LevelTiles::offset`]; `None` past the level.
    pub fn position(&self, offset: usize) -> Option<(usize, usize)> {
        let (x, y) = if self.vertical {
            let (screen, rest) = (offset / 0x200, offset % 0x200);
            (
                (rest / 0x100) * 16 + rest % 16,
                screen * 16 + (rest % 0x100) / 16,
            )
        } else {
            let len = self.screen_len();
            let (screen, rest) = (offset / len, offset % len);
            (screen * 16 + rest % 16, rest / 16)
        };
        let (width, height) = self.size();
        (x < width && y < height).then_some((x, y))
    }
}

impl Layer2Objects {
    /// The level position of a plane offset, the reverse of
    /// [`Layer2Objects::offset`].
    pub fn position(self, offset: usize) -> Option<(usize, usize)> {
        match self {
            Self::Horizontal {
                base,
                rows,
                screens,
            } => {
                let rest = offset.checked_sub(base)?;
                let (screen, rest) = (rest / (rows * 16), rest % (rows * 16));
                (screen < screens).then_some((screen * 16 + rest % 16, rest / 16))
            }
            Self::Vertical => {
                let rest = offset.checked_sub(0x1C00)?;
                let (screen, rest) = (rest / 0x200, rest % 0x200);
                (screen < 14).then_some((
                    (rest / 0x100) * 16 + rest % 16,
                    screen * 16 + (rest % 0x100) / 16,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::LevelMode;

    #[test]
    fn layer2_positions_reverse_offsets() {
        for layout in [
            Layer2Objects::for_level(LevelMode(0x02), 27).unwrap(),
            Layer2Objects::Vertical,
        ] {
            for (x, y) in [(0, 0), (5, 9), (17, 3), (31, 15), (20, 40)] {
                if let Some(offset) = layout.offset(x, y) {
                    assert_eq!(layout.position(offset), Some((x, y)), "{layout:?}");
                }
            }
        }
    }

    #[test]
    fn writes_belong_to_the_object_read_last() {
        // Two objects at 5 and 8, the terminator at 11.
        let watch = ObjectWatch {
            streams: vec![(ObjectLayer::One, vec![5, 8], 12)],
        };
        let read = |offset| Watched::Read { range: 0, offset };
        let write = |offset| Watched::Write { range: 0, offset };
        let mut log = AccessLog::default();
        log.events = vec![
            read(0),
            write(1),
            read(5),
            read(7),
            write(10),
            write(11),
            read(8),
            write(11),
            write(12),
            read(11),
            write(13),
        ];
        let map = watch.finish(&log);
        let object = |index| ObjectRef {
            layer: ObjectLayer::One,
            index,
        };
        assert_eq!(map.owner(1), None);
        assert_eq!(map.owner(10), Some(object(0)));
        assert_eq!(map.owner(11), Some(object(1)));
        assert_eq!(map.owner(13), None);
        assert_eq!(map.footprint(object(0)), [10, 11]);
        assert_eq!(map.footprint(object(1)), [11, 12]);
    }
}
