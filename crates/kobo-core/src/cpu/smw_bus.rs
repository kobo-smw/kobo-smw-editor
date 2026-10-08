//! A bus that maps a SMW ROM plus work RAM, and captures what the game
//! uploads to video memory.
//!
//! Only the pieces of hardware the loaders touch are modelled: the VRAM,
//! CGRAM, and OAM data ports, and general-purpose DMA to them. Everything
//! else reads as zero and ignores writes.
//!
//! On an SA-1 cartridge the bus also runs the SA-1. Only one processor
//! runs at a time: the SA-1 gets its turn when the S-CPU stops to wait,
//! and runs until it in turn waits (its idle loop, or for the S-CPU to
//! answer an IRQ). The game hands work over and then waits for it, so
//! this is the order things happen in on the console too. It is not when
//! the S-CPU starts the SA-1 or sends it an IRQ: SA-1 Pack's start-up
//! clears the flag the SA-1 answers with just after releasing it from
//! reset, counting on the SA-1 to take longer than that. The SA-1 sees
//! the bus through [`Sa1View`].

use crate::cpu::access::{AccessKind, UnsupportedAccesses};
use crate::cpu::sa1::Processor;
use crate::cpu::{Bus, Cpu, CpuError};
use crate::ram::{Ram, RamMap};
use crate::rom::Rom;

/// Instruction limit for one turn of the SA-1.
const SA1_STEP_LIMIT: u64 = 200_000_000;

pub const VRAM_LEN: usize = 0x1_0000;
pub const CGRAM_LEN: usize = 0x200;
/// The PPU's object memory: four bytes for each of 128 objects, then
/// their size and X-high bits four objects to a byte.
pub const OAM_LEN: usize = 0x220;

#[derive(Clone, Copy, Default, Debug)]
struct DmaChannel {
    control: u8,
    dest: u8,
    src: u32,
    size: u16,
}

/// ROM, the game's RAM, VRAM, CGRAM, and enough register state to follow
/// uploads. The game can write video memory but never reads it back here,
/// so a routine's outcome depends on `ram` alone.
pub struct SmwBus<'a> {
    pub(crate) operation: Option<crate::operation::Operation>,
    pub rom: &'a Rom,
    pub ram: Ram,
    pub vram: Vec<u8>,
    /// Which VRAM bytes have been written since reset, so callers can
    /// tell uploaded data from the untouched zero fill.
    pub vram_written: Vec<bool>,
    /// The cartridge's save RAM, as much as the header declares, in
    /// LoROM banks `$70`-`$7D` and `$F0`-`$FF` below `$8000`, mirrored
    /// as the chip is across that window. Empty on an SA-1 cartridge,
    /// whose BW-RAM `Ram` holds, and where those banks map nothing.
    pub sram: Vec<u8>,
    pub cgram: Vec<u8>,
    /// Object memory as the game's OAM upload last left it.
    pub oam: Vec<u8>,
    /// `OAMADD` (`$2102`-`$2103`): the word address a write to either
    /// register reloads the port from, and the priority rotation bit.
    oam_reload: u16,
    oam_rotation: bool,
    /// The port's byte address. The first 512 bytes are written a word at
    /// a time, the low byte waiting in the latch for the high one.
    oam_address: u16,
    oam_latch: u8,
    vmain: u8,
    /// VRAM word address.
    vmadd: u16,
    /// The word a read of `VMDATAL`/`VMDATAH` (`$2139`-`$213A`) returns:
    /// the port reads ahead, so it holds the word at the address as it
    /// was set, and is refilled before the address steps, as the
    /// hardware's port does. Graphics uploads that put work RAM aside in
    /// VRAM read it back through here.
    vram_latch: u16,
    /// CGRAM byte address (colour index * 2 + half).
    cgadd: u16,
    /// Work RAM address of the data port `WMDATA` (`$2180`), 17 bits.
    /// SA-1 Pack decompresses graphics into BW-RAM on the SA-1 and has
    /// the S-CPU DMA them into work RAM through it.
    wmadd: u32,
    /// Last values written to `BG1SC`-`BG4SC` (`$2107`-`$210A`): tilemap
    /// VRAM base and size per layer. Lunar Magic moves the layer 1 and 2
    /// tilemaps, so these are how to find them.
    pub bg_sc: [u8; 4],
    pub bg_character_base: [u16; 4],
    pub bg_scroll: [[u16; 2]; 4],
    pub bg_mode: u8,
    pub object_select: u8,
    /// Last values written to `TM` and `TS` (`$212C`-`$212D`): the layers
    /// on the main screen and the subscreen. The game writes them once
    /// per level load from its mirrors (`$0D9D`-`$0D9E`) and never again,
    /// so code that writes the registers afterwards has the last word
    /// (Akogare2 puts layer 2 on the main screen that way).
    pub screen_layers: [u8; 2],
    /// Last values written to the window registers the game keeps no RAM
    /// mirror of: `WH2`/`WH3` (`$2128`-`$2129`), `WBGLOG`/`WOBJLOG`
    /// (`$212A`-`$212B`), and `TMW`/`TSW` (`$212E`-`$212F`).
    pub window2: [u8; 2],
    pub window_logic: [u8; 2],
    pub window_masks: [u8; 2],
    pub mode7: crate::video::Mode7,
    bg_scroll_latch: u8,
    mode7_latch: u8,
    pub irq_scanline: u16,
    pub interrupt_enable: u8,
    /// `TIMEUP` (`$4211`): the PPU's timer has asked for an IRQ, until
    /// the handler reads it.
    timer_irq: bool,
    hblank: bool,
    /// CPU multiply and divide registers (`$4202`-`$4206` in, `$4214`-`$4217`
    /// out). Sprite code leans on them.
    multiplicand: u8,
    dividend: u16,
    quotient: u16,
    product: u16,
    dma: [DmaChannel; 8],
    /// Last values written to the APU I/O ports `$2140`-`$2143`.
    apu_ports: [u8; 4],
    /// Models the SPC700 boot ROM upload protocol: outside a transfer the
    /// ports read `$AA`/`$BB` while port 0 holds zero; during a transfer
    /// every write is echoed. A block header with port 1 zero ends the
    /// transfer (the boot ROM jumps to the uploaded code, and SMW's sound
    /// engine then presents `$AA`/`$BB` again for the sample upload).
    apu_in_transfer: bool,
    apu_expected: u8,
    /// The block-end header byte, echoed exactly once after a transfer
    /// ends before the ports go back to `$AA`/`$BB`.
    apu_jump_echo: Option<u8>,
    /// A block header was just written to port 0; a zero written to
    /// port 1 right after it ends the transfer (AddmusicK's ordering).
    apu_header_pending: bool,
    /// Bounded detail of the accesses to hardware this bus does not
    /// model, which a picture may be missing something for.
    pub unsupported: UnsupportedAccesses,
    /// Accesses that need no model: open bus, read-only registers, idle
    /// controllers, and registers whose effect the capture passes read
    /// from RAM instead (`write_register` lists them).
    pub stubbed_writes: u64,
    pub stubbed_reads: u64,
    /// What controller 1 holds, as the auto-read leaves it in `$4218`
    /// (low byte: A, X, L, R) and `$4219` (high: B, Y, Select, Start, and
    /// the pad): none, unless a pass presses something, as the overworld's
    /// new game does.
    pub pad: u16,
    /// Game variables held at a value: the S-CPU's write to one leaves it
    /// as it was. A pass's way to give the game a state its own code
    /// would set otherwise (the overworld's events passed, which a new
    /// game clears as it starts).
    pub pinned: Vec<(crate::ram::RamAddr, u8)>,
    /// Every unmodelled access, including those beyond the report's cap.
    pub unmapped_reads: u64,
    pub unmapped_writes: u64,
    /// The CPU is reading an instruction, not data: an unmodelled access
    /// is then reported as a fetch.
    fetching: bool,
    /// Debugging aid: a VRAM word address whose every write is reported
    /// on standard error, from `KOBO_VRAM_WATCH` (`$3A1E` or `3A1E`).
    vram_watch: Option<usize>,
    /// The A-bus address the DMA in progress is reading, for the report.
    dma_source: Option<u32>,
    /// When set, the S-CPU's data reads and writes in its ranges are
    /// logged, in order (`expand::ObjectMap` watches the loader so).
    pub access_log: Option<super::watch::AccessLog>,
}

