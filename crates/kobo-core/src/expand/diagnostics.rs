//! What a capture gave up on without failing the level.

use std::fmt;

use crate::cpu::CpuError;

/// The pass of a capture that the CPU core could not finish. A crashing
/// custom sprite, or per-level code that is broken whatever runs it,
/// costs that pass its picture and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pass {
    /// The player's entrance frames; the level goes without a player.
    Player,
    /// The ROM's sprite loader, called with the camera here. The
    /// column's sprites become markers.
    SpriteLoader { camera: (i32, i32) },
    /// The drawing pass of sprite `id`, from the entry at this tile.
    Sprite { id: u8, x: i32, y: i32 },
    /// The shared pass of the entries that took no sprite slot, from the
    /// loader's camera.
    Slotless { camera: (i32, i32) },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Diagnostic {
    Cpu { pass: Pass, error: CpuError },
    Unsupported(crate::cpu::access::UnsupportedAccesses),
}

/// One line per distinct error, naming the first pass it stopped and how
/// many others: broken per-level code fails every pass the same way.
pub fn summarize(diagnostics: &[Diagnostic]) -> Vec<String> {
    let mut reported: Vec<&CpuError> = Vec::new();
    let mut lines = Vec::new();
    for diagnostic in diagnostics {
        let Diagnostic::Cpu { error, .. } = diagnostic else {
            lines.push(diagnostic.to_string());
            continue;
        };
        if reported.contains(&error) {
            continue;
        }
        reported.push(error);
        let passes = diagnostics
            .iter()
            .filter(|other| matches!(other, Diagnostic::Cpu { error: other, .. } if other == error))
            .count();
        lines.push(match passes {
            1 => diagnostic.to_string(),
            2 => format!("{diagnostic} (and 1 more pass)"),
            n => format!("{diagnostic} (and {} more passes)", n - 1),
        });
    }
    lines
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (pass, error) = match self {
            Self::Cpu { pass, error } => (pass, error),
            Self::Unsupported(report) => return report.fmt(f),
        };
        match pass {
            Pass::Player => write!(f, "player entrance")?,
            Pass::SpriteLoader { camera: (x, y) } => {
                write!(f, "sprite loader with the camera at ({x}, {y})")?
            }
            Pass::Sprite { id, x, y } => write!(f, "sprite {id:02X} at tile ({x}, {y})")?,
            Pass::Slotless { camera: (x, y) } => {
                write!(f, "slotless sprites with the camera at ({x}, {y})")?
            }
        }
        write!(f, ": {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clean_room::CodeAddr;

    #[test]
    fn a_repeated_error_is_summarized_once() {
        let (brk_at, cop_at) = (CodeAddr::new(0x93, 0x9D9F), CodeAddr::new(0x01, 0x8000));
        let brk = CpuError::Brk { at: brk_at };
        let cop = CpuError::Cop { at: cop_at };
        let diagnostic = |pass, error: &CpuError| Diagnostic::Cpu {
            pass,
            error: error.clone(),
        };
        let lines = summarize(&[
            diagnostic(Pass::Player, &brk),
            diagnostic(
                Pass::Sprite {
                    id: 0xB9,
                    x: 1,
                    y: 19,
                },
                &cop,
            ),
            diagnostic(Pass::Slotless { camera: (16, 0) }, &brk),
        ]);
        // The addresses are shown as the process allows (clean room).
        assert_eq!(
            lines,
            [
                format!("player entrance: BRK at {brk_at} (and 1 more pass)"),
                format!("sprite B9 at tile (1, 19): COP at {cop_at}"),
            ]
        );
        assert!(summarize(&[]).is_empty());
    }
}
