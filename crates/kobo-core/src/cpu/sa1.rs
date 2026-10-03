//! The SA-1: a second 65816 on the cartridge, and the registers at
//! `$2200`-`$23FF` the two processors talk through.
//!
//! What is modelled is what a game needs to hand work across: the
//! control registers with their message nibbles and IRQ flags, the
//! vectors, the BW-RAM windows, the arithmetic unit, and DMA between the
//! cartridge's memories, character conversion of the first type (which
//! SA-1 Pack uploads dynamic sprites through), and the Super MMC's bank
//! registers. Timers, the second type of character conversion, the
//! variable-length bit reader, and write protection are not.

use super::{Cpu, CpuError};
use crate::addr::SuperMmc;

/// Which of the two processors is on the bus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Processor {
    /// The console's CPU.
    Main,
    Sa1,
}

/// How banks `$60`-`$6F` slice BW-RAM into cells (`BBF`, `$223F`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Bitmap {
    #[default]
    FourBits,
    TwoBits,
}

impl Bitmap {
    /// BW-RAM byte, shift, and mask of cell `at`.
    pub fn cell(self, at: usize) -> (usize, u32, u8) {
        match self {
            Self::FourBits => (at / 2, (at as u32 & 1) * 4, 0x0F),
            Self::TwoBits => (at / 4, (at as u32 & 3) * 2, 0x03),
        }
    }
}

/// One direction of the link between the processors: a four-bit message
/// and an IRQ the receiver can mask and has to acknowledge.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Link {
    message: u8,
    irq: bool,
    irq_enabled: bool,
}

impl Link {
    fn send(&mut self, control: u8) {
        self.message = control & 0x0F;
        self.irq |= control & 0x80 != 0;
    }

    fn line(self) -> bool {
        self.irq && self.irq_enabled
    }
}

/// A copy the SA-1's DMA has been asked for, in bus addresses as the SA-1
/// sees them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DmaTransfer {
    pub source: u32,
    pub dest: u32,
    pub len: u16,
}

/// Character conversion of the first type (`DCNT` = `$B0`) while it runs:
/// the S-CPU reads BW-RAM and gets, in place of the bitmap that is there,
/// the same picture as the PPU's planar 8x8 characters, which its own DMA
/// can then copy to VRAM. The bitmap is rows of packed pixels, the first
/// pixel in the lowest bits, `1 << width` characters across.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Conversion {
    /// BW-RAM address of the bitmap (`SDA`), where character 0 reads from.
    source: u32,
    /// `CDMA` bits 1-0: 8, 4, or 2 bits to a pixel.
    depth: u8,
    /// `CDMA` bits 4-2.
    width: u8,
}

impl Conversion {
    /// The byte the S-CPU reads at BW-RAM address `addr`, given `bwram`
    /// to read the bitmap with. A character is 64, 32, or 16 bytes, so
    /// the offset from the bitmap's address says which character it is
    /// and which byte of it.
    pub fn read(self, addr: u32, mut bwram: impl FnMut(u32) -> u8) -> u8 {
        let depth = self.depth.min(2) as u32;
        // Bytes in a character's row of the bitmap, and in a whole row.
        let bytes = 8 >> depth;
        let line = (8 << self.width as u32) >> depth;
        let offset = addr.wrapping_sub(self.source) & 0x0F_FFFF;
        let (character, byte) = (offset >> (6 - depth), offset & ((64 >> depth) - 1));
        let (row, column) = (character >> self.width, character & ((1 << self.width) - 1));
        // Planes come in pairs, 16 bytes each: a byte of either per line.
        let (y, plane) = ((byte & 15) >> 1, (byte >> 4) * 2 + (byte & 1));
        let start = self.source + (row * 8 + y) * line + column * bytes;
        let pixels = (0..bytes).fold(0u64, |pixels, i| {
            pixels | (bwram(start + i) as u64) << (8 * i)
        });
        (0..8).fold(0, |out, x| {
            let pixel = pixels >> (x * (8 >> depth));
            out | ((pixel >> plane) as u8 & 1) << (7 - x)
        })
    }
}

