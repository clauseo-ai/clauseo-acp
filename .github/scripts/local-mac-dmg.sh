#!/usr/bin/env bash
# local-mac-dmg.sh: release.yml's dry run (MODE=dry) for aarch64-apple-darwin, on a Mac instead of
# GitHub Actions: the `check`, `blocklist` (with release.yml's block-list switch on), `ui` and
# `build` jobs' steps through "Collect this leg's assets", in order, with the same sidecars,
# overlays, env, throwaway updater key, ad hoc signing and .app checks. It publishes nothing: no gh release, upload, push or notarisation.
#
#   bash .github/scripts/local-mac-dmg.sh [--out DIR] [--plan] [--allow-dirty]
#
# Run it from inside a clauseo-acp checkout; it builds that checkout's HEAD and finds the repo
# from the current directory, so a copy outside the repo (/tmp/local-mac-dmg.sh) works too.
# --plan prints the numbered steps and their commands and runs nothing (works on Linux).
# The DMG lands in --out (default target/local-mac-dmg/) as Clauseo_<version>_aarch64-<sha9>.dmg.
# Pins come from the files release.yml reads (rust-toolchain.toml, ui/package.json,
# tauri.conf.json) and release.yml itself (the leg's rustflags, the workflow env), never copied.
# Tests: local-mac-dmg-test.sh next to this file.

set -euo pipefail

TARGET=aarch64-apple-darwin
OUT_ARG="" PLAN=false ALLOW_DIRTY=false
die() { echo "local-mac-dmg: $*" >&2; exit 1; }
usage() { echo "usage: local-mac-dmg.sh [--out DIR] [--plan] [--allow-dirty]" >&2; exit 2; }
while [ $# -gt 0 ]; do
  case $1 in
    --out) [ $# -ge 2 ] || usage; OUT_ARG=$2; shift 2 ;;
    --out=*) OUT_ARG=${1#--out=}; shift ;;
    --plan) PLAN=true; shift ;;
    --allow-dirty) ALLOW_DIRTY=true; shift ;;
    -h | --help) usage ;;
    *) echo "local-mac-dmg: unknown option $1" >&2; usage ;;
  esac
done

# ---- The checkout: from the current directory, never from this script's folder ----
ROOT=$(git rev-parse --show-toplevel 2> /dev/null) ||
  die "$PWD is not in a git checkout; cd into a clauseo-acp checkout first"
for f in .github/workflows/release.yml .github/scripts/sidecars.mjs; do
  [ -f "$ROOT/$f" ] || die "$ROOT is not a clauseo-acp checkout (no $f)"
done
cd "$ROOT"
WF=.github/workflows/release.yml

# One value from release.yml, or a clear failure when its shape has moved on.
wf_env() { # a top-level env entry, quotes dropped
  local v
  v=$(sed -nE "s/^  $1: \"?([^\"]*)\"?\$/\1/p" "$WF" | head -1)
  [ -n "$v" ] || die "$WF has no top-level env $1; update this script"
  printf '%s' "$v"
}
wf_output() { # an output of the check job set from a literal, quotes dropped
  local v
  v=$(sed -nE "s/^      $1: '?([^']*)'?\$/\1/p" "$WF" | head -1)
  [ -n "$v" ] || die "$WF's check job has no output $1; update this script"
  printf '%s' "$v"
}
wf_matrix() { # a key of this target's matrix row
  local v
  v=$(grep -E "\{ target: $TARGET," "$WF" | sed -nE "s/.* $1: \"?([^,\"}]*)\"?[,} ].*/\1/p" | head -1)
  [ -n "$v" ] || die "$WF's build matrix has no $1 for $TARGET; update this script"
  printf '%s' "$v"
}

SHA9=$(git rev-parse --short=9 HEAD)
VERSION=$(jq -r .version crates/desktop/tauri.conf.json)
ARCH=$(wf_matrix arch)
OUT=${OUT_ARG:-target/local-mac-dmg}
case $OUT in /*) ;; *) OUT=$ROOT/$OUT ;; esac
DMG_NAME=Clauseo_${VERSION}_${ARCH}-${SHA9}.dmg
TOOLCHAIN=$(sed -nE 's/^channel *= *"([^"]+)".*/\1/p' rust-toolchain.toml)
NODE_MIN=$(jq -r '.engines.node' ui/package.json | sed -nE 's/^>=([0-9.]+)$/\1/p')
PNPM_PIN=$(jq -r '.packageManager' ui/package.json | sed -nE 's/^pnpm@([0-9.]+).*/\1/p')
[ -n "$TOOLCHAIN" ] || die "rust-toolchain.toml has no channel"
[ -n "$NODE_MIN" ] || die "ui/package.json's engines.node is not >=X.Y.Z; update this script"
[ -n "$PNPM_PIN" ] || die "ui/package.json's packageManager is not pnpm@X.Y.Z"

