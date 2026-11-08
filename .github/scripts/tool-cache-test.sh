#!/usr/bin/env bash
# Tests install-pinned-tools' cache (tool-cache.sh) with a fake cache folder, fake tools and a
# fake curl: a miss, the fill on main, a restore on a fresh runner, and each way a tool can be
# wrong (an asset or a binary that is not its pin, even with a SHA256SUMS that matches it, a wrong
# version, no pin for the platform, a failed download), which must fall back to the download path
# and must not be saved; a tool that is not its pin in fill fails the job. Also that the pins the
# steps after the restore read are written after it, so a crafted cache entry cannot replace them.
# fast-lane runs it.
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

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
ok() { echo "ok: $1"; }
# $1 the step, with TOOL_CACHE_DIR, CARGO_BIN and PINS_DIR from the caller; sets out (stdout and
# stderr), rc (its exit code) and outputs.
step() {
  : > "$tmp/output"
  rc=0
  out=$(GITHUB_OUTPUT=$tmp/output bash "$script" "$1" 2>&1) || rc=$?
}
output() { sed -n "s/^$1=//p" "$tmp/output" | tail -1; }

# The pins, as `cargo xtask setup --list=json` prints them (PINS_JSON, a step output in the
# action), written by `tool-cache.sh pins` into a new PINS_DIR, as the action's pins-after step
# does. The cargo tools' binaries are pinned on linux-x86_64 only. $1 the gitleaks asset pin; $2
# typos' pin, $3 typos' version and $4 cargo-nextest's version, each with a default.
export RUNNER_TEMP=$tmp/runner RUNNER_OS=Linux RUNNER_ARCH=X64 ImageOS=ubuntu22 CURL=$tmp/curl NAMES=""
mkdir -p "$RUNNER_TEMP"
write_pins() {
  PINS_JSON=$(jq -cn --arg g "$1" --arg l "$(sha "$tmp/server/lefthook_2.0.0_Linux_x86_64")" \
    --arg n "$(sha "$tmp/installed/cargo-nextest")" --arg t "${2:-$(sha "$tmp/installed/typos")}" \
    --arg tv "${3:-1.2.3}" --arg nv "${4:-0.9.1}" '{
    tools: [
      {name: "cargo-nextest", version: $nv, bin_sha256: [{os: "linux", arch: "x86_64", sha256: $n}]},
      {name: "typos-cli", version: $tv, bin_sha256: [{os: "linux", arch: "x86_64", sha256: $t}]}],
    binaries: [
      {name: "gitleaks", version: "8.0.0", os: "linux", arch: "x86_64", sha256: $g,
       asset: "gitleaks_8.0.0_linux_x64.tar.gz", url: "https://example.invalid/gitleaks_8.0.0_linux_x64.tar.gz"},
      {name: "gitleaks", version: "8.0.0", os: "windows", arch: "x86_64", sha256: "00",
       asset: "gitleaks_8.0.0_windows_x64.zip", url: "https://example.invalid/gitleaks_8.0.0_windows_x64.zip"},
      {name: "lefthook", version: "2.0.0", os: "linux", arch: "x86_64", sha256: $l,
       asset: "lefthook_2.0.0_Linux_x86_64", url: "https://example.invalid/lefthook_2.0.0_Linux_x86_64"}]}')
  export PINS_JSON
  step pins
  [ "$rc" -eq 0 ] || { echo "pins failed: $out"; exit 1; }
  PINS_DIR=$(output dir)
  export PINS_DIR
}
gitleaks_pin=$(sha "$tmp/server/gitleaks_8.0.0_linux_x64.tar.gz")
write_pins "$gitleaks_pin"

# 0. pins: the lists the action reads, in a new folder each time.
first=$PINS_DIR
{ [ "$(cat "$PINS_DIR/cargo-tools.tsv")" = "$(printf 'cargo-nextest\t0.9.1\ntypos-cli\t1.2.3')" ] \
  && [ "$(cat "$PINS_DIR/binary-names.txt")" = "$(printf 'gitleaks\nlefthook')" ] \
  && [ "$(output spec)" = "cargo-nextest@0.9.1,typos-cli@1.2.3" ] \
  && [ "$(output binaries)" = "--only gitleaks --only lefthook " ]; } \
  && ok "pins writes the tool lists, the spec and the --only flags" || fail "pins: $out $(output spec)"
NAMES="typos-cli lefthook" step pins
{ [ "$(cat "$(output dir)/cargo-tools.tsv")" = "$(printf 'typos-cli\t1.2.3')" ] && [ "$(output dir)" != "$first" ]; } \
  && ok "pins narrows to NAMES, in a new folder" || fail "pins NAMES: $out"