/// `DCNT`, `SDA`, `DDA`, and `DTC`. The source is ROM, BW-RAM, or I-RAM
/// and the destination I-RAM or BW-RAM; writing the last byte of the
/// destination address that its memory uses starts the copy.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Dma {
    control: u8,
    /// `CDMA`: the bitmap's depth and width, and bit 7 to end a conversion.
    conversion: u8,
    source: u32,
    dest: u32,
    len: u16,
    irq: bool,
    irq_enabled: bool,
}

impl Dma {
    /// The transfer the registers describe, if `DCNT` enables one this
    /// models: character conversion is not.
    fn transfer(self) -> Option<DmaTransfer> {
        let place = |memory: u8, addr: u32| match memory {
            0 => Some(addr),
            1 => Some(0x40_0000 | (addr & 0x03_FFFF)),
            2 => Some(0x00_3000 | (addr & 0x07FF)),
            _ => None,
        };
        (self.control & 0xA0 == 0x80).then_some(DmaTransfer {
            source: place(self.control & 3, self.source)?,
            dest: place(2 - (self.control >> 2 & 1), self.dest)?,
            len: self.len,
        })
    }
}

impl Dma {
    /// The conversion the registers describe, if `DCNT` enables one of
    /// the first type.
    fn conversion(self) -> Option<Conversion> {
        (self.control & 0xB0 == 0xB0).then_some(Conversion {
            source: 0x40_0000 | (self.source & 0x0F_FFFF),
            depth: self.conversion & 3,
            width: self.conversion >> 2 & 7,
        })
    }
}

fn set_byte(value: &mut u32, byte: u16, to: u8) {
    let shift = 8 * byte as u32;
    *value = (*value & !(0xFF << shift)) | (to as u32) << shift;
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sa1 {
    /// The SA-1's CPU. While it runs it is out on the bus and this is a
    /// placeholder. Not public: where it stopped is where a ROM's code
    /// is (clean room).
    pub(crate) cpu: Cpu,
    /// Held in reset (`CCNT` bit 5), as it is from power-on, or stopped
    /// (`CCNT` bit 6).
    held: bool,
    /// What stopped the CPU for good, if anything has.
    pub(crate) fault: Option<CpuError>,
    to_sa1: Link,
    to_main: Link,
    /// `CRV`, `CNV`, `CIV`: the SA-1 takes its vectors from registers.
    reset_vector: u16,
    irq_vector: u16,
    /// `SIV` and `SNV`, which replace the S-CPU's IRQ and NMI vectors
    /// when `SCNT` bits 6 and 4 are set.
    main_irq_vector: u16,
    main_irq_vector_selected: bool,
    main_nmi_vector: u16,
    main_nmi_vector_selected: bool,
    /// `BMAPS` and `BMAP`: the BW-RAM block at `$6000`-`$7FFF` for the
    /// S-CPU and for the SA-1. Bit 7 of the SA-1's selects the bitmap
    /// view instead.
    pub bwram_window: [u8; 2],
    pub bitmap: Bitmap,
    /// `CXB`-`FXB`, once the game has written any of them. Until then
    /// the ROM is where its [`crate::addr::Mapping`] says, which is where
    /// SA-1 Pack's start-up code is about to put it.
    pub super_mmc: Option<SuperMmc>,
    arithmetic: Arithmetic,
    dma: Dma,
    /// A transfer the registers have started, for the bus to carry out.
    dma_started: Option<DmaTransfer>,
    /// The character conversion in progress, and the IRQ that tells the
    /// S-CPU it has begun and the first character can be read.
    conversion: Option<Conversion>,
    conversion_irq: bool,
    conversion_irq_enabled: bool,
}

impl Default for Sa1 {
    fn default() -> Self {
        Self {
            cpu: Cpu::new(),
            held: true,
            fault: None,
            to_sa1: Link::default(),
            to_main: Link::default(),
            reset_vector: 0,
            irq_vector: 0,
            main_irq_vector: 0,
            main_irq_vector_selected: false,
            main_nmi_vector: 0,
            main_nmi_vector_selected: false,
            bwram_window: [0; 2],
            bitmap: Bitmap::default(),
            super_mmc: None,
            arithmetic: Arithmetic::default(),
            dma: Dma::default(),
            dma_started: None,
            conversion: None,
            conversion_irq: false,
            conversion_irq_enabled: false,
        }
    }
}

/// `MCNT`, `MA`, `MB`, and the 40-bit result `MR`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Arithmetic {
    divide: bool,
    cumulative: bool,
    a: u16,
    b: u16,
    result: u64,
    overflow: bool,
}

