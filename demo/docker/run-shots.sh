#!/bin/bash
# Seeds a demo library inside the container, then drives the real binary to
# capture screenshots. The container's app-data dir is throwaway, so the host's
# real library is never in reach.
set -eu
export DISPLAY=:99

Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
sleep 1

test -x /app/benchvault || { echo "FAIL: /app/benchvault missing"; exit 2; }
test -x /app/seed_demo  || { echo "FAIL: /app/seed_demo missing"; exit 2; }

/app/seed_demo /root/.local/share/dev.manzari.benchvault --force

tauri-driver --port 4444 &
sleep 2

mkdir -p /out
python3 /spike/shots.py
