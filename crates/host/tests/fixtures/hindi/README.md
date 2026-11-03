# Hindi fixture matter

Four papers in Hindi for the Hindi question test (`tests/host/hindi.rs`): the fake-agent
variant copies them into a matter folder in CI, and the live variant asks Claude or Codex
about them. `generate.py` makes all four, and `patra-edited.docx` (the fifth file, below), which
the Word round trip test (`tests/host/word_round_trip.rs`) uses. Rerunning it gives the same
bytes.

| File | What it is | Planted fact |
|---|---|---|
| `kirayanama.pdf` | A Unicode PDF: a lease (किरायानामा), shaped with HarfBuzz, set in an embedded subset of Noto Sans Devanagari with a ToUnicode map, and each paragraph's text as `ActualText` (a shaped glyph such as a reordered i-matra or a conjunct stands for letters in another order, which a ToUnicode map cannot say). `pdftotext` gives the exact text; PyMuPDF gives it with a few repeats; pypdf ignores `ActualText` and loses matras, as with many real Hindi PDFs. | Yes: ₹3,47,215, 17 मार्च 2024, सुरेखा भार्गव (ASCII digits) |
| `notice-legacy.pdf` | A "legacy-encoded" PDF: a legal notice made the same way with no `ActualText` and its ToUnicode map deleted. The glyphs still draw correctly, but text extraction gives the subset's one-byte codes (control characters and WinAnsi symbols such as `€†‡`; only the ASCII digits and commas survive): the failure a PDF set in Kruti Dev or a similar legacy font shows. No legacy font is used or copied: they are freeware with unclear redistribution terms. | No |
| `affidavit-scan.pdf` | A scanned PDF: an affidavit (शपथपत्र) made as a Unicode page, rasterised at 110 dpi to one greyscale JPEG, with no text layer and no font. | No |
| `patra.docx` | A Hindi Word letter from the tenant's advocate, in NFC. Its third paragraph holds nukta letters (ज़, क़, फ़), which NFC keeps as base letter + U+093C. | Yes: ₹३,४७,२१५, १७ मार्च २०२४, सुरेखा भार्गव (Devanagari digits) |
| `patra-edited.docx` | `patra.docx` with one paragraph added before the signature (`संशोधन: यह पत्र १८ मार्च २०२४ को संशोधित किया गया।`). It stands in for the edit a stetdoc change makes to the Word document, in the round trip test only; the live test does not use it. | Yes, as `patra.docx` (the same letter) |

The landlord, दीनदयाल मिश्र, appears in all four; only the tenant's name, the deposit and the
lease date make the planted fact. The same values are in `generate.py` and in
`tests/host/hindi/verdict.rs`; `hindi::the_fixture_generator_plants_the_facts_the_check_looks_for`
fails when they disagree.

## Making them again

```
python3 generate.py /path/to/NotoSansDevanagari-Regular.ttf
```

Needs Python 3.11 with reportlab 4.4, uharfbuzz 0.57 (reportlab shapes with it), pikepdf,
PyMuPDF, Pillow and python-docx. The font is the hinted TrueType from
<https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSansDevanagari/hinted/ttf/NotoSansDevanagari-Regular.ttf>
(version 2.007, SHA-256 `4e3c66638958c3e2ab5d37f47a8deb89fffeb7be9985c665a519bbc7ba762313`).

## Licences

- Noto Sans Devanagari: SIL Open Font License 1.1, Copyright 2022 The Noto Project Authors
  (<https://github.com/notofonts/devanagari>). The two Unicode-made PDFs embed a subset of it,
  which the OFL allows in documents; the font file itself is not committed.
- The text of the papers is made up for these tests (no real people or cases) and is under the
  repository's licence, as is `generate.py`.
- The `.docx` is python-docx's default template with this text; the template carries no
  third-party content beyond python-docx's MIT-licensed styles.
