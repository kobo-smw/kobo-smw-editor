//! The overworld, read from a ROM: the game's own layout or Lunar Magic's
//! (docs/smw.md and docs/lunar-magic-install.md, "The overworld").
//!
//! Lunar Magic keeps most of the game's code reading the game's tables,
//! moved into RATS blocks and grown (`$78` events, not `$6F`), so a table
//! is found through the operand of the game's instruction that reads it,
//! whichever layout the ROM has. Three tables only Lunar Magic's code
//! reads are found through the places its layout keeps their pointers,
//! each found by moving the table and diffing (never by reading its
//! code): the translevels and directions (`$04D803`, bank `$04D808`),
//! layer 1's high bytes (`$04D822`, bank `$04D827`), and the level names
//! (`$03BB57`). What the reader gives is checked against what the ROM's
//! own load leaves in RAM ([`crate::expand::load_overworld`]).

use thiserror::Error;

use crate::addr::SnesAddr;
use crate::compress::lz2;
use crate::rom::{Rom, RomError};

/// Layer 1's tiles: `$800`, the main map's 32x32 then the submaps' area.
pub const LAYER1_TILES: usize = 0x800;
/// Layer 2's 8x8 tiles: `$2000`, 64 to a row.
pub const LAYER2_TILES: usize = 0x2000;
/// Levels with a name: the translevels `00` to `5F`.
pub const NAMES: usize = 0x60;
/// Tiles in a name.
pub const NAME_TILES: usize = 19;

/// Where the game keeps layer 1's tile numbers, which Lunar Magic leaves.
pub const LAYER1_DATA: SnesAddr = SnesAddr::new(0x0C_F7DF);
/// The direction a level tile is left by when its level is passed, by
/// translevel (`DATA_04D678`), which the game copies per tile at the load.
pub const DIRECTIONS: SnesAddr = SnesAddr::new(0x04_D678);
/// The first event-tile entry of each event, and of the one after the
/// last (`DATA_04E359`), which both layouts keep here.
pub const EVENT_RANGES: SnesAddr = SnesAddr::new(0x04_E359);
/// The reveal list: the layer 1 tiles an event turns (`DATA_04DA1D`)
/// into others (`DATA_04DA33`).
pub const REVEAL_FROM: SnesAddr = SnesAddr::new(0x04_DA1D);
pub const REVEAL_TO: SnesAddr = SnesAddr::new(0x04_DA33);
pub const REVEALS: usize = 0x16;

/// The game's instructions whose operands lead to tables, in both layouts.
mod operands {
    use crate::addr::SnesAddr;
    /// `LDA #OWTileNumbers` (low word), its bank, and `LDA #OWTilemap`.
    pub const LAYER2_NUMBERS: SnesAddr = SnesAddr::new(0x04_DC72);
    pub const LAYER2_BANK: SnesAddr = SnesAddr::new(0x04_DC79);
    pub const LAYER2_PROPERTIES: SnesAddr = SnesAddr::new(0x04_DC8D);
    /// `CMP #$6F`, the events the load goes through.
    pub const EVENT_COUNT: SnesAddr = SnesAddr::new(0x04_D859);
    /// `LDA.l DATA_04D85D,X` and `LDA.l DATA_04D93D,X`: an event's layer 1
    /// tile place and VRAM address.
    pub const EVENT_LAYER1: SnesAddr = SnesAddr::new(0x04_DA74);
    pub const EVENT_VRAM: SnesAddr = SnesAddr::new(0x04_EDB8);
    /// `LDA.l DATA_04DD8D,X`: the event-tile entries, 4 bytes each.
    pub const EVENT_TILES: SnesAddr = SnesAddr::new(0x04_E49F);
    /// `LDA.l OWEventTileNum,X`.
    pub const EVENT_NUMBERS: SnesAddr = SnesAddr::new(0x04_EAF5);
    /// `LDY #OWEventTileProp` and its bank's `LDA #`.
    pub const EVENT_PROPERTIES: SnesAddr = SnesAddr::new(0x04_DD45);
    pub const EVENT_PROPERTIES_BANK: SnesAddr = SnesAddr::new(0x04_DD4A);
    /// The crushed tiles: `CMP.l DATA_04E5D6,X` (events), `LDA.l
    /// DATA_04E5B6,X` (places), `LDA.l DATA_04E587,X` (VRAM).
    pub const CRUSH_EVENTS: SnesAddr = SnesAddr::new(0x04_E67C);
    pub const CRUSH_PLACES: SnesAddr = SnesAddr::new(0x04_E69C);
    pub const CRUSH_VRAM: SnesAddr = SnesAddr::new(0x04_EEC9);
}