# 0b. A crafted cache entry that unpacks files over the pins written for the key (and over
#     RUNNER_TEMP/pins.json, where they used to live) does not win: the pins after the restore are
#     a new folder written from PINS_JSON.
cp -r "$PINS_DIR" "$tmp/pins-key"
crafted='{"tools":[{"name":"cargo-nextest","version":"0.9.1","bin_sha256":[{"os":"linux","arch":"x86_64","sha256":"evil"}]}],"binaries":[]}'
echo "$crafted" > "$tmp/pins-key/pins.json"
echo "$crafted" > "$RUNNER_TEMP/pins.json"
printf 'cargo-nextest\t0.9.1\n' > "$RUNNER_TEMP/cargo-tools.tsv"
step pins
after=$(output dir)
{ [ "$after" != "$tmp/pins-key" ] && [ "$(jq -c . "$after/pins.json")" = "$PINS_JSON" ] \
  && [ "$(wc -l < "$after/cargo-tools.tsv")" -eq 2 ]; } \
  && ok "pins written after the restore ignore files the restore dropped" || fail "crafted pins won: $out"
rm -f "$RUNNER_TEMP/pins.json" "$RUNNER_TEMP/cargo-tools.tsv"

# 0c. The action: pins-after runs after the restore, every step after the restore reads PINS_DIR
#     from pins-after only, none reads RUNNER_TEMP, and every script runs from the checkout.
# The Ruby is in single quotes on purpose: $GITHUB_WORKSPACE in it is text to look for.
# shellcheck disable=SC2016
if ruby -ryaml -e '
  steps = YAML.load_file(ARGV[0])["runs"]["steps"]
  ids = steps.map { |s| s["id"] }
  restore = steps.index { |s| s["uses"].to_s.start_with?("actions/cache/restore@") }
  after = ids.index("pins-after")
  bad = []
  bad << "pins-after (#{after.inspect}) is not after the restore (#{restore})" unless after && after > restore
  steps.each_with_index do |s, i|
    text = s.to_yaml
    bad << "step #{i} reads RUNNER_TEMP" if text.include?("RUNNER_TEMP")
    text.scan(/tool-cache.sh/) { bad << "step #{i} runs tool-cache.sh from elsewhere" unless text.include?("\"$GITHUB_WORKSPACE/.github/actions/install-pinned-tools/tool-cache.sh\"") }
    next unless i > restore
    # Only runner-held outputs from before the restore: the pins JSON, the key, its folder and
    # whether the restore hit.
    used = text.scan(/steps\.([\w-]+)\.outputs\.([\w-]+)/).map { |id, o| "#{id}.#{o}" }
    early = used.select { |u| ids[0..restore].include?(u.split(".").first) }
    bad += (early - %w[pins.json cache-key.key cache-key.dir cache.cache-hit]).map { |u| "step #{i} reads #{u}, from before the restore" }
  end
  abort bad.join("\n") unless bad.empty?' "$GITHUB_WORKSPACE/.github/actions/install-pinned-tools/action.yml" 2> "$tmp/order"; then
  ok "action.yml writes the pins again after the restore and reads only those afterwards"
else
  fail "action.yml order: $(cat "$tmp/order")"
fi

export TOOL_CACHE_DIR=$tmp/cache

# 1. The key: its family's form, and a new key for a new version.
step key
key=$(output key)
[[ "$key" =~ ^pinned-tools-v1-ubuntu22-X64-cargo-nextest\+gitleaks\+lefthook\+typos-cli-[0-9a-f]{16}$ ]] \
  && ok "key: $key" || fail "key: $key"
write_pins "$gitleaks_pin" "" "" 0.9.2
step key
[ "$(output key)" != "$key" ] && ok "a version bump makes a new key" || fail "same key after a bump"
write_pins "$gitleaks_pin" "$(printf '%064d' 1)"
step key
[ "$(output key)" != "$key" ] && ok "a cargo tool pin bump makes a new key" || fail "same key after a pin bump"
write_pins "$gitleaks_pin"

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
{ [ "$(output save)" = true ] && [ "$rc" -eq 0 ] && [ "$(sha "$TOOL_CACHE_DIR/bin/typos")" = "$(sha "$tmp/installed/typos")" ] \
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
  && [ "$rc" -ne 0 ] && grep -q "^::error title=pinned tools cache::.*not the pinned" <<< "$out"; } \
  && ok "fill on main fails the job on a cargo tool that is not its pin, and does not save it" \
  || fail "fill wrong pin (rc $rc): $out"
export TOOL_CACHE_DIR=$tmp/cache

# 7. A cargo tool that matches its sum but prints another version: install-action again.
rm -rf "$TOOL_CACHE_DIR"; cp -r "$tmp/cache-good" "$TOOL_CACHE_DIR"
write_pins "$gitleaks_pin" "" 9.9.9
export CARGO_BIN=$tmp/bin-bad-version
step restore
{ [ "$(output cargo_tools)" = false ] && grep -q "not 9.9.9" <<< "$out"; } \
  && ok "a wrong version falls back to install-action" || fail "bad version: $out"
write_pins "$gitleaks_pin"

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
