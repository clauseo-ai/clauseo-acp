# Tracked changes outside the body

Two Word files from the field report that a file whose tracked changes are all in its headers,
footers, notes or formatting was never offered Tracked changes. They are kept byte for byte as they
were made (a stdlib-only Python script, section 1 of the report's `make_docs.py`).

- `tracked-nonbody-only.docx`: six tracked changes, none an insertion, deletion or move of the
  body's text. A run format change (id 2) and a paragraph format change (id 3) in the body, an
  insertion in the header (11), a deletion in the footer (12), and an insertion in a footnote (13)
  and in an endnote (14).
- `tracked-nonbody-and-format.docx`: the same, plus one insertion in the body (id 1).

stetdoc 0.9.3 lists, accepts and rejects all of them. Its `view --revisions` ledger says
`in headers` for the header and the footer and `in notes` for both notes.

Used by `src/revisions/marks/tests.rs` (the papers scan) and `tests/host/revisions_parts.rs`
(listing and deciding them, with the fake stetdoc and with the real one when it is on `PATH`).
