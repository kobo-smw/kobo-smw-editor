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

- **ExAnimation project format** (2026-09-30, `source::animation`). A level's
  `[animation]` has the settings as four booleans (written only when not what a build
  gives the level: all on, level `104`'s lists off), the list's header as optional keys,
  and one slot per line: `vram` for tiles, `colour`/`colours` for colours, `delay` for a
  rotation, `frames` and, for a trigger with a second set, `triggered`, as hex words. The
  global list is its own file (`[animation] global` in the manifest), and the
  uncompressed ExGFX `60`-`63` are `.bin` files beside it (`[animation] 0x60`), not in
  `[exgfx]`, since they are not compressed or loaded into slots. Settles it: review of the
  names and of keeping `60`-`63` apart from `[exgfx]`.
- **Layer 2 scroll settings 8 to 11, `S`, and `H` build, and a save keeps Kobo's camera**
  (2026-10-01, `entrance.asm`, lunar-magic-install.md "Layer 2 scroll settings"; replaces
  the 2026-09-28 entry that refused them). Kobo's camera and entrance give the same RAM
  as Lunar Magic's in every case tried. Decided without review:
  - `"LM"` at `$05DD7C`, the bytes Lunar Magic's save checks before it keeps `S` and `H`.
    Without them a save drops every level's separate settings; with them it also leaves
    Kobo's entrance code and camera in place, and whatever is at eight more sites where
    it would install its own. Since 2026-10-02 builds have Kobo's code at the five whose
    Lunar Magic code changes anything (face left at `$009708` and `$00D2B2`, the end
    fade's colours at `$00AF72`, layer 2's interaction a frame behind at `$00E966`, a
    vertical level's camera at `$00F77B`), each playing as Lunar Magic's on every frame
    tried, and the game's at three where nothing different was seen (`$00F871`,
    `$05BCA5`, `$05D7BA`; lunar-magic-install.md, "The sites a save keeps with the
    marker"). Settles it: a difference at one of those three along a path not tried.
  - A moving setting steps from game mode `$13` on, which matches Lunar Magic's step count
    before a level's first frame; what decides it is not known.
  - In the level file, `layer2_scroll` is the horizontal setting when
    `layer2_vertical_scroll` is there, up to `$1F` (`H`); past 15 without it is refused.
  Settles it: a hack with these settings built and played against itself (the corpus
  sweep), or a difference along a path not tried (layer 2 objects, a scrolling sprite
  command, a level with both a moving setting and a relative camera far from its start).
- **Tides: what they act like, and `advanced` with one** (2026-10-01, `layer3.asm`
  `tide_tiles` and `tide_offsets`, `build::check_layer3`, docs/lunar-magic-install.md
  "Layer 3 settings"). `tides_act_as` fills the tide's rows with its tiles as Lunar
  Magic 3.30-3.33 hacks were seen to (every value, Kobo's code in their place plays the
  same); Lunar Magic does it in its taller levels code, Kobo in its layer 3 code, in the
  rows Kobo's taller levels code fills with water at every size, by the rule found
  (2026-10-02, lunar-magic-install.md "Layer 3 settings"). `advanced` with a tide builds
  with Kobo's code computing the tide's interaction offsets from the frame's positions,
  and since 2026-10-02 with Kobo's code after the player's collision with layer 2
  (`$00E966`), which makes them trail a frame as Lunar Magic's do: Super Dram World 2
  v1.3's level `0D3` plays as the hack on every frame tried. Refused: `advanced` with a
  tide in a vertical level, which only Lunar Magic's code supports. Since 2026-10-04
  Kobo's code also bounds a tide's vertical position as Lunar Magic 3.70's does
  (lunar-magic-install.md, "Layer 3 settings"), and the corpus comparison, with every QLDC
  entry, agrees on every tide level in hacks from 3.10 on but for what a sprite in a tide
  touches, which comes from `$00E966` too and so takes a build (QLDC 2021 `06_Friday`'s
  level `106` built by Kobo touches what the hack does). Settles it: more tide levels
  built and played against their hacks.
- **Builds move the entrance tables to `$2000` entrances, not to the last one in use**
  (2026-10-02, `entrances.asm`). A project with an entrance or a long exit past `1FF`
  gets six tables of `$2000` (48 KiB), whatever its highest entrance; Lunar Magic sizes
  them to the last entrance in use, and its save re-allocates Kobo's so. Sizing them to
  the project would need the patch to take the size as a define. Settles it: whether
  the space matters.
