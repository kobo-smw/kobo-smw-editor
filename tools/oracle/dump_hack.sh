#!/usr/bin/env bash
# Dump a spread of the levels a hack has changed, for tests/oracle_levels.rs
# with KOBO_ORACLE_ROM set to the hack.
# Usage: tools/oracle/dump_hack.sh <vanilla rom> <hack rom> <outdir> [count]
# A level counts as changed when its layer 1 pointer ($05E000 table) is not
# vanilla's; count (default 12) of them are taken at even intervals.
# KOBO_ORACLE_VIDEO=1 and the other dump.sh settings pass through.
set -euo pipefail
vanilla=$1; hack=$2; out=$3; count=${4:-12}

# The 512 three-byte pointers, one per line, past any copier header.
pointers() {
  local header=$(( $(stat -c %s "$1") % 32768 ))
  tail -c +$(( header + 0x2E000 + 1 )) "$1" | head -c 1536 | od -An -v -tx1 -w3
}

levels=$(paste -d'|' <(pointers "$vanilla") <(pointers "$hack") |
  awk -F'|' -v count="$count" '
    $1 != $2 { changed[n++] = NR - 1 }
    END {
      if (n < count) count = n
      for (i = 0; i < count; i++) printf "%s%03X", (i ? "," : ""), changed[int(i * n / count)]
    }')
if [ -z "$levels" ]; then
  echo "no level of $hack differs from vanilla" >&2
  exit 1
fi
echo "levels $levels"
exec "$(dirname "$0")/dump.sh" "$hack" "$out" "$levels"
