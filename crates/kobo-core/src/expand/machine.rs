//! The CPU and bus a capture runs on, and how its routines are called.

use super::{ExpandError, routines};
use crate::cpu::smw_bus::SmwBus;
use crate::cpu::{Bus, Cpu, CpuError, Flags};
use crate::rom::Rom;

/// Instruction limit for the game's own loading and per-frame routines.
const STEP_LIMIT: u64 = 200_000_000;
/// Instruction limit for an interrupt handler.
const HANDLER_STEP_LIMIT: u64 = 1_000_000;
/// Instruction limit for the short lookups a tool hooks into the game.
pub(super) const LOOKUP_STEP_LIMIT: u64 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Return {
    Rtl,
    Rts,
}

/// One call into the ROM: the routine and the register state its callers
/// give it. Every call starts from freshly reset registers (8-bit
/// accumulator and index, data bank zero, the direct page on the game's
/// own, interrupts enabled as they are outside the game's handlers) plus
/// whatever is set here, so no routine sees what the previous one left
/// behind.
#[derive(Clone, Copy, Debug)]
pub(super) struct Call {
    addr: u32,
    returns: Return,
    wide_accumulator: bool,
    wide_index: bool,
    data_bank: u8,
    accumulator: u16,
    index_x: u16,
    index_y: u16,
    limit: u64,
}

impl Call {
    const fn new(addr: u32, returns: Return) -> Self {
        Self {
            addr,
            returns,
            wide_accumulator: false,
            wide_index: false,
            data_bank: 0,
            accumulator: 0,
            index_x: 0,
            index_y: 0,
            limit: STEP_LIMIT,
        }
    }

    /// A routine that returns with `RTL`.
    pub const fn jsl(addr: u32) -> Self {
        Self::new(addr, Return::Rtl)
    }

    /// A routine that returns with `RTS`.
    pub const fn jsr(addr: u32) -> Self {
        Self::new(addr, Return::Rts)
    }

    /// Enters with a 16-bit accumulator.
    pub const fn wide_accumulator(mut self) -> Self {
        self.wide_accumulator = true;
        self
    }

    /// Enters with 16-bit index registers.
    pub const fn wide_index(mut self) -> Self {
        self.wide_index = true;
        self
    }

    pub const fn data_bank(mut self, bank: u8) -> Self {
        self.data_bank = bank;
        self
    }

    pub const fn accumulator(mut self, value: u16) -> Self {
        self.accumulator = value;
        self
    }

    pub const fn index_x(mut self, value: u16) -> Self {
        self.index_x = value;
        self
    }

    pub const fn index_y(mut self, value: u16) -> Self {
        self.index_y = value;
        self
    }

    pub const fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }
}

/// The interrupts the console raises each frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Interrupt {
    /// Vertical blank.
    Nmi,
    /// The scanline the game asked for in `VTIME`.
    TimerIrq,
}

/// A CPU on a bus, loading or running one level.
pub(super) struct Machine<'r> {
    pub cpu: Cpu,
    pub bus: SmwBus<'r>,
    pub level: u16,
}

impl<'r> Machine<'r> {
    pub fn new(rom: &'r Rom, level: u16) -> Self {
        // The bus first: it checks the ROM (clean room) before the CPU
        // decides whether to keep a trace.
        let bus = SmwBus::new(rom);
        Self {
            cpu: Cpu::new(),
            bus,
            level,
        }
    }

    /// Debugging aid: with `KOBO_CPU_TRACE` set, prints the instructions
    /// that led to `error` on standard error.
    fn dump_history(&self, error: &CpuError) {
        self.cpu.print_history(&error.to_string());
    }

    /// The registers every entry into the ROM starts from.
    fn reset_registers(&mut self) {
        self.cpu.reset_registers();
        self.cpu.dp = self.bus.ram.map().direct_page();
        // The only IRQs there are come from an SA-1 asking for something.
        self.cpu.p &= !Flags::I;
    }

    /// Runs a routine to its return. The registers it left are in `cpu`.
    pub fn try_call(&mut self, call: Call) -> Result<(), CpuError> {
        self.try_call_to(call, None).map(|_| ())
    }

