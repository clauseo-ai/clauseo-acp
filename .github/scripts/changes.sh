#!/usr/bin/env bash
# Decides which heavy CI jobs a pull request needs, from the paths it changes. ci.yml's `changes`
# job and e2e-windows.yml's `e2e-changes` job both call it; changes-test.sh pins its rules with
# fixtures (paths in, outputs out) and runs in `changes` before it.
#
#   git diff --no-renames -z --name-only BASE...HEAD | .github/scripts/changes.sh
#
# Reads NUL-separated paths on stdin (newline-separated works too, for paths without newlines)
# and prints one `name=value` line per output, ready for $GITHUB_OUTPUT:
#
# - rust:    the Rust builds and tests on every target (ci.yml's build matrix, reducer-bench).
# - app:     the app end to end (e2e-windows.yml's smoke and scoreboard-windows): the desktop
#            crate, the crates it runs on, fake_agent, the ui, and what builds them.
# - ui:      the ui changed. Informational: the fast lane decides its own ui steps.
# - product: product code changed (crates/, ui/, Cargo.toml, Cargo.lock), the paths of
#            comment.ts's isProductPath: the scoreboards run and the scoreboard comment posts.
#
# Fail safe: a path no rule knows turns every output on, and so does an empty diff (a diff that
# lists nothing is more likely a broken fetch than a no-op pull request). Each rule is a `case`
# pattern below; a new top-level file or folder needs a rule, or it runs everything.
#
# APP_CRATES is clauseo-desktop and every crate it reaches through path dependencies,
# dev-dependencies included (fake_agent). changes-test.sh recomputes that closure from
# crates/*/Cargo.toml and fails when this list differs.

set -euo pipefail

APP_CRATES="desktop host protocol settings process fake_agent reducer store matters acp"

rust=false app=false ui=false product=false unknown=""
count=0
paths=()

all() { rust=true app=true ui=true; }

is_app_crate() {
  local c
  for c in $APP_CRATES; do
    [ "$c" = "$1" ] && return 0
  done
  return 1
}

classify() {
  local p=$1
  case "$p" in
    # Review routing and the pull request template: read by people, not jobs.
    .github/CODEOWNERS | .github/pull_request_template.md) ;;
    # Workflows, actions and these scripts: a CI change runs every job it could break.
    .github/*) all ;;
    # Prose. The fast lane still runs and checks it (links, typos, the docs rules).
    *.md | docs/* | LICENSE | LICENSES/* | cla/*) ;;
    # Read by the fast lane alone (formatting, typos, TOML style, licences, secrets, hooks).
    rustfmt.toml | taplo.toml | _typos.toml | deny.toml | .gitleaks.toml | lefthook.yml) ;;
    .gitignore | .gitattributes | */.gitignore | */.gitattributes) ;;
    # Nightly fuzzing only (fuzz.yml); no pull request job builds fuzz/.
    fuzz/*) ;;
    # What every Rust build reads.
    Cargo.toml | Cargo.lock | rust-toolchain.toml | .cargo/* | clippy.toml) rust=true app=true ;;
    .config/nextest.toml) rust=true ;;
    # reducer-bench's limits and the scoreboard budgets.
    budgets.toml) rust=true ;;
    xtask/*) rust=true ;;
    crates/*/*)
      rust=true
      local crate=${p#crates/}
      crate=${crate%%/*}
      if is_app_crate "$crate"; then app=true; fi
      ;;
    # Written by a unit test of the desktop crate (src/commands/bindings.rs).
    ui/app/src/transport/bindings.ts) ui=true app=true rust=true ;;
    ui/*) ui=true app=true ;;
    *)
      all
      unknown="$unknown $p"
      ;;
  esac
}

while IFS= read -r -d '' p || [ -n "$p" ]; do
  # Newline-separated input arrives as one record; split it.
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    count=$((count + 1))
    paths+=("$line")
    classify "$line"
  done <<< "$p"
done

if [ "$count" -eq 0 ]; then
  echo "changes.sh: no changed paths; running everything" >&2
  all
  product=true
else
  # One grep, which ui/e2e/scoreboard/comment.test.ts checks against comment.ts's isProductPath.
  product=$(printf '%s\n' "${paths[@]}" | grep -E '^(crates/|ui/|Cargo\.toml$|Cargo\.lock$)' || true)
  if [ -n "$product" ]; then product=true; else product=false; fi
fi
if [ -n "$unknown" ]; then
  echo "changes.sh: no rule for:$unknown; running everything" >&2
fi

echo "rust=$rust"
echo "app=$app"
echo "ui=$ui"
echo "product=$product"
