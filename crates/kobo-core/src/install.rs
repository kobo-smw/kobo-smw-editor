//! Kobo's ROM-side code: the Asar patches that give a build the code Lunar
//! Magic's layout needs, written clean-room (docs/clean-room.md).
//! The patches are sources in `asm/`, assembled into the ROM through
//! [`crate::asar`] at build time.
//!
//! Lunar Magic decides piece by piece whether its install is in a ROM, and
//! a save installs a missing piece over whatever is at its sites and
//! resets its tables (docs/lunar-magic-install.md). A build that writes a
//! piece's tables installs Kobo's code for that piece, with Lunar Magic's
//! check for it met. So far that is the Map16 routine and the acts-like
//! chain in bank `$06`, whose check is `$06F600`, for Map16 pages past 1;
//! the placed objects (`22`, `23`, `27`, `29`), whose check is on Lunar
//! Magic's own code, so its first save puts its own in their place; the
//! level number (`$0EF550` occupied); and backgrounds and BG Map16 (a `JML`
//! at `$0EF519`, a `JSL` at `$058DA4`); custom palettes; screen exits in
//! Lunar Magic's format; the entrance settings, whose check is the `JSL`
//! at `$05DA17`; sprite lists in any bank (the `JSL` at `$05D8F5`); and
//! the VRAM patch (64x32 tilemaps at `$3000` and `$3800`), whose check, a
//! `JML` at `$00A5A2`, Kobo leaves unmet, so that Lunar Magic's first save
//! puts its own in Kobo's place; and taller levels, whose check is the
//! `JSL` at `$05DA8A`, with the level size table `$240` bytes before the
//! code it calls. A build that uses Lunar Magic's graphics formats also
//! gets [`GRAPHICS`], whose checks (`$00AAD8`, `$00AA47`, the `JSL` at
//! `$0583B8`, `"LM"` at `$0FF15C`) it meets, and a build whose lists have
//! layer 3 settings [`LAYER3`] (a `JSL` at `$00A01F`), and a build with
//! secondary entrances past `1FF` [`ENTRANCES`], which moves their tables
//! behind the pointers Lunar Magic's code keeps, sized to the last one in
//! use. Every patch takes
//! the ROM's mapping and the game's variables from [`MEMORY`], so the same
//! sources go on a LoROM image and on SA-1 Pack's.

use crate::asar::{Asar, AsarError, Patch, Patched};
use crate::rom::Rom;

/// What every patch includes first: the ROM's mapping and where the
/// game's variables are, LoROM's or SA-1 Pack's.
pub const MEMORY: (&str, &str) = ("memory.asm", include_str!("../asm/lunar-magic/memory.asm"));

/// The patches, by file name under `asm/lunar-magic/`, in the order they
/// apply.
pub const LUNAR_MAGIC: &[(&str, &str)] = &[
    ("map16.asm", include_str!("../asm/lunar-magic/map16.asm")),
    (
        "actslike.asm",
        include_str!("../asm/lunar-magic/actslike.asm"),
    ),
    (
        "objects.asm",
        include_str!("../asm/lunar-magic/objects.asm"),
    ),
    ("level.asm", include_str!("../asm/lunar-magic/level.asm")),
    (
        "sprite-banks.asm",
        include_str!("../asm/lunar-magic/sprite-banks.asm"),
    ),
    (
        "background.asm",
        include_str!("../asm/lunar-magic/background.asm"),
    ),
    (
        "palette.asm",
        include_str!("../asm/lunar-magic/palette.asm"),
    ),
    ("exits.asm", include_str!("../asm/lunar-magic/exits.asm")),
    (
        "entrance.asm",
        include_str!("../asm/lunar-magic/entrance.asm"),
    ),
    (
        "sprites.asm",
        include_str!("../asm/lunar-magic/sprites.asm"),
    ),
    ("vram.asm", include_str!("../asm/lunar-magic/vram.asm")),
    (
        "exlevel.asm",
        include_str!("../asm/lunar-magic/exlevel.asm"),
    ),
];

/// Lunar Magic's graphics formats (4bpp GFX files, ExGFX, per-level and
/// older graphics lists), applied after [`LUNAR_MAGIC`] by a build that
/// uses them, since it changes how every GFX file is stored.
pub const GRAPHICS: (&str, &str) = (
    "graphics.asm",
    include_str!("../asm/lunar-magic/graphics.asm"),
);

