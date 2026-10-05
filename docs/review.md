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

- **Three pictures Kobo's patches change (2026-10-05).** With the checks of every level on,
  Kobo's base patches on the vanilla ROM and on the SA-1 base, and an SA-1 build (which
  installs them), draw every level as without them but `012` and `0F8` (drawn and as
  markers) and `101` (drawn). lunar-magic-install.md records the three ("where the patches
  change a picture there") without why. They are the listed exceptions of
  `tests/install.rs`'s and `tests/tool_stages.rs`'s full checks, which fail once one draws
  the same again. Settled by finding what differs in each and recording it, or fixing it.
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
