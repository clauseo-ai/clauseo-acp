#!/usr/bin/env bash
# Tests pre-push.sh, lefthook's pre-push command: a push of deletes only passes without the check,
# and every other push runs `cargo xtask check --quick --no-tests`, in CI mode unless CI says otherwise, with
# `--skip-ui` only when changes.sh says every pushed branch changes no ui path, and then Windows clippy for
# the crates the push touches when the x86_64-pc-windows-gnu target and MinGW-w64 are there. First
# the script alone, fed git's ref lines; then through lefthook, with this repo's lefthook.yml, in a
# scratch repo that pushes to a local bare remote, so the test also fails when lefthook stops
# passing git's stdin on. A fake `cargo` first on PATH records each call instead of checking; a fake
# `rustup` lists the targets in FAKE_TARGETS, and a fake MinGW-w64 gcc is on PATH only where a case
# puts $tmp/mingw there.
# fast-lane runs it after installing the pinned tools, lefthook among them.
# Run it from anywhere: bash .github/scripts/pre-push-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD

# Inside a git hook these point git at the outer repo.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_PREFIX

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin"
cat > "$tmp/bin/cargo" <<'EOF'
#!/usr/bin/env bash
echo "CI=${CI-unset} $*" >> "$CALLS"
if [ "$1" = clippy ]; then exit "${FAKE_CLIPPY_EXIT:-0}"; fi
exit "${FAKE_CARGO_EXIT:-0}"
EOF
cat > "$tmp/bin/rustup" <<'EOF'
#!/usr/bin/env bash
if [ "$*" = "target list --installed" ] && [ -n "${FAKE_TARGETS:-}" ]; then printf '%s\n' $FAKE_TARGETS; fi
EOF
mkdir "$tmp/mingw"
printf '#!/usr/bin/env bash\n' > "$tmp/mingw/x86_64-w64-mingw32-gcc"
chmod +x "$tmp/bin/cargo" "$tmp/bin/rustup" "$tmp/mingw/x86_64-w64-mingw32-gcc"
export PATH="$tmp/bin:$PATH" CALLS="$tmp/calls"
unset FAKE_TARGETS PREPUSH_WINDOWS_CLIPPY

failures=0
fail() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}
# $1 name, $2 the cargo calls expected (empty: none), $3 the exit code expected; the run's output is in $tmp/out.
expect() {
  local calls
  calls=$(cat "$CALLS" 2>/dev/null || true)
  if [ "$calls" != "$2" ]; then fail "$1: cargo calls [$calls], expected [$2]"; return; fi
  if [ "$status" != "$3" ]; then fail "$1: exit $status, expected $3"; return; fi
  echo "ok: $1"
}

z40=0000000000000000000000000000000000000000
z64=$z40$z40
z64=${z64:0:64}
a=1111111111111111111111111111111111111111
b=2222222222222222222222222222222222222222
check="CI=1 xtask check --quick --no-tests"
skip="CI=1 xtask check --quick --no-tests --skip-ui"
skip_note="pre-push: no ui change on the pushed branch; skipping the ui steps (CI runs them)"
win="x86_64-pc-windows-gnu"
win_note="pre-push: Windows clippy skipped: it needs"
# The Windows clippy call for the crates named.
winclippy() {
  local args="" crate
  for crate in "$@"; do args="$args -p $crate"; done
  echo "CI=1 clippy --target $win --all-targets$args -- -D warnings"
}

# ---- the script alone: $1 name, $2 stdin, $3 calls expected, $4 exit expected; env from the caller.
alone() {
  rm -f "$CALLS"
  status=0
  printf '%s' "$2" | bash "$root/.github/scripts/pre-push.sh" > "$tmp/out" 2>&1 || status=$?
  expect "alone: $1" "$3" "$4"
}
unset CI
alone "one delete" "refs/heads/x $z40 refs/heads/x $a"$'\n' "" 0
grep -q "deletes only (1 ref(s)), nothing to check" "$tmp/out" || fail "alone: one delete: no message (output: $(cat "$tmp/out"))"
alone "two deletes" "(delete) $z40 refs/heads/x $a"$'\n'"(delete) $z40 refs/tags/t $b"$'\n' "" 0
alone "a delete in a SHA-256 repo" "(delete) $z64 refs/heads/x ${a}${a:0:24}"$'\n' "" 0
alone "an update" "refs/heads/x $a refs/heads/x $b"$'\n' "$check" 0
alone "a new branch" "refs/heads/x $a refs/heads/x $z40"$'\n' "$check" 0
alone "a delete and an update" "(delete) $z40 refs/heads/x $a"$'\n'"refs/heads/y $b refs/heads/y $a"$'\n' "$check" 0
alone "no refs" "" "$check" 0
alone "a line without a sha" "refs/heads/x"$'\n' "$check" 0
CI=0 alone "CI=0 runs the check in local mode" "refs/heads/x $a refs/heads/x $b"$'\n' "CI=0 xtask check --quick --no-tests" 0
FAKE_CARGO_EXIT=3 alone "a failed check fails the push" "refs/heads/x $a refs/heads/x $b"$'\n' "$check" 3
FAKE_CARGO_EXIT=3 alone "a delete passes whatever the check would say" "(delete) $z40 refs/heads/x $a"$'\n' "" 0

