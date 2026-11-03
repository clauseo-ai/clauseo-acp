#!/usr/bin/env bash
# lefthook's pre-push command (lefthook.yml, which passes git's stdin on with `use_stdin`). Git
# writes one line per ref it is about to push, `<local ref> <local sha> <remote ref> <remote sha>`,
# and a delete (`git push origin --delete <branch>`, `git push origin :<branch>`) has a local sha
# of all zeros. A push of deletes only sends no commits, so there is nothing to check: it passes at
# once. Every other push runs `cargo xtask check --quick` in CI mode, as fast-lane runs it (#210);
# `CI=0 git push` runs it in local mode. With no ref on stdin (`lefthook run pre-push` by hand) the
# check runs too.
#
# The ui steps are about half of the check, so a push that changes no ui path skips them with
# `--skip-ui`; CI runs them whatever the branch. For each pushed ref that is not a delete, the
# paths it changes since its merge-base with origin/main go to changes.sh (the classifier CI's
# `changes` job uses). Only when every git command and changes.sh succeed and changes.sh prints
# `ui=false` does the check get `--skip-ui`. Anything else (no ref on stdin, no origin/main, a git
# or changes.sh failure, `ui=true`) runs the full check. changes.sh turns everything on for an
# empty diff or a path it has no rule for.
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
if [ "$skip_ui" = true ]; then
  echo "pre-push: no ui change on the pushed branch; skipping the ui steps (CI runs them)"
  exec cargo xtask check --quick --skip-ui
fi
exec cargo xtask check --quick
