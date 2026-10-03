//! Bounded reports of hardware accesses the headless bus does not model.
//!
//! What is reported is what could leave a picture incomplete: an enabled
//! HDMA, a PPU latch read, an SA-1 register outside the model. Accesses
//! with nothing to model (open bus, read-only registers, controllers,
//! registers whose effect the capture passes read from RAM) are counted
//! on the bus as stubs and never reported. Reporting changes nothing:
//! an unmodelled read still returns zero and an unmodelled write is
//! still dropped. Register addresses are canonical (bank zero), whichever
//! mirror was used.

use std::fmt;

use super::sa1::Processor;

/// Maximum distinct accesses retained per bus. Counts keep accumulating;
/// additional distinct accesses contribute to `omitted` without allocating.
pub const MAX_ACCESSES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Read,
    Write,
    /// An instruction's opcode or operand read from where nothing is: a
    /// routine ran off into unmapped memory. Where is withheld in a
    /// [`crate::clean_room::forbidden`] process, since that is where the
    /// ROM's code goes.
    Fetch,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Access {
    pub processor: Processor,
    pub address: u32,
    pub kind: AccessKind,
    pub count: u64,
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct UnsupportedAccesses {
    /// First-seen order, deterministic for a given execution.
    pub accesses: Vec<Access>,
    pub omitted: u64,
}

impl UnsupportedAccesses {
    pub fn is_empty(&self) -> bool {
        self.accesses.is_empty() && self.omitted == 0
    }

    pub(super) fn record(&mut self, processor: Processor, address: u32, kind: AccessKind) {
        if let Some(access) = self
            .accesses
            .iter_mut()
            .find(|a| a.processor == processor && a.address == address && a.kind == kind)
        {
            access.count = access.count.saturating_add(1);
        } else if self.accesses.len() < MAX_ACCESSES {
            self.accesses.push(Access {
                processor,
                address,
                kind,
                count: 1,
            });
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

impl fmt::Display for UnsupportedAccesses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unmodelled hardware accesses, so the picture may be incomplete: "
        )?;
        let mut accesses = self.accesses.iter().peekable();
        let mut first = true;
        while let Some(access) = accesses.next() {
            // Consecutive addresses reached the same way, as a block
            // copy or a probe leaves them, are one range.
            let (mut end, mut count) = (access.address, access.count);
            while let Some(next) = accesses.next_if(|next| {
                next.processor == access.processor
                    && next.kind == access.kind
                    && next.address == end + 1
            }) {
                end = next.address;
                count += next.count;
            }
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            let cpu = match access.processor {
                Processor::Main => "S-CPU",
                Processor::Sa1 => "SA-1",
            };
            let kind = match access.kind {
                AccessKind::Read => "read",
                AccessKind::Write => "write",
                AccessKind::Fetch => "instruction fetch",
            };
            if access.kind == AccessKind::Fetch && crate::clean_room::forbidden() {
                write!(
                    f,
                    "{cpu} {kind} from unmapped memory (address withheld: clean room)"
                )?;
            } else {
                write!(f, "{cpu} {kind} ${:06X}", access.address)?;
                if end != access.address {
                    write!(f, "-${end:06X}")?;
                }
            }
            match count {
                1 => write!(f, " (once)")?,
                n => write!(f, " ({n} times)")?,
            }
        }
        if self.omitted > 0 {
            write!(f, "; {} further accesses omitted", self.omitted)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_bounded_without_losing_repeat_counts() {
        let mut report = UnsupportedAccesses::default();
        for address in 0..100 {
            report.record(Processor::Main, address, AccessKind::Read);
        }
        report.record(Processor::Main, 0, AccessKind::Read);
        assert_eq!(report.accesses.len(), MAX_ACCESSES);
        assert_eq!(report.accesses[0].count, 2);
        assert_eq!(report.omitted, 68);
    }

    #[test]
    fn consecutive_addresses_display_as_ranges() {
        let mut report = UnsupportedAccesses::default();
        for address in 0x3800..0x3808 {
            report.record(Processor::Main, address, AccessKind::Read);
        }
        report.record(Processor::Main, 0x420C, AccessKind::Write);
        report.record(Processor::Main, 0x420C, AccessKind::Write);
        report.record(Processor::Sa1, 0x420D, AccessKind::Write);
        assert_eq!(
            report.to_string(),
            "unmodelled hardware accesses, so the picture may be incomplete: \
             S-CPU read $003800-$003807 (8 times), S-CPU write $00420C (2 times), \
             SA-1 write $00420D (once)"
        );
    }
}
