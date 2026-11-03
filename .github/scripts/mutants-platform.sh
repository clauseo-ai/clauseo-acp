#!/usr/bin/env bash
# Prepares one job of mutants-platform.yml: writes the cargo-mutants config it runs with, checks
# that each regex in the job's skip list still names a mutant, and checks that each file it mutates
# gives at least one mutant.
#
# Usage: PACKAGES="clauseo-a ..." FILES="crates/a/src/x.rs ..." [SKIP_RE='["regex", ...]'] \
#          mutants-platform.sh OUT
#
# - OUT is .cargo/mutants.toml without the `exclude_globs` entries that name one of the FILES, so
#   every other setting (test_tool, the nextest args, gitignore, copy_vcs, exclude_re, the other
#   globs) still applies. Those entries keep the settings OS shims out of the Linux runs, where they
#   are not compiled; on their own OS they are, and they are why the workflow exists. Each entry is
#   one quoted path on its own line (taplo formats the file so), and a FILE whose entry is gone
#   from the file is fine: there is nothing to drop.
# - SKIP_RE is the job's `skip_re` as a JSON array (`null` or unset when it has none). Each regex
#   is added to the end of OUT's `exclude_re`, as a TOML literal string, so it applies to this job
#   only: a mutant missed only because of this job's OS (code not compiled here, or equivalent here
#   but caught on another OS). A mutant no OS can catch goes in .cargo/mutants.toml instead.
# - A skip regex that matches no mutant of the job fails it, before any mutant runs, and the error
#   names the regex: the code moved or changed, and the entry is stale. Each regex is matched with
#   perl (see below for why not cargo-mutants' `--re`) against the job's listing with
#   .cargo/mutants.toml's exclusions but none of the skip list, so an entry that only repeats a
#   global exclusion is stale too.
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
files=()
for file in $FILES; do
  files+=(--file "$file")
done

# The skip list, one regex per line. tr: jq on Windows may end its lines with CRLF.
skips=()
if [ -n "${SKIP_RE:-}" ]; then
  while IFS= read -r regex; do
    skips+=("$regex")
  done < <(printf '%s' "$SKIP_RE" | jq -r '(. // [])[]' | tr -d '\r')
fi
for regex in ${skips[@]+"${skips[@]}"}; do
  # A TOML literal string cannot hold a single quote, and an empty regex would match everything.
  case "$regex" in
    '' | *"'"*)
      echo "::error::skip regex '$regex' is empty or holds a single quote"
      exit 1
      ;;
  esac
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

# Stale skip regexes: each must match a mutant of the job's listing under the config without the
# skip list. Matched with perl, whose syntax covers what the regexes use (\d, escapes, groups,
# classes, \x27), not with cargo-mutants' `--re`: 27.1.0 applies neither `--re` nor `exclude_re`
# to its "delete field" mutants, so with `--re` a stale regex would still list one there.
stale=0
if [ "${#skips[@]}" -gt 0 ]; then
  status=0
  listed=$(cargo mutants --list --config "$out" "${packages[@]}" "${files[@]}") || status=$?
  if [ "$status" -ne 0 ]; then
    echo "::error::cargo mutants --list exited $status for the job's files"
    exit "$status"
  fi
  listed=$(printf '%s\n' "$listed" | tr -d '\r')
  for regex in "${skips[@]}"; do
    count=$(printf '%s\n' "$listed" | SKIP="$regex" perl -ne '$n++ if /$ENV{SKIP}/; END { print $n + 0 }') || {
      echo "::error::skip regex '$regex' is not a regex perl can read"
      exit 1
    }
    echo "skip regex '$regex': $count mutants"
    if [ "$count" -eq 0 ]; then
      echo "::error::stale skip regex '$regex' matches no mutant of this job; update or remove it in mutants-platform.yml"
      stale=1
    fi
  done
fi
if [ "$stale" -ne 0 ]; then
  exit 1
fi

# The skip list goes at the end of exclude_re, before the line that closes the array. The regexes
# reach awk through ENVIRON, which, unlike -v, leaves their backslashes as they are.
if [ "${#skips[@]}" -gt 0 ]; then
  base=$(cat "$out")
  printf '%s\n' "$base" | SKIP_LINES="$(printf '%s\n' "${skips[@]}")" awk -v q="'" '
    BEGIN { n = split(ENVIRON["SKIP_LINES"], skip, "\n") }
    /^exclude_re = \[/ { inside = 1 }
    inside && /^\]/ {
      print "  # The skip_re of this job (mutants-platform.yml)."
      for (i = 1; i <= n; i++) print "  " q skip[i] q ","
      inside = 0
      added = 1
    }
    { print }
    END { if (!added) { print "no exclude_re array to add the skip list to" > "/dev/stderr"; exit 1 } }
  ' > "$out"
fi

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