# The build job's env for a dry run: release.yml's workflow env, matrix row and check's outputs.
export CARGO_TERM_COLOR CARGO_PROFILE_RELEASE_DEBUG CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO
CARGO_TERM_COLOR=$(wf_env CARGO_TERM_COLOR)
CARGO_PROFILE_RELEASE_DEBUG=$(wf_env CARGO_PROFILE_RELEASE_DEBUG)
CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=$(wf_env CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO)
export SAFE_WORKSPACE TARGET MODE=dry CLAUSEO_RELEASE_MODE=dry VERSION FEATURES="" RUSTFLAGS
SAFE_WORKSPACE=$(wf_env SAFE_WORKSPACE)
RUSTFLAGS=$(wf_matrix rustflags)
# The block-list switch: release.yml's check output bundle_blocklist, 'false' for launch (the Safe
# workspace sets it to 'true'), read from there so a local DMG matches the release. On, the
# blocklist step downloads and verifies the snapshot and every build passes blocklist.conf.json;
# off, neither, and minisign is not needed.
BUNDLE_BLOCKLIST=$(wf_output bundle_blocklist)
case $BUNDLE_BLOCKLIST in
  true) BLOCKLIST_OVERLAY=release/blocklist.conf.json ;;
  false) BLOCKLIST_OVERLAY="" ;;
  *) die "$WF's bundle_blocklist is $BUNDLE_BLOCKLIST, not true or false" ;;
