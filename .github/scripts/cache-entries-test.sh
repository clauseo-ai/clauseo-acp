#!/usr/bin/env bash
# Tests cache-entries.sh prune against fixtures/actions-caches-2026-11-08.json, main's real cache
# list on 2026-11-08 (`gh api repos/clauseo-ai/clauseo-acp/actions/caches?per_page=100`), in
# dry-run mode: it keeps every current entry, and deletes only what is off main, superseded, unused
# for its family's idle days, or of no family and unused for 3 days. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/cache-entries-test.sh

# ok never fails, so `test && ok || fail` is if-then-else here.
# shellcheck disable=SC2015
set -euo pipefail
cd "$(dirname "$0")/../.."
export GITHUB_WORKSPACE=$PWD
script=.github/scripts/cache-entries.sh
real=.github/scripts/fixtures/actions-caches-2026-11-08.json
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export CACHE_DRY_RUN=1
# Just after the list was taken (its newest access is 08:50Z).
export CACHE_NOW=2026-11-08T09:00:00Z

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
ok() { echo "ok: $1"; }

# $1 list file, then the arguments; sets out and status.
run() {
  local list=$1
  shift
  status=0
  out=$(CACHE_LIST_FILE=$list bash "$script" "$@" 2>&1) || status=$?
}
# The keys a dry run would delete, sorted.
deleted() { sed -n 's/^would delete: \([^ ]*\) .*/\1/p' <<< "$out" | sort; }

# 1. Today's list: prune deletes nothing.
run "$real" prune
if [ "$status" = 0 ] && [ -z "$(deleted)" ] && grep -q "nothing to delete" <<< "$out"; then
  ok "prune keeps all $(jq '.actions_caches | length' "$real") entries of today's list"
else
  fail "prune on today's list: exit $status, $out"
fi

# 2. Each of today's keys matches exactly one family.
families=$(awk '!/^[[:space:]]*(#|$)/ { print $2 }' .github/scripts/cache-families.txt)
while read -r key; do
  n=0
  while read -r re; do
    if jq -e -n --arg k "$key" --arg re "$re" '$k | test($re)' > /dev/null; then n=$((n + 1)); fi
  done <<< "$families"
  [ "$n" = 1 ] || fail "$key matches $n families, not 1"
done < <(jq -r '.actions_caches[].key' "$real")
ok "each key of today's list matches exactly one family"

# 3. Today's list plus entries: a branch's and a pull request's; superseded entries of three
#    families; two of no family (one used yesterday, one unused since 11-01); two older .deb image
#    versions (one used yesterday, one unused since 11-03).
entry() { # id ref key size created accessed
  jq -n --argjson id "$1" --arg ref "$2" --arg key "$3" --argjson size "$4" --arg c "$5" --arg a "$6" \
    '{id: $id, ref: $ref, key: $key, size_in_bytes: $size, created_at: $c, last_accessed_at: $a}'
}
{
  entry 1 refs/heads/ci-arm64 node-cache-Linux-x64-pnpm-3635f306cef28399a09af42fa6c70d825d525f15437c7a04622511f617948138 176000000 2026-11-07T09:00:00Z 2026-11-07T09:00:00Z
  entry 2 refs/pull/725/merge v1-rust-x86_64-pc-windows-msvc-build-Windows_NT-x64-9229ae9f-5fc7e12a 3000000000 2026-11-07T09:00:00Z 2026-11-07T09:00:00Z
  entry 3 refs/heads/main v1-rust-x86_64-pc-windows-msvc-build-Windows_NT-x64-9229ae9f-5fc7e12a 2965057259 2026-11-06T11:46:00Z 2026-11-07T12:00:00Z
  entry 4 refs/heads/main v1-rust-aarch64-apple-darwin-build-Darwin-arm64-50dbc163-5fc7e12a 1901841900 2026-11-06T11:31:00Z 2026-11-07T11:20:00Z
  entry 5 refs/heads/main v1-rust-aarch64-apple-darwin-build-Darwin-arm64-50dbc163-e57d2756 1997225364 2026-11-07T11:41:00Z 2026-11-07T11:41:00Z
  entry 6 refs/heads/main node-cache-Linux-x64-pnpm-32c357254a2030406397c34d47a264987ac9814093bc00c5ff626faa07370fb2 101000000 2026-11-04T11:00:00Z 2026-11-06T10:00:00Z
  entry 7 refs/heads/main v0-rust-new-job-Linux-x64-11111111-22222222 500000000 2026-11-07T10:00:00Z 2026-11-07T10:00:00Z
  entry 8 refs/heads/main v0-rust-retired-job-Linux-x64-11111111-22222222 400000000 2026-11-01T10:00:00Z 2026-11-01T10:00:00Z
  entry 9 refs/heads/main tauri-linux-debs-X64-ubuntu22-20261025.300.1-libwebkit2gtk-4.1-dev+ripgrep 53000000 2026-10-26T10:00:00Z 2026-11-07T10:00:00Z
  entry 10 refs/heads/main tauri-linux-debs-X64-ubuntu22-20261018.200.1-libwebkit2gtk-4.1-dev+ripgrep 53000000 2026-10-19T10:00:00Z 2026-11-03T10:00:00Z
} | jq -s --slurpfile real "$real" '$real[0].actions_caches + .' > "$tmp/more.json"
run "$tmp/more.json" prune
want=$(printf '%s\n' \
  node-cache-Linux-x64-pnpm-3635f306cef28399a09af42fa6c70d825d525f15437c7a04622511f617948138 \
  node-cache-Linux-x64-pnpm-32c357254a2030406397c34d47a264987ac9814093bc00c5ff626faa07370fb2 \
  tauri-linux-debs-X64-ubuntu22-20261018.200.1-libwebkit2gtk-4.1-dev+ripgrep \
  v0-rust-retired-job-Linux-x64-11111111-22222222 \
  v1-rust-aarch64-apple-darwin-build-Darwin-arm64-50dbc163-5fc7e12a \
  v1-rust-x86_64-pc-windows-msvc-build-Windows_NT-x64-9229ae9f-5fc7e12a \
  v1-rust-x86_64-pc-windows-msvc-build-Windows_NT-x64-9229ae9f-5fc7e12a | sort)