impl Arithmetic {
    const RESULT_MASK: u64 = (1 << 40) - 1;

    fn control(&mut self, value: u8) {
        self.divide = value & 1 != 0;
        self.cumulative = value & 2 != 0;
        if self.cumulative {
            self.result = 0;
        }
    }

    /// Writing the high byte of `MB` starts the operation.
    fn run(&mut self) {
        let (a, b) = (self.a as i16 as i64, self.b);
        if self.cumulative {
            let sum = self.result.wrapping_add((a * b as i16 as i64) as u64);
            self.overflow = sum >> 40 != 0;
            self.result = sum & Self::RESULT_MASK;
        } else if self.divide {
            // Signed dividend, unsigned divisor, remainder never negative.
            self.result = match b as i64 {
                0 => 0,
                b => (a.rem_euclid(b) as u64) << 16 | (a.div_euclid(b) as u64 & 0xFFFF),
            };
            self.a = 0;
        } else {
            self.result = (a * b as i16 as i64) as u64 & 0xFFFF_FFFF;
        }
        self.b = 0;
    }
}

fn set_low(word: &mut u16, value: u8) {
    *word = (*word & 0xFF00) | value as u16;
}

fn set_high(word: &mut u16, value: u8) {
    *word = (*word & 0x00FF) | (value as u16) << 8;
}

impl Sa1 {
    /// Whether the CPU has anything to run on.
    pub fn runnable(&self) -> bool {
        !self.held && self.fault.is_none()
    }

    /// The IRQ line into `processor`.
    pub fn irq(&self, processor: Processor) -> bool {
        match processor {
            Processor::Main => {
                self.to_main.line() || (self.conversion_irq && self.conversion_irq_enabled)
            }
            Processor::Sa1 => self.to_sa1.line() || (self.dma.irq && self.dma.irq_enabled),
        }
    }

    /// The transfer a register write has just started. Once the bus has
    /// made the copy, the SA-1 gets its end-of-DMA IRQ.
    pub fn take_dma(&mut self) -> Option<DmaTransfer> {
        let transfer = self.dma_started.take()?;
        self.dma.irq = true;
        Some(transfer)
    }

    /// The character conversion the S-CPU's BW-RAM reads go through.
    pub fn conversion(&self) -> Option<Conversion> {
        self.conversion
    }

    pub fn irq_vector(&self) -> u16 {
        self.irq_vector
    }

    /// The S-CPU's IRQ vector, if the SA-1 has replaced the cartridge's.
    pub fn main_irq_vector(&self) -> Option<u16> {
        self.main_irq_vector_selected
            .then_some(self.main_irq_vector)
    }

    /// The S-CPU's NMI vector, likewise.
    pub fn main_nmi_vector(&self) -> Option<u16> {
        self.main_nmi_vector_selected
            .then_some(self.main_nmi_vector)
    }

    /// Reads a register, or `None` if `processor` has nothing there.
    pub fn read(&self, processor: Processor, reg: u16) -> Option<u8> {
        let flags = |link: Link| (link.irq as u8) << 7 | link.message;
        Some(match (processor, reg) {
            (Processor::Main, 0x2300) => {
                flags(self.to_main)
                    | (self.conversion_irq as u8) << 5
                    | (self.main_irq_vector_selected as u8) << 6
                    | (self.main_nmi_vector_selected as u8) << 4
            }
            (Processor::Sa1, 0x2301) => flags(self.to_sa1) | (self.dma.irq as u8) << 5,
            (Processor::Sa1, 0x2306..=0x230A) => {
                (self.arithmetic.result >> (8 * (reg - 0x2306))) as u8
            }
            (Processor::Sa1, 0x230B) => (self.arithmetic.overflow as u8) << 7,
            _ => return None,
        })
    }

