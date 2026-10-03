#!/usr/bin/env python3
"""Diff two SMW ROMs (LoROM) as regions. Clean-room: never prints the contents of
changed code; at vanilla-area sites it reports only JSL/JML/JSR/JMP opcodes and targets,
and only where the first ROM had something there: a jump written into vanilla's $FF free
space is Lunar Magic's own code, not a hook, and is not decoded. Nor is anything when
Lunar Magic saved the first ROM (clean_room.py): a change there can start inside Lunar
Magic's code."""
import os, sys, struct
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
from clean_room import saved_by_lunar_magic
def load(p):
    b = open(p, 'rb').read()
    return b[512:] if len(b) % 0x8000 == 512 else b
def snes(pc): return ((pc // 0x8000) << 16) | (pc % 0x8000) | 0x8000
def rats(d):
    out = {}; i = 0
    while True:
        i = d.find(b'STAR', i)
        if i < 0: return out
        if i + 8 <= len(d):
            s, inv = struct.unpack_from('<HH', d, i + 4)
            if s ^ inv == 0xFFFF: out[i] = s + 1
        i += 1
def ranges(a, b, gap=8):
    n = max(len(a), len(b)); a = a.ljust(n, b'\0'); b = b.ljust(n, b'\0'); r = []; i = 0
    while i < n:
        if a[i] != b[i]:
            j = i; last = i
            while j < n and j - last <= gap:
                if a[j] != b[j]: last = j
                j += 1
            r.append((i, last + 1)); i = last + 1
        else: i += 1
    return r
OPS = {0x22: ('JSL', 3), 0x5C: ('JML', 3), 0x20: ('JSR', 2), 0x4C: ('JMP', 2)}
def main(pa, pb):
    a, b = load(pa), load(pb)
    print(f'size {len(a):#x} -> {len(b):#x}')
    decode = not saved_by_lunar_magic(a)
    if not decode:
        print('jumps not decoded: Lunar Magic saved the first ROM (clean room)')
    ra, rb = rats(a), rats(b)
    for s, e in ranges(a, b):
        tag = ''
        for t, sz in rb.items():
            if t <= s < t + 8 + sz or s <= t < e: tag = f' RATS@{snes(t):06X} size {sz:#x}'; break
        area = 'vanilla' if s < 0x80000 else 'expanded'
        line = f'{snes(s):06X}-{snes(e-1):06X} ({e-s:#6x} bytes, {area}){tag}'
        # A hook: a change of the first ROM's code, not of its $FF fill, starting there.
        hook = a[s - 1:s + 1] != b'\xff\xff' and a[s:s + 2] != b'\xff\xff'
        if decode and hook and area == 'vanilla' and e - s <= 8 and b[s] in OPS:
            name, w = OPS[b[s]]
            if all(x != 0xFF for x in a[s:s+1+w]):
                t = int.from_bytes(b[s+1:s+1+w], 'little')
                line += f'  {name} ${t:0{2*w}X}'
        print(line)
    gone = set(ra) - set(rb); new = set(rb) - set(ra)
    print(f'RATS blocks: {len(ra)} -> {len(rb)} (+{len(new)} -{len(gone)})')
if __name__ == '__main__': main(*sys.argv[1:3])
