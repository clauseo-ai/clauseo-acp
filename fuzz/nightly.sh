#!/usr/bin/env bash
# The nightly fuzz run. .github/workflows/fuzz.yml calls it, and it runs the same way locally:
#
#   cd fuzz && cargo fuzz build --fuzz-dir . -O --debug-assertions && ./nightly.sh 480
#
# - Fuzzes each target named after SECONDS (all of TARGETS when none is) for SECONDS, as many at
#   once as there are cores, or FUZZ_JOBS when it is set.
#   Each target reads seeds/<target> (committed, never written) and corpus/<target> (grown in
#   place; in CI the Actions cache keeps it from night to night).
# - Then cuts each corpus down to the inputs that add coverage (libFuzzer -merge=1).
# - A finding is a crash, a timeout or an out-of-memory stop. It fails the run, but only after
#   every target has had its time. A panic's input is minimised for up to 5 minutes.
# - Sanitizer reports (memory errors, leaks, UB) and crashes with no panic are withheld:
#   their log and input are never copied out and never printed. The issue only says that the
#   target crashed. Everything else about a finding is public: this repo is public.
#
# Writes out/: summary.md (a table for the job summary), upload/ (each reported finding's input,
# minimised input and log), title.txt and issue.md (the issue text, naming only the target and
# the panic), and found ("true" or "false"). Exits 0 when it ran, findings or not, so the
# workflow can save the corpus before it fails the job; it exits 2 when it cannot run.
set -euo pipefail

cd "$(dirname "$0")"

ALL=(acp_stdout protocol_decode matters_objects matters_read store_read settings_parse
  reducer_snapshot)
BIN_DIR=target/x86_64-unknown-linux-gnu/release
OUT=out
MINIMISE_SECONDS=300

seconds=${1:-}
if [[ ! $seconds =~ ^[0-9]+$ ]] || ((seconds < 1)); then
  echo "usage: nightly.sh SECONDS [TARGET...] (SECONDS: whole seconds per target)" >&2
  exit 2
fi
shift
if (($# > 0)); then
  TARGETS=("$@")
else
  TARGETS=("${ALL[@]}")
fi

# libFuzzer flags per target: -max_len, -timeout (s per input) and the dictionary, if any.
flags() {
  case $1 in
    acp_stdout) echo "-max_len=65536 -timeout=30 -dict=dict/json.dict" ;;
    protocol_decode) echo "-max_len=16384 -timeout=30 -dict=dict/json.dict" ;;
    matters_objects) echo "-max_len=65536 -timeout=60" ;;
    matters_read) echo "-max_len=8192 -timeout=30" ;;
    store_read) echo "-max_len=65536 -timeout=30" ;;
    settings_parse) echo "-max_len=8192 -timeout=30 -dict=dict/json.dict" ;;
    reducer_snapshot) echo "-max_len=65536 -timeout=30" ;;
    *) return 1 ;;
  esac
}

for target in "${TARGETS[@]}"; do
  if ! flags "$target" >/dev/null; then
    echo "nightly.sh: $target is not a target: ${ALL[*]}" >&2
    exit 2
  fi
  if [[ ! -x $BIN_DIR/$target ]]; then
    echo "nightly.sh: $BIN_DIR/$target is not built" >&2
    exit 2
  fi
done

rm -rf "$OUT"
mkdir -p "$OUT/logs" "$OUT/status" "$OUT/upload" "$OUT/merged" artifacts
# Harnesses that need a folder make it under TMPDIR; it is removed at the end.
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export TMPDIR=$scratch
export RUST_BACKTRACE=1

# A one-line, markdown-safe excerpt: no backticks, no control characters, at most 300 characters.
excerpt() {
  tr -d '`\r' | tr -c '[:print:]\n' '?' | head -c 300
}

# Prints what a run that exited non-zero found, from its log and whether it left an input:
# panic, timeout, oom, withheld (a sanitizer report, or a crash with no panic) or error (the
# fuzzer stopped without finding anything, such as a bad flag or a missing file).
classify() {
  local log=$1 input=$2
  if grep -qE 'ERROR: (Address|Leak|Memory|Thread|UndefinedBehavior)Sanitizer|runtime error:' "$log"; then
    echo withheld
  elif grep -q 'panicked at ' "$log"; then
    echo panic
  elif grep -q 'ERROR: libFuzzer: timeout' "$log"; then
    echo timeout
  elif grep -qE 'ERROR: libFuzzer: (out-of-memory|malloc)' "$log"; then
    echo oom
  elif [[ -n $input ]] || grep -q 'deadly signal' "$log"; then
    echo withheld
  else
    echo error
  fi
}

