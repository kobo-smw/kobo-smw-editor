# Testing log

Results of past verification runs, moved here from [testing.md](testing.md) so that file
holds how to run each check. What fails now is `crates/kobo-core/tests/fixtures/known_failures.toml`,
and pictures are pinned by `tests/fixtures/render_hashes/`; the entries below are the
record of what earlier runs found and fixed, under the check they belong to. Add a dated
line here when a run finds something worth keeping that the tests do not hold.

## Emulator oracle

Test level `132` used to differ: its Lakitu's cloud had thrown two Spinies here and none in the emulator. The cloud throws when the frame counter's low seven bits are clear (`$01E98D`), and the loader's counter started at zero; it now starts at `$40` ([smw.md](smw.md)).

`KOBO_RAM_WATCH` found PIXI's offscreen routine erasing a sprite in `43_gui` on the SA-1.

`KOBO_ORACLE_VIDEO=1` captures made before the four-frame wait at full brightness was added can be a fade step darker than the render and match nothing.

Boss arena dumps made before the loader override was limited to game mode `$11` fail the boss comparison: overriding the title-screen load in mode `$03` contaminated the graphics cache.

`KOBO_VRAM_WATCH` showed the rows Akogare2 `111` uploads as its layer 2 moves.

## Emulator oracle on hacks

On 12 levels of each of the 42 `.smc` hacks the tile grids and layer 3 tilemaps all match, and the sprite slots in 38; the rest are sprites already moving at the dump (`apes1.13` `02E`, Luminescent `103`, `SMW_2021-4-24` `0C5`/`1C5`: 8 to 10 pixels on, where the test allows 4) and one the emulator has not spawned yet (Super Hark Bros 2 `138` slot 5). The 38 SA-1 entries of QLDC 2021 and 2022 (BPS patches, applied first) were dumped the same way, six levels each: 37 get into a level (`28_Kitikuchan`'s title screen is a room to play through), and 35 of those match throughout. `61_Wakana_Sariel` level `13B` differs in the layer 3 tilemap, which its per-frame status bar code has drawn into by the time of the dump, and `43_gui` level `105` differs in slots 10 to 13 by the parity of the frame its sprites first ran on. The hack places sprites beyond the camera's right edge, which PIXI's offscreen routine (one side per frame by `$13 & 1`, PIXI's source) erases on even frames and the loader brings back; the dump's first sprite frame was odd and kept them all, the loader's here is even (`$13 = $40`, [smw.md](smw.md)) and one had gone before a spawner's child took its slot. Captures of the level one to twelve frames on (`KOBO_ORACLE_VISIBLE_FRAMES`) show the emulator erasing them from its second frame.

With `KOBO_ORACLE_VIDEO=1` the same dumps give whole frames; of 27 levels of four hacks whose pictures changed when the faults below were fixed, the entry screen of 23 went from 10-56% of pixels matching to 91-99.7% (Luminescent `148` only to 50%: an HDMA sky, see [known-gaps.md](known-gaps.md); the other three barely moved). What the hack dumps found, all in code no vanilla level runs: Lunar Magic's graphics upload reading work RAM back out of VRAM, `TM` written past its mirror, a game loop in a FastROM bank ([lunar-magic.md](lunar-magic.md)), and layer 2 left wherever level-init code put it ([smw.md](smw.md)).

## Per-sprite comparison on hacks

Run on 2026-09-24 over 12 `dump_hack.sh` levels each of Akogare2, Grand Poo World 2, Luminescent, Invictus, Super Hark Bros 2, and QLDC 2021 `70_DPBOX`, `77_NerDose` and `44_Daizo Dee Von` (84 levels, 108 sprite scores; captures and scores in `~/.local/share/kobo/oracle/sprite-video/`), every custom sprite that scored under 85% was looked at side by side and found drawn as the emulator draws it, at its first-frame position (Akogare2 `008`'s piranha plant is up its stem, NerDose `003`'s mushrooms have fallen). The whole-picture scores under 85% are layer 2 parallax positions (Akogare2 `11A`, GPW2 `107`, DPBOX `102`), an HDMA sky (Luminescent `154`), a player still in his pipe in the frame (Invictus `152`, Luminescent `142`), and Invictus `030`'s layer 3 fog. NerDose `104` scores its info box (`B9`) at 0% because the emulator's frame masks the main screen with a window where it and Mario stand (the dumped `windowMaskMain` has BG1, BG3 and objects on): the object is in the dumped OAM where the capture puts it. Daizo's capture got one level: the emulator stays in game mode `$14` on `026`, the cutscene level whose sprite waits for a button ([known-gaps.md](known-gaps.md)).

