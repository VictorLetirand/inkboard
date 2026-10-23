#!/usr/bin/env bash
# Load-test matrix of shared sessions: host and guests in two processes on this machine (loopback), one run per (guests, objects).
# Usage: scripts/crowd-matrix.sh OUT_DIR "5 10 20 50 100" "1000 5000 20000" [extra options of `crowd run`...]
# Needs: cargo +1.99.1; builds the host test executable and the `crowd` tool first (release, no LTO: faster to build, same hot code).
# Method and how to read the results: context/mcp-remote/test-charge.md
set -u
OUT="${1:?output folder}"
GUESTS="${2:-5 10 20 50 100}"
OBJECTS="${3:-1000 5000 20000}"
shift 3 2>/dev/null || shift $#
cd "$(dirname "$0")/.."
export CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo +1.99.1 build --release -p inkboard-crowd || exit 1
# HOST_EXE: a test executable kept from an earlier build (to compare two versions of the host under the same conditions)
EXE="${HOST_EXE:-}"
if [ -z "$EXE" ]; then
  EXE=$(cargo +1.99.1 test -p inkboard-app --release --no-run --message-format=json 2>/dev/null \
    | grep -o '"executable":"[^"]*inkboard-[0-9a-f]*\.exe"\|"executable":"[^"]*deps/inkboard-[0-9a-f]*"' | head -1 | sed 's/"executable":"//; s/"$//; s/\\\\/\//g')
fi
[ -n "$EXE" ] || { echo "cannot find the test executable of inkboard-app"; exit 1; }
# WAIT_IDLE=50: wait (30 minutes at most) until the computer is below 50 % busy before each run; the load is in the report either way
WAIT=""
[ -n "${WAIT_IDLE:-}" ] && WAIT="--wait-idle $WAIT_IDLE"
CROWD=target/release/crowd
[ -x "$CROWD" ] || CROWD=target/release/crowd.exe
mkdir -p "$OUT"
for o in $OBJECTS; do
  for g in $GUESTS; do
    procs=$(( g > 40 ? 2 : 1 ))
    echo "== $g guests, $o objects"
    "$CROWD" run $WAIT --host-exe "$EXE" --guests "$g" --objects "$o" --procs "$procs" --duration "${DURATION:-40}" --settle "${SETTLE:-12}" --out "$OUT/g$g-o$o" "$@" > "$OUT/g$g-o$o.stdout" 2>&1
  done
done
"$CROWD" matrix "$OUT"/g*-o*/ > "$OUT/matrix.md" 2>&1 || true
cat "$OUT/matrix.md"
