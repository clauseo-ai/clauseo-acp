#!/usr/bin/env bash
# Tests gate-prior.sh, and the mode steps that call it: windows-pr-tests.yml's `decide` and
# e2e-windows.yml's `e2e-changes`, each step's script taken from the workflow (ruby's YAML) and run
# against a fake `gh` that answers from fixtures. The point: the gate's own answers (wait, failed,
# covered, run on ubuntu) are not results, so a label or an approval after a wait stop runs the
# tests, and `failed` stays for a real failure. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/gate-prior-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin"
# `gh api [-X M] <endpoint> [-f k=v]... [--paginate] [-q|--jq filter]`: the fixture file named after
# the endpoint ('/' as '_', query cut off), through jq -r when a filter is given, as gh's -q is.
# A missing fixture fails as gh does on a 404. A POST succeeds with nothing. Every call is logged.
cat > "$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$CALLS"
shift # api
method=GET endpoint="" filter=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -X) method=$2; shift 2 ;;
    -f) shift 2 ;;
    -q | --jq) filter=$2; shift 2 ;;
    --paginate) shift ;;
    *) endpoint=$1; shift ;;
  esac
done
[ "$method" = POST ] && exit 0
file="$FIXTURES/$(printf '%s' "${endpoint%%\?*}" | tr '/' '_')"
if [ ! -f "$file" ]; then
  echo '{"message":"Not Found","status":"404"}'
  exit 1
fi
if [ -n "$filter" ]; then jq -r "$filter" "$file"; else cat "$file"; fi
EOF
chmod +x "$tmp/bin/gh"
export PATH="$tmp/bin:$PATH" CALLS="$tmp/calls" GH_TOKEN=fake
export REPO=clauseo-ai/clauseo-acp HEAD_SHA=4daea874 RUN_ID=900 PR=732 HEAD_REF=stage-c

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

# A fixture set: fixtures <dir> <check name> then "<job id> <conclusion> <completed_at> <runner>"...
# for the check runs (a job id is its run id and one digit: job 9001 is in run 900, this run)
# and the jobs behind them.
fixtures() {
  local dir=$1 check=$2
  shift 2
  rm -rf "$dir"
  mkdir -p "$dir"
  local runs="" id conclusion at runner run
  for row in "$@"; do
    read -r id conclusion at runner <<< "$row"
    run=$((id / 10))
    runs="$runs{\"status\":\"completed\",\"conclusion\":\"$conclusion\",\"completed_at\":\"$at\",\"name\":\"$check\",\"details_url\":\"https://github.com/$REPO/actions/runs/$run/job/$id\"},"
    if [ "$runner" != unreadable ]; then
      echo "{\"id\":$id,\"labels\":[\"$runner\"]}" > "$dir/repos_${REPO//\//_}_actions_jobs_$id"
    fi
  done
  echo "{\"check_runs\":[${runs%,}]}" > "$dir/repos_${REPO//\//_}_commits_${HEAD_SHA}_check-runs"
  # The rest of what the mode steps read: the head is current, a Rust and app change, no review.
  echo "{\"head\":{\"sha\":\"$HEAD_SHA\"}}" > "$dir/repos_${REPO//\//_}_pulls_$PR"
  echo '{"workflow_runs":[]}' > "$dir/repos_${REPO//\//_}_actions_workflows_windows-pr-tests.yml_runs"
  echo '[{"filename":"crates/desktop/src/lib.rs"}]' > "$dir/repos_${REPO//\//_}_pulls_${PR}_files"
  echo '[]' > "$dir/repos_${REPO//\//_}_pulls_${PR}_reviews"
}

# --- gate-prior.sh alone ---------------------------------------------------------------------
# prior <name> <want output, lines joined by |> <check> <runner> <failing> [RELEVANT]
prior() {
  local name=$1 want=$2 got
  got=$(RELEVANT=${6:-false} FIXTURES=$tmp/f bash .github/scripts/gate-prior.sh "$3" "$4" "$5" 2> "$tmp/err" | paste -sd '|' -)
  if [ "$got" = "$want" ]; then echo "ok: gate-prior: $name"; else fail "gate-prior: $name: got [$got], want [$want]; stderr: $(tr '\n' ' ' < "$tmp/err")"; fi
}
W="windows tests (PR)"
U="https://github.com/$REPO/actions/runs"

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:21:11Z ubuntu-24.04"
prior "a wait stop alone is no result" "verdict=none" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:10:00Z windows-2025"
prior "a real failure, an event that is not relevant" "prior=$U/10/job/101|verdict=failed" "$W" windows-2025 "failure timed_out"
prior "a real failure, a relevant event" "prior=$U/10/job/101|verdict=none" "$W" windows-2025 "failure timed_out" true

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:10:00Z windows-2025" "111 failure 2026-11-08T09:20:00Z ubuntu-24.04" "121 failure 2026-11-08T09:30:00Z ubuntu-24.04"
prior "failed and wait answers after a real failure: the failure decides" "prior=$U/10/job/101|verdict=failed" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "101 success 2026-11-08T09:10:00Z windows-2025" "111 success 2026-11-08T09:20:00Z ubuntu-24.04"
prior "a real pass behind a covered answer" "prior=$U/10/job/101|verdict=covered" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:10:00Z windows-2025" "111 success 2026-11-08T09:20:00Z windows-2025"
prior "the newest real result decides" "prior=$U/11/job/111|verdict=covered" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "9001 failure 2026-11-08T09:30:00Z windows-2025"
prior "this run's own result never counts (a re-run tests again)" "verdict=none" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:10:00Z windows-2025" "111 failure 2026-11-08T09:20:00Z unreadable"
prior "a job the API cannot read is skipped" "prior=$U/10/job/101|verdict=failed" "$W" windows-2025 "failure timed_out"

