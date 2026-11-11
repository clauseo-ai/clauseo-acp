# Word blackline fixtures

The inputs and the captured stetdoc output for the Word blackline (`HostQuery::WordBlackline`):
the parser's unit tests (`src/actor/query/blackline/parse/tests.rs`) and the host tests
(`tests/host/blackline.rs`) read them.

- `old.docx`, `new.docx`: a short English lease, before and after. Between them: a heading kept,
  words changed in a paragraph, a paragraph deleted and one added, a numbered list item changed,
  a table cell changed, a footnote changed, a paragraph holding `{`, `}`, `\` and a tab, and a
  word made bold (a format change only).
- `compare.json`: the stdout of `stetdoc compare --json --author Clauseo --date
  2026-11-09T10:00:00 -o redline.docx old.docx new.docx`.
- `view.json`: the stdout of `stetdoc view --all --json redline.docx` on that redline.

`generate.py` made all four with stetdoc 0.9.0 on `PATH` and Python 3.9, standard library only.
It takes the package of `../hindi/patra.docx` (styles, numbering, settings) as the template,
replaces the body of `word/document.xml`, adds a footnotes part, and writes the redline to a
temporary folder, which it removes. The temporary redline's path in both JSON files is written
as `redline.docx`. To make them again:

```text
python3 crates/host/tests/fixtures/blackline/generate.py
```

The view's format was read from this output and from stetdoc 0.9.0's output for other
documents (links, comments, fields, content controls, images, deleted list items, inserted and
deleted table rows). Its markup is listed in the parser's module doc. A stetdoc that prints
anything else makes the blackline fail (`blackline.failed`), and does not show a guess.
