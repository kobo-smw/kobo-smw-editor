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
  submap change, four of Lunar Magic's overworld Extra Options (two needing code of
  Kobo's own), the title screen's demo moves, and breakpoints on code rather than RAM
  (older Lunar Magic versions' overworlds and the other Extra Options followed on
  2026-10-09).
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

- **Older installs' GFX, imported as Lunar Magic 3.70 stores them (2026-10-09).** A 4bpp
  hack whose install predates Lunar Magic's change of the game's operand at `$00AA8D`
  (Kaizo Mario 1 to 3) draws `GFX08` on the overworld and `GFX1E` in the upper colours by
  its upload, from files without the fourth plane. Lunar Magic 3.70, storing the GFX
  again, sets that plane on `GFX08`'s 24 upper-colour tiles, `GFX17`'s berry, and all of
  `GFX1E`, and the rest of FG3 then draws in the lower colours. Taken without asking:
  an import does what 3.70 does (`exgfx::upgraded_4bpp`), so the project builds to what
  Lunar Magic would make of the hack rather than to the hack's own overworld, whose
  look under Lunar Magic 3's code no file can hold for both a level and the overworld;
  and Kobo's install writes `$32` at `$00AA8D`, a second lone check byte in code its
  upload leaves dead, so that Lunar Magic exports a build's files as stored
  (lunar-magic-install.md, "The overworld"). Settled by agreeing, or by choosing the
  hack's own look (then `GFX08` would need a copy for the overworld).

- **The Extra Options as their bytes (2026-10-09).** Ten of Lunar Magic's overworld
  Extra Options are a byte or two of the game's code each (a branch made `BRA`, an
  operand made 0, two `NOP`s), which its transfer reads back from a Kobo build. Taken
  without asking: Kobo writes exactly those bytes for an option off, since they are
  both what Lunar Magic checks and the whole of the effect, rather than code of its own
  (`overworld::GAME_OPTIONS`); and Kobo's code for the save tiles' hook reads its
  option from `$03BA26`, where Lunar Magic's code keeps it, rather than a flag of its
  own beside it (as with the FG1-2 merge's byte). Names in the overworld file are
  Kobo's (`life_exchange = false`), the help's wording in the editor
  (lunar-magic-install.md, "The overworld"). Settled by agreeing, or by a rule for
  options that are bytes of the game's code.
