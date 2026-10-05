#!/usr/bin/env python3
"""Kobo's settings for the scripts in tools/, found as the test suite finds
them (kobo_core::tiers): the environment variable, then the config file
(`roms.smw`, the `[tests]` table). Needs Python 3.11 (tomllib).

    kobo_config.py NAME     prints the setting's path, or its paths a line each
                            (lm_roms, video_oracle); exits 1 if it is not set

NAME is a tier's name as `cargo xtask tiers` prints it: rom, lunar_magic,
lm_roms, mwl, oracle, boss_oracle, video_oracle, 65816_tests, sa1_reference.
tests/harness.rs checks that this file and the library agree.
"""

import os
import sys
import tomllib
from pathlib import Path

# name: (environment variable, table, key, a list)
SETTINGS = {
    "rom": ("KOBO_SMW_ROM", "roms", "smw", False),
    "lunar_magic": ("KOBO_LUNAR_MAGIC", "tests", "lunar_magic", False),
    "lm_roms": ("KOBO_LM_ROMS", "tests", "lm_roms", True),
    "mwl": ("KOBO_MWL_DIR", "tests", "mwl_dir", False),
    "oracle": ("KOBO_ORACLE_DIR", "tests", "oracle_dir", False),
    "boss_oracle": ("KOBO_BOSS_ORACLE_DIR", "tests", "boss_oracle_dir", False),
    "video_oracle": ("KOBO_VIDEO_ORACLE_DIRS", "tests", "video_oracle_dirs", True),
    "65816_tests": ("KOBO_65816_TESTS", "tests", "cpu_tests", False),
    "sa1_reference": ("KOBO_SA1_REFERENCE", "tests", "sa1_reference", False),
}


def config_path():
    home = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    return Path(home) / "kobo" / "config.toml"


def expand_roms(entries):
    """A file is itself; a folder its ROMs (.smc, .sfc) and .bps patches in
    name order, a patch left out when a ROM of the same name is beside it
    (kobo_core::tiers::expand_roms)."""
    out = []
    for entry in entries:
        if not entry.is_dir():
            out.append(entry)
            continue
        files = sorted(p for p in entry.iterdir() if p.is_file())
        roms = {p.stem for p in files if p.suffix.lower() in (".smc", ".sfc")}
        out.extend(p for p in files
                   if p.suffix.lower() in (".smc", ".sfc")
                   or (p.suffix.lower() == ".bps" and p.stem not in roms))
    return out


def paths(name):
    """The setting's paths, or None if it is not set."""
    env, table, key, is_list = SETTINGS[name]
    value = os.environ.get(env)
    if value:
        found = [Path(p) for p in value.split(os.pathsep) if p] if is_list else [Path(value)]
    else:
        try:
            with open(config_path(), "rb") as f:
                value = tomllib.load(f).get(table, {}).get(key)
        except FileNotFoundError:
            value = None
        if value is None:
            return None
        found = [Path(p).expanduser() for p in (value if is_list else [value])]
    return expand_roms(found) if name == "lm_roms" else found


def path(name):
    """The setting's first path, or None."""
    found = paths(name)
    return found[0] if found else None


def main(args):
    if len(args) != 1 or args[0] not in SETTINGS:
        print(__doc__, file=sys.stderr)
        return 2
    found = paths(args[0])
    if not found:
        env, table, key, _ = SETTINGS[args[0]]
        print(f"{args[0]} is not set: {env} or `{table}.{key}` in {config_path()}", file=sys.stderr)
        return 1
    for p in found:
        print(p)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
