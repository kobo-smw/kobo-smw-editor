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

- **Whether step 4 is done (2026-10-09).** Every item of step-4.md's work order now has
  a first version in the editor and the build: Map16, graphics, the shared palettes,
  ExAnimation, layer 3 tilemaps, the overworld (read, built in Lunar Magic's layout,
  imported, and edited, with its graphics, palettes, ExAnimation, border, and lists),
  and the emulator (Play, symbols, and watching RAM in Mesen with pauses on writes and
  the player on the canvas). Left, as step-4.md's entries say: the first frames after a
  submap change, Lunar Magic's other overworld Extra Options, the title screen's demo
  moves, older
  Lunar Magic versions' overworld formats, and breakpoints on code rather than RAM.
  Settled by closing step 4 (folding step-4.md into the other docs) or naming what
  else it needs.

- **The FG1-2 merge's byte (2026-10-09).** Lunar Magic keeps its overworld option to
  merge FG1-2 into SP3-4 as `$D0` at `$0FF9F0`, in its own area, where unmerged ROMs
  have `$F0` (lunar-magic-install.md, "The overworld"). Kobo writes `$D0` there for a
  merged project, Lunar Magic's transfer reading it (tested with and without), and
  Kobo's own loader reads the same byte to decide the merge, rather than keep a flag
  of its own beside it; an unmerged Kobo build has `$FF` there, which Lunar Magic
  reads as unmerged. Settled by agreeing, or by a rule for such option bytes.

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