/// `KOBO_VRAM_WATCH`: the VRAM word every write to is reported.
fn watched_vram_word() -> Option<usize> {
    let value = std::env::var("KOBO_VRAM_WATCH").ok()?;
    let word = u32::from_str_radix(value.trim().trim_start_matches('$'), 16).ok()?;
    Some(word as usize & 0x7FFF)
}

/// Bytes of save RAM a LoROM cartridge declares, capped at the 512 KiB a
/// header can mean rather than trusting a malformed one with more.
fn sram_len(rom: &Rom) -> usize {
    if rom.mapping() != crate::addr::Mapping::LoRom {
        return 0;
    }
    rom.internal_header().sram_size().unwrap_or(0).min(0x8_0000)
}

impl<'a> SmwBus<'a> {
    /// Puts the pinned variables back after a write that may have landed
    /// on one: any whose address agrees in its low 13 bits, the part every
    /// mapping of low RAM keeps.
    fn repin(&mut self, addr: u32) {
        let low = addr & 0x1FFF;
        if self
            .pinned
            .iter()
            .any(|(at, _)| at.vanilla() & 0x1FFF == low)
        {
            for &(at, value) in &self.pinned {
                self.ram.set_u8(at, value);
            }
        }
    }

    pub fn new(rom: &'a Rom) -> Self {
        crate::clean_room::check(rom);
        Self {
            rom,
            operation: None,
            ram: Ram::new(RamMap::of(rom)),
            vram: vec![0; VRAM_LEN],
            vram_written: vec![false; VRAM_LEN],
            sram: vec![0; sram_len(rom)],
            cgram: vec![0; CGRAM_LEN],
            oam: vec![0; OAM_LEN],
            oam_reload: 0,
            oam_rotation: false,
            oam_address: 0,
            oam_latch: 0,
            vmain: 0,
            vmadd: 0,
            vram_latch: 0,
            cgadd: 0,
            wmadd: 0,
            bg_sc: [0; 4],
            bg_character_base: [0; 4],
            bg_scroll: [[0; 2]; 4],
            bg_mode: 0,
            object_select: 0,
            screen_layers: [0; 2],
            window2: [0; 2],
            window_logic: [0; 2],
            window_masks: [0; 2],
            mode7: crate::video::Mode7::default(),
            bg_scroll_latch: 0,
            mode7_latch: 0,
            irq_scanline: 0,
            interrupt_enable: 0,
            timer_irq: false,
            hblank: false,
            multiplicand: 0,
            dividend: 0,
            quotient: 0,
            product: 0,
            dma: [DmaChannel::default(); 8],
            apu_ports: [0; 4],
            apu_in_transfer: false,
            apu_expected: 0,
            apu_jump_echo: None,
            apu_header_pending: false,
            unsupported: UnsupportedAccesses::default(),
            stubbed_writes: 0,
            stubbed_reads: 0,
            pad: 0,
            pinned: Vec::new(),
            unmapped_reads: 0,
            unmapped_writes: 0,
            fetching: false,
            vram_watch: watched_vram_word(),
            dma_source: None,
            access_log: None,
        }
    }

    /// Reports a write of VRAM byte `a` when its word is watched.
    fn report_vram_write(&self, a: usize, value: u8) {
        if self.vram_watch == Some(a / 2) {
            let from = match self.dma_source {
                Some(src) => format!("DMA from ${src:06X}"),
                None => "port write".to_string(),
            };
            eprintln!(
                "vram watch: word ${:04X} {} byte <- ${value:02X} ({from}, VMAIN ${:02X})",
                a / 2,
                if a.is_multiple_of(2) { "low" } else { "high" },
                self.vmain
            );
        }
    }

    /// Where `addr` falls in save RAM, if it is a LoROM save RAM address
    /// and the cartridge has any.
    fn sram_index(&self, addr: u32) -> Option<usize> {
        if self.sram.is_empty() || addr & 0x8000 != 0 {
            return None;
        }
        let bank = (addr >> 16) as u8;
        if !matches!(bank, 0x70..=0x7D | 0xF0..=0xFF) {
            return None;
        }
        let offset = (bank as usize & 0x0F) * 0x8000 + (addr as usize & 0x7FFF);
        Some(offset % self.sram.len())
    }

    fn unsupported_access(&mut self, processor: Processor, address: u32, kind: AccessKind) {
        let kind = match kind {
            AccessKind::Read if self.fetching => AccessKind::Fetch,
            kind => kind,
        };
        match kind {
            AccessKind::Read | AccessKind::Fetch => self.unmapped_reads += 1,
            AccessKind::Write => self.unmapped_writes += 1,
        }
        self.unsupported.record(processor, address, kind);
    }

    /// Whether an S-CPU address in `$2000`-`$7FFF` of a system bank has
    /// nothing behind it on this cartridge: a read sees open bus and a
    /// write goes nowhere. Hacks probe these for expansion hardware.
    fn open_bus(&self, reg: u16) -> bool {
        match reg {
            0x2100..=0x2183 | 0x4016..=0x4017 | 0x4200..=0x421F | 0x4300..=0x437F => false,
            0x2200..=0x23FF => self.ram.sa1.is_none(),
            _ => true,
        }
    }

    /// Reads a 24-bit little-endian pointer from the bus.
    pub fn read_u24(&mut self, addr: u32) -> u32 {
        (0..3).fold(0, |value, i| {
            value | (self.read(addr + i) as u32) << (8 * i)
        })
    }

    /// The object the PPU draws in front of all others, the rest
    /// following in order and wrapping round: object 0, unless the game
    /// turned priority rotation on, which starts from the object `OAMADD`
    /// was left pointing into.
    pub fn first_object(&self) -> usize {
        if self.oam_rotation {
            (self.oam_reload >> 1) as usize & 0x7F
        } else {
            0
        }
    }

    /// The S-CPU's NMI handler in bank 0: the cartridge's native-mode
    /// vector, unless an SA-1 has replaced it.
    pub fn nmi_vector(&mut self) -> u16 {
        let sa1 = self.ram.sa1.as_ref();
        sa1.and_then(|sa1| sa1.main_nmi_vector())
            .unwrap_or_else(|| u16::from_le_bytes([self.read(0xFFEA), self.read(0xFFEB)]))
    }

