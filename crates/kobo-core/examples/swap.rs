//! A ROM Lunar Magic saved, with Kobo's code for a piece of Lunar Magic's
//! layout in place of Lunar Magic's, keeping the ROM's tables
//! (`tests/common/swap.rs` has what each piece puts back and keeps):
//!
//! `cargo run --release --example swap -- PIECE lm.smc out.sfc`
//!
//! PIECE is `bank06` (the Map16 routine, acts-like chain, placed objects,
//! level number, backgrounds, palettes, and exits), `vram`, `graphics`
//! (`vram+graphics` swaps the VRAM patch first), `layer3`, `sprites`,
//! `exanim`, `exlevel`, or `entrances`; several, joined by `+`, apply in
//! order. Then compare the two ROMs with render_hashes, ramdiff.py, or the
//! probe for the piece (docs/testing.md). The game's bytes come from the
//! vanilla ROM, or for an SA-1 ROM the SA-1 reference ROM
//! (`KOBO_SA1_REFERENCE`); Asar's library is the configured one or the
//! pinned build. The output's checksum is fixed.

#[path = "../tests/common/swap.rs"]
mod swap;

use kobo_core::asar::Asar;
use kobo_core::tiers::Tier;
use kobo_core::tools::Tool;
use kobo_core::{Rom, config};
use swap::Piece;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [pieces, lm, out] = args.as_slice() else {
        eprintln!(
            "usage: swap PIECE[+PIECE...] lm.smc out.sfc (pieces: {})",
            names()
        );
        std::process::exit(2);
    };
    let pieces: Vec<Piece> = pieces
        .split('+')
        .map(|name| {
            Piece::from_name(name).unwrap_or_else(|| {
                eprintln!("no piece {name}: {}", names());
                std::process::exit(2)
            })
        })
        .collect();
    let lm = Rom::load(lm).unwrap_or_else(|e| fail(format!("{lm}: {e}")));
    let base = if lm.mapping().is_sa1() {
        let path = Tier::Sa1Reference
            .path()
            .unwrap_or_else(|e| fail(e.to_string()))
            .unwrap_or_else(|| {
                fail("an SA-1 ROM needs the SA-1 reference ROM: KOBO_SA1_REFERENCE".into())
            });
        Rom::load(&path).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())))
    } else {
        let path = config::vanilla_rom_path().unwrap_or_else(|e| fail(e.to_string()));
        Rom::load(&path).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())))
    };
    let library = Tool::Asar
        .locate()
        .unwrap_or_else(|e| fail(e.to_string()))
        .path;
    let asar = Asar::load(&library).unwrap_or_else(|e| fail(e.to_string()));
    let mut rom = lm;
    for piece in pieces {
        rom = swap::swap(&asar, piece, &rom, &base)
            .unwrap_or_else(|e| fail(format!("{}: {e}", piece.name())));
    }
    rom.fix_checksum().unwrap_or_else(|e| fail(e.to_string()));
    rom.save(out)
        .unwrap_or_else(|e| fail(format!("{out}: {e}")));
}

fn names() -> String {
    Piece::ALL.map(|p| p.name()).join(", ")
}

fn fail(message: String) -> ! {
    eprintln!("swap: {message}");
    std::process::exit(1)
}
