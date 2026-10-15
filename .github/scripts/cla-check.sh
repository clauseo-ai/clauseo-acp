#!/usr/bin/env bash
# CLA check: every login in a pull request has signed the CLA (version 1.0), or is exempt.
#
# Inputs, from the environment:
#   LOGINS    whitespace-separated logins to check. "UNLINKED:<email>" marks a commit
#             whose author email is not linked to a GitHub account.
#   TREE_REF  a git ref whose tree holds cla/signatures/ and cla/entities/.
#   EXEMPT    whitespace-separated logins that need no signature (case-insensitive).
#
# A login passes when it is exempt, ends in [bot], or has either
#   - cla/signatures/<login>.md in TREE_REF containing "version 1.0" and the login, or
#   - a cla/entities/*.md file in TREE_REF with a line "- @<login>".
# Output: ::error:: lines with a fix: line, then one "<login>: ok|missing" line each.
set -euo pipefail
# Logins are never globbed: "[bot]" would otherwise match file names.
set -f

if [ -z "${LOGINS:-}" ]; then
  echo "::error::no logins to check"
  exit 1
fi
: "${TREE_REF:?TREE_REF is not set}"
EXEMPT="${EXEMPT:-}"

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

# Entity files in the tree, one path per line.
entity_files=$(git ls-tree --name-only "$TREE_REF" cla/entities/ | grep -E '^cla/entities/[^/]+\.md$' || true)

# $1 = lower-case login
has_signature() {
  local path="cla/signatures/$1.md" content
  git cat-file -e "$TREE_REF:$path" 2>/dev/null || return 1
  content=$(git show "$TREE_REF:$path")
  grep -qiF -- "version 1.0" <<< "$content" && grep -qiF -- "$1" <<< "$content"
}

# $1 = login as written in LOGINS
is_covered_by_entity() {
  local f content
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    content=$(git show "$TREE_REF:$f")
    if grep -qixF -- "- @$1" <<< "$content"; then
      return 0
    fi
  done <<< "$entity_files"
  return 1
}

fail=0
seen=" "
results=""
for entry in $LOGINS; do
  key=$(lower "$entry")
  case "$seen" in *" $key "*) continue ;; esac
  seen="$seen$key "

  if [ "${entry#UNLINKED:}" != "$entry" ]; then
    echo "::error::commit author email ${entry#UNLINKED:} is not linked to a GitHub account"
    echo "    fix: add this email to your GitHub account, or rewrite the commit author with git commit --amend --author and push again"
    results="$results$entry: missing"$'\n'
    fail=1
    continue
  fi

  exempt=0
  for e in $EXEMPT; do
    if [ "$(lower "$e")" = "$key" ]; then
      exempt=1
    fi
  done
  case "$key" in
    *'[bot]') exempt=1 ;;
  esac

  if [ "$exempt" -eq 1 ] || has_signature "$key" || is_covered_by_entity "$entry"; then
    results="$results$entry: ok"$'\n'
  else
    echo "::error::@$entry has not signed the CLA (version 1.0)"
    echo "    fix: copy cla/TEMPLATE-individual.md to cla/signatures/$key.md, fill it in and push it to this PR (see cla/README.md)"
    results="$results$entry: missing"$'\n'
    fail=1
  fi
done

printf '%s' "$results"
exit "$fail"