    /// Sets the timer IRQ flag the game's IRQ handler looks for in
    /// `TIMEUP`. The handler is for the caller to enter: the PPU's
    /// timer does not run here.
    pub fn raise_timer_irq(&mut self) {
        self.timer_irq = true;
    }

    /// Gives the SA-1 a turn: it runs until it waits. False if it got
    /// nowhere, or there is none.
    pub fn run_sa1(&mut self) -> bool {
        let Some(sa1) = self.ram.sa1.as_deref_mut().filter(|sa1| sa1.runnable()) else {
            return false;
        };
        let mut cpu = std::mem::take(&mut sa1.cpu);
        let before = cpu.writes();
        let result = cpu.run(&mut Sa1View(self), SA1_STEP_LIMIT, Cpu::returned);
        let progressed = cpu.writes() != before;
        let sa1 = self.ram.sa1.as_deref_mut().expect("checked above");
        sa1.cpu = cpu;
        sa1.fault = result.err();
        progressed
    }

    /// The error that stopped the SA-1, which is why the S-CPU is about
    /// to report waiting for it.
    pub fn sa1_fault(&self) -> Option<CpuError> {
        let fault = self.ram.sa1.as_ref()?.fault.clone()?;
        Some(CpuError::Sa1(Box::new(fault)))
    }

    fn sa1_register(&mut self, processor: Processor, reg: u16) -> u8 {
        let Some(sa1) = self.ram.sa1.as_ref() else {
            // Without an SA-1 the address is open bus.
            self.stubbed_reads += 1;
            return 0;
        };
        match sa1.read(processor, reg) {
            Some(value) => value,
            None => {
                self.unsupported_access(processor, reg as u32, AccessKind::Read);
                0
            }
        }
    }

    fn set_sa1_register(&mut self, processor: Processor, reg: u16, value: u8) {
        let Some(sa1) = self.ram.sa1.as_deref_mut() else {
            self.stubbed_writes += 1;
            return;
        };
        let supported = sa1.write(processor, reg, value);
        let transfer = sa1.take_dma();
        if !supported {
            self.unsupported_access(processor, reg as u32, AccessKind::Write);
        }
        // The SA-1's DMA, which takes no time here.
        if let Some(transfer) = transfer {
            let mut sa1 = Sa1View(self);
            for i in 0..transfer.len as u32 {
                let value = sa1.read(transfer.source + i);
                sa1.write(transfer.dest + i, value);
            }
        }
    }

