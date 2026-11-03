#!/usr/bin/env bash
# lefthook's pre-push command (lefthook.yml, which passes git's stdin on with `use_stdin`). Git
# writes one line per ref it is about to push, `<local ref> <local sha> <remote ref> <remote sha>`,
# and a delete (`git push origin --delete <branch>`, `git push origin :<branch>`) has a local sha
# of all zeros. A push of deletes only sends no commits, so there is nothing to check: it passes at
# once. Every other push runs `cargo xtask check --quick` in CI mode, as fast-lane runs it (#210);
# `CI=0 git push` runs it in local mode. With no ref on stdin (`lefthook run pre-push` by hand) the
# check runs too.
#
# pre-push-test.sh tests it, alone and through lefthook with a local remote.

set -euo pipefail

if [ ! -t 0 ]; then
  refs=0
  deletes=0
  while read -r _local_ref local_sha _remote_ref _remote_sha; do
    refs=$((refs + 1))
    # All zeros, at either hash length (SHA-1 or SHA-256).
    if [ -n "$local_sha" ] && [ -z "${local_sha//0/}" ]; then
      deletes=$((deletes + 1))
    fi
  done
  if [ "$refs" -gt 0 ] && [ "$deletes" -eq "$refs" ]; then
    echo "pre-push: deletes only ($refs ref(s)), nothing to check"
    exit 0
  fi
fi

export CI="${CI:-1}"
exec cargo xtask check --quick
