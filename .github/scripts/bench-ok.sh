#!/usr/bin/env bash
# Prints `true` when pull request $PR carries the label `bench-ok` now, else `false`. ci.yml's
# reducer-bench reads it before its bench runs. The labels come from the API, not from the event:
# a re-run keeps the event's payload, labels included, so a label added after the run started
# never reached a re-run (#276). When the API call fails, the event's labels decide
# ($BENCH_OK_EVENT, `true` or `false`), with a warning.
#
# env: PR (the pull request's number), GH_REPO (owner/name), GH_TOKEN, BENCH_OK_EVENT.
# bench-ok-test.sh tests it.

set -euo pipefail

# A failed `gh api` prints its error to stdout and exits non-zero; only a success is read.
if labels=$(gh api --paginate "repos/$GH_REPO/issues/$PR/labels" --jq '.[].name' 2> /dev/null); then
  if printf '%s\n' "$labels" | grep -qxF bench-ok; then echo true; else echo false; fi
else
  echo "::warning title=bench-ok::Could not read the labels of #$PR; the event's decide (bench-ok: ${BENCH_OK_EVENT:-false})." >&2
  if [ "${BENCH_OK_EVENT:-false}" = true ]; then echo true; else echo false; fi
fi