    fn read_register(&mut self, reg: u16) -> u8 {
        match reg {
            0x2200..=0x23FF => self.sa1_register(Processor::Main, reg),
            0x2140..=0x2143 => {
                let i = (reg & 3) as usize;
                if self.apu_in_transfer || i >= 2 {
                    self.apu_ports[i]
                } else if i == 0 && self.apu_jump_echo.is_some() {
                    self.apu_jump_echo.take().unwrap()
                } else {
                    [0xAA, 0xBB][i]
                }
            }
            0x2139 => {
                let value = self.vram_latch as u8;
                if self.vmain & 0x80 == 0 {
                    self.vram_read_ahead();
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
                value
            }
            0x213A => {
                let value = (self.vram_latch >> 8) as u8;
                if self.vmain & 0x80 != 0 {
                    self.vram_read_ahead();
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
                value
            }
            0x2180 => {
                let value = self.ram.peek(0x7E_0000 + self.wmadd);
                self.wmadd = (self.wmadd + 1) & 0x1_FFFF;
                value
            }
            0x4218 => self.pad as u8,
            0x4219 => (self.pad >> 8) as u8,
            // Captures enter NMI explicitly and provide no other input.
            0x4016..=0x4017 | 0x4210 | 0x421A..=0x421F => {
                self.stubbed_reads += 1;
                0
            }
            0x4211 => (std::mem::take(&mut self.timer_irq) as u8) << 7,
            // The DMA channel registers read back what was written, or
            // what a transfer left: the size counts down to zero and the
            // source address ends past the last byte, which code that
            // continues a transfer where the last one stopped reads.
            0x4300..=0x437F => {
                let ch = &self.dma[((reg >> 4) & 7) as usize];
                match reg & 0x0F {
                    0x0 => ch.control,
                    0x1 => ch.dest,
                    0x2 => ch.src as u8,
                    0x3 => (ch.src >> 8) as u8,
                    0x4 => (ch.src >> 16) as u8,
                    0x5 => ch.size as u8,
                    0x6 => (ch.size >> 8) as u8,
                    // HDMA state, which is not run here.
                    _ => {
                        self.stubbed_reads += 1;
                        0
                    }
                }
            }
            0x4214 => self.quotient as u8,
            0x4215 => (self.quotient >> 8) as u8,
            0x4216 => self.product as u8,
            0x4217 => (self.product >> 8) as u8,
            0x4212 => {
                // Let the ROM's wait-for-HBlank handshake finish.
                // This is a headless loader, not a cycle-timed PPU.
                let value = if self.hblank { 0x40 } else { 0 };
                self.hblank = !self.hblank;
                value
            }
            _ if self.open_bus(reg) => {
                self.stubbed_reads += 1;
                0
            }
            _ => {
                self.unsupported_access(Processor::Main, reg as u32, AccessKind::Read);
                0
            }
        }
    }

    /// Where in the ROM image a bus address reads from. An SA-1 game can
    /// move the ROM about under both processors.
    fn rom_offset(&self, addr: u32) -> Option<crate::addr::PcAddr> {
        let addr = crate::addr::SnesAddr::new(addr);
        match self.ram.sa1.as_ref().and_then(|sa1| sa1.super_mmc) {
            Some(mmc) => mmc.snes_to_pc(addr),
            None => self.rom.mapping().snes_to_pc(addr).ok(),
        }
    }

    /// The addresses a ROM routine may be running at: its own, and the
    /// same bank from `$80` up where the cartridge has the same ROM
    /// there. A FastROM patch runs the whole game from those banks, so
    /// the game loop it reaches is `$80806B`.
    pub fn code_mirrors(&self, addr: u32) -> Vec<u32> {
        let mirror = addr ^ 0x80_0000;
        let same =
            self.rom_offset(addr).is_some() && self.rom_offset(addr) == self.rom_offset(mirror);
        if same { vec![addr, mirror] } else { vec![addr] }
    }

    fn rom_read(&mut self, processor: Processor, addr: u32) -> u8 {
        let pc = self.rom_offset(addr);
        let byte = pc.and_then(|pc| self.rom.data().get(pc.as_usize()).copied());
        byte.unwrap_or_else(|| {
            self.unsupported_access(processor, addr, AccessKind::Read);
            0
        })
    }

    fn vram_step(&self) -> u16 {
        match self.vmain & 0x03 {
            0 => 1,
            1 => 32,
            _ => 128,
        }
    }

    /// Fills the read latch from the word at the VRAM address.
    fn vram_read_ahead(&mut self) {
        let a = (self.vram_remap(self.vmadd) as usize * 2) % VRAM_LEN;
        self.vram_latch = u16::from_le_bytes([self.vram[a], self.vram[(a + 1) % VRAM_LEN]]);
    }

    /// Applies the VRAM address remapping selected by VMAIN bits 2-3.
    fn vram_remap(&self, addr: u16) -> u16 {
        match (self.vmain >> 2) & 0x03 {
            0 => addr,
            1 => (addr & 0xFF00) | ((addr & 0x001F) << 3) | ((addr & 0x00E0) >> 5),
            2 => (addr & 0xFE00) | ((addr & 0x003F) << 3) | ((addr & 0x01C0) >> 6),
            _ => (addr & 0xFC00) | ((addr & 0x007F) << 3) | ((addr & 0x0380) >> 7),
        }
    }

    fn write_register(&mut self, reg: u16, value: u8) {
        match reg {
            0x2200..=0x23FF => self.set_sa1_register(Processor::Main, reg, value),
            0x2101 => self.object_select = value,
            0x2102 => {
                self.oam_reload = (self.oam_reload & 0x100) | value as u16;
                self.oam_address = self.oam_reload << 1;
            }
            0x2103 => {
                self.oam_reload = (self.oam_reload & 0xFF) | ((value & 1) as u16) << 8;
                self.oam_rotation = value & 0x80 != 0;
                self.oam_address = self.oam_reload << 1;
            }
            0x2104 => {
                let at = self.oam_address as usize;
                if at >= 0x200 {
                    self.oam[0x200 + (at & 0x1F)] = value;
                } else if at & 1 == 0 {
                    self.oam_latch = value;
                } else {
                    self.oam[at - 1] = self.oam_latch;
                    self.oam[at] = value;
                }
                self.oam_address = (self.oam_address + 1) & 0x3FF;
            }
            0x2105 => self.bg_mode = value,
            0x2107..=0x210A => self.bg_sc[(reg - 0x2107) as usize] = value,
            0x210B..=0x210C => {
                let layer = (reg - 0x210B) as usize * 2;
                self.bg_character_base[layer] = ((value & 0x0F) as u16) << 13;
                self.bg_character_base[layer + 1] = ((value >> 4) as u16) << 13;
            }
            0x210D..=0x2114 => {
                let layer = (reg - 0x210D) as usize / 2;
                let axis = (reg - 0x210D) as usize % 2;
                let low = if axis == 0 {
                    (self.bg_scroll_latch & 0xF8) | ((self.bg_scroll[layer][axis] >> 8) as u8 & 7)
                } else {
                    self.bg_scroll_latch
                };
                self.bg_scroll[layer][axis] = u16::from_le_bytes([low, value]);
                self.bg_scroll_latch = value;
                if layer == 0 {
                    self.mode7.scroll[axis] = u16::from_le_bytes([self.mode7_latch, value]);
                    self.mode7_latch = value;
                }
            }
            0x211A => self.mode7.control = value,
            0x211B..=0x2120 => {
                let word = u16::from_le_bytes([self.mode7_latch, value]);
                if reg <= 0x211E {
                    self.mode7.matrix[(reg - 0x211B) as usize] = word as i16;
                } else {
                    self.mode7.center[(reg - 0x211F) as usize] = word;
                }
                self.mode7_latch = value;
            }
            0x2115 => self.vmain = value,
            0x2116 => {
                self.vmadd = (self.vmadd & 0xFF00) | value as u16;
                self.vram_read_ahead();
            }
            0x2117 => {
                self.vmadd = (self.vmadd & 0x00FF) | ((value as u16) << 8);
                self.vram_read_ahead();
            }
            0x2118 => {
                let a = (self.vram_remap(self.vmadd) as usize * 2) % VRAM_LEN;
                self.report_vram_write(a, value);
                self.vram[a] = value;
                self.vram_written[a] = true;
                if self.vmain & 0x80 == 0 {
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
            }
            0x2119 => {
                let a = (self.vram_remap(self.vmadd) as usize * 2 + 1) % VRAM_LEN;
                self.report_vram_write(a, value);
                self.vram[a] = value;
                self.vram_written[a] = true;
                if self.vmain & 0x80 != 0 {
                    self.vmadd = self.vmadd.wrapping_add(self.vram_step());
                }
            }
            0x2128..=0x2129 => self.window2[(reg - 0x2128) as usize] = value,
            0x212A..=0x212B => self.window_logic[(reg - 0x212A) as usize] = value,
            0x212C..=0x212D => self.screen_layers[(reg - 0x212C) as usize] = value,
            0x212E..=0x212F => self.window_masks[(reg - 0x212E) as usize] = value,
            0x2180 => {
                self.ram.poke(0x7E_0000 + self.wmadd, value);
                self.wmadd = (self.wmadd + 1) & 0x1_FFFF;
            }
            0x2181 => self.wmadd = (self.wmadd & 0x1_FF00) | value as u32,
            0x2182 => self.wmadd = (self.wmadd & 0x1_00FF) | (value as u32) << 8,
            0x2183 => self.wmadd = (self.wmadd & 0x0_FFFF) | ((value & 1) as u32) << 16,
            0x2121 => self.cgadd = value as u16 * 2,
            0x2122 => {
                self.cgram[self.cgadd as usize % CGRAM_LEN] = value;
                self.cgadd = self.cgadd.wrapping_add(1);
            }
            0x2140..=0x2143 => {
                let port = (reg & 3) as usize;
                self.apu_ports[port] = value;
                if port == 0 {
                    self.apu_header_pending = false;
                    if !self.apu_in_transfer {
                        if value == 0xCC && self.apu_ports[1] != 0 {
                            self.apu_in_transfer = true;
                            self.apu_expected = 0;
                        }
                    } else if value == self.apu_expected {
                        self.apu_expected = self.apu_expected.wrapping_add(1);
                    } else if self.apu_ports[1] == 0 {
                        self.apu_in_transfer = false;
                        self.apu_jump_echo = Some(value);
                    } else {
                        self.apu_expected = 0;
                        self.apu_header_pending = true;
                    }
                } else if port == 1 && value == 0 && self.apu_in_transfer && self.apu_header_pending
                {
                    self.apu_in_transfer = false;
                    self.apu_header_pending = false;
                    self.apu_jump_echo = Some(self.apu_ports[0]);
                }
            }
            0x420B => self.run_dma(value),
            0x4200 => self.interrupt_enable = value,
            0x4202 => self.multiplicand = value,
            0x4203 => self.product = self.multiplicand as u16 * value as u16,
            0x4204 => self.dividend = (self.dividend & 0xFF00) | value as u16,
            0x4205 => self.dividend = (self.dividend & 0x00FF) | ((value as u16) << 8),
            0x4206 => {
                // Division by zero yields $FFFF with the dividend as remainder.
                (self.quotient, self.product) = if value == 0 {
                    (0xFFFF, self.dividend)
                } else {
                    (self.dividend / value as u16, self.dividend % value as u16)
                };
            }
            0x4209 => self.irq_scanline = (self.irq_scanline & 0x100) | value as u16,
            0x420A => self.irq_scanline = (self.irq_scanline & 0xFF) | (((value & 1) as u16) << 8),
            0x4300..=0x437F => {
                let ch = &mut self.dma[((reg >> 4) & 7) as usize];
                match reg & 0x0F {
                    0x0 => ch.control = value,
                    0x1 => ch.dest = value,
                    0x2 => ch.src = (ch.src & 0xFF_FF00) | value as u32,
                    0x3 => ch.src = (ch.src & 0xFF_00FF) | ((value as u32) << 8),
                    0x4 => ch.src = (ch.src & 0x00_FFFF) | ((value as u32) << 16),
                    0x5 => ch.size = (ch.size & 0xFF00) | value as u16,
                    0x6 => ch.size = (ch.size & 0x00FF) | ((value as u16) << 8),
                    _ => {}
                }
            }
            // Brightness, mirrors consumed by the capture passes, CPU I/O,
            // and disabled HDMA have no further effect in this model.
            0x2100 | 0x2123..=0x2127 | 0x2130..=0x2132 | 0x4201 | 0x4207..=0x4208 | 0x420D => {
                self.stubbed_writes += 1
            }
            0x2106 | 0x2133 | 0x420C if value == 0 => self.stubbed_writes += 1,
            // Writes to read-only registers, the controller strobe, and
            // open bus go nowhere.
            0x2134..=0x213F | 0x4016 | 0x4210..=0x421F => self.stubbed_writes += 1,
            _ if self.open_bus(reg) => self.stubbed_writes += 1,
            _ => self.unsupported_access(Processor::Main, reg as u32, AccessKind::Write),
        }
    }

    /// Executes the general-purpose DMA channels enabled in `mask`,
    /// between an A bus address and the B bus registers, in the
    /// direction the channel's control byte gives.
    fn run_dma(&mut self, mask: u8) {
        for ch in 0..8 {
            if mask & (1 << ch) == 0 {
                continue;
            }
            let c = self.dma[ch];
            let regs: [u16; 4] = {
                let base = 0x2100 | c.dest as u16;
                match c.control & 0x07 {
                    0 => [base; 4],
                    1 => [base, base + 1, base, base + 1],
                    2 | 6 => [base; 4],
                    3 | 7 => [base, base, base + 1, base + 1],
                    4 => [base, base + 1, base + 2, base + 3],
                    _ => [base, base + 1, base, base + 1],
                }
            };
            let fixed = c.control & 0x08 != 0;
            let step: i32 = if c.control & 0x10 != 0 { -1 } else { 1 };
            let mut src = c.src;
            let mut count = if c.size == 0 { 0x1_0000 } else { c.size as u32 };
            let mut i = 0;
            let to_a_bus = c.control & 0x80 != 0;
            while count > 0 {
                if to_a_bus {
                    let value = self.read_register(regs[i & 3]);
                    self.write(src, value);
                } else {
                    let value = self.read(src);
                    self.dma_source = Some(src);
                    self.write_register(regs[i & 3], value);
                    self.dma_source = None;
                }
                if !fixed {
                    let off = ((src as u16) as i32 + step) as u16;
                    src = (src & 0xFF_0000) | off as u32;
                }
                i += 1;
                count -= 1;
            }
            self.dma[ch].src = src;
            self.dma[ch].size = 0;
        }
    }
}

impl Bus for SmwBus<'_> {
    fn before_instruction(&mut self) -> Result<(), CpuError> {
        if let Some(operation) = &self.operation {
            operation.instruction()?;
        }
        Ok(())
    }

    fn fetch(&mut self, addr: u32) -> u8 {
        self.fetching = true;
        let value = self.read(addr);
        self.fetching = false;
        value
    }

    fn read(&mut self, addr: u32) -> u8 {
        if !self.fetching
            && let Some(log) = &mut self.access_log
        {
            log.read(addr);
        }
        // Most reads are instruction fetches from the upper half of a
        // bank, which is ROM in every bank that is not RAM on some
        // cartridge.
        if addr & 0x8000 != 0 && !(0x40..=0x7F).contains(&(addr >> 16)) {
            return self.rom_read(Processor::Main, addr);
        }
        if (0x40..=0x4F).contains(&(addr >> 16))
            && let Some(conversion) = self.ram.sa1.as_ref().and_then(|sa1| sa1.conversion())
        {
            // The S-CPU's DMA, reading characters out of a bitmap.
            return conversion.read(addr, |at| self.ram.read(at).unwrap_or(0));
        }
        // Before work RAM: the low half of a save RAM bank is the save
        // RAM, not the work RAM mirror the system banks have there.
        if let Some(i) = self.sram_index(addr) {
            return self.sram[i];
        }
        if let Some(value) = self.ram.read(addr) {
            return value;
        }
        match ((addr >> 16) as u8, addr as u16) {
            (0x00..=0x3F | 0x80..=0xBF, reg @ 0x2000..=0x7FFF) => self.read_register(reg),
            _ => self.rom_read(Processor::Main, addr),
        }
    }

    fn write(&mut self, addr: u32, value: u8) {
        if let Some(log) = &mut self.access_log {
            log.write(addr);
        }
        if let Some(i) = self.sram_index(addr) {
            self.sram[i] = value;
            return;
        }
        if self.ram.write(addr, value) {
            if !self.pinned.is_empty() {
                self.repin(addr);
            }
            return;
        }
        match ((addr >> 16) as u8, addr as u16) {
            (0x00..=0x3F | 0x80..=0xBF, reg @ 0x2000..=0x7FFF) => self.write_register(reg, value),
            _ => self.unsupported_access(Processor::Main, addr, AccessKind::Write),
        }
    }

    fn irq(&mut self) -> bool {
        let sa1 = self.ram.sa1.as_ref();
        sa1.is_some_and(|sa1| sa1.irq(Processor::Main))
    }

    fn irq_vector(&mut self, emulation: bool) -> u16 {
        let sa1 = self.ram.sa1.as_ref();
        sa1.and_then(|sa1| sa1.main_irq_vector())
            .unwrap_or_else(|| {
                let addr = if emulation { 0xFFFE } else { 0xFFEE };
                u16::from_le_bytes([self.read(addr), self.read(addr + 1)])
            })
    }

    fn wait(&mut self) -> bool {
        self.run_sa1()
    }

    fn vblank(&mut self) -> Option<u16> {
        (self.interrupt_enable & 0x80 != 0).then(|| self.nmi_vector())
    }
}

/// The bus as the SA-1 sees it: the cartridge and its own registers, and
/// nothing of the console (no work RAM, no PPU or CPU registers).
pub struct Sa1View<'a, 'r>(pub &'a mut SmwBus<'r>);

impl Bus for Sa1View<'_, '_> {
    fn before_instruction(&mut self) -> Result<(), CpuError> {
        self.0.before_instruction()
    }

    fn fetch(&mut self, addr: u32) -> u8 {
        self.0.fetching = true;
        let value = self.read(addr);
        self.0.fetching = false;
        value
    }

    fn read(&mut self, addr: u32) -> u8 {
        let bus = &mut *self.0;
        if addr & 0x8000 != 0 && !(0x40..=0x7F).contains(&(addr >> 16)) {
            return bus.rom_read(Processor::Sa1, addr);
        }
        if let Some(value) = bus.ram.read_sa1(addr) {
            return value;
        }
        match ((addr >> 16) as u8, addr as u16) {
            (0x00..=0x3F | 0x80..=0xBF, reg @ 0x2200..=0x23FF) => {
                bus.sa1_register(Processor::Sa1, reg)
            }
            (0xC0..=0xFF, _) => bus.rom_read(Processor::Sa1, addr),
            _ => {
                bus.unsupported_access(Processor::Sa1, addr, AccessKind::Read);
                0
            }
        }
    }

    fn write(&mut self, addr: u32, value: u8) {
        let bus = &mut *self.0;
        if bus.ram.write_sa1(addr, value) {
            return;
        }
        match ((addr >> 16) as u8, addr as u16) {
            (0x00..=0x3F | 0x80..=0xBF, reg @ 0x2200..=0x23FF) => {
                bus.set_sa1_register(Processor::Sa1, reg, value)
            }
            _ => bus.unsupported_access(Processor::Sa1, addr, AccessKind::Write),
        }
    }

    fn irq(&mut self) -> bool {
        let sa1 = self.0.ram.sa1.as_ref();
        sa1.is_some_and(|sa1| sa1.irq(Processor::Sa1))
    }

    fn irq_vector(&mut self, _emulation: bool) -> u16 {
        let sa1 = self.0.ram.sa1.as_ref();
        sa1.map_or(0, |sa1| sa1.irq_vector())
    }

    // `wait` stays false: a waiting SA-1 ends its turn.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::Rom;

    #[test]
    fn controller_1_reads_what_a_pass_presses() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        assert_eq!((bus.read(0x4218), bus.read(0x4219)), (0, 0));
        bus.pad = 0x1080;
        assert_eq!((bus.read(0x4218), bus.read(0x4219)), (0x80, 0x10));
    }

    #[test]
    fn stubs_and_unsupported_accesses_are_distinct() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        // Controller 2, which no pass presses.
        bus.read(0x421A);
        bus.write(0x420C, 0);
        // Open bus on a LoROM cartridge, a write to a read-only register.
        bus.read(0x3800);
        bus.read(0x2300);
        bus.write(0x7808, 1);
        bus.write(0x4216, 1);
        assert!(bus.unsupported.is_empty());
        assert_eq!((bus.stubbed_reads, bus.stubbed_writes), (3, 3));
        bus.write(0x420C, 1); // enabled HDMA is not simulated
        bus.read(0x2137); // unsupported latch read
        Sa1View(&mut bus).read(0x2137); // different processor, same address
        assert_eq!(bus.unsupported.accesses.len(), 3);
        assert_eq!(bus.unsupported.accesses[2].processor, Processor::Sa1);
    }

