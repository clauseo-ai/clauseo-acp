#!/usr/bin/env bash
# The cache of install-pinned-tools (action.yml beside this script): the pinned tools, restored
# from main's Actions cache before anything is downloaded, and checked against their checksums.
#
#   tool-cache.sh pins           writes the pins (PINS_JSON, NAMES) into a new folder: pins.json,
#                                cargo-tools.tsv and binary-names.txt; its path (dir), the
#                                install-action spec and the --only flags, to $GITHUB_OUTPUT
#   tool-cache.sh key            the cache key and folder, to $GITHUB_OUTPUT
#   tool-cache.sh restore        checks and places what the restore brought; cargo_tools=true when
#                                every cargo tool came from it, to $GITHUB_OUTPUT
#   tool-cache.sh fetch-assets   on main after a miss: downloads each binary's release asset into
#                                the folder, checks its pinned sha256 and places it
#   tool-cache.sh fill           on main after a miss, once the tools are installed and checked:
#                                copies each cargo tool in from CARGO_HOME/bin when its sha256 is
#                                the pin; save=true when the folder holds every tool, to
#                                $GITHUB_OUTPUT. A tool that is not its pin fails the step (exit 1):
#                                fill runs only on main, where a stale pin must show at once
#
# The folder holds two things:
# - assets/: the binaries' release assets (gitleaks, lefthook), as downloaded. The sha256 of each is
#   pinned in xtask/src/setup.rs (`setup --list=json`), so a restored asset is checked against the
#   same pin a download is, then unpacked into CARGO_HOME/bin as `cargo xtask setup --binaries`
#   does (a .tar.gz holds the binary at its top level; any other asset is the binary). Windows
#   installs these with winget, so no asset is cached there.
# - bin/: the cargo tools taiki-e/install-action installed. The sha256 of each binary, as
#   install-action unpacks it, is pinned per OS and arch in xtask/src/setup.rs (`bin_sha256` in
#   `setup --list=json`). A tool is checked against that pin twice: in `fill`, before it goes into
#   the folder that is saved, and in `restore`, before it is placed. A restored tool must match and
#   then print its pinned version, or install-action installs every cargo tool again. With no pin
#   for this OS and arch, a tool is never cached.
# A mismatch or a miss leaves the download path as it was: install-action, then
# `cargo xtask setup --binaries`, each with its own checksum check.
#
# The key: pinned-tools-v1-<ImageOS>-<arch>-<tool names joined by +>-<16 hex digits>, the digits a
# sha256 of the versions, this OS's pinned asset and binary sha256s, and this script and action.yml
# (whose install-action commit pins install-action's manifests). A pin bump makes a new key.
#
# The restore: actions/cache unpacks with `tar -P` and no path filter, so a crafted entry could
# write files outside its folder. So nothing the steps after it read is written before it: the
# action runs `pins` once for the key and again after the restore, into a new folder, from
# PINS_JSON, a step output (held by the runner, not in a file); restore, fetch-assets and fill read
# only that second folder. This script runs from the checkout, never from a copy.
#
# Environment: PINS_DIR (the folder `pins` wrote; every command but `pins` reads it), PINS_JSON
# and NAMES (for `pins`), RUNNER_TEMP, RUNNER_OS, RUNNER_ARCH, ImageOS, GITHUB_OUTPUT, CARGO_HOME.
# GH_TOKEN or GITHUB_TOKEN, when set, authenticates fetch-assets' downloads through gh (never
# printed). For the tests: CARGO_BIN (where tools go), TOOL_CACHE_DIR (the folder), CURL and GH
# (the download commands), SLEEP (the wait between gh attempts).
set -euo pipefail

here=${GITHUB_WORKSPACE:?}/.github/actions/install-pinned-tools
pins=${PINS_DIR:-/nonexistent}/pins.json
cargo_tools=${PINS_DIR:-/nonexistent}/cargo-tools.tsv
binary_names=${PINS_DIR:-/nonexistent}/binary-names.txt
dir=${TOOL_CACHE_DIR:-$RUNNER_TEMP/pinned-tools-cache}
output=${GITHUB_OUTPUT:-/dev/null}

warn() { echo "::warning title=pinned tools cache::$*"; }

unix_path() {
  if command -v cygpath > /dev/null; then cygpath -u "$1"; else printf '%s\n' "$1"; fi
}
bin=$(unix_path "${CARGO_BIN:-${CARGO_HOME:-$HOME/.cargo}/bin}")

