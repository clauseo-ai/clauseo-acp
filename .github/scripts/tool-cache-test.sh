#!/usr/bin/env bash
# Tests install-pinned-tools' cache (tool-cache.sh) with a fake cache folder, fake tools and a
# fake curl: a miss, the fill on main, a restore on a fresh runner, and each way a tool can be
# wrong (an asset or a binary that is not its pin, even with a SHA256SUMS that matches it, a wrong
# version, no pin for the platform, a failed download), which must fall back to the download path
# and must not be saved. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/tool-cache-test.sh

# ok never fails, so `test && ok || fail` is if-then-else here.
# shellcheck disable=SC2015
set -euo pipefail
cd "$(dirname "$0")/../.."
export GITHUB_WORKSPACE=$PWD
script=$PWD/.github/actions/install-pinned-tools/tool-cache.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

sha() { if command -v sha256sum > /dev/null; then sha256sum < "$1"; else shasum -a 256 < "$1"; fi | awk '{ print $1 }'; }

# The release server: a gitleaks .tar.gz with the binary at its top level, a bare lefthook.
mkdir -p "$tmp/server/pack"
printf '#!/usr/bin/env bash\necho 8.0.0\n' > "$tmp/server/pack/gitleaks"
chmod +x "$tmp/server/pack/gitleaks"
tar -czf "$tmp/server/gitleaks_8.0.0_linux_x64.tar.gz" -C "$tmp/server/pack" gitleaks
printf '#!/usr/bin/env bash\necho 2.0.0\n' > "$tmp/server/lefthook_2.0.0_Linux_x86_64"
# curl -sSfL ... -o <file> <url>: copies the URL's file name from the server; FAKE_CURL_FAIL fails.
cat > "$tmp/curl" <<EOF
#!/usr/bin/env bash
[ -z "\${FAKE_CURL_FAIL:-}" ] || exit 22
while [ "\$#" -gt 2 ]; do [ "\$1" = -o ] && out=\$2; shift; done
cp "$tmp/server/\$(basename "\$2")" "\$out"
EOF
chmod +x "$tmp/curl"

# Cargo tools as install-action leaves them: `cargo-nextest nextest --version`, `typos --version`.
mkdir -p "$tmp/installed"
printf '#!/usr/bin/env bash\necho "cargo-nextest 0.9.1"\n' > "$tmp/installed/cargo-nextest"
printf '#!/usr/bin/env bash\necho "typos-cli 1.2.3"\n' > "$tmp/installed/typos"
chmod +x "$tmp/installed/"*
# install-action installs them into CARGO_HOME/bin (CARGO_BIN here).
install_cargo_tools() { mkdir -p "$CARGO_BIN"; cp "$tmp/installed/"* "$CARGO_BIN/"; }

# The pins, as `cargo xtask setup --list=json` prints them, and the action's lists. The cargo
# tools' binaries are pinned on linux-x86_64 only.
export RUNNER_TEMP=$tmp/runner RUNNER_OS=Linux RUNNER_ARCH=X64 ImageOS=ubuntu22 CURL=$tmp/curl
mkdir -p "$RUNNER_TEMP"
write_pins() {
  jq -n --arg g "$1" --arg l "$(sha "$tmp/server/lefthook_2.0.0_Linux_x86_64")" \
    --arg n "$(sha "$tmp/installed/cargo-nextest")" --arg t "${2:-$(sha "$tmp/installed/typos")}" '{
    tools: [
      {name: "cargo-nextest", version: "0.9.1", bin_sha256: [{os: "linux", arch: "x86_64", sha256: $n}]},
      {name: "typos-cli", version: "1.2.3", bin_sha256: [{os: "linux", arch: "x86_64", sha256: $t}]}],
    binaries: [
      {name: "gitleaks", version: "8.0.0", os: "linux", arch: "x86_64", sha256: $g,
       asset: "gitleaks_8.0.0_linux_x64.tar.gz", url: "https://example.invalid/gitleaks_8.0.0_linux_x64.tar.gz"},
      {name: "gitleaks", version: "8.0.0", os: "windows", arch: "x86_64", sha256: "00",
       asset: "gitleaks_8.0.0_windows_x64.zip", url: "https://example.invalid/gitleaks_8.0.0_windows_x64.zip"},
      {name: "lefthook", version: "2.0.0", os: "linux", arch: "x86_64", sha256: $l,
       asset: "lefthook_2.0.0_Linux_x86_64", url: "https://example.invalid/lefthook_2.0.0_Linux_x86_64"}]}' \
    > "$RUNNER_TEMP/pins.json"
}
write_pins "$(sha "$tmp/server/gitleaks_8.0.0_linux_x64.tar.gz")"
printf 'cargo-nextest\t0.9.1\ntypos-cli\t1.2.3\n' > "$RUNNER_TEMP/cargo-tools.tsv"
printf 'gitleaks\nlefthook\n' > "$RUNNER_TEMP/binary-names.txt"

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
ok() { echo "ok: $1"; }
# $1 the step, with TOOL_CACHE_DIR and CARGO_BIN from the caller; sets out (stdout) and outputs.
step() {
  : > "$tmp/output"
  out=$(GITHUB_OUTPUT=$tmp/output bash "$script" "$1" 2>&1)
}
output() { sed -n "s/^$1=//p" "$tmp/output" | tail -1; }

