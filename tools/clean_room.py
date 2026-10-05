#!/usr/bin/env python3
"""The clean room for Kobo's scripts (docs/clean-room.md), as
kobo_core::clean_room is for the library: what tells a ROM Lunar Magic saved, and
where a script may stop or call into a ROM's code.

    clean_room.py check rom.smc            exit 1 if Lunar Magic saved the ROM
    clean_room.py sites rom.smc ADDR...    exit 1 if a ROM's code at an address is
                                           neither the game's nor where a hook starts
                                           (the vanilla ROM from KOBO_SMW_ROM or the
                                           config file)

Imported by romdiff.py and sites.py; run by the Mesen wrappers in tools/oracle/.
"""
import hashlib, os, sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import kobo_config  # noqa: E402

VANILLA_SHA1 = '6b47bb75d16514b6a476aa0c73a683a2a4c18765'
MARKER = 0x0FF0A0
# Lunar Magic fills these with its own code on every save, whatever is there; vanilla,
# Kobo's builds, SA-1 Pack, and the pinned tools leave them $FF
# (kobo_core::clean_room::SAVE_AREAS).
SAVE_AREAS = [(0x03BB00, 0x1F), (0x03BCA0, 0x20)]  # not $0EF510: Kobo's background entry


def load(path):
    data = open(path, 'rb').read()
    return data[512:] if len(data) % 0x8000 == 512 else data


def pc(addr):
    """LoROM file offset; SA-1 Pack's first 4 MiB are laid out the same."""
    return ((addr >> 16) & 0x7F) * 0x8000 + (addr & 0x7FFF)


def is_vanilla(data):
    return hashlib.sha1(data).hexdigest() == VANILLA_SHA1


def saved_by_lunar_magic(data):
    """The marker, or anything but $FF in one of the areas every save fills."""
    if data[pc(MARKER):pc(MARKER) + 20] == b'Lunar Magic Version ':
        return True
    return any(any(b != 0xFF for b in data[pc(a):pc(a) + n]) for a, n in SAVE_AREAS)


def may_stop(data, vanilla, addr):
    """Whether a script may stop at, or call into, `addr` (kobo_core::clean_room::may_call),
    an address where one of the game's instructions starts: the instruction is still there,
    as its first two bytes say, or a hook, where a change to the game's code starts (the
    byte before is still the game's). Anywhere else may be the middle of Lunar Magic's
    code, which breakpoints or calls would read an instruction at a time."""
    at = pc(addr)
    # Vanilla's free space is $FF, and Lunar Magic writes its code there: two $FF in a
    # row are not the game's code.
    def code(i):
        return vanilla[i:i + 2] != b'\xff\xff'
    if data[at:at + 2] == vanilla[at:at + 2] and code(at):
        return True
    return (addr & 0xFFFF != 0x8000 and data[at] != vanilla[at]
            and data[at - 1] == vanilla[at - 1] and code(at - 1))


def vanilla_path():
    path = kobo_config.path('rom')
    if path is None:
        sys.exit('no vanilla ROM: set KOBO_SMW_ROM or `roms.smw`')
    return path


def main(args):
    if len(args) >= 2 and args[0] == 'check':
        if saved_by_lunar_magic(load(args[1])):
            print(f'{args[1]}: Lunar Magic saved it (clean room)', file=sys.stderr)
            return 1
        return 0
    if len(args) >= 3 and args[0] == 'sites':
        data, vanilla = load(args[1]), load(vanilla_path())
        bad = [a for a in args[2:] if not may_stop(data, vanilla, int(a.lstrip('$'), 16))]
        if bad:
            print(f'{args[1]}: not the game\'s code or a hook at {", ".join(bad)} (clean room)',
                  file=sys.stderr)
            return 1
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
