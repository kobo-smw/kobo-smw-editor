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

- **Downloads trust webpki-roots, not the system's certificate store** (2026-09-27,
  `tools::pinned`, `ureq` with rustls). A network that intercepts TLS with its own root
  fails to download; `KOBO_TOOL_MIRROR` (a folder URL with the same files) or a
  configured path are the ways round it, and every download is checked against the
  pinned hash either way. Settles it: `ureq`'s `platform-verifier` feature, if users hit it.
- **GPS, AddmusicK, and SA-1 Pack are not fetched from upstream** (2026-09-27). The plan
  ([step-2.md](step-2.md#tools)) has Kobo fetch them from upstream by hash as well; they
  have no licence and no stable upstream URL (GPS is on the Wayback Machine only), so they
  still come only from a configured path, and `kobo build` notes the build is repeatable
  only with the same copy. Settles it: stable URLs and a decision that fetching an
  unlicensed tool from its author is acceptable.
- **Map16 pages 0 and 1 list only what a project changes** (2026-09-27,
  `source::map16::GamePage`, step-2.md "Source formats"). A page 0 or 1 file holds the
  tiles whose graphics or acts-like setting differ from the clean ROM's, each with `acts`,
  `gfx`, or both; a tile it leaves out keeps the clean ROM's. That differs from pages 2
  and up (a tile left out is empty) and from BG Map16 pages 0 and 1 (a whole page when
  listed), and keeps Nintendo's tiles out of projects. Settles it: whether the three kinds
  should read the same way.
- **The tiles the game keeps per object tileset go in tileset files** (2026-09-27,
  `[map16_tileset]`, `build::check_map16`). Tilesets that share the game's table (vanilla:
  0, 7, C; 2, 6, 8; 3, 9, A, B, E; 4, 5, D) share one file's tiles of pages 0 and 1: a
  build refuses two of their files listing them, and import writes them under the first
  tileset of each group. Page 2 per tileset goes in the same files, and is on exactly when
  one of them lists a tile of page 2, with no separate switch; page 2's own file then
  holds only what its tiles act like. Settles it: whether the manifest should name
  sharing groups, or have an explicit switch for page 2 per tileset.
- **The diagonal pipes and the vertical pipes' colours are a file of their own**
  (2026-10-01, `[map16_pipes] file`, `source::map16::Pipes`; replaces the 2026-09-27
  entry that left them out). They are neither pages 0 and 1 nor a tileset's table, so a
  page file does not hold them: the pipes file lists colour sets 0, 2, and 3 (set 1 is
  page 1's own tiles) and the diagonal pipes, only what a project changes. Lunar Magic's
  full export and save read them back (`tests/lunar_magic_save.rs`). Settles it: a
  different layout the maintainer prefers.
- **LC_LZ3 GFX are for SA-1 builds only** (2026-09-27, `[rom] lz3`, step-2.md). An SA-1
  build gets its LC_LZ3 decompressor from SA-1 Pack, the user's own tool; a LoROM build
  would need one of Kobo's own at `$00B8E3`, written clean-room, which is not done, so
  such a build is refused. A LoROM hack with LC_LZ3 GFX imports and builds with LC_LZ2,
  which reads the same. The encoder leaves out LC_LZ3's reversed and backwards copies.
  Settles it: whether LC_LZ3 is wanted in LoROM builds (then Kobo's own decompressor),
  and whether the few percent those copies would save matter.
- **Yoshi's berry check is Lunar Magic 3.70's where it can be told, and bounded where
  Lunar Magic's is not** (2026-10-01, lunar-magic-install.md "Yoshi and berries";
  replaces the 2026-09-27 entry that kept the game's check). Kobo's hooks follow the
  acts-like chain, read vertical levels and layer 2, and put a layer 2 berry's mushroom
  on layer 1, as Lunar Magic's code does in every scenario probed. Three things are left
  out on purpose: Lunar Magic's code going outside the tile planes when a layer 2 point
  is off the level, which hangs the game in a vertical level with the player's X at
  `$D0` or more (in Mesen 2 too), so Kobo bounds the point as the game's lookups do;
  `$1B7D`, which Lunar Magic sets while a berry's mushroom is on its way and nothing
  observed reads; and the tongue's custom block slot (`$06F9F0`), which Lunar Magic
  never called in any probe, so Kobo does not either. Yoshi walking into a berry stays
  the game's check, as under Lunar Magic. Settles it: a hack or tool that relies on
  `$1B7D` or writes the tongue's slot, or a newer Lunar Magic that fixes the hang (then
  check it still matches).
- **Builds refuse a sprite list out of screen order** (2026-09-28, `build::sprite_list`).
  The game's loader stops at the first sprite past the screen it loads, and Lunar Magic's
  loader crashes the game on such a list (random scenarios with unsorted lists broke to
  `BRK` and `COP` in a Lunar Magic-saved ROM, not with Kobo's loader); every list of the
  corpus and of vanilla is in order. Level files keep data order, so a list is refused
  rather than sorted, which would change its load indexes. Kept (2026-10-01): the sweep
  found one hack with such lists, QLDC 2021 `34_idol` (Lunar Magic 3.30; levels `016`,
  `08F`, `090`, `0B1`, `13F`), and played under its own code those five levels, and no
  other of its levels tried, stop on the first frame (a `BRK`, or a loop that never
  ends), so the lists are broken in the hack too. (Mesen could not confirm it: the
  oracle script stays on the hack's title screen.) Settles it: whether the build should
  sort instead.
- **More than 128 sprites in a level is refused unless the ROM declares a 255-sprite
  loader** (2026-09-28, `sprites::max_sprites`). Lunar Magic's layout keeps 128 load flags;
  PIXI's 255-sprite option (on unless `-d255spl`) moves them to `$7FAF00` and clears bit 0
  of `$0FFFE0`, the bit Lunar Magic's help says declares 255, and Kobo's loader takes
  entry numbers to 255. So a build with the sprites stage takes up to 255 sprites a level
  (tested: entries past 200 spawn and set PIXI's flags), and one without refuses 129.
  Settles it: whether a project should be able to ask for PIXI's `-d255spl`.
- **Kobo's sprite loader differs from Lunar Magic 3.30's in one taller level** (2026-09-28,
  lunar-magic-install.md "Sprites"). Luminescent's own ROM (saved by 3.30) with Kobo's
  loader swapped in plays every level as the hack does but level `156` (298 rows, `TT` 1,
  smart spawning), where the hack's loader erases a Sumo Brother `$130` below the camera
  a frame before Kobo's does (reproduced carrying the player down from the level's top).
  Luminescent moved into a 3.70 ROM plays the same way with 3.70's loader and with
  Kobo's, on nine routes through that level: the difference is between 3.30 and 3.70,
  and Kobo matches 3.70, the version step 2 targets (taller levels piece, 2026-09-28).
  Settles it: nothing more, unless older versions' behaviour matters.
- **`KOBO_RAM_WATCH` was used on a Lunar Magic-saved ROM** (2026-09-28, and again
  2026-10-01). To see which sprite slots a level load wrote, it watched `$14D0` and
  `$14D1` in vanilla saved by Lunar Magic and in Kaizo Kindergarten with Kobo's loader;
  in the berry probes, `$168E`. With the trace forbidden it printed each write's value,
  its address, and the writing instruction's address and registers; the addresses were
  the game's own code (`$02A975`, `$018174`, `$01ACA1`, `$02AC0C`, `$008A53`) and one in
  the hack's own code. Settled 2026-10-01: once a ROM Lunar Magic saved is loaded
  (`kobo_core::clean_room` since 2026-10-02), a watch line gives the write alone, without
  the instruction's address or registers.
- **Two values Lunar Magic's loader leaves are not reproduced** (2026-09-28): `$54` (the
  first byte of the last sprite entry loaded) and `$0BEE`-`$0BEF` (`$FFFF`, or other values
  with smart spawning). Nothing known reads them; PIXI reads `$0BF0`-`$0BF3`, which Kobo
  leaves as Lunar Magic does. Settles it: a tool or patch that reads either.
- **Kobo's VRAM patch keeps its own state where Lunar Magic's keeps its own**
  (2026-09-28, `asm/lunar-magic/vram.asm`). The RAM a level load leaves in `$0695`-`$06BE`,
  the upload buffers `$1BE6`-`$1DE7`, and `$7FBC00`/`$7FC300` (the background) are the
  patch's own in both, laid out differently, so a load leaves them different
  (docs/lunar-magic-install.md, "Tilemap streaming"). No code outside either patch was
  seen to read them. Kobo's also leaves the unseen row above a level's top as it was,
  where a Lunar Magic-saved ROM has other words there (seen in VRAM). Settles it: a patch
  or tool that reads Lunar Magic's state there.
- **Parts of Lunar Magic's VRAM patch have no counterpart in Kobo's** (2026-09-28,
  updated 2026-10-02): its game loop hook (`$008072`/`$00BA56`) and its stripe upload
  (`$0085D2`). Kobo leaves the game's code there, and a save puts Lunar Magic's in. The
  game loop hook changes only the player's tile words (the entry on them, below) and
  Lunar Magic's own RAM; stripe images (a message box, a level's end into a cutscene)
  leave the same VRAM with Kobo's patch as with Lunar Magic's, lagging or not. Its lag
  path (`$0081E2`) has Kobo's own code since 2026-10-02, which keeps layer 1's and 2's
  scroll registers on a lagging frame as Lunar Magic's does (lunar-magic-install.md,
  "Graphics"). Settles it: stripe images of other kinds (a hack's own), or lag measured
  against an emulator rather than an NMI put before the frame's end.
- **Builds refuse layer 3's tilemap of size 3 and files too large for a slot**
  (2026-09-28, updated 2026-09-30). LT3's `DD` = 1 is settled (the whole file from
  `$5000`, as Kobo's loader had it) and builds take it; size 3 (`FF` = 3), which Lunar
  Magic's dialog does not offer and no corpus ROM uses, loads other than size 0 under
  Lunar Magic's loader (layer 3's tiles at `$4000`-`$4FFF` end up otherwise), so it is
  refused. A file over `$1000` bytes (`$1A00` for AN2, `$2000` for LT3) was not tried
  under Lunar Magic's loader, and runs over the RAM after the buffer. A file `32`-`7E`
  in a list is refused too, and objects `24` and `25` on layer 2, which neither loader
  reads. Settles it: levels made for each.
- **A build that uses any of Lunar Magic's graphics formats stores every GFX file as
  4bpp** (2026-09-28, `build::write_gfx`). Lunar Magic's check is one byte for all files,
  so once `graphics.asm` is in, every file the game keeps as 3bpp is converted as Lunar
  Magic 3.70 converts it, into free space (about 135 KiB compressed, 8 KiB more than
  the game's files, whose old copies stay), and the project's own GFX files take 16
  colours. `[graphics] bpp = 4` asks for it without ExGFX or lists. Settles it: whether
  a project should be able to keep 3bpp files with ExGFX (Lunar Magic's "use 4bpp"
  option off), which would need Kobo's code for the 3bpp case.
- **Project format for Lunar Magic's graphics** (2026-09-28, `source::project`,
  `source::level`). `[exgfx]` lists ExGFX files as indexed PNGs (4bpp, or
  `{ file, bpp = 2 }`) or `.bin` bytes; import writes a PNG when every list using the
  file loads it at one depth (4 for FG/BG/SP and the older lists, 2 for LG) and it is
  whole rows of 16 tiles, and bytes otherwise (layer 3 tilemaps, AN2 files).
  `[bypass_lists]` holds the older lists, first file first. A level's `[graphics]` names
  every slot, with `bypass`, `layer3_files`, `layer3_tilemap`, and `tilemap` (LT3's
  nibble) apart, and the layer 3 settings in `[graphics.layer3]`; it is written only when the list changes
  what the level loads. Settles it: review of the names and of PNG versus bytes.
- **Kobo's graphics loader leaves `$7FC009` different from Lunar Magic's** (2026-09-28,
  updated 2026-09-30). It now gives its direct page back (the player's tile pointers are
  built from it, which made the player's tiles differ) and sets `$7FC01A` bit 7 for a
  list with `T` as Lunar Magic's does; `$7FC009` (`$41` in Lunar Magic's boss arenas)
  still differs, and while a `$2000`-byte layer 3 tilemap loads, Kobo's puts
  `$7EBD00`-`$7ECCFF` aside in VRAM at `$4000` and back, then reloads layer 3's first two
  files when the list names none. Settles it: a tool or patch that reads those.
- **Builds write the submaps' lists but have no overworld graphics code** (2026-09-28).
  Lists `200`-`206` are written as Lunar Magic 3.70 writes them (the overworld's own
  files), so its editor and save find them; Kobo leaves the overworld's, cutscenes', and
  credits' loading as the game has it, where Lunar Magic's reads those lists. Settles it:
  overworld work (roadmap step 4).
- **ExAnimation: only trigger `0F` on types `01`-`15` is refused** (2026-10-01,
  `exanimation::refusal`; replaces the 2026-09-30 entry that refused `09`-`0F` and
  rotations with manual, one-shot, or once-only triggers). Each was probed against Lunar
  Magic's code and Kobo's made to match (lunar-magic-install.md, "ExAnimation"); the
  random comparison (`tests/lunar_magic_exanimation.rs`) now draws them too and passes
  800 cases on both mappings. `0F` on a type that uploads frames uploads bytes of Lunar
  Magic's own work RAM instead, which no other code can give, so it stays refused; no
  corpus list uses it. Settles it: a hack that does.
- **ExAnimation project format** (2026-09-30, `source::animation`). A level's
  `[animation]` has the settings as four booleans (written only when not what a build
  gives the level: all on, level `104`'s lists off), the list's header as optional keys,
  and one slot per line: `vram` for tiles, `colour`/`colours` for colours, `delay` for a
  rotation, `frames` and, for a trigger with a second set, `triggered`, as hex words. The
  global list is its own file (`[animation] global` in the manifest), and the
  uncompressed ExGFX `60`-`63` are `.bin` files beside it (`[animation] 0x60`), not in
  `[exgfx]`, since they are not compressed or loaded into slots. Settles it: review of the
  names and of keeping `60`-`63` apart from `[exgfx]`.
- **A rendered level shows every slot's first frame** (2026-09-30). The level's setup
  runs the animation once for each of the eight phases, so a picture has what the level
  shows as it appears; there is no option to render a later frame. Settles it: a GUI need
  for it, which would add a frame count to `RenderOptions` and run the game loop that
  many frames before drawing (`expand::play_game_loop` has the loop).
- **Taller levels: `$02950B` is left as the game has it** (2026-09-28). Lunar Magic's
  piece rewrites the cape's block checks' entry (`STZ $0F : JSR CODE_029540`), and no
  memory effect was found with every size, `$5B`, and cape position tried. Settles it:
  a case where the cape behaves differently in a taller level.
- **Taller levels: the castle intro, Choc Island 2's rooms, and the credits get the tables
  for the game's 27 rows** (2026-09-28, `exlevel.asm`; observed 2026-10-01). Lunar Magic
  hooks each where the game loads its own data over the level entered; Kobo sets up size
  0 there. Mesen runs into castle `101` and Choc Island 2's target `024` at size `01`,
  and into the credits' enemy list after `024`, gave the same screen pointers and the
  same credits tile grid under Kobo's code as under Lunar Magic's. One difference, kept:
  in the intro and the rooms Lunar Magic leaves the pointers' low and high bytes
  (`$0CB6`, `$0CD6`, which the block lookups read) at the level's own size, while Kobo
  sets them to size 0's with the pointers; nothing was seen to read them there (the
  intro is one screen). Settles it: a room past its first screen in a taller level's
  Choc Island 2 played under both.
- **Choc Island 2's rooms get their banks from Kobo's code** (2026-10-01,
  `choc-island.asm`). A build writes listed levels to free space, and the rooms take
  their 16-bit pointers' banks from the level the exit led to (`0CD`-`0CF`), so a build
  that wrote one of them crashed the game in the rooms. Lunar Magic's own fix (3.50,
  `$05DB5B`) installs with a save; Kobo's hooks the pointer load before it (`$05DB4B`)
  and sets the banks the game's data has, in any build that writes `0CD`-`0CF`, which
  then needs Asar. A Lunar Magic save adds its own beside Kobo's and the rooms load the
  same (Mesen, all three levels). Settles it: whether to install it only with Lunar
  Magic's layout, or to keep those levels' data in bank `$06`.
