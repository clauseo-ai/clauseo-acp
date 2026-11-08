#!/usr/bin/env bash
# Launches the app under xvfb for 15 s (ci.yml's Linux build job). It must still be running when
# the timeout stops it (exit 124), and its output must hold no panic.
#
#   .github/scripts/launch-xvfb.sh target/debug/clauseo-desktop
#
# The app's own exit status is written to a file inside xvfb-run and read from there: xvfb-run
# exits 5 when it cannot remove its temporary directory afterwards, and that status replaces the
# command's, so #479's merge group (2026-11-05) failed with "exited with 5" although the app had
# run its 15 s. A clean-up failure is printed as a warning; only the app's status decides.

set -uo pipefail

app=$1
log=$(mktemp)
code=$(mktemp)
echo none > "$code"
xvfb_status=0
# $0 and $1 inside the inner shell are the app and the status file.
# shellcheck disable=SC2016 # expanded by the inner sh, not here
xvfb-run -a sh -c 'timeout 15 "$0"; echo $? > "$1"' "$app" "$code" > "$log" 2>&1 || xvfb_status=$?
cat "$log"
status=$(cat "$code")
rm -f "$code"
if [ "$status" = none ]; then
  echo "::error::xvfb-run exited with $xvfb_status before it ran $app"
  exit 1
fi
if [ "$xvfb_status" -ne 0 ] && [ "$xvfb_status" -ne "$status" ]; then
  echo "::warning::xvfb-run exited with $xvfb_status after the app (5: it could not remove its temporary directory); the app's own status was $status"
fi
if [ "$status" -ne 124 ]; then
  echo "::error::$app exited with $status before the 15 s timeout; expected 124"
  exit 1
fi
if grep -n 'panicked' "$log"; then
  echo "::error::$app panicked"
  exit 1
fi
