//! A 65816 interpreter for running the ROM's own routines headlessly.
//!
//! This is not a console emulator: there is no cycle timing or full PPU. IRQs and interrupt handlers run
//! where the headless bus and capture passes request them. It exists so the level loader, object expansion, and
//! similar pure-CPU routines can run on the real ROM code, which keeps
//! Kobo faithful to patched ROMs without re-implementing every routine.

pub mod access;
mod core65816;
pub mod sa1;
pub(crate) mod smw_bus;
pub mod watch;

pub use core65816::{Bus, Cpu, CpuError, Executed, Flags, Run};