- **Taller levels: one entrance corner a pixel off** (2026-09-28). With a relative camera,
  size `1E` (15 rows) with `B` at the slowest layer 2 rate places the background one
  pixel lower than Lunar Magic's; every other size, rate, and entrance byte tried gives
  the same RAM. Settles it: more data points on sizes shorter than the screen, if anyone
  uses them.
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
- **Lunar Magic's layer 3 settings: what builds refuse, and the project format**
  (2026-09-30, `build::check_layer3`, `layer3.asm`, docs/lunar-magic-install.md "Layer 3
  settings"). Kobo's code does what Lunar Magic's was seen to do for `B` and its scroll
  settings, `X`, `Y`, `C`, `S`, `I`, and `O`, and matches it frame by frame. Builds refuse
  AN2's bit 12 (`unknown`; no effect seen, no corpus ROM sets it); `tides_act_as` and
  `advanced` in tide levels build since 2026-10-01 (next entry). Scroll settings `12`-`17` and `1B`-`1F`,
  which the dialog does not offer, build: they moved as `01` in every test. Level files
  hold the settings as `[graphics.layer3]` (`advanced`, `horizontal`, `vertical` with
  names from `names.toml`, `x` in tiles, `y` signed, `cgadsub`, `subscreen`, `sync_fix`,
  `sprites_air`, `tides_act_as`, `unknown`); a slot of `$FFFF` keeps its word, whose
  nibble Lunar Magic's code (and Kobo's) reads as `F`, so settings needing another value
  there are refused. Import leaves out the settings of a ROM without Lunar Magic's layer 3
  code (nothing reads them there), but for a tide level's AAAA, which its taller levels
  code reads. Settles it: a ROM that sets AN2's bit 12.
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
  tide in a vertical level, which only Lunar Magic's code supports. Settles it: the 7
  corpus levels moved into a 3.70 ROM and played again (before the `$00E966` code they
  played the same but where a sprite meets the tide's end, and on the first frame).
- **Kobo's layer 3 code keeps its state where Lunar Magic's does** (2026-09-30,
  `layer3.asm`). It leaves the RAM Lunar Magic's leaves after a load (`$145E`-`$1460`,
  `$1458`-`$145D`, `$146A`-`$146D`, `$1B78`-`$1B7B`, `$7FC01A`-`$7FC01C`) with the same
  values, since a patch might read them, and borrows `$7FC01D` (left 0) during the setup.
  Settles it: whether matching those is wanted or a layout of Kobo's own would do.