case "$RUNNER_OS" in
  Linux) os=linux; exe="" ;;
  macOS) os=macos; exe="" ;;
  Windows) os=windows; exe=.exe ;;
  *) os=unknown; exe="" ;;
esac
case "$RUNNER_ARCH" in
  X64) arch=x86_64 ;;
  ARM64) arch=aarch64 ;;
  *) arch=unknown ;;
esac

sha256() {
  if command -v sha256sum > /dev/null; then
    sha256sum < "$1" | awk '{ print $1 }'
  else
    shasum -a 256 < "$1" | awk '{ print $1 }'
  fi
}

# The command a cargo tool runs as: cargo-<x> is a file of that name, <x>-cli is <x>.
tool_file() {
  case "$1" in
    cargo-*) echo "$1$exe" ;;
    *) echo "${1%-cli}$exe" ;;
  esac
}

# The version a tool prints, as the action's version check reads it.
tool_version() {
  case "$1" in
    cargo-*) "$bin/$1$exe" "${1#cargo-}" --version 2> /dev/null | awk 'NR == 1 { print $2 }' ;;
    *) "$bin/${1%-cli}$exe" --version 2> /dev/null | awk 'NR == 1 { print $2 }' ;;
  esac
}

# The binaries pinned for this OS and arch, as "name<TAB>asset<TAB>sha256<TAB>url"; none on
# Windows, which installs them with winget.
binary_pins() {
  [ "$os" != windows ] || return 0
  [ -s "$binary_names" ] || return 0
  jq -r --arg os "$os" --arg arch "$arch" --rawfile names "$binary_names" '
    ($names | split("\n") | map(select(. != ""))) as $want
    | .binaries[] | select(.os == $os and .arch == $arch and (.name as $n | $want | index($n)))
    | [.name, .asset, .sha256, .url] | @tsv' "$pins" | tr -d '\r'
}

# The pinned sha256 of cargo tool $1's binary on this OS and arch; empty when none is pinned.
tool_pin() {
  jq -r --arg n "$1" --arg os "$os" --arg arch "$arch" '
    .tools[] | select(.name == $n) | (.bin_sha256 // [])[]
    | select(.os == $os and .arch == $arch) | .sha256' "$pins" | tr -d '\r'
}

# Unpacks a checked asset into $bin as `cargo xtask setup --binaries` does; returns 1 on failure.
place_asset() {
  local name=$1 asset=$2 work
  work=$(mktemp -d)
  case "$asset" in
    *.tar.gz)
      if ! tar -xzf "$dir/assets/$asset" -C "$work" "$name" 2> /dev/null \
        || [ -L "$work/$name" ] || [ ! -f "$work/$name" ]; then
        rm -rf "$work"
        return 1
      fi
      ;;
    *) cp "$dir/assets/$asset" "$work/$name" ;;
  esac
  mkdir -p "$bin"
  chmod 755 "$work/$name"
  mv -f "$work/$name" "$bin/$name"
  rm -rf "$work"
}

# jq on Windows ends its lines with CRLF, so each of its outputs drops the CR.
cmd_pins() {
  local new names spec binaries
  new=$(mktemp -d "${RUNNER_TEMP:?}/pinned-tools-pins.XXXXXX")
  printf '%s\n' "${PINS_JSON:?}" > "$new/pins.json"
  names=$(jq -cn --arg s "${NAMES:-}" '$s | split(" ") | map(select(. != ""))' | tr -d '\r')
  jq -r --argjson want "$names" \
    '.tools[] | select(($want | length) == 0 or (.name as $n | $want | index($n)))
     | "\(.name)\t\(.version)"' "$new/pins.json" | tr -d '\r' > "$new/cargo-tools.tsv"
  jq -r --argjson want "$names" \
    '[.binaries[].name] | unique[] | select(($want | length) == 0 or (. as $n | $want | index($n)))' \
    "$new/pins.json" | tr -d '\r' > "$new/binary-names.txt"
  spec=$(awk -F '\t' '{ printf "%s%s@%s", (NR > 1 ? "," : ""), $1, $2 }' "$new/cargo-tools.tsv")
  binaries=$(sed 's/^/--only /' "$new/binary-names.txt" | tr '\n' ' ')
  {
    echo "dir=$new"
    echo "spec=$spec"
    echo "binaries=$binaries"
  } >> "$output"
  echo "pins in $new; cargo tools: ${spec:-none}; binaries: ${binaries:-none}"
}

