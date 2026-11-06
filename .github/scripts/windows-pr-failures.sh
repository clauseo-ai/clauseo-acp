#!/usr/bin/env bash
# Names the failed tests of a nextest log: windows-pr-tests.yml runs it after its test step fails,
# so a pull request's checks page shows each failed Windows test and where it panicked without
# opening the log. windows-pr-failures-test.sh pins it with a fake log; fast-lane runs that.
#
#   bash .github/scripts/windows-pr-failures.sh nextest.log
#
# For each failed test, once, in the order the log first shows it, it prints one annotation:
#
#   ::error title=Windows x64 (windows-2025) test failed::<test> — <its first 'panicked at' line>
#
# (`timed out` for a TMT, the outcome for a signal or an abort without a panic line), and appends
# the same list to $GITHUB_STEP_SUMMARY as a table: test, outcome, location. A failed test is one
# with a FAIL, TMT, SIG* or ABORT line, a try included (`TRY 1 FAIL`), that never passed: a test
# that passed on its retry (`TRY 2 PASS`, then `FLAKY 2/2` in the summary) is left out. The log is
# nextest's stdout and stderr together, colours or not. It always exits 0: the test step has
# already failed the job.

set -euo pipefail

log=${1:?usage: windows-pr-failures.sh NEXTEST_LOG}
summary=${GITHUB_STEP_SUMMARY:-/dev/null}
title="Windows x64 (windows-2025) test failed"

# One tab-separated line per failed test: name, outcome, first panic line (or empty).
failed=$(
  awk '
    {
      sub(/\r$/, "")
      gsub(/\033\[[0-9;]*[A-Za-z]/, "")
    }
    # A status line (PASS, FAIL, TRY 2 PASS, FLAKY 2/2, SLOW, TMT, ...) or the summary ends the
    # output block of the test before it.
    /^ *(TRY [0-9]+ )?[A-Z]+( [0-9]+\/[0-9]+)? \[[> ]*[0-9.]+s\] / || /^ *Summary \[/ {
      cur = ""
      line = $0
      sub(/^ */, "", line)
      try = 0
      if (line ~ /^TRY [0-9]+ /) { try = 1; sub(/^TRY [0-9]+ /, "", line) }
      outcome = line
      sub(/ .*/, "", outcome)
      name = line
      sub(/^[^]]*\] /, "", name)
      sub(/^\([^)]*\) /, "", name)
      if (outcome == "PASS" || outcome == "FLAKY") { passed[name] = 1; next }
      if (outcome == "FAIL" || outcome == "TMT" || outcome == "ABORT" || outcome ~ /^SIG[A-Z]+$/) {
        if (!(name in seen)) { seen[name] = 1; order[++n] = name }
        last[name] = outcome
        cur = name
      }
      next
    }
    cur != "" && !(cur in panic) && /panicked at/ {
      p = $0
      sub(/^[ \t]*/, "", p)
      sub(/[ \t]*$/, "", p)
      panic[cur] = p
    }
    END {
      for (i = 1; i <= n; i++) {
        t = order[i]
        if (t in passed) continue
        printf "%s\t%s\t%s\n", t, last[t], panic[t]
      }
    }
  ' "$log"
)

if [ -z "$failed" ]; then
  echo "::notice title=No failed test in the log::The log names no failed test; the step failed before or outside the tests (a build error?). See the test step's log."
  printf '%s\n\n%s\n' "### Windows x64 tests" "No failed test in the log: the step failed before or outside the tests (a build error?)." >> "$summary"
  exit 0
fi

{
  echo "### Windows x64 tests that failed"
  echo
  echo "| test | outcome | location |"
  echo "| --- | --- | --- |"
} >> "$summary"

while IFS=$'\t' read -r name outcome panic; do
  if [ "$outcome" = TMT ]; then
    what="timed out"
    shown="timed out"
  else
    what=${panic:-$outcome}
    shown=$outcome
  fi
  # Workflow commands read %, CR and LF as escapes in the message.
  msg="$name — $what"
  msg=${msg//%/%25}
  echo "::error title=$title::$msg"
  # The panic's file:line:col, from either form of the line (`panicked at src/x.rs:4:24:` or the
  # older `panicked at 'msg', src/x.rs:4:24`).
  location=$(printf '%s\n' "$panic" | grep -oE "[^ ':]+:[0-9]+:[0-9]+" | tail -n 1 || true)
  printf '| %s | %s | %s |\n' "${name//|/\\|}" "$shown" "${location:-—}" >> "$summary"
done <<< "$failed"
