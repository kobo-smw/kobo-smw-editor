//! Kobo's code for one piece of Lunar Magic's layout swapped into a ROM
//! Lunar Magic saved, in place of Lunar Magic's, keeping the ROM's tables,
//! so that a hack's content runs through both implementations and the two
//! can be compared (docs/testing.md). Each piece's sites are recorded once,
//! here: the tests use them, and `examples/swap.rs` writes such a ROM for
//! the probes.
//!
//! A swap puts the piece's sites back to the game's bytes (`base`'s: the
//! clean ROM, or for an SA-1 ROM the clean ROM with SA-1 Pack), clears the
//! operands its patch autocleans (so that Asar frees nothing of the ROM's),
//! applies the patch, and copies the tables the patch initialises back from
//! the ROM. Nothing of Lunar Magic's code is read: bytes are copied between
//! ROMs by address. The checksum is left to the caller.
//! Included by `examples/swap.rs` too.
#![allow(dead_code)]

use kobo_core::asar::{Asar, AsarError};
use kobo_core::{Rom, SnesAddr, install};

/// A piece of Lunar Magic's layout that Kobo has code for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece {
    /// Bank `$06` and what goes with it: the Map16 routine, the acts-like
    /// chain, the placed objects, the level number, backgrounds, custom
    /// palettes, and screen exits. `entrance.asm` is left out: swapped in
    /// alone it changes `$5B` and `$0BE7`, which come with the hook at
    /// `$05DA17` (docs/lunar-magic-install.md).
    Bank06,
    /// The VRAM patch (`vram.asm`).
    Vram,
    /// The graphics loader (`graphics.asm`): 4bpp GFX, ExGFX, the lists.
    Graphics,
    /// Layer 3 settings (`layer3.asm`).
    Layer3,
    /// The sprite loader (`sprites.asm`).
    Sprites,
    /// ExAnimation (`exanimation.asm`).
    ExAnimation,
    /// Taller levels (`exlevel.asm`), the size table moved to where Kobo's
    /// code has it.
    ExLevel,
    /// Screen exits and entrances (`exits.asm`, `entrance.asm`).
    Entrances,
}

