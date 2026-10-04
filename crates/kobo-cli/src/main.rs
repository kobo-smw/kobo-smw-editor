//! `kobo`: command-line shell over the Kobo core library.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use kobo_core::asar::{Asar, AsarError, Patch};
use kobo_core::bps;
use kobo_core::clean_room;
use kobo_core::expand;
use kobo_core::gfx::{self, Bpp, GFX_FILE_COUNT};
use kobo_core::image::{grayscale, tile_sheet};
use kobo_core::level::{self, Layer2Data};
use kobo_core::map16::{self, Map16Tile};
use kobo_core::mwl::{self, MwlFile, Section};
use kobo_core::palette::{self, LevelPaletteSelect};
use kobo_core::ram::RamAddr;
use kobo_core::rats::{self, FreeSpace};
use kobo_core::render::{self, LayerTiles, RenderOptions, Sprites};
use kobo_core::sprites;
use kobo_core::tools::{self, Tool, ToolError};
use kobo_core::{Mapping, PcAddr, Rom, SnesAddr, config};

#[derive(Parser)]
#[command(
    name = "kobo",
    version,
    about = "Super Mario World ROM editor and build system"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect and expand ROM images.
    Rom {
        #[command(subcommand)]
        command: RomCommand,
    },
    /// Work with GFX files (8x8 tile graphics).
    Gfx {
        #[command(subcommand)]
        command: GfxCommand,
    },
    /// Inspect levels.
    Level {
        #[command(subcommand)]
        command: LevelCommand,
    },
    /// Render palettes.
    Palette {
        #[command(subcommand)]
        command: PaletteCommand,
    },
    /// Render Map16 tiles.
    Map16 {
        #[command(subcommand)]
        command: Map16Command,
    },
    /// Make a new project from a template: a widely used baserom, fetched
    /// from its own release and checked by hash.
    New {
        /// The project directory, which must not have a project yet.
        dir: Option<PathBuf>,
        /// The template (`--list` names them).
        #[arg(long)]
        template: Option<String>,
        /// List the templates.
        #[arg(long)]
        list: bool,
        /// The clean ROM. Defaults to the configured vanilla ROM.
        #[command(flatten)]
        rom: RomArg,
    },
    /// Import a ROM's levels into a new project, a Lunar Magic MWL file
    /// into a project, or a Callisto project's folder into a new project.
    Import {
        /// The ROM, `.mwl` file, or Callisto project folder to import from.
        from: PathBuf,
        /// The project directory: a new one for a ROM; for an MWL file, an
        /// existing one or a new one.
        dir: PathBuf,
        /// For a ROM, import every level, not only those that differ from
        /// the clean ROM.
        #[arg(long)]
        all: bool,
        /// For an MWL file, the level to import it as (hex) instead of the
        /// one it was saved from.
        #[arg(long)]
        level: Option<String>,
        /// For an MWL file, the ROM it came from, whose PIXI sprite size
        /// table says how many extension bytes each sprite has.
        #[arg(long)]
        sizes_from: Option<PathBuf>,
        /// For a ROM made with PIXI, the PIXI folder its sprites were
        /// inserted from: the project takes its list and sprites as source.
        /// Without it, the sprites are carried as the compiled code the ROM
        /// holds.
        #[arg(long)]
        pixi: Option<PathBuf>,
        /// The clean ROM. Defaults to the configured vanilla ROM.
        #[command(flatten)]
        rom: RomArg,
    },
    /// Build a project into a ROM.
    Build {
        /// The project directory.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Output ROM path.
        #[arg(long, short = 'o', default_value = "build.sfc")]
        out: PathBuf,
        /// Also write the build as a BPS patch against the clean ROM
        /// (headerless), for distribution.
        #[arg(long)]
        bps: Option<PathBuf>,
        /// Run every stage, without reading or keeping snapshots.
        #[arg(long)]
        no_cache: bool,
        /// The clean ROM. Defaults to the configured vanilla ROM.
        #[command(flatten)]
        rom: RomArg,
    },
    /// Compare the levels of two ROMs as Kobo reads them, whatever their
    /// layouts; exits nonzero if any differ.
    Diff {
        a: PathBuf,
        b: PathBuf,
        /// Compare only the levels this project defines.
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Rewrite a project's level files in Kobo's format, keeping comments
    /// on lines of their own.
    Fmt {
        /// The project directory.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Only report files that would change, and fail if any would.
        #[arg(long)]
        check: bool,
    },
    /// Apply an Asar patch to a copy of a ROM, with the checksum fixed.
    Asm {
        #[command(flatten)]
        rom: RomArg,
        /// The patch.
        patch: PathBuf,
        /// Output path. The image is written without a copier header.
        out: PathBuf,
        /// Another directory to look for included files in.
        #[arg(long = "include", short = 'I')]
        include: Vec<PathBuf>,
        /// Define `!name`, as `name=value` or `name` for an empty value.
        #[arg(long = "define", short = 'D')]
        define: Vec<String>,
        /// Asar's shared library. Defaults to `KOBO_ASAR_LIB`, then
        /// `tools.asar` in the user config, then the build Kobo pins.
        #[arg(long)]
        asar: Option<PathBuf>,
    },
    /// Inspect Lunar Magic's MWL level files.
    Mwl {
        #[command(subcommand)]
        command: MwlCommand,
    },
    /// Apply and create BPS patches.
    Bps {
        #[command(subcommand)]
        command: BpsCommand,
    },
    /// The toolchain's programs: where each comes from, and fetching the
    /// builds Kobo pins.
    Tools {
        #[command(subcommand)]
        command: Option<ToolsCommand>,
    },
    /// Convert between SNES addresses and ROM file offsets.
    Addr {
        /// Address to convert. `$05E000` or `05E000` is a SNES address,
        /// `0x2E000` is a file offset.
        addr: String,
        /// Use SA-1 mapping instead of LoROM.
        #[arg(long)]
        sa1: bool,
    },
}

#[derive(Subcommand)]
enum ToolsCommand {
    /// List each tool and where a build would take it from (the default).
    List,
    /// Download the pinned builds for this platform, and SA-1 Pack's
    /// release, into the cache, so builds need no network later.
    Fetch {
        /// The tools to fetch (`asar`, `pixi`, `uberasm`, `sa1pack`); all by
        /// default.
        tools: Vec<String>,
    },
    /// Print the path a build would use for a tool, fetching its pinned
    /// build first if needed.
    Path {
        /// The tool: `asar` (its library), `pixi`, `uberasm`, `gps`,
        /// `addmusick`, or `sa1pack`.
        tool: String,
    },
}

