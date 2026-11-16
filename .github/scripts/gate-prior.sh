#!/usr/bin/env bash
# This head commit's last real result for a gated check, and what it decides. windows-pr-tests.yml's
# `decide` and e2e-windows.yml's `e2e-changes` call it before they pick a mode (windows-arm64-pr-
# tests.yml's too, once it lands); gate-prior-test.sh pins it with a fake gh and fixtures.
#
#   gate-prior.sh <check name> <runner of a real run> "<conclusions that fail>"
#   e.g. gate-prior.sh "windows tests (PR)" windows-2025 "failure timed_out"
#
# Reads REPO, HEAD_SHA, RUN_ID and RELEVANT from the environment (gh reads GH_TOKEN), and prints,
# for $GITHUB_OUTPUT:
#   prior=<job URL>   the last real result's job, when there is one
#   verdict=covered   it passed: nothing is tested twice
#   verdict=failed    it failed (one of the conclusions given) and this event is not relevant:
#                     the check fails again, naming it
#   verdict=none      no real result, or a relevant event: the caller decides on as if there were
#                     none (the label `ready`, an approval, the paths)
#
# A real result is a run of the check on its real runner (windows-2025, windows-11-arm), the job's
# `labels` in the jobs API. Every gate answer runs on ubuntu: wait (not ready, which must fail so
# that it blocks the merge), failed, covered, stale, and the no-change modes. Those are answers
# about a result, not results, so they are skipped and the newest real run before them decides.
# Before this, a wait stop counted as "the last result failed": on #732 (2026-11-08) the label
# `ready` and an approval came within two seconds, the labeled run was cancelled as the pending run
# of its concurrency group, and the review's run (not relevant: the label was already on) found the
# wait stop and failed in `failed` mode until a re-run by hand.
# Why the runner and not a step name or an annotation: the runner is where the job ran, set by the
# workflow's own runs-on, and no gate answer can take a Windows runner; step names and messages
# change with the wording, and e2e-windows.yml's wait and failed answers share one step. A job the
# API cannot read is skipped too, with a warning: at worst the tests run again, never a pass or a
# failure that no test gave.
#
# The current run (RUN_ID) never counts, so a re-run of it tests again. A failed read of the check
# runs prints verdict=none with a warning. It exits 0 unless its arguments are wrong.

set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: gate-prior.sh <check name> <runner> \"<conclusions that fail>\"" >&2
  exit 2
fi
check=$1
runner=$2
failing=" $3 "
: "${REPO:?}" "${HEAD_SHA:?}" "${RUN_ID:?}"
relevant=${RELEVANT:-false}

if ! runs=$(gh api -X GET "repos/$REPO/commits/$HEAD_SHA/check-runs" \
    -f check_name="$check" -f filter=all -f per_page=100); then
  echo "::warning::The check runs API failed; deciding without this commit's earlier result" >&2
  echo "verdict=none"
  exit 0
fi

# Finished runs of the check in other workflow runs, newest first: "<conclusion> <job URL>".
if ! candidates=$(jq -r --arg own "/runs/$RUN_ID/" '
    [.check_runs[] | select(.status == "completed" and (.details_url | contains($own) | not))]
    | sort_by(.completed_at) | reverse | .[] | "\(.conclusion) \(.details_url)"' <<< "$runs"); then
  echo "::warning::The check runs could not be read; deciding without this commit's earlier result" >&2
  echo "verdict=none"
  exit 0
fi

while read -r conclusion url; do
  [ -n "${url:-}" ] || continue
  job=${url##*/job/}
  job=${job%%[!0-9]*}
  if [ -z "$job" ] || ! labels=$(gh api "repos/$REPO/actions/jobs/$job" | jq -r '.labels[]'); then
    echo "::warning::Could not read the job behind $url; it is not counted" >&2
    continue
  fi
  if ! grep -qxF -- "$runner" <<< "$labels"; then
    echo "Not a result: $url ($conclusion) is a gate's answer on $(tr '\n' ' ' <<< "$labels")" >&2
    continue
  fi
  echo "This commit's last result: $conclusion $url" >&2
  echo "prior=$url"
  if [ "$conclusion" = success ]; then
    echo "verdict=covered"
  elif [ "${failing#* "$conclusion" }" != "$failing" ] && [ "$relevant" != true ]; then
    echo "verdict=failed"
  else
    echo "verdict=none"
  fi
  exit 0
done <<< "$candidates"

echo "verdict=none"