    /// Runs a routine until it returns or first reaches `stop`. False if
    /// it stopped there: the call is still open, for the caller to look
    /// at the machine and then [`Machine::finish_call`].
    pub fn try_call_to(&mut self, call: Call, stop: Option<u32>) -> Result<bool, CpuError> {
        self.reset_registers();
        if call.wide_accumulator {
            self.cpu.p &= !Flags::M;
        }
        if call.wide_index {
            self.cpu.p &= !Flags::X;
        }
        self.cpu.db = call.data_bank;
        self.cpu.a = call.accumulator;
        self.cpu.x = call.index_x;
        self.cpu.y = call.index_y;
        match call.returns {
            Return::Rtl => self.cpu.enter(&mut self.bus, call.addr),
            Return::Rts => self.cpu.enter_jsr(&mut self.bus, call.addr),
        }
        let stop = stop.map_or(Vec::new(), |stop| self.bus.code_mirrors(stop));
        self.finish(call.limit, &stop)
    }

    /// Runs the call [`Machine::try_call_to`] left open to its return.
    pub fn finish_call(&mut self, call: Call) -> Result<(), CpuError> {
        self.finish(call.limit, &[]).map(|_| ())
    }

    fn finish(&mut self, limit: u64, stop: &[u32]) -> Result<bool, CpuError> {
        let returned = self
            .cpu
            .finish(&mut self.bus, limit, stop)
            .map_err(|error| self.cause(error))?;
        if returned {
            // Work handed to an SA-1 that nothing waited for is still
            // part of the routine.
            self.bus.run_sa1();
        }
        Ok(returned)
    }

    /// An SA-1 that has stopped is why the S-CPU gave up waiting for it.
    fn cause(&self, error: CpuError) -> CpuError {
        let error = if matches!(error, CpuError::Operation(_)) {
            error
        } else {
            self.bus.sa1_fault().unwrap_or(error)
        };
        self.dump_history(&error);
        error
    }

    /// [`Machine::try_call`] for routines the level cannot load without.
    pub fn call(&mut self, call: Call) -> Result<(), ExpandError> {
        self.try_call(call).map_err(|source| self.error(source))
    }

    /// Runs the ROM's handler for `interrupt` from its vector to its
    /// `RTI`, as the console would between two passes of the game loop.
    /// There is no entering one further in or stopping it short: SA-1
    /// Pack and other patches replace both ends of the handlers.
    pub fn interrupt(&mut self, interrupt: Interrupt) -> Result<(), ExpandError> {
        self.try_interrupt(interrupt)
            .map_err(|source| self.error(source))
    }

    /// [`Machine::interrupt`] for the passes a level can do without.
    pub fn try_interrupt(&mut self, interrupt: Interrupt) -> Result<(), CpuError> {
        self.reset_registers();
        let handler = match interrupt {
            Interrupt::Nmi => self.bus.nmi_vector(),
            Interrupt::TimerIrq => {
                self.bus.raise_timer_irq();
                self.bus.irq_vector(false)
            }
        };
        self.cpu.enter_handler(&mut self.bus, handler);
        self.finish(HANDLER_STEP_LIMIT, &[]).map(|_| ())
    }

    /// Runs from power-on, through the cartridge's reset vector, until the
    /// program counter reaches `stop`.
    pub fn run_from_reset(&mut self, stop: u32, limit: u64) -> Result<(), ExpandError> {
        self.cpu.reset_registers();
        self.cpu.emulation = true;
        let vector = routines::RESET_VECTOR;
        let start = u16::from_le_bytes([self.bus.read(vector), self.bus.read(vector + 1)]);
        let stop = self.bus.code_mirrors(stop);
        self.cpu
            .run_until(&mut self.bus, start as u32, &stop, limit)
            .map_err(|source| self.error(self.cause(source)))
    }

    pub fn error(&self, source: CpuError) -> ExpandError {
        ExpandError::Cpu {
            level: self.level,
            source,
        }
    }
}
