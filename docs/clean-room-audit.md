# Clean-room audit, 2026-10-03

Kobo looks at Lunar Magic only to interoperate with it, and never reads the instructions of
its code (step-2.md, "Clean room"). This is the record of the audit that checked that
against everything Kobo knew of Lunar Magic on 2026-10-03: how the rule was broken before
it covered everything it does now, what was learnt that way, what was removed for it, and
what stays and why. It names nothing of Lunar Magic's code: no address inside it, no byte
of it, nothing of how it works.

## How it was done

Seven reviews, each of one part, against the rule as step-2.md has it now: the docs
(`lunar-magic.md` and the rest; `lunar-magic-install.md` in two halves), Kobo's patches
(`asm/lunar-magic/`), the Rust library, examples, tests, and tools, the git history from
the first commit, and the transcripts of every agent session this machine has
(2026-09-24 on), to find what each session that wrote a piece of Kobo had seen of Lunar
Magic's code before it did. An eighth reviewed the six commits merged on 2026-10-02 while
the others ran. Then the sessions of step 1, from the machine it was done on (Claude Code
and Codex, 2026-09-14 to 09-27, kept outside the repository with the rest), were reviewed
the same way, by what each session ran and concluded rather than by reading its output
again. No review ran anything on a ROM Lunar Magic saved. Each fact was checked
for an allowed source (the community's documentation, Lunar Magic's help, the open tools'
sources, the vanilla game's code, region diffs, bisecting checked bytes, memory effects);
a fact with none, or one that described how Lunar Magic's code works rather than what it
leaves, was removed or restated as the effect seen, and the code that relied on one was
changed or removed.

## How the rule was broken

- Step 1 (2026-09-14 to 09-23) ran under a rule that forbade disassembling Lunar Magic's
  executable but said nothing of the code it puts in a ROM, and its tools showed that code
  by design: an instruction trace, the instruction behind a watched write, the reading
  instructions of `kobo level reads`. Its sessions show Lunar Magic's code read in 8 of the
  16 from 2026-09-14 to 09-19 and in 4 more to 09-24: `kobo level reads` listing the
  instructions that read the level data, byte dumps of the code at hook targets and of
  the routines that replace the game's, its decompression routine among them, one routine
  described step by step, and on 2026-09-24 a trace of the code that uploads a level's
  rows. No disassembler was run on it, and nothing of the executable was looked at.
  What the sessions concluded went into the docs of the time (removed above, or restated
  with an allowed source) and into two pieces of code, rewritten (below).
- From 2026-09-26 the rule covered the code in a ROM, but outputs still showed it until
  2026-10-02: CPU errors gave the address where a ROM's code stopped, which was sometimes
  inside Lunar Magic's code; `KOBO_RAM_WATCH` gave the writing instruction until
  2026-10-01 (used on Lunar Magic-saved ROMs on 2026-09-28 and 10-01, it printed only
  the game's and a hack's own code, `$02A975`, `$018174`, `$01ACA1`, `$02AC0C`,
  `$008A53`; reviewed and accepted, maintainer, 2026-10-03); `exlevel_probe call` gave the registers a routine returned; `romdiff.py` and
  `sites.py` decoded bytes inside regions Lunar Magic rewrote until 2026-09-27.
- On 2026-09-27 a session printed the byte before each of the entrance tables' pointers
  and named the instruction, beyond what any check needs.
- On 2026-09-27 a session overwrote the marker of a ROM Lunar Magic had saved, which then
  turned the trace off, and traced it. What it printed was Kobo's own code and the game's,
  as far as the record shows; it was not disclosed at the time.
- On 2026-10-02 a session ran `kobo level reads` on a hack Lunar Magic saved, read the
  byte at one of its hook sites across the corpus, and decoded one byte of its code next
  to another hook.

Since 2026-10-02 `kobo_core::clean_room` closes all of these (step-2.md).

## What was removed or restated

Docs:

- Addresses inside Lunar Magic's code, from traces, errors, and watches, and the
  descriptions of what that code does there: in `lunar-magic.md` (the expanded levels'
  layer split, the graphics upload, a hack's screen designation), `smw.md` (the initial
  tilemap upload, the rows a level's code moves), `testing.md`, `known-gaps.md` (the 2026-09
  hack sweep's causes: kept as what was observed), and `player.rs`, `tiles.rs`, `ram.rs`.
- How Lunar Magic's code works where only its effect is known: the order of the graphics
  upload's register accesses, which routine its code calls and with what register sizes,
  which registers its routines leave, what it tests, where it returns to, what reads a
  scratch byte, why its object code is where it is. Each is now the effect seen, with the
  probe that saw it, or gone. One review entry (registers on the taller levels' bounds)
  was removed with its premise.
- The instruction named before the entrance tables' pointers: the pointers are cited from
  the community's level format page, which documents them.
- The record of how things were found where it claimed more than was so ("printing
  addresses only" for the entrance pointers; `sites.py`'s output before 2026-09-27).

Code:

- `level::entrance_tables` checked Lunar Magic's opcode before each pointer; it now takes
  the documented pointers when each is the game's table or the start of a RATS block. Every
  ROM of the corpus gives the same tables as before.
- `exanimation` read Lunar Magic 2.41's older lists through an offset whose origin was not
  recorded, and converted that format; both are gone ([known-gaps.md](known-gaps.md)).
- `gfx::is_locked` compared the first bytes of the lock's routine, a fingerprint of Lunar
  Magic's code from step 1; it reads only the hook's opcode since 2026-10-02.
- Comments in the patches that described Lunar Magic's code now describe what a Lunar
  Magic-saved ROM leaves.

Found again by an allowed method: the level size table's place, `$240` before the
`$05DA8A` hook's target, by a byte diff (lunar-magic-install.md, "Taller levels").

## What stays, and why

- No patch was found to hold Lunar Magic's code beyond the bytes its checks need, which
  are registered in lunar-magic-install.md. Every fact a patch relies on now cites an
  allowed source; where Kobo's bytes at a site are small (the slanted pipe's speed, the
  palette fade's operands), they follow from the game's instruction there and the effect
  observed.
- Several patches were written by sessions that had seen something of Lunar Magic's code
  first, almost always an address in an error or one byte at a hook. The facts those
  sessions used are the ones above; the rest of their code follows the vanilla game, the
  open tools, and memory effects. The pieces written closest to an exposure are rewritten
  from the cleaned docs by sessions that do not see the old code: the background entry and
  its table routine (`background.asm`), whose shape followed a step-by-step description
  of Lunar Magic's own written in step 1, the graphics loader's putting work RAM aside in
  VRAM (`graphics.asm`), and the LC_LZ3 codec (`compress::lz3`), first written three
  hours after a session dumped Lunar Magic's decompression routine, though the format is
  the public one Pokémon Gold and Silver use. Each is written by a session working in a
  copy of the repository without its history or the old piece, from the cleaned docs,
  the vanilla game's code, the open tools, and public documentation:
  - `compress::lz3`, 2026-10-03, from pret/pokecrystal's description of the format; it
    decodes the corpus's LC_LZ3 hack as Lunar Magic's export does, and its encoder's
    output is as small as the old one's.
  - `graphics.asm`'s `park` and `unpark`, 2026-10-03, from the SNES hardware's
    documentation of the VRAM port and DMA (the read-ahead word a read of `$2139` gives
    first); `tests/graphics_build.rs` now checks them, which nothing did before.
  - `background.asm`, 2026-10-03, from the docs' observed effects, the community's level
    format page, and the game's code at the two hooks; Kaizo Kindergarten built by Kobo
    gives every background level the hack's flags, table, stride, tilemap, and BG Map16.
    Where the docs were not specific (which table a background with `C` and not `F`
    takes) it went by what a ROM shows, and the docs now say so. Two others were written close to an exposure but rest on a source of their own:
  the screen exit hook's site (`exits.asm`), where a region diff shows Lunar Magic's change
  starting, and the slanted pipe's speed (`entrance.asm`), from the RAM a Lunar
  Magic-saved ROM leaves; both are recorded in lunar-magic-install.md.
- The emulator oracle stops at three of the game's addresses; one is where a change of
  Lunar Magic's starts, a hook, which the rule allows (`tools/clean_room.py sites`).
- Some code follows a finding a trace led to but holds nothing of Lunar Magic's: the
  player pass puts the loader's tilemaps back after its frames (`expand::player`), because
  a level's code moves a layer and the rows it uploads then differ from the picture's
  position, which an emulator frame confirms; levels are entered as screen exits
  (`expand::load`), which rests on the game's own code; and Kobo calls the hook targets
  `$06F540` and `$0EFD00` by the contracts the vanilla instructions they cover give.
- Hook sites first learnt by printing Lunar Magic's bytes in step 1 are cited from an
  address-only diff now (the column upload's `$058A65`, the initial upload's `$0580D3`).

## What the audit could not see

- What an agent thought, beyond the summaries the session logs keep.
- Which of some crash addresses were in Lunar Magic's code and which in a hack's: telling
  them apart would mean looking at the code. None of them is in Kobo any more.
- Work done outside an agent session.