#[derive(Subcommand)]
enum RomCommand {
    /// Print header and identification details.
    Info {
        #[command(flatten)]
        rom: RomArg,
    },
    /// Write a copy expanded to a larger size, with the new space free and
    /// the checksum fixed.
    Expand {
        #[command(flatten)]
        rom: RomArg,
        /// New size: `1M`, `3M`, `1536K`, or a byte count.
        size: String,
        /// Output path. The image is written without a copier header.
        out: PathBuf,
    },
    /// List the RATS-tagged blocks from `$108000` on, and the free space.
    Rats {
        #[command(flatten)]
        rom: RomArg,
    },
}

#[derive(Subcommand)]
enum GfxCommand {
    /// List the GFX files in a ROM.
    List {
        #[command(flatten)]
        rom: RomArg,
    },
    /// Write GFX00 to GFX33 as .bin files in Lunar Magic's export layout.
    Export {
        #[command(flatten)]
        rom: RomArg,
        /// Output directory. Created if missing.
        dir: PathBuf,
    },
    /// Render a GFX file as a grayscale tile sheet PNG.
    Png {
        #[command(flatten)]
        rom: RomArg,
        /// GFX file index in hex, for example `00` or `1A`.
        index: String,
        /// Output PNG path.
        out: PathBuf,
        /// Tiles per row.
        #[arg(long, default_value_t = 16)]
        columns: u32,
        /// Reinterpret the stored bytes at this bit depth (2, 3, or 4)
        /// instead of the inferred one. Useful for checking unknown files.
        #[arg(long)]
        bpp: Option<u8>,
    },
}