/// Where Lunar Magic's layout keeps the pointers only its code reads.
mod lunar_magic {
    use crate::addr::SnesAddr;
    pub const TRANSLEVELS: SnesAddr = SnesAddr::new(0x04_D803);
    pub const TRANSLEVELS_BANK: SnesAddr = SnesAddr::new(0x04_D808);
    pub const LAYER1_HIGH: SnesAddr = SnesAddr::new(0x04_D822);
    pub const LAYER1_HIGH_BANK: SnesAddr = SnesAddr::new(0x04_D827);
    pub const NAMES: SnesAddr = SnesAddr::new(0x03_BB57);
    /// The level name hook's `JSL`, where the game has `ASL A`: what tells
    /// the layouts apart.
    pub const NAME_HOOK: SnesAddr = SnesAddr::new(0x04_8E81);
}

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("the overworld's {0} does not decode")]
    Decode(&'static str),
}

/// Which layout a ROM keeps its overworld in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// The game's own.
    Game,
    /// Lunar Magic's, as its overworld save writes it (3.x: two pages of
    /// layer 1 tiles).
    LunarMagic,
}

impl Layout {
    pub fn of(rom: &Rom) -> Self {
        if rom.read_u8(lunar_magic::NAME_HOOK).is_ok_and(|b| b == 0x22) {
            Layout::LunarMagic
        } else {
            Layout::Game
        }
    }
}

/// One entry of an event's layer 2 changes: a block of tiles from the
/// event tile data at `data` (6x6 tiles, or 2x2 from `$900` on), put at
/// `place` in layer 2's tilemap (a byte offset into `$7F4000`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventTile {
    pub data: u16,
    pub place: u16,
}

impl EventTile {
    /// The tiles the block has: 36 or 4.
    pub fn tiles(&self) -> usize {
        if self.data >= 0x900 { 4 } else { 36 }
    }
}

/// A tile an event crushes (a castle or fortress destroyed).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Crush {
    pub event: u8,
    /// Its place in layer 1.
    pub place: u16,
    pub vram: u16,
}

/// What the overworld's events change.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Events {
    /// Each event's layer 1 tile, by its place, and that place's VRAM
    /// address (0 for none).
    pub layer1: Vec<(u16, u16)>,
    /// The first entry of `tiles` of each event, and the end of the last.
    pub ranges: Vec<u16>,
    pub tiles: Vec<EventTile>,
    /// The event tile data: tile numbers and properties, as many as the
    /// entries reach.
    pub numbers: Vec<u8>,
    pub properties: Vec<u8>,
    pub crush: Vec<Crush>,
    /// The reveal list.
    pub reveal: Vec<(u8, u8)>,
}

impl Events {
    pub fn count(&self) -> usize {
        self.layer1.len()
    }

    /// The entries of event `n`.
    pub fn of(&self, n: usize) -> &[EventTile] {
        let (a, b) = (usize::from(self.ranges[n]), usize::from(self.ranges[n + 1]));
        self.tiles.get(a..b.max(a)).unwrap_or(&[])
    }
}

/// An overworld, as a ROM has it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Overworld {
    pub layout: Layout,
    /// Layer 1's tile numbers, pages 0 and 1.
    pub layer1: Vec<u16>,
    /// The translevel of each layer 1 place, 0 for none.
    pub translevels: Vec<u8>,
    /// The direction byte of each layer 1 place.
    pub directions: Vec<u8>,
    /// Layer 2's tiles: the number in the low byte, the properties in the
    /// high.
    pub layer2: Vec<u16>,
    pub names: Vec<[u8; NAME_TILES]>,
    pub events: Events,
}

