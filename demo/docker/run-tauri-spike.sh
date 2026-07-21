#!/bin/bash
# Drives the real BenchVault binary (mounted at /app/benchvault) through
# tauri-driver and grabs a screenshot.
set -u
export DISPLAY=:99

Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
sleep 1

test -x /app/benchvault || { echo "FAIL: /app/benchvault missing or not executable"; exit 2; }
ldd /app/benchvault 2>&1 | grep -i "not found" && { echo "FAIL: unresolved libs above"; exit 2; }

tauri-driver --port 4444 &
sleep 2

mkdir -p /out
python3 /spike/tauri_spike.py
rc=$?
echo "=== exit $rc ==="
exit $rc
