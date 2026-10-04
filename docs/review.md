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

- **A byte at `$0FF0A0` (2026-10-04).** step-2.md decided a build does not carry Lunar
  Magic's version string there. The retry system, which the Romhack Races baserom and many
  hacks use, refuses to assemble unless `$0FF0A0` is not `$FF`, so every build in Lunar
  Magic's layout now writes `$00` there: one byte, not the string, which `Rom::
  lunar_magic_version` and Lunar Magic do not read as a version, and which a save
  overwrites (lunar-magic-install.md, register). Settle by keeping it, or by writing it
  only for projects that need it.
- **Freeplay's level graphics loading optimization is inert in Kobo builds (2026-10-04).**
  It checks `$00AACD` for a 4bpp upload, which builds with 4bpp GFX now write, and then
  hooks the game's upload in code Kobo's graphics loader no longer runs: it assembles and
  does nothing. The alternative was to refuse it, which would stop the Romhack Races
  template building. Settle by keeping it, or by giving Kobo's loader the same speed-up.
- **Lunar Magic's empty Map16 tile from a `.map16` file (2026-10-04).** A full export
  writes `$1004` four times for every tile it has nothing for, allocated or not, so the
  import (`import::map16_from_file`) reads it as Kobo's empty tile. A ROM import keeps it
  where Lunar Magic allocated the page. A level that places such a tile draws tile `000`
  where the hack draws tile `004`; the Romhack Races baserom's levels place none.
