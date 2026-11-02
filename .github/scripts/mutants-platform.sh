#!/usr/bin/env bash
# Prepares one job of mutants-platform.yml: writes the cargo-mutants config it runs with, and checks
# that each file it mutates gives at least one mutant.
#
# Usage: PACKAGES="clauseo-a ..." FILES="crates/a/src/x.rs ..." mutants-platform.sh OUT
#
# - OUT is .cargo/mutants.toml without the `exclude_globs` entries that name one of the FILES, so
#   every other setting (test_tool, the nextest args, gitignore, copy_vcs, exclude_re, the other
#   globs) still applies. Those entries keep the settings OS shims out of the Linux runs, where they
#   are not compiled; on their own OS they are, and they are why the workflow exists. Each entry is
#   one quoted path on its own line (taplo formats the file so), and a FILE whose entry is gone
#   from the file is fine: there is nothing to drop.
# - cargo-mutants reads OUT with `--config OUT` (cargo-mutants 27.1.0, src/main.rs: "Read
#   configuration from this file instead of .cargo/mutants.toml").
# - The workspace has default-members, so cargo-mutants mutates only those unless `-p` names the
#   packages; a `--file` outside them lists nothing.
# - A FILE with no mutant fails the job: a typo, a file moved, or a glob or `exclude_re` that still
#   covers it would otherwise leave it untested and the report clean.
set -euo pipefail

if [ "$#" -ne 1 ] || [ -z "${PACKAGES:-}" ] || [ -z "${FILES:-}" ]; then
  echo "usage: PACKAGES=... FILES=... $0 OUT" >&2
  exit 1
fi
out=$1
packages=()
for package in $PACKAGES; do
  packages+=(-p "$package")
done

awk -v files="$FILES" '
  BEGIN { n = split(files, list, " "); for (i = 1; i <= n; i++) drop["\"" list[i] "\""] = 1 }
  {
    entry = $0
    sub(/^[ \t]+/, "", entry)
    sub(/,?[ \t]*$/, "", entry)
    if (entry in drop) { print "dropped from exclude_globs: " entry > "/dev/stderr"; next }
    print
  }
' .cargo/mutants.toml > "$out"

failed=0
for file in $FILES; do
  # A failed listing (a broken config, a package that does not exist) stops here: its stderr,
  # cargo-mutants' own error, goes straight to the log, and only stdout is counted.
  status=0
  listed=$(cargo mutants --list --config "$out" "${packages[@]}" --file "$file") || status=$?
  if [ "$status" -ne 0 ]; then
    echo "::error::cargo mutants --list exited $status for $file"
    exit "$status"
  fi
  # tr: cargo on Windows may end its lines with CRLF.
  count=$(printf '%s' "$listed" | tr -d '\r' | grep -c . || true)
  echo "$file: $count mutants"
  if [ "$count" -eq 0 ]; then
    echo "::error::$file gives no mutant with $out; check the path and the exclusions"
    failed=1
  fi
done
exit "$failed"
