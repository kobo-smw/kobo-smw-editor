#!/usr/bin/env python3
"""Name the vanilla code or data a Lunar Magic change covers.

    sites.py --wla smw.wla vanilla.smc lm.smc 00C17A-00C17E 05D8E2 ...

For each SNES range (LoROM) it lists the vanilla instructions or data lines the range
touches, from SMWDisX's source (through the addr-to-line map of its WLA symbols), and which
byte offsets of each changed. Build the symbols once from the SMWDisX checkout:

    asar -D_VER=1 --symbols=wla --symbols-path=smw.wla smw.asm smw.smc

Clean room (docs/step-2.md): it prints SMWDisX's vanilla source and offsets, never the
bytes Lunar Magic wrote. The one exception is the interface of a hook: where a vanilla
jump (JSL, JML, JSR, JMP) keeps its opcode and gets a new operand, the bytes on either side
of the instruction unchanged, or where a change that was not vanilla's $FF free space
starts with one of those opcodes, the byte before it still vanilla's, it prints the jump
and its target, as romdiff.py does. A jump opcode inside a region Lunar Magic rewrote,
wherever the range given starts, is not decoded: its operand there is Lunar Magic's code.
Do not extend it to decode anything else it wrote.
"""
import argparse, bisect, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
from clean_room import is_vanilla

JUMPS = {0x22: ('JSL', 3), 0x5C: ('JML', 3), 0x20: ('JSR', 2), 0x4C: ('JMP', 2)}


def load(p):
    d = open(p, 'rb').read()
    return d[512:] if len(d) % 0x8000 == 512 else d


def pc(s):
    return ((s >> 16) & 0x7F) * 0x8000 + (s & 0x7FFF)


def read_wla(path):
    files, amap, sec = {}, [], None
    for line in open(path):
        line = line.strip()
        if line.startswith('['):
            sec = line
        elif line and sec == '[source files]':
            i, _, f = line.split()
            files[int(i, 16)] = f
        elif line and sec == '[addr-to-line mapping]':
            a, b = line.split()
            bank, addr = a.split(':')
            fi, ln = b.split(':')
            amap.append(((int(bank, 16) << 16) | int(addr, 16), int(fi, 16), int(ln, 16)))
    amap.sort()
    return files, amap


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--wla', required=True)
    p.add_argument('vanilla'); p.add_argument('lm'); p.add_argument('ranges', nargs='+')
    o = p.parse_args()
    files, amap = read_wla(o.wla)
    keys = [a for a, _, _ in amap]
    root = os.path.dirname(os.path.abspath(o.wla))
    src = {}

    def text(f, n):
        if f not in src:
            src[f] = open(os.path.join(root, files[f]), encoding='latin-1').read().split('\n')
        return src[f][n - 1].split(';')[0].strip()

    v, e = load(o.vanilla), load(o.lm)
    # Jumps are decoded against the vanilla ROM only: against another, a change can
    # start inside Lunar Magic's code (clean room).
    decode = is_vanilla(v)
    if not decode:
        print('jumps not decoded: the first ROM is not the vanilla ROM (clean room)')
    for r in o.ranges:
        s, _, t = r.partition('-')
        s = int(s.lstrip('$'), 16); t = int(t.lstrip('$'), 16) if t else s
        print(f'== ${s:06X}-${t:06X}')
        i = bisect.bisect_right(keys, s) - 1
        while i < len(amap) and amap[i][0] <= t:
            a, f, ln = amap[i]
            n = (amap[i + 1][0] if i + 1 < len(amap) else a + 1) - a
            i += 1
            line = text(f, ln)
            changed = [k for k in range(n) if v[pc(a + k)] != e[pc(a + k)]]
            data = n > 16 or line[:2] in ('db', 'dw', 'dl') or '%' in line[:1]
            note = ''
            op, new = v[pc(a)], e[pc(a)]
            # Only the operand changed: the bytes around the instruction did not.
            alone = v[pc(a) - 1] == e[pc(a) - 1] and v[pc(a + n)] == e[pc(a + n)]
            fill = all(v[pc(a + k)] == 0xFF for k in range(n))
            if not decode:
                pass
            elif not data and op in JUMPS and changed and 0 not in changed and alone:
                name, w = JUMPS[op]
                note = f'  -> {name} ${int.from_bytes(e[pc(a) + 1:pc(a) + 1 + w], "little"):0{2 * w}X}'
            elif (not data and 0 in changed and new in JUMPS and not fill
                  and v[pc(a) - 1] == e[pc(a) - 1]):
                name, w = JUMPS[new]
                note = f'  -> becomes {name} ${int.from_bytes(e[pc(a) + 1:pc(a) + 1 + w], "little"):0{2 * w}X}'
            kind = 'data' if data else f'[{n}]'
            print(f'   ${a:06X} {kind:5} {line[:50]:50} changed offsets {changed[:12]}{note}')


if __name__ == '__main__':
    main()
