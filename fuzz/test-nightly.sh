#!/usr/bin/env bash
# Checks what nightly.sh reports and what it withholds, with stand-in fuzzers: no build and no
# fuzzing, a few seconds. fuzz.yml runs it before the night's fuzzing; locally: ./test-nightly.sh
#
# Each stand-in is a copy of FAKE below, named after a target, acting out the case its file in
# $FAKE_CASES names, in each of the ways nightly.sh runs a libFuzzer binary: fuzzing, merging,
# minimising and re-running one input. A withheld detail carries the text WITHHELD-DETAIL, which
# must never reach nightly.sh's output, out/upload/, summary.md, issue.md or title.txt.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
TARGETS=(acp_stdout protocol_decode matters_objects matters_read store_read settings_parse
  reducer_snapshot)
failures=0

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

read -r -d '' FAKE <<'EOF' || true
#!/usr/bin/env bash
# A stand-in for a libFuzzer binary. Its case is in $FAKE_CASES/<its name>.
name=$(basename "$0")
case=$(cat "$FAKE_CASES/$name")
mode=fuzz prefix="" pos=()
for arg in "$@"; do
  case $arg in
    -merge=1) mode=merge ;;
    -minimize_crash=1) mode=minimise ;;
    -artifact_prefix=*) prefix=${arg#-artifact_prefix=} ;;
    -*) ;;
    *) pos+=("$arg") ;;
  esac
done
asan() { echo "==1==ERROR: AddressSanitizer: heap-buffer-overflow WITHHELD-DETAIL $name"; }
panic() {
  echo "thread '<unnamed>' (7) panicked at fuzz_targets/$name.rs:12:5:"
  echo "$1"
  echo "==1== ERROR: libFuzzer: deadly signal"
}
case $mode in
  merge) cp -r "${pos[1]}"/. "${pos[0]}"/ && exit 0 ;;
  # libFuzzer keeps any crash a step finds; for drift, that is the sanitizer report below.
  minimise) printf 'mini' >"${prefix}minimized-from-1" && exit 0 ;;
esac
if [[ -f ${pos[0]} ]]; then
  if [[ $case == drift ]]; then asan; else panic "the same panic"; fi
  exit 1
fi
echo new >"${pos[0]}/new-input"
echo "stat::number_of_executed_units: 42"
echo "stat::average_exec_per_sec: 7"
case $case in
  ok) exit 0 ;;
  panic | drift)
    printf 'crash input' >"${prefix}crash-1"
    panic 'assertion `left == right` failed @someone'
    ;;
  ub)
    printf 'x' >"${prefix}crash-1"
    echo "thread '<unnamed>' (7) panicked at library/core/src/panicking.rs:220:5:"
    echo "unsafe precondition(s) violated: slice::from_raw_parts WITHHELD-DETAIL"
    echo "thread caused non-unwinding panic. aborting."
    echo "==1== ERROR: libFuzzer: deadly signal"
    ;;
  asan) printf 'x' >"${prefix}crash-1" && asan ;;
  oom)
    printf 'x' >"${prefix}oom-1"
    echo "==1== ERROR: libFuzzer: out-of-memory (malloc(4294967296))"
    ;;
  oom-asan)
    printf 'x' >"${prefix}oom-1"
    echo "==1== ERROR: libFuzzer: out-of-memory (malloc(4294967296))"
    asan
    ;;
  timeout)
    printf 'x' >"${prefix}timeout-1"
    echo "==1== ERROR: libFuzzer: timeout after 30 seconds"
    ;;
  error) echo "ERROR: cannot read the dictionary" ;;
  long) printf 'x' >"${prefix}crash-1" && panic "$(head -c 70000 /dev/zero | tr '\0' a)" ;;
esac
exit 1
EOF

mkdir -p "$work/fuzz/dict" "$work/fuzz/target/x86_64-unknown-linux-gnu/release" "$work/cases"
cp "$here/nightly.sh" "$work/fuzz/"
: >"$work/fuzz/dict/json.dict"
for target in "${TARGETS[@]}"; do
  mkdir -p "$work/fuzz/seeds/$target"
  printf '%s\n' "$FAKE" >"$work/fuzz/target/x86_64-unknown-linux-gnu/release/$target"
  chmod +x "$work/fuzz/target/x86_64-unknown-linux-gnu/release/$target"
done
export FAKE_CASES="$work/cases" FUZZ_JOBS=4
out="$work/fuzz/out"

