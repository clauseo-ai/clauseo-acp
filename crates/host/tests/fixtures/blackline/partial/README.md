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
- `inner-last-old.docx`, `inner-last-new.docx`: an inner table that is the last thing in its
  cell, removed in the new version. `compare-inner-last.json` is what `stetdoc compare --json`
  printed for them: `validation_failed`, exit 5 (stetdoc's fix comes after its PR #22).

`make_docs.py` (from the stetdoc thread's repro, unchanged) made the first three pairs; it also
writes two files for another blocker, so run it in a temporary folder. `make_more.py` makes the
other pairs. Both are Python 3, standard library only. The views and the compare's answer came
from:

```text
stetdoc compare --json --author=Clauseo -o redline.docx <name>-old.docx <name>-new.docx
stetdoc view --all --json redline.docx
```

`view-nested.json` and `view-hidden.json` came from the pinned v0.9.3 release binary for
aarch64-apple-darwin; the rest from stetdoc 0.9.3's Linux wheel on PyPI (`uvx --from
stetdoc==0.9.3`), since the pinned Linux binary needs a newer glibc than the machine had. The
two agree on `view-nested.json` but for its `base` hash.
