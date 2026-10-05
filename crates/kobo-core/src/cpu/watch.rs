//! A log of the S-CPU's data reads and writes in chosen ranges, in the
//! order they happened: what a pass can learn of where a routine takes
//! its data and puts its results without looking at the routine. Only
//! the addresses are kept, which are memory effects; never the
//! instruction that made an access (clean room).

use std::ops::Range;

/// One logged access.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watched {
    /// A data read (not an instruction fetch), by its range's index and
    /// the offset in it.
    Read { range: usize, offset: u32 },
    /// A write, likewise.
    Write { range: usize, offset: u32 },
}

/// The ranges to watch and what was logged in them.
#[derive(Clone, Default, Debug)]
pub struct AccessLog {
    /// Read ranges, compared with bit 23 of the bank clear, so a ROM bank
    /// read through its mirror past `$80` is the same read.
    reads: Vec<Range<u32>>,
    writes: Vec<Range<u32>>,
    pub events: Vec<Watched>,
}

/// Bit 23: banks `$80`-`$FF` mirror `$00`-`$7F` on LoROM.
const MIRROR: u32 = 0x80_0000;

impl AccessLog {
    pub fn new(reads: Vec<Range<u32>>, writes: Vec<Range<u32>>) -> Self {
        let reads = reads
            .into_iter()
            .map(|r| (r.start & !MIRROR)..(r.end & !MIRROR))
            .collect();
        Self {
            reads,
            writes,
            events: Vec::new(),
        }
    }

    #[inline]
    pub(crate) fn read(&mut self, addr: u32) {
        let at = addr & !MIRROR;
        if let Some(range) = self.reads.iter().position(|r| r.contains(&at)) {
            let offset = at - self.reads[range].start;
            self.events.push(Watched::Read { range, offset });
        }
    }

    #[inline]
    pub(crate) fn write(&mut self, addr: u32) {
        if let Some(range) = self.writes.iter().position(|r| r.contains(&addr)) {
            let offset = addr - self.writes[range].start;
            self.events.push(Watched::Write { range, offset });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_reads_through_either_mirror_and_writes_in_order() {
        let mut log = AccessLog::new(
            vec![0x06_8000..0x06_8010, 0x0C_8000..0x0C_8004],
            vec![0x7E_C800..0x7E_C900, 0x7F_C800..0x7F_C900],
        );
        log.read(0x86_8003);
        log.read(0x06_8010);
        log.write(0x7E_C805);
        log.write(0x7F_C805);
        log.read(0x8C_8001);
        log.write(0x7E_0000);
        assert_eq!(
            log.events,
            [
                Watched::Read {
                    range: 0,
                    offset: 3
                },
                Watched::Write {
                    range: 0,
                    offset: 5
                },
                Watched::Write {
                    range: 1,
                    offset: 5
                },
                Watched::Read {
                    range: 1,
                    offset: 1
                },
            ]
        );
    }
}