# run "<target>=<case> ..." [targets to pass to nightly.sh...]: every target not named is ok.
run() {
  local pair
  for target in "${TARGETS[@]}"; do echo ok >"$FAKE_CASES/$target"; done
  for pair in $1; do echo "${pair#*=}" >"$FAKE_CASES/${pair%%=*}"; done
  shift
  rm -rf "$work/fuzz/corpus"
  "$work/fuzz/nightly.sh" 1 "$@" >"$work/stdout" 2>&1 || fail "nightly.sh exited $?"
  if grep -rq WITHHELD-DETAIL "$work/stdout" "$out/upload" "$out/summary.md" "$out/issue.md" \
    "$out/title.txt" 2>/dev/null; then
    fail "a withheld detail reached a public output"
  fi
}

# expect <what> <actual> <expected>
expect() {
  if [[ $2 != "$3" ]]; then fail "$1: got '$2', expected '$3'"; fi
}

kind() { cut -f 1 "$out/status/$1"; }
files() { (cd "$out/upload/$1" 2>/dev/null && find . -type f | sed 's|^\./||' | sort | tr '\n' ' '); }

run "protocol_decode=panic matters_objects=drift matters_read=ub store_read=asan \
settings_parse=oom reducer_snapshot=timeout"
expect "found" "$(cat "$out/found")" true
expect "withheld" "$(cat "$out/withheld")" true
for pair in acp_stdout=ok protocol_decode=panic matters_objects=panic matters_read=withheld \
  store_read=withheld settings_parse=oom reducer_snapshot=timeout; do
  expect "${pair%%=*} kind" "$(kind "${pair%%=*}")" "${pair#*=}"
done
expect "panic upload" "$(files protocol_decode)" \
  "crash-1 minimised-crash-1 minimised-crash-1.log protocol_decode.log "
expect "drift upload (no minimised input)" "$(files matters_objects)" "crash-1 matters_objects.log "
expect "ub upload" "$(files matters_read)" ""
expect "asan upload" "$(files store_read)" ""
expect "oom upload" "$(files settings_parse)" "oom-1 settings_parse.log "
expect "timeout upload" "$(files reducer_snapshot)" "reducer_snapshot.log timeout-1 "
expect "corpus merged" "$(ls "$work/fuzz/corpus/acp_stdout")" new-input
expect "title" "$(cat "$out/title.txt")" \
  "nightly fuzz: findings in protocol_decode, matters_objects, matters_read, store_read, settings_parse, reducer_snapshot"
expect "issue" "$(cat "$out/issue.md")" "$(
  cat <<'EOF'
- `protocol_decode` panicked at `fuzz_targets/protocol_decode.rs:12:5: assertion left == right failed @someone `
- `matters_objects` panicked at `fuzz_targets/matters_objects.rs:12:5: assertion left == right failed @someone `
- `matters_read` crashed. Its report and input are not posted.
- `store_read` crashed. Its report and input are not posted.
- `settings_parse`: an input went over the memory limit (2048 MB).
- `reducer_snapshot`: an input ran past its timeout.
EOF
)"

run "acp_stdout=oom-asan protocol_decode=error matters_objects=long"
expect "oom with a sanitizer report" "$(kind acp_stdout)" withheld
expect "oom-asan upload" "$(files acp_stdout)" ""
expect "error kind" "$(kind protocol_decode)" error
expect "error upload" "$(files protocol_decode)" "protocol_decode.log "
expect "long panic kind" "$(kind matters_objects)" panic
expect "long panic excerpt length" "$(wc -c <"$out/status/matters_objects.panic")" 300
expect "title" "$(cat "$out/title.txt")" \
  "nightly fuzz: findings in acp_stdout, protocol_decode, matters_objects"

run "protocol_decode=panic" protocol_decode
expect "one target" "$(cut -d '|' -f 2 "$out/summary.md" | tail -n +3 | tr -d ' ')" protocol_decode
expect "single title" "$(cat "$out/title.txt")" "nightly fuzz: protocol_decode panicked"

run ""
expect "found" "$(cat "$out/found")" false
expect "withheld" "$(cat "$out/withheld")" false
expect "no title" "$(cat "$out/title.txt" 2>/dev/null)" ""
expect "no issue" "$(cat "$out/issue.md")" ""

if ((failures > 0)); then
  echo "test-nightly.sh: $failures failed" >&2
  exit 1
fi
echo "test-nightly.sh: all cases pass"