    #[test]
    fn both_cpus_and_restored_snapshots_share_the_operation_budget() {
        use crate::operation::{Operation, OperationError};
        let mut bytes = vec![0xEA; 0x8000]; // NOP
        bytes[0x7FD5] = 0x23;
        let rom = Rom::from_bytes(bytes).unwrap();
        let mut bus = SmwBus::new(&rom);
        let operation = Operation::new(Some(2));
        bus.operation = Some(operation.clone());
        let snapshot = bus.ram.clone();
        let mut main = Cpu::new();
        main.pc = 0x8000;
        main.step(&mut bus).unwrap();
        let mut second = Cpu::new();
        second.pc = 0x8000;
        second.step(&mut Sa1View(&mut bus)).unwrap();
        bus.ram = snapshot;
        assert_eq!(
            main.step(&mut bus),
            Err(CpuError::Operation(OperationError::Budget { limit: 2 }))
        );
        assert_eq!(operation.progress().instructions, 2);
    }

    #[test]
    fn cancellation_interrupts_a_running_cpu() {
        use crate::operation::{Operation, OperationError};
        let operation = Operation::default();
        let worker_operation = operation.clone();
        let worker = std::thread::spawn(move || {
            let mut bytes = vec![0; 0x8000];
            bytes[0x7FD5] = 0x20;
            bytes[..3].copy_from_slice(&[0x1A, 0x80, 0xFD]); // INC A; BRA back
            let rom = Rom::from_bytes(bytes).unwrap();
            let mut bus = SmwBus::new(&rom);
            bus.operation = Some(worker_operation);
            let mut cpu = Cpu::new();
            cpu.pc = 0x8000;
            cpu.run(&mut bus, u64::MAX, |_| false)
        });
        while operation.progress().instructions < 1000 && !worker.is_finished() {
            std::thread::yield_now();
        }
        operation.cancel();
        assert_eq!(
            worker.join().unwrap(),
            Err(CpuError::Operation(OperationError::Cancelled))
        );
    }