/// Lunar Magic's ExAnimation (level and global animations of tiles and
/// colours), applied after [`LUNAR_MAGIC`] by a build that has any. Its
/// `JSL` at `$00A390` is what Lunar Magic checks before it treats its own
/// as installed (docs/lunar-magic-install.md, "ExAnimation").
pub const EXANIMATION: (&str, &str) = (
    "exanimation.asm",
    include_str!("../asm/lunar-magic/exanimation.asm"),
);
/// Lunar Magic's layer 3 settings in the graphics lists, applied after
/// [`GRAPHICS`] by a build whose lists have any. Its check, a `JSL` at
/// `$00A01F`, is met.
pub const LAYER3: (&str, &str) = ("layer3.asm", include_str!("../asm/lunar-magic/layer3.asm"));

/// The banks of Choc Island 2's rooms, applied by a build that writes one
/// of levels `0CD`-`0CF`, whose banks those rooms would otherwise take
/// (docs/lunar-magic-install.md, "Choc Island 2's rooms").
pub const CHOC_ISLAND: (&str, &str) = (
    "choc-island.asm",
    include_str!("../asm/lunar-magic/choc-island.asm"),
);

/// Where [`CHOC_ISLAND`] hooks the rooms' pointer load: a `JSL` there.
pub const CHOC_ISLAND_HOOK: crate::addr::SnesAddr = crate::addr::SnesAddr::new(0x05DB4B);

/// The secondary entrance tables moved to hold as many entrances as the
/// define `!entrance_count` says, for the entrances past `1FF` Lunar
/// Magic's long screen exits name, applied after [`LUNAR_MAGIC`] by a
/// build that has any ([`apply_entrances`]). Kobo's code reads every
/// entrance through the pointers it changes.
pub const ENTRANCES: (&str, &str) = (
    "entrances.asm",
    include_str!("../asm/lunar-magic/entrances.asm"),
);

/// One of the patches, with [`MEMORY`] beside it for its `incsrc`.
pub fn patch((name, source): (&str, &str)) -> Patch {
    let (memory, text) = MEMORY;
    Patch::source(name, source).file(memory, text.as_bytes())
}

/// Applies the Lunar Magic layout patches to a ROM.
pub fn apply_lunar_magic(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    let mut rom = Rom::from_bytes(rom.data().to_vec())?;
    for &piece in LUNAR_MAGIC {
        let Patched { rom: patched, .. } = asar.patch(&rom, &patch(piece))?;
        rom = patched;
    }
    Ok(rom)
}

/// Applies [`ENTRANCES`] with tables of `count` entrances, `$201` to
/// `$2000` (the last in use plus one, as Lunar Magic sizes them), with the
/// entrances the tables held before copied into the moved ones, Lunar
/// Magic's further tables' too.
pub fn apply_entrances(asar: &Asar, rom: &Rom, count: u16) -> Result<Rom, InstallError> {
    let entrances = crate::level::read_entrances(rom)?;
    let layout = crate::entrance::Layout::of(rom);
    let extra_count = crate::entrance::extra_count(rom, &layout);
    let extras = match layout.extra {
        Some(tables) => tables
            .iter()
            .map(|&t| Ok(rom.read(t, extra_count)?.to_vec()))
            .collect::<Result<Vec<_>, crate::rom::RomError>>()?,
        None => Vec::new(),
    };
    let sized = patch(ENTRANCES).define("entrance_count", format!("${count:04X}"));
    let mut rom = asar.patch(rom, &sized)?.rom;
    crate::level::write_entrances(&mut rom, &entrances)?;
    if let Some(tables) = crate::entrance::Layout::of(&rom).extra {
        for (table, bytes) in tables.into_iter().zip(&extras) {
            rom.write(table, bytes)?;
        }
    }
    Ok(rom)
}

/// What applying a patch that carries data across can fail with.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Asar(#[from] AsarError),
    #[error(transparent)]
    Level(#[from] crate::level::LevelError),
    #[error(transparent)]
    Rom(#[from] crate::rom::RomError),
}

/// Applies [`EXANIMATION`], whose tables the build then writes.
pub fn apply_exanimation(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    Ok(asar.patch(rom, &patch(EXANIMATION))?.rom)
}

/// Applies [`GRAPHICS`], whose checks tell Lunar Magic the ROM's GFX files
/// are 4bpp and it has ExGFX and lists, which the build must then write.
pub fn apply_graphics(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    Ok(asar.patch(rom, &patch(GRAPHICS))?.rom)
}

/// Applies [`LAYER3`], which reads the lists [`GRAPHICS`] finds.
pub fn apply_layer3(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    Ok(asar.patch(rom, &patch(LAYER3))?.rom)
}

/// Applies [`CHOC_ISLAND`].
pub fn apply_choc_island(asar: &Asar, rom: &Rom) -> Result<Rom, AsarError> {
    Ok(asar.patch(rom, &patch(CHOC_ISLAND))?.rom)
}
