# Marked for review

Decisions taken without the maintainer, to be reviewed in a batch. Step 2's remaining work
runs unattended (from 2026-09-27): each piece merges once CI and the local ROM, corpus, and
Lunar Magic checks pass, and review comes after. Anything a piece could not settle on its
own goes here instead of blocking it: a setting builds refuse because its behaviour could
not be found by observation, a judgement call between two defensible designs, a check that
was weakened or skipped, anything that needs a test ROM from Lunar Magic's GUI.

One entry per item, newest last, removed once reviewed (the decision then lives in the
doc it belongs to). Each says what was decided, why, where it is recorded, and what would
settle it.

## Open

- **Two tide corners that play otherwise than Lunar Magic 3.70's** (2026-10-04,
  `layer3.asm` `tide_offsets` and the load's `start`; docs/lunar-magic-install.md "Layer 3
  settings"). Found while playing the corpus tide levels against 3.70 (the approved tides
  review, now recorded there), in settings no corpus level has, by writing them into
  akogare v1.2's level `115` in its 3.70 transfer and in Kobo's build: with `advanced`
  and a vertical autoscroll, the tide's vertical offset (`$0BEA`, the next frame's `$28`)
  is a step behind 3.70's; and with layer 3 following layer 1 or autoscrolling
  vertically, the offset the load leaves (`$28` on the first frame) comes from layer 3's
  position before Kobo's code places it, which matters where the player's first
  collision is on that frame (a few frames of what he touches). Builds take both as
  they are rather than refuse them. Settles it: the two rules found the same way, or
  refusing those settings in a tide level.