fixtures "$tmp/f" "$W" "101 cancelled 2026-11-08T09:10:00Z windows-2025"
prior "cancelled is not a failure unless listed" "prior=$U/10/job/101|verdict=none" "$W" windows-2025 "failure timed_out"
prior "cancelled, listed (e2e-windows.yml)" "prior=$U/10/job/101|verdict=failed" "$W" windows-2025 "failure timed_out cancelled"

fixtures "$tmp/f" "$W" "101 failure 2026-11-08T09:10:00Z windows-2025"
prior "another check's runner is not this one's" "verdict=none" "$W" windows-11-arm "failure timed_out"

fixtures "$tmp/f" "$W"
rm "$tmp/f/repos_${REPO//\//_}_commits_${HEAD_SHA}_check-runs"
prior "a failed read of the check runs decides without them" "verdict=none" "$W" windows-2025 "failure timed_out"
if grep -q '^::warning::The check runs API failed' "$tmp/err"; then echo "ok: gate-prior: the failed read warns"; else fail "gate-prior: no warning for the failed read"; fi

# --- the mode steps, from the workflows --------------------------------------------------------
step() { # <workflow> <job> <step id>
  ruby -ryaml -e 'puts YAML.load_file(ARGV[0])["jobs"][ARGV[1]]["steps"].find { |s| s["id"] == ARGV[2] }["run"]' "$@"
}
step .github/workflows/windows-pr-tests.yml decide mode > "$tmp/decide.sh"
step .github/workflows/e2e-windows.yml e2e-changes mode > "$tmp/e2e.sh"

# mode <name> <want> <script> then VAR=value... for the event
mode() {
  local name=$1 want=$2 script=$3 got
  shift 3
  : > "$tmp/out"
  env GITHUB_OUTPUT="$tmp/out" RUNNER_TEMP="$tmp" FIXTURES="$tmp/f" "$@" bash "$script" > "$tmp/log" 2>&1 || true
  got=$(sed -n 's/^mode=//p' "$tmp/out")
  if [ "$got" = "$want" ]; then echo "ok: $(basename "$script" .sh): $name"; else fail "$(basename "$script" .sh): $name: mode [$got], want [$want]; log: $(tail -5 "$tmp/log" | tr '\n' ' ')"; fi
}

for wf in decide e2e; do
  if [ "$wf" = decide ]; then
    check="windows tests (PR)"
  else
    check="smoke (windows-2025)"
  fi
  s="$tmp/$wf.sh"
  # #732: opened before ready (a wait stop), then the label ready.
  fixtures "$tmp/f" "$check" "101 failure 2026-11-08T09:21:11Z ubuntu-24.04"
  mode "wait stop, then the label ready" run "$s" EVENT=pull_request RELEVANT=true READY=true APP=true
  # #732: the approval two seconds after the label, whose run is not relevant (the label is on).
  mode "wait stop, then an approval with ready on" run "$s" EVENT=pull_request_review RELEVANT=false READY=true REVIEW_STATE=approved APP=true
  mode "wait stop, then an approval without the label" run "$s" EVENT=pull_request_review RELEVANT=true READY=true REVIEW_STATE=approved APP=true
  mode "wait stop, then another label, still not ready" wait "$s" EVENT=pull_request RELEVANT=false READY=false APP=true
  # A real failure, then a label that cannot change it.
  fixtures "$tmp/f" "$check" "101 failure 2026-11-08T09:10:00Z windows-2025" "111 failure 2026-11-08T09:20:00Z ubuntu-24.04"
  mode "real failure, then another label" failed "$s" EVENT=pull_request RELEVANT=false READY=true APP=true
  mode "real failure, then the label ready" run "$s" EVENT=pull_request RELEVANT=true READY=true APP=true
  fixtures "$tmp/f" "$check" "101 success 2026-11-08T09:10:00Z windows-2025"
  mode "real pass" covered "$s" EVENT=pull_request RELEVANT=false READY=true APP=true
done

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "gate-prior-test: all cases pass"