impl Piece {
    pub const ALL: [Piece; 8] = [
        Piece::Bank06,
        Piece::Vram,
        Piece::Graphics,
        Piece::Layer3,
        Piece::Sprites,
        Piece::ExAnimation,
        Piece::ExLevel,
        Piece::Entrances,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Piece::Bank06 => "bank06",
            Piece::Vram => "vram",
            Piece::Graphics => "graphics",
            Piece::Layer3 => "layer3",
            Piece::Sprites => "sprites",
            Piece::ExAnimation => "exanim",
            Piece::ExLevel => "exlevel",
            Piece::Entrances => "entrances",
        }
    }

    pub fn from_name(name: &str) -> Option<Piece> {
        Piece::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// The sites of Lunar Magic's sprite loader group, inclusive (docs/
/// lunar-magic-install.md, "Sprites"): Kobo's patch does not write every
/// byte Lunar Magic's took.
pub const SPRITE_LOADER: [(u32, u32); 18] = [
    (0x02A826, 0x02A83B),
    (0x02A846, 0x02A84D),
    (0x02A95B, 0x02A95E),
    (0x02A968, 0x02A968),
    (0x02A9D7, 0x02A9D9),
    (0x02AA61, 0x02AA64),
    (0x02AB54, 0x02AB58),
    (0x02ABD0, 0x02ABD4),
    (0x02AC64, 0x02AC67),
    (0x02ACA4, 0x02ACA7),
    (0x02AF3D, 0x02AF40),
    (0x02AFA7, 0x02AFAA),
    (0x01AC40, 0x01AC49),
    (0x02D03A, 0x02D043),
    (0x02FED6, 0x02FEDF),
    (0x03B86C, 0x03B875),
    (0x01C08C, 0x01C093),
    (0x01C0E2, 0x01C0E2),
];

/// Layer 3 settings' hooks, inclusive (docs/lunar-magic-install.md,
/// "Layer 3 settings"): the setup, the overworld's, the sprite edge tile,
/// and the frame's scroll.
pub const LAYER3: [(u32, u32); 4] = [
    (0x00A01F, 0x00A024),
    (0x00A153, 0x00A156),
    (0x0194B6, 0x0194BA),
    (0x05C40C, 0x05C410),
];

/// The VRAM patch's group (docs/lunar-magic-install.md, "Graphics"), and
/// `CODE_00C0FB`'s branches, which Kobo's tile address code answers as the
/// game's do; inclusive.
pub const VRAM: [(u32, u32); 10] = [
    (0x008072, 0x008074),
    (0x00BA56, 0x00BA5C),
    (0x0081E2, 0x0081E5),
    (0x008209, 0x00820C),
    (0x0085D2, 0x0085DE),
    (0x00A5A2, 0x00A5A5),
    (0x00F6E4, 0x00F6E7),
    (0x0580A9, 0x0580AC),
    (0x0586F7, 0x0586FA),
    (0x00C116, 0x00C123),
];

/// Lunar Magic's 4bpp and ExGFX pieces outside what `graphics.asm`
/// replaces, inclusive: the credits, cutscenes, overworld, MARIO START and
/// Mode 7 boss conversions, and the switch palace blocks' RAM
/// (docs/lunar-magic-install.md, "Graphics").
pub const GRAPHICS: [(u32, u32); 10] = [
    (0x0093F7, 0x0093FB),
    (0x0095E9, 0x0095F5),
    (0x00A82F, 0x00A832),
    (0x03DDC8, 0x03DDCB),
    (0x048000, 0x0480D0),
    (0x04F2B6, 0x04F3CD),
    (0x009471, 0x009475),
    (0x00A140, 0x00A14C),
    (0x049DFD, 0x049E00),
    (0x00A873, 0x00A876),
];

/// The graphics tables `graphics.asm` initialises, kept from the ROM
/// (start, end exclusive).
pub const GRAPHICS_TABLES: [(u32, u32); 4] = [
    (0x0FF200, 0x0FF780),
    (0x0FF7FF, 0x0FF802),
    (0x0FF873, 0x0FF876),
    (0x0FF937, 0x0FF93A),
];

/// Every site of Lunar Magic's taller levels piece, inclusive: the bytes a
/// save leaves alone when `$05DA8A` is a `JSL` and rewrites when it is not
/// (docs/lunar-magic-install.md), but for GenerateTile's VRAM address and
/// its view checks (`$00BF35`-`$00BF9A`, `$00C116`, `$00C122`), which
/// belong to the VRAM patch in Kobo's layout.
pub const EXLEVEL: [(u32, u32); 59] = [
    (0x00A1CF, 0x00A1D2),
    (0x00A2AF, 0x00A2D4),
    (0x00BDA8, 0x00BEA7),
    (0x00BEE8, 0x00BEEE),
    (0x00C07D, 0x00C07D),
    (0x00C0CA, 0x00C0CA),
    (0x00C1B5, 0x00C1B5),
    (0x00C3D7, 0x00C3D7),
    (0x00F478, 0x00F47A),
    (0x00F493, 0x00F494),
    (0x00F49B, 0x00F49C),
    (0x00F4F3, 0x00F4F5),
    (0x00F50E, 0x00F50F),
    (0x00F516, 0x00F517),
    (0x00F70D, 0x00F710),
    (0x0194D6, 0x0194D8),
    (0x019501, 0x019502),
    (0x01950A, 0x01950B),
    (0x019513, 0x019514),
    (0x01951C, 0x01951D),
    (0x01D97C, 0x01D97D),
    (0x01D982, 0x01D983),
    (0x0292D7, 0x0292DA),
    (0x0292FA, 0x0292FB),
    (0x029302, 0x029303),
    (0x02930B, 0x02930C),
    (0x029313, 0x029314),
    (0x02950B, 0x02950F),
    (0x0295BE, 0x0295C1),
    (0x0295ED, 0x0295EE),
    (0x0295F5, 0x0295F6),
    (0x0295FE, 0x0295FF),
    (0x029606, 0x029607),
    (0x02A6BB, 0x02A6BC),
    (0x02A6C3, 0x02A6C4),
    (0x02A6CC, 0x02A6CD),
    (0x02A6D4, 0x02A6D5),
    (0x02BA4E, 0x02BA51),
    (0x02BA72, 0x02BA73),
    (0x02BA7A, 0x02BA7B),
    (0x02BA83, 0x02BA84),
    (0x02BA8B, 0x02BA8C),
    (0x02D158, 0x02D15B),
    (0x02D18D, 0x02D18E),
    (0x02D195, 0x02D196),
    (0x02D19E, 0x02D19F),
    (0x02D1A6, 0x02D1A7),
    (0x03D793, 0x03D795),
    (0x058A1A, 0x058A1A),
    (0x058AF6, 0x058AF6),
    (0x058BE8, 0x058BE8),
    (0x058CDB, 0x058CDB),
    (0x05D8FC, 0x05D8FC),
    (0x05D9A1, 0x05D9A4),
    (0x05DA8A, 0x05DA8D),
    (0x05DB5F, 0x05DB62),
    (0x0C9436, 0x0C9439),
    (0x0DA963, 0x0DA973),
    (0x0DA9D6, 0x0DAA04),
];

/// Bank `$06`'s tables and the page pointers, kept from the ROM: the
/// Map16 page tables, the acts-like tables' pointers, and the operands of
/// the routine's table loads (start, length).
pub const BANK06_TABLES: [(u32, usize); 23] = [
    (0x0EF310, 512),
    (0x0EFD50, 48),
    (0x06F547, 1),
    (0x06F553, 2),
    (0x06F557, 1),
    (0x06F55C, 2),
    (0x06F560, 1),
    (0x06F567, 2),
    (0x06F56B, 1),
    (0x06F570, 2),
    (0x06F574, 1),
    (0x06F586, 2),
    (0x06F58A, 1),
    (0x06F594, 2),
    (0x06F598, 1),
    (0x06F59D, 2),
    (0x06F5A1, 1),
    (0x06F5A8, 2),
    (0x06F5AC, 1),
    (0x06F5B1, 2),
    (0x06F5B5, 1),
    (0x06F624, 3),
    (0x06F63A, 3),
];

/// What a swap can refuse.
#[derive(Debug)]
pub enum SwapError {
    /// The ROM does not have Lunar Magic's code for the piece.
    NotInstalled(&'static str),
    Asar(AsarError),
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwapError::NotInstalled(why) => write!(f, "{why}"),
            SwapError::Asar(e) => write!(f, "{e}"),
        }
    }
}