- **The player's tile words differ for a level's first frames in a whole build**
  (2026-10-01). They come from direct page `$0C`-`$0D` as a load leaves it; Kobo's
  graphics loader now leaves it as Lunar Magic's does (all 512 vanilla levels the same),
  but other pieces of a build leave other scratch there, so the player's first frame's
  tiles (VRAM `$6040`-`$6187`, `$67F0`) can differ before the game redraws him. Settled
  (2026-10-01): left as it is. A whole build (vanilla imported from a ROM Lunar Magic
  saved once, and built) leaves the player's tile words as the vanilla ROM does, after
  every level's load and on every frame played (`exlevel_probe compare ... vram=6000-6800`,
  14 levels for 40 frames); it is the Lunar Magic-saved ROM that differs from vanilla,
  at the first frame and on every frame after in words `$6064`-`$6067` and
  `$6084`-`$6087`, which the game fills from scratch RAM that Lunar Magic's code leaves
  otherwise. Swapping single pieces of Kobo's code into that ROM changes the words too
  (graphics, VRAM, sprites), each leaving the game's scratch where Lunar Magic's leaves
  its own. Matching Lunar Magic there would mean copying its scratch use, which nothing
  shown needs.
- **SA-1: the patches name SA-1 Pack's addresses through Asar defines, not ones made from
  `ram::RamMap`** (2026-10-01, `asm/lunar-magic/memory.asm`). The plan had each patch's
  RAM "as defines through `ram::RamMap`'s SA-1 map". The patches use the community's
  convention instead (`$010B|!addr`, `$000E|!dp`, the sprite tables as `!9E`, `!14D4`),
  which PIXI, GPS, and UberASM Tool code is written in, with one file choosing the values
  by the ROM's map mode; `tests/install.rs` checks them against `RamMap::resolve`. A
  define puts the address last (`!addr|$0BF6`) so that `!name+2` means the same in
  either order Asar evaluates; an immediate built from a define is sized (`LDA.b`), since
  Asar sizes it from its literals (`$0000` on LoROM made one 16-bit). Settles it: whether
  Rust should generate the defines instead, which would keep one list but make the
  patches unlike the tools'.