cmd_key() {
  local names hash
  names=$( (cut -f1 "$cargo_tools"; cat "$binary_names" 2> /dev/null) | tr -d '\r' | sed '/^$/d' | sort -u | paste -sd+ -)
  hash=$( {
    cat "$cargo_tools"
    while IFS=$'\t' read -r name _version; do
      printf '%s\t%s\n' "$name" "$(tool_pin "$name")"
    done < <(tr -d '\r' < "$cargo_tools")
    binary_pins | cut -f1-3
    cat "$here/action.yml" "$here/tool-cache.sh"
  } | if command -v sha256sum > /dev/null; then sha256sum; else shasum -a 256; fi | cut -c1-16)
  echo "key=pinned-tools-v1-${ImageOS:-unknown}-$RUNNER_ARCH-${names:-none}-$hash" >> "$output"
  echo "dir=$dir" >> "$output"
  echo "key pinned-tools-v1-${ImageOS:-unknown}-$RUNNER_ARCH-${names:-none}-$hash, folder $dir"
}

cmd_restore() {
  # The binaries: each asset whose sha256 is the pin is placed; any other is removed, and
  # `cargo xtask setup --binaries` downloads it.
  while IFS=$'\t' read -r name asset want _url; do
    [ -n "$name" ] || continue
    local file=$dir/assets/$asset
    if [ ! -f "$file" ]; then
      echo "$name: not in the cache"
    elif [ "$(sha256 "$file")" != "$want" ]; then
      warn "$asset from the cache has sha256 $(sha256 "$file"), not the pinned $want; downloading instead"
      rm -f "$file"
    elif place_asset "$name" "$asset"; then
      echo "$name: from the cache, sha256 $want matches the pin"
    else
      warn "$asset from the cache matched its sha256 but holds no regular file named $name; downloading instead"
      rm -f "$file"
    fi
  done < <(binary_pins)

  # The cargo tools: all from the cache, or none. Each must match its pin in xtask/src/setup.rs.
  local all=true
  if [ ! -s "$cargo_tools" ]; then
    echo "cargo_tools=true" >> "$output"
    return 0
  fi
  while IFS=$'\t' read -r name _version; do
    local file want
    file=$(tool_file "$name")
    want=$(tool_pin "$name")
    if [ -z "$want" ]; then
      echo "$name: no sha256 pinned for $os-$arch, so it is not taken from the cache"
      all=false
    elif [ ! -f "$dir/bin/$file" ]; then
      echo "$name: not in the cache"
      all=false
    elif [ "$(sha256 "$dir/bin/$file")" != "$want" ]; then
      warn "$file from the cache has sha256 $(sha256 "$dir/bin/$file"), not the pinned $want; installing it instead"
      all=false
    fi
  done < <(tr -d '\r' < "$cargo_tools")
  if [ "$all" = true ]; then
    mkdir -p "$bin"
    while IFS=$'\t' read -r name version; do
      local file
      file=$(tool_file "$name")
      cp "$dir/bin/$file" "$bin/$file.tmp"
      chmod 755 "$bin/$file.tmp"
      mv -f "$bin/$file.tmp" "$bin/$file"
      if [ "$(tool_version "$name")" != "$version" ]; then
        warn "$name from the cache prints version '$(tool_version "$name")', not $version; installing it instead"
        all=false
      else
        echo "$name $version: from the cache, sha256 matches the pin"
      fi
    done < <(tr -d '\r' < "$cargo_tools")
  fi
  echo "cargo_tools=$all" >> "$output"
}

