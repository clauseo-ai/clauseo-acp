#!/usr/bin/env bash
# The fixtures of changes.sh: each case is a list of changed paths and the outputs they must
# give. ci.yml's `changes` job runs this before it classifies, so a rule that would let a code
# change skip its jobs fails the required check `changes` instead of reporting green.
# Run it from the repo root: bash .github/scripts/changes-test.sh
#
# It also recomputes changes.sh's APP_CRATES, clauseo-desktop's path-dependency closure, from
# crates/*/Cargo.toml, and fails when the two differ.

set -euo pipefail
cd "$(dirname "$0")/../.."

script=.github/scripts/changes.sh
fail=0

# check <expected "rust app ui product"> <path>...
check() {
  local want=$1
  shift
  local got
  got=$(printf '%s\0' "$@" | bash "$script" 2> /dev/null \
    | sed -nE 's/^(rust|app|ui|product)=//p' | tr '\n' ' ' | sed 's/ $//')
  if [ "$got" != "$want" ]; then
    echo "FAIL: $* -> rust app ui product = $got, want $want"
    fail=1
  fi
}

T="true"
F="false"

# Docs-only: nothing heavy runs.
check "$F $F $F $F" README.md
check "$F $F $F $F" docs/architecture/overview.md docs/img/flow.svg
# product follows comment.ts's isProductPath, which goes by folder alone.
check "$F $F $F $T" crates/host/README.md ui/app/NOTES.md
check "$F $F $F $F" LICENSE LICENSES/MIT.txt cla/signatures.json CLA.md
check "$F $F $F $F" .github/CODEOWNERS .github/pull_request_template.md
check "$F $F $F $F" rustfmt.toml taplo.toml _typos.toml deny.toml .gitleaks.toml lefthook.yml
check "$F $F $F $F" .gitignore .gitattributes
check "$F $F $F $T" ui/.gitignore
check "$F $F $F $F" fuzz/fuzz_targets/acp.rs fuzz/nightly.sh

# ui-only: smoke runs, the Rust build matrix does not.
check "$F $T $T $T" ui/app/src/App.tsx
check "$F $T $T $T" ui/pnpm-lock.yaml ui/e2e/app-windows/smoke.spec.ts
check "$F $T $T $T" ui/components/button.tsx docs/ui.md
# Read by Rust tests (the scan at the end finds every such file).
check "$T $T $T $T" ui/app/src/transport/bindings.ts
check "$T $T $T $T" ui/components/src/clauseo/shell.css
check "$T $F $F $F" docs/decisions/0006-store-format.md
check "$T $F $F $F" docs/ui-states.md docs/other.md

# Rust in an app crate: everything but the ui flag.
check "$T $T $F $T" crates/desktop/src/lib.rs
check "$T $T $F $T" crates/host/src/session.rs
check "$T $T $F $T" crates/fake_agent/src/main.rs
check "$T $T $F $T" crates/acp/Cargo.toml
# Rust outside the app: the build matrix, no smoke.
check "$T $F $F $T" crates/limits/src/lib.rs
check "$T $F $F $T" crates/reducer_bench/benches/apply.rs
check "$T $F $F $T" crates/codex_appserver/src/lib.rs
check "$T $F $F $F" xtask/src/main.rs
check "$T $F $F $F" .config/nextest.toml
check "$T $F $F $T" budgets.toml
# What every Rust build reads.
check "$T $T $F $T" Cargo.lock
check "$T $T $F $T" Cargo.toml
check "$T $T $F $T" rust-toolchain.toml
check "$T $T $F $T" .cargo/config.toml clippy.toml
check "$T $T $F $F" clippy.toml

# Mixed.
check "$T $T $T $T" crates/limits/src/lib.rs ui/app/src/App.tsx
check "$T $T $F $T" README.md crates/store/src/lib.rs