# Runs git in the scratch repo `$repo` (set by each section), whatever the user's git config says.
g() { git -C "$repo" -c user.name=test -c user.email=test@example.com -c commit.gpgsign=false "$@"; }
# A scratch repo at $1 with this pre-push.sh and changes.sh (and lefthook.yml) on main, its first
# commit; `repo` names it.
new_repo() {
  repo=$1
  git init -q -b main "$repo"
  mkdir -p "$repo/.github/scripts"
  cp "$root/lefthook.yml" "$repo/"
  cp "$root/.github/scripts/pre-push.sh" "$root/.github/scripts/changes.sh" "$repo/.github/scripts/"
  g add -A
  g commit -q --no-verify -m init
}
# A branch off main in `$repo` whose one commit adds the files named: $1 the branch, the rest the files.
branch() {
  local name=$1 file
  shift
  g checkout -q -b "$name" main
  for file in "$@"; do
    mkdir -p "$repo/$(dirname "$file")"
    echo "$name" > "$repo/$file"
  done
  g add -A
  g commit -q --no-verify -m "$name"
  g checkout -q main
}

# ---- the script alone in a scratch repo whose origin/main is its main, fed real shas: which
# pushes skip the ui steps. No remote, so it needs neither lefthook nor a push.
new_repo "$tmp/plain"
g update-ref refs/remotes/origin/main main
branch rust crates/core/src/lib.rs
branch rust2 crates/core/Cargo.toml
branch ui ui/app/src/App.tsx
branch mixed crates/core/src/lib.rs ui/app/src/App.tsx
# The line git writes for a push of the branch $1 to a new remote branch.
line() { echo "refs/heads/$1 $(g rev-parse "$1") refs/heads/$1 $z40"; }
# $1 name, $2 stdin, $3 calls expected, $4 exit expected; env from the caller.
in_repo() {
  rm -f "$CALLS"
  status=0
  printf '%s' "$2" | (cd "$repo" && bash .github/scripts/pre-push.sh) > "$tmp/out" 2>&1 || status=$?
  expect "in a repo: $1" "$3" "$4"
}
unset CI
in_repo "a Rust-only branch skips the ui steps" "$(line rust)"$'\n' "$skip" 0
grep -qxF "$skip_note" "$tmp/out" || fail "in a repo: a Rust-only branch: no note (output: $(cat "$tmp/out"))"
in_repo "two Rust-only branches skip the ui steps" "$(line rust)"$'\n'"$(line rust2)"$'\n' "$skip" 0
in_repo "a delete and a Rust-only branch skip the ui steps" "(delete) $z40 refs/heads/x $a"$'\n'"$(line rust)"$'\n' "$skip" 0
CI=0 in_repo "CI=0 skips the ui steps in local mode" "$(line rust)"$'\n' "CI=0 xtask check --quick --no-tests --skip-ui" 0
FAKE_CARGO_EXIT=3 in_repo "a failed check without the ui steps fails the push" "$(line rust)"$'\n' "$skip" 3
in_repo "a ui-only branch runs the ui steps" "$(line ui)"$'\n' "$check" 0
grep -qF "skipping the ui steps" "$tmp/out" && fail "in a repo: a ui-only branch: the skip note printed"
in_repo "a Rust and ui branch runs the ui steps" "$(line mixed)"$'\n' "$check" 0
in_repo "two branches, one with a ui change, run the ui steps" "$(line rust)"$'\n'"$(line ui)"$'\n' "$check" 0
in_repo "a branch with no change since origin/main runs the ui steps" "$(line main)"$'\n' "$check" 0
in_repo "no refs run the ui steps" "" "$check" 0
in_repo "a sha git does not know runs the ui steps" "refs/heads/x $a refs/heads/x $z40"$'\n' "$check" 0
in_repo "one delete is still a no-op" "(delete) $z40 refs/heads/x $a"$'\n' "" 0
rust_line="$(line rust)"$'\n'
g update-ref -d refs/remotes/origin/main
in_repo "no origin/main runs the ui steps" "$rust_line" "$check" 0
g update-ref refs/remotes/origin/main main
cp "$repo/.github/scripts/changes.sh" "$tmp/changes.sh"
printf '#!/usr/bin/env bash\necho ui=false\nexit 1\n' > "$repo/.github/scripts/changes.sh"
in_repo "changes.sh failing after ui=false runs the ui steps" "$rust_line" "$check" 0
cp "$tmp/changes.sh" "$repo/.github/scripts/changes.sh"
in_repo "changes.sh restored skips the ui steps again" "$rust_line" "$skip" 0

