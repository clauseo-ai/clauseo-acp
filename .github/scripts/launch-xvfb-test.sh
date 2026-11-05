#!/usr/bin/env bash
# Tests launch-xvfb.sh with a fake xvfb-run and a fake timeout first on PATH: the app's own status
# decides, whatever xvfb-run's clean-up says afterwards. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/launch-xvfb-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin"
# xvfb-run: drops -a, runs the command unless FAKE_XVFB_EARLY is set, then exits FAKE_XVFB_EXIT.
cat > "$tmp/bin/xvfb-run" <<'EOF'
#!/usr/bin/env bash
[ "$1" = -a ] && shift
if [ -n "${FAKE_XVFB_EARLY:-}" ]; then echo "xvfb-run: error: Xvfb failed to start" >&2; exit "$FAKE_XVFB_EARLY"; fi
"$@"
exit "${FAKE_XVFB_EXIT:-0}"
EOF
# timeout: runs the app for its output, then exits FAKE_APP_STATUS (124: the timeout stopped it).
cat > "$tmp/bin/timeout" <<'EOF'
#!/usr/bin/env bash
shift
"$@"
exit "${FAKE_APP_STATUS:-124}"
EOF
# shellcheck disable=SC2016 # the fake app expands it when it runs
printf '#!/usr/bin/env bash\necho "${FAKE_APP_OUTPUT:-started}"\n' > "$tmp/app"
chmod +x "$tmp/bin/xvfb-run" "$tmp/bin/timeout" "$tmp/app"
export PATH="$tmp/bin:$PATH"

failures=0
# $1 name, $2 exit expected, $3 text the output must hold; env from the caller.
case_() {
  local status=0
  bash "$root/.github/scripts/launch-xvfb.sh" "$tmp/app" > "$tmp/out" 2>&1 || status=$?
  if [ "$status" != "$2" ]; then echo "FAIL: $1: exit $status, expected $2 ($(tr '\n' ' ' < "$tmp/out"))"; failures=$((failures + 1)); return; fi
  if ! grep -qF -- "$3" "$tmp/out"; then echo "FAIL: $1: output lacks [$3] ($(tr '\n' ' ' < "$tmp/out"))"; failures=$((failures + 1)); return; fi
  echo "ok: $1"
}
case_ "the timeout stops a running app: pass" 0 "started"
FAKE_XVFB_EXIT=5 case_ "xvfb-run's clean-up fails after a full run: pass, with a warning" 0 "::warning::xvfb-run exited with 5"
FAKE_APP_STATUS=1 case_ "the app exits early: fail" 1 "exited with 1 before the 15 s timeout"
FAKE_APP_STATUS=1 FAKE_XVFB_EXIT=5 case_ "an early exit and a clean-up failure: fail on the app's status" 1 "exited with 1 before the 15 s timeout"
FAKE_XVFB_EARLY=3 case_ "xvfb-run fails before the app runs: fail" 1 "xvfb-run exited with 3 before it ran"
FAKE_APP_OUTPUT="thread 'main' panicked at src/main.rs:1:1" case_ "a panic in the output: fail" 1 "panicked"

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "launch-xvfb-test: all cases pass"