# CI itself runs everything, the scoreboards included.
check "$T $T $T $T" .github/workflows/ci.yml
check "$T $T $T $T" .github/scripts/changes.sh
check "$T $T $T $T" .github/actions/install-tauri-linux-deps/action.yml

# Fail safe: an unknown path, or no path at all, runs everything, product included.
check "$T $T $T $T" some-new-dir/thing.rs
check "$T $T $T $T" README.md Makefile
check "$T $T $T $T" crates
check "$T $T $T $T"

# A path with a newline in it is one path: "README.md<newline>" is not prose, and
# "README.md<newline>ui/x" is neither prose nor ui; both are unknown and run everything.
check "$T $T $T $T" $'README.md\n'
check "$T $T $T $T" $'README.md\nui/app/src/App.tsx'
check "$T $T $T $T" $'docs/a.md\ncrates/desktop/src/lib.rs'

# APP_CRATES is clauseo-desktop's path-dependency closure. Each crate's Cargo.toml names a
# workspace crate as `path = "../<dir>"`; `path = "."` and `path = "src/..."` are its own targets.
deps() {
  sed -n 's/.*path = "\.\.\/\([a-z_]*\)".*/\1/p' "crates/$1/Cargo.toml" | sort -u
}
closure="desktop"
frontier="desktop"
while [ -n "$frontier" ]; do
  next=""
  for c in $frontier; do
    for d in $(deps "$c"); do
      case " $closure " in
        *" $d "*) ;;
        *)
          closure="$closure $d"
          next="$next $d"
          ;;
      esac
    done
  done
  frontier=$next
done
# shellcheck disable=SC2086 # split the space-separated list
want=$(printf '%s\n' $closure | sort | tr '\n' ' ')
listed=$(sed -n 's/^APP_CRATES="\(.*\)"$/\1/p' "$script" | tr ' ' '\n' | sort | tr '\n' ' ')
if [ "$want" != "$listed" ]; then
  echo "FAIL: APP_CRATES in $script is '$listed'; clauseo-desktop's path-dependency closure is '$want'"
  fail=1
fi

# Every file outside crates/ that Rust code reads by a relative path must run the Rust jobs:
# literals that climb out of the crate into ui/ or docs/ (include_str!, or a read from
# CARGO_MANIFEST_DIR), and every Markdown file a crate include_str!s, wherever it is.
normalize() { # a/b/../c -> a/c
  local out=() seg
  local IFS=/
  for seg in $1; do
    case "$seg" in
      "" | .) ;;
      ..) [ ${#out[@]} -gt 0 ] && unset 'out[${#out[@]}-1]' ;;
      *) out+=("$seg") ;;
    esac
  done
  printf '%s\n' "${out[*]}"
}
reads=$(
  grep -rhoE --include='*.rs' '"(\.\./)+(ui|docs)/[^"]+"' crates | tr -d '"' | sed -E 's#^(\.\./)+##'
  grep -rnoE --include='*.rs' 'include_str!\("[^"]+\.md"\)' crates | while IFS= read -r hit; do
    file=${hit%%:*}
    lit=${hit#*include_str!(\"}
    lit=${lit%\")}
    normalize "$(dirname "$file")/$lit"
  done
)
reads=$(printf '%s\n' "$reads" | sort -u)
case "$reads" in
  *ui/app/src/transport/bindings.ts*) ;;
  *)
    echo "FAIL: the scan for files Rust reads found no ui/app/src/transport/bindings.ts; it is broken"
    fail=1
    ;;
esac
for p in $reads; do
  rust=$(printf '%s\0' "$p" | bash "$script" 2> /dev/null | sed -n 's/^rust=//p')
  if [ "$rust" != true ]; then
    echo "FAIL: Rust code under crates/ reads $p, but changes.sh gives it rust=$rust"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "changes.sh: all fixtures pass; APP_CRATES matches clauseo-desktop's closure ($want);"
echo "every file Rust reads outside the crates runs the Rust jobs: $(printf '%s\n' "$reads" | tr '\n' ' ')"