## Decompression

QLDC 2021 `34_idol`, the corpus's one LC_LZ3 hack: all 50 of its files agree, as do those of LC_LZ2 hacks on LoROM, SA-1, and a 6 MiB SA-1 image. Lunar Magic's `-ExportGFX` of it agrees on all 52 files as well (`fixtures/lunar_magic_gfx_export.txt`, by ROM hash).

## The hack corpus

2026-10-05: the corpus tests collected every failure for the first time (they stopped at
a hack's first one before) and found the recorded list incomplete. QLDC 2021
`62_Rykon-V73` and `32_theunkaizoing` fail on every level that shares vanilla's empty
sprite list at `$07E76D` (about 275 each), not only `012`; `level_data` fails six of
`34_idol`'s nine levels (`012`, `016`, `038`, `0EB`, `113`, `13F`), not one. Both are
in `known_failures.toml` as found.

Until 2026-10-04 `corpus_levels` was two tests, `layer2_background` and `sprite_lists`, each loading the corpus on one core: about 45 of a full run's 57 minutes.

A 2026-09-26 run of every `KOBO_LM_ROMS` test on env.sh's list and the 128 QLDC patches (175 entries, each test restarted after a failing hack, so a hack's first failure only): every loose and `corpus_more` hack passes; of the QLDC entries, all rebuild from a patch, and what fails is - `layer2_background` (now `corpus_levels`): 2021 `34_idol`'s nine levels and `76_Bench-kun`'s eighteen ([known-gaps.md](known-gaps.md)), and `76_Bench-kun` level `114`, whose background is missing entirely (2048 of 2048 words; it renders black, with SA-1 `$002FFF` and `$420B`, `$2130` reads reported unmodelled); - `gfx_decompression`: 2021 `70_DPBOX`, whose `GFX03` decompresses to 4095 bytes, so `GfxReader` refuses it (the ROM's own routine gives the same 4095); - `level_data`: 2021 `34_idol` level `012`, one of its nine, whose sprite list at `$E38008` parses past its 206-byte RATS block; - `sprite_lists` (now `corpus_levels`): 2021 `32_theunkaizoing` and `62_Rykon-V73` level `012`, vanilla's empty list at `$07E76D` with a RATS tag in front that claims the rest of bank `$07`; 2021 `69_bebn legg+E-man38` level `1CB` (13 bytes, block 14) and `77_NerDose` level `136` (407, block 408), a block one byte longer than the list; and 2022 `09_idol` level `105`, 32 screens of 64 rows, more than the planes hold. Run again over the QLDC patches on 2026-10-04 (every `kobo-core` test, release build): the four above fail at the same hacks, and nothing else does. Two tests added since needed changes to take them: `graphics_build` builds an SA-1 hack's lists as an SA-1 project, and `layer3_settings` takes a tide by `$1403` too and leaves out what a sprite touches in a tide (lunar-magic-install.md, "Layer 3 settings"); its first run over them found two differences in Kobo's layer 3 code, since fixed.

A 2026-09-22 rerun also found Akogare2 levels `0F8` (1792/1920 words) and `111` (21/2048 words) failing this tilemap check. Both reproduced at `9b69ab8`, before the input hardening and operation control. `0F8` passes since the loader runs the ROM's NMI at the frame boundaries (its picture was garbage without the upload the third blank does). `111` failed because the player pass kept every VRAM upload of its entrance frames, among them the rows uploaded as the level's own code pans layer 2 upward from its second dozen frames, while the level's layer 2 position stayed the loader's; the pass now puts the tilemaps back ([smw.md](smw.md)), and an emulator frame of the level agreed with the expected words.

Known exception in the corpus: `Smb2dx` (LM 1.63; 173 levels fail, its mode `$00` levels carry object layer 2 pointers), failing before the vertical-level checks were added. `Super Hark Bros 2` level `00A` used to fail with 896 of 2048 words: its level-init code leaves layer 2 at `$5D` and the game had uploaded for `$C0`, which the camera update `expand` now runs after preparation restores.

All 512 levels of each `corpus_more` hack render without a fatal error (2026-09-25); none has Lunar Magic export hashes in the fixtures yet.

## Callisto import

The Romhack Races template was checked by hand on 2026-10-04 (`kobo new`, a build with every tool configured, `render_hashes` against the baserom's own Callisto build, and `save-check`); the release is downloaded, so no test does it.

## Corpus sweep

The 2026-10-01 run's results are in step-2.md (work order, item 2).

## Map16 pages

Every LoROM hack's pages 0 and 1, imported and built: all 173 LoROM ROMs of the corpus and the QLDC entries pass.

## Kobo's VRAM patch against Lunar Magic's

On vanilla saved once by Lunar Magic (2026-09-28): the same tilemaps after every level's load but for the unseen row above a level's top, and nothing visible different for longer than a frame on any level along seven paths (`3,0`, `0,-2`, `3,-2@80/-2,2`, `3,2`, `-3,-3`, `1,-1@80/-1,1`, `6,-5@60/-7,6`), held still (`0,0`), and three that jump (`-1000,0@1/3,0`, `1000,0@1/-3,0`, `400,0@1/0,0@30/-400,0@1/0,0`), 240 frames each (2026-10-04).

## Kobo's graphics loader against Lunar Magic's

The corpus hacks' ExGFX files, older lists, and the lists a build takes, built onto vanilla levels and read back: 41 hacks (2026-09-28).

## Layer 3 settings

Found and checked against a copy of `+ExGFX` with a level imported with settings (so it has Lunar Magic's code), with settings then written straight into its lists (2026-09-30): 1,500 random cases all the same, and Kaizo Kindergarten's content (`transfer`) all the same.

Kobo's layer 3 code in every corpus hack with Lunar Magic's, saved by 3.10 or later: 13 hacks (2026-10-01).

## Entrances

Kaizo Kindergarten's `levels` against Kobo's build: every level the same by a screen exit, by `w`, and from the overworld with and without the midway point, but for the level the build leaves out (and those the overworld override maps onto it, and one vertical "No Yoshi" intro). The overworld can name a level only up to low byte `$DB` (`CODE_05D8A2` takes `$24` off a name of `$25` and up); the runs recorded here named levels `125`-`13B` by their low byte, so entered `101`-`117` for them, which the probe no longer does. Both sides of each comparison ran the same way.

## A hack built by Kobo

Kaizo Kindergarten imported, built without the levels the build refused, and compared with `tiles_diff`: all 337 the same; `save-check` passes, and `tiles_diff` of the saved copy against the hack gives the same; import, build, and import again gives the same files.

## A hack's content through Kobo's code

Kaizo Kindergarten passes. `swap bank06` also works on a hack itself, for its Lunar Magic 2.52-and-later layout: Grand Poo World 2, Invictus, Luminescent, Baby Kaizo World 3, and SMW_2022-4-9 draw every level alike, but for Grand Poo World 2's `09F` (no background table) and 3 to 9 levels each whose pictures differ only with sprites, most likely because the swap drops the hack's GPS blocks (not yet confirmed; 2026-09-26).

## Lunar Magic's layout on SA-1

300 further scenarios each of kobos_sprite_loader_spawns_as_lunar_magics_on_sa1 and kobos_exanimation_runs_as_lunar_magics passed (2026-10-01).

By hand: the twelve base patches on the SA-1 reference ROM, `render_hashes` against the reference ROM (all 512 levels the same but `012`, `0F8`, and `101`, which draw as the LoROM build does), and `examples/swap.rs` on Extended Interactions (an SA-1 hack's sites go back to SA-1 Pack's bytes: the SA-1 reference ROM, `KOBO_SA1_REFERENCE`). Results (2026-10-01): on the SA-1 reference ROM saved once by Lunar Magic, the VRAM patch and the base pieces swapped in draw all 512 levels as Lunar Magic's code does (and on vanilla saved once, as before). Extended Interactions' content moved into it (`transfer`, which takes an SA-1 hack into the SA-1 base): ExAnimation, taller levels, layer 3, and the sprite loader draw every level as Lunar Magic's code does; the base pieces differ in `01B` and `047`, the graphics loader in `00E`, `03B`, `0DE`, `180`, `189`, and `1DD`, and the VRAM patch in 18 levels, and the same content moved into the LoROM vanilla ROM differs in exactly the same levels: gaps of Kobo's code with this content, not of SA-1, left for the corpus sweep (step-2.md). Level `006` breaks (`BRK`) in both ROMs alike: its custom content needs the hack's own code.

## Sprite loader

By hand, Kaizo Kindergarten with Kobo's loader swapped in the same way (and PIXI's goal tape init kept) against the hack, every level, moving right and along a path up and down: the same.

## ExAnimation

Every trigger on every type is drawn but `0F` on frame types; 400 cases each of seeds 1 and 7 passed on both mappings (2026-10-01). The same comparison, hack by hack, on every level with a list (and every 16th with a global one) of the corpus's 32 LoROM hacks with ExAnimation found no difference in 1,332 levels (2026-09-28; Invictus level `136` breaks to `BRK` in both; Extended Interactions and Super Diagonal Mario 2, being SA-1, were left out then: see "Lunar Magic's layout on SA-1" above).

## Level data

Every run of `level_data` over the corpus on 2026-09-25 passed.

## MWL files

The MWL export of the vanilla ROM and 41 loose and `corpus_more` hacks: all 21,504 files passed on 2026-09-25, in two seconds.

## Full hack render sweep (2026-09-22)

The 2026-09-22 sweep at revision `25cca50e847ee679c500e2a287ef3e71faf3322f` ran
`kobo level png` in release mode on every slot `000`–`1FF`, with default sprite and
player rendering. It recursively included the local hack collection's ROMs and all
129 BPS patches, including QLDC entries and development projects. All patches applied
successfully to the headerless vanilla ROM. Grouping identical headerless ROM SHA-1s
and excluding five unmodified vanilla copies left 173 distinct hacks.

All 88,576 slots were attempted: 88,522 PNGs, 54 fatal failures, and 209 PNGs with
warnings; no attempt reached the export script's 120-second timeout. The failures and
warnings affect ten hacks. Their status and investigation priorities are recorded in
[known-gaps.md](known-gaps.md#full-hack-render-sweep-2026-09-22). This was not an emulator
comparison or a visual review of every PNG, and includes slots that may not be playable.

The local, uncommitted output is `~/Pictures/Kobo-level-renders/2026-09-22/`:

- `manifest.json`: source paths, headerless ROM hashes, duplicate aliases, exclusions,
  and the renderer revision. Temporary patched-ROM paths no longer exist; reapply the
  source BPS patch (`kobo bps apply`) when reproducing one of those entries.
- `results.jsonl`: every attempted slot's status and complete CLI diagnostics.
- `run-report.md`, `diagnostics.tsv`: per-hack totals and the failures and warnings.
- `index.html`: the PNG gallery, with diagnostic filters and optional filters for
  pictures identical to vanilla or to another slot in the same hack.
- `scripts/`: the one-off export, gallery, reporting and verification scripts. The
  export script starts a fresh run; use a separate output directory to keep this snapshot.

To reproduce an individual slot with its diagnostics, apply its patch if needed, then run:

```sh
cargo run --release -- level png 105 /tmp/kobo-level-105.png -r /path/to/hack.sfc
```

The export verification checked PNG headers and dimensions, preview presence, all
512 results per hack, and gallery links. These checks establish output completeness,
not correctness of the rendered game state.
