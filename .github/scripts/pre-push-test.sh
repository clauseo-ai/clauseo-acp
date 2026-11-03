#!/usr/bin/env bash
# Tests pre-push.sh, lefthook's pre-push command: a push of deletes only passes without the check,
# and every other push runs `cargo xtask check --quick`, in CI mode unless CI says otherwise. First
# the script alone, fed git's ref lines; then through lefthook, with this repo's lefthook.yml, in a
# scratch repo that pushes to a local bare remote, so the test also fails when lefthook stops
# passing git's stdin on. A fake `cargo` first on PATH records each call instead of checking.
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
exit "${FAKE_CARGO_EXIT:-0}"
EOF
chmod +x "$tmp/bin/cargo"
export PATH="$tmp/bin:$PATH" CALLS="$tmp/calls"

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
check="CI=1 xtask check --quick"

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
CI=0 alone "CI=0 runs the check in local mode" "refs/heads/x $a refs/heads/x $b"$'\n' "CI=0 xtask check --quick" 0
FAKE_CARGO_EXIT=3 alone "a failed check fails the push" "refs/heads/x $a refs/heads/x $b"$'\n' "$check" 3
FAKE_CARGO_EXIT=3 alone "a delete passes whatever the check would say" "(delete) $z40 refs/heads/x $a"$'\n' "" 0

# ---- through lefthook and git push, in a scratch repo with this lefthook.yml and pre-push.sh.
if ! command -v lefthook > /dev/null; then
  fail "lefthook is not on PATH (cargo xtask setup installs it)"
else
  repo="$tmp/repo"
  git init -q --bare "$tmp/remote.git"
  git init -q -b main "$repo"
  mkdir -p "$repo/.github/scripts"
  cp "$root/lefthook.yml" "$repo/"
  cp "$root/.github/scripts/pre-push.sh" "$repo/.github/scripts/"
  g() { git -C "$repo" -c user.name=test -c user.email=test@example.com -c commit.gpgsign=false "$@"; }
  g add -A
  g commit -q --no-verify -m init
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
  pushed "a new branch runs the check" "$check" 0 main:refs/heads/d
  pushed "a delete and a new branch run the check" "$check" 0 :d main:refs/heads/e
  FAKE_CARGO_EXIT=3 pushed "a failed check stops the push" "$check" 1 main:refs/heads/f
  git -C "$tmp/remote.git" rev-parse -q --verify refs/heads/f > /dev/null && fail "lefthook: a failed check stops the push: the branch reached the remote"
fi

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "pre-push-test: all cases pass"
