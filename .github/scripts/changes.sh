#!/usr/bin/env bash
# Decides which heavy CI jobs a pull request needs, from the paths it changes. ci.yml's `changes`
# job and e2e-windows.yml's `e2e-changes` job both call it; changes-test.sh pins its rules with
# fixtures (paths in, outputs out) and runs in `changes` before it.
#
#   git diff --no-renames -z --name-only BASE...HEAD | .github/scripts/changes.sh
#
# Reads NUL-separated paths on stdin, as `git diff -z` writes them (a path may hold a newline,
# so nothing splits on one), and prints one `name=value` line per output, ready for $GITHUB_OUTPUT:
#
# - rust:    the Rust builds and tests on every target (ci.yml's build matrix, reducer-bench).
# - app:     the app end to end (e2e-windows.yml's smoke and scoreboard-windows): the desktop
#            crate, the crates it runs on, fake_agent, the ui, and what builds them.
# - ui:      the ui changed. Informational: the fast lane decides its own ui steps.
# - product: product code changed (crates/, ui/, Cargo.toml, Cargo.lock), the paths of
#            comment.ts's isProductPath, or what builds or budgets it (rust-toolchain.toml,
#            .cargo/, budgets.toml): the scoreboards run. Everything on when all() is.
# - worker_lock: what the counts Worker (services/counts-worker/, a workspace with its own
#            Cargo.lock) resolves could have changed: its folder, crates/counts/, a Cargo.toml in
#            crates/ or at the root (crates/counts inherits the root's workspace dependencies),
#            or the root Cargo.lock. ci.yml's counts-worker-lock job checks that its lock file
#            still resolves with --locked (2026-11-16: #1173 gave crates/counts a libc dependency,
#            updated only the root Cargo.lock, and counts-deploy.yml failed on main).
#
# Fail safe: a path no rule knows turns every output on, and so does an empty diff (a diff that
# lists nothing is more likely a broken fetch than a no-op pull request). Each rule is a `case`
# pattern below; a new top-level file or folder needs a rule, or it runs everything.
#
# APP_CRATES is clauseo-desktop and every crate it reaches through path dependencies,
# dev-dependencies included (fake_agent; agents, limits' dev-dependency). changes-test.sh
# recomputes that closure from crates/*/Cargo.toml and fails when this list differs.

set -euo pipefail

APP_CRATES="desktop host search protocol settings process fake_agent reducer store matters acp limits codex_appserver connectors profile agents counts sandbox blocklist"

nl=$'\n'
rust=false app=false ui=false product=false product_also=false worker_lock=false unknown=""
count=0
paths=()

# Fail safe means everything: the scoreboards and the comment too.
all() { rust=true app=true ui=true product_also=true worker_lock=true; }

is_app_crate() {
  local c
  for c in $APP_CRATES; do
    [ "$c" = "$1" ] && return 0
  done
  return 1
}

