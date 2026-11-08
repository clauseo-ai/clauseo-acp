#!/usr/bin/env bash
# The fallback of install-pinned-tools (action.yml beside this script), for when GitHub's release
# downloads fail (2026-11-08: 504s from about 08:50Z). It runs only after the download step has
# failed, never first, and installs from other registries at the same pins:
#
#   fallback.sh cargo-tools   each cargo tool from cargo-tools.tsv not already at its pin:
#                             cargo install --locked <crate> --version =<pin>, from crates.io
#                             (its index checksums each crate; --locked uses the crate's own
#                             Cargo.lock). The repo's rust-toolchain.toml toolchain builds it: every
#                             pinned tool's MSRV is at most 1.95 (crates.io, 2026-11-08).
#   fallback.sh binaries      each binary named in $ONLY ("--only <name> ..."), not already at its pin:
#                             lefthook from npm (the platform package lefthook-<os>-<arch>, whose
#                             tarball npm checks against the registry's sha512), gitleaks with
#                             `go install` through the Go module proxy (checked against
#                             sum.golang.org), its version stamped as its release builds do.
#
# Neither path can check setup.rs's sha256 pins, which are of the GitHub release assets: the
# registries' own checksums stand in, and the action's version checks run after either. Every
# install is announced with a ::warning:: so a run that used it says so. Versions come from
# pins.json (`cargo xtask setup --list=json`), never from this file.
#
# Environment: RUNNER_TEMP (pins.json, cargo-tools.tsv), RUNNER_OS, RUNNER_ARCH, CARGO_HOME, ONLY.
set -euo pipefail

pins=$RUNNER_TEMP/pins.json
bin=${CARGO_HOME:-$HOME/.cargo}/bin
if command -v cygpath > /dev/null; then bin=$(cygpath -u "$bin"); fi
exe=""
[ "${RUNNER_OS:-}" = Windows ] && exe=.exe

warn() { echo "::warning title=pinned tools fallback::$*"; }

# The version a cargo tool prints, as the action's version check reads it.
tool_version() {
  case "$1" in
    cargo-*) cargo "${1#cargo-}" --version 2> /dev/null | awk 'NR == 1 { print $2 }' || true ;;
    *) "${1%-cli}" --version 2> /dev/null | awk 'NR == 1 { print $2 }' || true ;;
  esac
}

cargo_tools() {
  local failed=0
  while IFS=$'\t' read -r name version; do
    [ -n "$name" ] || continue
    if [ "$(tool_version "$name")" = "$version" ]; then
      echo "$name $version: already installed"
      continue
    fi
    warn "the GitHub download of $name failed; installing $name $version from crates.io (cargo install --locked $name --version =$version)"
    if ! cargo install --locked "$name" --version "=$version"; then
      echo "::error::cargo install --locked $name --version =$version failed too"
      failed=1
    fi
  done < <(tr -d '\r' < "$RUNNER_TEMP/cargo-tools.tsv")
  exit "$failed"
}

pinned_version() {
  jq -r --arg n "$1" '[.binaries[] | select(.name == $n) | .version][0] // empty' "$pins" | tr -d '\r'
}

npm_platform() {
  local os arch
  case "$RUNNER_OS" in Linux) os=linux ;; macOS) os=darwin ;; Windows) os=windows ;; *) return 1 ;; esac
  case "$RUNNER_ARCH" in X64) arch=x64 ;; ARM64) arch=arm64 ;; *) return 1 ;; esac
  echo "$os-$arch"
}

install_lefthook() {
  local version=$1 platform work
  command -v npm > /dev/null || { echo "::error::npm is not on PATH, so lefthook has no fallback here"; return 1; }
  platform=$(npm_platform) || { echo "::error::no lefthook npm package for $RUNNER_OS $RUNNER_ARCH"; return 1; }
  work=$(mktemp -d)
  (cd "$work" && npm pack --silent "lefthook-$platform@$version" > /dev/null) || { rm -rf "$work"; return 1; }
  tar -xzf "$work"/lefthook-*.tgz -C "$work"
  mkdir -p "$bin"
  install -m 755 "$work/package/bin/lefthook$exe" "$bin/lefthook$exe"
  rm -rf "$work"
}

install_gitleaks() {
  local version=$1
  command -v go > /dev/null || { echo "::error::go is not on PATH, so gitleaks has no fallback here"; return 1; }
  mkdir -p "$bin"
  # GOTOOLCHAIN=auto: go fetches the Go its go.mod asks for, from the module proxy too.
  GOBIN=$bin GOTOOLCHAIN=auto go install \
    -ldflags "-X=github.com/zricethezav/gitleaks/v8/version.Version=$version" \
    "github.com/zricethezav/gitleaks/v8@v$version"
}

binaries() {
  local failed=0 name version
  # $ONLY is "--only <name> ..."; the names are every second word.
  # shellcheck disable=SC2086
  set -- ${ONLY:-}
  while [ "$#" -ge 2 ]; do
    name=$2
    shift 2
    version=$(pinned_version "$name")
    if [ -n "$version" ] && [ "$("$name" version 2> /dev/null || true)" = "$version" ]; then
      echo "$name $version: already installed"
      continue
    fi
    case "$name" in
      lefthook)
        warn "the GitHub download of lefthook failed; installing lefthook $version from npm (lefthook-$(npm_platform || echo unknown)@$version)"
        install_lefthook "$version" || failed=1
        ;;
      gitleaks)
        warn "the GitHub download of gitleaks failed; building gitleaks $version with go install (github.com/zricethezav/gitleaks/v8@v$version)"
        install_gitleaks "$version" || failed=1
        ;;
      *)
        echo "::error::$name has no fallback for a failed GitHub download"
        failed=1
        ;;
    esac
  done
  exit "$failed"
}

case "${1:-}" in
  cargo-tools) cargo_tools ;;
  binaries) binaries ;;
  *) echo "usage: fallback.sh cargo-tools | binaries" >&2; exit 2 ;;
esac