run_target() {
  local target=$1 log="$OUT/logs/$1.log" status="$OUT/status/$1" code=0
  local -a extra
  read -r -a extra <<<"$(flags "$target")"
  mkdir -p "corpus/$target" "artifacts/$target"
  rm -f "artifacts/$target"/*
  "$BIN_DIR/$target" "corpus/$target" "seeds/$target" "${extra[@]}" -rss_limit_mb=2048 \
    -max_total_time="$seconds" -print_final_stats=1 -artifact_prefix="artifacts/$target/" \
    >"$log" 2>&1 || code=$?

  local execs rate
  execs=$(sed -n 's/^stat::number_of_executed_units: *//p' "$log" | tail -n 1)
  rate=$(sed -n 's/^stat::average_exec_per_sec: *//p' "$log" | tail -n 1)

  # Keep only the inputs that add coverage. If the merge fails, the corpus stays as it was.
  mkdir -p "$OUT/merged/$target"
  if "$BIN_DIR/$target" -merge=1 "${extra[@]}" -rss_limit_mb=2048 "$OUT/merged/$target" \
    "corpus/$target" >"$OUT/logs/$target.merge.log" 2>&1; then
    rm -rf "corpus/$target"
    mv "$OUT/merged/$target" "corpus/$target"
  fi
  local files
  files=$(find "corpus/$target" -type f | wc -l)

  local kind=ok input=""
  if ((code != 0)); then
    input=$(find "artifacts/$target" -type f | head -n 1)
    kind=$(classify "$log" "$input")
  fi
  printf '%s\t%s\t%s\t%s\n' "$kind" "${execs:-?}" "${rate:-?}" "$files" >"$status"

  case $kind in
    ok | withheld) return 0 ;;
  esac
  local dest="$OUT/upload/$target"
  mkdir -p "$dest"
  cp "$log" "$dest/"
  if [[ -n $input ]]; then
    cp "$input" "$dest/"
    if [[ $kind == panic ]]; then
      # As cargo fuzz tmin does: each step writes a shorter crashing input as minimized-from-*.
      # -max_total_time bounds each step and timeout the whole. The shortest input is kept,
      # if it still crashes. Only -timeout is passed on: with -max_len, libFuzzer writes an
      # empty input that does not crash.
      local steps="$OUT/minimise/$target" min flag
      local -a keep=()
      for flag in "${extra[@]}"; do
        if [[ $flag == -timeout=* ]]; then keep+=("$flag"); fi
      done
      mkdir -p "$steps"
      timeout "$MINIMISE_SECONDS" "$BIN_DIR/$target" -minimize_crash=1 -runs=100000 \
        -max_total_time=60 "${keep[@]}" -artifact_prefix="$steps/" "$input" \
        >"$dest/minimise.log" 2>&1 || true
      min=$(find "$steps" -type f -name 'minimized-from-*' -printf '%s %p\n' | sort -n |
        head -n 1 | cut -d ' ' -f 2-)
      if [[ -n $min ]] && ! "$BIN_DIR/$target" "${keep[@]}" "$min" >>"$dest/minimise.log" 2>&1; then
        cp "$min" "$dest/minimised-$(basename "$input")"
      fi
    fi
  fi
  if [[ $kind == panic ]]; then
    # "thread '...' panicked at <file>:<line>:<column>:" and then the message on the next line.
    grep -m 1 -A 1 'panicked at ' "$log" | sed 's/^.*panicked at //' | tr '\n' ' ' |
      excerpt >"$status.panic"
  fi
}

jobs=${FUZZ_JOBS:-$(nproc)}
running=0
for target in "${TARGETS[@]}"; do
  if ((running >= jobs)); then
    wait -n || true
    running=$((running - 1))
  fi
  run_target "$target" &
  running=$((running + 1))
done
wait

found=false
reported=()
{
  echo "| target | result | execs | exec/s | corpus files |"
  echo "|---|---|---|---|---|"
} >"$OUT/summary.md"
: >"$OUT/issue.md"
for target in "${TARGETS[@]}"; do
  if [[ ! -s $OUT/status/$target ]]; then
    echo "nightly.sh: $target left no status" >&2
    exit 2
  fi
  IFS=$'\t' read -r kind execs rate files <"$OUT/status/$target"
  echo "| $target | $kind | $execs | $rate | $files |" >>"$OUT/summary.md"
  [[ $kind == ok ]] && continue
  found=true
  reported+=("$target")
  case $kind in
    panic) echo "- \`$target\` panicked at \`$(cat "$OUT/status/$target.panic")\`" ;;
    timeout) echo "- \`$target\`: an input ran past its timeout." ;;
    oom) echo "- \`$target\`: an input went over the 2048 MB memory limit." ;;
    withheld) echo "- \`$target\` crashed. Its report and input are not posted." ;;
    error) echo "- \`$target\`: the fuzzer stopped with an error before its time was up." ;;
  esac >>"$OUT/issue.md"
done

if [[ $found == true ]]; then
  if ((${#reported[@]} == 1)); then
    echo "nightly fuzz: ${reported[0]} found a crash" >"$OUT/title.txt"
  else
    echo "nightly fuzz: ${reported[*]} found crashes" >"$OUT/title.txt"
  fi
fi
echo "$found" >"$OUT/found"
cat "$OUT/summary.md"
[[ -s $OUT/issue.md ]] && cat "$OUT/issue.md"
exit 0
