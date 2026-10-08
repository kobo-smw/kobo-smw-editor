# The clean room

Why Kobo looks at Lunar Magic only to interoperate with it, what that allows and rules
out, and how Kobo's own outputs keep to it. Settled for step 2 (2026-09-25, extended
2026-09-27 and 2026-10-02); the rule itself is in [AGENTS.md](../AGENTS.md). The audit
that checked everything Kobo knew of Lunar Magic against it is
[clean-room-audit.md](clean-room-audit.md).

## The rule

- Purpose: Kobo looks at what Lunar Magic does for one reason, to interoperate with it, so
  that Lunar Magic and the tools around it can open, save, and extend a Kobo build. Kobo
  observes Lunar Magic only as far as that needs, and matches the least that a check
  requires. Anything learned that is not needed for interoperability is not used.
- Interface, which Kobo matches exactly: hook addresses, where each table and block lives
  and its format, the RAM values a hook leaves for other code (`$13D7`, `$7FC00B`), and the
  bytes Lunar Magic or another tool checks for.
- A pointer Lunar Magic keeps at a fixed offset in the code it installs is interface too,
  once documentation or a byte diff shows where it is: `read3($05D9E4)+$0A` (the midway
  tables), `$05DC86` and `$05DC8B` (the secondary entrance tables), `$0FF873` and
  `$0FF937` (ExGFX), `$0DE191`, `$0DE198`, `$0DE19F`, and `$05DC81` (the secondary entrance
  tables where Lunar Magic moved them). Kobo reads and writes such a pointer, and shapes its
  own code so the pointer sits at the same offset. Decided 2026-09-27.
- Bytes Lunar Magic checks before it treats a piece as installed are interface when Kobo
  needs that piece kept: a marker, or the opcode of an instruction around a pointer. Kobo
  writes exactly the bytes the check needs, no more, and records each case, with how it was
  found and what it makes Lunar Magic do, in
  [lunar-magic-install.md](lunar-magic-install.md#bytes-kobo-writes-because-lunar-magic-or-other-tools-check-them).
  Such bytes can tell Lunar Magic more than Kobo means, so every one is tested with and
  without it ([testing.md](testing.md)). Decided 2026-09-27.
- Implementation, which Kobo writes itself: the code behind each hook, and every routine.
  Where a checked byte is an instruction's, the instruction is Kobo's own and does its work
  in Kobo's code.
- Allowed evidence: SMWCentral and SNESLab documentation, Lunar Magic's readme and help
  file, the sources of open tools (PIXI, GPS, UberASM Tool, SA-1 Pack), a community
  patch's hook sites and stated behaviour, read to interoperate with it, though not its
  code where that says it is adapted from Lunar Magic's (decided 2026-10-08), byte diffs of a ROM
  before and after a Lunar Magic operation, bisecting which bytes make Lunar Magic keep or
  export something (printing addresses only), and running Lunar Magic-saved ROMs to
  observe which addresses they read and write, with what values.
- Not allowed: reading Lunar Magic's instructions, as a disassembly of the ROM or the
  executable or as an instruction trace (`KOBO_CPU_TRACE` over its code), and copying its
  code or any bytes a check does not need. Nor is anything else that shows where its
  instructions are or what they do: the address in a CPU error, the instruction behind a
  watched write or a data read, the registers a routine of its code returns, the stack.
  Bytes of its code seen by accident and not used (a tool printing a changed range whole)
  need no record; fix the tool that printed them.
- Where Lunar Magic recognises a piece only by its code, beyond bytes a check needs, Kobo
  does not match it; Lunar Magic then installs its own over Kobo's in the copy it saves,
  which a build allows ([build.md](build.md#lunar-magic-and-kobo-builds)).

## How Kobo's ROM-side code was written

Lunar Magic's runtime code had to be reimplemented without reading it: from its help file
(which documents the GFX decompression routine, the screen exit routine, and the Map16
acts-like code's contract and `JSL` slots; [lunar-magic.md](lunar-magic.md)), tool
sources, and the memory effects of running Lunar Magic-saved ROMs. The method, per piece:
a probe that runs the ROM's own code with chosen state and records what it leaves
(`examples/*_probe.rs`), then an A/B comparison with Kobo's code swapped in for Lunar
Magic's (`examples/swap.rs`), and `install-gate.py` for the checks a save
makes ([testing.md](testing.md)). What each piece was found to do, and how, is in
[lunar-magic-install.md](lunar-magic-install.md).

GPS patches the code it finds at fixed places in bank `$06` (the entry slots from
`$06F690`, the compare chain at `$06F67B` and `$06F717`, the exit at `$06F602`), so Kobo's
code there has the shape GPS's source describes. That pulls it towards Lunar Magic's; it
is written from GPS's source, the vanilla disassembly, and observed behaviour only, and
reviewed with that in mind. A licence for GPS would let Kobo patch it to use the
documented `JSL` slots instead ([toolchain.md](toolchain.md)).

## Kobo's outputs

Decided 2026-10-02. `kobo_core::clean_room` takes a ROM as Lunar Magic's by its marker, or
by code in two areas every save fills whatever is there (`$03BB00`, `$03BCA0`), so a hack
whose marker was removed is still one. Once a process has loaded such a ROM, nothing it
prints or writes says where an instruction of a ROM's code is: no trace, no address in a
CPU error or for an instruction fetched from unmapped memory, no instruction behind a
watched write or a data read, no registers from a called routine, which may only be
entered at the game's code or a hook, and no stack bytes in a RAM dump; `kobo asm`
withholds what a patch prints. The scripts check the same way (`tools/clean_room.py`):
`romdiff.py` and `sites.py` decode a hook's jump only against a ROM Lunar Magic did not
save, `trace_writes.lua` refuses one, and the emulator oracle stops only at the game's
instructions or a hook. A debugging aid added later goes through the module, or says why
it shows nothing of a ROM's code.

## History

Step 1 (2026-09-14 to 09-23) ran before the rule covered the code Lunar Magic puts in a
ROM, and traced it; so did later work through the outputs above, until 2026-10-02.
Everything Kobo knew of Lunar Magic was audited on 2026-10-03: what came that way was
removed, with the code that relied on it, and every fact kept cites an allowed source
([clean-room-audit.md](clean-room-audit.md)).

## Why

Matching what another program checks, in order to interoperate with it, is the position
the law has long recognised (*Sega v. Accolade*, 1992, where copying Sega's small lockout
code so games would run was fair use; the EU Software Directive allows even decompiling
for interoperability). Kobo stays well inside it by never reading Lunar Magic's code at
all. The concern beyond that is provenance and trust. Hook code is small and constrained,
so code written after reading Lunar Magic's comes out nearly the same, and an agent with a
disassembly in context reproduces it. Every patch has to be licensable under MPL-2.0, and
the project depends on the community's trust (compare Wine's contribution policy, or
ReactOS halting for an audit in 2006). Stating the purpose, and keeping to the least it
needs, is what makes that position clear.

The standing risk is contamination, from an agent or a contributor working from Lunar
Magic's code. The tooling above keeps Kobo's own outputs from showing it; the review of
every change to `asm/` and to the docs that describe Lunar Magic is what keeps the rest
out.