# ---- Windows clippy for the touched crates, in the same scratch repo. The crates are read from the
# working tree, which the check lints too, so each case checks out the branch it pushes.
# A branch off main in `$repo` whose one commit adds a crate per name given, each with its Cargo.toml
# and a source file: $1 the branch, the rest folder:package pairs (folder under crates/, or xtask).
crate_branch() {
  local name=$1 pair dir
  shift
  g checkout -q -b "$name" main
  for pair in "$@"; do
    dir=${pair%%:*}
    [ "$dir" = xtask ] || dir=crates/$dir
    mkdir -p "$repo/$dir/src"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\n' "${pair#*:}" > "$repo/$dir/Cargo.toml"
    echo "// $name" > "$repo/$dir/src/lib.rs"
  done
  g add -A
  g commit -q --no-verify -m "$name"
  g checkout -q main
}
crate_branch one core:clauseo-core
crate_branch two core:clauseo-core zeta:clauseo-zeta xtask:xtask
branch root Cargo.lock
ready="$tmp/mingw:$PATH"
nl=$'\n'
g checkout -q one
FAKE_TARGETS="$win" PATH="$ready" in_repo "a crate's change runs Windows clippy for it after the check" \
  "$(line one)$nl" "$skip$nl$(winclippy clauseo-core)" 0
grep -qF "pre-push: Windows clippy ($win) for clauseo-core" "$tmp/out" || fail "in a repo: one crate: no Windows clippy note (output: $(cat "$tmp/out"))"
FAKE_TARGETS="$win" PATH="$ready" FAKE_CLIPPY_EXIT=4 in_repo "a failed Windows clippy fails the push" \
  "$(line one)$nl" "$skip$nl$(winclippy clauseo-core)" 4
FAKE_TARGETS="$win" PATH="$ready" FAKE_CARGO_EXIT=3 in_repo "a failed check stops before Windows clippy" \
  "$(line one)$nl" "$skip" 3
FAKE_TARGETS="$win" PATH="$ready" PREPUSH_WINDOWS_CLIPPY=0 in_repo "PREPUSH_WINDOWS_CLIPPY=0 skips it" \
  "$(line one)$nl" "$skip" 0
grep -qF "PREPUSH_WINDOWS_CLIPPY=0; skipping Windows clippy" "$tmp/out" || fail "in a repo: PREPUSH_WINDOWS_CLIPPY=0: no note"
PATH="$ready" in_repo "without the target it is skipped with a note" "$(line one)$nl" "$skip" 0
grep -qF "$win_note" "$tmp/out" || fail "in a repo: no target: no note (output: $(cat "$tmp/out"))"
if command -v x86_64-w64-mingw32-gcc > /dev/null; then
  echo "skip: without MinGW-w64: this machine has a real one on PATH (CI's runner has none)"
else
  FAKE_TARGETS="$win" in_repo "without MinGW-w64 it is skipped with a note" "$(line one)$nl" "$skip" 0
  grep -qF "$win_note" "$tmp/out" || fail "in a repo: no MinGW: no note (output: $(cat "$tmp/out"))"
fi
FAKE_TARGETS="$win" PATH="$ready" in_repo "a crate the working tree lacks is left out" "$(line two)$nl" "$skip$nl$(winclippy clauseo-core)" 0
FAKE_TARGETS="$win" PATH="$ready" in_repo "a crate change and a ui change run both" "$(line one)$nl$(line ui)$nl" \
  "$check$nl$(winclippy clauseo-core)" 0
