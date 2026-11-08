#!/usr/bin/env bash
# Tests windows-pr-failures.sh against a fake nextest log (the ci profile's shape: one retry,
# immediate-final output, a summary that repeats the failures): a FAIL with a panic, a TMT and a
# FLAKY test, which must not be reported. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/windows-pr-failures-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# nextest 0.9.148's layout; one status line in colour, as a terminal-less run may still print it.
esc=$'\033'
cat > "$tmp/nextest.log" <<EOF
    Finished \`test\` profile [unoptimized + debuginfo] target(s) in 0.28s
────────────
 Nextest run ID db215389-3766-4bf9-8a26-99418415cb1e with nextest profile: ci
    Starting 5 tests across 1 binary
        PASS [   0.017s] (1/5) clauseo-store tests::ok
  TRY 1 FAIL [   0.017s] (───) clauseo-store tests::flaky
  stderr ───

    thread 'tests::flaky' (3460154) panicked at crates\\store\\src\\lib.rs:8:60:
    first try fails

  ${esc}[31mTRY 1 FAIL${esc}[0m [   0.017s] (───) clauseo-store tests::bad
  stdout ───

    running 1 test
    test tests::bad ... FAILED

  stderr ───

    thread 'tests::bad' (3460151) panicked at crates\\store\\src\\lib.rs:4:24:
    assertion \`left == right\` failed: one is not two
      left: 1
     right: 2

  TRY 2 FAIL [   0.012s] (2/5) clauseo-store tests::bad
  stderr ───

    thread 'tests::bad' (3460162) panicked at crates\\store\\src\\lib.rs:5:9:
    assertion \`left == right\` failed: one is not two

  TRY 2 PASS [   0.013s] (3/5) clauseo-store tests::flaky
        SLOW [>  1.000s] (───) clauseo-store tests::slow
TRY 1 TRMNTG [>  2.000s] (───) clauseo-store tests::slow
   TRY 1 TMT [   2.004s] (───) clauseo-store tests::slow
  stdout ───

    running 1 test

    (test timed out)

  TRY 2 TMT [   2.004s] (4/5) clauseo-store tests::slow
        PASS [   0.002s] (5/5) clauseo-store tests::after
────────────
     Summary [   4.007s] 5/5 tests run: 3 passed (1 flaky), 2 failed, 0 skipped
   FLAKY 2/2 [   0.013s] (3/5) clauseo-store tests::flaky
  TRY 2 FAIL [   0.012s] (2/5) clauseo-store tests::bad
  stderr ───

    thread 'tests::bad' (3460162) panicked at crates\\store\\src\\lib.rs:5:9:

  TRY 2 TMT [   2.004s] (4/5) clauseo-store tests::slow
error: test run failed
EOF
printf '\r\n' >> "$tmp/nextest.log"

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

GITHUB_STEP_SUMMARY=$tmp/summary bash "$root/.github/scripts/windows-pr-failures.sh" "$tmp/nextest.log" > "$tmp/out"

title="::error title=Windows x64 (windows-2025) test failed::"
cat > "$tmp/want-out" <<EOF
${title}clauseo-store tests::bad — thread 'tests::bad' (3460151) panicked at crates\\store\\src\\lib.rs:4:24:
${title}clauseo-store tests::slow — timed out
EOF
if diff -u "$tmp/want-out" "$tmp/out"; then echo "ok: annotations"; else fail "annotations"; fi

cat > "$tmp/want-summary" <<'EOF'
### Windows x64 tests that failed

| test | outcome | location |
| --- | --- | --- |
| clauseo-store tests::bad | FAIL | crates\store\src\lib.rs:4:24 |
| clauseo-store tests::slow | timed out | — |
EOF
if diff -u "$tmp/want-summary" "$tmp/summary"; then echo "ok: summary"; else fail "summary"; fi

if grep -q flaky "$tmp/out" "$tmp/summary"; then fail "the flaky test is reported"; else echo "ok: the flaky test is left out"; fi

# A log with no failed test (a build error): a notice, a summary line, exit 0.
printf 'error[E0425]: cannot find value x in this scope\nerror: could not compile clauseo-store\n' > "$tmp/build.log"
status=0
GITHUB_STEP_SUMMARY=$tmp/summary2 bash "$root/.github/scripts/windows-pr-failures.sh" "$tmp/build.log" > "$tmp/out2" || status=$?
if [ "$status" = 0 ] && grep -q '^::notice title=No failed test in the log::' "$tmp/out2" && ! grep -q '::error' "$tmp/out2" && grep -q 'No failed test in the log' "$tmp/summary2"; then
  echo "ok: no failed test"
else
  fail "no failed test: exit $status, $(tr '\n' ' ' < "$tmp/out2")"
fi

# A % in a name is escaped for the workflow command.
printf '        FAIL [   0.010s] (1/1) clauseo-x tests::pct_100%%\n' > "$tmp/pct.log"
GITHUB_STEP_SUMMARY=$tmp/summary3 bash "$root/.github/scripts/windows-pr-failures.sh" "$tmp/pct.log" > "$tmp/out3"
if grep -qxF "${title}clauseo-x tests::pct_100%25 — FAIL" "$tmp/out3"; then echo "ok: % escaped"; else fail "% escaped: $(cat "$tmp/out3")"; fi

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all windows-pr-failures.sh cases passed"