fn read_word_ptr(rom: &Rom, low: SnesAddr, bank: SnesAddr) -> Result<SnesAddr, RomError> {
    Ok(SnesAddr::new(
        u32::from(rom.read_u8(bank)?) << 16 | u32::from(rom.read_u16(low)?),
    ))
}

/// The run-length format the game's layer 2 and event properties use
/// (`CODE_04DABA`, `CODE_04DD57`): a byte `n` below `$80` copies the next
/// `n + 1` bytes, one with bit 7 set the next byte `(n & $7F) + 1` times.
/// Writes every `step`th byte of `out` from `start`, until `end` (layer
/// 2) or a `$FFFF` word where a command would be (`stop_word`).
fn run_length(
    rom: &Rom,
    at: SnesAddr,
    out: &mut [u8],
    start: usize,
    step: usize,
    stop_word: bool,
) -> Result<(), OverworldError> {
    let mut x = start;
    let mut p = at;
    loop {
        if stop_word && rom.read_u16(p)? == 0xFFFF {
            return Ok(());
        }
        if !stop_word && x >= out.len() {
            return Ok(());
        }
        let n = rom.read_u8(p)?;
        p = p.add(1);
        let count = usize::from(n & 0x7F) + 1;
        let fill = (n & 0x80 != 0).then(|| rom.read_u8(p));
        for _ in 0..count {
            let value = match &fill {
                Some(v) => *v
                    .as_ref()
                    .map_err(|_| OverworldError::Decode("run-length data"))?,
                None => {
                    let v = rom.read_u8(p)?;
                    p = p.add(1);
                    v
                }
            };
            let slot = out
                .get_mut(x)
                .ok_or(OverworldError::Decode("run-length data"))?;
            *slot = value;
            x += step;
        }
        if fill.is_some() {
            p = p.add(1);
        }
    }
}

fn lz2_table(
    rom: &Rom,
    at: SnesAddr,
    len: usize,
    what: &'static str,
) -> Result<Vec<u8>, OverworldError> {
    let pc = rom
        .pc(at)
        .map_err(|_| OverworldError::Decode(what))?
        .as_usize();
    let data = rom.data().get(pc..).ok_or(OverworldError::Decode(what))?;
    let unpacked = lz2::decompress(data).map_err(|_| OverworldError::Decode(what))?;
    if unpacked.data.len() < len {
        return Err(OverworldError::Decode(what));
    }
    Ok(unpacked.data[..len].to_vec())
}

impl Overworld {
    pub fn read(rom: &Rom) -> Result<Self, OverworldError> {
        let layout = Layout::of(rom);
        let low = rom.read(LAYER1_DATA, LAYER1_TILES)?;
        let high = match layout {
            Layout::LunarMagic => {
                let at =
                    read_word_ptr(rom, lunar_magic::LAYER1_HIGH, lunar_magic::LAYER1_HIGH_BANK)?;
                lz2_table(rom, at, LAYER1_TILES, "layer 1 pages")?
            }
            Layout::Game => vec![0; LAYER1_TILES],
        };
        let layer1: Vec<u16> = low
            .iter()
            .zip(&high)
            .map(|(&l, &h)| u16::from(h) << 8 | u16::from(l))
            .collect();
        let (translevels, directions) = match layout {
            Layout::LunarMagic => {
                let at =
                    read_word_ptr(rom, lunar_magic::TRANSLEVELS, lunar_magic::TRANSLEVELS_BANK)?;
                let both = lz2_table(rom, at, 2 * LAYER1_TILES, "translevels")?;
                (both[..LAYER1_TILES].to_vec(), both[LAYER1_TILES..].to_vec())
            }
            Layout::Game => scan_translevels(rom, low)?,
        };
        let layer2 = read_layer2(rom)?;
        let names = match layout {
            Layout::LunarMagic => {
                let at = SnesAddr::new(rom.read_u24(lunar_magic::NAMES)?);
                let bytes = rom.read(at, NAMES * NAME_TILES)?;
                bytes
                    .chunks(NAME_TILES)
                    .map(|c| c.try_into().expect("19 tiles"))
                    .collect()
            }
            // A translevel whose parts lead off the tables has no name
            // the game could show, so none.
            Layout::Game => (0..NAMES as u8)
                .map(|t| game_name(rom, t).unwrap_or([0x1F; NAME_TILES]))
                .collect(),
        };
        let events = read_events(rom, layout)?;
        Ok(Self {
            layout,
            layer1,
            translevels,
            directions,
            layer2,
            names,
            events,
        })
    }
}

