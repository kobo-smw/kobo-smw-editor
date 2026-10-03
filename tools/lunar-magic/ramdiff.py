#!/usr/bin/env python3
"""Compare the memory two ROMs leave behind after Kobo's emulator loads the same levels.

    ramdiff.py a.smc b.smc 105 106 ...        byte runs that differ, per level
    ramdiff.py --summary a.smc b.smc all      addresses that differ, counted over levels
    ramdiff.py --revert 05D8E2-05D8E5 vanilla.smc lm.smc 105
                                              ablation: b with those ranges put back to
                                              a's bytes, against b itself, to see what the
                                              reverted hook leaves behind
    options: --kobo PATH (default: kobo on PATH), --vram (also VRAM and CGRAM),
             --sa1 (also an SA-1 ROM's I-RAM and BW-RAM, raw, at their bus addresses)

Clean room (docs/step-2.md): this reports memory effects only, addresses and the
values each ROM left there after `kobo level wram` / `kobo level dump` ran its load.
It never prints instructions, program counters, or ROM bytes, so it may be run on
Lunar Magic-saved ROMs to find what a hook leaves behind. Do not extend it to print
where in the ROM a write came from, or to dump ROM contents: that would be an
instruction trace or a copy of Lunar Magic's code. Work RAM itself can hold code a ROM
copies there (vanilla builds an OAM routine at $7F8000): a run of differing bytes there
may be code, so do not read such a run as values.
"""
import argparse, collections, os, subprocess, sys, tempfile

WRAM = 0x20000
STACK = (0x0110, 0x0200)


# An SA-1 ROM's own memories, after work RAM: I-RAM and BW-RAM as the S-CPU
# sees them, raw (--sa1). Reported at their bus addresses.
SA1 = [(0x003000, 0x800), (0x400000, 0x40000)]
SA1_STACK = (0x003700, 0x003800)


def dump(kobo, rom, level, start, length, bus=False):
    out = subprocess.run([kobo, 'level', 'wram', '-r', rom, level, f'${start:06X}', str(length)]
                         + (['--bus'] if bus else []), capture_output=True, text=True)
    if out.returncode:
        return None
    data = bytearray()
    for line in out.stdout.splitlines():
        data += bytes.fromhex(line.split(':', 1)[1])
    return data


def wram(kobo, rom, level, sa1=False):
    data = dump(kobo, rom, level, 0x7E0000, WRAM)
    if data is None:
        return None
    for start, length in SA1 if sa1 else []:
        more = dump(kobo, rom, level, start, length, bus=True)
        if more is None:
            return None
        if start == 0x003000:
            # I-RAM's last page holds the SA-1's stack: return addresses.
            more[SA1_STACK[0] - start:SA1_STACK[1] - start] = bytes(SA1_STACK[1] - SA1_STACK[0])
        data += more
    # The stack holds return addresses, which are program counters: blank it, and
    # keep $0100-$010F, which the game and the tools use as variables.
    data[STACK[0]:STACK[1]] = bytes(STACK[1] - STACK[0])
    return bytes(data)


def video(kobo, rom, level):
    with tempfile.TemporaryDirectory() as d:
        out = subprocess.run([kobo, 'level', 'dump', '-r', rom, level, d], capture_output=True)
        if out.returncode:
            return None
        n = int(level, 16)
        read = lambda k: open(os.path.join(d, f'level_{n:03X}.{k}.bin'), 'rb').read()
        return {'vram': read('vram'), 'cgram': read('cgram')}


def reverted(a, b, spec):
    """b's image with the given LoROM ranges taken from a: an ablation. Headerless."""
    def load(p):
        d = open(p, 'rb').read()
        return d[512:] if len(d) % 0x8000 == 512 else d
    va, out = load(a), bytearray(load(b))
    for r in spec.split(','):
        s, _, e = r.partition('-')
        s = int(s.lstrip('$'), 16); e = int((e or s).lstrip('$'), 16) if e else s
        for x in range(s, e + 1):
            o = ((x >> 16) & 0x7F) * 0x8000 + (x & 0x7FFF)
            out[o] = va[o]
    return bytes(out)


def bus(x):
    """The address of offset x in a dump: work RAM, then the SA-1 regions."""
    if x < WRAM:
        return 0x7E0000 + x
    x -= WRAM
    for start, length in SA1:
        if x < length:
            return start + x
        x -= length
    raise ValueError(x)


def runs(a, b, gap=4):
    i, n, out = 0, min(len(a), len(b)), []
    while i < n:
        if a[i] == b[i]:
            i += 1
            continue
        j = last = i
        while j < n and j - last <= gap:
            if a[j] != b[j]:
                last = j
            j += 1
        out.append((i, last + 1))
        i = last + 1
    return out


def show(name, base, a, b, limit=24):
    for s, e in runs(a, b):
        cut = min(e, s + limit)
        more = ' ...' if e > cut else ''
        at = bus(s) if base is None else base + s
        print(f'  {name} ${at:06X}+{e - s:<4} {a[s:cut].hex(" ")}{more}  ->  {b[s:cut].hex(" ")}{more}')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('a'); p.add_argument('b'); p.add_argument('levels', nargs='+')
    p.add_argument('--kobo', default='kobo'); p.add_argument('--vram', action='store_true')
    p.add_argument('--summary', action='store_true')
    p.add_argument('--sa1', action='store_true')
    p.add_argument('--revert', help='SNES ranges (6-digit hex, START-END,...) copied from a '
                   'into a temporary copy of b, which then stands in for a')
    o = p.parse_args()
    levels = [f'{n:X}' for n in range(0x200)] if o.levels == ['all'] else o.levels
    if o.revert:
        tmp = tempfile.NamedTemporaryFile(suffix='.smc', delete=False)
        tmp.write(reverted(o.a, o.b, o.revert)); tmp.close()
        o.a = tmp.name
    try:
        compare(o, levels)
    finally:
        if o.revert:
            os.unlink(o.a)


def compare(o, levels):
    count, loaded = collections.Counter(), 0
    for lv in levels:
        a, b = wram(o.kobo, o.a, lv, o.sa1), wram(o.kobo, o.b, lv, o.sa1)
        if a is None or b is None:
            print(f'level {lv}: failed to load in {"a" if a is None else "b"}', file=sys.stderr)
            continue
        loaded += 1
        if o.summary:
            for s, e in runs(a, b, gap=0):
                for x in range(s, e):
                    count[bus(x)] += 1
            continue
        print(f'level {lv}:')
        show('ram', None, a, b)
        if o.vram:
            va, vb = video(o.kobo, o.a, lv), video(o.kobo, o.b, lv)
            if va and vb:
                show('vram', 0, va['vram'], vb['vram'], 16)
                show('cgram', 0, va['cgram'], vb['cgram'], 16)
    if o.summary:
        print(f'{loaded} levels loaded in both; addresses that differ, with the number of levels:')
        addrs = sorted(count)
        i = 0
        while i < len(addrs):
            j = i
            while j + 1 < len(addrs) and addrs[j + 1] == addrs[j] + 1 and count[addrs[j + 1]] == count[addrs[i]]:
                j += 1
            print(f'  ${addrs[i]:06X}-${addrs[j]:06X}: {count[addrs[i]]}')
            i = j + 1


if __name__ == '__main__':
    main()
