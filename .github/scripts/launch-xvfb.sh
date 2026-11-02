#!/usr/bin/env bash
# Launches the app under xvfb for 15 s (ci.yml's Linux build job). It must still be running when
# the timeout stops it (exit 124), and its output must hold no panic.
#
#   .github/scripts/launch-xvfb.sh target/debug/clauseo-desktop

set -uo pipefail

app=$1
log=$(mktemp)
status=0
xvfb-run -a timeout 15 "$app" > "$log" 2>&1 || status=$?
cat "$log"
if [ "$status" -ne 124 ]; then
  echo "::error::$app exited with $status before the 15 s timeout; expected 124"
  exit 1
fi
if grep -n 'panicked' "$log"; then
  echo "::error::$app panicked"
  exit 1
fi