#[derive(Subcommand)]
enum LevelCommand {
    /// Print a level's header and data pointers.
    Info {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
    /// Render a level to PNG by running the ROM's own loader.
    Png {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Output PNG path.
        out: PathBuf,
        /// Leave out sprites entirely.
        #[arg(long)]
        no_sprites: bool,
        /// Leave out the player at the level's entrance.
        #[arg(long)]
        no_player: bool,
        /// Maximum total CPU instructions across loading and all sprite passes.
        #[arg(long)]
        max_instructions: Option<u64>,
        /// Draw every sprite as an ID marker instead of running the game's
        /// sprite engine for its graphics.
        #[arg(long)]
        markers: bool,
    },
    /// List a level's sprites.
    Sprites {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
    /// Write a level's expanded tile grid planes as `level_XXX.l1lo.bin`
    /// and `.l1hi.bin` in the oracle dump layout.
    Dump {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Output directory.
        dir: PathBuf,
    },
    /// Summarise which ROM pages the loader reads, for finding tables. On a
    /// ROM Lunar Magic saved, the pages alone, without the instructions
    /// that read them (clean room).
    Reads {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Only report addresses at or above this SNES address (hex).
        #[arg(long, default_value = "0F8000")]
        from: String,
        /// Only report instructions that also read inside this 64 KiB bank
        /// (hex bank number), plus instructions within 256 bytes of them.
        #[arg(long)]
        near_bank: Option<String>,
    },
    /// Print the foreground Map16 definitions the loaded level resolved,
    /// as `NNNN: 8 hex bytes` lines.
    Map16 {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
    /// Hex-dump a work RAM range after the level loader has run.
    Wram {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
        /// Start address, `$7E0FBE` style.
        addr: String,
        /// Byte count (decimal).
        #[arg(default_value_t = 64)]
        len: usize,
        /// Read bus addresses as the S-CPU sees them (an SA-1's I-RAM at
        /// `$003000`, its BW-RAM at `$400000`) instead of resolving the
        /// vanilla layout's addresses through the ROM's RAM map.
        #[arg(long)]
        bus: bool,
    },
    /// Print a level's expanded tile grid as hex, one screen row per line.
    Tiles {
        #[command(flatten)]
        rom: RomArg,
        /// Level number in hex, for example `105`.
        level: String,
    },
}

#[derive(Subcommand)]
enum BpsCommand {
    /// Apply a patch to the clean ROM, made against it with or without a
    /// copier header, and write the result without one.
    Apply {
        /// The BPS patch.
        patch: PathBuf,
        /// Output ROM path.
        out: PathBuf,
        /// The clean ROM. Defaults to the configured vanilla ROM.
        #[command(flatten)]
        rom: RomArg,
    },
    /// Write a patch from the clean ROM to a modified one, both headerless.
    Create {
        /// The modified ROM.
        modified: PathBuf,
        /// Output patch path.
        out: PathBuf,
        #[command(flatten)]
        rom: RomArg,
    },
}

#[derive(Subcommand)]
enum MwlCommand {
    /// Print an MWL file's header and the sections found in it.
    Info {
        /// The MWL file.
        file: PathBuf,
        /// The ROM the file came from, whose PIXI size table says how
        /// many extension bytes its sprites have; the file does not.
        #[arg(long, short = 'r')]
        rom: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PaletteCommand {
    /// Render a level palette as a 16x16 swatch grid. With `--level`, the
    /// palette the ROM's loader uploaded for that level, custom palettes
    /// and patches included; otherwise the vanilla assembly of the rows
    /// given.
    Png {
        #[command(flatten)]
        rom: RomArg,
        #[command(flatten)]
        sel: PaletteArgs,
        /// Output PNG path.
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum Map16Command {
    /// Render a layer's Map16 tiles as a 16-column sheet in tile number
    /// order. With `--level`, the definitions, graphics, and palette the
    /// ROM's loader resolved for that level, Lunar Magic pages included;
    /// otherwise the vanilla tables of a tileset.
    Png {
        #[command(flatten)]
        rom: RomArg,
        #[command(flatten)]
        sel: PaletteArgs,
        /// Object tileset (0 to 14) for the vanilla tables. Not used with
        /// `--level`, whose graphics are the ones the level loaded.
        #[arg(long)]
        tileset: Option<u8>,
        /// Layer 1 (foreground) or 2 (background, tiles from 200).
        #[arg(long, default_value_t = 1)]
        layer: u8,
        /// Output PNG path.
        out: PathBuf,
    },
}

/// Palette selection, either from a level header or explicit fields.
#[derive(Args)]
struct PaletteArgs {
    /// Take palette and tileset settings from this level's header (hex).
    #[arg(long)]
    level: Option<String>,
    #[arg(long, default_value_t = 0)]
    fg: u8,
    #[arg(long, default_value_t = 0)]
    bg: u8,
    #[arg(long, default_value_t = 0)]
    sprite: u8,
    #[arg(long, default_value_t = 0)]
    back_area: u8,
}

impl PaletteArgs {
    /// Resolves to (palette selection, object tileset if a level was given).
    fn resolve(&self, rom: &Rom) -> Result<(LevelPaletteSelect, Option<u8>)> {
        if let Some(level) = &self.level {
            let level = parse_level(level)?;
            let h = level::read_primary_header(rom, level)?;
            Ok((h.palette_select(), Some(h.object_tileset)))
        } else {
            Ok((
                LevelPaletteSelect {
                    fg: self.fg,
                    bg: self.bg,
                    sprite: self.sprite,
                    back_area: self.back_area,
                },
                None,
            ))
        }
    }
}

fn parse_level(text: &str) -> Result<u16> {
    u16::from_str_radix(text, 16).context("level must be hex, e.g. 105")
}

#[derive(Args)]
struct RomArg {
    /// ROM path. Defaults to the configured vanilla ROM.
    #[arg(long, short = 'r')]
    rom: Option<PathBuf>,
}

impl RomArg {
    fn load(&self) -> Result<Rom> {
        let path = match &self.rom {
            Some(p) => p.clone(),
            None => config::vanilla_rom_path()?,
        };
        Rom::load(&path).with_context(|| format!("loading {}", path.display()))
    }
}

fn main() -> Result<()> {
    // Let a closed pipe (e.g. `| head`) end the process quietly.
    #[cfg(unix)]
    // SAFETY: resetting SIGPIPE to its default disposition has no
    // preconditions and happens before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match cli.command {
        Command::Rom { command } => match command {
            RomCommand::Info { rom } => rom_info(&rom.load()?),
            RomCommand::Expand { rom, size, out } => rom_expand(rom.load()?, &size, &out),
            RomCommand::Rats { rom } => rom_rats(&rom.load()?),
        },
        Command::Gfx { command } => match command {
            GfxCommand::List { rom } => gfx_list(&rom.load()?),
            GfxCommand::Export { rom, dir } => gfx_export(&rom.load()?, &dir),
            GfxCommand::Png {
                rom,
                index,
                out,
                columns,
                bpp,
            } => gfx_png(&rom.load()?, &index, &out, columns, bpp),
        },
        Command::Level { command } => match command {
            LevelCommand::Info { rom, level } => level_info(&rom.load()?, &level),
            LevelCommand::Png {
                rom,
                level,
                out,
                no_sprites,
                no_player,
                max_instructions,
                markers,
            } => level_png(
                &rom.load()?,
                &level,
                &out,
                !no_sprites,
                !no_player,
                markers,
                max_instructions,
            ),
            LevelCommand::Sprites { rom, level } => level_sprites(&rom.load()?, &level),
            LevelCommand::Tiles { rom, level } => level_tiles(&rom.load()?, &level),
            LevelCommand::Dump { rom, level, dir } => level_dump(&rom.load()?, &level, &dir),
            LevelCommand::Reads {
                rom,
                level,
                from,
                near_bank,
            } => level_reads(&rom.load()?, &level, &from, near_bank.as_deref()),
            LevelCommand::Map16 { rom, level } => level_map16(&rom.load()?, &level),
            LevelCommand::Wram {
                rom,
                level,
                addr,
                len,
                bus,
            } => level_wram(&rom.load()?, &level, &addr, len, bus),
        },
        Command::Palette {
            command: PaletteCommand::Png { rom, sel, out },
        } => palette_png(&rom.load()?, &sel, &out),
        Command::Map16 {
            command:
                Map16Command::Png {
                    rom,
                    sel,
                    tileset,
                    layer,
                    out,
                },
        } => map16_png(&rom.load()?, &sel, tileset, layer, &out),
        Command::Asm {
            rom,
            patch,
            out,
            include,
            define,
            asar,
        } => asm(
            &rom.load()?,
            &patch,
            &out,
            &include,
            &define,
            asar.as_deref(),
        ),
        Command::Mwl {
            command: MwlCommand::Info { file, rom },
        } => mwl_info(&file, rom.as_deref()),
        Command::Bps { command } => match command {
            BpsCommand::Apply { patch, out, rom } => bps_apply(&rom.load()?, &patch, &out),
            BpsCommand::Create { modified, out, rom } => bps_create(&rom.load()?, &modified, &out),
        },
        Command::Addr { addr, sa1 } => convert_addr(&addr, sa1),
        Command::Tools { command } => match command.unwrap_or(ToolsCommand::List) {
            ToolsCommand::List => tools_list(),
            ToolsCommand::Fetch { tools } => tools_fetch(&tools),
            ToolsCommand::Path { tool } => tools_path(&tool),
        },
        Command::New {
            dir,
            template,
            list,
            rom,
        } => new_project(dir.as_deref(), template.as_deref(), list, &rom),
        Command::Import {
            from,
            dir,
            all,
            level,
            sizes_from,
            pixi,
            rom,
        } => import(
            &from,
            &dir,
            all,
            level.as_deref(),
            sizes_from.as_deref(),
            pixi.as_deref(),
            &rom.load()?,
        ),
        Command::Build {
            dir,
            out,
            bps,
            no_cache,
            rom,
        } => build(&dir, &out, bps.as_deref(), no_cache, &rom.load()?),
        Command::Fmt { dir, check } => fmt(&dir, check),
        Command::Diff { a, b, project } => diff(&a, &b, project.as_deref()),
    }
}

fn asm(
    rom: &Rom,
    patch: &Path,
    out: &Path,
    include: &[PathBuf],
    define: &[String],
    asar: Option<&Path>,
) -> Result<()> {
    let asar = match asar {
        Some(path) => Asar::load(path)?,
        None => Asar::configured()?,
    };
    let mut spec = Patch::new(patch);
    for dir in include {
        spec = spec.include_path(dir);
    }
    for d in define {
        let (name, value) = d.split_once('=').unwrap_or((d, ""));
        spec = spec.define(name, value);
    }
    let patched = match asar.patch(rom, &spec) {
        Ok(patched) => patched,
        Err(AsarError::Failed {
            errors,
            warnings,
            prints,
        }) => {
            show_prints(rom, &prints);
            warnings
                .iter()
                .for_each(|w| eprintln!("{}", shown_message(rom, w)));
            errors
                .iter()
                .for_each(|e| eprintln!("{}", shown_message(rom, e)));
            bail!("Asar failed on {}", patch.display());
        }
        Err(e) => return Err(e.into()),
    };
    show_prints(rom, &patched.output.prints);
    patched
        .output
        .warnings
        .iter()
        .for_each(|w| eprintln!("{}", shown_message(rom, w)));
    let mut rom = patched.rom;
    rom.fix_checksum()?;
    rom.save(out)?;
    println!(
        "{}: {} KiB, checksum ${:04X} (Asar {})",
        out.display(),
        rom.len() / 1024,
        rom.internal_header().checksum,
        asar.version()
    );
    Ok(())
}

/// What a patch printed, unless the ROM it was applied to is one Lunar
/// Magic saved: a patch can print the bytes of its code (clean room).
fn show_prints(rom: &Rom, prints: &[String]) {
    if prints.is_empty() {
        return;
    }
    if clean_room::saved_by_lunar_magic(rom) {
        eprintln!(
            "{} lines the patch printed withheld: a ROM Lunar Magic saved (clean room)",
            prints.len()
        );
    } else {
        prints.iter().for_each(|p| println!("{p}"));
    }
}

/// An Asar message, or on a ROM Lunar Magic saved only where it is from:
/// any message can carry a value the patch read from the ROM's code, a
/// patch's own `error`, `warn`, or `assert` as much as an address out of
/// bounds (clean room).
fn shown_message(rom: &Rom, message: &kobo_core::asar::Message) -> String {
    if !clean_room::saved_by_lunar_magic(rom) {
        return message.to_string();
    }
    let at = match (&message.file, message.line) {
        (Some(file), Some(line)) => format!("{file}:{line}: "),
        _ => String::new(),
    };
    format!("{at}(text withheld: a ROM Lunar Magic saved, clean room)")
}

fn mwl_info(path: &std::path::Path, rom: Option<&std::path::Path>) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let file = MwlFile::parse(&bytes)?;
    let comment: String = file.comment.iter().map(|&b| b as char).collect();
    println!(
        "version:          {}.{:02X}",
        file.version >> 8,
        file.version & 0xFF
    );
    println!(
        "flags:            {:02X?}{}",
        file.flags,
        if file.is_sma2() { " (SMA2)" } else { "" }
    );
    println!("comment:          {:?}", comment.trim_end());
    println!("sections:");
    for (index, data) in file.sections.iter().enumerate() {
        let name = Section::ALL.get(index).map_or("unknown", |s| s.name());
        println!("  {index}  {name:20} {:5} bytes", data.len());
    }
    let rom = rom.map(Rom::load).transpose()?;
    let sizes = rom
        .as_ref()
        .map(sprites::pixi_size_table)
        .transpose()?
        .flatten();
    let level = file.decode(sizes)?;
    let source = |h: mwl::SectionHeader| h.source().map_or("-".into(), |a| a.to_string());
    let header = level.layer1.primary_header();
    let info = &level.info;
    println!("level:            {:03X}", info.level);
    println!(
        "secondary header: {:02X?}, Lunar Magic {:02X?} {:02X?}, midway {:02X?}",
        info.secondary.to_bytes(),
        info.secondary_lm,
        info.lm3,
        info.midway
    );
    println!(
        "layer 1:          {} objects from {}, {} screens, mode {}, tileset {}",
        level.layer1.data.objects.len(),
        source(level.layer1.header),
        header.screens,
        header.level_mode,
        header.object_tileset
    );
    let layer2 = match &level.layer2.data {
        mwl::Layer2Data::Objects(data) => format!("{} objects", data.objects.len()),
        mwl::Layer2Data::Background(tiles) => format!("background of {} tiles", tiles.len()),
        mwl::Layer2Data::Empty => "empty".into(),
    };
    println!(
        "layer 2:          {layer2} from {}, flags ${:02X}",
        source(level.layer2.header),
        level.layer2.flags()
    );
    let list = &level.sprites.list;
    println!(
        "sprites:          {} from {}, header ${:02X}",
        list.sprites.len(),
        source(level.sprites.header),
        list.header.to_byte()
    );
    println!(
        "palette:          {}, back area ${:04X}",
        if level.layer1.custom_palette() {
            "custom"
        } else {
            "the header's"
        },
        level.palette.back_area.0
    );
    let ids: Vec<String> = level
        .entrances
        .entries
        .iter()
        .map(|e| format!("{:03X}", e.id))
        .collect();
    println!("entrances:        {}", ids.join(" "));
    println!(
        "ExAnimation:      {} bytes, settings ${:02X}",
        level.animation.data.len(),
        level.animation.settings()
    );
    let exgfx: Vec<String> = mwl::ExGfx::SLOTS
        .iter()
        .zip(level.exgfx.0)
        .map(|(slot, file)| format!("{slot} {file:03X}"))
        .collect();
    println!("ExGFX:            {}", exgfx.join(", "));
    Ok(())
}

fn level_info(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let h = level::read_primary_header(rom, level)?;
    println!("level:            {level:03X}");
    println!("layer 1 data:     {}", level::layer1_ptr(rom, level)?);
    match level::layer2_ptr(rom, level)? {
        Layer2Data::Objects(a) => println!("layer 2 data:     {a} (objects)"),
        Layer2Data::Tilemap(a) => println!("layer 2 data:     {a} (background tilemap)"),
    }
    println!("sprite data:      {}", level::sprite_ptr(rom, level)?);
    println!("screens:          {}", h.screens);
    println!("level mode:       {}", h.level_mode);
    println!(
        "object tileset:   {} ({:?})",
        h.object_tileset,
        gfx::object_tileset_files(rom, h.object_tileset)
            .map(|f| f.map(|x| format!("{x:02X}")))
            .unwrap_or_default()
    );
    println!(
        "sprite tileset:   {} ({:?})",
        h.sprite_tileset,
        gfx::sprite_tileset_files(rom, h.sprite_tileset)
            .map(|f| f.map(|x| format!("{x:02X}")))
            .unwrap_or_default()
    );
    println!("fg palette:       {}", h.fg_palette);
    println!("bg palette:       {}", h.bg_palette);
    println!("sprite palette:   {}", h.sprite_palette);
    println!("back area colour: {}", h.back_area);
    println!("music:            {}", h.music);
    println!("time:             {}", h.time);
    println!("layer 3 priority: {}", h.layer3_priority);
    println!("item memory:      {}", h.item_memory);
    println!("vertical scroll:  {}", h.vertical_scroll);
    Ok(())
}

fn level_png(
    rom: &Rom,
    level: &str,
    out: &PathBuf,
    with_sprites: bool,
    with_player: bool,
    markers: bool,
    max_instructions: Option<u64>,
) -> Result<()> {
    let level = parse_level(level)?;
    let options = RenderOptions {
        sprites: match (with_sprites, markers) {
            (false, _) => Sprites::Hidden,
            (true, true) => Sprites::Markers,
            (true, false) => Sprites::Drawn,
        },
        player: with_player,
    };
    let rendered = match max_instructions {
        Some(limit) => render::render_level_with_control(
            rom,
            level,
            options,
            &kobo_core::operation::Operation::new(Some(limit)),
        )?,
        None => render::render_level(rom, level, options)?,
    };
    for line in expand::summarize(&rendered.diagnostics) {
        eprintln!("warning: level {level:03X}: {line}");
    }
    rendered.image.write_png(out)?;
    println!(
        "level {level:03X}: {} screens, mode {}, {}x{} -> {}",
        rendered.level.tiles.screens,
        rendered.level.tiles.level_mode,
        rendered.image.width,
        rendered.image.height,
        out.display()
    );
    Ok(())
}

fn level_dump(rom: &Rom, level: &str, dir: &PathBuf) -> Result<()> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join(format!("level_{level:03X}.l1lo.bin")),
        &loaded.tiles.low,
    )?;
    fs::write(
        dir.join(format!("level_{level:03X}.l1hi.bin")),
        &loaded.tiles.high,
    )?;
    fs::write(
        dir.join(format!("level_{level:03X}.vram.bin")),
        &loaded.video.vram,
    )?;
    fs::write(
        dir.join(format!("level_{level:03X}.cgram.bin")),
        &loaded.video.cgram,
    )?;
    println!("level {level:03X}: wrote planes to {}", dir.display());
    Ok(())
}

fn level_reads(rom: &Rom, level: &str, from: &str, near_bank: Option<&str>) -> Result<()> {
    let level = parse_level(level)?;
    let from = u32::from_str_radix(from.trim_start_matches('$'), 16).context("bad address")?;
    let bank = near_bank
        .map(|b| u32::from_str_radix(b.trim_start_matches('$'), 16))
        .transpose()
        .context("bad bank")?;
    let (_, trace) = expand::expand_level_traced(rom, level, true)?;
    let trace = trace.unwrap_or_default();
    if trace.iter().any(|(pc, _)| pc.is_none()) {
        return reads_by_page(&trace, from, bank);
    }
    let trace: Vec<(u32, u32)> = trace.iter().filter_map(|&(pc, a)| Some((pc?, a))).collect();
    // Instructions of interest: those reading in the given bank, and neighbours.
    let mut hot: Vec<u32> = Vec::new();
    if let Some(bank) = bank {
        hot = trace
            .iter()
            .filter(|(_, a)| a >> 16 == bank)
            .map(|(pc, _)| *pc)
            .collect();
        hot.sort_unstable();
        hot.dedup();
    }
    let interesting = |pc: u32| bank.is_none() || hot.iter().any(|h| pc.abs_diff(*h) <= 0x100);
    type Pages = std::collections::BTreeMap<u32, (u64, u32, u32)>;
    let mut by_pc: std::collections::BTreeMap<u32, Pages> = Default::default();
    for &(pc, a) in &trace {
        let wram = (0x7E_0000..0x80_0000).contains(&a) || (a & 0xFFFF) < 0x2000;
        if (a < from && !wram) || (wram && bank.is_none()) {
            continue;
        }
        // Skip operand reads of the instruction itself.
        if (a > pc && a - pc <= 3) || !interesting(pc) {
            continue;
        }
        let e = by_pc
            .entry(pc)
            .or_default()
            .entry(a & 0xFF_FF00)
            .or_insert((0, a, a));
        e.0 += 1;
        e.1 = e.1.min(a);
        e.2 = e.2.max(a);
    }
    println!(
        "{} data reads; ROM data reads at or above ${from:06X} by instruction:",
        trace.len()
    );
    for (pc, pages) in by_pc {
        let total: u64 = pages.values().map(|p| p.0).sum();
        println!("  instruction ${pc:06X}: {total} reads");
        for (page, (count, lo, hi)) in pages.iter().take(6) {
            println!("      ${page:06X}: {count:>6} reads, ${lo:06X}-${hi:06X}");
        }
        if pages.len() > 6 {
            println!("      ... {} more pages", pages.len() - 6);
        }
    }
    Ok(())
}

/// `level reads` without the reading instructions, which a ROM Lunar
/// Magic saved withholds (clean room): the ROM pages read at or above
/// `from`, and with a bank given, none of the work RAM, since which of it
/// the reads near that bank made is the instructions' to say.
fn reads_by_page(trace: &expand::ReadTrace, from: u32, bank: Option<u32>) -> Result<()> {
    if bank.is_some() {
        bail!(
            "--near-bank needs the reading instructions, which a ROM Lunar Magic saved withholds (clean room)"
        );
    }
    let mut pages: std::collections::BTreeMap<u32, (u64, u32, u32)> = Default::default();
    for &(_, a) in trace {
        let wram = (0x7E_0000..0x80_0000).contains(&a) || (a & 0xFFFF) < 0x2000;
        if a < from || wram {
            continue;
        }
        let e = pages.entry(a & 0xFF_FF00).or_insert((0, a, a));
        e.0 += 1;
        e.1 = e.1.min(a);
        e.2 = e.2.max(a);
    }
    println!(
        "{} data reads; ROM data reads at or above ${from:06X} by page (instructions withheld: clean room):",
        trace.len()
    );
    for (page, (count, lo, hi)) in pages {
        println!("  ${page:06X}: {count:>6} reads, ${lo:06X}-${hi:06X}");
    }
    Ok(())
}

fn level_sprites(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    let start = loaded.sprite_data_ptr();
    let list = sprites::read_sprites_at(rom, start)?;
    println!(
        "level {level:03X}: sprite data at {start}, {} bytes, memory {}, buoyancy {}, new system {}",
        list.len, list.header.memory, list.header.buoyancy, list.header.new_sprite_system
    );
    println!("  id  xb screen  x  y  extension");
    for s in &list.sprites {
        let ext: Vec<String> = s.extension.iter().map(|b| format!("{b:02X}")).collect();
        println!(
            "  {:02X}  {}  {:02X}     {:X}  {:02X} {}",
            s.id,
            s.extra_bits,
            s.screen,
            s.x,
            s.y,
            ext.join(" ")
        );
    }
    Ok(())
}

fn level_map16(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    let mut numbers: Vec<_> = loaded.tiles.map16.keys().copied().collect();
    numbers.sort_unstable();
    let shown = loaded.tiles.foreground_map16_shown(rom);
    let mut withheld = 0;
    for n in numbers {
        let Some(tile) = shown[n as usize] else {
            withheld += 1;
            continue;
        };
        let hex: Vec<String> = tile.to_bytes().iter().map(|x| format!("{x:02X}")).collect();
        println!("{n:04X}: {}", hex.join(" "));
    }
    if withheld > 0 {
        eprintln!(
            "{withheld} definitions withheld: the ROM's routine found them outside its Map16 \
             tables (clean room)"
        );
    }
    Ok(())
}

fn level_wram(rom: &Rom, level: &str, addr: &str, len: usize, bus: bool) -> Result<()> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    let start = u32::from_str_radix(addr.trim_start_matches('$'), 16).context("bad address")?;
    // The stacks hold return addresses: they read as zero (clean room).
    let bytes: Vec<u8> = if bus {
        let len = len.min((0x100_0000 - start) as usize) as u32;
        (start..start + len)
            .map(|a| {
                loaded
                    .ram
                    .read(a)
                    .with_context(|| format!("${a:06X} is not RAM"))?;
                Ok(clean_room::peek(&loaded.ram, a))
            })
            .collect::<Result<_>>()?
    } else {
        let Some(first) = RamAddr::checked(start) else {
            bail!("address must be in $7E0000-$7FFFFF");
        };
        let len = len.min((0x80_0000 - start) as usize);
        // Each address resolved on its own, so that a ROM that moves the
        // game's variables (SA-1 Pack) still dumps as the vanilla layout.
        clean_room::bytes(&loaded.ram, first, len)
    };
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
        println!("${:06X}: {}", start as usize + i * 16, hex.join(" "));
    }
    Ok(())
}

fn level_tiles(rom: &Rom, level: &str) -> Result<()> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    println!(
        "level {level:03X}: {} screens, mode {}, vertical {}",
        loaded.tiles.screens, loaded.tiles.level_mode, loaded.tiles.vertical
    );
    let (w, h) = loaded.tiles.size();
    for y in 0..h {
        let row: Vec<String> = (0..w)
            .map(|x| format!("{:03X}", loaded.tiles.tile_at(x, y)))
            .collect();
        println!("{}", row.join(" "));
    }
    Ok(())
}

