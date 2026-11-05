#!/usr/bin/env bash
# lefthook's pre-push command (lefthook.yml, which passes git's stdin on with `use_stdin`). Git
# writes one line per ref it is about to push, `<local ref> <local sha> <remote ref> <remote sha>`,
# and a delete (`git push origin --delete <branch>`, `git push origin :<branch>`) has a local sha
# of all zeros. A push of deletes only sends no commits, so there is nothing to check: it passes at
# once. Every other push runs `cargo xtask check --quick --no-tests` in CI mode, so clippy denies
# warnings as fast-lane's does (#210); `CI=0 git push` runs it in local mode. With no ref on stdin
# (`lefthook run pre-push` by hand) the check runs too.
#
# `--no-tests` leaves out the five test steps (nextest, nextest-specta, doctest, vitest,
# playwright-unit): a push runs the format, lint, typecheck and build steps only (fmt, both clippy
# steps, deny, shear, typos, taplo, gitleaks, the static checks, tsc, oxlint, oxfmt, vite-build).
# Under load on the dev machines the tests timed out and failed pushes; CI and the merge queue run
# them.
#
# The ui steps are about half of the check, so a push that changes no ui path skips them with
# `--skip-ui` as well; CI runs them whatever the branch. For each pushed ref that is not a delete, the
# paths it changes since its merge-base with origin/main go to changes.sh (the classifier CI's
# `changes` job uses). Only when every git command and changes.sh succeed and changes.sh prints
# `ui=false` does the check get `--skip-ui`. Anything else (no ref on stdin, no origin/main, a git
# or changes.sh failure, `ui=true`) runs the ui steps too. changes.sh turns everything on for an
# empty diff or a path it has no rule for.
#
# Then, when the check passes, Windows clippy for the crates the push touches: PR CI compiles Rust
# only in Windows x64 clippy (PR_LINT_ONLY in ci.yml), so code that is dead on Windows (imports and
# helpers only `#[cfg(unix)]` tests use, a lint only Windows trips) turned PRs red after the push.
# `cargo clippy --target x86_64-pc-windows-gnu --all-targets -p <crate>... -- -D warnings`, with
# MinGW-w64 for the C that libsqlite3-sys and zstd-sys build (the msvc target needs Microsoft's headers).
# A touched crate is a workspace folder, `crates/<name>/` or `xtask/`, holding a path the pushed
# branch changes since its merge-base with origin/main; root files (Cargo.lock, clippy.toml) touch
# none. It is skipped, with a note, on Windows (the check already lints there), with
# PREPUSH_WINDOWS_CLIPPY=0, and where the target or MinGW-w64 is missing (the note says how to
# install them). On the Mac mini it added 5 to 22 s to warm pushes of the four PRs Windows clippy
# turned red on 2026-11-05, and found each one's errors.
#
# pre-push-test.sh tests it, alone and through lefthook with a local remote.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
shas=()

if [ ! -t 0 ]; then
  refs=0
  deletes=0
  while read -r _local_ref local_sha _remote_ref _remote_sha; do
    refs=$((refs + 1))
    # All zeros, at either hash length (SHA-1 or SHA-256).
    if [ -n "$local_sha" ] && [ -z "${local_sha//0/}" ]; then
      deletes=$((deletes + 1))
    elif [ -n "$local_sha" ]; then
      shas+=("$local_sha")
    fi
  done
  if [ "$refs" -gt 0 ] && [ "$deletes" -eq "$refs" ]; then
    echo "pre-push: deletes only ($refs ref(s)), nothing to check"
    exit 0
  fi
fi

# The NUL-separated paths each pushed sha changes since its merge-base with origin/main, one diff
# after another. Fails when git does.
pushed_paths() {
  local sha base
  for sha in "$@"; do
    base=$(git merge-base origin/main "$sha" 2> /dev/null) || return 1
    git diff --no-renames -z --name-only "$base" "$sha" || return 1
  done
}

skip_ui=false
if [ "${#shas[@]}" -gt 0 ]; then
  if outputs=$(pushed_paths "${shas[@]}" | bash "$here/changes.sh"); then
    if printf '%s\n' "$outputs" | grep -qx 'ui=false'; then
      skip_ui=true
    fi
  fi
fi

export CI="${CI:-1}"
args=(--quick --no-tests)
if [ "$skip_ui" = true ]; then
  echo "pre-push: no ui change on the pushed branch; skipping the ui steps (CI runs them)"
  args+=(--skip-ui)
fi
cargo xtask check "${args[@]}"

# The package names of the workspace crates holding a path on stdin (NUL-separated), one per line,
# sorted. A folder with no Cargo.toml in the working tree (a crate the push deletes) gives none.
touched_crates() {
  local dir name
  tr '\0' '\n' | sed -n -E 's#^(crates/[^/]+|xtask)/.*#\1#p' | sort -u | while read -r dir; do
    [ -f "$dir/Cargo.toml" ] || continue
    name=$(sed -n -E 's/^name *= *"([^"]+)".*/\1/p' "$dir/Cargo.toml" | head -n 1)
    if [ -n "$name" ]; then echo "$name"; fi
  done | sort -u
}

windows=x86_64-pc-windows-gnu
if [ "${PREPUSH_WINDOWS_CLIPPY:-1}" = 0 ]; then
  echo "pre-push: PREPUSH_WINDOWS_CLIPPY=0; skipping Windows clippy"
  exit 0
fi
[ "${#shas[@]}" -gt 0 ] || exit 0
crates=$(pushed_paths "${shas[@]}" | touched_crates) || crates=""
[ -n "$crates" ] || exit 0
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    echo "pre-push: on Windows the check above already linted Windows"
    exit 0
    ;;
esac
if ! rustup target list --installed 2> /dev/null | grep -qx "$windows" \
  || ! command -v x86_64-w64-mingw32-gcc > /dev/null; then
  echo "pre-push: Windows clippy skipped: it needs \`rustup target add $windows\` and MinGW-w64"
  echo "  (\`brew install mingw-w64\`, or \`apt install gcc-mingw-w64-x86-64\`). Without it a Windows-only"
  echo "  clippy error shows up in the PR's Windows check instead."
  exit 0
fi
packages=()
for crate in $crates; do packages+=(-p "$crate"); done
echo "pre-push: Windows clippy ($windows) for ${crates//$'\n'/ }"
exec cargo clippy --target "$windows" --all-targets "${packages[@]}" -- -D warnings