/// The game's numbering of translevels: each level tile (`56` to `80`),
/// in place order, takes the next number from 1, and its direction from
/// [`DIRECTIONS`] (`CODE_04D7F2`).
fn scan_translevels(rom: &Rom, layer1: &[u8]) -> Result<(Vec<u8>, Vec<u8>), OverworldError> {
    let mut translevels = vec![0; LAYER1_TILES];
    let mut directions = vec![0; LAYER1_TILES];
    let mut next = 1u8;
    for (i, &tile) in layer1.iter().enumerate() {
        if (0x56..0x81).contains(&tile) {
            translevels[i] = next;
            directions[i] = rom.read_u8(DIRECTIONS.add(u32::from(next)))?;
            next = next.wrapping_add(1);
        }
    }
    Ok((translevels, directions))
}

fn read_layer2(rom: &Rom) -> Result<Vec<u16>, OverworldError> {
    let bank = u32::from(rom.read_u8(operands::LAYER2_BANK)?) << 16;
    let numbers = SnesAddr::new(bank | u32::from(rom.read_u16(operands::LAYER2_NUMBERS)?));
    let properties = SnesAddr::new(bank | u32::from(rom.read_u16(operands::LAYER2_PROPERTIES)?));
    let mut map = vec![0u8; 2 * LAYER2_TILES];
    run_length(rom, numbers, &mut map, 0, 2, false)?;
    run_length(rom, properties, &mut map, 1, 2, false)?;
    Ok(map
        .chunks(2)
        .map(|w| u16::from(w[1]) << 8 | u16::from(w[0]))
        .collect())
}

/// A part of a name in the game's layout: its table, its index there, and
/// whether a first tile leaves it out.
type NamePart = (SnesAddr, u16, fn(u8) -> bool);

/// A name in the game's layout, as the overworld writes it: its three
/// parts' tiles (`CODE_049D07`), padded with spaces to 19.
fn game_name(rom: &Rom, translevel: u8) -> Result<[u8; NAME_TILES], OverworldError> {
    use crate::level::{
        LEVEL_NAME_FIRST, LEVEL_NAME_SECOND, LEVEL_NAME_STRINGS, LEVEL_NAME_THIRD, LEVEL_NAMES,
    };
    let word = rom.read_u16(LEVEL_NAMES.add(2 * u32::from(translevel)))?;
    let mut tiles = Vec::new();
    let parts: [NamePart; 3] = [
        (LEVEL_NAME_FIRST, word >> 8 & 0x7F, |t| t & 0x80 != 0),
        (LEVEL_NAME_SECOND, word >> 4 & 0x0F, |t| t == 0x9F),
        (LEVEL_NAME_THIRD, word & 0x0F, |_| false),
    ];
    for (table, index, skip) in parts {
        let at = u32::from(rom.read_u16(table.add(2 * u32::from(index)))?);
        if skip(rom.read_u8(LEVEL_NAME_STRINGS.add(at))?) {
            continue;
        }
        for i in 0..64 {
            let tile = rom.read_u8(LEVEL_NAME_STRINGS.add(at + i))?;
            tiles.push(tile & 0x7F);
            if tile & 0x80 != 0 {
                break;
            }
        }
    }
    let mut name = [0x1F; NAME_TILES];
    for (slot, tile) in name.iter_mut().zip(tiles) {
        *slot = tile;
    }
    Ok(name)
}