/// Loads a level for a sheet command, reporting its diagnostics as
/// `level png` does.
fn load_for_sheet(rom: &Rom, level: &str) -> Result<(u16, expand::LoadedLevel)> {
    let level = parse_level(level)?;
    let loaded = expand::expand_level(rom, level)?;
    for line in expand::summarize(&loaded.diagnostics) {
        eprintln!("warning: level {level:03X}: {line}");
    }
    Ok((level, loaded))
}

fn palette_png(rom: &Rom, sel: &PaletteArgs, out: &PathBuf) -> Result<()> {
    let (pal, what) = match &sel.level {
        Some(level) => {
            let (level, loaded) = load_for_sheet(rom, level)?;
            (loaded.video.palette(), format!("level {level:03X}"))
        }
        None => {
            let (sel, _) = sel.resolve(rom)?;
            (
                palette::vanilla_level_palette(rom, sel)?,
                format!("palette {sel:?}"),
            )
        }
    };
    let img = render::palette_swatch(&pal, 16);
    img.write_png(out)?;
    println!("{what} -> {}", out.display());
    Ok(())
}

fn map16_png(
    rom: &Rom,
    sel: &PaletteArgs,
    tileset: Option<u8>,
    layer: u8,
    out: &PathBuf,
) -> Result<()> {
    if !(1..=2).contains(&layer) {
        bail!("layer must be 1 or 2");
    }
    let first = if layer == 1 { 0 } else { map16::FG_TILE_COUNT };
    let (definitions, tiles, pal, back, what): (Vec<Option<Map16Tile>>, _, _, _, _) =
        match &sel.level {
            Some(level) => {
                if tileset.is_some() {
                    bail!("--tileset selects the vanilla tables; leave it out with --level");
                }
                let (level, loaded) = load_for_sheet(rom, level)?;
                // Clean room: a definition found outside the ROM's Map16
                // tables may be code, which the picture would show.
                let definitions = if layer == 1 {
                    loaded.tiles.foreground_map16_shown(rom)
                } else {
                    loaded.tiles.bg_map16_shown(rom)
                };
                // The back area colour is the fixed colour the level set
                // up, not a CGRAM entry.
                let back = loaded.scene.screen.fixed_color.to_rgb8();
                (
                    definitions,
                    LayerTiles::from_vram(&loaded.video.vram),
                    loaded.video.palette(),
                    back,
                    format!("level {level:03X}"),
                )
            }
            None => {
                let (sel, _) = sel.resolve(rom)?;
                let tileset = tileset.unwrap_or(0);
                let table = map16::vanilla_map16(rom, tileset, true)?;
                let (foreground, background) = table.tiles.split_at(map16::FG_TILE_COUNT);
                let tiles = if layer == 1 { foreground } else { background };
                (
                    tiles.iter().copied().map(Some).collect(),
                    LayerTiles::for_object_tileset(rom, tileset)?,
                    palette::vanilla_level_palette(rom, sel)?,
                    palette::vanilla_back_area_color(rom, sel.back_area)?.to_rgb8(),
                    format!("tileset {tileset}, palette {sel:?}"),
                )
            }
        };
    let img = render::map16_sheet(&definitions, &tiles, &pal, back, 16);
    img.write_png(out)?;
    println!(
        "{what}, layer {layer}: tiles {first:03X}-{:03X}, {}x{} -> {}",
        first + definitions.len().max(1) - 1,
        img.width,
        img.height,
        out.display()
    );
    Ok(())
}