# A path in a crate: the Rust jobs, and the app's too when the crate is in clauseo-desktop's closure.
crate_path() {
  rust=true
  local crate=${1#crates/}
  crate=${crate%%/*}
  if is_app_crate "$crate"; then app=true; fi
}

classify() {
  local p=$1
  # No rule is written for a path with a newline in it (a glob's * matches one), so it is unknown.
  case "$p" in
    *"$nl"*)
      all
      unknown="$unknown $p"
      return
      ;;
  esac
  # The counts Worker's lock file, on top of whatever the rule below decides for the path. A case
  # glob's * matches a /, so crates/*/Cargo.toml is a Cargo.toml at any depth under crates/.
  case "$p" in
    Cargo.toml | Cargo.lock | crates/*/Cargo.toml | crates/counts/* | services/counts-worker/*)
      worker_lock=true
      ;;
  esac
  case "$p" in
    # Review routing and the pull request template: read by people, not jobs.
    .github/CODEOWNERS | .github/pull_request_template.md) ;;
    # Workflows, actions and these scripts: a CI change runs every job it could break.
    .github/*) all ;;
    # Files outside the crates that Rust tests read (include_str! or a read from the crate's
    # folder). changes-test.sh finds every such read under crates/ and fails when one is missing.
    docs/decisions/0002-event-schema.md | docs/decisions/0003-error-codes.md) rust=true ;;
    docs/decisions/0006-store-format.md | docs/ui-states.md) rust=true ;;
    ui/app/src/transport/bindings.ts | ui/components/src/styles/tokens-parts.css) ui=true app=true rust=true ;;
    ui/components/src/clauseo/shell.css | ui/components/src/clauseo/room-toolbar.tsx) ui=true app=true rust=true ;;
    ui/components/src/clauseo/responsive.css | ui/components/src/styles/tokens-colour.css) ui=true app=true rust=true ;;
    ui/app/src/review/registry-h11.tsx | ui/package.json) ui=true app=true rust=true ;;
    # The page's first mark: native_mark.rs tests that its comma matches index.html's.
    ui/app/index.html) ui=true app=true rust=true ;;
    # The native dialogs' review page: dialogs/words and webview2 tests check it matches their tables.
    ui/app/src/review/registry-nd.tsx) ui=true app=true rust=true ;;
    # The review registry pages and the room's commands: clauseo-desktop's menu and MCP words check
    # their tables against them (menu.rs, menu/words.rs, mcp/words.rs, mcp/import_words.rs), and
    # new registry pages keep coming (in review/auto/, which registry-auto.ts collects).
    ui/app/src/review/registry-*.tsx | ui/app/src/room/room-commands.ts) ui=true app=true rust=true ;;
    ui/app/src/review/auto/*.tsx) ui=true app=true rust=true ;;
    docs/firm-it.md) rust=true ;;
    ui/app/doc-approvals.json) ui=true app=true rust=true ;;
    # The copy approvals: profile's legal tests (crates/profile/tests/legal/) check its lines against them.
    ui/app/copy-approvals/*) ui=true app=true rust=true ;;
    # The window's specs and scenarios: the host's e2e_scenarios test launches each one they play.
    ui/e2e/app-windows | ui/e2e/app-windows/*) ui=true app=true rust=true ;;
    # A crate's assets are built into it (include_str!), Markdown too, so they are not prose.
    crates/*/assets/*) crate_path "$p" ;;
    # Prose. The fast lane still runs and checks it (links, typos, the docs rules).
    *.md | docs/* | LICENSE | LICENSES/* | cla/*) ;;
    # Read by the fast lane alone (formatting, typos, TOML style, licences, secrets, hooks).
    rustfmt.toml | taplo.toml | _typos.toml | deny.toml | .gitleaks.toml | lefthook.yml) ;;
    .gitignore | .gitattributes | */.gitignore | */.gitattributes) ;;
    # Nightly fuzzing only (fuzz.yml); no pull request job builds fuzz/.
    fuzz/*) ;;
    # The counts Worker: its own workspace, built and tested by counts-worker.yml; ci.yml checks
    # only its lock file (worker_lock, above).
    services/counts-worker/*) ;;
    # What every Rust build reads.
    Cargo.toml | Cargo.lock | clippy.toml) rust=true app=true ;;
    # The toolchain and cargo config change the binaries, so the scoreboards run too.
    rust-toolchain.toml | .cargo/*) rust=true app=true product_also=true ;;
    .config/nextest.toml) rust=true ;;
    # reducer-bench's limits and the scoreboard budgets.
    budgets.toml) rust=true product_also=true ;;
    xtask/*) rust=true ;;
    crates/*/*) crate_path "$p" ;;
    ui/*) ui=true app=true ;;
    *)
      all
      unknown="$unknown $p"
      ;;
  esac
}

while IFS= read -r -d '' p || [ -n "$p" ]; do
  [ -n "$p" ] || continue
  count=$((count + 1))
  paths+=("$p")
  classify "$p"
done

if [ "$count" -eq 0 ]; then
  echo "changes.sh: no changed paths; running everything" >&2
  all
  product=true
else
  # One grep, which ui/e2e/scoreboard/comment.test.ts checks against comment.ts's isProductPath.
  # A newline inside a path becomes a space first, so no part of a path passes for a path.
  product=$(printf '%s\n' "${paths[@]//$nl/ }" | grep -E '^(crates/|ui/|Cargo\.toml$|Cargo\.lock$)' || true)
  if [ -n "$product" ] || [ "$product_also" = true ]; then product=true; else product=false; fi
fi
if [ -n "$unknown" ]; then
  echo "changes.sh: no rule for:$unknown; running everything" >&2
fi

echo "rust=$rust"
echo "app=$app"
echo "ui=$ui"
echo "product=$product"
echo "worker_lock=$worker_lock"