if [ "$status" = 0 ] && [ "$(deleted)" = "$want" ] && grep -q "deleted 7 of 7" <<< "$out"; then
  ok "prune deletes 2 off main, 3 superseded, 1 unknown unused for 7 days, 1 .deb image unused for 5 days; keeps today's 13, the newest macOS, the unknown used yesterday and the older .deb image used yesterday"
else
  fail "prune with extra entries: exit $status, got [$(deleted | tr '\n' ' ')], want [$(tr '\n' ' ' <<< "$want")]"
fi
grep -q "kept for now: v0-rust-new-job-Linux-x64-11111111-22222222 matches no family" <<< "$out" \
  && ok "an unknown entry used within 3 days is kept and logged" || fail "unknown kept log: $out"
grep -q "superseded by v1-rust-aarch64-apple-darwin-build-Darwin-arm64-50dbc163-e57d2756" <<< "$out" \
  && ok "prune logs what superseded an entry" || fail "prune's reason for the old macOS entry: $out"
grep -q "(refs/heads/main, 2827.7 MiB)" <<< "$out" && ok "prune logs each size in MiB" || fail "sizes: $out"

# 4. Four days on with no run: the .deb entries, unused for 3 days, go; every other family's
#    newest stays, however long unused.
CACHE_NOW=2026-11-12T09:00:00Z run "$real" prune
want=$(jq -r '.actions_caches[].key | select(startswith("tauri-linux-debs-"))' "$real" | sort)
[ "$(deleted)" = "$want" ] && ok "after 4 idle days only the 5 .deb entries go" || fail "idle: got [$(deleted | tr '\n' ' ')]"

# 5. A workflow's own prune (--families) touches its families on main only: not the branch or pull
#    request entries, not the unknown one, not other families.
run "$tmp/more.json" prune --families rust-e2e-x86_64-pc-windows-msvc,node-cache
[ "$(deleted)" = node-cache-Linux-x64-pnpm-32c357254a2030406397c34d47a264987ac9814093bc00c5ff626faa07370fb2 ] \
  && ok "prune --families deletes only its own families' superseded main entries" || fail "--families: $out"

# 6. The list cannot be read: a warning, exit 0, nothing deleted.
run "$tmp/missing.json" prune
{ [ "$status" = 0 ] && grep -q "could not list" <<< "$out"; } && ok "an unreadable list warns and exits 0" || fail "unreadable list: exit $status, $out"

# 7. A bad argument: exit 2, nothing deleted.
run "$real" delete-family --key x
{ [ "$status" = 2 ] && [ -z "$(deleted)" ]; } && ok "an unknown command deletes nothing" || fail "usage: exit $status"

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
