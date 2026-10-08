#!/usr/bin/env bash
# Dump the overworld a ROM loads with Mesen 2 (dump_overworld.lua).
# Usage: tools/oracle/dump_overworld.sh <rom> <outdir>
set -euo pipefail
rom=$1; out=$2
mesen=${MESEN:-$HOME/.local/share/kobo/tools/mesen2/Mesen}
mkdir -p "$out"
# Mesen wants a .sfc/.smc extension; copy so it never touches the source ROM.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$rom" "$tmp/rom.sfc"
KOBO_ORACLE_OUT=$(realpath "$out")
export KOBO_ORACLE_OUT LC_ALL=C.UTF-8
export DOTNET_ROOT=${DOTNET_ROOT:-$HOME/.dotnet} DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1
script=$(realpath "$(dirname "$0")/dump_overworld.lua")
set +e
xvfb-run -a "$mesen" --testRunner "$script" "$tmp/rom.sfc" --timeout="${KOBO_ORACLE_TIMEOUT:-180}" > "$out/mesen.stdout" 2>&1
code=$?
set -e
echo "mesen exit $code"
tail -n 3 "$out/oracle.log" 2>/dev/null || true
exit "$code"
