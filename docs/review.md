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

- **The editor draws without vsync on Linux** (2026-10-08). The desktop called the
  editor not responding when Play's emulator covered it: with vsync, a frame drawn
  while the window is covered waits on Wayland until it is shown again
  (docs/step-3.md, Toolkit). The editor now paces its own frames at 60 a second on
  Linux; Windows and macOS keep vsync. The diagnosis is from Mesa's and eframe's
  code, not a reproduction on a Wayland desktop. Settled by the maintainer no longer
  seeing the dialog on Play, and by no tearing or busy CPU while the canvas scrolls.