fn read_events(rom: &Rom, layout: Layout) -> Result<Events, OverworldError> {
    let operand =
        |at: SnesAddr| -> Result<SnesAddr, RomError> { Ok(SnesAddr::new(rom.read_u24(at)?)) };
    let count = usize::from(rom.read_u8(operands::EVENT_COUNT)?);
    let places = operand(operands::EVENT_LAYER1)?;
    let vram = operand(operands::EVENT_VRAM)?;
    let mut layer1 = Vec::with_capacity(count);
    for e in 0..count as u32 {
        layer1.push((
            rom.read_u16(places.add(2 * e))?,
            rom.read_u16(vram.add(2 * e))?,
        ));
    }
    let mut ranges = Vec::with_capacity(count + 1);
    for e in 0..=count as u32 {
        ranges.push(rom.read_u16(EVENT_RANGES.add(2 * e))?);
    }
    let entries = operand(operands::EVENT_TILES)?;
    let total = ranges.iter().copied().max().unwrap_or(0);
    let mut tiles = Vec::with_capacity(usize::from(total));
    for i in 0..u32::from(total) {
        tiles.push(EventTile {
            data: rom.read_u16(entries.add(4 * i))?,
            place: rom.read_u16(entries.add(4 * i + 2))?,
        });
    }
    let used = tiles
        .iter()
        .map(|t| usize::from(t.data) + t.tiles())
        .max()
        .unwrap_or(0);
    let numbers = rom.read(operand(operands::EVENT_NUMBERS)?, used)?.to_vec();
    let properties_at = read_word_ptr(
        rom,
        operands::EVENT_PROPERTIES,
        operands::EVENT_PROPERTIES_BANK,
    )?;
    let mut properties = vec![0; 0x1_0000];
    run_length(rom, properties_at, &mut properties, 0, 1, true)?;
    properties.truncate(used);
    let crushes = match layout {
        Layout::LunarMagic => 0x18,
        Layout::Game => 0x10,
    };
    let (events, places, vrams) = (
        operand(operands::CRUSH_EVENTS)?,
        operand(operands::CRUSH_PLACES)?,
        operand(operands::CRUSH_VRAM)?,
    );
    let mut crush = Vec::with_capacity(crushes);
    for i in 0..crushes as u32 {
        crush.push(Crush {
            event: rom.read_u8(events.add(i))?,
            place: rom.read_u16(places.add(2 * i))?,
            vram: rom.read_u16(vrams.add(2 * i))?,
        });
    }
    let mut reveal = Vec::with_capacity(REVEALS);
    for i in 0..REVEALS as u32 {
        reveal.push((
            rom.read_u8(REVEAL_FROM.add(i))?,
            rom.read_u8(REVEAL_TO.add(i))?,
        ));
    }
    Ok(Events {
        layer1,
        ranges,
        tiles,
        numbers,
        properties,
        crush,
        reveal,
    })
}

/// What of `read` differs from what the ROM's own load left
/// (`loaded`): a line per table that differs, none when all agree.
pub fn differences(read: &Overworld, loaded: &crate::expand::LoadedOverworld) -> Vec<String> {
    use crate::ram::RamAddr;
    let ram = |at: u32, len: usize| loaded.ram.bytes(RamAddr::new(at), len);
    let low: Vec<u8> = read.layer1.iter().map(|&t| t as u8).collect();
    let high: Vec<u8> = read.layer1.iter().map(|&t| (t >> 8) as u8).collect();
    let layer2: Vec<u8> = read.layer2.iter().flat_map(|w| w.to_le_bytes()).collect();
    let properties = &read.events.properties;
    [
        ("layer 1", low, ram(0x7E_C800, LAYER1_TILES)),
        ("layer 1 pages", high, ram(0x7F_C800, LAYER1_TILES)),
        (
            "translevels",
            read.translevels.clone(),
            ram(0x7E_D000, LAYER1_TILES),
        ),
        (
            "directions",
            read.directions.clone(),
            ram(0x7E_D800, LAYER1_TILES),
        ),
        ("layer 2", layer2, ram(0x7F_4000, 2 * LAYER2_TILES)),
        (
            "event properties",
            properties.clone(),
            ram(0x7F_0000, properties.len()),
        ),
    ]
    .into_iter()
    .filter_map(|(what, ours, theirs)| {
        let differ = ours.iter().zip(&theirs).filter(|(a, b)| a != b).count();
        (differ > 0).then(|| format!("{what}: {differ} bytes differ"))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_tile_is_a_6x6_block_below_900_and_2x2_from_it() {
        assert_eq!(
            EventTile {
                data: 0x8FC,
                place: 0
            }
            .tiles(),
            36
        );
        assert_eq!(
            EventTile {
                data: 0x900,
                place: 0
            }
            .tiles(),
            4
        );
    }
}