export TOOL_CACHE_DIR=$tmp/cache

# 1. The key: its family's form, and a new key for a new version.
step key
key=$(output key)
[[ "$key" =~ ^pinned-tools-v1-ubuntu22-X64-cargo-nextest\+gitleaks\+lefthook\+typos-cli-[0-9a-f]{16}$ ]] \
  && ok "key: $key" || fail "key: $key"
printf 'cargo-nextest\t0.9.2\ntypos-cli\t1.2.3\n' > "$RUNNER_TEMP/cargo-tools.tsv"
step key
[ "$(output key)" != "$key" ] && ok "a version bump makes a new key" || fail "same key after a bump"
printf 'cargo-nextest\t0.9.1\ntypos-cli\t1.2.3\n' > "$RUNNER_TEMP/cargo-tools.tsv"
write_pins "$(sha "$tmp/server/gitleaks_8.0.0_linux_x64.tar.gz")" "$(printf '%064d' 1)"
step key
[ "$(output key)" != "$key" ] && ok "a cargo tool pin bump makes a new key" || fail "same key after a pin bump"
write_pins "$(sha "$tmp/server/gitleaks_8.0.0_linux_x64.tar.gz")"

# 2. A miss: nothing placed, the cargo tools left to install-action.
export CARGO_BIN=$tmp/bin-miss
step restore
{ [ "$(output cargo_tools)" = false ] && [ ! -e "$CARGO_BIN/gitleaks" ]; } \
  && ok "a miss places nothing and leaves the cargo tools to install-action" || fail "miss: $out"

# 3. Main after a miss: fetch-assets downloads, checks and places the binaries; fill gathers the
#    cargo tools from CARGO_HOME/bin, not from whatever PATH finds first; the folder is complete.
step fetch-assets
{ [ -x "$CARGO_BIN/gitleaks" ] && [ -x "$CARGO_BIN/lefthook" ] && [ "$("$CARGO_BIN/lefthook")" = 2.0.0 ]; } \
  && ok "fetch-assets places gitleaks (unpacked) and lefthook (bare), executable" || fail "fetch-assets: $out"
install_cargo_tools
mkdir -p "$tmp/decoy"
printf '#!/usr/bin/env bash\necho "typos-cli 1.2.3"; echo decoy\n' > "$tmp/decoy/typos"
chmod +x "$tmp/decoy/typos"
PATH="$tmp/decoy:$PATH" step fill
{ [ "$(output save)" = true ] && [ "$(sha "$TOOL_CACHE_DIR/bin/typos")" = "$(sha "$tmp/installed/typos")" ] \
  && [ -x "$TOOL_CACHE_DIR/bin/cargo-nextest" ]; } \
  && ok "fill copies the pinned cargo tools from CARGO_HOME/bin, not PATH; save=true" || fail "fill: $out"

# 4. A fresh runner restores everything from the folder.
export CARGO_BIN=$tmp/bin-hit
step restore
{ [ "$(output cargo_tools)" = true ] && [ -x "$CARGO_BIN/cargo-nextest" ] && [ -x "$CARGO_BIN/typos" ] \
  && [ -x "$CARGO_BIN/gitleaks" ] && [ "$("$CARGO_BIN/gitleaks")" = 8.0.0 ]; } \
  && ok "a hit puts every tool in CARGO_HOME/bin and skips install-action" || fail "hit: $out"

# 5. An asset whose sha256 is not the pin is removed, not placed; the cargo tools still come.
cp -r "$TOOL_CACHE_DIR" "$tmp/cache-good"
echo tampered >> "$TOOL_CACHE_DIR/assets/lefthook_2.0.0_Linux_x86_64"
export CARGO_BIN=$tmp/bin-bad-asset
step restore
{ [ ! -e "$CARGO_BIN/lefthook" ] && [ ! -e "$TOOL_CACHE_DIR/assets/lefthook_2.0.0_Linux_x86_64" ] \
  && grep -q "not the pinned" <<< "$out" && [ -x "$CARGO_BIN/gitleaks" ] && [ "$(output cargo_tools)" = true ]; } \
  && ok "a bad asset checksum is not placed: lefthook falls back to cargo xtask setup --binaries" \
  || fail "bad asset: $out"

