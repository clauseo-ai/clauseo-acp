#!/usr/bin/env bash
# Tests local-mac-dmg.sh without a Mac: its --plan output (the steps, the commands, the values it
# reads from release.yml and the pin files), a copy run from outside the repo, and the guards that
# stop it before any step runs: not a checkout, not macOS arm64 (a fake uname), a dirty tree, a
# missing prerequisite. Nothing is built or downloaded.
# Run it from anywhere: bash .github/scripts/local-mac-dmg-test.sh

set -euo pipefail
cd "$(dirname "$0")/../.."
root=$PWD
script=$root/.github/scripts/local-mac-dmg.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { echo "FAIL: $*"; fail=1; }
has() { grep -qF -e "$2" <<< "$1" || bad "$3: no \"$2\" in the output"; }

# --plan in this checkout: every step numbered in order, the CI commands, the read values.
sha9=$(git rev-parse --short=9 HEAD)
version=$(jq -r .version crates/desktop/tauri.conf.json)
plan=$(bash "$script" --plan) || bad "--plan exited non-zero"
steps=$(sed -nE 's/^== step ([0-9]+): .*/\1/p' <<< "$plan" | tr '\n' ' ')
[ "$steps" = "$(seq -s ' ' 1 15) " ] || bad "--plan's steps are numbered $steps"
has "$plan" "node .github/scripts/sidecars.mjs --safe-workspace=false aarch64-apple-darwin" plan
has "$plan" "node .github/scripts/sidecars.mjs --check aarch64-apple-darwin" plan
has "$plan" "pnpm -C ui install --frozen-lockfile" plan
has "$plan" "vite build --sourcemap hidden" plan
has "$plan" "CLAUSEO_VERSION_SUFFIX=dev.$sha9" plan
has "$plan" "--no-bundle --config release/release.conf.json --config release/sidecars.generated.json -- --locked" plan
# The block-list switch off, as release.yml's check output bundle_blocklist is for launch: no
# snapshot download, no blocklist.conf.json in a build (the overlay check still names the file).
has "$plan" "BUNDLE_BLOCKLIST=false" plan
has "$plan" "skipped: bundle_blocklist is false in release.yml" plan
grep -q 'fetch_blocklist$' <<< "$plan" && bad "--plan runs fetch_blocklist with the switch off"
grep -E 'tauri (build|bundle)' <<< "$plan" | grep -q blocklist && bad "--plan passes blocklist.conf.json with the switch off"
has "$plan" "export APPLE_SIGNING_IDENTITY=-" plan
has "$plan" 'codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime "$helper"' plan
has "$plan" "codesign --verify --deep --strict" plan
has "$plan" "tauri signer generate --ci" plan
has "$plan" "Clauseo_${version}_aarch64-$sha9.dmg" plan
has "$plan" "RUSTFLAGS='$(grep -E '\{ target: aarch64-apple-darwin,' .github/workflows/release.yml |
  sed -nE 's/.*rustflags: "([^"]*)".*/\1/p')'" plan
has "$plan" "$root/target/local-mac-dmg/Clauseo_" "the default --out"
grep -q CLAUSEO_VERSION_SUFFIX ui/app/vite.config.ts && reads="reads" || reads="does not read"
has "$plan" "ui/app/vite.config.ts $reads CLAUSEO_VERSION_SUFFIX" plan
has "$(bash "$script" --plan --out "$tmp/o")" "DMG: $tmp/o/Clauseo_" "--out"

# It never publishes, notarises or pushes.
if grep -nE 'gh release (create|upload|edit|delete)|git push|notarytool|stapler|altool|--no-verify' "$script"; then
  bad "the script holds a publish, notarise or push command"
fi

# A copy outside the repo, run from inside the checkout, plans the same.
cp "$script" "$tmp/copy.sh"
[ "$(bash "$tmp/copy.sh" --plan)" = "$plan" ] || bad "a copy outside the repo plans differently"

# Not a checkout: refused, before any step.
mkdir "$tmp/empty"
out=$(cd "$tmp/empty" && bash "$script" --plan 2>&1) && bad "outside git: exit 0"
has "$out" "is not in a git checkout" "outside git"
git init -q "$tmp/other"
out=$(cd "$tmp/other" && bash "$tmp/copy.sh" --plan 2>&1) && bad "another repo: exit 0"
has "$out" "is not a clauseo-acp checkout (no .github/workflows/release.yml)" "another repo"
grep -q '^== step' <<< "$out" && bad "another repo: a step ran"

