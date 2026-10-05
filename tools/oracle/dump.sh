#!/usr/bin/env bash
# Dump level oracle data from a ROM with Mesen 2.
# Usage: tools/oracle/dump.sh <rom> <outdir> <level>[,<level>...]   (levels in hex)
set -euo pipefail
rom=$1; out=$2; levels=$3
mesen=${MESEN:-$HOME/.local/share/kobo/tools/mesen2/Mesen}
mkdir -p "$out"
# Clean room (docs/clean-room.md): dump_levels.lua stops at three of the game's
# addresses, so each must still be the game's instruction or where a hook
# starts, not the middle of Lunar Magic's code.
python3 "$(dirname "$0")/../clean_room.py" sites "$rom" 0096D5 05D8B7 05DA65
# Mesen wants a .sfc/.smc extension; copy so it never touches the source ROM.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$rom" "$tmp/rom.sfc"
KOBO_ORACLE_OUT=$(realpath "$out")
export KOBO_ORACLE_OUT KOBO_ORACLE_LEVELS=$levels
export LC_ALL=C.UTF-8
script=$(realpath "$(dirname "$0")/dump_levels.lua")
set +e
"$mesen" --testRunner "$script" "$tmp/rom.sfc" --timeout="${KOBO_ORACLE_TIMEOUT:-900}" > "$out/mesen.stdout" 2>&1
code=$?
set -e
echo "mesen exit $code"
tail -n 5 "$out/oracle.log" 2>/dev/null || true
exit "$code"