- **SA-1: only SA-1 Pack 1.40 is supported** (2026-10-01, decided with the maintainer).
  Builds apply the configured SA-1 Pack, which the tests take to be 1.40; Kobo's sprite
  loader follows 1.40's changes to the game's loop. Super Diagonal Mario 2 has an older
  SA-1 Pack (no marker at `$0084C0`) with Lunar Magic 2.52; it imports, but Kobo's code
  swapped into it is not expected to match. Settles it: a hack built on an older SA-1
  Pack that someone wants to build with Kobo.
- **SA-1: the conditional Direct Map16 flags are read through SA-1 Pack's call to the
  S-CPU** (2026-10-01, `objects.asm`, `%call_scpu` in `memory.asm`). The flags stay in
  work RAM (`$7FC060`), where a Lunar Magic-saved SA-1 ROM reads them, and Kobo's object
  code runs on the SA-1 there, which cannot reach work RAM. The call costs an interrupt
  per conditional object at the load. The alternative, copying the 16 bytes to BW-RAM on
  the S-CPU before the load, needs a hook of Kobo's own in SA-1 Pack's level load.
  Settles it: nothing found wrong; recorded as the one place Kobo's code relies on SA-1
  Pack's inter-processor calls.
- **SA-1: uploads use DMA channel 2 and read from bank `$00`** (2026-10-01,
  `memory.asm` `!dma`). SA-1 Pack keeps channel 1 for windowing HDMA and moves the game's
  own NMI and blank-screen uploads to channel 2, so Kobo's (the VRAM patch, graphics)
  follow; the buffers are read from `$00:6xxx` (BW-RAM's window) as SA-1 Pack's own
  uploads read them. On LoROM the channel is still 1, and the buffers are read from
  bank `$00` too (work RAM's mirror) where they were read from `$7E`: the same memory.
- **SA-1 with PIXI: the goal tape's extra bits are not kept in `$187B` by Kobo's loader**
  (2026-10-01, `sprites.asm`). PIXI on an SA-1 ROM puts its own jump at `$02A9D7`, the
  site Kobo's loader uses after a spawn, so `sprite_after` does not run; Lunar Magic's
  loader is bypassed there the same way, and PIXI keeps the extra bits itself
  (`!extra_bits`): a goal tape placed for the secret exit gives it in an SA-1 build with
  PIXI, and after Lunar Magic saves it (`a_pixi_build_survives_a_lunar_magic_save`). On
  LoROM PIXI jumps from `$02A9DB` and Kobo's code still runs. Settles it: whether
  anything other than the goal tape reads `$187B` from the loader on SA-1.
- **A screen exit in Lunar Magic's format writes its whole destination** (2026-10-02,
  `source::level`). Long screen exits name entrances up to `1FFF`, so an exit with
  `lm_format` now has `dest` as the whole number (`dest = 0x320`), where it was the low
  byte with `high = true` for bit 8; `high` stays for the game's format, whose bit 0 the
  object keeps, and is refused beside `lm_format`. Exits in the game's format are
  unchanged. No committed project had `high`. Settles it: the maintainer's taste.
- **Builds move the entrance tables to `$2000` entrances, not to the last one in use**
  (2026-10-02, `entrances.asm`). A project with an entrance or a long exit past `1FF`
  gets six tables of `$2000` (48 KiB), whatever its highest entrance; Lunar Magic sizes
  them to the last entrance in use, and its save re-allocates Kobo's so. Sizing them to
  the project would need the patch to take the size as a define. Settles it: whether
  the space matters.
- **A graphics list may name an ExGFX file the project does not have** (2026-10-02,
  `build::check_graphics_list`). The slot then loads nothing, as a slot naming a file the
  ROM lacks does under Lunar Magic's code (the same VRAM as `7F`) and Kobo's. Hacks have
  such lists (nine corpus hacks), so a build writes them as they are, where it refused
  them before; an import notes each, but a hand-written project that names a file it
  forgot to list is no longer stopped. Settles it: whether a build should refuse what
  Lunar Magic's editor allows.
- **Kobo's acts-like code runs the help file's custom block slots, which Lunar Magic 3.70
  does not** (2026-10-02, `actslike.asm`, lunar-magic-install.md "The ranges"). Every
  documented slot (`$06F890`-`$06F9FF`) holds `NOP`s and a `JMP $F602` in a Kobo build,
  so a `JSL` written there runs; 3.70 ran none written at the head, sprite, cape,
  fireball, and tongue slots, and has its own code over parts of the area. With the
  slots empty the two play the same. Settles it: whether a build should leave them
  unrun as 3.70 does, which a tool written for older Lunar Magic versions would then
  find not working, as under 3.70.
- **Exits without `u` take bit 8 from the translevel, not the submap** (2026-10-02,
  `exits.asm`, lunar-magic-install.md "Entrances, exits, and midway points"; replaces
  the earlier choice to keep the game's rule there). Lunar Magic's code does so, and its
  save puts that code at `$05D7CE` over Kobo's, so with the game's rule a saved build
  could send such an exit, or Yoshi's wings or the bonus game, to the other bank than
  before the save. For vanilla's overworld the two agree. Settles it: an overworld that
  puts a level on a submap its number does not say, built and played before and after
  a save.
