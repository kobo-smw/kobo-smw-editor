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

- **kobo-core optimised in dev builds (2026-10-05).** `[profile.dev.package.kobo-core]
  opt-level = 2` makes a plain `cargo test` run the ROM-backed tests as fast as release
  (render_levels: 20 s to 3 s) while keeping debug assertions and overflow checks, which a
  release run drops; `cargo xtask verify` uses it. The cost is a less faithful debugger in
  kobo-core (`CARGO_PROFILE_DEV_PACKAGE_KOBO_CORE_OPT_LEVEL=0` undoes it) and a little
  more compile time. The first full run this way found no overflow.
- **Test tiers in the config file (2026-10-05).** The tiers' data is set in the config
  file's `[tests]` table as well as by environment variable (`kobo_core::tiers`), so that
  shells that skip `~/.bashrc` (agents' among them) run them; `~/.config/kobo/env.sh`
  can go once this is merged and its settings are in `[tests]` (docs/testing.md has this
  machine's). The config keeps `deny_unknown_fields`, so an older Kobo refuses a config
  with `[tests]`: move the settings after merging. `KOBO_SA1_BASE` (the with-kobo scripts')
  is now `KOBO_SA1_REFERENCE`.
- **The probes' RAM through `clean_room::bytes` (2026-10-05).** `entry_probe` and
  `exlevel_probe` zeroed `$0100`-`$01FF` themselves; `clean_room::bytes` withholds the
  stack (both processors' on SA-1) but keeps `$0100`-`$010F`, where the game keeps
  variables, so their comparisons now include those 16 bytes. The ignore lists recorded in
  docs/testing.md for `entry_probe` against a Lunar Magic ROM did not need them; a
  comparison that differs there now shows it.
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
- **Play from here, ahead of step 4 (2026-10-06).** The roadmap puts play-from-level in
  step 4; the editor has it now, as a build (`kobo_core::playtest`, `kobo play`) and an
  emulator of the user's choosing, which the system opens `play.sfc` with. No emulator is
  driven: Mesen-S and bsnes-plus integration stays step 4's. The build hooks two game
  modes of the game's own (`asm/playtest.asm`): the title screen, which starts a game and
  takes a screen exit, and the overworld's load, which goes back into the level, so a play
  ROM never shows the overworld. It writes `play.sfc` into the project's folder, beside
  `build.sfc`; a project's `.gitignore` should name both. To settle: the file's place (the
  user's cache instead), whether the overworld should show after the level's end, and the
  power-up and lives a play starts with (four lives, the power-up chosen in the menu).
- **Lunar Magic's "Auto-Set Number of Screens" (2026-10-06).** The editor does not set a
  level's screen count by itself, as Lunar Magic does when it saves; the flag is shown as
  what it is ("Lunar Magic sets screens"), and *Fit* beside the screen count sets it from
  the objects and sprites when the user asks (`edit::screens_used`).
