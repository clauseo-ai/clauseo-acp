# 0043: Search inside matters and conversations

## Context

Find is a day-one feature. The lawyer types, and the results show matching matters, conversations,
entries, tool calls and document file names, each with a snippet and a time. Results must keep up
with typing: p95 under 100 ms for a 2-year practice, taken as 5,000 conversations in about 200
matters. Conversations mix English, Hindi in Devanagari and Hinglish, and cite sections and cases
(`s. 138`, `Order XXXIX Rule 1`, `(2019) 5 SCC 1`). The index must be local, update as events
commit, and be rebuildable from the log ([0006](0006-store-format.md)). Document contents
("papers") come later; this record keeps room for them.

A spike measured a synthetic corpus of 20-entry conversations (prompts of about 300 characters,
answers of about 3,000, mixed scripts, citations; 44.6 MB of raw text per 1,000 conversations) on a
loaded 8-core Linux VM with a warm cache, snippets for 20 rows included.

| Tokenizer | Conversations | FTS index | Whole file | Bulk build |
|---|---|---|---|---|
| unicode61 + `M*` (chosen) | 1,000 | 12.1 MB (27% of raw) | 66 MB | 1.6–1.8 s |
| unicode61 + `M*` (chosen) | 5,000 | 59.8 MB | 331 MB | 12–13 s |
| unicode61 + `M*` (chosen) | 10,000 | 119 MB | 659 MB | 19–28 s |
| unicode61 (default) | 5,000 | 68.7 MB (31%) | 340 MB | 10 s |
| trigram | 5,000 | 426 MB (191%) | 697 MB | 46 s |

At 5,000 conversations, ranked over the newest 2,000 matches, p95 was 5.4 ms (a rare word) to
16.2 ms (two words); a common word with 48k matching rows took 11.5 ms. Newest first with a half
typed word took 7.6–13.3 ms (27 ms or less at 10,000). Ranking every match took 196 ms for a
common word, and a ranked prefix 288 ms: bm25 counts a term's rows in a full pass, so both fail
the target. Inserting one entry: p95 1.1 ms, max 17 ms when FTS5 merges segments. Deleting a
matter: 24–37 ms, or 1.4–1.6 s with secure-delete.

## Decision

- **Engine.** SQLite FTS5 from the bundled SQLite that rusqlite already links, in the crate
  `clauseo-search` (protocol and rusqlite only). No new external crate.
- **Tokenizer.** `unicode61 remove_diacritics 2 categories 'L* N* Co M*'`. The default unicode61
  treats combining marks as separators, so it drops Devanagari vowel signs and the virama:
  `किरायेदार` is stored as `क | र | य | द | र`, a query matches only because it splits the same
  way, `ना` gives false hits through `न`, prefixes mean nothing, and Devanagari queries run about
  3× slower. With `M*`, words stay whole: `न्यायालय` and the prefix `अनु*` match, a fragment does
  not. `remove_diacritics 2` still folds Latin accents, and punctuation is dropped, so the phrase
  `"2019 5 scc 1"` matches `(2019) 5 SCC 1`.
- **The file.** `search.sqlite` (with `-wal` and `-shm`) in the data folder beside
  `state.sqlite`. It is derived, outside 0006's frozen schema, and left out of backups and exports.
  It holds a plain-text copy of conversations, so it gets the store's protection: created with mode
  `0o600` before SQLite opens it, and the OS's full-disk encryption at rest.