    fn rom() -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x7FC0 + 0x15] = 0x20;
        for (i, b) in data.iter_mut().enumerate().take(0x100) {
            *b = i as u8;
        }
        Rom::from_bytes(data).unwrap()
    }

    #[test]
    fn a_rom_lunar_magic_saved_turns_the_instruction_trace_off() {
        let mut data = vec![0u8; 0x8_0000];
        data[0x7FC0 + 0x15] = 0x20;
        let marker = b"Lunar Magic Version 3.70";
        data[0x7_F0A0..0x7_F0A0 + marker.len()].copy_from_slice(marker);
        let rom = Rom::from_bytes(data).unwrap();
        assert!(rom.lunar_magic_version().is_some());
        let _bus = SmwBus::new(&rom);
        let mut cpu = Cpu::new();
        cpu.keep_history(8);
        assert!(!cpu.keeps_history());
    }

    #[test]
    fn video_registers_keep_separate_shared_latches() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.write(0x210B, 0x73);
        assert_eq!(&bus.bg_character_base[..2], &[0x6000, 0xE000]);
        bus.write(0x210D, 0xAB);
        bus.write(0x210D, 0x01);
        assert_eq!(bus.bg_scroll[0][0], 0x01AB);
        assert_eq!(bus.mode7.scroll[0], 0x01AB);
        bus.write(0x211B, 0xFE);
        bus.write(0x211C, 0xFF); // Matrix registers share a latch.
        assert_eq!(bus.mode7.matrix[1], -2);
        bus.write(0x210E, 0x02);
        assert_eq!(bus.bg_scroll[0][1], 0x0201);
        assert_eq!(bus.mode7.scroll[1], 0x02FF);
        bus.write(0x212E, 0x11);
        bus.write(0x212F, 0x02);
        bus.write(0x2129, 0x80);
        bus.write(0x212B, 0x04);
        assert_eq!(bus.window_masks, [0x11, 0x02]);
        assert_eq!((bus.window2, bus.window_logic), ([0, 0x80], [0, 0x04]));
        bus.write(0x4209, 0xAE);
        bus.write(0x420A, 0xFF);
        assert_eq!(bus.irq_scanline, 0x1AE);
        assert_eq!(
            [bus.read(0x4212), bus.read(0x4212), bus.read(0x4212)],
            [0, 0x40, 0]
        );
    }

    /// A 32 KiB SA-1 cartridge with `code` placed at bank 0 addresses.
    fn sa1_rom(code: &[(u16, &[u8])]) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x7FC0 + 0x15] = 0x23;
        for (addr, bytes) in code {
            let at = (addr - 0x8000) as usize;
            data[at..at + bytes.len()].copy_from_slice(bytes);
        }
        Rom::from_bytes(data).unwrap()
    }

    /// Points the SA-1's reset vector at `$8100` and its IRQ vector at
    /// `$8140`, lets the SA-1 interrupt the S-CPU, and releases it.
    const START_SA1: &[u8] = &[
        0xA9, 0x00, 0x8D, 0x03, 0x22, // LDA #$00 : STA $2203
        0xA9, 0x81, 0x8D, 0x04, 0x22, // LDA #$81 : STA $2204
        0xA9, 0x40, 0x8D, 0x07, 0x22, // LDA #$40 : STA $2207
        0xA9, 0x81, 0x8D, 0x08, 0x22, // LDA #$81 : STA $2208
        0xA9, 0x80, 0x8D, 0x01, 0x22, // LDA #$80 : STA $2201
        0x9C, 0x00, 0x22, // STZ $2200
    ];

    #[test]
    fn the_processors_hand_work_to_each_other_and_wait() {
        let mut main = START_SA1.to_vec();
        main.extend([
            0x58, // CLI
            0xA9, 0x85, 0x8D, 0x00, 0x22, // LDA #$85 : STA $2200 (IRQ, message 5)
            0xAD, 0x00, 0x30, 0xF0, 0xFB, // - LDA $3000 : BEQ -
            0x6B, // RTL
        ]);
        let rom = sa1_rom(&[
            (0x8000, &main),
            (
                // SA-1 reset: native mode, IRQs from the S-CPU on, idle.
                0x8100,
                &[
                    0x18, 0xFB, // CLC : XCE
                    0xA9, 0x80, 0x8D, 0x0A, 0x22, // LDA #$80 : STA $220A
                    0x58, // CLI
                    0xA5, 0x10, 0xF0, 0xFC, // - LDA $10 : BEQ -
                    0xDB, // STP
                ],
            ),
            (
                // SA-1 IRQ: keeps the message, then has the S-CPU (vector
                // `$8180`, message 7) do its part before finishing.
                0x8140,
                &[
                    0xAD, 0x01, 0x23, 0x8D, 0x01, 0x30, // LDA $2301 : STA $3001
                    0xA9, 0x80, 0x8D, 0x0B, 0x22, // LDA #$80 : STA $220B
                    0xA9, 0x80, 0x8D, 0x0E, 0x22, // LDA #$80 : STA $220E
                    0xA9, 0x81, 0x8D, 0x0F, 0x22, // LDA #$81 : STA $220F
                    0xA9, 0xC7, 0x8D, 0x09, 0x22, // LDA #$C7 : STA $2209
                    0xAD, 0x02, 0x30, 0xF0, 0xFB, // - LDA $3002 : BEQ -
                    0xA9, 0x01, 0x8D, 0x00, 0x30, // LDA #$01 : STA $3000
                    0x40, // RTI
                ],
            ),
            (
                // S-CPU IRQ, reached through the vector the SA-1 supplied.
                0x8180,
                &[
                    0xAD, 0x00, 0x23, 0x8D, 0x03, 0x30, // LDA $2300 : STA $3003
                    0xA9, 0x80, 0x8D, 0x02, 0x22, // LDA #$80 : STA $2202
                    0xA9, 0x01, 0x8D, 0x02, 0x30, // LDA #$01 : STA $3002
                    0x40, // RTI
                ],
            ),
        ]);
        let mut bus = SmwBus::new(&rom);
        let mut cpu = Cpu::new();
        cpu.call(&mut bus, 0x00_8000, 10_000).unwrap();
        // Each side saw the other's message with the IRQ flag, and the
        // S-CPU only got past its wait once the SA-1 had finished.
        assert_eq!(bus.ram.peek(0x00_3001), 0x85);
        assert_eq!(bus.ram.peek(0x00_3003), 0xC7);
        assert_eq!(bus.ram.peek(0x00_3000), 1);
        assert!(!bus.irq());
        assert_eq!(bus.sa1_fault(), None);
        // The SA-1 is back in its idle loop, which a turn gets nowhere in.
        assert!(!bus.run_sa1());
    }

    #[test]
    fn an_sa1_that_stops_is_why_the_wait_never_ends() {
        let mut main = START_SA1.to_vec();
        main.extend([0xAD, 0x00, 0x30, 0xF0, 0xFB, 0x6B]); // - LDA $3000 : BEQ - : RTL
        let rom = sa1_rom(&[(0x8000, &main), (0x8100, &[0xDB])]); // STP
        let mut bus = SmwBus::new(&rom);
        let mut cpu = Cpu::new();
        let error = cpu.call(&mut bus, 0x00_8000, 10_000).unwrap_err();
        assert!(
            matches!(error, CpuError::Waiting { at } if at.bank() == 0),
            "{error}"
        );
        let halted = CpuError::Halted {
            at: crate::clean_room::CodeAddr::new(0, 0x8100),
        };
        assert_eq!(bus.sa1_fault(), Some(CpuError::Sa1(Box::new(halted))));
    }

    #[test]
    fn the_sa1_sees_the_cartridge_but_not_the_console() {
        let rom = sa1_rom(&[(0x8000, &[0x12, 0x34])]);
        let mut bus = SmwBus::new(&rom);
        bus.write(0x7E_0010, 0xAA);
        bus.write(0x00_3010, 0xBB);
        bus.write(0x40_0123, 0xCC);
        assert_eq!(bus.read(0x00_0010), 0xAA); // work RAM
        let mut sa1 = Sa1View(&mut bus);
        assert_eq!(sa1.read(0x00_0010), 0xBB); // I-RAM, mirrored down
        assert_eq!(sa1.read(0x00_6123), 0xCC); // BW-RAM through the window
        assert_eq!(sa1.read(0x00_8001), 0x34);
        assert_eq!(sa1.read(0xC0_0001), 0x34); // the same ROM, whole banks
        assert_eq!(sa1.read(0x7E_0010), 0); // no work RAM out here
        sa1.write(0x00_2118, 0x55); // nor a PPU
        assert_eq!(bus.vram[0], 0);
        assert_eq!(bus.unmapped_writes, 1);
    }

    #[test]
    fn the_work_ram_port_fills_work_ram() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        for (reg, value) in [(0x2181, 0xFE), (0x2182, 0xFF), (0x2183, 0x01)] {
            bus.write(reg, value);
        }
        // Four bytes from ROM `$008010` by DMA, across the 17-bit wrap.
        for (reg, value) in [
            (0x4300, 0x00),
            (0x4301, 0x80),
            (0x4302, 0x10),
            (0x4303, 0x80),
            (0x4304, 0x00),
            (0x4305, 0x04),
            (0x4306, 0x00),
            (0x420B, 0x01),
        ] {
            bus.write(reg, value);
        }
        assert_eq!(bus.ram.peek(0x7F_FFFE), 0x10);
        assert_eq!(bus.ram.peek(0x7F_FFFF), 0x11);
        assert_eq!(bus.ram.peek(0x7E_0000), 0x12);
        assert_eq!(bus.ram.peek(0x7E_0001), 0x13);
        bus.write(0x2181, 0x00);
        bus.write(0x2182, 0x00);
        bus.write(0x2183, 0x00);
        assert_eq!([bus.read(0x2180), bus.read(0x2180)], [0x12, 0x13]);
    }

    #[test]
    fn the_oam_port_takes_words_and_remembers_where_to_start() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        for (i, value) in (0..OAM_LEN as u32).zip((1..=0xFFu8).cycle()) {
            bus.ram.poke(0x7E_0200 + i, value);
        }
        // The game's upload: all of `$0200`-`$041F` by DMA from address 0.
        for (reg, value) in [
            (0x2102, 0x00),
            (0x2103, 0x00),
            (0x4300, 0x00),
            (0x4301, 0x04),
            (0x4302, 0x00),
            (0x4303, 0x02),
            (0x4304, 0x00),
            (0x4305, 0x20),
            (0x4306, 0x02),
            (0x420B, 0x01),
        ] {
            bus.write(reg, value);
        }
        assert_eq!(bus.oam, bus.ram.bytes(crate::ram::OAM, OAM_LEN));
        assert_eq!(bus.first_object(), 0);
        bus.write(0x2103, 0x80);
        bus.write(0x2102, 200);
        assert_eq!(bus.first_object(), 100);
        // A low byte waits for its high byte; the last 32 bytes do not.
        bus.write(0x2104, 0xAA);
        assert_eq!(bus.oam[400], bus.ram.peek(0x7E_0200 + 400));
        bus.write(0x2104, 0xBB);
        assert_eq!(&bus.oam[400..402], &[0xAA, 0xBB]);
        bus.write(0x2102, 0x00);
        bus.write(0x2103, 0x01);
        bus.write(0x2104, 0xCC);
        assert_eq!(bus.oam[0x200], 0xCC);
        assert_eq!(bus.first_object(), 0);
    }

    #[test]
    fn vram_port_writes_words() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.write(0x002115, 0x80); // increment after high byte, step 1 word
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x10); // word address $1000
        bus.write(0x002118, 0xAA);
        bus.write(0x002119, 0xBB);
        bus.write(0x002118, 0xCC);
        bus.write(0x002119, 0xDD);
        assert_eq!(&bus.vram[0x2000..0x2004], &[0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn a_routine_is_also_at_its_fastrom_mirror() {
        let rom = rom();
        let bus = SmwBus::new(&rom);
        assert_eq!(bus.code_mirrors(0x00_806B), [0x00_806B, 0x80_806B]);
        assert_eq!(bus.code_mirrors(0x80_806B), [0x80_806B, 0x00_806B]);
        // Work RAM is nowhere else.
        assert_eq!(bus.code_mirrors(0x7E_2000), [0x7E_2000]);
    }

    #[test]
    fn screen_layers_are_what_was_last_written() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.write(0x00212C, 0x15);
        bus.write(0x00212D, 0x02);
        bus.write(0x91212C, 0x17);
        assert_eq!(bus.screen_layers, [0x17, 0x02]);
    }

    /// Reading VRAM back as the hardware allows: set the address, read one
    /// word to move past the read-ahead, then DMA from `$2139`-`$213A`.
    #[test]
    fn vram_reads_back_through_the_read_ahead_latch() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        bus.vram[0x2000..0x2006].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        bus.write(0x002115, 0x80); // step after the high byte
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x10); // word address $1000
        // The latch was filled when the address was set, and again, from
        // the same word, before the address stepped.
        assert_eq!([bus.read(0x002139), bus.read(0x00213A)], [1, 2]);
        assert_eq!([bus.read(0x002139), bus.read(0x00213A)], [1, 2]);
        assert_eq!([bus.read(0x002139), bus.read(0x00213A)], [3, 4]);

        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x10);
        bus.read(0x002139);
        bus.read(0x00213A);
        for (reg, value) in [
            (0x4320, 0x81), // B to A, two registers alternating
            (0x4321, 0x39),
            (0x4322, 0x00),
            (0x4323, 0xBD),
            (0x4324, 0x7E),
            (0x4325, 0x06),
            (0x4326, 0x00),
            (0x420B, 0x04),
        ] {
            bus.write(reg, value);
        }
        let back: Vec<u8> = (0..6).map(|i| bus.ram.peek(0x7E_BD00 + i)).collect();
        assert_eq!(back, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn dma_registers_read_back_and_a_transfer_leaves_its_end() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        for (reg, value) in [
            (0x4310, 0x00), // A to B, one register
            (0x4311, 0x22), // CGRAM data
            (0x4312, 0x00),
            (0x4313, 0x80),
            (0x4314, 0x00),
            (0x4315, 0x04),
            (0x4316, 0x00),
        ] {
            bus.write(reg, value);
            assert_eq!(bus.read(reg), value);
        }
        bus.write(0x420B, 0x02);
        assert_eq!([bus.read(0x4312), bus.read(0x4313)], [0x04, 0x80]);
        assert_eq!([bus.read(0x4315), bus.read(0x4316)], [0, 0]);
        assert!(bus.unsupported.is_empty());
    }

    #[test]
    fn save_ram_is_mirrored_across_its_banks() {
        let mut data = vec![0u8; 0x8000];
        data[0x7FC0 + 0x15] = 0x20;
        data[0x7FC0 + 0x18] = 0x01; // 2 KiB, as vanilla declares
        let rom = Rom::from_bytes(data).unwrap();
        let mut bus = SmwBus::new(&rom);
        assert_eq!(bus.sram.len(), 0x800);
        bus.write(0x70_210E, 0x6B);
        assert_eq!(bus.read(0x70_210E), 0x6B);
        assert_eq!(bus.read(0x70_010E), 0x6B);
        assert_eq!(bus.read(0x71_010E), 0x6B);
        assert_eq!(bus.read(0xF0_010E), 0x6B);
        assert_eq!(bus.read(0x70_0000), 0);
        assert!(bus.unsupported.is_empty());
    }

    #[test]
    fn dma_to_cgram_and_vram() {
        let rom = rom();
        let mut bus = SmwBus::new(&rom);
        // CGRAM: colour index 4, 8 bytes from ROM $008010 via channel 0, mode 0 to $2122.
        bus.write(0x002121, 0x04);
        bus.write(0x004300, 0x00);
        bus.write(0x004301, 0x22);
        bus.write(0x004302, 0x10);
        bus.write(0x004303, 0x80);
        bus.write(0x004304, 0x00);
        bus.write(0x004305, 0x08);
        bus.write(0x004306, 0x00);
        bus.write(0x00420B, 0x01);
        assert_eq!(
            &bus.cgram[8..16],
            &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]
        );
        // VRAM: mode 1 to $2118/$2119, 6 bytes from WRAM $7E1000 via channel 1.
        for (i, value) in (1..=6).enumerate() {
            bus.ram.poke(0x7E_1000 + i as u32, value);
        }
        bus.write(0x002115, 0x80);
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x20);
        bus.write(0x004310, 0x01);
        bus.write(0x004311, 0x18);
        bus.write(0x004312, 0x00);
        bus.write(0x004313, 0x10);
        bus.write(0x004314, 0x7E);
        bus.write(0x004315, 0x06);
        bus.write(0x004316, 0x00);
        bus.write(0x00420B, 0x02);
        assert_eq!(&bus.vram[0x4000..0x4006], &[1, 2, 3, 4, 5, 6]);
        // Fixed-source fill: 4 bytes of the same value.
        bus.write(0x002116, 0x00);
        bus.write(0x002117, 0x30);
        bus.ram.poke(0x7E_1100, 0x25);
        bus.write(0x004310, 0x09);
        bus.write(0x004312, 0x00);
        bus.write(0x004313, 0x11);
        bus.write(0x004315, 0x04);
        bus.write(0x00420B, 0x02);
        assert_eq!(&bus.vram[0x6000..0x6004], &[0x25; 4]);
    }
}