esac
# cargo and tauri put the build under CARGO_TARGET_DIR when it is set (a Mac's own setting).
TARGET_DIR=${CARGO_TARGET_DIR:-target}
case $TARGET_DIR in /*) ;; *) TARGET_DIR=$ROOT/$TARGET_DIR ;; esac
RELEASE_DIR=$TARGET_DIR/$TARGET/release
SUFFIX=dev.$SHA9

N=0
step() { N=$((N + 1)); echo; echo "== step $N: $*"; }
# run CMD...: runs it, or under --plan prints it (a function of this script with its body).
run() {
  if ! $PLAN; then "$@"; return; fi
  printf '    $'; printf ' %q' "$@"; echo
  if declare -F "$1" > /dev/null; then declare -f "$1" | sed '1,2d;$d; s/^/      /'; fi
}
note() { echo "    # $*"; }

if $PLAN; then
  echo "local-mac-dmg plan: $ROOT at $SHA9, $TARGET, MODE=dry, version $VERSION"
  echo "DMG: $OUT/$DMG_NAME"
  echo "env: RUSTFLAGS='$RUSTFLAGS' SAFE_WORKSPACE=$SAFE_WORKSPACE BUNDLE_BLOCKLIST=$BUNDLE_BLOCKLIST" \
    "CARGO_PROFILE_RELEASE_DEBUG=$CARGO_PROFILE_RELEASE_DEBUG" \
    "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=$CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO"
fi

# ---- Guards: platform, clean tree, prerequisites ----
step "guards: macOS arm64, a clean tree, the prerequisites"
if $PLAN; then
  note "refuses unless uname -s/-m is Darwin/arm64; refuses a dirty tree without --allow-dirty"
  note "needs rustup $TOOLCHAIN + $TARGET, node >=$NODE_MIN, pnpm $PNPM_PIN, jq,"
  note "codesign, hdiutil, /usr/libexec/PlistBuddy, shasum; minisign with BUNDLE_BLOCKLIST=true"
  [ -z "$(git status --porcelain)" ] || note "warning: this tree is dirty now"
else
  [ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] ||
    die "builds only on macOS arm64 (this is $(uname -s) $(uname -m)); --plan works anywhere"
  if [ -n "$(git status --porcelain)" ]; then
    $ALLOW_DIRTY || die "the tree is dirty, so dev.$SHA9 would not name what is built;" \
      "commit or stash, or pass --allow-dirty"
    echo "warning: building a dirty tree (--allow-dirty); dev.$SHA9 is HEAD, not what is built"
  fi
  missing=0
  need() { # need <ok?> <what> <install command>
    if ! eval "$1" > /dev/null 2>&1; then echo "missing: $2 -> $3" >&2; missing=1; fi
  }
  ver_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -1)" = "$2" ]; }
  need "command -v rustup" "rustup" "curl --proto '=https' -sSf https://sh.rustup.rs | sh"
  need "rustup toolchain list | grep -q '^$TOOLCHAIN-'" "Rust $TOOLCHAIN (rust-toolchain.toml)" \
    "(cd '$ROOT' && rustup toolchain install)"
  need "rustup target list --installed --toolchain '$TOOLCHAIN' | grep -qx '$TARGET'" \
    "the $TARGET target" "rustup target add $TARGET --toolchain $TOOLCHAIN"
  need "command -v node && ver_ge \"\$(node -p process.versions.node)\" '$NODE_MIN'" \
    "node >=$NODE_MIN (ui/package.json engines.node)" "brew install node"
  need "[ \"\$(pnpm -C ui --version)\" = '$PNPM_PIN' ]" "pnpm $PNPM_PIN (ui/package.json)" \
    "npm install -g pnpm@$PNPM_PIN"
  need "command -v jq" "jq" "brew install jq"
  if [ "$BUNDLE_BLOCKLIST" = true ]; then
    need "command -v minisign" "minisign (blocklist step)" "brew install minisign"
  fi
  need "command -v codesign && command -v hdiutil && command -v shasum" \
    "codesign, hdiutil, shasum" "xcode-select --install"
  need "[ -x /usr/libexec/PlistBuddy ]" "/usr/libexec/PlistBuddy" "xcode-select --install"
  [ "$missing" = 0 ] || die "install what is missing above, then run again"
  echo "prerequisites present: rustup $TOOLCHAIN/$TARGET, node $(node -v), pnpm $PNPM_PIN," \
    "jq, $([ "$BUNDLE_BLOCKLIST" = true ] && echo "minisign, ")codesign, hdiutil, PlistBuddy"
fi

# The throwaway updater key and its password live here only, and go on exit.
TMP=""
if ! $PLAN; then
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT
fi
# The download token: GH_TOKEN, else gh's own login, else none.
if $PLAN; then
  echo "GH_TOKEN: from the environment, else gh auth token, else none (anonymous downloads)"
elif [ -n "${GH_TOKEN:-}" ]; then
  echo "GH_TOKEN: from the environment"
elif command -v gh > /dev/null && GH_TOKEN=$(gh auth token 2> /dev/null) && [ -n "$GH_TOKEN" ]; then
  export GH_TOKEN; echo "GH_TOKEN: from gh auth token"
else
  unset GH_TOKEN; echo "GH_TOKEN: none; downloads go anonymously through github.com"
fi
# A dry run has no Apple identity, notarisation account or release key; none may leak in.
unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID \
  APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH CLAUSEO_UPDATER_PUBKEY CLAUSEO_RELEASE_DATE \
  TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD CLAUSEO_VERSION_SUFFIX

# ---- check job ----
check_mode() {
  local crate
  crate=$(cargo metadata --no-deps --format-version 1 --manifest-path crates/desktop/Cargo.toml |
    jq -c '.packages[] | select(.name == "clauseo-desktop")')
  [ "$(jq -r .version <<< "$crate")" = "$VERSION" ] ||
    die "tauri.conf.json says $VERSION, crates/desktop/Cargo.toml says $(jq -r .version <<< "$crate")"
  [[ "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] ||
    die "version $VERSION is not plain X.Y.Z"
  local on
  on=$(jq -r '.features as $f | [($f.default // []) | recurse(.[] | $f[.] // []) | .[]?]
    | unique | map(select(IN("e2e","marks","crash_test","staging-connector","local-connector")))
    | join(" ")' <<< "$crate")
  [ -z "$on" ] || die "the desktop crate's default features turn on $on"
  echo "mode=dry version=$VERSION features= release_date= version_suffix=$SUFFIX"
}
step "check: the mode, the version, the default features (check job)"
run check_mode

check_overlays() {
  ( cd crates/desktop/release
    leaves() {
      jq -r '[paths as $p | select(($p | all(type == "string")) and (getpath($p) | type) != "object")
        | $p | join(".")] | sort | join(" ")' "$1"
    }
    status=0
    want() { [ "$(leaves "$1")" = "$2" ] || { echo "$1 holds $(leaves "$1"); only $2" >&2; status=1; }; }
    want release.conf.json "build.beforeBuildCommand mainBinaryName"
    want machine.conf.json "bundle.windows.nsis.installMode"
    want windows.conf.json "bundle.windows.nsis.installerHooks"
    want blocklist.conf.json "bundle.resources.blocklist/blocklist-v1.txt.gz bundle.resources.blocklist/blocklist-v1.txt.gz.minisig"
    [ "$(jq -c .bundle.resources blocklist.conf.json)" = '{"blocklist/blocklist-v1.txt.gz":"blocklist/blocklist-v1.txt.gz","blocklist/blocklist-v1.txt.gz.minisig":"blocklist/blocklist-v1.txt.gz.minisig"}' ] ||
      { echo "blocklist.conf.json must put each file at blocklist/<the same name>" >&2; status=1; }
    [ "$(jq -r .mainBinaryName release.conf.json)" = Clauseo ] || { echo "mainBinaryName is not Clauseo" >&2; status=1; }
    [ "$(jq -r .bundle.windows.nsis.installMode machine.conf.json)" = perMachine ] ||
      { echo "machine.conf.json's installMode is not perMachine" >&2; status=1; }
    exit "$status" ) || die "the release overlays hold more than their own keys"
}
step "check: the release overlays hold only their own keys (check job)"
run check_overlays

# ---- blocklist job: download into crates/desktop/blocklist, where blocklist.conf.json points ----
fetch_blocklist() {
  local dir=crates/desktop/blocklist pub=crates/blocklist/keys/stage-f.pub attempt ok=false
  local base=https://github.com/clauseo-ai/blocklist/releases/download/current
  mkdir -p "$dir"
  pair() { # as release.yml with a token (gh), else github.com's anonymous download
    if [ -n "${GH_TOKEN:-}" ] && command -v gh > /dev/null; then
      gh release download current --repo clauseo-ai/blocklist --clobber --dir "$dir" \
        --pattern blocklist-v1.txt.gz --pattern blocklist-v1.txt.gz.minisig
    else
      curl -fsSL -o "$dir/blocklist-v1.txt.gz" "$base/blocklist-v1.txt.gz" &&
        curl -fsSL -o "$dir/blocklist-v1.txt.gz.minisig" "$base/blocklist-v1.txt.gz.minisig"
    fi
  }
  for attempt in 1 2 3 4 5; do
    if pair; then
      minisign -V -q -p "$pub" -m "$dir/blocklist-v1.txt.gz" -x "$dir/blocklist-v1.txt.gz.minisig" &&
        { ok=true; break; }
      echo "The pair does not verify (attempt $attempt of 5)."
    fi
    [ "$attempt" -eq 5 ] || sleep 30
  done
  $ok || die "could not download a verifying snapshot from clauseo-ai/blocklist (5 attempts)"
  local out comment generated entries age
  out=$(minisign -V -p "$pub" -m "$dir/blocklist-v1.txt.gz" -x "$dir/blocklist-v1.txt.gz.minisig")
  comment=$(sed -n 's/^Trusted comment: //p' <<< "$out")
  local pattern='^clauseo-blocklist v1 generated=([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z) entries=([0-9]+)$'
  [[ "$comment" =~ $pattern ]] || die "the trusted comment is \"$comment\""
  generated=${BASH_REMATCH[1]} entries=${BASH_REMATCH[2]}
  age=$(( $(date -u +%s) - $(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$generated" +%s) ))
  echo "block-list snapshot: generated $generated, $entries entries"
  [ "$age" -le 86400 ] || echo "warning: the snapshot is $((age / 3600)) hours old (more than 24)"
}
step "blocklist: download clauseo-ai/blocklist's current snapshot, verify it with stage-f.pub"
if [ "$BUNDLE_BLOCKLIST" = true ]; then
  run fetch_blocklist
else
  note "skipped: bundle_blocklist is false in release.yml, where the blocklist job does nothing"
fi

# ---- ui job, then the build job's installs ----
step "installs: the ui workspace (frozen lockfile) and the crates (cargo fetch --locked)"
note "rustup toolchain install / target add: the guards checked both; nothing to install"
run pnpm -C ui install --frozen-lockfile
run cargo fetch --locked

build_ui() {
  local maps=$OUT/ui-sourcemaps-$SHA9
  rm -rf "$maps"
  CLAUSEO_VERSION_SUFFIX=$SUFFIX pnpm -C ui --filter @clauseo/app exec vite build --sourcemap hidden
  ( cd ui/app/dist
    find . -name '*.map' -print0 | while IFS= read -r -d '' map; do
      mkdir -p "$maps/$(dirname "$map")"
      mv "$map" "$maps/$map"
    done
    [ -z "$(find . -name '*.map')" ] || die "a source map is left in ui/app/dist"
    [ -n "$(find "$maps" -name '*.map')" ] || die "vite wrote no source map"
    if grep -rl 'sourceMappingURL' .; then die "a file in ui/app/dist names its source map"; fi )
  echo "source maps moved to $maps"
}
step "ui: vite build --sourcemap hidden with CLAUSEO_VERSION_SUFFIX=$SUFFIX, maps moved out"
if grep -q CLAUSEO_VERSION_SUFFIX ui/app/vite.config.ts; then
  echo "ui/app/vite.config.ts reads CLAUSEO_VERSION_SUFFIX: About shows $VERSION-$SUFFIX"
else
  echo "ui/app/vite.config.ts does not read CLAUSEO_VERSION_SUFFIX (PR #922 not here): About shows $VERSION"
fi
run mkdir -p "$OUT"
run build_ui

# ---- build job ----
step "updater public key for the sandbox's build: none in a dry run (CLAUSEO_UPDATER_PUBKEY unset)"
note "unset above, as release.yml leaves it for MODE=dry"

step "sidecars for $TARGET (sidecars.mjs, then --check)"
run node .github/scripts/sidecars.mjs --safe-workspace="$SAFE_WORKSPACE" "$TARGET"
run node .github/scripts/sidecars.mjs --check "$TARGET"

step "build the app (tauri build --no-bundle with the release overlays)"
run pnpm -C ui tauri build --ci --target "$TARGET" --no-bundle \
  --config release/release.conf.json ${BLOCKLIST_OVERLAY:+--config "$BLOCKLIST_OVERLAY"} \
  --config release/sidecars.generated.json -- --locked

scan() { # scan <what> <form>...: the app binary holds none of the forms (NULs dropped first)
  local what=$1 exe=$RELEASE_DIR/Clauseo form found
  shift
  [ -f "$exe" ] || die "no app binary at $exe"
  for form in "$@"; do
    found=$(LC_ALL=C tr -d '\000' < "$exe" | LC_ALL=C grep -c -a -F -e "$form") || [ $? -eq 1 ]
    [ "$found" = 0 ] || die "$exe holds $what"
  done
  echo "$exe holds no $what"
}
check_test_key() {
  local file decoded id key
  file=$(tr -d '\r\n' < crates/desktop/release/updater-test.pub)
  decoded=$(printf '%s' "$file" | base64 -d)
  id=$(printf '%s\n' "$decoded" | sed -n 1p | awk '{ print $NF }')
  key=$(printf '%s\n' "$decoded" | sed -n 2p | tr -d '\r')
  [ "${#id}" = 16 ] && [ "${key#RW}" != "$key" ] || die "updater-test.pub is not a minisign public key"
  scan "form of the test updater key" "$file" "$key" "$id"
}
step "the app binary holds no form of the test updater key"
run check_test_key
step "the app binary holds no connector test host"
run scan "connector test host" data-staging.clauseo.chat 127.0.0.1:8793

make_key() {
  head -c 32 /dev/urandom | base64 > "$TMP/dry-updater.password"
  pnpm -s -C ui tauri signer generate --ci -p "$(cat "$TMP/dry-updater.password")" \
    -w "$TMP/dry-updater.key" > /dev/null
  jq -n --arg key "$(cat "$TMP/dry-updater.key.pub")" '{plugins: {updater: {pubkey: $key}}}' \
    > "$TMP/updater-key.conf.json"
  echo "throwaway updater key in $TMP (removed on exit)"
}
step "make a throwaway updater key (dry run) and choose its public key, in a mktemp -d"
run make_key

bundle() {
  local key rel helper macos tarball
  export APPLE_SIGNING_IDENTITY=-
  TAURI_SIGNING_PRIVATE_KEY=$(cat "$TMP/dry-updater.key")
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$(cat "$TMP/dry-updater.password")
  export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  for key in Helpers/clauseo-agent-browser Helpers/stetdoc/stetdoc; do
    rel=$(jq -er --arg key "$key" '.bundle.macOS.files[$key]' crates/desktop/release/sidecars.generated.json) ||
      die "sidecars.generated.json puts nothing at Contents/$key"
    helper=crates/desktop/$rel
    [ -f "$helper" ] || die "no helper for Contents/$key at $helper"
    codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime "$helper"
    codesign --verify --strict --verbose=2 "$helper" || die "$helper does not verify after signing"
  done
  rm -rf "$RELEASE_DIR/bundle" # a fresh runner has none; an old DMG must not be collected
  pnpm -C ui tauri bundle --ci --target "$TARGET" --bundles app,dmg \
    --config release/release.conf.json ${BLOCKLIST_OVERLAY:+--config "$BLOCKLIST_OVERLAY"} \
    --config "$TMP/updater-key.conf.json" --config release/sidecars.generated.json
  macos=$RELEASE_DIR/bundle/macos
  tarball=Clauseo_${VERSION}_${ARCH}.app.tar.gz
  mv "$macos/Clauseo.app.tar.gz" "$macos/$tarball"
  rm "$macos/Clauseo.app.tar.gz.sig"
  pnpm -C ui tauri signer sign --app-version "$VERSION" "$macos/$tarball"
}
step "bundle the .app and DMG ad hoc (APPLE_SIGNING_IDENTITY=-), sign the updater archive"
note "codesign --force -s - --options runtime on Contents/Helpers' two files first"
run bundle

check_app() {
  local app=$RELEASE_DIR/bundle/macos/Clauseo.app main want have helpers helper
  [ -d "$app" ] || die "no .app at $app"
  main=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
  [ "$main" = Clauseo ] || die "$app's CFBundleExecutable is $main, not Clauseo"
  want=$({ echo "$main"; jq -r '.bundle.externalBin[] | ltrimstr("binaries/")' crates/desktop/release/sidecars.generated.json; } |
    LC_ALL=C sort | tr '\n' ' ')
  have=$(ls -A "$app/Contents/MacOS" | LC_ALL=C sort | tr '\n' ' ')
  [ "$have" = "$want" ] || die "$app/Contents/MacOS holds $have; it may hold only $want"
  [ -d "$app/Contents/Helpers" ] || die "$app has no Contents/Helpers"
  helpers=$(ls -A "$app/Contents/Helpers" | LC_ALL=C sort | tr '\n' ' ')
  [ "$helpers" = "clauseo-agent-browser stetdoc " ] || die "$app/Contents/Helpers holds $helpers"
  [ "$(ls -A "$app/Contents/Helpers/stetdoc")" = stetdoc ] ||
    die "$app/Contents/Helpers/stetdoc must hold only stetdoc"
  for helper in "$app/Contents/Helpers/clauseo-agent-browser" "$app/Contents/Helpers/stetdoc/stetdoc"; do
    [ -f "$helper" ] && [ -x "$helper" ] || die "$helper is not an executable file"
    codesign --verify --strict --verbose=2 "$helper" || die "codesign --verify --strict fails on $helper"
  done
  codesign --verify --deep --strict --verbose=2 "$app" || die "codesign --verify --deep --strict fails on $app"
  echo "Contents/MacOS holds $have; Contents/Helpers holds $helpers(stetdoc/stetdoc; signed);" \
    "the .app verifies --deep --strict"
}
step "the .app keeps the browser helper in Contents/Helpers, and verifies"
run check_app

collect() {
  local bundle=$RELEASE_DIR/bundle tarball=Clauseo_${VERSION}_${ARCH}.app.tar.gz f
  cp "$bundle/dmg/Clauseo_${VERSION}_${ARCH}.dmg" "$OUT/$DMG_NAME"
  cp "$bundle/macos/$tarball" "$bundle/macos/$tarball.sig" "$OUT/"
  # assemble's check of this leg's archive, with the throwaway public key.
  node .github/scripts/latest-json.mjs --verify "$OUT/$tarball" "$TMP/dry-updater.key.pub" "$tarball" "$VERSION" ||
    die "$tarball.sig does not hold under the throwaway updater key"
  echo
  for f in "$OUT/$DMG_NAME" "$OUT/$tarball" "$OUT/$tarball.sig"; do
    echo "$f"
    echo "  size $(stat -f %z "$f") bytes, sha256 $(shasum -a 256 "$f" | cut -d ' ' -f 1)"
  done
}
step "collect the DMG as $DMG_NAME, the updater archive and its .sig, in $OUT"
run collect
