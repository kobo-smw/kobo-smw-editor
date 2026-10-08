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
/// What the game's new game opens (`InitLevelTileMovementData`): 8 level
/// tiles' save bytes, by translevel, and the directions they are left by.
pub const START_OPENED: SnesAddr = SnesAddr::new(0x00_9EE0);
pub const OPENED: usize = 8;
/// What a new game gives each translevel in Lunar Magic's layout: its
/// whole byte of settings (`OWLevelTileSettings`), `$60` of them, here.
pub const LEVEL_FLAGS: SnesAddr = SnesAddr::new(0x05_DDA0);
/// The flags of a translevel's settings: its directions (bits 0 to 3, as
/// the game's), a save prompt when it is passed (Lunar Magic's), no entry
/// once it is passed (Lunar Magic's), the midway point, and passed.
pub const FLAG_SAVE: u8 = 0x10;
pub const FLAG_NO_ENTRY: u8 = 0x20;
/// Where a new game puts the players (`InitPlayerOverworldData`): their
/// submaps, a byte each, then two words each of their walking animation,
/// their position in pixels, and that position in tiles.
pub const START_PLAYERS: SnesAddr = SnesAddr::new(0x00_9EF0);
/// A table of the overworld's that both layouts keep where the game does,
/// and the game's own code reads (Lunar Magic's overworld editor changes it
/// in place): carried as its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableSite {
    pub name: &'static str,
    pub at: SnesAddr,
    pub len: usize,
    pub about: &'static str,
}

const fn site(name: &'static str, at: u32, len: usize, about: &'static str) -> TableSite {
    TableSite {
        name,
        at: SnesAddr::new(at),
        len,
        about,
    }
}

/// The overworld's tables kept in place (smw.md, "The overworld").
pub const TABLES: [TableSite; 14] = [
    site(
        "boss_levels",
        0x00_C9A7,
        8,
        "the levels whose boss passed plays a sequence (DATA_00C9A7)",
    ),
    site(
        "boss_secret_exit",
        0x00_CA0C,
        1,
        "the level whose boss passed takes the secret exit (CMP #$13)",
    ),
    site(
        "earthquake_level",
        0x00_CA13,
        1,
        "the level passed with an earthquake (CMP #$31)",
    ),
    site(
        "warps",
        0x04_8431,
        0xD8,
        "the star, pipe, and location warps: 27 places and destinations (DATA_048431)",
    ),
    site("music", 0x04_8D8A, 7, "each map's music (OverworldMusic)"),
    site(
        "music_2",
        0x04_DBC8,
        7,
        "each map's music, as a warp plays it (OverworldMusic2)",
    ),
    site(
        "koopa_teleports",
        0x04_8E49,
        12,
        "where a player who fails a Koopa Kid's level goes (DATA_048E49)",
    ),
    site(
        "no_auto_move_levels",
        0x04_906C,
        12,
        "the levels passed without walking on (DATA_04906C)",
    ),
    site(
        "exit_tiles",
        0x04_9964,
        0xA8,
        "the exit tiles between maps: 14 places and destinations (DATA_049964)",
    ),
    site(
        "sprites",
        0x04_F625,
        0x41,
        "the sprite list: 13 of a number and a place (OverworldSprites)",
    ),
    site(
        "ghosts",
        0x04_F666,
        12,
        "the extra ghosts' places (ExtraOWGhostXPos)",
    ),
    site(
        "fish",
        0x04_FA2E,
        12,
        "the fish's places, by their path tiles (DATA_04FA2E)",
    ),
    site(
        "koopa_kids",
        0x04_FB88,
        12,
        "the Koopa Kids' places, by their tiles (DATA_04FB88)",
    ),
    site(
        "sprite_by_submap",
        0x04_FC1E,
        8,
        "a sprite's place on the main map and the first submap (DATA_04FC1E)",
    ),
];

/// Each translevel's event (`DATA_05D608`), which a level passed makes (the
/// next one for its secret exit); both layouts keep it here.
pub const LEVEL_EVENTS: SnesAddr = SnesAddr::new(0x05_D608);

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
    /// The events' further tiles in the game's layout (`CODE_04E9F1`):
    /// `LDX #$2B` (the last entry), and the `CMP.l`/`LDA.l` operands of
    /// each entry's event (`DATA_04E8E4`), kind (`DATA_04E910`), data
    /// (`DATA_04E994`), and place (`DATA_04E93C`).
    pub const EXTRA_LAST: SnesAddr = SnesAddr::new(0x04_E9F2);
    pub const EXTRA_EVENTS: SnesAddr = SnesAddr::new(0x04_E9F4);
    pub const EXTRA_KINDS: SnesAddr = SnesAddr::new(0x04_EA27);
    pub const EXTRA_DATA: SnesAddr = SnesAddr::new(0x04_EA32);
    pub const EXTRA_PLACES: SnesAddr = SnesAddr::new(0x04_EA38);
    /// `CPY #$0900`: where the event tile data's 2x2 blocks start; and the
    /// path fade's `CMP #$0900` (`CODE_04EAC9`), which draws a block's
    /// tiles as sprites by the same split.
    pub const EVENT_SPLIT: SnesAddr = SnesAddr::new(0x04_E4C0);
    pub const FADE_SPLIT: SnesAddr = SnesAddr::new(0x04_EAD8);
    /// Layer 1's 16x16 tiles (`OWL1CharData`): the tilemap build's `LDX
    /// #` and its bank's `LDA #` (`CODE_04DCB6`), and the load's `LDA #`,
    /// from which it points `Map16Pointers` at each tile (`CODE_04DC09`).
    /// The scroll uploads take the bank for the overworld's tilesets from
    /// Kobo's Map16 code, which reads it from `TILES_BANK`.
    pub const TILES: SnesAddr = SnesAddr::new(0x04_DCBC);
    pub const TILES_BANK: SnesAddr = SnesAddr::new(0x04_DCC1);
    pub const TILES_POINTERS: SnesAddr = SnesAddr::new(0x04_DC3B);
}

/// The game's layer 1 16x16 tiles, `00`-`C0`; Lunar Magic's layout holds as
/// many as its block, up to [`MAX_TILES`].
pub const GAME_TILES: usize = 0xC1;
/// The tiles the two pages of layer 1's tile numbers reach.
pub const MAX_TILES: usize = 0x200;

/// Where Lunar Magic's layout keeps the pointers only its code reads.
mod lunar_magic {
    use crate::addr::SnesAddr;
    pub const TRANSLEVELS: SnesAddr = SnesAddr::new(0x04_D803);
    pub const TRANSLEVELS_BANK: SnesAddr = SnesAddr::new(0x04_D808);
    pub const LAYER1_HIGH: SnesAddr = SnesAddr::new(0x04_D822);
    pub const LAYER1_HIGH_BANK: SnesAddr = SnesAddr::new(0x04_D827);
    pub const NAMES: SnesAddr = SnesAddr::new(0x03_BB57);
    /// The events' further tiles: a `JSL` here (`$22`, where the game's
    /// loop through its table branches) leads to code with the tables'
    /// pointers at fixed offsets from its start: the ranges, then each
    /// entry's data, place, and kind.
    pub const EXTRA_HOOK: SnesAddr = SnesAddr::new(0x04_E9F7);
    pub const EXTRA_RANGES: u32 = 0x0D;
    pub const EXTRA_DATA: u32 = 0x22;
    pub const EXTRA_PLACES: u32 = 0x28;
    pub const EXTRA_KINDS: u32 = 0x34;
    /// The bank of layer 1's 16x16 tiles, as Lunar Magic reads it with the
    /// low word at `operands::TILES_POINTERS`: the second scroll upload's
    /// `LDY #$05` (`$058B21`), which its layout sets to the tiles' bank
    /// as it does the tilemap build's (found by bisecting its transfer).
    pub const TILES_BANK: SnesAddr = SnesAddr::new(0x05_8B22);
    /// The first byte of the game's translevel scan, `LDA #` (`$A9`) in the
    /// game; Lunar Magic's layout and Kobo's code for it replace the scan,
    /// which tells the layouts apart.
    pub const SCAN: SnesAddr = SnesAddr::new(0x04_D7F9);
}

