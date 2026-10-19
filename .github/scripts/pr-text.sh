#!/usr/bin/env bash
# PR text check: every "## " section of the pull request template is in the description, and has
# something in it besides HTML comments and whitespace. Any other text, "none" included, fills it.
#
# Usage: PR_BODY="<description>" pr-text.sh <template>
#
# The description is read only from the PR_BODY environment variable and only ever as awk data,
# so nothing in it is run. An unset or empty PR_BODY (a pull request with no description) fails
# with every section missing.
#
# Both texts are read the same way: carriage returns are dropped, HTML comments are removed (an
# unclosed "<!--" hides the rest), and a "## " line inside a ``` or ~~~ fence is text, not a
# heading. Comment markers inside code, a fence or a `code span` on one line, are text too, as
# GitHub shows them. A section runs to the next "## " heading.
#
# Output, one pair per failing section, in the house style:
#   PR text: section "Deviations" is empty
#   fix: write what applies under "## Deviations", or "none"
# Exit status: 0 when every section is filled, 1 when one is missing or empty, 2 on a usage error.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: PR_BODY=<description> $0 <template>" >&2
  exit 2
fi

TEMPLATE=$1 awk '
# Where the run of n backticks that closes a code span starts in s, or 0 if none does.
function closing_run(s, n,   offset) {
  offset = 0
  while (match(s, /`+/)) {
    if (RLENGTH == n) {
      return offset + RSTART
    }
    offset += RSTART + RLENGTH - 1
    s = substr(s, RSTART + RLENGTH)
  }
  return 0
}

# The part of line s outside HTML comments. in_comment carries an open comment to the next line.
# A code span is copied whole, so a comment marker inside it stays text.
function visible(s,   out, open, tick, len, rest, at, end) {
  out = ""
  while (s != "") {
    if (in_comment) {
      at = index(s, "-->")
      if (at == 0) {
        return out
      }
      s = substr(s, at + 3)
      in_comment = 0
      continue
    }
    open = index(s, "<!--")
    tick = index(s, "`")
    if (open == 0 && tick == 0) {
      return out s
    }
    if (tick > 0 && (open == 0 || tick < open)) {
      match(substr(s, tick), /^`+/)
      len = RLENGTH
      rest = substr(s, tick + len)
      at = closing_run(rest, len)
      end = at > 0 ? tick + len + at + len - 2 : tick + len - 1
      out = out substr(s, 1, end)
      s = substr(s, end + 1)
      continue
    }
    out = out substr(s, 1, open - 1)
    s = substr(s, open + 4)
    in_comment = 1
  }
  return out
}

# Reads the "## " sections of text. order[1..n] holds the headings in first-seen order, and
# filled[heading] is 1 when any line under it has a non-blank character. Returns n.
function sections(text, order, filled,   n, count, lines, i, line, fence, heading) {
  gsub(/\r/, "", text)
  count = split(text, lines, "\n")
  n = 0
  fence = 0
  in_comment = 0
  heading = ""
  for (i = 1; i <= count; i++) {
    line = lines[i]
    if (!in_comment && line ~ /^[ \t]*(```|~~~)/) {
      fence = !fence
    } else if (!fence) {
      line = visible(line)
      if (line ~ /^##[ \t]/) {
        heading = line
        sub(/^##[ \t]+/, "", heading)
        sub(/[ \t]+$/, "", heading)
        if (!(heading in filled)) {
          order[++n] = heading
          filled[heading] = 0
        }
        continue
      }
    }
    if (heading != "" && line ~ /[^ \t]/) {
      filled[heading] = 1
    }
  }
  return n
}

BEGIN {
  path = ENVIRON["TEMPLATE"]
  template = ""
  while ((status = (getline line < path)) > 0) {
    template = template line "\n"
  }
  if (status < 0) {
    printf "PR text: cannot read the template %s\n", path
    print "fix: pass the path of .github/pull_request_template.md"
    exit 2
  }
  close(path)

  wanted = sections(template, order, template_filled)
  if (wanted == 0) {
    printf "PR text: the template %s has no \"## \" headings\n", path
    print "fix: pass the path of .github/pull_request_template.md"
    exit 2
  }

  sections(ENVIRON["PR_BODY"], body_order, filled)
  failed = 0
  for (i = 1; i <= wanted; i++) {
    heading = order[i]
    if (!(heading in filled)) {
      printf "PR text: section \"%s\" is missing\n", heading
      printf "fix: add \"## %s\" and write what applies under it, or \"none\"\n", heading
      failed = 1
    } else if (!filled[heading]) {
      printf "PR text: section \"%s\" is empty\n", heading
      printf "fix: write what applies under \"## %s\", or \"none\"\n", heading
      failed = 1
    }
  }
  if (!failed) {
    printf "PR text: all %d sections are filled\n", wanted
  }
  exit failed
}'
