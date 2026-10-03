//! Controls shared by all passes and both CPUs of one operation.
//!
//! An [`Operation`] is the handle a long operation runs under: the
//! `_with_control` variants of `expand_level`, `capture_sprites`,
//! `render_level`, and `render_loaded` take one. It carries an instruction
//! budget shared by loading, every sprite pass, and both processors,
//! cancellation, and a [`Stage`] for progress. Clone it into a UI thread to
//! cancel or poll while a worker renders:
//!
//! ```no_run
//! use kobo_core::{Rom, operation::Operation, render};
//! let rom = Rom::load("/path/to/rom.sfc")?;
//! let operation = Operation::new(Some(500_000_000));
//! let ui = operation.clone(); // ui.progress(), ui.cancel()
//! let rendered = render::render_level_with_control(&rom, 0x105, Default::default(), &operation)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Create a new handle for each operation: clones share one count, and
//! cancellation and exhaustion are sticky. The count lives outside emulated
//! RAM, so restoring a snapshot or resetting a CPU never refunds it. Limits
//! count instructions, not wall time, pixels, or memory; `None` keeps only
//! the per-call CPU limits and `Some(0)` allows no instruction. A check
//! comes before each instruction and at each stage boundary, so the DMA or
//! block move within one instruction finishes first, and drawing between
//! stages is not interrupted.
//!
//! Cancellation and an exhausted budget fail the operation, even in a
//! player or sprite pass whose CPU faults would only be diagnosed: there is
//! no successful partial picture. [`Stage::Finished`] is set only after
//! success. Progress is observational; the stage and the count need not
//! describe the same instant.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Starting,
    Boot,
    Loading,
    Preparing,
    Sprites {
        completed: usize,
        total: usize,
    },
    Composing,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub stage: Stage,
    pub instructions: u64,
    pub instruction_limit: Option<u64>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum OperationError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation instruction budget of {limit} exhausted")]
    Budget { limit: u64 },
}

#[derive(Debug)]
struct State {
    cancelled: AtomicBool,
    exhausted: AtomicBool,
    instructions: AtomicU64,
    limit: Option<u64>,
    stage: Mutex<Stage>,
}

#[derive(Clone, Debug)]
pub struct Operation(Arc<State>);

impl Default for Operation {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Operation {
    /// `None` retains the per-call CPU limits without an aggregate limit.
    /// `Some(0)` permits no CPU instruction. Both CPUs share this budget.
    pub fn new(instruction_limit: Option<u64>) -> Self {
        Self(Arc::new(State {
            cancelled: AtomicBool::new(false),
            exhausted: AtomicBool::new(false),
            instructions: AtomicU64::new(0),
            limit: instruction_limit,
            stage: Mutex::new(Stage::Starting),
        }))
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }

    /// Stage and count are observational; they need not describe precisely
    /// the same instant while a worker is running.
    pub fn progress(&self) -> Progress {
        Progress {
            stage: *self.0.stage.lock().unwrap(),
            instructions: self.0.instructions.load(Ordering::Relaxed),
            instruction_limit: self.0.limit,
        }
    }

    pub fn check(&self) -> Result<(), OperationError> {
        if self.0.cancelled.load(Ordering::Relaxed) {
            return Err(OperationError::Cancelled);
        }
        if self.0.exhausted.load(Ordering::Relaxed) {
            return Err(OperationError::Budget {
                limit: self.0.limit.expect("only a limited operation exhausts"),
            });
        }
        Ok(())
    }

    pub(crate) fn stage(&self, stage: Stage) -> Result<(), OperationError> {
        self.check()?;
        *self.0.stage.lock().unwrap() = stage;
        Ok(())
    }

    pub(crate) fn instruction(&self) -> Result<(), OperationError> {
        self.check()?;
        if let Some(limit) = self.0.limit {
            if self
                .0
                .instructions
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    (n < limit).then(|| n + 1)
                })
                .is_err()
            {
                self.0.exhausted.store(true, Ordering::Relaxed);
                return Err(OperationError::Budget { limit });
            }
        } else {
            self.0.instructions.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_budget_and_cancellation() {
        let operation = Operation::new(Some(2));
        let other = operation.clone();
        operation.instruction().unwrap();
        other.instruction().unwrap();
        // Completing on exactly the limit succeeds; attempting more fails.
        operation.check().unwrap();
        assert_eq!(
            other.instruction(),
            Err(OperationError::Budget { limit: 2 })
        );
        assert_eq!(operation.progress().instructions, 2);
        assert!(operation.check().is_err());
        other.cancel();
        assert_eq!(operation.check(), Err(OperationError::Cancelled));
    }
}