FAKE_TARGETS="$win" PATH="$ready" in_repo "no refs run no Windows clippy" "" "$check" 0
g checkout -q two
FAKE_TARGETS="$win" PATH="$ready" in_repo "three crates go in one Windows clippy, sorted" \
  "$(line two)$nl" "$skip$nl$(winclippy clauseo-core clauseo-zeta xtask)" 0
FAKE_TARGETS="$win" PATH="$ready" in_repo "two branches touching one crate name it once" \
  "$(line one)$nl$(line two)$nl" "$skip$nl$(winclippy clauseo-core clauseo-zeta xtask)" 0
FAKE_TARGETS="$win" PATH="$ready" in_repo "a root file touches no crate" "$(line root)$nl" "$skip" 0
g checkout -q main
FAKE_TARGETS="$win" PATH="$ready" in_repo "a path under crates/ with no Cargo.toml names no crate" "$(line rust)$nl" "$skip" 0

# ---- through lefthook and git push, in a scratch repo with this lefthook.yml and pre-push.sh.
if ! command -v lefthook > /dev/null; then
  fail "lefthook is not on PATH (cargo xtask setup installs it)"
else
  git init -q --bare "$tmp/remote.git"
  new_repo "$tmp/repo"
  g remote add origin "$tmp/remote.git"
  # Its own hooks folder, whatever the user's global git config says; lefthook installs into a set
  # core.hooksPath only with --force.
  g config core.hooksPath "$repo/.git/hooks"
  g push -q --no-verify origin main main:refs/heads/a main:refs/heads/b main:refs/heads/c
  (cd "$repo" && lefthook install --force > /dev/null)
  # $1 name, $2 calls expected, $3 exit expected, the rest: git push's arguments.
  pushed() {
    local name=$1 want=$2 code=$3
    shift 3
    rm -f "$CALLS"
    status=0
    (cd "$repo" && g push -q origin "$@") > "$tmp/out" 2>&1 || status=$?
    expect "lefthook: $name" "$want" "$code"
  }
  unset CI
  pushed "git push --delete" "" 0 --delete a
  git -C "$tmp/remote.git" rev-parse -q --verify refs/heads/a > /dev/null && fail "lefthook: git push --delete: the branch is still on the remote"
  pushed "two deletes" "" 0 :b :c
  pushed "a new branch with no change runs the check" "$check" 0 main:refs/heads/d
  pushed "a delete and a new branch run the check" "$check" 0 :d main:refs/heads/e
  FAKE_CARGO_EXIT=3 pushed "a failed check stops the push" "$check" 1 main:refs/heads/f
  git -C "$tmp/remote.git" rev-parse -q --verify refs/heads/f > /dev/null && fail "lefthook: a failed check stops the push: the branch reached the remote"
  branch rust crates/core/src/lib.rs
  branch ui ui/app/src/App.tsx
  branch mixed crates/core/src/lib.rs ui/app/src/App.tsx
  pushed "a Rust-only branch skips the ui steps" "$skip" 0 rust:refs/heads/rust
  pushed "a ui-only branch runs the ui steps" "$check" 0 ui:refs/heads/ui
  pushed "a Rust and ui branch runs the ui steps" "$check" 0 mixed:refs/heads/mixed
  pushed "two branches, one with a ui change, run the ui steps" "$check" 0 rust:refs/heads/rust-b ui:refs/heads/ui-b
  crate_branch one core:clauseo-core
  g checkout -q one
  FAKE_TARGETS="$win" PATH="$tmp/mingw:$PATH" pushed "a crate's change runs Windows clippy for it" \
    "$skip$nl$(winclippy clauseo-core)" 0 one:refs/heads/one
  FAKE_TARGETS="$win" PATH="$tmp/mingw:$PATH" FAKE_CLIPPY_EXIT=4 pushed "a failed Windows clippy stops the push" \
    "$skip$nl$(winclippy clauseo-core)" 1 one:refs/heads/one-b
  git -C "$tmp/remote.git" rev-parse -q --verify refs/heads/one-b > /dev/null && fail "lefthook: a failed Windows clippy stops the push: the branch reached the remote"
  g checkout -q main
fi

# ---- CI never skips the ui steps: no workflow passes the flag.
if grep -rlF -- "--skip-ui" "$root/.github/workflows"; then
  fail "a workflow passes --skip-ui; CI must run the ui steps"
else
  echo "ok: no workflow passes --skip-ui"
fi

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "pre-push-test: all cases pass"
