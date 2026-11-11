#!/usr/bin/env bash
# Prunes the Actions cache by the families in .github/scripts/cache-families.txt.
#
#   cache-entries.sh prune
#     ci.yml's nightly cache-prune job, after the saves. Deletes:
#     - every entry whose ref is not refs/heads/main;
#     - in each family, every main entry but the newest (created_at, then id);
#     - in a family whose line sets idle=<days>, any main entry not accessed for that long;
#     - a main entry no family matches, once it has not been accessed for 3 days (logged until
#       then, so a family forgotten in cache-families.txt shows before anything is lost).
#   cache-entries.sh prune --families <name,name...>
#     The same, for main entries of the named families only: a workflow's own prune job, after
#     its own saves (e2e-windows.yml). Nothing off main, and nothing unknown, is touched.
#
# It never deletes a family's newest entry unless that family's idle rule says so: the newest is
# the one runs restore. Each deletion is logged with its key, ref, size and reason. Nothing here
# fails a job: without gh or jq, without `actions: write`, or with the API down, it prints a
# warning and exits 0.
#
# Environment: GH_TOKEN and GITHUB_REPOSITORY, as in a job; GITHUB_WORKSPACE, the checkout. For
# the tests: CACHE_LIST_FILE, the JSON of `gh api repos/<repo>/actions/caches` (or a plain array of
# its entries), read instead of the API; CACHE_DRY_RUN=1, which prints each deletion instead of
# making it; CACHE_NOW, the time to measure idleness from (ISO 8601).
set -euo pipefail

families_file=${CACHE_FAMILIES_FILE:-${GITHUB_WORKSPACE:-.}/.github/scripts/cache-families.txt}
# How long a main entry of no family is kept after its last access.
UNKNOWN_IDLE_DAYS=3

warn() { echo "::warning title=cache prune::$*"; }

usage() {
  warn "usage: cache-entries.sh prune [--families <name,name...>]"
  exit 2
}

command -v jq > /dev/null || { warn "jq is not installed; no cache entry deleted"; exit 0; }
if [ -z "${CACHE_LIST_FILE:-}" ] && ! command -v gh > /dev/null; then
  warn "gh is not installed; no cache entry deleted"
  exit 0
fi

# The families as a JSON array of {name, re, idle}; idle is in days, or null.
families=$(awk '!/^[[:space:]]*(#|$)/ {
    idle = ""
    for (i = 3; i <= NF; i++) if ($i ~ /^idle=[0-9]+$/) idle = substr($i, 6)
    print $1 "\t" $2 "\t" idle
  }' "$families_file" |
  jq -R -s 'split("\n") | map(select(. != "") | split("\t")
    | {name: .[0], re: .[1], idle: (if .[2] == "" then null else (.[2] | tonumber) end)})')

# Every entry, as a JSON array.
list_entries() {
  if [ -n "${CACHE_LIST_FILE:-}" ]; then
    jq 'if type == "array" then . else .actions_caches end' "$CACHE_LIST_FILE"
  else
    gh api --paginate "repos/${GITHUB_REPOSITORY:?}/actions/caches?per_page=100" \
      --jq '.actions_caches[]' | jq -s .
  fi
}

