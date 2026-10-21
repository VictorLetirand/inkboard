#!/usr/bin/env bash
# Runs the native scenario for one network mode: host + 2 concurrent guests, then a cut/resume test.
# Usage: ./run-native.sh direct|relay-n0|relay-local|hybrid-n0   (expects `cargo build --profile native --features native`)
set -u
cd "$(dirname "$0")"
NET="$1"
OUT="out/$NET"
mkdir -p "$OUT"
rm -f "$OUT"/*
BIN=target/native

RUST_LOG="${HOST_LOG:-info,iroh=warn}" $BIN/host.exe --net "$NET" --ticket-file "$OUT/ticket" >"$OUT/host.stdout" 2>"$OUT/host.log" &
HOST_PID=$!
for _ in $(seq 1 100); do
  [ -s "$OUT/ticket" ] && break
  sleep 0.3
done
[ -s "$OUT/ticket" ] || { echo "host did not produce a ticket"; cat "$OUT/host.log"; kill $HOST_PID; exit 1; }

# Two guests at the same time (full scenario each: 1000 pings, 5 MiB down, 5 MiB up).
RUST_LOG="${GUEST_LOG:-info,iroh=warn}" $BIN/guest.exe --ticket-file "$OUT/ticket" >"$OUT/guest1.json" 2>"$OUT/guest1.log" &
G1=$!
RUST_LOG="${GUEST_LOG:-info,iroh=warn}" $BIN/guest.exe --ticket-file "$OUT/ticket" >"$OUT/guest2.json" 2>"$OUT/guest2.log" &
G2=$!
wait $G1 $G2

# Abrupt cut (process exit without close frame), then resume with the same participant id.
$BIN/guest.exe --ticket-file "$OUT/ticket" --cut >"$OUT/cut.out" 2>"$OUT/cut.log"
PID_LINE=$(grep '^participant=' "$OUT/cut.out" | head -1)
ID="${PID_LINE#participant=}"
START=$(date +%s%N)
$BIN/guest.exe --ticket-file "$OUT/ticket" --resume "$ID" --pings 20 --bytes 1024 >"$OUT/resume.json" 2>"$OUT/resume.log"
END=$(date +%s%N)
echo "resume wall-clock (process start to exit): $(( (END - START) / 1000000 )) ms" >"$OUT/resume.time"

sleep 1
kill $HOST_PID 2>/dev/null
echo "== $NET done"
for f in guest1 guest2 resume; do echo "-- $f"; cut -c1-1200 "$OUT/$f.json"; done
cat "$OUT/resume.time"
