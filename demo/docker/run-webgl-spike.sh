#!/bin/bash
# Verifies WebGL works in this image: context -> rasterise -> composite ->
# WebDriver screenshot. The STL/3MF preview is @react-three/fiber, so if this
# fails the whole screenshot pipeline is dead in the water.
set -u
export DISPLAY=:99

Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
sleep 1

echo "=== GL under Xvfb ==="
glxinfo -B 2>&1 | grep -i "renderer string\|OpenGL version" | head -3

WebKitWebDriver --port=4444 --host=127.0.0.1 &
sleep 2

mkdir -p /out
python3 /spike/spike.py
rc=$?
echo "=== exit $rc ==="
exit $rc