    /// Writes a register; false if `processor` has nothing there.
    pub fn write(&mut self, processor: Processor, reg: u16, value: u8) -> bool {
        match (processor, reg) {
            (Processor::Main, 0x2200) => {
                self.to_sa1.send(value);
                let held = value & 0x60 != 0;
                if self.held && value & 0x20 == 0 {
                    // Out of reset: a 65816 starts in emulation mode.
                    self.cpu = Cpu::new();
                    self.cpu.emulation = true;
                    self.cpu.pc = self.reset_vector;
                    self.fault = None;
                }
                self.held = held;
            }
            (Processor::Main, 0x2201) => {
                self.to_main.irq_enabled = value & 0x80 != 0;
                self.conversion_irq_enabled = value & 0x20 != 0;
            }
            (Processor::Main, 0x2202) => {
                self.to_main.irq &= value & 0x80 == 0;
                self.conversion_irq &= value & 0x20 == 0;
            }
            (Processor::Main, 0x2203) => set_low(&mut self.reset_vector, value),
            (Processor::Main, 0x2204) => set_high(&mut self.reset_vector, value),
            (Processor::Main, 0x2207) => set_low(&mut self.irq_vector, value),
            (Processor::Main, 0x2208) => set_high(&mut self.irq_vector, value),
            (Processor::Sa1, 0x2209) => {
                self.to_main.send(value);
                self.main_irq_vector_selected = value & 0x40 != 0;
                self.main_nmi_vector_selected = value & 0x10 != 0;
            }
            (Processor::Sa1, 0x220A) => {
                self.to_sa1.irq_enabled = value & 0x80 != 0;
                self.dma.irq_enabled = value & 0x20 != 0;
            }
            (Processor::Sa1, 0x220B) => {
                self.to_sa1.irq &= value & 0x80 == 0;
                self.dma.irq &= value & 0x20 == 0;
            }
            (Processor::Sa1, 0x220C) => set_low(&mut self.main_nmi_vector, value),
            (Processor::Sa1, 0x220D) => set_high(&mut self.main_nmi_vector, value),
            (Processor::Sa1, 0x220E) => set_low(&mut self.main_irq_vector, value),
            (Processor::Sa1, 0x220F) => set_high(&mut self.main_irq_vector, value),
            (Processor::Main, 0x2220..=0x2223) => {
                let SuperMmc(blocks) = self.super_mmc.get_or_insert(SuperMmc::RESET);
                blocks[(reg - 0x2220) as usize] = value;
            }
            (Processor::Main, 0x2224) => self.bwram_window[0] = value & 0x1F,
            (Processor::Sa1, 0x2225) => self.bwram_window[1] = value,
            (Processor::Sa1, 0x223F) => {
                self.bitmap = if value & 0x80 != 0 {
                    Bitmap::TwoBits
                } else {
                    Bitmap::FourBits
                };
            }
            (Processor::Sa1, 0x2230) => {
                self.dma.control = value;
                if self.dma.conversion().is_none() {
                    self.conversion = None;
                }
            }
            // The addresses and `CDMA` are either processor's: the S-CPU
            // sets a conversion up, since it is the one to read it.
            (_, 0x2231) => {
                self.dma.conversion = value;
                if value & 0x80 != 0 {
                    self.conversion = None;
                }
            }
            (_, 0x2232..=0x2234) => set_byte(&mut self.dma.source, reg - 0x2232, value),
            (_, 0x2235..=0x2237) => {
                set_byte(&mut self.dma.dest, reg - 0x2235, value);
                // I-RAM addresses end at the middle byte, and a
                // conversion's buffer is in I-RAM.
                let last = match self.dma.control & 0x24 {
                    4 => 0x2237,
                    _ => 0x2236,
                };
                if reg == last {
                    self.dma_started = self.dma.transfer();
                    if let Some(conversion) = self.dma.conversion() {
                        self.conversion = Some(conversion);
                        self.conversion_irq = true;
                    }
                }
            }
            (Processor::Sa1, 0x2238) => set_low(&mut self.dma.len, value),
            (Processor::Sa1, 0x2239) => set_high(&mut self.dma.len, value),
            (Processor::Sa1, 0x2250) => self.arithmetic.control(value),
            (Processor::Sa1, 0x2251) => set_low(&mut self.arithmetic.a, value),
            (Processor::Sa1, 0x2252) => set_high(&mut self.arithmetic.a, value),
            (Processor::Sa1, 0x2253) => set_low(&mut self.arithmetic.b, value),
            (Processor::Sa1, 0x2254) => {
                set_high(&mut self.arithmetic.b, value);
                self.arithmetic.run();
            }
            // The SA-1's NMI vector and the write protection registers,
            // which SA-1 Pack sets once at start-up: without effect here,
            // since the SA-1's NMI is never raised and protection is not
            // enforced. A write to a read-only status register is nothing.
            (Processor::Main, 0x2205..=0x2206) | (_, 0x2226..=0x222A) | (_, 0x2300..=0x230B) => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arithmetic(sa1: &mut Sa1, control: u8, a: u16, b: u16) -> u64 {
        let [a_low, a_high] = a.to_le_bytes();
        let [b_low, b_high] = b.to_le_bytes();
        for (reg, value) in [
            (0x2250, control),
            (0x2251, a_low),
            (0x2252, a_high),
            (0x2253, b_low),
            (0x2254, b_high),
        ] {
            assert!(sa1.write(Processor::Sa1, reg, value));
        }
        (0..5).fold(0, |result, i| {
            result | (sa1.read(Processor::Sa1, 0x2306 + i).unwrap() as u64) << (8 * i)
        })
    }

    #[test]
    fn arithmetic_is_signed_with_an_unsigned_divisor() {
        let mut sa1 = Sa1::default();
        assert_eq!(arithmetic(&mut sa1, 0, 0xFFFE, 3), 0xFFFF_FFFA); // -2 * 3
        assert_eq!(arithmetic(&mut sa1, 0, 300, 300), 90_000);
        // -7 / 2 = -4 remainder 1; the remainder is in the upper word.
        assert_eq!(arithmetic(&mut sa1, 1, 0xFFF9, 2), 0x0001_FFFC);
        assert_eq!(arithmetic(&mut sa1, 1, 100, 0), 0);
        // Selecting the cumulative sum clears it; each run adds a product.
        assert_eq!(arithmetic(&mut sa1, 2, 1000, 1000), 1_000_000);
        for (reg, value) in [(0x2251, 0xFF), (0x2252, 0xFF), (0x2253, 1), (0x2254, 0)] {
            sa1.write(Processor::Sa1, reg, value);
        }
        assert_eq!(sa1.arithmetic.result, 999_999);
    }

    #[test]
    fn dma_starts_on_the_destination_and_ends_with_an_irq() {
        let mut sa1 = Sa1::default();
        let write = |sa1: &mut Sa1, regs: &[(u16, u8)]| {
            for &(reg, value) in regs {
                assert!(sa1.write(Processor::Sa1, reg, value));
            }
        };
        // `$0400` bytes of ROM at `$128000` to BW-RAM `$402400`.
        write(
            &mut sa1,
            &[
                (0x220A, 0x20),
                (0x2230, 0xC4),
                (0x2232, 0x00),
                (0x2233, 0x80),
                (0x2234, 0x12),
                (0x2238, 0x00),
                (0x2239, 0x04),
                (0x2235, 0x00),
                (0x2236, 0x24),
            ],
        );
        assert_eq!(sa1.take_dma(), None); // BW-RAM addresses have a bank
        write(&mut sa1, &[(0x2237, 0x40)]);
        let transfer = DmaTransfer {
            source: 0x12_8000,
            dest: 0x40_2400,
            len: 0x400,
        };
        assert_eq!(sa1.take_dma(), Some(transfer));
        assert_eq!(sa1.take_dma(), None);
        assert!(sa1.irq(Processor::Sa1));
        assert_eq!(sa1.read(Processor::Sa1, 0x2301), Some(0x20));
        write(&mut sa1, &[(0x220B, 0x20)]);
        assert!(!sa1.irq(Processor::Sa1));
        // BW-RAM to I-RAM starts a byte sooner; character conversion is
        // not modelled and starts nothing.
        write(&mut sa1, &[(0x2230, 0x81), (0x2235, 0x10), (0x2236, 0x31)]);
        assert_eq!(sa1.take_dma().map(|t| t.dest), Some(0x00_3110));
        write(&mut sa1, &[(0x2230, 0xA1), (0x2235, 0x10), (0x2236, 0x31)]);
        assert_eq!(sa1.take_dma(), None);
    }

    #[test]
    fn each_processor_raises_and_the_other_acknowledges() {
        let mut sa1 = Sa1::default();
        assert!(!sa1.runnable());
        sa1.write(Processor::Main, 0x2203, 0x34);
        sa1.write(Processor::Main, 0x2204, 0x12);
        sa1.write(Processor::Main, 0x2200, 0x00);
        assert!(sa1.runnable());
        assert_eq!((sa1.cpu.pc, sa1.cpu.emulation), (0x1234, true));

        sa1.write(Processor::Main, 0x2200, 0x83);
        assert_eq!(sa1.read(Processor::Sa1, 0x2301), Some(0x83));
        assert!(!sa1.irq(Processor::Sa1)); // masked until `CIE` allows it
        sa1.write(Processor::Sa1, 0x220A, 0x80);
        assert!(sa1.irq(Processor::Sa1));
        sa1.write(Processor::Sa1, 0x220B, 0x80);
        assert!(!sa1.irq(Processor::Sa1));
        // The S-CPU cannot write the SA-1's side, nor read its flags.
        assert!(!sa1.write(Processor::Main, 0x2209, 0x80));
        assert_eq!(sa1.read(Processor::Main, 0x2301), None);

        sa1.write(Processor::Sa1, 0x220E, 0x00);
        sa1.write(Processor::Sa1, 0x220F, 0x1D);
        sa1.write(Processor::Sa1, 0x2209, 0xC5);
        sa1.write(Processor::Main, 0x2201, 0x80);
        assert!(sa1.irq(Processor::Main));
        assert_eq!(sa1.read(Processor::Main, 0x2300), Some(0xC5));
        assert_eq!(sa1.main_irq_vector(), Some(0x1D00));
        assert_eq!(sa1.main_nmi_vector(), None);
        sa1.write(Processor::Sa1, 0x220C, 0x6A);
        sa1.write(Processor::Sa1, 0x220D, 0x81);
        sa1.write(Processor::Sa1, 0x2209, 0x50);
        assert_eq!(sa1.main_nmi_vector(), Some(0x816A));
        assert_eq!(sa1.read(Processor::Main, 0x2300), Some(0xD0));
    }
    #[test]
    fn the_super_mmc_starts_from_reset_once_it_is_written() {
        let mut sa1 = Sa1::default();
        assert_eq!(sa1.super_mmc, None);
        // Only the S-CPU has the registers.
        assert!(!sa1.write(Processor::Sa1, 0x2221, 0x85));
        assert!(sa1.write(Processor::Main, 0x2221, 0x85));
        assert_eq!(sa1.super_mmc, Some(SuperMmc([0, 0x85, 2, 3])));
    }

    #[test]
    fn character_conversion_turns_a_bitmap_into_planes() {
        let mut sa1 = Sa1::default();
        sa1.write(Processor::Main, 0x2201, 0x20);
        sa1.write(Processor::Sa1, 0x2230, 0xB0);
        // 4 bits to a pixel, four characters across, bitmap at `$402000`.
        for (reg, value) in [(0x2231, 0x09), (0x2233, 0x20), (0x2234, 0x40)] {
            assert!(sa1.write(Processor::Main, reg, value));
        }
        assert_eq!(sa1.conversion(), None);
        sa1.write(Processor::Main, 0x2235, 0x00);
        sa1.write(Processor::Main, 0x2236, 0x37);
        assert!(sa1.irq(Processor::Main));
        assert_eq!(sa1.read(Processor::Main, 0x2300), Some(0x20));
        sa1.write(Processor::Main, 0x2202, 0x20);
        assert!(!sa1.irq(Processor::Main));

        // Character 5 is the second of the second row. A bitmap row is 16
        // bytes, so its line 2 starts at byte (8 + 2) * 16 + 4: pixels
        // 0 to 7 there are colours 1, 2, 4, 8, 15, 0, 0, 3.
        let mut bitmap = [0u8; 0x200];
        bitmap[164..168].copy_from_slice(&[0x21, 0x84, 0x0F, 0x30]);
        let conversion = sa1.conversion().unwrap();
        let read = |byte: u32| {
            conversion.read(0x40_2000 + 5 * 32 + byte, |at| {
                bitmap[(at - 0x40_2000) as usize]
            })
        };
        assert_eq!(read(4), 0b1000_1001); // plane 0 of line 2
        assert_eq!(read(5), 0b0100_1001); // plane 1
        assert_eq!(read(16 + 4), 0b0010_1000); // plane 2
        assert_eq!(read(16 + 5), 0b0001_1000); // plane 3
        assert_eq!(read(0), 0);

        // `CDMA` bit 7 ends it.
        sa1.write(Processor::Main, 0x2231, 0x80);
        assert_eq!(sa1.conversion(), None);
    }
}
