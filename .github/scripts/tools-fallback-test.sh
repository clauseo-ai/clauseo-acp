#!/usr/bin/env bash
# Tests install-pinned-tools' fallback (fallback.sh) with fake cargo, npm and go first on PATH:
# a tool already at its pin is left alone; a missing one is installed at the exact pin
# (cargo install --locked <crate> --version =<pin>; lefthook's npm platform package; gitleaks
# with go install and its version stamped); a failed install, a missing npm or go, and a binary
# with no fallback fail the step. fast-lane runs it.
# Run it from anywhere: bash .github/scripts/tools-fallback-test.sh

# ok never fails, so `test && ok || fail` is if-then-else here.
# shellcheck disable=SC2015
set -euo pipefail
cd "$(dirname "$0")/../.."
script=$PWD/.github/actions/install-pinned-tools/fallback.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

export RUNNER_TEMP=$tmp/runner RUNNER_OS=Linux RUNNER_ARCH=X64 CARGO_HOME=$tmp/cargo
mkdir -p "$RUNNER_TEMP" "$CARGO_HOME/bin" "$tmp/fake"
jq -n '{tools: [{name: "cargo-nextest", version: "0.9.1"}, {name: "typos-cli", version: "1.2.3"}],
        binaries: [{name: "lefthook", version: "2.0.0", os: "linux", arch: "x86_64"},
                   {name: "gitleaks", version: "8.0.0", os: "linux", arch: "x86_64"}]}' > "$RUNNER_TEMP/pins.json"
printf 'cargo-nextest\t0.9.1\ntypos-cli\t1.2.3\n' > "$RUNNER_TEMP/cargo-tools.tsv"

# cargo: `cargo nextest --version` prints what $CARGO_HOME/bin/cargo-nextest holds; `cargo
# install` logs its arguments and installs a tool printing the asked version, unless
# FAKE_CARGO_FAIL names the crate.
cat > "$tmp/fake/cargo" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = install ]; then
  echo "cargo $*" >> "$RUNNER_TEMP/log"
  [ "${FAKE_CARGO_FAIL:-}" != "$3" ] || exit 101
  case "$3" in typos-cli) f=typos ;; *) f=$3 ;; esac
  printf '#!/usr/bin/env bash\necho "%s %s"\n' "$3" "${5#=}" > "$CARGO_HOME/bin/$f"
  chmod +x "$CARGO_HOME/bin/$f"
  exit 0
