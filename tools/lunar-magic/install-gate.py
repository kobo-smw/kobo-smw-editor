#!/usr/bin/env python3
"""Find what makes Lunar Magic's save treat one piece of its install as present.

    install-gate.py base.smc lm.smc vanilla.smc level.mwl WATCH [WATCH...]

Lunar Magic's save installs its pieces one by one into a ROM that lacks them, rewriting
their sites and resetting their tables (docs/lunar-magic-install.md). For each WATCH, a
SNES address or range (`00BDA8` or `06FC00-06FFFF`), this bisects over the ranges where
lm.smc (vanilla after one Lunar Magic save) differs from vanilla: which of them, copied
onto base.smc (a ROM Lunar Magic installs into, such as Kobo's), stop a save (importing
level.mwl as level 105) from rewriting WATCH. A watched range that holds a table is filled
with a marker first, so that resetting it shows.

Clean room (docs/clean-room.md): it prints SNES addresses only, never the bytes of either ROM.
The ROMs it writes are scratch copies in a temporary directory, never kept. Run it where
Lunar Magic's restore system finds the original ROM (it copies vanilla.smc there itself).
KOBO_LM replaces the Lunar Magic wrapper (default: lm beside this script). A save that
fails, or does not report the level imported, stops the search with its output.
"""
import os, shutil, subprocess, sys, tempfile

LM = os.environ.get('KOBO_LM') or os.path.join(os.path.dirname(os.path.realpath(__file__)), 'lm')
MARK = 0x5A


def pc(a): return ((a >> 16) & 0x7F) * 0x8000 + (a & 0x7FFF)
def snes(i): return ((i // 0x8000) << 16) | (i % 0x8000) | 0x8000


def load(p):
    d = open(p, 'rb').read()
    return bytearray(d[len(d) % 0x8000:])


def span(text):
    a, _, b = text.partition('-')
    s = pc(int(a, 16))
    return s, (pc(int(b, 16)) + 1 if b else s + 4)


def main():
    if len(sys.argv) < 6:
        sys.exit(__doc__)
    base, lm, vanilla = (load(p) for p in sys.argv[1:4])
    mwl = os.path.realpath(sys.argv[4])
    watches = [span(w) for w in sys.argv[5:]]
    work = tempfile.mkdtemp()
    os.makedirs(os.path.join(work, 'sysLMRestore'))
    original = open(sys.argv[3], 'rb').read()
    if len(original) % 0x8000 == 0:
        original = bytes(512) + original
    open(os.path.join(work, 'sysLMRestore', 'smwOrig.smc'), 'wb').write(original)
    limit = min(len(base), len(lm), 0x80000)
    # Level pointers and the watched ranges are left out of the candidates.
    skip = [(pc(0x05E000), pc(0x05F000))] + watches
    runs, i = [], 0
    while i < limit:
        if lm[i] != vanilla[i] and not any(s <= i < e for s, e in skip):
            j = i
            while j < limit and not any(s <= j < e for s, e in skip) and \
                    any(lm[k] != vanilla[k] for k in range(j, min(j + 16, limit))):
                j += 1
            runs.append((i, j))
            i = j
        else:
            i += 1

    def kept(sub, watch):
        s, e = watch
        d = bytearray(base)
        for a, b in sub:
            d[a:b] = lm[a:b]
        table = e - s > 4
        if table:
            d[s:e] = bytes([MARK]) * (e - s)
        before = bytes(d[s:e])
        rom = os.path.join(work, 't.smc')
        open(rom, 'wb').write(d)
        # A save that did not happen leaves the ROM as it was, which would
        # read as kept: stop instead.
        r = subprocess.run([LM, '-ImportLevel', rom, mwl, '105'], cwd=work,
                           capture_output=True, text=True)
        if r.returncode != 0 or 'imported' not in r.stdout:
            shutil.rmtree(work, ignore_errors=True)
            sys.exit(f'Lunar Magic did not save (exit {r.returncode}):\n{r.stdout}{r.stderr}')
        after = load(rom)[s:e]
        return after.count(MARK) > (e - s) // 2 if table else after == before

    fmt = lambda rs: ' '.join('%06X-%06X' % (snes(a), snes(b - 1)) for a, b in rs)
    for watch in watches:
        name = '%06X-%06X' % (snes(watch[0]), snes(watch[1] - 1))
        if kept([], watch):
            print(f'{name}: kept with nothing copied', flush=True)
            continue
        if not kept(runs, watch):
            print(f'{name}: rewritten even with every range copied', flush=True)
            continue
        cand = runs
        while len(cand) > 1:
            a, b = cand[:len(cand) // 2], cand[len(cand) // 2:]
            if kept(a, watch):
                cand = a
            elif kept(b, watch):
                cand = b
            else:
                print(f'{name}: needs ranges from both halves of {fmt(cand)}', flush=True)
                break
        else:
            print(f'{name}: kept once {fmt(cand)} is copied', flush=True)
    shutil.rmtree(work)


if __name__ == '__main__':
    main()
