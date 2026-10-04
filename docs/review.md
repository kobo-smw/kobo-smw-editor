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

- **Lunar Magic's empty Map16 tile from a `.map16` file (2026-10-04).** A full export
  writes `$1004` four times for every tile it has nothing for, allocated or not, so the
  import (`import::map16_from_file`) reads it as Kobo's empty tile. A ROM import keeps it
  where Lunar Magic allocated the page. A level that places such a tile draws tile `000`
  where the hack draws tile `004`; the Romhack Races baserom's levels place none.
