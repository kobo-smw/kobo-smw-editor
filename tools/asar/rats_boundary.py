#!/usr/bin/env python3
"""Reproduce Asar 1.91's RATS bank-boundary overwrite using a synthetic ROM.

This diagnostic expects the known bug; see docs/toolchain.md. No game ROM is used.
"""

import argparse
from pathlib import Path
import struct
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("asar", nargs="?", default="asar", help="Asar 1.91 executable")
    args = parser.parse_args()
    subprocess.run([args.asar, "--version"], check=True)

    data = bytearray(b"\xff" * 0x80000 + b"\x00" * 0x80000)
    data[0x7FD5] = 0x20  # LoROM
    # Kobo's first two allocations: contents at PC 0x88000 and 0x90008.
    for offset, length in [(0x87FF8, 0x8000), (0x90000, 0x7FF8)]:
        size = length - 1
        data[offset : offset + 8] = b"STAR" + struct.pack("<HH", size, size ^ 0xFFFF)

    with tempfile.TemporaryDirectory(prefix="kobo-asar-rats-") as directory:
        root = Path(directory)
        rom = root / "synthetic.sfc"
        patch = root / "boundary.asm"
        rom.write_bytes(data)
        patch.write_text("lorom\nfreecode cleaned\nfillbyte $00\nfill $8000\n")
        subprocess.run([args.asar, "--no-title-check", str(patch), str(rom)], check=True)
        result = rom.read_bytes()

    overwritten = result[0x97FF8:0x98000]
    expected = b"STAR\xff\x7f\x00\x80"
    if overwritten != expected or result[0x90000:0x97FF8] != data[0x90000:0x97FF8]:
        raise SystemExit("The Asar 1.91 overwrite was not reproduced; investigate this version.")
    print(f"Reproduced: protected zeros at PC 0x97FF8 became {overwritten.hex(' ')}")


if __name__ == "__main__":
    main()