fi
[ -x "$CARGO_HOME/bin/cargo-$1" ] && exec "$CARGO_HOME/bin/cargo-$1" "$@"
exit 1
EOF
# npm pack <pkg>@<v>: a tarball whose package/bin/lefthook prints <v>.
cat > "$tmp/fake/npm" <<'EOF'
#!/usr/bin/env bash
echo "npm $*" >> "$RUNNER_TEMP/log"
spec=${@: -1}
mkdir -p p/package/bin
printf '#!/usr/bin/env bash\necho %s\n' "${spec##*@}" > p/package/bin/lefthook
tar -czf lefthook-fake.tgz -C p package
rm -rf p
EOF
# go install -ldflags <flags> <module>@v<v>: GOBIN/gitleaks printing the stamped version.
cat > "$tmp/fake/go" <<'EOF'
#!/usr/bin/env bash
echo "go $* GOBIN=$GOBIN" >> "$RUNNER_TEMP/log"
v=${3##*Version=}
printf '#!/usr/bin/env bash\necho %s\n' "$v" > "$GOBIN/gitleaks"
chmod +x "$GOBIN/gitleaks"
EOF
chmod +x "$tmp/fake/"*
# A hermetic PATH: the stubs, and links to the plain system tools the script and the stubs use,
# nothing else, so a real cargo, npm or go on the machine (the runners ship all three) never runs.
mkdir -p "$tmp/sys"
for tool in bash env cat grep sed awk tr jq tar gzip mktemp mkdir rm cp mv chmod install; do
  found=$(command -v "$tool") || { echo "FAIL: $tool is not installed"; exit 1; }
  ln -s "$found" "$tmp/sys/$tool"
done
export PATH="$CARGO_HOME/bin:$tmp/fake:$tmp/sys"

failures=0
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
ok() { echo "ok: $1"; }
run() { status=0; : > "$RUNNER_TEMP/log"; out=$(bash "$script" "$@" 2>&1) || status=$?; }

# 1. Nothing installed: each cargo tool from crates.io at the exact pin, announced.
run cargo-tools
{ [ "$status" = 0 ] && grep -qxF "cargo install --locked cargo-nextest --version =0.9.1" "$RUNNER_TEMP/log" \
  && grep -qxF "cargo install --locked typos-cli --version =1.2.3" "$RUNNER_TEMP/log" \
  && [ "$(grep -c '::warning title=pinned tools fallback::' <<< "$out")" = 2 ]; } \
  && ok "cargo tools: cargo install --locked --version =<pin>, each with a warning" || fail "cargo tools: $out"

# 2. Already at the pin (the download got that far, or a cache restored it): nothing installed.
run cargo-tools
{ [ "$status" = 0 ] && [ ! -s "$RUNNER_TEMP/log" ]; } && ok "a tool already at its pin is not installed again" || fail "rerun: $(cat "$RUNNER_TEMP/log")"

# 3. At another version: installed at the pin.
printf '#!/usr/bin/env bash\necho "typos-cli 1.0.0"\n' > "$CARGO_HOME/bin/typos"
run cargo-tools
grep -qxF "cargo install --locked typos-cli --version =1.2.3" "$RUNNER_TEMP/log" && ok "a tool at another version is installed at its pin" || fail "other version: $(cat "$RUNNER_TEMP/log")"

# 4. cargo install fails: the step fails.
rm "$CARGO_HOME/bin/cargo-nextest"
FAKE_CARGO_FAIL=cargo-nextest run cargo-tools
{ [ "$status" = 1 ] && grep -q "failed too" <<< "$out"; } && ok "a failed cargo install fails the step" || fail "cargo failure: exit $status"

# 5. The binaries: lefthook from its npm platform package, gitleaks with go install, stamped.
export ONLY="--only gitleaks --only lefthook"
run binaries
{ [ "$status" = 0 ] && grep -q "npm pack --silent lefthook-linux-x64@2.0.0" "$RUNNER_TEMP/log" \
  && grep -q "go install -ldflags -X=github.com/zricethezav/gitleaks/v8/version.Version=8.0.0 github.com/zricethezav/gitleaks/v8@v8.0.0 GOBIN=$CARGO_HOME/bin" "$RUNNER_TEMP/log" \
  && [ "$("$CARGO_HOME/bin/lefthook" version)" = 2.0.0 ] && [ "$("$CARGO_HOME/bin/gitleaks" version)" = 8.0.0 ]; } \
  && ok "lefthook from npm and gitleaks from go install, each at its pin and on PATH" || fail "binaries: $out $(cat "$RUNNER_TEMP/log")"

# 6. Already at the pins: nothing installed.
run binaries
{ [ "$status" = 0 ] && [ ! -s "$RUNNER_TEMP/log" ]; } && ok "binaries already at their pins are not installed again" || fail "binaries rerun: $(cat "$RUNNER_TEMP/log")"

# 7. No go on PATH (the stub removed; nothing else on PATH has one), and a binary with no
#    fallback: the step fails, saying why.
rm "$CARGO_HOME/bin/gitleaks" "$tmp/fake/go"
run binaries
{ [ "$status" = 1 ] && grep -q "go is not on PATH" <<< "$out"; } && ok "without go, gitleaks fails the step" || fail "no go: $out"
ONLY="--only somethingelse" run binaries
{ [ "$status" = 1 ] && grep -q "has no fallback" <<< "$out"; } && ok "a binary with no fallback fails the step" || fail "unknown binary: $out"

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "all passed"
