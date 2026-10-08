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

- **Play from here reads the Retry System's RAM file** (2026-10-08). kkevinm's Retry
  System sets where it respawns the player only on an entry from the overworld, which a
  play build never makes, so its retries went to level `000`. The play build now takes
  the addresses from the project's own `retry_config/ram.asm` (in its UberASM Tool
  folder) and sets the respawn point and its copy of the time to the entrance's;
  without the file, nothing of it is written. The other way, entering through the
  overworld's path, cannot start in a sublevel or at a tile. Recorded in
  [step-3.md](step-3.md#play-from-here) and `asm/playtest.asm`. Settled by agreeing that
  Kobo may know this one community resource, as it knows Callisto, or by another way
  for a play build to tell a retry system where it started.
- **The VRAM patch's stripe move, and where its game modes came from** (2026-10-08).
  Kobo's `GenerateTile` now queues the game's address and Kobo's code at the game loop
  hook (`$008072`, `$00BA56`) moves the stripes into Kobo's tilemaps, as the 2026-10-04
  review said to once a hack read the stripe buffer: the Romhack Races baserom's
  `vram_optimize.asm` does. The layout conversion follows from the two tilemap layouts,
  the rows kept from Kobo's own 2026-10-03 finding, and the hook's bytes from a 3.70
  save's byte diff; the game modes it runs in (`$05`, `$07`, `$13`, `$14`) are the ones
  `vram_optimize.asm` runs its move in, whose source says its move is adapted from Lunar
  Magic's code. That patch's source is not on the clean room's list of evidence
  ([clean-room.md](clean-room.md)); it was read to interoperate with the patch itself (its
  hook site and its `autoclean`). Recorded in lunar-magic-install.md ("Graphics").
  Settled by agreeing that a community patch's hook sites and stated behaviour are
  evidence, or by finding the game modes from memory effects: a stripe for `$2000`
  queued in each game mode of a Lunar Magic-saved ROM, and where VRAM changes.
