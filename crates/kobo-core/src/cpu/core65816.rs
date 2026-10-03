//! The 65816 core: registers, addressing modes, and the opcode table.

use thiserror::Error;

use crate::clean_room::{self, CodeAddr};

/// Memory as seen by the CPU: a flat 24-bit address space.
pub trait Bus {
    /// Called before each instruction, including instructions on a second
    /// processor. Implementations may enforce an operation-wide budget.
    fn before_instruction(&mut self) -> Result<(), CpuError> {
        Ok(())
    }

    fn read(&mut self, addr: u32) -> u8;
    fn write(&mut self, addr: u32, value: u8);

    /// Reads an instruction's opcode or operand, as [`Bus::read`] does; a
    /// bus that reports what it reads tells the two apart.
    fn fetch(&mut self, addr: u32) -> u8 {
        self.read(addr)
    }

    /// Whether the IRQ line is asserted. Polled between instructions while
    /// the CPU has interrupts enabled.
    fn irq(&mut self) -> bool {
        false
    }

    /// Where an IRQ takes the CPU: the cartridge's vector, unless the bus
    /// supplies it from somewhere else (the SA-1 reads its own from
    /// registers).
    fn irq_vector(&mut self, emulation: bool) -> u16 {
        let addr = if emulation { 0xFFFE } else { 0xFFEE };
        u16::from_le_bytes([self.read(addr), self.read(addr + 1)])
    }

    /// The CPU is waiting for something outside itself (see
    /// [`Cpu::run`]). Runs whatever else shares the bus and returns
    /// whether any of it got anywhere; false means nothing the CPU waits
    /// for is going to happen.
    fn wait(&mut self) -> bool {
        false
    }

    /// The CPU is waiting and nothing on the bus answered: the console
    /// would answer a wait for the vertical blank with the NMI. The
    /// native-mode vector to take, if the bus has one to give.
    fn vblank(&mut self) -> Option<u16> {
        None
    }
}

/// `KOBO_CPU_TRACE`: how many instructions every CPU keeps in
/// [`Cpu::history`] (64 if it is set to nothing a number can be made of),
/// for the trace printed when a routine fails or a watched write happens.
/// None once the process is [`clean_room::forbidden`]: an instruction
/// trace of a ROM Lunar Magic saved prints its code.
fn trace_length() -> Option<usize> {
    if clean_room::forbidden() {
        return None;
    }
    let value = std::env::var("KOBO_CPU_TRACE").ok()?;
    Some(value.parse().unwrap_or(64))
}

/// What a watched write is reported as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Watched {
    /// The instruction that wrote it: its address, and A, X, and Y.
    At(u32, u16, u16, u16),
    /// The write alone, in a [`clean_room::forbidden`] process, where the
    /// instruction would show where Lunar Magic's code is.
    Alone,
    /// Nothing of the write: in a forbidden process, a write to the
    /// stack's page, which is how a return address gets there.
    Stack,
}

/// The line a watched write is reported with.
fn watch_report(addr: u32, value: u8, watched: Watched) -> String {
    match watched {
        Watched::At(pc, a, x, y) => {
            format!(
                "watch: ${addr:06X} <- ${value:02X} at ${pc:06X} (A={a:04X} X={x:04X} Y={y:04X})"
            )
        }
        Watched::Alone => format!("watch: ${addr:06X} <- ${value:02X}"),
        Watched::Stack => format!("watch: ${addr:06X} written on the stack (withheld: clean room)"),
    }
}

/// `KOBO_RAM_WATCH`: the bus address every CPU reports its writes to.
fn watched_address() -> Option<u32> {
    let value = std::env::var("KOBO_RAM_WATCH").ok()?;
    let addr = u32::from_str_radix(value.trim().trim_start_matches('$'), 16).ok()?;
    Some(mirror_free(addr & 0xFF_FFFF))
}

/// The one address of a location the system area mirrors: the low half
/// of banks `$00`-`$3F` and `$80`-`$BF` is the same registers and RAM in
/// every one of them, and its first 8 KiB are the start of `$7E`.
fn mirror_free(addr: u32) -> u32 {
    let (bank, offset) = (addr >> 16, addr & 0xFFFF);
    if (bank & 0x40 != 0) || offset >= 0x8000 {
        addr
    } else if offset < 0x2000 {
        0x7E_0000 | offset
    } else {
        offset
    }
}

/// Processor status bits.
pub struct Flags;

impl Flags {
    pub const N: u8 = 0x80;
    pub const V: u8 = 0x40;
    pub const M: u8 = 0x20;
    pub const X: u8 = 0x10;
    pub const D: u8 = 0x08;
    pub const I: u8 = 0x04;
    pub const Z: u8 = 0x02;
    pub const C: u8 = 0x01;
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CpuError {
    #[error(transparent)]
    Operation(#[from] crate::operation::OperationError),
    #[error("BRK at {at}")]
    Brk { at: CodeAddr },
    #[error("COP at {at}")]
    Cop { at: CodeAddr },
    #[error("CPU halted (STP) at {at}")]
    Halted { at: CodeAddr },
    #[error("waiting at {at} for something that never happens")]
    Waiting { at: CodeAddr },
    #[error("on the SA-1: {0}")]
    Sa1(Box<CpuError>),
    #[error("instruction limit of {limit} exceeded at {at}")]
    Limit { limit: u64, at: CodeAddr },
    #[error("refused: a ROM Lunar Magic saved is loaded (clean room)")]
    Forbidden,
}

/// One executed instruction and the registers it was executed with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Executed {
    pub addr: u32,
    pub opcode: u8,
    pub a: u16,
    pub x: u16,
    pub y: u16,
    pub sp: u16,
    pub dp: u16,
    pub db: u8,
    pub p: u8,
    pub emulation: bool,
}

/// How [`Cpu::run`] ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Run {
    Done,
    /// Waiting for something the bus could not make happen.
    Waiting,
}