fn rom_info(rom: &Rom) -> Result<()> {
    let h = rom.internal_header();
    let computed = rom.compute_checksum();
    if let Some(path) = rom.source() {
        println!("path:            {}", path.display());
    }
    println!(
        "size:            {} bytes ({} KiB)",
        rom.len(),
        rom.len() / 1024
    );
    println!(
        "copier header:   {}",
        if rom.has_copier_header() { "yes" } else { "no" }
    );
    println!("mapping:         {:?}", rom.mapping());
    println!("title:           {:?}", h.title);
    println!("map mode:        ${:02X}", h.map_mode);
    println!("cartridge type:  ${:02X}", h.cartridge_type);
    println!("declared size:   {} KiB", h.rom_size()? / 1024);
    println!("sram:            {} KiB", h.sram_size()? / 1024);
    println!("region:          ${:02X}", h.region);
    println!("version:         1.{}", h.version);
    println!(
        "checksum:        ${:04X} (complement ${:04X}, computed ${:04X}, {})",
        h.checksum,
        h.checksum_complement,
        computed,
        if h.checksum_pair_valid() && computed == h.checksum {
            "ok"
        } else {
            "MISMATCH"
        }
    );
    println!("sha1:            {}", rom.sha1_hex());
    println!("identity:        {:?}", rom.identify());
    if let Some(v) = rom.lunar_magic_version() {
        println!("lunar magic:     {v}");
    }
    Ok(())
}

