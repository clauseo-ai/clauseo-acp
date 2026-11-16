# Word blackline fixtures: tables inside tables, hidden text, long paragraphs

stetdoc 0.9.3's real output for what the first fixtures (`../`) did not show, read by the parser's
unit tests (`src/actor/query/blackline/parse/tests/partial.rs`) and the host tests
(`tests/host/blackline_partial.rs`).

- `nested-table-old.docx`, `nested-table-new.docx`: an outer 2-column table whose left cell holds
  a 1-cell table; the change is in a body paragraph. `view-nested.json` is the view of their
  redline.
- `nested-inner-old.docx`, `nested-inner-new.docx`: the same shape, the change inside the inner
  cell. `view-nested-inner.json`.
- `hidden-text-old.docx`, `hidden-text-new.docx`: a paragraph with a hidden run (`w:vanish`)
  that stays; the change is in another paragraph. `view-hidden.json` (it warns `hidden_text`).
- `view-hidden-change.json`: hidden text that changes (`hidden-change-*.docx` of
  `make_more.py`).
- `view-deep3.json`: tables three deep, the change in the deepest (`deep3-*.docx`).
- `view-long.json`: a paragraph of about 4,500 characters, which stetdoc wraps over four lines
  with a trailing `\` (`long-*.docx`).
- `strike-old.docx`, `strike-new.docx` and `hide-old.docx`, `hide-new.docx`: a sentence struck through,
  and one hidden, as tracked format changes only. `view-strike.json`, `view-hide.json`: stetdoc
  prints each paragraph's change as `{rev run #0 C}` and nothing else.
- `notes-old.docx`, `notes-new.docx`: the body, a footnote and a header each change. `view-notes.json`:
  the summary counts 6 tracked changes, the body's line names 2, and the footnote follows
  `{appendix}` in its final text, unmarked.
- `view-row-ins.json`, `view-row-del.json`, `view-cell-ins.json`, `view-cell-del.json`,
  `view-row-ins-nested.json`: `stetdoc view --all --json` on documents of `make_more.py` that
  already hold a tracked row insertion or deletion, a cell insertion or deletion, and a row
  inserted in a table inside a cell (`{row inserted #11 R}`, `{cell deleted #14 R}`; the
  paragraphs inside show no change of their own).
- `picture-old.docx`, `picture-new.docx`: a signature picture inserted after "Signed: ".
  `view-picture.json`: `p2 Signed: {++{img}++}{#0 C}`, a change with no text.
- `link-old.docx`, `link-new.docx`: only a link's address changes (`acct-111` to `acct-999`).
  `view-link.json`: stetdoc marks no change and gives no warning.
- `inner-last-old.docx`, `inner-last-new.docx`: an inner table that is the last thing in its
  cell, removed in the new version. `compare-inner-last.json` is what `stetdoc compare --json`
  printed for them: `validation_failed`, exit 5 (stetdoc's fix comes after its PR #22).

What stetdoc 0.9.3's compare does not mark, or its view does not show (`make_links.py`; read by
`parse/ledger/tests.rs`, `parse/sides/tests.rs`, `parse/tests/forms.rs` and
`tests/host/blackline_crossed.rs`):

- `final-link-old.json`, `final-link-new.json`: `stetdoc view --all --json --mode final` of the
  bank-details pair above; `check-link.json`: `stetdoc check --json` of its redline (no change).
- `comment-old.docx`, `comment-new.docx`: a comment's text changes ("Check the fee with the
  client." to "The client agreed the fee."). The compare marks nothing; `view-comment.json` lists
  the new text after `{appendix}`. `final-comment-*.json`, `check-comment.json`.
- `textbox-old.docx`, `textbox-new.docx`: a text box's text changes. `view-textbox.json`:
  `{----}{#1 C}{++{textbox box "Cap of 2 million."}++}{#0 C}`, the old text not shown.
  `final-textbox-*.json`, `check-textbox.json`.
- `cellfmt-old.docx`, `cellfmt-new.docx`: a cell's shading changes. stetdoc 0.9.3's compare marks
  nothing for it at all (nor for a cell's borders or alignment, a row's or a table's format), so
  there is no view of it: the host's own comparison of the two files' tables names it
  (`src/actor/query/blackline/tables.rs`).
- `footnote-*.docx`, `header-*.docx`: only the footnote, or only the header, changes.
  `view-footnote.json`, `view-header.json` (the body unchanged; the summary counts 2) and
  `check-footnote.json`, `check-header.json` (ids 2 and 3, in `p3` and `p2` of their stories).
- `props.docx`: a document that already holds tracked format changes of a table, its grid, a row,
  a cell, a list paragraph and the last section, and a move, viewed as it is: `view-props.json`
  (`{rev table #23 R}`, `{rev grid #24 R}`, `{rev row #22 R}`, `{rev para #25 R}`, `move to -`,
  `move from -`; nothing for the cell's #21 or the section's #26) and `check-props.json` (all
  eight ids, `cell_format` and `section_format` among the kinds).
- `../check.json`: `stetdoc check --json` of the lease's redline (ids 2 to 17; 16 and 17 are its
  footnote's).

The notes, headers and footers, what compare says beside the redline, and table formatting
(`make_stories.py`; read by `parse/story/tests.rs`, `parse/receipt/tests.rs`,
`parse/ledger/tests.rs`, `tests/host/blackline_crossed.rs` and `tests/host/blackline_parts.rs`):

- `story-notes-<pair>.json`, `story-headers-<pair>.json`: `stetdoc view --all --json --story notes`
  and `--story headers` of the redline of each of `footnote`, `header`, `notes`, `stories`,
  `unmatched`, `excluded` and `cellfmt-body` (`{footnote 1}`, `{endnote 1}`, `{header default
  sections=1,2}`, `{footer first sections=2}`; a story with nothing in it prints `{position:
  nothing of notes (0 of 0 blocks)}`).
- `unmatched-old.docx`, `unmatched-new.docx`: a header in the newer version only, beside a body
  change. `compare-unmatched.json` is compare's whole answer: its receipt warns
  `compare_story_unmatched`, and the redline's header shows no change.
- `stories-old.docx`, `stories-new.docx`: two sections; a footnote, an endnote, a header both
  sections use and a first-page footer in the second, each changed. `compare-stories.json` warns
  nothing.
- `excluded-old.docx`, `excluded-new.docx`: an inserted paragraph that ends a section, which
  compare can't mark up whole: `compare-excluded.json` warns `compare_not_restored` for `p2` (one
  of its exclusions), and the section's break shows no mark.
- `cellfmt-body-old.docx`, `cellfmt-body-new.docx`: the shading of `cellfmt-*.docx` beside a body
  change; compare marks the body's change only.
- `cover-old.docx`, `cover-new.docx`: a cover section inserted first with a header of its own,
  while the shading of the table in the old header (now on section 2) changes, beside a body
  change. compare warns `compare_not_restored` for the inserted paragraph that ends the cover
  section; its headers' view shows both headers unchanged.
- `added-old.docx`, `added-new.docx`: an existing paragraph ends a new first section with a header
  of its own, beside a body change. compare warns nothing, marks neither the header nor the
  section break, and its headers' view shows the new header unchanged.
- Each of these four pairs has `view-<pair>.json`, `check-<pair>.json` and
  `final-<pair>-old.json`, `final-<pair>-new.json` as above.

All of them came from the pinned v0.9.3 release binary for aarch64-apple-darwin:

```text
stetdoc compare --json --author=Clauseo -o redline.docx <pair>-old.docx <pair>-new.docx > compare-<pair>.json
stetdoc view --all --json --story notes redline.docx
stetdoc view --all --json --story headers redline.docx
```

`make_docs.py` (from the stetdoc thread's repro, unchanged) made the first three pairs; it also
writes two files for another blocker, so run it in a temporary folder. `make_more.py` makes the
other pairs, and `make_links.py` (which reads both for their helpers) the ones above. Both are Python 3, standard library only. The views and the compare's answer came
from:

```text
stetdoc compare --json --author=Clauseo -o redline.docx <name>-old.docx <name>-new.docx
stetdoc view --all --json redline.docx
```

`view-nested.json` and `view-hidden.json` came from the pinned v0.9.3 release binary for
aarch64-apple-darwin; the rest from stetdoc 0.9.3's Linux wheel on PyPI (`uvx --from
stetdoc==0.9.3`), since the pinned Linux binary needs a newer glibc than the machine had. The
two agree on `view-nested.json` but for its `base` hash.