# 6. A planted cargo tool with a SHA256SUMS that matches it, as a cache entry written by code on
#    main could hold: it is not the pin, so none is placed and install-action installs them.
rm -rf "$TOOL_CACHE_DIR"; cp -r "$tmp/cache-good" "$TOOL_CACHE_DIR"
printf '#!/usr/bin/env bash\necho "typos-cli 1.2.3"; echo evil\n' > "$TOOL_CACHE_DIR/bin/typos"
(cd "$TOOL_CACHE_DIR/bin" && for f in cargo-nextest typos; do echo "$(sha "$f")  $f"; done > SHA256SUMS)
export CARGO_BIN=$tmp/bin-bad-tool
step restore
{ [ "$(output cargo_tools)" = false ] && [ ! -e "$CARGO_BIN/typos" ] && [ ! -e "$CARGO_BIN/cargo-nextest" ] \
  && grep -q "not the pinned" <<< "$out"; } \
  && ok "a planted cargo tool with matching SHA256SUMS but not the pin is a miss" || fail "planted tool: $out"

# 6b. No pin for this platform (linux-aarch64 here): the cargo tools are not taken from the cache.
rm -rf "$TOOL_CACHE_DIR"; cp -r "$tmp/cache-good" "$TOOL_CACHE_DIR"
export CARGO_BIN=$tmp/bin-no-pin
RUNNER_ARCH=ARM64 step restore
{ [ "$(output cargo_tools)" = false ] && [ ! -e "$CARGO_BIN/typos" ] && grep -q "no sha256 pinned" <<< "$out"; } \
  && ok "a platform with no cargo tool pin is a miss" || fail "no pin: $out"

# 6c. Before the save: an installed cargo tool that is not its pin is not copied, and fill does
#     not save.
export TOOL_CACHE_DIR=$tmp/cache-unpinned CARGO_BIN=$tmp/bin-unpinned
step fetch-assets
install_cargo_tools
echo "# changed" >> "$CARGO_BIN/typos"
step fill
{ [ "$(output save)" = false ] && [ ! -e "$TOOL_CACHE_DIR/bin/typos" ] && [ -e "$TOOL_CACHE_DIR/bin/cargo-nextest" ] \
  && grep -q "not the pinned" <<< "$out"; } \
  && ok "fill does not save a cargo tool that is not its pin" || fail "fill wrong pin: $out"
export TOOL_CACHE_DIR=$tmp/cache

# 7. A cargo tool that matches its sum but prints another version: install-action again.
rm -rf "$TOOL_CACHE_DIR"; cp -r "$tmp/cache-good" "$TOOL_CACHE_DIR"
printf 'cargo-nextest\t0.9.1\ntypos-cli\t9.9.9\n' > "$RUNNER_TEMP/cargo-tools.tsv"
export CARGO_BIN=$tmp/bin-bad-version
step restore
{ [ "$(output cargo_tools)" = false ] && grep -q "not 9.9.9" <<< "$out"; } \
  && ok "a wrong version falls back to install-action" || fail "bad version: $out"
printf 'cargo-nextest\t0.9.1\ntypos-cli\t1.2.3\n' > "$RUNNER_TEMP/cargo-tools.tsv"

# 8. A failed download, and a download that is not the pinned file: nothing cached, no save.
export TOOL_CACHE_DIR=$tmp/cache-fail CARGO_BIN=$tmp/bin-fail
FAKE_CURL_FAIL=1 step fetch-assets
grep -q "could not download" <<< "$out" && ok "a failed download warns" || fail "failed download: $out"
install_cargo_tools
step fill
[ "$(output save)" = false ] && ok "a folder without every asset is not saved" || fail "fill after failure: $out"
write_pins "0000000000000000000000000000000000000000000000000000000000000000"
export TOOL_CACHE_DIR=$tmp/cache-badpin CARGO_BIN=$tmp/bin-badpin
step fetch-assets
{ [ ! -e "$TOOL_CACHE_DIR/assets/gitleaks_8.0.0_linux_x64.tar.gz" ] && [ ! -e "$CARGO_BIN/gitleaks" ]; } \
  && ok "a download that is not the pinned file is neither cached nor placed" || fail "bad pin: $out"

# 9. Windows: the binaries come from winget, so no asset is cached; the cargo tools are, as .exe.
export RUNNER_OS=Windows TOOL_CACHE_DIR=$tmp/cache-win CARGO_BIN=$tmp/bin-win
step fetch-assets
[ -z "$(ls -A "$TOOL_CACHE_DIR/assets" 2> /dev/null)" ] && ok "Windows caches no binary asset" || fail "windows: $(ls "$TOOL_CACHE_DIR/assets")"

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