fn rom_expand(mut rom: Rom, size: &str, out: &PathBuf) -> Result<()> {
    let len = parse_size(size)?;
    rom.expand(len)?;
    rom.fix_checksum()?;
    rom.save(out)?;
    println!(
        "{}: {} KiB, checksum ${:04X}",
        out.display(),
        rom.len() / 1024,
        rom.internal_header().checksum
    );
    Ok(())
}

fn parse_size(text: &str) -> Result<usize> {
    let (digits, unit) = match text.char_indices().last() {
        Some((i, 'K' | 'k')) => (&text[..i], 1024),
        Some((i, 'M' | 'm')) => (&text[..i], 1024 * 1024),
        _ => (text, 1),
    };
    digits
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .with_context(|| format!("size must be like 1M, 1536K, or a byte count, not {text:?}"))
}

fn rom_rats(rom: &Rom) -> Result<()> {
    let blocks = rats::blocks(rom);
    for block in &blocks {
        let pc = rom.pc(block.start)?;
        println!("{}  {pc}  {:5} bytes", block.start, block.len);
    }
    let tagged: usize = blocks.iter().map(|b| b.len + rats::TAG_LEN).sum();
    println!(
        "{} blocks, {tagged} bytes tagged; {} bytes free",
        blocks.len(),
        FreeSpace::scan(rom).free_bytes()
    );
    Ok(())
}