impl From<AsarError> for SwapError {
    fn from(e: AsarError) -> Self {
        SwapError::Asar(e)
    }
}

fn copy(to: &mut Rom, from: &Rom, at: u32, len: usize) {
    let bytes = from.read(SnesAddr::new(at), len).unwrap().to_vec();
    to.write(SnesAddr::new(at), &bytes).unwrap();
}

fn revert(rom: &mut Rom, base: &Rom, sites: &[(u32, u32)]) {
    for &(a, b) in sites {
        copy(rom, base, a, (b - a + 1) as usize);
    }
}

fn clear(rom: &mut Rom, at: u32, len: usize) {
    rom.write(SnesAddr::new(at), &vec![0xFF; len]).unwrap();
}

fn apply(asar: &Asar, rom: &Rom, file: &str) -> Result<Rom, AsarError> {
    let piece = [
        install::LUNAR_MAGIC,
        &[install::GRAPHICS, install::EXANIMATION, install::LAYER3],
    ]
    .concat()
    .into_iter()
    .find(|(name, _)| *name == file)
    .unwrap_or_else(|| panic!("no patch {file}"));
    Ok(asar.patch(rom, &install::patch(piece))?.rom)
}

/// `lm`, a ROM Lunar Magic saved, with Kobo's code for `piece` in place of
/// Lunar Magic's. `base` holds the game's bytes: the clean ROM, or for an
/// SA-1 ROM the clean ROM with SA-1 Pack. The checksum is not fixed.
pub fn swap(asar: &Asar, piece: Piece, lm: &Rom, base: &Rom) -> Result<Rom, SwapError> {
    let mut rom = Rom::from_headerless(lm.data().to_vec()).unwrap();
    let byte = |rom: &Rom, at: u32| rom.read_u8(SnesAddr::new(at)).unwrap();
    match piece {
        Piece::Bank06 => {
            for at in [0x06F540, 0x06F624, 0x06F660] {
                clear(&mut rom, at, 4);
            }
            for file in [
                "map16.asm",
                "actslike.asm",
                "objects.asm",
                "level.asm",
                "background.asm",
                "palette.asm",
                "exits.asm",
            ] {
                rom = apply(asar, &rom, file)?;
            }
            for (at, len) in BANK06_TABLES {
                copy(&mut rom, lm, at, len);
            }
        }
        Piece::Vram => {
            revert(&mut rom, base, &VRAM);
            clear(&mut rom, 0x0580BF, 4);
            rom = apply(asar, &rom, "vram.asm")?;
        }
        Piece::Graphics => {
            let src = Rom::from_headerless(rom.data().to_vec()).unwrap();
            revert(&mut rom, base, &GRAPHICS);
            clear(&mut rom, 0x0583B8, 4);
            rom = apply(asar, &rom, "graphics.asm")?;
            for (a, b) in GRAPHICS_TABLES {
                copy(&mut rom, &src, a, (b - a) as usize);
            }
        }
        Piece::Layer3 => {
            revert(&mut rom, base, &LAYER3);
            rom = apply(asar, &rom, "layer3.asm")?;
        }
        Piece::Sprites => {
            revert(&mut rom, base, &SPRITE_LOADER);
            rom = apply(asar, &rom, "sprites.asm")?;
            // A ROM with PIXI keeps PIXI's goal tape init, which PIXI
            // writes over Kobo's in a build, and the jump PIXI's 255-sprite
            // option puts at $02ABF2 over the operand Kobo's patch sets.
            if byte(lm, 0x0EF30F) == 0x42 {
                copy(&mut rom, lm, 0x01C089, 11);
            }
            if byte(lm, 0x02ABF2) == 0x5C {
                copy(&mut rom, lm, 0x02ABF2, 4);
            }
        }
        Piece::ExAnimation => {
            if byte(lm, 0x0583AD) != 0x22 {
                return Err(SwapError::NotInstalled(
                    "no ExAnimation installed (no JSL at $0583AD)",
                ));
            }
            // Lunar Magic's change to the palette copy at $00A5E1, which
            // Kobo's code does not make.
            copy(&mut rom, base, 0x00A5E1, 6);
            clear(&mut rom, 0x0583AE, 3);
            rom = apply(asar, &rom, "exanimation.asm")?;
            // The pointers the patch initialises: the level table's and the
            // global list's, at offsets of the hook's target; the
            // alternative files; the levels' settings.
            let target = |rom: &Rom| rom.read_u24(SnesAddr::new(0x0583AE)).unwrap();
            let (from, to) = (target(lm), target(&rom));
            for (offset, len) in [(0xEA, 3), (0x5B, 2), (0x65, 2)] {
                let bytes = lm.read(SnesAddr::new(from + offset), len).unwrap().to_vec();
                rom.write(SnesAddr::new(to + offset), &bytes).unwrap();
            }
            copy(&mut rom, lm, 0x03BCC0, 16);
            copy(&mut rom, lm, 0x03FE00, 512);
        }
        Piece::ExLevel => {
            if byte(lm, 0x05DA8A) != 0x22 {
                return Err(SwapError::NotInstalled(
                    "no taller levels in this ROM (no JSL at $05DA8A)",
                ));
            }
            // The size table, $240 bytes before the code $05DA8A calls.
            let table =
                |rom: &Rom| SnesAddr::new(rom.read_u24(SnesAddr::new(0x05DA8B)).unwrap() - 0x240);
            let sizes = lm.read(table(lm), 0x200).unwrap().to_vec();
            revert(&mut rom, base, &EXLEVEL);
            rom = apply(asar, &rom, "exlevel.asm")?;
            let at = table(&rom);
            rom.write(at, &sizes).unwrap();
        }
        Piece::Entrances => {
            for (at, len) in [(0x05D7CE, 4), (0x05DA17, 4), (0x05DC86, 3), (0x05DC8B, 3)] {
                clear(&mut rom, at, len);
            }
            rom = apply(asar, &rom, "exits.asm")?;
            rom = apply(asar, &rom, "entrance.asm")?;
        }
    }
    Ok(rom)
}
