#!/usr/bin/env bash
# Measures how long the host takes to notice a guest process that died without a close frame.
# Usage: ./run-cut.sh direct|relay-local
set -u
cd "$(dirname "$0")"
NET="$1"
OUT="out/cut-$NET"
mkdir -p "$OUT"
rm -f "$OUT"/*
BIN=target/native
$BIN/host.exe --net "$NET" --ticket-file "$OUT/ticket" >"$OUT/host.stdout" 2>"$OUT/host.log" &
HOST_PID=$!
for _ in $(seq 1 100); do [ -s "$OUT/ticket" ] && break; sleep 0.3; done
$BIN/guest.exe --ticket-file "$OUT/ticket" --cut >"$OUT/cut.out" 2>"$OUT/cut.log"
START=$(date +%s)
for _ in $(seq 1 90); do
  if grep -q "stream ended" "$OUT/host.log" && grep -q "connection closed" "$OUT/host.log"; then break; fi
  sleep 1
done
END=$(date +%s)
echo "host noticed the cut after about $((END - START)) s (poll granularity 1 s)" | tee "$OUT/result.txt"
grep -E "stream ended|connection closed" "$OUT/host.log" | cut -c1-300 | tee -a "$OUT/result.txt"
kill $HOST_PID 2>/dev/null
