# Marked for review

Decisions taken without the maintainer, to be reviewed in a batch. Work that runs
unattended (as step 2's did from 2026-09-27) merges each piece once CI and the local ROM,
corpus, and Lunar Magic checks pass, and review comes after. Anything a piece could not
settle on its own goes here instead of blocking it: a setting builds refuse because its
behaviour could not be found by observation, a judgement call between two defensible
designs, a check that was weakened or skipped, anything that needs a test ROM from Lunar
Magic's GUI.

One entry per item, newest last, removed once reviewed (the decision then lives in the
doc it belongs to). Each says what was decided, why, where it is recorded, and what would
settle it.

## Open

- **The overworld's ExAnimation (2026-10-09).** Kobo carries each submap's list, the
  global list, and the settings, with its own code for the three hooks
  (lunar-magic-install.md, "The overworld"). Taken without asking: a submap change
  runs the eight first frames but uploads only the last, where Lunar Magic waits for
  vertical blanks to upload each (for about 12 frames after a change the list's
  tiles differ; waiting would hang Kobo's machine, which raises no vertical blank
  flags by default); the settings'
  low bits are carried as data, the lightning colour's among them unknown; and the
  game's animated tiles go where the game's `LDY #$0750` at `$00A4EA` says, so that
  carrying Lunar Magic's FG1-2 merge later is that operand.

- **A check byte that is never executed (2026-10-09).** Lunar Magic reads an overworld's
  layer 1 pages only when `$04D818` holds `$A2` (found by bisecting its overworld
  transfer, lunar-magic-install.md, "The overworld"). In its layout that byte begins an
  instruction; Kobo's hook jumps away from `$04D7F9`, so in a Kobo build it is a lone
  byte nothing runs, written only for the check (`asm/lunar-magic/overworld.asm`,
  tested with and without in `tests/lunar_magic_overworld.rs`). The other byte the
  check needs, `$04D7F9`, is Kobo's own `LDX #`, which its code uses. The alternative,
  laying Kobo's load code out in the scan's place so that an instruction of its own
  starts at `$04D818`, would shape Kobo's code after Lunar Magic's for no other reason.
  Settled by agreeing, or by a rule for check bytes nothing runs.