/// Everything a loop can change about the CPU, as it stood at a backward
/// jump. Two the same in a row mean the loop is polling memory for a
/// change that has to come from outside.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct LoopState {
    target: u32,
    registers: [u16; 5],
    db: u8,
    p: u8,
    writes: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cpu {
    pub a: u16,
    pub x: u16,
    pub y: u16,
    pub sp: u16,
    pub dp: u16,
    pub db: u8,
    pub pb: u8,
    pub pc: u16,
    pub p: u8,
    pub emulation: bool,
    /// Instructions executed so far.
    pub steps: u64,
    /// When set, every data read (not instruction fetch) is recorded as
    /// (address of the instruction, address read)
    /// ([`Cpu::trace_data_reads`]).
    data_reads: Option<Vec<(Option<u32>, u32)>>,
    /// When set, the last instructions executed, oldest first, for
    /// tracing what led a routine somewhere ([`Cpu::keep_history`]).
    history: Option<std::collections::VecDeque<Executed>>,
    history_len: usize,
    /// Debugging aid: a bus address whose every write is reported on
    /// standard error with the instruction that made it, from
    /// `KOBO_RAM_WATCH` (`$40D5D5` or `40D5D5`; a bus address, so a
    /// relocated variable is named where the ROM's map puts it).
    watch: Option<u32>,
    op_addr: u32,
    /// Bytes written so far.
    writes: u64,
    last_loop: Option<LoopState>,
    /// The last instruction was `WAI`, or closed a loop that changes
    /// nothing.
    waiting: bool,
    /// Where the stack stands when the `RTS` of a routine entered by
    /// [`Cpu::enter_jsr`] has returned.
    return_sp: Option<u16>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

/// Tag on an operand address whose second byte stays in the same bank
/// instead of carrying into the next: direct page and stack-relative
/// operands (bank 0), immediate operands, and the pointers of indexed
/// indirect jumps (the program bank). Bit 24 is outside the address bus;
/// the memory helpers strip it.
const WRAPS_IN_BANK: u32 = 1 << 24;

/// How many vertical blanks a single run answers waits with before
/// giving the wait up as one nothing is going to end: a handler that
/// changes nothing the loop looks at would otherwise be run forever.
const WAIT_VBLANK_LIMIT: u32 = 16;

/// Sentinel return address used by [`Cpu::call`].
const RETURN_PB: u8 = 0xFF;
const RETURN_PC: u16 = 0xFFFF;

impl Cpu {
    /// A CPU in native mode with 8-bit accumulator and index registers,
    /// interrupts disabled, stack at `$01FF`.
    pub fn new() -> Self {
        let mut cpu = Self::fresh();
        if let Some(len) = trace_length() {
            cpu.keep_history(len);
        }
        cpu
    }

    fn fresh() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0x01FF,
            dp: 0,
            db: 0,
            pb: 0,
            pc: 0,
            p: Flags::M | Flags::X | Flags::I,
            emulation: false,
            steps: 0,
            data_reads: None,
            history: None,
            history_len: 0,
            watch: watched_address(),
            op_addr: 0,
            writes: 0,
            last_loop: None,
            waiting: false,
            return_sp: None,
        }
    }

    /// Puts the registers back to what [`Cpu::new`] gives, so a routine
    /// starts from a known state whatever ran before it. The step count
    /// and any read trace carry on.
    pub fn reset_registers(&mut self) {
        let steps = self.steps;
        let trace = self.data_reads.take();
        let history = self.history.take();
        let history_len = self.history_len;
        *self = Self::new();
        self.steps = steps;
        self.data_reads = trace;
        self.history = history;
        self.history_len = history_len;
    }

    /// Prints [`Cpu::history`] on standard error under `heading`, if it
    /// is kept.
    pub fn print_history(&self, heading: &str) {
        let Some(history) = &self.history else {
            return;
        };
        if clean_room::forbidden() {
            return;
        }
        eprintln!("--- {heading}: last {} instructions ---", history.len());
        for e in history.iter() {
            eprintln!(
                "${:06X}: {:02X}  A={:04X} X={:04X} Y={:04X} SP={:04X} DP={:04X} DB={:02X} P={:02X}{}",
                e.addr,
                e.opcode,
                e.a,
                e.x,
                e.y,
                e.sp,
                e.dp,
                e.db,
                e.p,
                if e.emulation { " E" } else { "" }
            );
        }
    }

    /// Keeps the last `len` instructions executed, for
    /// [`Cpu::print_history`]. Does nothing in a
    /// [`clean_room::forbidden`] process.
    pub fn keep_history(&mut self, len: usize) {
        if clean_room::forbidden() {
            return;
        }
        self.history = Some(std::collections::VecDeque::with_capacity(len));
        self.history_len = len;
    }

    /// Whether the last instructions are being kept.
    pub fn keeps_history(&self) -> bool {
        self.history.is_some()
    }

    /// Starts recording every data read as (address of the reading
    /// instruction, address read), or stops and returns what was
    /// recorded. In a [`clean_room::forbidden`] process the instruction's
    /// address is `None`: it would show where Lunar Magic's code is.
    pub fn trace_data_reads(&mut self, on: bool) -> Option<Vec<(Option<u32>, u32)>> {
        if on {
            self.data_reads = Some(Vec::new());
            None
        } else {
            let mut trace = self.data_reads.take()?;
            if clean_room::forbidden() {
                trace.iter_mut().for_each(|(at, _)| *at = None);
            }
            Some(trace)
        }
    }

    // --- Flag helpers -------------------------------------------------

    fn flag(&self, f: u8) -> bool {
        self.p & f != 0
    }

    fn set_flag(&mut self, f: u8, on: bool) {
        if on {
            self.p |= f;
        } else {
            self.p &= !f;
        }
    }

    /// True when the accumulator and memory are 8 bits wide.
    pub fn m8(&self) -> bool {
        self.emulation || self.flag(Flags::M)
    }

    /// True when the index registers are 8 bits wide.
    pub fn x8(&self) -> bool {
        self.emulation || self.flag(Flags::X)
    }

    fn set_nz(&mut self, value: u16, width8: bool) {
        if width8 {
            self.set_flag(Flags::Z, value & 0xFF == 0);
            self.set_flag(Flags::N, value & 0x80 != 0);
        } else {
            self.set_flag(Flags::Z, value == 0);
            self.set_flag(Flags::N, value & 0x8000 != 0);
        }
    }

    fn set_p(&mut self, p: u8) {
        self.p = p;
        if self.emulation {
            self.p |= Flags::M | Flags::X;
        }
        if self.x8() {
            self.x &= 0xFF;
            self.y &= 0xFF;
        }
    }

    // --- Memory helpers -----------------------------------------------

    fn read8(&mut self, bus: &mut impl Bus, addr: u32) -> u8 {
        if let Some(t) = &mut self.data_reads {
            let at = (!clean_room::forbidden()).then_some(self.op_addr);
            t.push((at, addr & 0xFF_FFFF));
        }
        bus.read(addr & 0xFF_FFFF)
    }

    /// The address after `addr`. Absolute and indirect addresses carry
    /// into the next bank; those tagged [`WRAPS_IN_BANK`] stay in theirs.
    fn next_addr(addr: u32) -> u32 {
        if addr & WRAPS_IN_BANK != 0 {
            (addr & !0xFFFF) | (addr.wrapping_add(1) & 0xFFFF)
        } else {
            addr.wrapping_add(1)
        }
    }

    fn read16(&mut self, bus: &mut impl Bus, addr: u32) -> u16 {
        let lo = self.read8(bus, addr) as u16;
        let hi = self.read8(bus, Self::next_addr(addr)) as u16;
        lo | (hi << 8)
    }

    /// 16-bit read that wraps within bank 0, for direct page and stack.
    fn read16_bank0(&mut self, bus: &mut impl Bus, addr: u16) -> u16 {
        let lo = self.read8(bus, addr as u32) as u16;
        let hi = self.read8(bus, addr.wrapping_add(1) as u32) as u16;
        lo | (hi << 8)
    }

    fn read24_bank0(&mut self, bus: &mut impl Bus, addr: u16) -> u32 {
        let lo = self.read16_bank0(bus, addr) as u32;
        let bank = self.read8(bus, addr.wrapping_add(2) as u32) as u32;
        lo | (bank << 16)
    }

    fn write8(&mut self, bus: &mut impl Bus, addr: u32, value: u8) {
        self.writes += 1;
        let addr = addr & 0xFF_FFFF;
        if self.watch == Some(mirror_free(addr)) {
            let on_stack = addr < 0x1_0000 && addr & 0xFF00 == self.sp as u32 & 0xFF00;
            let watched = if !clean_room::forbidden() {
                Watched::At(self.op_addr, self.a, self.x, self.y)
            } else if on_stack {
                Watched::Stack
            } else {
                Watched::Alone
            };
            let what = watch_report(addr, value, watched);
            self.print_history(&what);
            eprintln!("{what}");
        }
        bus.write(addr, value);
    }

    fn write16(&mut self, bus: &mut impl Bus, addr: u32, value: u16) {
        self.write8(bus, addr, value as u8);
        self.write8(bus, Self::next_addr(addr), (value >> 8) as u8);
    }

    /// Reads a value of the accumulator width.
    fn read_m(&mut self, bus: &mut impl Bus, addr: u32) -> u16 {
        if self.m8() {
            self.read8(bus, addr) as u16
        } else {
            self.read16(bus, addr)
        }
    }

    fn write_m(&mut self, bus: &mut impl Bus, addr: u32, value: u16) {
        if self.m8() {
            self.write8(bus, addr, value as u8);
        } else {
            self.write16(bus, addr, value);
        }
    }

    fn read_x(&mut self, bus: &mut impl Bus, addr: u32) -> u16 {
        if self.x8() {
            self.read8(bus, addr) as u16
        } else {
            self.read16(bus, addr)
        }
    }

    fn write_x(&mut self, bus: &mut impl Bus, addr: u32, value: u16) {
        if self.x8() {
            self.write8(bus, addr, value as u8);
        } else {
            self.write16(bus, addr, value);
        }
    }

    fn pc_addr(&self) -> u32 {
        ((self.pb as u32) << 16) | self.pc as u32
    }

    fn fetch8(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.fetch(self.pc_addr());
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        lo | (hi << 8)
    }

    fn fetch24(&mut self, bus: &mut impl Bus) -> u32 {
        let lo = self.fetch16(bus) as u32;
        let bank = self.fetch8(bus) as u32;
        lo | (bank << 16)
    }

    // --- Stack ---------------------------------------------------------

    fn push8(&mut self, bus: &mut impl Bus, value: u8) {
        self.write8(bus, self.sp as u32, value);
        self.sp = self.sp.wrapping_sub(1);
        if self.emulation {
            self.sp = 0x0100 | (self.sp & 0xFF);
        }
    }

    fn push16(&mut self, bus: &mut impl Bus, value: u16) {
        self.push8(bus, (value >> 8) as u8);
        self.push8(bus, value as u8);
    }

    fn pull8(&mut self, bus: &mut impl Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        if self.emulation {
            self.sp = 0x0100 | (self.sp & 0xFF);
        }
        self.read8(bus, self.sp as u32)
    }

    fn pull16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pull8(bus) as u16;
        let hi = self.pull8(bus) as u16;
        lo | (hi << 8)
    }

    // --- Addressing modes ---------------------------------------------
    // Each returns the 24-bit effective address.

    fn db_addr(&self, offset: u16) -> u32 {
        ((self.db as u32) << 16) | offset as u32
    }

    fn am_abs(&mut self, bus: &mut impl Bus) -> u32 {
        let o = self.fetch16(bus);
        self.db_addr(o)
    }

    fn am_abs_x(&mut self, bus: &mut impl Bus) -> u32 {
        let o = self.fetch16(bus);
        (self.db_addr(o) + self.x as u32) & 0xFF_FFFF
    }

    fn am_abs_y(&mut self, bus: &mut impl Bus) -> u32 {
        let o = self.fetch16(bus);
        (self.db_addr(o) + self.y as u32) & 0xFF_FFFF
    }

    fn am_long(&mut self, bus: &mut impl Bus) -> u32 {
        self.fetch24(bus)
    }

    fn am_long_x(&mut self, bus: &mut impl Bus) -> u32 {
        (self.fetch24(bus) + self.x as u32) & 0xFF_FFFF
    }

    fn dp_base(&mut self, bus: &mut impl Bus) -> u16 {
        let o = self.fetch8(bus) as u16;
        self.dp.wrapping_add(o)
    }

    fn am_dp(&mut self, bus: &mut impl Bus) -> u32 {
        self.dp_base(bus) as u32 | WRAPS_IN_BANK
    }

    fn am_dp_x(&mut self, bus: &mut impl Bus) -> u32 {
        self.dp_base(bus).wrapping_add(self.x) as u32 | WRAPS_IN_BANK
    }

    fn am_dp_y(&mut self, bus: &mut impl Bus) -> u32 {
        self.dp_base(bus).wrapping_add(self.y) as u32 | WRAPS_IN_BANK
    }

    fn am_dp_ind(&mut self, bus: &mut impl Bus) -> u32 {
        let d = self.dp_base(bus);
        let p = self.read16_bank0(bus, d);
        self.db_addr(p)
    }

    fn am_dp_x_ind(&mut self, bus: &mut impl Bus) -> u32 {
        let d = self.dp_base(bus).wrapping_add(self.x);
        let p = self.read16_bank0(bus, d);
        self.db_addr(p)
    }

    fn am_dp_ind_y(&mut self, bus: &mut impl Bus) -> u32 {
        let d = self.dp_base(bus);
        let p = self.read16_bank0(bus, d);
        (self.db_addr(p) + self.y as u32) & 0xFF_FFFF
    }

    fn am_dp_ind_long(&mut self, bus: &mut impl Bus) -> u32 {
        let d = self.dp_base(bus);
        self.read24_bank0(bus, d)
    }

    fn am_dp_ind_long_y(&mut self, bus: &mut impl Bus) -> u32 {
        let d = self.dp_base(bus);
        (self.read24_bank0(bus, d) + self.y as u32) & 0xFF_FFFF
    }

    fn am_sr(&mut self, bus: &mut impl Bus) -> u32 {
        let o = self.fetch8(bus) as u16;
        self.sp.wrapping_add(o) as u32 | WRAPS_IN_BANK
    }

    fn am_sr_ind_y(&mut self, bus: &mut impl Bus) -> u32 {
        let o = self.fetch8(bus) as u16;
        let p = self.read16_bank0(bus, self.sp.wrapping_add(o));
        (self.db_addr(p) + self.y as u32) & 0xFF_FFFF
    }

    fn am_imm_m(&mut self, _bus: &mut impl Bus) -> u32 {
        let a = self.pc_addr() | WRAPS_IN_BANK;
        self.pc = self.pc.wrapping_add(if self.m8() { 1 } else { 2 });
        a
    }

    fn am_imm_x(&mut self, _bus: &mut impl Bus) -> u32 {
        let a = self.pc_addr() | WRAPS_IN_BANK;
        self.pc = self.pc.wrapping_add(if self.x8() { 1 } else { 2 });
        a
    }

    // --- ALU -----------------------------------------------------------

    fn op_ora(&mut self, v: u16) {
        self.a = if self.m8() {
            (self.a & 0xFF00) | ((self.a | v) & 0xFF)
        } else {
            self.a | v
        };
        self.set_nz(self.a, self.m8());
    }

    fn op_and(&mut self, v: u16) {
        self.a = if self.m8() {
            (self.a & 0xFF00) | ((self.a & v) & 0xFF)
        } else {
            self.a & v
        };
        self.set_nz(self.a, self.m8());
    }

    fn op_eor(&mut self, v: u16) {
        self.a = if self.m8() {
            (self.a & 0xFF00) | ((self.a ^ v) & 0xFF)
        } else {
            self.a ^ v
        };
        self.set_nz(self.a, self.m8());
    }

    fn op_adc(&mut self, v: u16) {
        self.add_to_a(v, false);
    }

    /// A - M - !C is A + !M + C.
    fn op_sbc(&mut self, v: u16) {
        self.add_to_a(!v, true);
    }

    /// Adds `v` and the carry to the accumulator; `subtract` says `v` is
    /// an inverted subtrahend. In decimal mode the ALU adds a digit at a
    /// time and corrects each before carrying into the next: after an
    /// addition a digit over 9 has 6 added, after a subtraction a digit
    /// that produced no carry has 6 taken away. The overflow flag comes
    /// from the sum before the top digit is corrected. Digits that are
    /// not valid BCD come out as they do on the hardware.
    fn add_to_a(&mut self, v: u16, subtract: bool) {
        let width8 = self.m8();
        let (mask, sign, digits) = if width8 {
            (0xFF, 0x80, 2)
        } else {
            (0xFFFF, 0x8000, 4)
        };
        let a = (self.a & mask) as i32;
        let v = (v & mask) as i32;
        let mut carry = self.flag(Flags::C) as i32;
        let decimal = self.flag(Flags::D);
        // Corrects the digit at `shift` of a sum whose higher digits are
        // still to come.
        let correct = |sum: i32, shift: u32| {
            let digit_max = (0x10 << shift) - 1;
            if subtract && sum <= digit_max {
                sum - (6 << shift)
            } else if !subtract && sum > digit_max - (6 << shift) {
                sum + (6 << shift)
            } else {
                sum
            }
        };
        let mut sum = a + v + carry;
        if decimal {
            sum = 0;
            for digit in 0..digits {
                let shift = 4 * digit;
                let below = (1 << shift) - 1;
                let nibble = 0xF << shift;
                sum = (a & nibble) + (v & nibble) + (carry << shift) + (sum & below);
                if digit + 1 < digits {
                    sum = correct(sum, shift);
                    carry = (sum > (nibble | below)) as i32;
                }
            }
        }
        self.set_flag(Flags::V, !(a ^ v) & (a ^ sum) & sign != 0);
        if decimal {
            sum = correct(sum, 4 * (digits - 1));
        }
        self.set_flag(Flags::C, sum > mask as i32);
        self.a = (self.a & !mask) | (sum as u16 & mask);
        self.set_nz(self.a, width8);
    }

    fn op_cmp(&mut self, reg: u16, v: u16, width8: bool) {
        let (r, m) = if width8 {
            (reg & 0xFF, v & 0xFF)
        } else {
            (reg, v)
        };
        let diff = r.wrapping_sub(m);
        self.set_flag(Flags::C, r >= m);
        self.set_nz(diff, width8);
    }

    fn op_bit(&mut self, v: u16) {
        if self.m8() {
            self.set_flag(Flags::Z, (self.a & v & 0xFF) == 0);
            self.set_flag(Flags::N, v & 0x80 != 0);
            self.set_flag(Flags::V, v & 0x40 != 0);
        } else {
            self.set_flag(Flags::Z, (self.a & v) == 0);
            self.set_flag(Flags::N, v & 0x8000 != 0);
            self.set_flag(Flags::V, v & 0x4000 != 0);
        }
    }

    // Read-modify-write helpers operate on a value of accumulator width.

    fn rmw_asl(&mut self, v: u16) -> u16 {
        if self.m8() {
            self.set_flag(Flags::C, v & 0x80 != 0);
            let r = (v << 1) & 0xFF;
            self.set_nz(r, true);
            r
        } else {
            self.set_flag(Flags::C, v & 0x8000 != 0);
            let r = v << 1;
            self.set_nz(r, false);
            r
        }
    }

    fn rmw_lsr(&mut self, v: u16) -> u16 {
        let v = if self.m8() { v & 0xFF } else { v };
        self.set_flag(Flags::C, v & 1 != 0);
        let r = v >> 1;
        self.set_nz(r, self.m8());
        r
    }

    fn rmw_rol(&mut self, v: u16) -> u16 {
        let c = self.flag(Flags::C) as u16;
        if self.m8() {
            self.set_flag(Flags::C, v & 0x80 != 0);
            let r = ((v << 1) | c) & 0xFF;
            self.set_nz(r, true);
            r
        } else {
            self.set_flag(Flags::C, v & 0x8000 != 0);
            let r = (v << 1) | c;
            self.set_nz(r, false);
            r
        }
    }

    fn rmw_ror(&mut self, v: u16) -> u16 {
        let c = self.flag(Flags::C) as u16;
        if self.m8() {
            let v = v & 0xFF;
            self.set_flag(Flags::C, v & 1 != 0);
            let r = (v >> 1) | (c << 7);
            self.set_nz(r, true);
            r
        } else {
            self.set_flag(Flags::C, v & 1 != 0);
            let r = (v >> 1) | (c << 15);
            self.set_nz(r, false);
            r
        }
    }

    fn rmw_inc(&mut self, v: u16) -> u16 {
        let r = if self.m8() {
            (v.wrapping_add(1)) & 0xFF
        } else {
            v.wrapping_add(1)
        };
        self.set_nz(r, self.m8());
        r
    }

    fn rmw_dec(&mut self, v: u16) -> u16 {
        let r = if self.m8() {
            (v.wrapping_sub(1)) & 0xFF
        } else {
            v.wrapping_sub(1)
        };
        self.set_nz(r, self.m8());
        r
    }

    fn rmw_mem(&mut self, bus: &mut impl Bus, addr: u32, f: fn(&mut Self, u16) -> u16) {
        let v = self.read_m(bus, addr);
        let r = f(self, v);
        self.write_m(bus, addr, r);
    }

    fn rmw_acc(&mut self, f: fn(&mut Self, u16) -> u16) {
        let v = if self.m8() { self.a & 0xFF } else { self.a };
        let r = f(self, v);
        self.a = if self.m8() {
            (self.a & 0xFF00) | (r & 0xFF)
        } else {
            r
        };
    }

    fn set_a(&mut self, v: u16) {
        self.a = if self.m8() {
            (self.a & 0xFF00) | (v & 0xFF)
        } else {
            v
        };
        self.set_nz(self.a, self.m8());
    }

    fn set_x(&mut self, v: u16) {
        self.x = if self.x8() { v & 0xFF } else { v };
        self.set_nz(self.x, self.x8());
    }

    fn set_y(&mut self, v: u16) {
        self.y = if self.x8() { v & 0xFF } else { v };
        self.set_nz(self.y, self.x8());
    }

    fn branch(&mut self, bus: &mut impl Bus, cond: bool) {
        let rel = self.fetch8(bus) as i8;
        if cond {
            self.pc = self.pc.wrapping_add(rel as i16 as u16);
            if rel < 0 {
                self.note_loop();
            }
        }
    }

    /// Called on a backward jump: sets `waiting` if nothing has changed
    /// since the last one to the same place.
    fn note_loop(&mut self) {
        let state = LoopState {
            target: self.pc_addr(),
            registers: [self.a, self.x, self.y, self.sp, self.dp],
            db: self.db,
            p: self.p,
            writes: self.writes,
        };
        self.waiting = self.last_loop == Some(state);
        self.last_loop = Some(state);
    }

    // --- Execution -----------------------------------------------------

    /// Executes one instruction.
    pub fn step(&mut self, bus: &mut impl Bus) -> Result<(), CpuError> {
        bus.before_instruction()?;
        let op_pb = self.pb;
        let op_pc = self.pc;
        self.op_addr = self.pc_addr();
        let opcode = self.fetch8(bus);
        self.steps += 1;
        if let Some(history) = &mut self.history {
            if history.len() == self.history_len {
                history.pop_front();
            }
            history.push_back(Executed {
                addr: self.op_addr,
                opcode,
                a: self.a,
                x: self.x,
                y: self.y,
                sp: self.sp,
                dp: self.dp,
                db: self.db,
                p: self.p,
                emulation: self.emulation,
            });
        }
        match opcode {
            // ORA
            0x01 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x03 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x05 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x07 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x09 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x0D => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x0F => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x11 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x12 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x13 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x15 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x17 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x19 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x1D => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            0x1F => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_ora(v);
            }
            // AND
            0x21 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x23 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x25 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x27 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x29 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x2D => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x2F => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x31 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x32 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x33 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x35 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x37 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x39 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x3D => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            0x3F => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_and(v);
            }
            // EOR
            0x41 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x43 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x45 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x47 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x49 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x4D => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x4F => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x51 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x52 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x53 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x55 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x57 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x59 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x5D => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            0x5F => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_eor(v);
            }
            // ADC
            0x61 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x63 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x65 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x67 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x69 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x6D => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x6F => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x71 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x72 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x73 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x75 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x77 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x79 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x7D => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            0x7F => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_adc(v);
            }
            // STA
            0x81 => {
                let a = self.am_dp_x_ind(bus);
                self.write_m(bus, a, self.a);
            }
            0x83 => {
                let a = self.am_sr(bus);
                self.write_m(bus, a, self.a);
            }
            0x85 => {
                let a = self.am_dp(bus);
                self.write_m(bus, a, self.a);
            }
            0x87 => {
                let a = self.am_dp_ind_long(bus);
                self.write_m(bus, a, self.a);
            }
            0x8D => {
                let a = self.am_abs(bus);
                self.write_m(bus, a, self.a);
            }
            0x8F => {
                let a = self.am_long(bus);
                self.write_m(bus, a, self.a);
            }
            0x91 => {
                let a = self.am_dp_ind_y(bus);
                self.write_m(bus, a, self.a);
            }
            0x92 => {
                let a = self.am_dp_ind(bus);
                self.write_m(bus, a, self.a);
            }
            0x93 => {
                let a = self.am_sr_ind_y(bus);
                self.write_m(bus, a, self.a);
            }
            0x95 => {
                let a = self.am_dp_x(bus);
                self.write_m(bus, a, self.a);
            }
            0x97 => {
                let a = self.am_dp_ind_long_y(bus);
                self.write_m(bus, a, self.a);
            }
            0x99 => {
                let a = self.am_abs_y(bus);
                self.write_m(bus, a, self.a);
            }
            0x9D => {
                let a = self.am_abs_x(bus);
                self.write_m(bus, a, self.a);
            }
            0x9F => {
                let a = self.am_long_x(bus);
                self.write_m(bus, a, self.a);
            }
            // LDA
            0xA1 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xA3 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xA5 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xA7 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xA9 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xAD => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xAF => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB1 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB2 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB3 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB5 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB7 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xB9 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xBD => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            0xBF => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.set_a(v);
            }
            // CMP
            0xC1 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xC3 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xC5 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xC7 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xC9 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xCD => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xCF => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD1 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD2 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD3 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD5 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD7 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xD9 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xDD => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            0xDF => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_cmp(self.a, v, self.m8());
            }
            // SBC
            0xE1 => {
                let a = self.am_dp_x_ind(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xE3 => {
                let a = self.am_sr(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xE5 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xE7 => {
                let a = self.am_dp_ind_long(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xE9 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xED => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xEF => {
                let a = self.am_long(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF1 => {
                let a = self.am_dp_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF2 => {
                let a = self.am_dp_ind(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF3 => {
                let a = self.am_sr_ind_y(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF5 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF7 => {
                let a = self.am_dp_ind_long_y(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xF9 => {
                let a = self.am_abs_y(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xFD => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            0xFF => {
                let a = self.am_long_x(bus);
                let v = self.read_m(bus, a);
                self.op_sbc(v);
            }
            // Shifts and rotates
            0x0A => self.rmw_acc(Self::rmw_asl),
            0x06 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_asl);
            }
            0x16 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_asl);
            }
            0x0E => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_asl);
            }
            0x1E => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_asl);
            }
            0x2A => self.rmw_acc(Self::rmw_rol),
            0x26 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_rol);
            }
            0x36 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_rol);
            }
            0x2E => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_rol);
            }
            0x3E => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_rol);
            }
            0x4A => self.rmw_acc(Self::rmw_lsr),
            0x46 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_lsr);
            }
            0x56 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_lsr);
            }
            0x4E => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_lsr);
            }
            0x5E => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_lsr);
            }
            0x6A => self.rmw_acc(Self::rmw_ror),
            0x66 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_ror);
            }
            0x76 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_ror);
            }
            0x6E => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_ror);
            }
            0x7E => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_ror);
            }
            // INC / DEC
            0x1A => self.rmw_acc(Self::rmw_inc),
            0xE6 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_inc);
            }
            0xF6 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_inc);
            }
            0xEE => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_inc);
            }
            0xFE => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_inc);
            }
            0x3A => self.rmw_acc(Self::rmw_dec),
            0xC6 => {
                let a = self.am_dp(bus);
                self.rmw_mem(bus, a, Self::rmw_dec);
            }
            0xD6 => {
                let a = self.am_dp_x(bus);
                self.rmw_mem(bus, a, Self::rmw_dec);
            }
            0xCE => {
                let a = self.am_abs(bus);
                self.rmw_mem(bus, a, Self::rmw_dec);
            }
            0xDE => {
                let a = self.am_abs_x(bus);
                self.rmw_mem(bus, a, Self::rmw_dec);
            }
            0xE8 => {
                let v = self.x.wrapping_add(1);
                self.set_x(v);
            }
            0xC8 => {
                let v = self.y.wrapping_add(1);
                self.set_y(v);
            }
            0xCA => {
                let v = self.x.wrapping_sub(1);
                self.set_x(v);
            }
            0x88 => {
                let v = self.y.wrapping_sub(1);
                self.set_y(v);
            }
            // TSB / TRB
            0x04 | 0x0C => {
                let a = if opcode == 0x04 {
                    self.am_dp(bus)
                } else {
                    self.am_abs(bus)
                };
                let v = self.read_m(bus, a);
                let acc = if self.m8() { self.a & 0xFF } else { self.a };
                self.set_flag(Flags::Z, (v & acc) == 0);
                self.write_m(bus, a, v | acc);
            }
            0x14 | 0x1C => {
                let a = if opcode == 0x14 {
                    self.am_dp(bus)
                } else {
                    self.am_abs(bus)
                };
                let v = self.read_m(bus, a);
                let acc = if self.m8() { self.a & 0xFF } else { self.a };
                self.set_flag(Flags::Z, (v & acc) == 0);
                self.write_m(bus, a, v & !acc);
            }
            // BIT
            0x24 => {
                let a = self.am_dp(bus);
                let v = self.read_m(bus, a);
                self.op_bit(v);
            }
            0x2C => {
                let a = self.am_abs(bus);
                let v = self.read_m(bus, a);
                self.op_bit(v);
            }
            0x34 => {
                let a = self.am_dp_x(bus);
                let v = self.read_m(bus, a);
                self.op_bit(v);
            }
            0x3C => {
                let a = self.am_abs_x(bus);
                let v = self.read_m(bus, a);
                self.op_bit(v);
            }
            0x89 => {
                let a = self.am_imm_m(bus);
                let v = self.read_m(bus, a);
                let acc = if self.m8() { self.a & 0xFF } else { self.a };
                self.set_flag(Flags::Z, (v & acc) == 0);
            }
            // LDX / LDY / STX / STY / STZ
            0xA2 => {
                let a = self.am_imm_x(bus);
                let v = self.read_x(bus, a);
                self.set_x(v);
            }
            0xA6 => {
                let a = self.am_dp(bus);
                let v = self.read_x(bus, a);
                self.set_x(v);
            }
            0xAE => {
                let a = self.am_abs(bus);
                let v = self.read_x(bus, a);
                self.set_x(v);
            }
            0xB6 => {
                let a = self.am_dp_y(bus);
                let v = self.read_x(bus, a);
                self.set_x(v);
            }
            0xBE => {
                let a = self.am_abs_y(bus);
                let v = self.read_x(bus, a);
                self.set_x(v);
            }
            0xA0 => {
                let a = self.am_imm_x(bus);
                let v = self.read_x(bus, a);
                self.set_y(v);
            }
            0xA4 => {
                let a = self.am_dp(bus);
                let v = self.read_x(bus, a);
                self.set_y(v);
            }
            0xAC => {
                let a = self.am_abs(bus);
                let v = self.read_x(bus, a);
                self.set_y(v);
            }
            0xB4 => {
                let a = self.am_dp_x(bus);
                let v = self.read_x(bus, a);
                self.set_y(v);
            }
            0xBC => {
                let a = self.am_abs_x(bus);
                let v = self.read_x(bus, a);
                self.set_y(v);
            }
            0x86 => {
                let a = self.am_dp(bus);
                self.write_x(bus, a, self.x);
            }
            0x8E => {
                let a = self.am_abs(bus);
                self.write_x(bus, a, self.x);
            }
            0x96 => {
                let a = self.am_dp_y(bus);
                self.write_x(bus, a, self.x);
            }
            0x84 => {
                let a = self.am_dp(bus);
                self.write_x(bus, a, self.y);
            }
            0x8C => {
                let a = self.am_abs(bus);
                self.write_x(bus, a, self.y);
            }
            0x94 => {
                let a = self.am_dp_x(bus);
                self.write_x(bus, a, self.y);
            }
            0x64 => {
                let a = self.am_dp(bus);
                self.write_m(bus, a, 0);
            }
            0x74 => {
                let a = self.am_dp_x(bus);
                self.write_m(bus, a, 0);
            }
            0x9C => {
                let a = self.am_abs(bus);
                self.write_m(bus, a, 0);
            }
            0x9E => {
                let a = self.am_abs_x(bus);
                self.write_m(bus, a, 0);
            }
            // CPX / CPY
            0xE0 => {
                let a = self.am_imm_x(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.x, v, self.x8());
            }
            0xE4 => {
                let a = self.am_dp(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.x, v, self.x8());
            }
            0xEC => {
                let a = self.am_abs(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.x, v, self.x8());
            }
            0xC0 => {
                let a = self.am_imm_x(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.y, v, self.x8());
            }
            0xC4 => {
                let a = self.am_dp(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.y, v, self.x8());
            }
            0xCC => {
                let a = self.am_abs(bus);
                let v = self.read_x(bus, a);
                self.op_cmp(self.y, v, self.x8());
            }
            // Branches
            0x10 => self.branch(bus, !self.flag(Flags::N)),
            0x30 => self.branch(bus, self.flag(Flags::N)),
            0x50 => self.branch(bus, !self.flag(Flags::V)),
            0x70 => self.branch(bus, self.flag(Flags::V)),
            0x80 => self.branch(bus, true),
            0x90 => self.branch(bus, !self.flag(Flags::C)),
            0xB0 => self.branch(bus, self.flag(Flags::C)),
            0xD0 => self.branch(bus, !self.flag(Flags::Z)),
            0xF0 => self.branch(bus, self.flag(Flags::Z)),
            0x82 => {
                let rel = self.fetch16(bus);
                self.pc = self.pc.wrapping_add(rel);
            }
            // Jumps and calls
            0x4C => {
                self.pc = self.fetch16(bus);
                if self.pc <= op_pc {
                    self.note_loop();
                }
            }
            0x5C => {
                let t = self.fetch24(bus);
                self.pb = (t >> 16) as u8;
                self.pc = t as u16;
            }
            0x6C => {
                let p = self.fetch16(bus);
                self.pc = self.read16_bank0(bus, p);
            }
            0x7C => {
                let p = self.fetch16(bus).wrapping_add(self.x);
                let a = ((self.pb as u32) << 16) | p as u32 | WRAPS_IN_BANK;
                self.pc = self.read16(bus, a);
            }
            0xDC => {
                let p = self.fetch16(bus);
                let t = self.read24_bank0(bus, p);
                self.pb = (t >> 16) as u8;
                self.pc = t as u16;
            }
            0x20 => {
                let t = self.fetch16(bus);
                let ret = self.pc.wrapping_sub(1);
                self.push16(bus, ret);
                self.pc = t;
            }
            0x22 => {
                let t = self.fetch24(bus);
                let ret = self.pc.wrapping_sub(1);
                self.push8(bus, self.pb);
                self.push16(bus, ret);
                self.pb = (t >> 16) as u8;
                self.pc = t as u16;
            }
            0xFC => {
                let p = self.fetch16(bus);
                let ret = self.pc.wrapping_sub(1);
                self.push16(bus, ret);
                let a = ((self.pb as u32) << 16) | p.wrapping_add(self.x) as u32 | WRAPS_IN_BANK;
                self.pc = self.read16(bus, a);
            }
            0x60 => {
                self.pc = self.pull16(bus).wrapping_add(1);
            }
            0x6B => {
                self.pc = self.pull16(bus).wrapping_add(1);
                self.pb = self.pull8(bus);
            }
            0x40 => {
                let p = self.pull8(bus);
                self.set_p(p);
                self.pc = self.pull16(bus);
                if !self.emulation {
                    self.pb = self.pull8(bus);
                }
            }
            // Stack
            0x48 => {
                if self.m8() {
                    self.push8(bus, self.a as u8)
                } else {
                    self.push16(bus, self.a)
                }
            }
            0x68 => {
                let v = if self.m8() {
                    self.pull8(bus) as u16
                } else {
                    self.pull16(bus)
                };
                self.set_a(v);
            }
            0xDA => {
                if self.x8() {
                    self.push8(bus, self.x as u8)
                } else {
                    self.push16(bus, self.x)
                }
            }
            0xFA => {
                let v = if self.x8() {
                    self.pull8(bus) as u16
                } else {
                    self.pull16(bus)
                };
                self.set_x(v);
            }
            0x5A => {
                if self.x8() {
                    self.push8(bus, self.y as u8)
                } else {
                    self.push16(bus, self.y)
                }
            }
            0x7A => {
                let v = if self.x8() {
                    self.pull8(bus) as u16
                } else {
                    self.pull16(bus)
                };
                self.set_y(v);
            }
            0x08 => self.push8(bus, self.p),
            0x28 => {
                let p = self.pull8(bus);
                self.set_p(p);
            }
            0x8B => self.push8(bus, self.db),
            0xAB => {
                self.db = self.pull8(bus);
                self.set_nz(self.db as u16, true);
            }
            0x0B => self.push16(bus, self.dp),
            0x2B => {
                self.dp = self.pull16(bus);
                self.set_nz(self.dp, false);
            }
            0x4B => self.push8(bus, self.pb),
            0x62 => {
                let rel = self.fetch16(bus);
                let v = self.pc.wrapping_add(rel);
                self.push16(bus, v);
            }
            0xD4 => {
                let d = self.dp_base(bus);
                let v = self.read16_bank0(bus, d);
                self.push16(bus, v);
            }
            0xF4 => {
                let v = self.fetch16(bus);
                self.push16(bus, v);
            }
            // Transfers
            0xAA => {
                let v = self.a;
                self.set_x(v);
            }
            0xA8 => {
                let v = self.a;
                self.set_y(v);
            }
            0x8A => {
                let v = self.x;
                self.set_a(v);
            }
            0x98 => {
                let v = self.y;
                self.set_a(v);
            }
            0x9A => {
                self.sp = if self.emulation {
                    0x0100 | (self.x & 0xFF)
                } else {
                    self.x
                };
            }
            0xBA => {
                let v = self.sp;
                self.set_x(v);
            }
            0x9B => {
                let v = self.x;
                self.set_y(v);
            }
            0xBB => {
                let v = self.y;
                self.set_x(v);
            }
            0x5B => {
                self.dp = self.a;
                self.set_nz(self.dp, false);
            }
            0x7B => {
                self.a = self.dp;
                self.set_nz(self.a, false);
            }
            0x1B => {
                self.sp = if self.emulation {
                    0x0100 | (self.a & 0xFF)
                } else {
                    self.a
                };
            }
            0x3B => {
                self.a = self.sp;
                self.set_nz(self.a, false);
            }
            0xEB => {
                self.a = self.a.rotate_right(8);
                self.set_nz(self.a, true);
            }
            0xFB => {
                let c = self.flag(Flags::C);
                self.set_flag(Flags::C, self.emulation);
                self.emulation = c;
                if self.emulation {
                    self.p |= Flags::M | Flags::X;
                    self.x &= 0xFF;
                    self.y &= 0xFF;
                    self.sp = 0x0100 | (self.sp & 0xFF);
                }
            }
            // Flags
            0x18 => self.set_flag(Flags::C, false),
            0x38 => self.set_flag(Flags::C, true),
            0x58 => self.set_flag(Flags::I, false),
            0x78 => self.set_flag(Flags::I, true),
            0xB8 => self.set_flag(Flags::V, false),
            0xD8 => self.set_flag(Flags::D, false),
            0xF8 => self.set_flag(Flags::D, true),
            0xC2 => {
                let m = self.fetch8(bus);
                let p = self.p & !m;
                self.set_p(p);
            }
            0xE2 => {
                let m = self.fetch8(bus);
                let p = self.p | m;
                self.set_p(p);
            }
            // Block moves: A holds count-1 (always 16-bit), X source, Y destination.
            0x54 | 0x44 => {
                let dst_bank = self.fetch8(bus);
                let src_bank = self.fetch8(bus);
                self.db = dst_bank;
                let step: u16 = if opcode == 0x54 { 1 } else { 0xFFFF };
                loop {
                    let v = self.read8(bus, ((src_bank as u32) << 16) | self.x as u32);
                    self.write8(bus, ((dst_bank as u32) << 16) | self.y as u32, v);
                    self.x = self.x.wrapping_add(step);
                    self.y = self.y.wrapping_add(step);
                    if self.x8() {
                        self.x &= 0xFF;
                        self.y &= 0xFF;
                    }
                    if self.a == 0 {
                        self.a = 0xFFFF;
                        break;
                    }
                    self.a = self.a.wrapping_sub(1);
                }
            }
            // Misc
            0xEA => {}
            0x42 => {
                self.fetch8(bus);
            }
            0x00 => {
                return Err(CpuError::Brk {
                    at: CodeAddr::new(op_pb, op_pc),
                });
            }
            0x02 => {
                return Err(CpuError::Cop {
                    at: CodeAddr::new(op_pb, op_pc),
                });
            }
            0xCB => self.waiting = true,
            0xDB => {
                return Err(CpuError::Halted {
                    at: CodeAddr::new(op_pb, op_pc),
                });
            }
        }
        Ok(())
    }

    /// Takes an IRQ: pushes the return frame and continues at the bus's
    /// vector with interrupts disabled.
    pub fn interrupt(&mut self, bus: &mut impl Bus) {
        if self.emulation {
            self.push16(bus, self.pc);
            self.push8(bus, self.p & !Flags::X);
        } else {
            self.push8(bus, self.pb);
            self.push16(bus, self.pc);
            self.push8(bus, self.p);
        }
        self.p = (self.p | Flags::I) & !Flags::D;
        self.pb = 0;
        self.pc = bus.irq_vector(self.emulation);
        self.last_loop = None;
    }

    /// Runs until `done`, taking IRQs from the bus. A CPU that stops to
    /// wait (`WAI`, or a loop that polls memory without changing
    /// anything) hands over to [`Bus::wait`] and carries on if that got
    /// anywhere, or else takes the NMI [`Bus::vblank`] offers, as the
    /// console ends a wait for the vertical blank (up to
    /// [`WAIT_VBLANK_LIMIT`] times in one run); otherwise the run ends
    /// there, to be resumed by calling this again.
    pub fn run(
        &mut self,
        bus: &mut impl Bus,
        limit: u64,
        done: impl Fn(&Self) -> bool,
    ) -> Result<Run, CpuError> {
        let start = self.steps;
        let mut vblanks = 0;
        loop {
            // Before `done`: an IRQ raised as the routine returned is
            // still work it caused.
            if !self.flag(Flags::I) && bus.irq() {
                self.interrupt(bus);
            }
            if done(self) {
                return Ok(Run::Done);
            }
            if self.steps - start >= limit {
                return Err(CpuError::Limit {
                    limit,
                    at: CodeAddr::new(self.pb, self.pc),
                });
            }
            self.step(bus)?;
            if self.waiting {
                self.waiting = false;
                if bus.wait() {
                    continue;
                }
                match bus.vblank() {
                    Some(vector) if !self.emulation && vblanks < WAIT_VBLANK_LIMIT => {
                        vblanks += 1;
                        self.nmi(bus, vector);
                    }
                    _ => return Ok(Run::Waiting),
                }
            }
        }
    }

    /// Takes an NMI in native mode: pushes the return frame and continues
    /// at `vector` with interrupts disabled.
    fn nmi(&mut self, bus: &mut impl Bus, vector: u16) {
        self.push8(bus, self.pb);
        self.push16(bus, self.pc);
        self.push8(bus, self.p);
        self.p = (self.p | Flags::I) & !Flags::D;
        self.pb = 0;
        self.pc = vector;
        self.last_loop = None;
    }

    /// Bytes written so far: a measure of whether a run got anywhere.
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// [`Cpu::run`] for a caller with nothing to resume: a wait that
    /// nothing answers is an error.
    fn run_to(
        &mut self,
        bus: &mut impl Bus,
        limit: u64,
        done: impl Fn(&Self) -> bool,
    ) -> Result<(), CpuError> {
        match self.run(bus, limit, done)? {
            Run::Done => Ok(()),
            Run::Waiting => Err(CpuError::Waiting {
                at: CodeAddr::new(self.pb, self.pc),
            }),
        }
    }

    /// True once the routine entered by [`Cpu::enter`] or
    /// [`Cpu::enter_jsr`] has returned.
    pub fn returned(&self) -> bool {
        (self.pb == RETURN_PB && self.pc == RETURN_PC)
            || (self.pc == RETURN_PC && Some(self.sp) == self.return_sp)
    }

    /// Enters a subroutine as if by `JSL` from the sentinel address, so
    /// that its `RTL` leaves the CPU [`Cpu::returned`].
    pub fn enter(&mut self, bus: &mut impl Bus, addr: u32) {
        self.push8(bus, RETURN_PB);
        self.push16(bus, RETURN_PC.wrapping_sub(1));
        self.pb = (addr >> 16) as u8;
        self.pc = addr as u16;
        self.return_sp = None;
    }

    /// Enters the interrupt handler at `handler` in bank 0 as if the
    /// interrupt had come at the sentinel address, in native mode, so
    /// that its `RTI` leaves the CPU [`Cpu::returned`].
    pub fn enter_handler(&mut self, bus: &mut impl Bus, handler: u16) {
        self.push8(bus, RETURN_PB);
        self.push16(bus, RETURN_PC);
        self.push8(bus, self.p);
        self.p = (self.p | Flags::I) & !Flags::D;
        self.pb = 0;
        self.pc = handler;
        self.return_sp = None;
    }

    /// Enters a subroutine as if by `JSR` from its own bank. An `RTS`
    /// stays in the routine's bank, so it cannot land on the bank `$FF`
    /// sentinel; the return is recognised instead by the program counter
    /// reaching the sentinel offset with the stack back where the call
    /// found it. A `JSL` frame sits under the `JSR` one, so a routine
    /// that ends in `RTL` after all returns to the sentinel proper.
    pub fn enter_jsr(&mut self, bus: &mut impl Bus, addr: u32) {
        self.enter(bus, addr);
        self.return_sp = Some(self.sp);
        self.push16(bus, RETURN_PC.wrapping_sub(1));
    }

    /// Runs the routine entered until it returns, or until the program
    /// counter gets to one of `stop` (without executing it) if that comes
    /// first. True if it has returned; otherwise the call is still open,
    /// and calling this again carries on with it: a `stop` it is already
    /// standing on is one to come round to again, not one reached.
    pub fn finish(
        &mut self,
        bus: &mut impl Bus,
        limit: u64,
        stop: &[u32],
    ) -> Result<bool, CpuError> {
        let start = self.steps;
        self.run_to(bus, limit, |cpu| {
            cpu.returned() || (stop.contains(&cpu.pc_addr()) && cpu.steps != start)
        })?;
        if !self.returned() {
            return Ok(false);
        }
        // An `RTS` leaves the `JSL` frame behind.
        if self.return_sp.take() == Some(self.sp) {
            self.sp = self.sp.wrapping_add(3);
        }
        Ok(true)
    }

    /// Calls a subroutine as if by `JSL` and runs until it returns with
    /// `RTL`, or until `limit` instructions have executed.
    pub fn call(&mut self, bus: &mut impl Bus, addr: u32, limit: u64) -> Result<(), CpuError> {
        self.enter(bus, addr);
        self.finish(bus, limit, &[]).map(|_| ())
    }

    /// Calls a subroutine as if by `JSR` from its own bank and runs until
    /// it returns with `RTS`.
    pub fn call_jsr(&mut self, bus: &mut impl Bus, addr: u32, limit: u64) -> Result<(), CpuError> {
        self.enter_jsr(bus, addr);
        self.finish(bus, limit, &[]).map(|_| ())
    }

    /// Starts executing at `start` and stops when the program counter
    /// reaches `stop` (a 24-bit address), without executing it.
    pub fn run_until(
        &mut self,
        bus: &mut impl Bus,
        start: u32,
        stop: &[u32],
        limit: u64,
    ) -> Result<(), CpuError> {
        self.pb = (start >> 16) as u8;
        self.pc = start as u16;
        self.run_to(bus, limit, |cpu| stop.contains(&cpu.pc_addr()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_report_names_the_instruction_only_when_given_it() {
        assert_eq!(
            watch_report(
                0x7E_1693,
                0x45,
                Watched::At(0x02_BA9B, 0x1245, 0x0003, 0x0000)
            ),
            "watch: $7E1693 <- $45 at $02BA9B (A=1245 X=0003 Y=0000)"
        );
        assert_eq!(
            watch_report(0x7E_1693, 0x45, Watched::Alone),
            "watch: $7E1693 <- $45"
        );
        assert!(!watch_report(0x7E_01F0, 0x45, Watched::Stack).contains("45"));
    }

    /// 16 MiB flat RAM for tests.
    struct Ram(Vec<u8>);

    impl Bus for Ram {
        fn read(&mut self, addr: u32) -> u8 {
            self.0[addr as usize]
        }
        fn write(&mut self, addr: u32, value: u8) {
            self.0[addr as usize] = value;
        }
    }

    /// Flat RAM whose vertical blank is an NMI handler at `$9000`.
    struct Console(Ram);

    impl Bus for Console {
        fn read(&mut self, addr: u32) -> u8 {
            self.0.read(addr)
        }
        fn write(&mut self, addr: u32, value: u8) {
            self.0.write(addr, value)
        }
        fn vblank(&mut self) -> Option<u16> {
            Some(0x9000)
        }
    }

    #[test]
    fn a_wait_for_the_vertical_blank_takes_the_nmi() {
        let mut bus = Console(Ram(vec![0; 1 << 24]));
        // Wait for the NMI to set $10, then count the frame and return.
        let code = [0xA5, 0x10, 0xF0, 0xFC, 0x64, 0x10, 0xE6, 0x13, 0x6B];
        bus.0.0[0x8000..0x8000 + code.len()].copy_from_slice(&code);
        // The handler: INC $10 ; RTI.
        bus.0.0[0x9000..0x9003].copy_from_slice(&[0xE6, 0x10, 0x40]);
        let mut cpu = Cpu::new();
        cpu.call(&mut bus, 0x8000, 10_000).unwrap();
        assert_eq!(bus.0.0[0x13], 1);
        assert_eq!(bus.0.0[0x10], 0);
        assert_eq!(cpu.sp, 0x01FF);
        // A handler that changes nothing the loop looks at is not run
        // forever: the wait is given up after a few blanks.
        bus.0.0[0x9000] = 0xEA; // NOP
        bus.0.0[0x9001] = 0xEA;
        bus.0.0[0x9002] = 0x40;
        let mut cpu = Cpu::new();
        let error = cpu.call(&mut bus, 0x8000, 10_000).unwrap_err();
        assert_eq!(
            error,
            CpuError::Waiting {
                at: CodeAddr::new(0, 0x8000)
            }
        );
    }

    fn run(code: &[u8], setup: impl FnOnce(&mut Cpu, &mut Ram)) -> (Cpu, Ram) {
        let mut ram = Ram(vec![0; 0x100_0000]);
        let mut cpu = Cpu::new();
        // Place code at $80:8000, end with RTL.
        let base = 0x80_8000usize;
        ram.0[base..base + code.len()].copy_from_slice(code);
        ram.0[base + code.len()] = 0x6B;
        setup(&mut cpu, &mut ram);
        cpu.call(&mut ram, 0x80_8000, 100_000).unwrap();
        (cpu, ram)
    }

    /// RAM with something else on the bus: each wait it is asked for,
    /// it has an answer until `answers` run out. An answer stores 1 at
    /// `$0010`, or raises the IRQ line if `by_irq`.
    struct Shared {
        ram: Ram,
        answers: u32,
        by_irq: bool,
        irq: bool,
    }

    impl Bus for Shared {
        fn read(&mut self, addr: u32) -> u8 {
            self.ram.read(addr)
        }
        fn write(&mut self, addr: u32, value: u8) {
            if addr == 0x4000 {
                self.irq = false; // the handler's acknowledgement
            }
            self.ram.write(addr, value);
        }
        fn irq(&mut self) -> bool {
            self.irq
        }
        fn wait(&mut self) -> bool {
            if self.answers == 0 {
                return false;
            }
            self.answers -= 1;
            if self.by_irq {
                self.irq = true;
            } else {
                self.ram.0[0x10] = 1;
            }
            true
        }
    }

    /// `code` at `$8000`, and an IRQ handler at `$9000` that acknowledges,
    /// stores 1 at `$0010`, and returns.
    fn shared(code: &[u8], answers: u32, by_irq: bool) -> Shared {
        let mut ram = Ram(vec![0; 0x100_0000]);
        ram.0[0x8000..0x8000 + code.len()].copy_from_slice(code);
        let handler = [0x8D, 0x00, 0x40, 0xA9, 0x01, 0x85, 0x10, 0x40];
        ram.0[0x9000..0x9008].copy_from_slice(&handler); // STA $4000 : LDA #1 : STA $10 : RTI
        ram.0[0xFFEE..0xFFF0].copy_from_slice(&[0x00, 0x90]);
        Shared {
            ram,
            answers,
            by_irq,
            irq: false,
        }
    }

    /// `- LDA $10 : BEQ - : RTL`
    const POLL: [u8; 5] = [0xA5, 0x10, 0xF0, 0xFC, 0x6B];

    #[test]
    fn a_polling_loop_waits_for_the_rest_of_the_bus() {
        // Nothing answers: the second identical pass round the loop is
        // the last, not the two hundred millionth.
        let mut bus = shared(&POLL, 0, false);
        let mut cpu = Cpu::new();
        let error = cpu.call(&mut bus, 0x8000, 1_000_000).unwrap_err();
        assert_eq!(
            error,
            CpuError::Waiting {
                at: CodeAddr::new(0, 0x8000)
            }
        );
        assert!(cpu.steps < 10);

        let mut bus = shared(&POLL, 1, false);
        let mut cpu = Cpu::new();
        cpu.call(&mut bus, 0x8000, 1_000_000).unwrap();
        assert_eq!(bus.answers, 0);
    }

    #[test]
    fn a_loop_that_gets_somewhere_is_not_a_wait() {
        // `LDX #0 : - DEX : BNE - : RTL` and `- DEC $10 : BNE - : RTL`:
        // the same place every time round, a register or memory changed.
        for code in [
            &[0xA2, 0x00, 0xCA, 0xD0, 0xFD, 0x6B][..],
            &[0xC6, 0x10, 0xD0, 0xFC, 0x6B],
        ] {
            let mut bus = shared(code, 0, false);
            Cpu::new().call(&mut bus, 0x8000, 1_000_000).unwrap();
        }
    }

    #[test]
    fn an_irq_is_taken_once_interrupts_are_on() {
        // `CLI`, then the poll; the answer comes as an IRQ, whose handler
        // returns into the loop.
        let code = [&[0x58][..], &POLL].concat();
        let mut bus = shared(&code, 1, true);
        let mut cpu = Cpu::new();
        let sp = cpu.sp;
        cpu.call(&mut bus, 0x8000, 1_000_000).unwrap();
        assert_eq!((bus.ram.0[0x10], bus.irq), (1, false));
        assert_eq!(cpu.sp, sp);
        assert_eq!(cpu.p & Flags::I, 0); // as `RTI` restored it

        // With interrupts off the line goes unanswered, and so the wait.
        let mut bus = shared(&POLL, 1, true);
        let error = Cpu::new().call(&mut bus, 0x8000, 1_000_000).unwrap_err();
        assert!(matches!(error, CpuError::Waiting { .. }));
        assert!(bus.irq);
    }

    #[test]
    fn wai_waits_for_an_interrupt() {
        let code = [0x58, 0xCB, 0x6B]; // CLI : WAI : RTL
        let mut bus = shared(&code, 1, true);
        Cpu::new().call(&mut bus, 0x8000, 100).unwrap();
        assert_eq!(bus.ram.0[0x10], 1);
        let mut bus = shared(&code, 0, true);
        let error = Cpu::new().call(&mut bus, 0x8000, 100).unwrap_err();
        assert_eq!(
            error,
            CpuError::Waiting {
                at: CodeAddr::new(0, 0x8002)
            }
        );
    }

    #[test]
    fn a_call_can_stop_on_the_way_and_be_finished() {
        // `JSR $8010 : RTS`, and at `$8010` `INC $10 : RTS`.
        let mut ram = Ram(vec![0; 0x100_0000]);
        ram.0[0x8000..0x8004].copy_from_slice(&[0x20, 0x10, 0x80, 0x60]);
        ram.0[0x8010..0x8013].copy_from_slice(&[0xE6, 0x10, 0x60]);
        let mut cpu = Cpu::new();
        let sp = cpu.sp;
        cpu.enter_jsr(&mut ram, 0x8000);
        assert!(!cpu.finish(&mut ram, 100, &[0x8010]).unwrap());
        assert_eq!((ram.0[0x10], cpu.pc), (0, 0x8010));
        assert!(cpu.finish(&mut ram, 100, &[0x8010]).unwrap());
        assert_eq!((ram.0[0x10], cpu.sp), (1, sp));
        // A stop it never reaches is no different from none.
        cpu.enter_jsr(&mut ram, 0x8000);
        assert!(cpu.finish(&mut ram, 100, &[0x9999]).unwrap());
        assert_eq!((ram.0[0x10], cpu.sp), (2, sp));
    }

    #[test]
    fn lda_sta_8bit() {
        let (cpu, ram) = run(&[0xA9, 0x42, 0x8D, 0x00, 0x10], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0x42);
        assert_eq!(ram.0[0x1000], 0x42);
        assert!(!cpu.p & Flags::Z != 0 || cpu.p & Flags::Z == 0);
    }

    #[test]
    fn rep_sep_widths() {
        // REP #$30 ; LDA #$1234 ; STA $1000 ; SEP #$20 ; LDA #$56 ; STA $1002
        let (cpu, ram) = run(
            &[
                0xC2, 0x30, 0xA9, 0x34, 0x12, 0x8D, 0x00, 0x10, 0xE2, 0x20, 0xA9, 0x56, 0x8D, 0x02,
                0x10,
            ],
            |_, _| {},
        );
        assert_eq!(ram.0[0x1000], 0x34);
        assert_eq!(ram.0[0x1001], 0x12);
        assert_eq!(ram.0[0x1002], 0x56);
        assert_eq!(cpu.a, 0x1256);
    }

    #[test]
    fn adc_sbc_flags() {
        // CLC ; LDA #$7F ; ADC #$01 -> $80, V set, N set, C clear
        let (cpu, _) = run(&[0x18, 0xA9, 0x7F, 0x69, 0x01], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0x80);
        assert!(cpu.p & Flags::V != 0);
        assert!(cpu.p & Flags::N != 0);
        assert!(cpu.p & Flags::C == 0);
        // SEC ; LDA #$10 ; SBC #$20 -> $F0, C clear (borrow)
        let (cpu, _) = run(&[0x38, 0xA9, 0x10, 0xE9, 0x20], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0xF0);
        assert!(cpu.p & Flags::C == 0);
        // SEC ; LDA #$50 ; SBC #$20 -> $30, C set
        let (cpu, _) = run(&[0x38, 0xA9, 0x50, 0xE9, 0x20], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0x30);
        assert!(cpu.p & Flags::C != 0);
    }

    #[test]
    fn decimal_adc() {
        // SED ; CLC ; LDA #$19 ; ADC #$01 -> $20
        let (cpu, _) = run(&[0xF8, 0x18, 0xA9, 0x19, 0x69, 0x01], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0x20);
        // SED ; SEC ; LDA #$20 ; SBC #$01 -> $19
        let (cpu, _) = run(&[0xF8, 0x38, 0xA9, 0x20, 0xE9, 0x01], |_, _| {});
        assert_eq!(cpu.a & 0xFF, 0x19);
    }

    #[test]
    fn jsr_rts_and_loops() {
        // LDX #$05 ; loop: DEX ; BNE loop ; JSR sub ; RTL... sub: LDA #$99 ; RTS
        let code = [
            0xA2, 0x05, // LDX #5
            0xCA, // DEX
            0xD0, 0xFD, // BNE -3
            0x20, 0x0A, 0x80, // JSR $800A
            0x6B, // RTL (unused, call() returns on the sentinel)
            0xEA, // pad
            0xA9, 0x99, // sub: LDA #$99
            0x60, // RTS
        ];
        let (cpu, _) = run(&code, |_, _| {});
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.a & 0xFF, 0x99);
    }

    #[test]
    fn indirect_long_and_index() {
        // REP #$10 ; LDX #$0002 ; LDA [$10],Y with Y=1 ; pointer at DP $10 -> $7E2000
        let (cpu, _) = run(&[0xC2, 0x10, 0xA0, 0x01, 0x00, 0xB7, 0x10], |_, ram| {
            ram.0[0x10] = 0x00;
            ram.0[0x11] = 0x20;
            ram.0[0x12] = 0x7E;
            ram.0[0x7E2001] = 0xAB;
        });
        assert_eq!(cpu.a & 0xFF, 0xAB);
    }

    #[test]
    fn mvn_block_move() {
        // REP #$30 ; LDA #$0003 ; LDX #$1000 ; LDY #$2000 ; MVN $7E,$7E
        let (cpu, ram) = run(
            &[
                0xC2, 0x30, 0xA9, 0x03, 0x00, 0xA2, 0x00, 0x10, 0xA0, 0x00, 0x20, 0x54, 0x7E, 0x7E,
            ],
            |_, ram| {
                ram.0[0x7E1000..0x7E1004].copy_from_slice(&[1, 2, 3, 4]);
            },
        );
        assert_eq!(&ram.0[0x7E2000..0x7E2004], &[1, 2, 3, 4]);
        assert_eq!(cpu.a, 0xFFFF);
        assert_eq!(cpu.x, 0x1004);
        assert_eq!(cpu.y, 0x2004);
    }

    #[test]
    fn xba_and_rotates() {
        // LDA #$12 ; XBA ; -> A = $1200 (8-bit view 0), then ROL A with carry
        let (cpu, _) = run(&[0xA9, 0x12, 0xEB, 0x38, 0x2A], |_, _| {});
        assert_eq!(cpu.a, 0x1201);
    }
}