out=$(bash "$script" --bogus 2>&1) && bad "--bogus: exit 0"
has "$out" "unknown option --bogus" "--bogus"

# Not macOS arm64: refused, before step 2.
mkdir "$tmp/linux" "$tmp/mac"
printf '#!/bin/sh\n[ "$1" = -m ] && echo x86_64 || echo Linux\n' > "$tmp/linux/uname"
printf '#!/bin/sh\n[ "$1" = -m ] && echo arm64 || echo Darwin\n' > "$tmp/mac/uname"
chmod +x "$tmp/linux/uname" "$tmp/mac/uname"
out=$(PATH="$tmp/linux:$PATH" bash "$script" 2>&1) && bad "Linux: exit 0"
has "$out" "builds only on macOS arm64 (this is Linux x86_64)" Linux
grep -q '^== step 2' <<< "$out" && bad "Linux: step 2 ran"

# A fixture checkout holding the files the guards read, to dirty and to strip of tools.
fx=$tmp/fixture
for f in .github/workflows/release.yml .github/scripts/sidecars.mjs crates/desktop/tauri.conf.json \
  rust-toolchain.toml ui/package.json ui/app/vite.config.ts; do
  mkdir -p "$fx/$(dirname "$f")"
  cp "$f" "$fx/$f"
done
git -C "$fx" init -q
git -C "$fx" add -A
git -C "$fx" -c user.name=t -c user.email=t@t -c core.hooksPath=/dev/null commit -qm fixture
echo dirty > "$fx/untracked"
out=$(cd "$fx" && PATH="$tmp/mac:$PATH" bash "$tmp/copy.sh" 2>&1) && bad "dirty: exit 0"
has "$out" "the tree is dirty" dirty
grep -q '^== step 2' <<< "$out" && bad "dirty: step 2 ran"
rm "$fx/untracked"

# Missing prerequisites: rustup, minisign and codesign are on none of these paths.
mkdir "$tmp/tools"
for t in git jq sed grep awk head sort tr cat mktemp rm dirname; do
  ln -s "$(command -v "$t")" "$tmp/tools/$t"
done
out=$(cd "$fx" && PATH="$tmp/mac:$tmp/tools" "$(command -v bash)" "$tmp/copy.sh" 2>&1) &&
  bad "no tools: exit 0"
has "$out" "missing: rustup -> curl --proto '=https' -sSf https://sh.rustup.rs | sh" "no tools"
grep -q "missing: minisign" <<< "$out" && bad "no tools: minisign asked for with the switch off"
has "$out" "missing: codesign, hdiutil, shasum -> xcode-select --install" "no tools"
has "$out" "install what is missing above" "no tools"
grep -q '^== step 2' <<< "$out" && bad "no tools: step 2 ran"

# The switch on in the fixture's release.yml: the download and the overlay come back, and minisign
# is needed again.
sed -i.bak "s/^      bundle_blocklist: 'false'$/      bundle_blocklist: 'true'/" "$fx/.github/workflows/release.yml"
rm "$fx/.github/workflows/release.yml.bak"
grep -q "^      bundle_blocklist: 'true'$" "$fx/.github/workflows/release.yml" || bad "the fixture's switch did not turn on"
on=$(cd "$fx" && bash "$tmp/copy.sh" --plan 2>&1) || bad "switch on: --plan exited non-zero"
has "$on" "BUNDLE_BLOCKLIST=true" "switch on"
grep -q 'fetch_blocklist$' <<< "$on" || bad "switch on: --plan does not run fetch_blocklist"
has "$on" '--no-bundle --config release/release.conf.json --config release/blocklist.conf.json --config release/sidecars.generated.json -- --locked' "switch on"
out=$(cd "$fx" && PATH="$tmp/mac:$tmp/tools" "$(command -v bash)" "$tmp/copy.sh" --allow-dirty 2>&1) &&
  bad "switch on, no tools: exit 0"
has "$out" "missing: minisign (blocklist step) -> brew install minisign" "switch on, no tools"
sed -i.bak "s/^      bundle_blocklist: 'true'$/      bundle_blocklist: maybe/" "$fx/.github/workflows/release.yml"
rm "$fx/.github/workflows/release.yml.bak"
out=$(cd "$fx" && bash "$tmp/copy.sh" --plan 2>&1) && bad "switch neither true nor false: exit 0"
has "$out" "bundle_blocklist is maybe, not true or false" "switch neither"

[ "$fail" = 0 ] && echo "local-mac-dmg-test: all cases pass"
exit "$fail"