fn import(
    from: &Path,
    dir: &Path,
    all: bool,
    level: Option<&str>,
    sizes_from: Option<&Path>,
    pixi: Option<&Path>,
    clean: &Rom,
) -> Result<()> {
    if from.is_dir() {
        if all || level.is_some() || sizes_from.is_some() || pixi.is_some() {
            bail!(
                "--all, --level, --sizes-from, and --pixi are for a ROM or an MWL file; a \
                 Callisto project's configuration says what to import"
            );
        }
        let options = kobo_core::import::CallistoOptions {
            origin: Some(format!("the Callisto project in {}", from.display())),
            ..Default::default()
        };
        for tool in [Tool::Asar, Tool::Pixi] {
            announce_download(tool);
        }
        let report = kobo_core::import::import_callisto(from, clean, dir, &options)?;
        print_report(dir, &report);
        return Ok(());
    }
    let is_mwl = from
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mwl"));
    if is_mwl && all {
        bail!("--all is for importing a ROM; an MWL file holds one level");
    }
    if !is_mwl && (level.is_some() || sizes_from.is_some()) {
        bail!("--level and --sizes-from are for importing an MWL file");
    }
    if is_mwl && pixi.is_some() {
        bail!("--pixi is for importing a ROM");
    }
    let report = if is_mwl {
        let bytes = fs::read(from).with_context(|| format!("reading {}", from.display()))?;
        let level = level.map(parse_level).transpose()?;
        let sizes_rom = sizes_from
            .map(|path| Rom::load(path).with_context(|| format!("loading {}", path.display())))
            .transpose()?;
        let sizes = match &sizes_rom {
            Some(rom) => kobo_core::sprites::pixi_size_table(rom)?,
            None => None,
        };
        kobo_core::import::import_mwl_sized(&bytes, clean, dir, level, sizes)?
    } else {
        let rom = Rom::load(from).with_context(|| format!("loading {}", from.display()))?;
        if let Some(folder) = pixi {
            for tool in [Tool::Asar, Tool::Pixi] {
                announce_download(tool);
            }
            if !folder.is_dir() {
                bail!("--pixi: {} is not a folder", folder.display());
            }
        }
        let options = kobo_core::import::Options { all, pixi };
        kobo_core::import::import_rom_with(&rom, clean, dir, &options)?
    };
    print_report(dir, &report);
    Ok(())
}