#[derive(Debug, Error)]
pub enum OverworldError {
    #[error(transparent)]
    Rom(#[from] RomError),
    #[error("the overworld's {0} does not decode")]
    Decode(&'static str),
    #[error("the overworld has more {0} than the game's code can reach")]
    Full(&'static str),
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
        if rom.read_u8(lunar_magic::SCAN).is_ok_and(|b| b != 0xA9) {
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
    /// The tiles the block has, with 2x2 blocks' data from `split`: 36 or
    /// 4.
    pub fn tiles(&self, split: u16) -> usize {
        if self.data >= split { 4 } else { 36 }
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

/// A further tile an event changes (`CODE_04E9F1`), beyond its layer 1
/// tile and layer 2 entries: a layer 1 tile set outright at a place in
/// layer 1's tables (its page in the high byte, which Lunar Magic's layout
/// sets too; the game's sets the low byte alone, and has no pages), or a
/// layer 2 block from the event tile data, as an entry of the event's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extra {
    Layer1 { place: u16, tile: u16 },
    Layer2(EventTile),
}

/// [`Extra`], as a project holds it: a layer 2 block by its tiles.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExtraTile {
    Layer1 { place: u16, tile: u16 },
    Layer2(EventBlock),
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
    /// Each event's further tiles, in the order they are made.
    pub extras: Vec<Vec<Extra>>,
    /// Where the event tile data of 2x2 blocks starts, 6x6 blocks' below
    /// (`CPY #$0900` in `CODE_04E4A9`), which Lunar Magic can move.
    pub split: u16,
}

/// A block of an event's layer 2 change, as a project holds it: where it
/// goes and its tiles (36 for a 6x6 block, 4 for a 2x2), each the number in
/// the low byte and the properties in the high.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventBlock {
    pub place: u16,
    pub tiles: Vec<u16>,
}

/// Where the game's event tile data of 2x2 blocks starts; 6x6 blocks are
/// below.
pub const SMALL_BLOCKS: u16 = 0x900;
/// The end of the event tile data: the game's own size, what its
/// properties decode into at `$7F0000` without reaching what follows.
pub const EVENT_DATA_END: u16 = 0xD00;

impl Events {
    pub fn count(&self) -> usize {
        self.layer1.len()
    }

    /// Event `n`'s layer 2 change, block by block.
    pub fn blocks(&self, n: usize) -> Vec<EventBlock> {
        self.of(n).iter().map(|t| self.block(t)).collect()
    }

    /// An entry's block, its tiles from the event tile data.
    fn block(&self, t: &EventTile) -> EventBlock {
        EventBlock {
            place: t.place,
            tiles: (0..t.tiles(self.split))
                .map(|i| {
                    let at = usize::from(t.data) + i;
                    let number = self.numbers.get(at).copied().unwrap_or(0);
                    let props = self.properties.get(at).copied().unwrap_or(0);
                    u16::from(props) << 8 | u16::from(number)
                })
                .collect(),
        }
    }

    /// Event `n`'s further tiles, as a project holds them.
    pub fn extra_tiles(&self, n: usize) -> Vec<ExtraTile> {
        self.extras
            .get(n)
            .map(|extras| {
                extras
                    .iter()
                    .map(|e| match *e {
                        Extra::Layer1 { place, tile } => ExtraTile::Layer1 { place, tile },
                        Extra::Layer2(t) => ExtraTile::Layer2(self.block(&t)),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The tables from each event's blocks and further tiles, the event
    /// tile data laid out afresh: 6x6 blocks from 0, 2x2 blocks from
    /// [`SMALL_BLOCKS`].
    pub fn from_blocks(
        layer1: Vec<(u16, u16)>,
        blocks: &[Vec<EventBlock>],
        extra_tiles: &[Vec<ExtraTile>],
        crush: Vec<Crush>,
        reveal: Vec<(u8, u8)>,
        split: u16,
    ) -> Result<Self, OverworldError> {
        let mut ranges = vec![0u16];
        let mut tiles = Vec::new();
        let mut layout = DataLayout::new(split);
        for event in blocks {
            for block in event {
                let data = layout.lay(block)?;
                tiles.push(EventTile {
                    data,
                    place: block.place,
                });
            }
            ranges.push(tiles.len() as u16);
        }
        let mut extras = Vec::with_capacity(extra_tiles.len());
        for event in extra_tiles {
            let mut list = Vec::with_capacity(event.len());
            for extra in event {
                list.push(match extra {
                    &ExtraTile::Layer1 { place, tile } => Extra::Layer1 { place, tile },
                    ExtraTile::Layer2(block) => Extra::Layer2(EventTile {
                        data: layout.lay(block)?,
                        place: block.place,
                    }),
                });
            }
            extras.push(list);
        }
        let (numbers, properties) = layout.finish();
        Ok(Self {
            layer1,
            ranges,
            tiles,
            numbers,
            properties,
            crush,
            reveal,
            extras,
            split,
        })
    }

    /// The entries of event `n`.
    pub fn of(&self, n: usize) -> &[EventTile] {
        let (a, b) = (usize::from(self.ranges[n]), usize::from(self.ranges[n + 1]));
        self.tiles.get(a..b.max(a)).unwrap_or(&[])
    }
}

/// The event tile data being laid out: 6x6 blocks from 0, 2x2 blocks from
/// [`SMALL_BLOCKS`], a block whose tiles another has already taking its
/// data, as the game's own data shares blocks between entries.
struct DataLayout {
    split: u16,
    large: u16,
    small: u16,
    numbers: Vec<u8>,
    properties: Vec<u8>,
    used: usize,
    laid: std::collections::HashMap<Vec<u16>, u16>,
}

impl DataLayout {
    fn new(split: u16) -> Self {
        Self {
            split,
            large: 0,
            small: split,
            numbers: vec![0; usize::from(EVENT_DATA_END)],
            properties: vec![0; usize::from(EVENT_DATA_END)],
            used: 0,
            laid: std::collections::HashMap::new(),
        }
    }

    /// Where `block`'s tiles go in the data.
    fn lay(&mut self, block: &EventBlock) -> Result<u16, OverworldError> {
        if let Some(&data) = self.laid.get(&block.tiles) {
            return Ok(data);
        }
        let data = match block.tiles.len() {
            36 => {
                let at = self.large;
                self.large += 36;
                if self.large > self.split {
                    return Err(OverworldError::Full("6x6 event blocks"));
                }
                at
            }
            4 => {
                let at = self.small;
                self.small += 4;
                if self.small > EVENT_DATA_END {
                    return Err(OverworldError::Full("2x2 event blocks"));
                }
                at
            }
            _ => {
                return Err(OverworldError::Decode(
                    "an event block of neither 36 nor 4 tiles",
                ));
            }
        };
        for (i, w) in block.tiles.iter().enumerate() {
            self.numbers[usize::from(data) + i] = *w as u8;
            self.properties[usize::from(data) + i] = (w >> 8) as u8;
        }
        self.used = self.used.max(usize::from(data) + block.tiles.len());
        self.laid.insert(block.tiles.clone(), data);
        Ok(data)
    }

    /// The tile numbers and properties, as far as a block reaches.
    fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        self.numbers.truncate(self.used);
        self.properties.truncate(self.used);
        (self.numbers, self.properties)
    }
}

/// Where a new game puts a player: the submap (0 the main map) and the
/// position in pixels, which the game keeps in tiles too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Start {
    pub submap: u8,
    pub x: u16,
    pub y: u16,
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
    /// Where a new game puts Mario and Luigi.
    pub start: [Start; 2],
    /// Each translevel's settings at a new game ([`LEVEL_FLAGS`]).
    pub level_flags: Vec<u8>,
    /// Each translevel's event.
    pub level_events: Vec<u8>,
    /// Layer 1's 16x16 tiles, by number: each one's four 8x8 tiles'
    /// tilemap words, in the game's order (top left, bottom left, top
    /// right, bottom right).
    pub tiles: Vec<[u16; 4]>,
    /// The tables kept in place, as [`TABLES`] lists them.
    pub tables: Vec<Vec<u8>>,
    /// Lunar Magic's overworld palettes ([`PALETTES_LEN`] bytes: 256
    /// colours for each map, then each again once Special World is
    /// passed), if the ROM has them.
    pub palettes: Option<Vec<u8>>,
}

/// Lunar Magic's overworld palettes: 14 of 256 colours.
pub const PALETTES_LEN: usize = 14 * 0x200;
/// The `JSL` of its palette load, at `CODE_00AD25`'s `STY $00`; the
/// palettes' pointer is in the code it leads to, the low word at `+$12` and
/// the bank at `+$1E`.
pub const PALETTE_HOOK: SnesAddr = SnesAddr::new(0x00_AD32);

/// Where the pointer to Lunar Magic's overworld palettes is: its low word's
/// place and its bank's, `None` without the hook's `JSL`.
pub fn palette_pointer(rom: &Rom) -> Result<Option<(SnesAddr, SnesAddr)>, RomError> {
    if rom.read_u8(PALETTE_HOOK)? != 0x22 {
        return Ok(None);
    }
    let code = SnesAddr::new(rom.read_u24(PALETTE_HOOK.add(1))?);
    Ok(Some((code.add(0x12), code.add(0x1E))))
}

fn read_palettes(rom: &Rom) -> Result<Option<Vec<u8>>, RomError> {
    let Some((low, bank)) = palette_pointer(rom)? else {
        return Ok(None);
    };
    let at = SnesAddr::from_bank_offset(rom.read_u8(bank)?, rom.read_u16(low)?);
    // A pointer that leads nowhere readable is no palettes.
    Ok(rom.read(at, PALETTES_LEN).ok().map(<[u8]>::to_vec))
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
        let player = |p: u32| -> Result<Start, RomError> {
            Ok(Start {
                submap: rom.read_u8(START_PLAYERS.add(p))?,
                x: rom.read_u16(START_PLAYERS.add(6 + 4 * p))?,
                y: rom.read_u16(START_PLAYERS.add(8 + 4 * p))?,
            })
        };
        let start = [player(0)?, player(1)?];
        let level_flags = match layout {
            Layout::LunarMagic => rom.read(LEVEL_FLAGS, NAMES)?.to_vec(),
            // The game's 8 pairs, as its new game writes them.
            Layout::Game => {
                let mut flags = vec![0; NAMES];
                for i in 0..OPENED as u32 {
                    let t = usize::from(rom.read_u8(START_OPENED.add(2 * i))?);
                    if let Some(slot) = flags.get_mut(t) {
                        *slot = rom.read_u8(START_OPENED.add(2 * i + 1))?;
                    }
                }
                flags
            }
        };
        let level_events = rom.read(LEVEL_EVENTS, NAMES)?.to_vec();
        let mut tables = Vec::with_capacity(TABLES.len());
        for table in &TABLES {
            tables.push(rom.read(table.at, table.len)?.to_vec());
        }
        let palettes = read_palettes(rom)?;
        let tiles = read_tiles(rom)?;
        Ok(Self {
            layout,
            layer1,
            translevels,
            directions,
            layer2,
            names,
            events,
            start,
            level_flags,
            level_events,
            tiles,
            tables,
            palettes,
        })
    }
}

/// Layer 1's 16x16 tiles, where the tilemap build reads them: the game's
/// [`GAME_TILES`], or as many as the RATS block there holds.
fn read_tiles(rom: &Rom) -> Result<Vec<[u16; 4]>, OverworldError> {
    let at = read_word_ptr(rom, operands::TILES, operands::TILES_BANK)?;
    let count = crate::level::block_len(rom, at).map_or(GAME_TILES, |len| (len / 8).min(MAX_TILES));
    let bytes = rom.read(at, 8 * count)?;
    Ok(bytes
        .chunks(8)
        .map(|t| std::array::from_fn(|i| u16::from_le_bytes([t[2 * i], t[2 * i + 1]])))
        .collect())
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

fn read_events(rom: &Rom, _layout: Layout) -> Result<Events, OverworldError> {
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
    let extras = read_extras(rom, count)?;
    let split = rom.read_u16(operands::EVENT_SPLIT)?;
    let used = tiles
        .iter()
        .chain(extras.iter().flatten().filter_map(|e| match e {
            Extra::Layer2(t) => Some(t),
            Extra::Layer1 { .. } => None,
        }))
        .map(|t| usize::from(t.data) + t.tiles(split))
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
    // The game goes through 16; Lunar Magic's layout through 24, taking
    // the eight bytes and words past the game's tables as the rest, which
    // reading 24 here keeps for both.
    let crushes = LUNAR_MAGIC_CRUSHES;
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
        extras,
        split,
    })
}

/// The pointer places of the events' further tiles' tables, in Lunar
/// Magic's layout: the ranges, data, places, and kinds; `None` without its
/// `JSL`.
pub fn extra_pointers(rom: &Rom) -> Result<Option<[SnesAddr; 4]>, RomError> {
    if rom.read_u8(lunar_magic::EXTRA_HOOK)? != 0x22 {
        return Ok(None);
    }
    let code = SnesAddr::new(rom.read_u24(lunar_magic::EXTRA_HOOK.add(1))?);
    Ok(Some(
        [
            lunar_magic::EXTRA_RANGES,
            lunar_magic::EXTRA_DATA,
            lunar_magic::EXTRA_PLACES,
            lunar_magic::EXTRA_KINDS,
        ]
        .map(|k| code.add(k)),
    ))
}

/// Each event's further tiles. The game's table is a list the game goes
/// through from its last entry down, making each entry of the event; Lunar
/// Magic's, each event's entries in that order, its ranges in bytes of the
/// word tables.
fn read_extras(rom: &Rom, events: usize) -> Result<Vec<Vec<Extra>>, OverworldError> {
    let extra = |kind: u8, data: u16, place: u16, pages: bool| {
        if kind & 1 != 0 {
            Extra::Layer2(EventTile { data, place })
        } else {
            Extra::Layer1 {
                place,
                tile: if pages { data } else { data & 0xFF },
            }
        }
    };
    let mut extras = vec![Vec::new(); events];
    if let Some(pointers) = extra_pointers(rom)? {
        let [ranges, data, places, kinds] = pointers.map(|p| rom.read_u24(p).map(SnesAddr::new));
        let (ranges, data, places, kinds) = (ranges?, data?, places?, kinds?);
        for (e, list) in extras.iter_mut().enumerate() {
            let from = u32::from(rom.read_u16(ranges.add(2 * e as u32))?);
            let to = u32::from(rom.read_u16(ranges.add(2 * e as u32 + 2))?);
            for at in (from..to).step_by(2) {
                list.push(extra(
                    rom.read_u8(kinds.add(at / 2))?,
                    rom.read_u16(data.add(at))?,
                    rom.read_u16(places.add(at))?,
                    true,
                ));
            }
        }
    } else {
        let operand =
            |at: SnesAddr| -> Result<SnesAddr, RomError> { Ok(SnesAddr::new(rom.read_u24(at)?)) };
        let last = u32::from(rom.read_u8(operands::EXTRA_LAST)?);
        let (events_at, kinds, data, places) = (
            operand(operands::EXTRA_EVENTS)?,
            operand(operands::EXTRA_KINDS)?,
            operand(operands::EXTRA_DATA)?,
            operand(operands::EXTRA_PLACES)?,
        );
        for i in (0..=last).rev() {
            let event = usize::from(rom.read_u8(events_at.add(i))?);
            if let Some(list) = extras.get_mut(event) {
                list.push(extra(
                    rom.read_u8(kinds.add(i))?,
                    rom.read_u16(data.add(2 * i))?,
                    rom.read_u16(places.add(2 * i))?,
                    false,
                ));
            }
        }
    }
    Ok(extras)
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

/// What two loads of an overworld leave differently in the tables the
/// overworld is drawn from: a line per table that differs.
pub fn load_differences(
    a: &crate::expand::LoadedOverworld,
    b: &crate::expand::LoadedOverworld,
) -> Vec<String> {
    use crate::ram::RamAddr;
    let mut tables: Vec<(&str, Vec<u8>, Vec<u8>)> = [
        ("layer 1", 0x7E_C800, LAYER1_TILES),
        ("layer 1 pages", 0x7F_C800, LAYER1_TILES),
        ("translevels", 0x7E_D000, LAYER1_TILES),
        ("directions", 0x7E_D800, LAYER1_TILES),
        ("layer 2", 0x7F_4000, 2 * LAYER2_TILES),
        // The stripe image the level's name went out in (CODE_049D07).
        ("the name's stripe image", 0x7F_837D, 4 + 2 * NAME_TILES + 1),
    ]
    .into_iter()
    .map(|(what, at, len)| {
        (
            what,
            a.ram.bytes(RamAddr::new(at), len),
            b.ram.bytes(RamAddr::new(at), len),
        )
    })
    .collect();
    // The name on layer 3, from VRAM word $508B.
    let name = 2 * 0x508B..2 * (0x508B + NAME_TILES);
    tables.push((
        "the name in VRAM",
        a.vram[name.clone()].to_vec(),
        b.vram[name].to_vec(),
    ));
    tables
        .into_iter()
        .filter_map(|(what, x, y)| {
            let differ = x.iter().zip(&y).filter(|(p, q)| p != q).count();
            (differ > 0).then(|| format!("{what}: {differ} bytes differ"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_1_places_and_indices_agree() {
        for i in 0..LAYER1_TILES {
            let (map, x, y) = layer1_place(i);
            assert_eq!(layer1_index(map, x, y), i);
        }
        // The second 16x16 block across, and the submaps' map.
        assert_eq!(layer1_index(0, 16, 0), 0x100);
        assert_eq!(layer1_index(0, 0, 16), 0x200);
        assert_eq!(layer1_index(1, 0, 0), 0x400);
        assert_eq!(layer2_index(0, 32, 0), 0x400);
        assert_eq!(layer2_index(1, 0, 0), 0x1000);
    }

    #[test]
    fn run_length_encoding_runs_and_copies() {
        let data = [1, 1, 1, 1, 2, 3, 3, 4];
        assert_eq!(encode_run_length(&data), [0x83, 1, 0x03, 2, 3, 3, 4]);
        let long = vec![7u8; 300];
        assert_eq!(encode_run_length(&long), [0xFF, 7, 0xFF, 7, 0xAB, 7]);
    }

    #[test]
    fn an_event_tile_s_vram_address_is_its_word_in_layer_1_s_tilemap() {
        // The game's event 3, at 9, 6 of the submaps' map.
        assert_eq!(event_vram(9, 6), 0x9221);
        // The second screen across and down: $2000 + $400 + $800, a row of
        // 16x16 tiles (two of 8x8) down, and two 8x8 tiles across.
        assert_eq!(event_vram(18, 17), 0x2C44u16.swap_bytes());
    }

    #[test]
    fn layer_2_places_and_indices_agree() {
        for map in 0..MAPS {
            for y in 0..LAYER2_SIZE {
                for x in 0..LAYER2_SIZE {
                    let offset = (layer2_index(map, x, y) * 2) as u16;
                    assert_eq!(layer2_place(offset), (map, x, y));
                }
            }
        }
    }

    #[test]
    fn a_block_steps_into_the_next_screen_across_and_down() {
        let block = EventBlock {
            place: (layer2_index(0, 30, 30) * 2) as u16,
            tiles: vec![0; 36],
        };
        let cells: Vec<_> = block.offsets().into_iter().map(layer2_place).collect();
        assert_eq!(cells[0], (0, 30, 30));
        assert_eq!(cells[2], (0, 32, 30));
        assert_eq!(cells[6], (0, 30, 31));
        assert_eq!(cells[12], (0, 30, 32));
        assert_eq!(cells[35], (0, 35, 35));
    }

    #[test]
    fn an_event_tile_is_a_6x6_block_below_900_and_2x2_from_it() {
        assert_eq!(
            EventTile {
                data: 0x8FC,
                place: 0
            }
            .tiles(SMALL_BLOCKS),
            36
        );
        assert_eq!(
            EventTile {
                data: 0x900,
                place: 0
            }
            .tiles(SMALL_BLOCKS),
            4
        );
    }
}

/// Encodes `data` in the game's run-length format ([`run_length`]): a run
/// of three or more equal bytes as a fill, the rest as copies, 128 bytes
/// at most a command.
pub fn encode_run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut literal: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literal: &mut Vec<u8>| {
        for chunk in literal.chunks(128) {
            out.push(chunk.len() as u8 - 1);
            out.extend_from_slice(chunk);
        }
        literal.clear();
    };
    let mut i = 0;
    while i < data.len() {
        let run = data[i..]
            .iter()
            .take(128)
            .take_while(|&&b| b == data[i])
            .count();
        if run >= 3 {
            flush(&mut out, &mut literal);
            out.push(0x80 | (run as u8 - 1));
            out.push(data[i]);
            i += run;
        } else {
            literal.push(data[i]);
            i += 1;
        }
    }
    flush(&mut out, &mut literal);
    out
}

/// How a pointer to a placed block is written: the block's address, plus
/// `offset`, as three bytes (low first), its low word, or its bank byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pointer {
    pub at: SnesAddr,
    pub form: Form,
    pub offset: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Long,
    Word,
    Bank,
}

impl Pointer {
    fn new(at: SnesAddr, form: Form) -> Self {
        Self {
            at,
            form,
            offset: 0,
        }
    }

    /// The bytes it writes for a block at `block`.
    pub fn bytes(&self, block: SnesAddr) -> Vec<u8> {
        let to = block.raw() + self.offset;
        match self.form {
            Form::Long => to.to_le_bytes()[..3].to_vec(),
            Form::Word => (to as u16).to_le_bytes().to_vec(),
            Form::Bank => vec![(to >> 16) as u8],
        }
    }
}

/// What writing an overworld in Lunar Magic's layout comes to: bytes at
/// fixed places, and blocks to place in free space (each within a bank)
/// with the pointers that lead to them.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Plan {
    pub fixed: Vec<(SnesAddr, Vec<u8>)>,
    pub blocks: Vec<(&'static str, Vec<u8>, Vec<Pointer>)>,
}

/// The events Lunar Magic's layout has.
pub const LUNAR_MAGIC_EVENTS: usize = 0x78;
/// The crushed tiles it has.
pub const LUNAR_MAGIC_CRUSHES: usize = 0x18;

/// The game's instructions that read the event tables, by table, beyond
/// those [`operands`] names (each a `LDA.l`'s or `CMP.l`'s operand).
mod event_reads {
    use crate::addr::SnesAddr;
    /// `DATA_04D85D`: an event's layer 1 place.
    pub const LAYER1: [SnesAddr; 6] = [
        SnesAddr::new(0x04_DA74),
        SnesAddr::new(0x04_EC8C),
        SnesAddr::new(0x04_ECBA),
        SnesAddr::new(0x04_ECC5),
        SnesAddr::new(0x04_ED97),
        SnesAddr::new(0x04_EDBE),
    ];
    /// `DATA_04D93D`: its VRAM address.
    pub const VRAM: [SnesAddr; 1] = [SnesAddr::new(0x04_EDB8)];
    /// `DATA_04DD8D`, the entries' first word, and `DATA_04DD8F`, their
    /// second.
    pub const ENTRY_DATA: [SnesAddr; 3] = [
        SnesAddr::new(0x04_E49F),
        SnesAddr::new(0x04_E709),
        SnesAddr::new(0x04_EE5A),
    ];
    pub const ENTRY_PLACE: [SnesAddr; 3] = [
        SnesAddr::new(0x04_E4A4),
        SnesAddr::new(0x04_E710),
        SnesAddr::new(0x04_EE3F),
    ];
}

impl Overworld {
    /// The overworld in Lunar Magic's layout (lunar-magic-install.md, "The
    /// overworld"). Kobo's code for the layout (`asm/lunar-magic/overworld.asm`)
    /// reads the tables only it reads through the same pointer places.
    /// `rom` is the ROM it is written into, with Kobo's code for the
    /// layout installed, whose code holds some of the pointers.
    pub fn plan(&self, rom: &Rom) -> Result<Plan, OverworldError> {
        let events = &self.events;
        if events.count() != LUNAR_MAGIC_EVENTS || events.crush.len() != LUNAR_MAGIC_CRUSHES {
            return Err(OverworldError::Decode(
                "events, which in Lunar Magic's layout are $78 with 24 crushed tiles",
            ));
        }
        let mut plan = Plan::default();
        let low: Vec<u8> = self.layer1.iter().map(|&t| t as u8).collect();
        let high: Vec<u8> = self.layer1.iter().map(|&t| (t >> 8) as u8).collect();
        plan.fixed.push((LAYER1_DATA, low));
        let lz2 = |data: &[u8], what| {
            crate::compress::lz2::compress(data).map_err(|_| OverworldError::Decode(what))
        };
        let word_and_bank = |word, bank| {
            vec![
                Pointer::new(word, Form::Word),
                Pointer::new(bank, Form::Bank),
            ]
        };
        plan.blocks.push((
            "layer 1 pages",
            lz2(&high, "layer 1 pages")?,
            word_and_bank(lunar_magic::LAYER1_HIGH, lunar_magic::LAYER1_HIGH_BANK),
        ));
        let both: Vec<u8> = self
            .translevels
            .iter()
            .chain(&self.directions)
            .copied()
            .collect();
        plan.blocks.push((
            "translevels",
            lz2(&both, "translevels")?,
            word_and_bank(lunar_magic::TRANSLEVELS, lunar_magic::TRANSLEVELS_BANK),
        ));
        // Layer 2's two streams in one block, as the game reads them from
        // one bank: the properties' place is the numbers' end.
        let numbers: Vec<u8> = self.layer2.iter().map(|&w| w as u8).collect();
        let properties: Vec<u8> = self.layer2.iter().map(|&w| (w >> 8) as u8).collect();
        let mut stream = encode_run_length(&numbers);
        let split = stream.len() as u32;
        stream.extend(encode_run_length(&properties));
        let mut layer2 = word_and_bank(operands::LAYER2_NUMBERS, operands::LAYER2_BANK);
        layer2.push(Pointer {
            at: operands::LAYER2_PROPERTIES,
            form: Form::Word,
            offset: split,
        });
        plan.blocks.push(("layer 2", stream, layer2));
        let names: Vec<u8> = self.names.iter().flatten().copied().collect();
        plan.blocks.push((
            "level names",
            names,
            vec![Pointer::new(lunar_magic::NAMES, Form::Long)],
        ));
        // The events.
        plan.fixed
            .push((operands::EVENT_COUNT, vec![LUNAR_MAGIC_EVENTS as u8]));
        plan.fixed.push((SINGLE_REVEAL, vec![0x80]));
        let words = |values: &mut dyn Iterator<Item = u16>| -> Vec<u8> {
            values.flat_map(u16::to_le_bytes).collect()
        };
        let long =
            |reads: &[SnesAddr]| reads.iter().map(|&r| Pointer::new(r, Form::Long)).collect();
        plan.blocks.push((
            "event layer 1 places",
            words(&mut events.layer1.iter().map(|e| e.0)),
            long(&event_reads::LAYER1),
        ));
        plan.blocks.push((
            "event layer 1 VRAM",
            words(&mut events.layer1.iter().map(|e| e.1)),
            long(&event_reads::VRAM),
        ));
        let mut ranges = Vec::new();
        for r in &events.ranges {
            ranges.extend(r.to_le_bytes());
        }
        plan.fixed.push((EVENT_RANGES, ranges));
        let mut entries = Vec::new();
        for t in &events.tiles {
            entries.extend(t.data.to_le_bytes());
            entries.extend(t.place.to_le_bytes());
        }
        let mut entry_reads: Vec<Pointer> = long(&event_reads::ENTRY_DATA);
        entry_reads.extend(event_reads::ENTRY_PLACE.iter().map(|&r| Pointer {
            at: r,
            form: Form::Long,
            offset: 2,
        }));
        plan.blocks
            .push(("event tile entries", entries, entry_reads));
        plan.blocks.push((
            "event tile numbers",
            events.numbers.clone(),
            vec![
                Pointer::new(operands::EVENT_NUMBERS, Form::Long),
                Pointer::new(EVENT_NUMBERS_BANK, Form::Bank),
                Pointer::new(EVENT_NUMBERS_WORD, Form::Word),
            ],
        ));
        // The game's decoder (CODE_04DD57) takes a run before it looks for
        // the $FFFF that ends them, so a stream with none would be read on
        // into whatever follows it: an overworld without event tiles has
        // one byte.
        let properties: &[u8] = if events.properties.is_empty() {
            &[0]
        } else {
            &events.properties
        };
        let mut props = encode_run_length(properties);
        props.extend([0xFF, 0xFF]);
        plan.blocks.push((
            "event tile properties",
            props,
            word_and_bank(operands::EVENT_PROPERTIES, operands::EVENT_PROPERTIES_BANK),
        ));
        plan.blocks.push((
            "crushed tiles' events",
            events.crush.iter().map(|c| c.event).collect(),
            vec![Pointer::new(operands::CRUSH_EVENTS, Form::Long)],
        ));
        plan.blocks.push((
            "crushed tiles' places",
            words(&mut events.crush.iter().map(|c| c.place)),
            vec![Pointer::new(operands::CRUSH_PLACES, Form::Long)],
        ));
        plan.blocks.push((
            "crushed tiles' VRAM",
            words(&mut events.crush.iter().map(|c| c.vram)),
            vec![Pointer::new(operands::CRUSH_VRAM, Form::Long)],
        ));
        // The further tiles: the ranges in bytes of the word tables.
        let Some([ranges_at, data_at, places_at, kinds_at]) = extra_pointers(rom)? else {
            return Err(OverworldError::Decode(
                "events' further tiles, whose code is not installed",
            ));
        };
        let (mut ranges, mut data, mut places, mut kinds) =
            (vec![0u8, 0], Vec::new(), Vec::new(), Vec::new());
        for list in &events.extras {
            for extra in list {
                let (kind, d, place) = match *extra {
                    Extra::Layer1 { place, tile } => (0u8, tile, place),
                    Extra::Layer2(t) => (1, t.data, t.place),
                };
                kinds.push(kind);
                data.extend(d.to_le_bytes());
                places.extend(place.to_le_bytes());
            }
            ranges.extend((data.len() as u16).to_le_bytes());
        }
        if data.len() > 0xFFFF {
            return Err(OverworldError::Full("events' further tiles"));
        }
        for (what, bytes, at) in [
            ("further tiles' ranges", ranges, ranges_at),
            ("further tiles' data", data, data_at),
            ("further tiles' places", places, places_at),
            ("further tiles' kinds", kinds, kinds_at),
        ] {
            plan.blocks
                .push((what, bytes, vec![Pointer::new(at, Form::Long)]));
        }
        // Where a new game starts: the submaps, then the positions in
        // pixels and in tiles, the animation words between left as they are.
        if self.level_flags.len() != NAMES {
            return Err(OverworldError::Decode(
                "level settings, of which there are $60",
            ));
        }
        plan.fixed.push((LEVEL_FLAGS, self.level_flags.clone()));
        if self.level_events.len() != NAMES {
            return Err(OverworldError::Decode(
                "level events, of which there are $60",
            ));
        }
        plan.fixed.push((LEVEL_EVENTS, self.level_events.clone()));
        for (table, bytes) in TABLES.iter().zip(&self.tables) {
            plan.fixed.push((table.at, bytes.clone()));
        }
        // Layer 1's 16x16 tiles, as many as there are, where the game's
        // three instructions that find them point, with the bank Lunar
        // Magic reads besides.
        if self.tiles.len() > MAX_TILES {
            return Err(OverworldError::Full("16x16 tiles"));
        }
        let mut tiles = word_and_bank(operands::TILES, operands::TILES_BANK);
        tiles.push(Pointer::new(operands::TILES_POINTERS, Form::Word));
        tiles.push(Pointer::new(lunar_magic::TILES_BANK, Form::Bank));
        plan.blocks.push((
            "layer 1's 16x16 tiles",
            self.tiles
                .iter()
                .flatten()
                .flat_map(|w| w.to_le_bytes())
                .collect(),
            tiles,
        ));
        plan.fixed
            .push((operands::EVENT_SPLIT, events.split.to_le_bytes().to_vec()));
        plan.fixed
            .push((operands::FADE_SPLIT, events.split.to_le_bytes().to_vec()));
        plan.fixed
            .push((START_PLAYERS, self.start.iter().map(|p| p.submap).collect()));
        let positions = |f: &dyn Fn(&Start) -> [u16; 2]| -> Vec<u8> {
            self.start
                .iter()
                .flat_map(f)
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        plan.fixed
            .push((START_PLAYERS.add(6), positions(&|p| [p.x, p.y])));
        plan.fixed
            .push((START_PLAYERS.add(14), positions(&|p| [p.x >> 4, p.y >> 4])));
        // Lunar Magic's palettes, whose pointer is in Kobo's code for them.
        if let Some(palettes) = &self.palettes {
            let Some((low, bank)) = palette_pointer(rom)? else {
                return Err(OverworldError::Decode(
                    "overworld palettes, whose code is not installed",
                ));
            };
            plan.blocks.push((
                "overworld palettes",
                palettes.clone(),
                vec![
                    Pointer::new(low, Form::Word),
                    Pointer::new(bank, Form::Bank),
                ],
            ));
        }
        let (from, to): (Vec<u8>, Vec<u8>) = events.reveal.iter().copied().unzip();
        plan.fixed.push((REVEAL_FROM, from));
        plan.fixed.push((REVEAL_TO, to));
        // An overworld whose events change no layer 2 tile has no entries
        // or tile data; a RATS block holds a byte at least, which nothing
        // reads then.
        for (_, bytes, _) in &mut plan.blocks {
            if bytes.is_empty() {
                bytes.push(0);
            }
        }
        Ok(plan)
    }
}

/// The `BNE` of `CODE_04DA49` that writes a second tile for the last reveal
/// entry; Lunar Magic's layout has a `BRA` (`$80`), every entry one tile.
pub const SINGLE_REVEAL: SnesAddr = SnesAddr::new(0x04_DA98);
/// The event tile numbers' other use (`CODE_04E4A9`): `LDA #$0C`, their
/// bank, and `LDA #$8000`, their address.
pub const EVENT_NUMBERS_BANK: SnesAddr = SnesAddr::new(0x04_E4B0);
pub const EVENT_NUMBERS_WORD: SnesAddr = SnesAddr::new(0x04_E4BB);

/// The two maps: the main map, and the submaps' map, which all six share.
pub const MAPS: u8 = 2;
/// A map's size in layer 1's 16x16 tiles, and in layer 2's 8x8 ones.
pub const LAYER1_SIZE: u8 = 32;
pub const LAYER2_SIZE: u8 = 64;

/// The index into layer 1's tables of tile (`x`, `y`) of `map`: 16x16-tile
/// blocks of 256, two across and two down, the submaps' map `$400` after
/// the main map's (`OW_TilePos_Calc`).
pub fn layer1_index(map: u8, x: u8, y: u8) -> usize {
    usize::from(map) * 0x400
        + usize::from(y >> 4) * 0x200
        + usize::from(x >> 4) * 0x100
        + usize::from(y & 15) * 16
        + usize::from(x & 15)
}

/// The map and tile of layer 1's index `i`.
pub fn layer1_place(i: usize) -> (u8, u8, u8) {
    let map = (i / 0x400) as u8;
    let i = i % 0x400;
    let x = ((i / 0x100) & 1) * 16 + i % 16;
    let y = (i / 0x200) * 16 + (i % 0x100) / 16;
    (map, x as u8, y as u8)
}

/// The index into layer 2's tiles of tile (`x`, `y`) of `map`: 32x32-tile
/// screens of `$800` bytes, two across and two down, the submaps' `$2000`
/// bytes after the main map's (`CODE_04E4D0`'s stepping).
pub fn layer2_index(map: u8, x: u8, y: u8) -> usize {
    (usize::from(map) * 0x2000
        + usize::from(y >> 5) * 0x1000
        + usize::from(x >> 5) * 0x800
        + usize::from(y & 31) * 0x40
        + usize::from(x & 31) * 2)
        / 2
}

/// The VRAM address of layer 1's tilemap word for 16x16 tile (`x`, `y`)
/// of either map, as the events' table keeps it (`DATA_04D93D`): high byte
/// first, from `$2000`, the tilemap's 32x32 screens two across and two
/// down. Every event of the game's has its tile's but `$14`, a row above.
pub fn event_vram(x: u8, y: u8) -> u16 {
    let (x, y) = (u16::from(x), u16::from(y));
    let word = 0x2000
        + if x >= 16 { 0x400 } else { 0 }
        + if y >= 16 { 0x800 } else { 0 }
        + (2 * y % 32) * 32
        + 2 * x % 32;
    word.swap_bytes()
}

/// The map and 8x8 tile of a byte offset into layer 2's tilemap
/// (`$7F4000`): the inverse of [`layer2_index`], twice.
pub fn layer2_place(offset: u16) -> (u8, u8, u8) {
    let at = usize::from(offset) & 0x3FFE;
    let map = (at / 0x2000) as u8;
    let at = at % 0x2000;
    let x = (at / 0x800 % 2) * 32 + at % 0x40 / 2;
    let y = (at / 0x1000) * 32 + at % 0x800 / 0x40;
    (map, x as u8, y as u8)
}

impl EventBlock {
    /// The side of the block in 8x8 tiles: 6, or 2.
    pub fn side(&self) -> usize {
        if self.tiles.len() == 36 { 6 } else { 2 }
    }

    /// The byte offsets into layer 2's tilemap its tiles go to, in their
    /// order: rows of [`EventBlock::side`] from its place, a row past a
    /// screen's last column going on in the next screen across, and a row
    /// past a screen's last row in the next screen down, as the game steps
    /// (`CODE_04E520`).
    pub fn offsets(&self) -> Vec<u16> {
        let side = self.side();
        let mut out = Vec::with_capacity(side * side);
        let mut row = usize::from(self.place);
        for _ in 0..side {
            let mut x = row;
            for _ in 0..side {
                out.push((x % 0x4000) as u16);
                x += 2;
                if x & 0x3F == 0 {
                    x = ((x - 1) & !0x3F) + 0x800;
                }
            }
            let old = row;
            row += 0x40;
            if row & 0x7C0 == 0 {
                row = (old & 0xF83F) + 0x1000;
            }
        }
        out
    }
}

/// One event, as a project holds it: its layer 1 tile's place and VRAM
/// address, and its layer 2 blocks.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Event {
    pub layer1: (u16, u16),
    pub blocks: Vec<EventBlock>,
    pub extras: Vec<ExtraTile>,
}

/// What a project changes of the clean ROM's overworld: rows of each map's
/// layers, level tiles, names, and events; anything else is the clean
/// ROM's, put in Lunar Magic's layout.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Changes {
    /// By map and row: the row's 32 layer 1 tiles.
    pub layer1: std::collections::BTreeMap<(u8, u8), Vec<u16>>,
    /// By map and row: the row's 64 layer 2 tiles.
    pub layer2: std::collections::BTreeMap<(u8, u8), Vec<u16>>,
    /// By map, x, and y: the translevel and direction byte of a place.
    pub levels: std::collections::BTreeMap<(u8, u8, u8), (u8, u8)>,
    pub names: std::collections::BTreeMap<u8, [u8; NAME_TILES]>,
    pub events: std::collections::BTreeMap<u8, Event>,
    pub crush: Option<Vec<Crush>>,
    pub reveal: Option<Vec<(u8, u8)>>,
    pub start: Option<[Start; 2]>,
    /// By translevel: its settings at a new game.
    pub level_flags: std::collections::BTreeMap<u8, u8>,
    /// By translevel: its event.
    pub level_events: std::collections::BTreeMap<u8, u8>,
    /// Where the event tile data's 2x2 blocks start.
    pub event_split: Option<u16>,
    /// Layer 1's 16x16 tiles, by number: those that differ, or that the
    /// clean ROM has none for.
    pub tiles: std::collections::BTreeMap<u16, [u16; 4]>,
    /// The tables kept in place, by name ([`TABLES`]): those that differ.
    pub tables: std::collections::BTreeMap<String, Vec<u8>>,
    /// The submaps' graphics lists (lists `200`-`206` of Lunar Magic's
    /// ExGFX block), by submap, where they differ from what Lunar Magic
    /// writes with nothing set: written with the ExGFX block, not read
    /// from the overworld's tables.
    pub graphics: std::collections::BTreeMap<u8, crate::exgfx::GraphicsList>,
    /// Lunar Magic's overworld palettes ([`PALETTES_LEN`] bytes), when they
    /// differ from the clean ROM's (which has none).
    pub palettes: Option<Vec<u8>>,
    /// Lunar Magic's option to turn the event path fade off, with how much
    /// each frame adds to a step's timer (a step every `$40`): installed as
    /// Kobo's code, not read from the ROM's tables (an import finds it with
    /// `expand::reveal_speed`).
    pub reveal_speed: Option<u8>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        *self == Changes::default()
    }
}

impl Overworld {
    /// The overworld in Lunar Magic's layout's shape: `$78` events, those
    /// past the game's with nothing (as Lunar Magic's own conversion leaves
    /// them).
    pub fn in_lunar_magic_shape(mut self) -> Self {
        let events = &mut self.events;
        // The game's crushed tiles past its 16, which its loop reads from
        // the tables after its own, mostly have places past layer 1's
        // tiles; Lunar Magic's conversion puts those at 0, which, as they
        // were, makes nothing differently in play.
        if self.layout == Layout::Game {
            for crush in events.crush.iter_mut() {
                if usize::from(crush.place) >= LAYER1_TILES {
                    crush.place = 0;
                }
            }
        }
        let last = events.ranges.last().copied().unwrap_or(0);
        while events.layer1.len() < LUNAR_MAGIC_EVENTS {
            events.layer1.push((0, 0));
            events.ranges.push(last);
        }
        events.extras.resize(LUNAR_MAGIC_EVENTS, Vec::new());
        self.layout = Layout::LunarMagic;
        self
    }

    /// Event `n` set as `event`, the event tile data laid out afresh.
    pub fn set_event(&mut self, n: usize, event: Event) -> Result<(), OverworldError> {
        let mut events = self.event_list();
        let Some(slot) = events.get_mut(n) else {
            return Err(OverworldError::Decode("an event past the last"));
        };
        *slot = event;
        let blocks: Vec<Vec<EventBlock>> = events.iter().map(|e| e.blocks.clone()).collect();
        let extras: Vec<Vec<ExtraTile>> = events.iter().map(|e| e.extras.clone()).collect();
        self.events = Events::from_blocks(
            events.iter().map(|e| e.layer1).collect(),
            &blocks,
            &extras,
            self.events.crush.clone(),
            self.events.reveal.clone(),
            self.events.split,
        )?;
        Ok(())
    }

    /// Every event, as a project holds them.
    pub fn event_list(&self) -> Vec<Event> {
        (0..self.events.count())
            .map(|n| Event {
                layer1: self.events.layer1[n],
                blocks: self.events.blocks(n),
                extras: self.events.extra_tiles(n),
            })
            .collect()
    }

    /// What `self` changes of `clean` (both in Lunar Magic's shape).
    pub fn changes_from(&self, clean: &Overworld) -> Changes {
        let mut changes = Changes::default();
        for map in 0..MAPS {
            for row in 0..LAYER1_SIZE {
                let tiles: Vec<u16> = (0..LAYER1_SIZE)
                    .map(|x| self.layer1[layer1_index(map, x, row)])
                    .collect();
                let theirs: Vec<u16> = (0..LAYER1_SIZE)
                    .map(|x| clean.layer1[layer1_index(map, x, row)])
                    .collect();
                if tiles != theirs {
                    changes.layer1.insert((map, row), tiles);
                }
                for x in 0..LAYER1_SIZE {
                    let i = layer1_index(map, x, row);
                    let ours = (self.translevels[i], self.directions[i]);
                    if ours != (clean.translevels[i], clean.directions[i]) {
                        changes.levels.insert((map, x, row), ours);
                    }
                }
            }
            for row in 0..LAYER2_SIZE {
                let tiles: Vec<u16> = (0..LAYER2_SIZE)
                    .map(|x| self.layer2[layer2_index(map, x, row)])
                    .collect();
                let theirs: Vec<u16> = (0..LAYER2_SIZE)
                    .map(|x| clean.layer2[layer2_index(map, x, row)])
                    .collect();
                if tiles != theirs {
                    changes.layer2.insert((map, row), tiles);
                }
            }
        }
        for (t, name) in self.names.iter().enumerate() {
            if clean.names.get(t) != Some(name) {
                changes.names.insert(t as u8, *name);
            }
        }
        let (ours, theirs) = (self.event_list(), clean.event_list());
        for (n, event) in ours.iter().enumerate() {
            if theirs.get(n) != Some(event) {
                changes.events.insert(n as u8, event.clone());
            }
        }
        if self.events.crush != clean.events.crush {
            changes.crush = Some(self.events.crush.clone());
        }
        if self.events.reveal != clean.events.reveal {
            changes.reveal = Some(self.events.reveal.clone());
        }
        if self.start != clean.start {
            changes.start = Some(self.start);
        }
        for (t, (&ours, &theirs)) in self.level_flags.iter().zip(&clean.level_flags).enumerate() {
            if ours != theirs {
                changes.level_flags.insert(t as u8, ours);
            }
        }
        if self.events.split != clean.events.split {
            changes.event_split = Some(self.events.split);
        }
        if self.palettes != clean.palettes {
            changes.palettes = self.palettes.clone();
        }
        for (n, ours) in self.tiles.iter().enumerate() {
            if clean.tiles.get(n) != Some(ours) {
                changes.tiles.insert(n as u16, *ours);
            }
        }
        for ((table, ours), theirs) in TABLES.iter().zip(&self.tables).zip(&clean.tables) {
            if ours != theirs {
                changes.tables.insert(table.name.to_string(), ours.clone());
            }
        }
        for (t, (&ours, &theirs)) in self
            .level_events
            .iter()
            .zip(&clean.level_events)
            .enumerate()
        {
            if ours != theirs {
                changes.level_events.insert(t as u8, ours);
            }
        }
        changes
    }

    /// `self` (the clean ROM's, in Lunar Magic's shape) with `changes`.
    pub fn with(mut self, changes: &Changes) -> Result<Self, OverworldError> {
        for (&(map, row), tiles) in &changes.layer1 {
            for (x, &t) in tiles.iter().enumerate().take(usize::from(LAYER1_SIZE)) {
                self.layer1[layer1_index(map, x as u8, row)] = t;
            }
        }
        for (&(map, row), tiles) in &changes.layer2 {
            for (x, &t) in tiles.iter().enumerate().take(usize::from(LAYER2_SIZE)) {
                self.layer2[layer2_index(map, x as u8, row)] = t;
            }
        }
        for (&(map, x, y), &(translevel, directions)) in &changes.levels {
            let i = layer1_index(map, x, y);
            self.translevels[i] = translevel;
            self.directions[i] = directions;
        }
        for (&t, name) in &changes.names {
            if let Some(slot) = self.names.get_mut(usize::from(t)) {
                *slot = *name;
            }
        }
        let mut events = self.event_list();
        for (&n, event) in &changes.events {
            if let Some(slot) = events.get_mut(usize::from(n)) {
                *slot = event.clone();
            }
        }
        let crush = changes.crush.clone().unwrap_or(self.events.crush.clone());
        let reveal = changes.reveal.clone().unwrap_or(self.events.reveal.clone());
        if let Some(start) = changes.start {
            self.start = start;
        }
        for (&t, &flags) in &changes.level_flags {
            if let Some(slot) = self.level_flags.get_mut(usize::from(t)) {
                *slot = flags;
            }
        }
        if let Some(palettes) = &changes.palettes {
            if palettes.len() != PALETTES_LEN {
                return Err(OverworldError::Decode(
                    "overworld palettes of another length",
                ));
            }
            self.palettes = Some(palettes.clone());
        }
        for (&n, words) in &changes.tiles {
            let n = usize::from(n);
            if n >= MAX_TILES {
                return Err(OverworldError::Full("16x16 tiles"));
            }
            if self.tiles.len() <= n {
                self.tiles.resize(n + 1, [0; 4]);
            }
            self.tiles[n] = *words;
        }
        for (name, bytes) in &changes.tables {
            let Some(i) = TABLES.iter().position(|t| t.name == name) else {
                return Err(OverworldError::Decode("a table no overworld has"));
            };
            if bytes.len() != TABLES[i].len {
                return Err(OverworldError::Decode("a table of another length"));
            }
            self.tables[i] = bytes.clone();
        }
        for (&t, &event) in &changes.level_events {
            if let Some(slot) = self.level_events.get_mut(usize::from(t)) {
                *slot = event;
            }
        }
        let blocks: Vec<Vec<EventBlock>> = events.iter().map(|e| e.blocks.clone()).collect();
        let extras: Vec<Vec<ExtraTile>> = events.iter().map(|e| e.extras.clone()).collect();
        self.events = Events::from_blocks(
            events.iter().map(|e| e.layer1).collect(),
            &blocks,
            &extras,
            crush,
            reveal,
            changes.event_split.unwrap_or(self.events.split),
        )?;
        Ok(self)
    }
}
