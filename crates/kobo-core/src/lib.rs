//! Kobo core library.
//!
//! Everything the editor, CLI, and scripting API can do lives here. The
//! shells on top of this crate are meant to be thin.

pub mod addr;
pub mod asar;
pub mod bps;
pub mod build;
pub mod callisto;
pub mod clean_room;
pub mod compress;
pub mod config;
pub mod cpu;
pub mod entrance;
pub mod exanimation;
pub mod exgfx;
pub mod expand;
pub mod gfx;
pub mod image;
pub mod import;
pub mod install;
pub mod level;
pub mod map16;
pub mod map16_file;
pub mod mwl;
pub mod names;
pub mod operation;
pub mod palette;
pub mod pixi;
pub mod ram;
pub mod rats;
pub mod render;
pub mod rom;
pub mod source;
pub mod sprites;
pub mod template;
pub mod tools;
pub mod video;

pub use addr::{MapError, Mapping, PcAddr, SnesAddr};
pub use rom::{Rom, RomError, RomIdentity};