fn new_project(dir: Option<&Path>, template: Option<&str>, list: bool, rom: &RomArg) -> Result<()> {
    use kobo_core::template;
    if list {
        for (name, recipe) in template::templates() {
            println!(
                "{name}: {} {} ({})",
                recipe.title, recipe.version, recipe.homepage
            );
        }
        return Ok(());
    }
    let (Some(dir), Some(name)) = (dir, template) else {
        bail!("give the project's directory and --template; --list names the templates");
    };
    let recipe = template::template(name)?;
    let clean = rom.load()?;
    eprintln!(
        "{} {}: {}{}, checked by SHA-256 (downloaded once, then cached)",
        recipe.title, recipe.version, recipe.url, recipe.build.file
    );
    let report = recipe.create(&clean, dir)?;
    print_report(dir, &report);
    println!(
        "{}",
        recipe
            .rights
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(())
}

fn print_report(dir: &Path, report: &kobo_core::import::Report) {
    for note in &report.notes {
        println!("note: {note}");
    }
    let bytes: usize = report.unmodelled.iter().map(|(_, len)| len).sum();
    if bytes > 0 {
        println!(
            "not imported: {} ranges ({bytes} bytes) of the clean ROM's space that the ROM changed outside its levels:",
            report.unmodelled.len()
        );
        for (at, len) in report.unmodelled.iter().take(20) {
            println!("  {at} +{len}");
        }
        if report.unmodelled.len() > 20 {
            println!("  ...");
        }
    }
    if !report.unread_blocks.is_empty() {
        let bytes: usize = report.unread_blocks.iter().map(|b| b.len).sum();
        println!(
            "not imported: {} tagged blocks ({bytes} bytes) in the expanded ROM that no level uses",
            report.unread_blocks.len()
        );
    }
    println!(
        "{}: {} levels and {} Map16 pages imported",
        dir.display(),
        report.levels.len(),
        report.map16.len()
    );
}

fn build(dir: &Path, out: &Path, patch: Option<&Path>, no_cache: bool, clean: &Rom) -> Result<()> {
    use kobo_core::build::{self, Cache, Project};
    let project = Project::load(dir)?;
    for warning in build::warnings(&project) {
        eprintln!("warning: {warning}");
    }
    for tool in build::tools(&project) {
        announce_download(tool);
    }
    // Every tool is found before the build starts, so what it depends on
    // is said even if it fails.
    for located in build::locate_tools(&project)? {
        if let Some(note) = located.note() {
            eprintln!("note: {note}");
        }
    }
    let cache = if no_cache { None } else { Cache::user() };
    let rom = build::build_cached(clean, &project, cache.as_ref())?;
    rom.save(out)?;
    println!(
        "{}: {} KiB, {} levels, sha1 {}",
        out.display(),
        rom.len() / 1024,
        project.levels.len(),
        rom.sha1_hex()
    );
    if let Some(path) = patch {
        let bytes = bps::create(clean.data(), rom.data());
        fs::write(path, &bytes).with_context(|| format!("writing {}", path.display()))?;
        println!("{}: {} bytes", path.display(), bytes.len());
    }
    Ok(())
}

fn diff(a: &Path, b: &Path, project: Option<&Path>) -> Result<()> {
    let load = |p: &Path| Rom::load(p).with_context(|| format!("loading {}", p.display()));
    let mut diffs = kobo_core::import::diff_levels(&load(a)?, &load(b)?);
    if let Some(dir) = project {
        let project = kobo_core::build::Project::load(dir)?;
        diffs.retain(|d| project.manifest.levels.contains_key(&d.level));
    }
    for d in &diffs {
        println!("{:03X}: {}", d.level, d.parts.join(", "));
    }
    if !diffs.is_empty() {
        bail!("{} levels differ", diffs.len());
    }
    println!("the levels are the same");
    Ok(())
}

fn fmt(dir: &Path, check: bool) -> Result<()> {
    let files = kobo_core::source::format_project(dir)?;
    let mut changed = 0;
    for file in files.into_iter().filter(|f| f.changed()) {
        changed += 1;
        if check {
            println!("would reformat {}", file.path.display());
        } else {
            fs::write(&file.path, file.formatted)
                .with_context(|| format!("writing {}", file.path.display()))?;
            println!("reformatted {}", file.path.display());
        }
    }
    if check && changed > 0 {
        bail!("{changed} files are not in Kobo's format");
    }
    Ok(())
}

fn gfx_list(rom: &Rom) -> Result<()> {
    let reader = gfx::GfxReader::new(rom)?;
    println!("compression: {}", reader.compression());
    println!("file   addr     format  tiles  stored  compressed");
    let mut failed = 0;
    for index in 0..GFX_FILE_COUNT {
        match reader.read(index) {
            Ok(f) => println!(
                "GFX{:02X}  {}  {:<6}  {:>5}  {:>6}  {:>10}",
                index,
                f.addr,
                f.bpp()
                    .map_or("packed".to_string(), |b| format!("{}bpp", b.bits())),
                f.tile_count(),
                f.data.len(),
                f.compressed_len
            ),
            Err(e) => {
                failed += 1;
                println!("GFX{index:02X}  error: {e}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} GFX files could not be read");
    }
    Ok(())
}

fn gfx_export(rom: &Rom, dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let reader = gfx::GfxReader::new(rom)?;
    for index in 0..GFX_FILE_COUNT {
        let f = reader.read(index)?;
        let path = dir.join(format!("GFX{index:02X}.bin"));
        fs::write(&path, f.to_lm_export())
            .with_context(|| format!("writing {}", path.display()))?;
    }
    println!("wrote {GFX_FILE_COUNT} files to {}", dir.display());
    Ok(())
}

fn gfx_png(rom: &Rom, index: &str, out: &PathBuf, columns: u32, bpp: Option<u8>) -> Result<()> {
    let index = u8::from_str_radix(index, 16).context("GFX index must be hex, e.g. 1A")?;
    let f = gfx::read_gfx_file(rom, index)?;
    let forced = match bpp {
        None => None,
        Some(2) => Some(Bpp::Two),
        Some(3) => Some(Bpp::Three),
        Some(4) => Some(Bpp::Four),
        Some(other) => bail!("unsupported bit depth {other}; use 2, 3, or 4"),
    };
    let (tiles, colors) = match forced {
        Some(bpp) => (gfx::decode_tiles(bpp, &f.data), bpp.colors()),
        None => (f.tiles(), f.colors()),
    };
    let img = tile_sheet(&tiles, columns, &grayscale(colors));
    img.write_png(out)?;
    println!(
        "GFX{index:02X}: {} tiles, {} colours, {}x{} -> {}",
        tiles.len(),
        colors,
        img.width,
        img.height,
        out.display()
    );
    Ok(())
}

fn bps_apply(rom: &Rom, patch: &Path, out: &Path) -> Result<()> {
    let bytes = fs::read(patch).with_context(|| format!("reading {}", patch.display()))?;
    let patched =
        bps::apply_to_rom(&bytes, rom).with_context(|| format!("applying {}", patch.display()))?;
    fs::write(out, &patched.data).with_context(|| format!("writing {}", out.display()))?;
    let form = match patched.source {
        bps::SourceForm::Headerless => "headerless",
        bps::SourceForm::OwnHeader => "with the ROM's copier header",
        bps::SourceForm::SizeHeader => "with a size copier header",
        bps::SourceForm::ZeroHeader => "with a zero copier header",
    };
    println!(
        "{}: made against the clean ROM {form}; wrote {} bytes to {}",
        patch.display(),
        patched.data.len(),
        out.display()
    );
    Ok(())
}

fn bps_create(rom: &Rom, modified: &Path, out: &Path) -> Result<()> {
    let target = Rom::load(modified).with_context(|| format!("loading {}", modified.display()))?;
    let patch = bps::create(rom.data(), target.data());
    fs::write(out, &patch).with_context(|| format!("writing {}", out.display()))?;
    println!(
        "{} -> {}: {} bytes",
        modified.display(),
        out.display(),
        patch.len()
    );
    Ok(())
}

fn convert_addr(text: &str, sa1: bool) -> Result<()> {
    let mapping = if sa1 { Mapping::Sa1Rom } else { Mapping::LoRom };
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        let pc = PcAddr::new(u32::from_str_radix(hex, 16).context("bad file offset")?);
        let snes = mapping.pc_to_snes(pc)?;
        println!("{pc} -> {snes} ({mapping:?})");
    } else {
        let hex = text.strip_prefix('$').unwrap_or(text);
        let raw = u32::from_str_radix(hex, 16).context("bad SNES address")?;
        if raw > 0xFF_FFFF {
            bail!("SNES address {text} does not fit in 24 bits");
        }
        let snes = SnesAddr::new(raw);
        let pc = mapping.snes_to_pc(snes)?;
        println!("{snes} -> {pc} ({mapping:?})");
    }
    Ok(())
}

/// Says that a tool's pinned build is about to be downloaded, if it is.
fn announce_download(tool: Tool) {
    if let (true, Some(source)) = (tool.needs_download(), tool.source()) {
        eprintln!(
            "downloading {} {} ({} KiB) from {}",
            tool,
            source.version,
            source.build.size / 1024,
            source.url
        );
    }
}

fn parse_tool(key: &str) -> Result<Tool> {
    Tool::from_key(key).with_context(|| {
        let keys: Vec<_> = Tool::ALL.iter().map(|t| t.key()).collect();
        format!("no tool {key:?}; the tools are {}", keys.join(", "))
    })
}

fn tools_list() -> Result<()> {
    let pins = tools::pinned::pins();
    println!(
        "pinned builds: kobo-tools {} for {}, cached in {}",
        pins.release,
        tools::pinned::platform().unwrap_or("no platform Kobo pins builds for"),
        tools::pinned::cache_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "nowhere (set KOBO_TOOL_CACHE)".into())
    );
    for tool in Tool::ALL {
        let version = tool.version().map(|v| format!(" {v}")).unwrap_or_default();
        let place = match tool.locate_offline() {
            Ok(located) => match &located.origin {
                tools::Origin::Configured(setting) => {
                    format!("{} (from {setting})", located.path.display())
                }
                tools::Origin::Pinned { .. } => format!("{} (pinned)", located.path.display()),
            },
            Err(ToolError::NotCached { .. }) => "pinned, not downloaded yet".into(),
            Err(e) => e.to_string(),
        };
        println!("{:<13}{version:<6} {place}", tool.name());
    }
    Ok(())
}

fn tools_fetch(keys: &[String]) -> Result<()> {
    let tools: Vec<Tool> = if keys.is_empty() {
        Tool::ALL
            .into_iter()
            .filter(|t| t.version().is_some())
            .collect()
    } else {
        keys.iter().map(|k| parse_tool(k)).collect::<Result<_>>()?
    };
    for tool in tools {
        if tool.version().is_none() {
            bail!("Kobo pins no build of {tool}, which has no licence; configure a path to it");
        }
        announce_download(tool);
        let located = tool.locate()?;
        match located.note() {
            Some(note) => println!("{tool}: {note}"),
            None => println!("{tool}: {}", located.path.display()),
        }
    }
    Ok(())
}

fn tools_path(key: &str) -> Result<()> {
    let tool = parse_tool(key)?;
    announce_download(tool);
    println!("{}", tool.locate()?.path.display());
    Ok(())
}
