#!/usr/bin/env bash
# Tests bench-ok.sh, which tells reducer-bench whether its pull request carries `bench-ok` now: it
# reads the labels through `gh api` (a fake `gh` first on PATH answers), matches the name exactly,
# and falls back to the event's labels only when the call fails. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/bench-ok-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin"
# Answers as `gh api ... --jq '.[].name'` does: one name a line. FAKE_GH_EXIT non-zero: the error
# JSON on stdout, as gh prints it.
cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$CALLS"
if [ "${FAKE_GH_EXIT:-0}" != 0 ]; then
  echo '{"message":"API rate limit exceeded","status":"403"}'
  exit "$FAKE_GH_EXIT"
fi
printf '%b' "${FAKE_GH_LABELS:-}"
EOF
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH" CALLS="$tmp/calls" PR=276 GH_REPO=clauseo-ai/clauseo-acp GH_TOKEN=fake

failures=0
# $1 name, $2 the output expected, $3 a warning expected (yes/no); env from the caller.
case_() {
  rm -f "$CALLS"
  local out status=0
  out=$(bash "$root/.github/scripts/bench-ok.sh" 2> "$tmp/err") || status=$?
  if [ "$status" != 0 ]; then echo "FAIL: $1: exit $status"; failures=$((failures + 1)); return; fi
  if [ "$out" != "$2" ]; then echo "FAIL: $1: printed [$out], expected [$2]"; failures=$((failures + 1)); return; fi
  if ! grep -qx -- "api --paginate repos/clauseo-ai/clauseo-acp/issues/276/labels --jq .\[\]\.name" "$CALLS"; then
    echo "FAIL: $1: gh called as [$(cat "$CALLS")]"; failures=$((failures + 1)); return
  fi
  if [ "$3" = yes ] && ! grep -q '^::warning title=bench-ok::' "$tmp/err"; then echo "FAIL: $1: no warning"; failures=$((failures + 1)); return; fi
  if [ "$3" = no ] && [ -s "$tmp/err" ]; then echo "FAIL: $1: stderr [$(cat "$tmp/err")]"; failures=$((failures + 1)); return; fi
  echo "ok: $1"
}

FAKE_GH_LABELS='bench-ok\n' BENCH_OK_EVENT=false case_ "the label now, not in the event (a re-run after adding it)" true no
FAKE_GH_LABELS='scoreboard\nbench-ok\n' BENCH_OK_EVENT=false case_ "the label among others" true no
FAKE_GH_LABELS='scoreboard\n' BENCH_OK_EVENT=true case_ "the label removed since the event" false no
FAKE_GH_LABELS='' BENCH_OK_EVENT=false case_ "no labels" false no
FAKE_GH_LABELS='bench-ok-later\nnot-bench-ok\n' BENCH_OK_EVENT=false case_ "names that only contain it" false no
FAKE_GH_EXIT=1 BENCH_OK_EVENT=true case_ "a failed call: the event's label decides (on)" true yes
FAKE_GH_EXIT=1 BENCH_OK_EVENT=false case_ "a failed call: the event's label decides (off)" false yes
FAKE_GH_EXIT=1 case_ "a failed call and no event value" false yes

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "bench-ok-test: all cases pass"