# jq: the families a key matches, each {name, id, idle}; id is the name plus each capture group's
# text. secs: an API timestamp in seconds. mib: bytes in MiB, one decimal.
# shellcheck disable=SC2016 # jq's own $variables
JQ_DEFS='
def families($fams):
  . as $k
  | [ $fams[] | . as $f | select($k | test($f.re))
      | {name: $f.name, idle: $f.idle,
         id: ([$f.name] + [$k | match($f.re).captures[] | (.string // "")] | join("|"))} ];
def secs: sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601;
def mib: (. / 1048576 * 10 | round / 10);
'

# Deletes the entries in $1, a JSON array of {id, key, ref, size_in_bytes, reason}.
delete_all() {
  local plan=$1 count deleted=0 failed=0 freed=0
  count=$(jq 'length' <<< "$plan")
  if [ "$count" -eq 0 ]; then
    echo "nothing to delete"
    return 0
  fi
  while IFS=$'\t' read -r id key ref size reason; do
    local mb
    mb=$(jq -n --argjson s "$size" "$JQ_DEFS"' $s | mib')
    if [ "${CACHE_DRY_RUN:-}" = 1 ]; then
      echo "would delete: $key ($ref, $mb MiB): $reason"
      deleted=$((deleted + 1)); freed=$((freed + size))
    elif gh api -X DELETE "repos/${GITHUB_REPOSITORY:?}/actions/caches/$id" > /dev/null 2>&1; then
      echo "deleted: $key ($ref, $mb MiB): $reason"
      deleted=$((deleted + 1)); freed=$((freed + size))
    else
      warn "could not delete $key ($ref, id $id); the job needs permissions: actions: write"
      failed=$((failed + 1))
    fi
  done < <(jq -r '.[] | [.id, .key, .ref, .size_in_bytes, .reason] | @tsv' <<< "$plan")
  local total
  total=$(jq -n --argjson s "$freed" "$JQ_DEFS"' $s | mib')
  echo "deleted $deleted of $count entries, $total MiB; $failed failed"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "Cache entries deleted: $deleted of $count, $total MiB; $failed failed" >> "$GITHUB_STEP_SUMMARY"
  fi
}

prune() {
  local only=${1:-} entries plan now
  if ! entries=$(list_entries 2> /dev/null) || [ -z "$entries" ]; then
    warn "could not list the cache entries (the API is down, or the job lacks actions: read); none deleted"
    exit 0
  fi
  now=${CACHE_NOW:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}
  # One JSON array: each entry with its families and its idle days, then what to delete.
  local annotated
  annotated=$(jq --argjson fams "$families" --arg now "$now" "$JQ_DEFS"'
    ($now | secs) as $t
    | map(. + {fams: (.key | families($fams)),
               idle_days: (($t - (.last_accessed_at // .created_at | secs)) / 86400)})' <<< "$entries")
  # Ambiguous keys and kept unknown entries, logged.
  jq -r --arg only "$only" --argjson days "$UNKNOWN_IDLE_DAYS" '
    .[] | select(.ref == "refs/heads/main")
    | if (.fams | length) > 1 then
        "::warning title=cache prune::\(.key) matches more than one family in cache-families.txt; kept"
      elif (.fams | length) == 0 and $only == "" and .idle_days < $days then
        "kept for now: \(.key) matches no family in cache-families.txt; deleted once not accessed for \($days) days"
      else empty end' <<< "$annotated"
  plan=$(jq --arg only "$only" --argjson days "$UNKNOWN_IDLE_DAYS" '
    ($only | split(",") | map(select(. != ""))) as $names
    | (if ($names | length) == 0 then
        map(select(.ref != "refs/heads/main") | . + {reason: "not on main"})
        + map(select(.ref == "refs/heads/main" and (.fams | length) == 0 and .idle_days >= $days)
            | . + {reason: "no family in cache-families.txt matches it, and not accessed for \(.idle_days | floor) days"})
      else [] end) as $other
    | map(select(.ref == "refs/heads/main" and (.fams | length) == 1)
        | . + {fam: .fams[0].id, famname: .fams[0].name, maxidle: .fams[0].idle}
        | select(($names | length) == 0 or (.famname as $n | $names | index($n))))
    | group_by(.fam)
    | map(sort_by(.created_at, .id) as $sorted
        | ($sorted | last) as $newest
        | $sorted[]
        | if .id != $newest.id then . + {reason: "superseded by \($newest.key)"}
          elif .maxidle != null and .idle_days >= .maxidle then
            . + {reason: "not accessed for \(.idle_days | floor) days (its family keeps \(.maxidle))"}
          else empty end) as $fam
    | $other + $fam' <<< "$annotated")
  delete_all "$plan"
}

case "${1:-}" in
  prune)
    if [ "$#" -eq 1 ]; then prune ""
    elif [ "$#" -eq 3 ] && [ "$2" = --families ]; then prune "$3"
    else usage
    fi
    ;;
  *) usage ;;
esac