# Downloads release asset $2 from $1 (https://github.com/<owner>/<repo>/releases/download/<tag>/
# <asset>) to $3; returns 1 on failure. With GH_TOKEN or GITHUB_TOKEN set (the job's read-only
# token, from action.yml) and gh on PATH, through `gh release download`, as xtask's setup does
# (xtask/src/setup/install.rs, Fetcher): github.com's release URLs ignore an Authorization header,
# so only the API is an authenticated download, and GitHub sheds anonymous ones first under load.
# gh reads the token from its environment: it is in no argument and never printed. gh has no
# retry, so it is tried 3 times, 10 s and then 30 s apart. Otherwise anonymous curl, as before.
download() {
  local url=$1 asset=$2 file=$3 path tag repo attempt
  if { [ -n "${GH_TOKEN:-}" ] || [ -n "${GITHUB_TOKEN:-}" ]; } && command -v "${GH:-gh}" > /dev/null; then
    path=${url#https://github.com/}
    repo=$(cut -d/ -f1-2 <<< "$path")
    tag=$(cut -d/ -f5 <<< "$path")
    if [ "$path" = "$url" ] || [ "$(cut -d/ -f3-4 <<< "$path")" != releases/download ] || [ "${path##*/}" != "$asset" ]; then
      warn "$url is not a GitHub release asset URL; downloading it with curl"
    else
      for attempt in 1 2 3; do
        "${GH:-gh}" release download "$tag" --repo "$repo" --pattern "$asset" --output "$file" --clobber && return 0
        [ "$attempt" -lt 3 ] || break
        echo "downloading $asset through gh (attempt $attempt of 3) failed; trying again in $((attempt * 20 - 10)) s"
        "${SLEEP:-sleep}" $((attempt * 20 - 10))
      done
      return 1
    fi
  fi
  ${CURL:-curl} -sSfL --retry 5 --retry-all-errors --connect-timeout 30 --max-time 300 -o "$file" "$url"
}

cmd_fetch_assets() {
  mkdir -p "$dir/assets"
  while IFS=$'\t' read -r name asset want url; do
    [ -n "$name" ] || continue
    local file=$dir/assets/$asset
    [ -f "$file" ] && continue
    if ! download "$url" "$asset" "$file"; then
      warn "could not download $url; cargo xtask setup --binaries tries again"
      rm -f "$file"
    elif [ "$(sha256 "$file")" != "$want" ]; then
      warn "$url has sha256 $(sha256 "$file"), not the pinned $want; not cached"
      rm -f "$file"
    elif place_asset "$name" "$asset"; then
      echo "$name: downloaded $asset, sha256 matches the pin"
    else
      warn "$asset holds no regular file named $name; not cached"
      rm -f "$file"
    fi
  done < <(binary_pins)
}

cmd_fill() {
  local complete=true mismatch=false
  while IFS=$'\t' read -r name _asset want _url; do
    [ -n "$name" ] || continue
    if [ ! -f "$dir/assets/$_asset" ] || [ "$(sha256 "$dir/assets/$_asset")" != "$want" ]; then
      echo "$name: no checked asset in the folder"
      complete=false
    fi
  done < <(binary_pins)
  # Each cargo tool comes from where install-action put it: CARGO_HOME/bin, the folder cargo runs
  # from on every runner (install-action installs next to cargo). Whatever else is on PATH is not
  # looked at. A copy goes in only when its sha256 is the pin.
  if [ -s "$cargo_tools" ]; then
    mkdir -p "$dir/bin"
    while IFS=$'\t' read -r name _version; do
      local file want
      file=$(tool_file "$name")
      want=$(tool_pin "$name")
      rm -f "$dir/bin/$file"
      if [ -z "$want" ]; then
        echo "$name: no sha256 pinned for $os-$arch; not cached"
        complete=false
      elif [ ! -f "$bin/$file" ]; then
        echo "$name: not installed in $bin"
        complete=false
      else
        cp "$bin/$file" "$dir/bin/$file"
        if [ "$(sha256 "$dir/bin/$file")" != "$want" ]; then
          echo "::error title=pinned tools cache::$bin/$file has sha256 $(sha256 "$dir/bin/$file"), not the pinned $want in xtask/src/setup/bin_sha256.rs; not cached. Update the pin if install-action now installs another build"
          rm -f "$dir/bin/$file"
          complete=false
          mismatch=true
        fi
      fi
    done < <(tr -d '\r' < "$cargo_tools")
  fi
  if [ "$complete" = true ] && [ -n "$(ls -A "$dir" 2> /dev/null)" ]; then
    echo "save=true" >> "$output"
    echo "the folder holds every tool; saving it"
  else
    echo "save=false" >> "$output"
    echo "the folder lacks a tool, or is empty; not saving it"
  fi
  [ "$mismatch" = false ]
}

case "${1:-}" in
  pins) cmd_pins ;;
  key) cmd_key ;;
  restore) cmd_restore ;;
  fetch-assets) cmd_fetch_assets ;;
  fill) cmd_fill ;;
  *) echo "usage: tool-cache.sh pins | key | restore | fetch-assets | fill" >&2; exit 2 ;;
esac