- **Schema.** `items` holds the text and its columns (`kind`, `matter`, `thread`, `entry` (the
  entry's order, the seq of its first event), `path`, `content` and `part` for papers, `at`,
  `archived`); `items_fts` indexes it as external content, so `snippet()` reads from `items`.
  Kinds: 1 matter title, 2 thread title, 3 prompt, 4 answer, 5 tool title, 6 file name, 7 paper
  text (reserved), 8 thought (reserved). `matters` and `threads` hold the current titles and the
  archived flag, so each hit carries its matter's and conversation's title. `meta` holds the format
  number, the tokenizer and whether the build completed. Rows go in in time order, so rowid tracks
  recency.
- **Indexed on day one.** Prompts when they commit; answers (one row per agent message of kind
  answer) at turn end; tool titles, at weight 0.8; thread titles; matter titles; file names from
  the matter's checkpoint listing, kept current from `FilesChanged`, `OutsideEdits` and
  `VersionRestored`. `ThreadArchived` sets `archived`. Thoughts are left out, under a reserved kind.
  Papers will be rows of kind 7 in parts of about 4 KB, their text extracted once into a cache,
  `extract.sqlite`, that survives a rebuild: no format change.
- **Writes** (the host's part 2 feeds them from committed events on a `clauseo-search` thread,
  never the actor): a put replaces the row with the same key (an entry or tool title by thread and
  entry, a title by its thread or matter, file names per matter). Undone steps' rows are deleted
  (`delete_entries`). Deletes of a thread or matter remove their rows under FTS5 `secure-delete`
  with `PRAGMA secure_delete=ON`, and end with a WAL checkpoint, so deleted text does not linger.
- **Query modes.** While the last word is being typed, matches come newest first and the last word
  is a prefix. Otherwise they are ranked over the newest 2,000 matches by
  `bm25 × kind weight / (1 + age in days / 180)`, with weights title 3, file name 2, prompt 1.2,
  answer 1, tool title 0.8. The crate builds the FTS5 query itself: every word is quoted, typed
  quotes make a phrase, a citation-like run becomes one phrase, at most 16 terms are kept. Raw
  input never reaches `MATCH` (`a"b`, `OR`, `NEAR(` and `foo:bar` are syntax errors there).
- **Results.** A page carries the hits, a cursor for the next page, the mode and a rebuild's
  progress (`done` of `total` conversations). A hit carries its kind, matter, thread, entry, path,
  time, the matter's and conversation's titles, and its snippet as segments (`text`, `hit`), at most
  32 tokens and 240 characters. A match marks its whole token, a prefix match included. Segments
  rather than offsets: the app never maps between UTF-8 and UTF-16.
- **Paging.** Newest first pages by rowid. Ranked pages are offsets into the window, pinned to the
  newest rowid and the time of the first page, so rows that arrive meanwhile do not shift them. A
  cursor travels as text (base64url of a small versioned record with an FNV-1a check of the
  query); text that does not decode is refused, and a cursor used with another query starts over
  from the first page. A newer query from the same window interrupts the running statement
  (`sqlite3_interrupt`).
- **Rebuilds.** When the file is missing, does not open, has another format or tokenizer, or its
  build never completed (a rebuild marks the file incomplete when it starts), it is emptied and
  reported as needing a rebuild. Nothing is migrated. The host reopens threads through the store
  and the reducer, so entries are assembled as the views assemble them, inserts in time order and ends with an `optimize`. At 5,000 conversations the FTS
  part takes 12–13 s, measured; the whole rebuild is estimated at 30–60 s.
- **The host's wiring.** A `clauseo-search` thread owns the index; the actor never touches it.
  At the end of applying each returned commit the actor `try_send`s the commit's envelopes on a
  bounded channel (1,024 commits) and never waits; when it is full the actor sets a lagged flag
  and the thread reads every stream whose head is past its watermark from the store. A
  watermark (table `watermarks`, the last `seq` of each stream the index reflects) is written in
  the same transaction as the rows it covers, so the thread only ever indexes durable events and
  a crash replays at most what the last transaction did not cover. A commit that does not follow
  on from its stream's watermark makes the thread read that stream from the store too. Each
  thread's fold (prompt and tool-call orders, each turn's entries, the text of running turns'
  answers) is kept for the 64 most recently touched threads and folded again from the log
  otherwise. An answer is assembled as the reducer assembles an agent message (its
  `TextAppended` chunks in seq order, the first chunk's kind and seq), tested against the
  reducer. A steer's prompt follows the turn its `SteerResolved` names, as the reducer moves it,
  so an undo takes exactly that turn's prompts; a queued prompt taken back before it was sent
  (`RemoveQueuedPrompt`, a turn that ends cancelled without starting) leaves the index. A thread
  whose matter is forgotten writes nothing, even when the fold only learns its matter then.
  File names are listed from the matter's history (`Matter::files_at`) at each
  `CheckpointRecorded`, `OutsideEdits` and `VersionRestored`, on the search thread: a checkpoint
  follows every change of files, so the listing replaces the per-change deltas. A reopen and
  listing of a matter of 10,000 files and 1,000 checkpoints took 38 ms (11.5 ms on a matter
  already open), measured on the review's request. `ForgetMatter`
  deletes the matter's rows (a forgotten matter must not show in search); importing it again
  indexes its streams from the whole log.
- **Rebuilds in place.** The crate has no build-beside-and-rename step, so a rebuild runs in the
  emptied `search.sqlite` and a search meanwhile answers from what is indexed so far, with
  `indexing: { done, total }` in conversations. The thread walks every matter's stream, then
  every conversation least recently active first, so the newest conversations get the highest
  row ids (newest first pages by row id); rows within a conversation keep their time order. The
  host serves at once: the index opens and rebuilds on its own thread, and searches are answered
  between two conversations of the rebuild.
- **The query.** `HostQuery::Search` (or `HostHandle::search`, which names the window) answers
  `QueryResult::Search`: a `SearchPage` or `Cancelled`. The host picks newest first while the
  query's last character is not whitespace and ranked otherwise; a cursor that does not decode
  gives the first page. A hit carries its entry's `order` in the conversation's view, which
  `PageEntries` pages by (the index keys rows by it; it holds no entry ids). A newer
  `generation` from the same window interrupts that window's running query, answers its queued
  older ones `Cancelled`, and an older one that finishes late is answered `Cancelled` instead of
  its page. The desktop sends the window's label and its page's `open_stream` number, so a
  reloaded page counts its generations afresh; only the main window may search (the settings
  window reads `GetSettings` alone, 0028).
- **Release one answers.** Tool titles are indexed at a low weight and can be filtered by kind.
  Thoughts are out, with kind 8 reserved. Undone steps' rows are dropped. Hinglish matches exact
  spellings; spelling variants and matching across scripts need transliteration, a later design.

### Rejected

- **Porter stemming.** It stems prefix terms too: `kiray*` becomes `kirai*` and finds nothing,
  while `kiraye*` does, so results would flicker while the lawyer types.
- **Trigram.** 7× the index and about 4× the build time; it cannot search under 3 characters,
  needs the citation's exact punctuation, and is slower on every ranked query. A small trigram table
  over titles and file names is the shape if substring search is wanted later.
- **`prefix='2 3'`.** Ranked 2- and 3-character prefixes drop to 15 ms p95, but the index grows
  2.5×. The file is derived, so it can be turned on later with a rebuild.
- **Tantivy.** A large dependency tree and a second storage engine (segment files, merge threads,
  locks). Its default tokenizer splits on `!is_alphanumeric()`, and the virama U+094D is not
  alphanumeric, so `न्यायालय` would become `न | यायालय`. Its gains (scoring by date, fuzzy
  queries) are not needed at this size.
- **Tables in `state.sqlite`.** They would freeze a tuning-prone shape under 0006, add about 1.2×
  the raw text to every backup, and put FTS merges and secure deletes on the store's one writer.
- **A contentless table.** It saves about 80% of the file, but each hit's snippet on each keystroke
  would then be reassembled from `TextAppended` chunks in the store.

## Consequences

- Search reaches every matter without loading threads, and never blocks the actor or the store's
  writer.
- It costs about 66 MB per 1,000 conversations, mostly the text copy kept for snippets. Keeping
  `items.body` compressed and building snippets in Rust is the lever if that matters.
- Any change to the format or the tokenizer bumps `FORMAT` and triggers a rebuild.
- A new event that adds, changes or removes searchable text must reach the index; any new delete
  command must delete index rows.
- Text of a running turn is not found until the turn ends; a file dropped into the folder is found
  by name only after the next checkpoint records it.
- Papers: about 800 MB of text for 200 matters × 100 documents gives about 216 MB of index and
  35–50 s of indexing. Extraction dominates (about 6 ms per PDF page with a text layer); OCR of
  scanned pages costs days of CPU, so it must be opt-in.

## Links

- [0006: Store format](0006-store-format.md): the log the index is built from, the file mode, and
  the at-rest rule.
- [0002: Event schema](0002-event-schema.md): the events the index reads.

## Status

proposed

## Date

2026-11-03
