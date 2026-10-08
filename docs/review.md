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

- **The overworld's scope and order** (2026-10-08). Step 4's last large piece is the
  overworld. The game's own format cannot be edited in place usefully: it numbers
  translevels by the order of the level tiles and fixes its events' places
  (smw.md, "The overworld"), so a moved level tile renumbers the levels after it. A
  project's overworld therefore has to be built in Lunar Magic's overworld layout, which
  means finding that layout and writing Kobo's own code for each overworld hook, as step
  2 did for levels, from `-TransferOverworld` byte diffs and the memory effects of
  playing Lunar Magic-saved overworlds; importing a hack's overworld needs the same
  reading. That is a research phase the size of step 2's, so it was not started
  unattended. Recorded in step-4.md (item 6). Settled by choosing to start it, or to
  leave the overworld to Lunar Magic for now (build.md, "What is left to Lunar Magic")
  and go on with other work.
