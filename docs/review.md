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

- **The editor's dependencies' licences (2026-10-05).** `kobo-editor` brings four
  licences `deny.toml` did not allow: BSL-1.0 (clipboard-win and error-code, its clipboard
  on Windows), allowed generally as a permissive code licence; CC0-1.0 for `notify` only
  (a public domain dedication; some distributions avoid it for code over patents, which
  a file watcher hardly raises); and OFL-1.1 with the Ubuntu Font Licence for
  `epaint_default_fonts` only, egui's built-in fonts, which both licences let any
  software bundle. To settle: accept, or replace `notify` with polling the project's
  files and ship fonts of Kobo's choosing (`default_fonts` off).
- **The editor's toolkit, egui on OpenGL (2026-10-05).** Taken with the maintainer's
  agreement (docs/step-3.md). eframe 0.36 defaults to wgpu; Kobo turns that off for its
  OpenGL renderer (`glow`), which needs no Vulkan or EGL and runs under Mesa's GLX on the
  development server. Either works on the three platforms; wgpu is a feature flag away
  if a driver gives trouble.
- **Lunar Magic's "Auto-Set Number of Screens" (2026-10-06).** The editor does not set a
  level's screen count by itself, as Lunar Magic does when it saves; the flag is shown as
  what it is ("Lunar Magic sets screens"), and *Fit* beside the screen count sets it from
  the objects and sprites when the user asks (`edit::screens_used`).
- **Level names from the clean ROM (2026-10-06).** The editor names levels as the
  overworld does (`level::level_name`), from the clean ROM, whose overworld a build keeps
  while Kobo carries none of a project's. For a project imported from a hack they are the
  game's names, not the hack's, though true of what the build makes. To settle once the
  overworld is a project's (step 4): the project's names then.
