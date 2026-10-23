# Frozen v1 corpus

Log entries (`Envelope`s) as `clauseo-protocol` wrote them at `SCHEMA_VERSION` 1: every known
`DomainEvent` kind and every variant of every open enum a log entry holds (each value of a
unit-only one too), each at least once, with every optional field filled somewhere. A kind may
have more than one entry. `Delivery::Queue` is the one value no entry spells: it is written as a
`user_message` without `delivery`, which the corpus holds.

The optional fields were checked once, when v1 was frozen: a throwaway check decoded every entry
and found each of the 54 optional fields of the logged types set in at least one. No test repeats
that check, because a field added after v1 is rightly absent from it. What the test does check is
the other direction: every field an entry stores is still there after the entry is decoded and
written back (`every_stored_field_survives_a_round_trip`), so a stored field that is renamed or
removed fails even when it was optional.

- `events.jsonl` holds one entry per line, as `serde_json::to_string` writes it.
- `events.msgpack` holds one MessagePack array of the same entries, as `rmp_serde::to_vec_named`
  writes them (named maps, the stored form).

These files are frozen. Never regenerate or edit them: nothing in the repo writes them, and insta
never touches them. `crates/protocol/tests/corpus.rs` decodes every entry in both formats with the
current types, checks that no entry reads as `Unknown`, that the two files hold the same entries,
that every stored field survives a round trip, that every v1 kind is still known, and that every
variant of every open enum, data or unit, appears under its own kind (`Delivery::Queue` as an
entry without the field). It also pins each file's byte length and FNV-1a hash, so any edit to
them fails that test.

## Adding a kind or a variant after v1

v1 cannot grow, so whatever is added later brings its own frozen entries in a folder `added/`
beside `v1/`, decoded and pinned the same way:

1. Give the new kind or variant an example in `crates/protocol/tests/support/examples.rs`, with
   every optional field filled (the snapshots need it anyway).
2. Run the ignored writer, from the repo root:

   ```sh
   INSTA_UPDATE=always cargo test -p clauseo-protocol --test corpus -- --ignored --nocapture write_the_added_pairs
   ```

   It finds each kind and each open enum's variant that neither v1 nor `added/` holds, and writes
   its pair into `target/tmp/corpus-added/`, never into this folder: `<kind>.jsonl` (one entry per
   line, `serde_json::to_string`) and `<kind>.msgpack` (one array, `rmp_serde::to_vec_named`)
   for a new kind, with every example of it; `<kind>-<Variant>` for a new variant, with the first
   example that holds it, `<kind>` being that example's event. The files are insta binary
   snapshots, the crate's one way to write a file from a test. For each pair it prints the two
   `mv` commands that put the files in `corpus/added/` and the pair's `ADDED_AFTER_V1` row, with
   each file's byte length and FNV-1a hash.
3. Run the printed `mv` commands and paste the row into `ADDED_AFTER_V1` in
   `crates/protocol/tests/corpus.rs`. A row whose pins are still `(0, 0)` fails with the pin to
   set.

The test fails until this is done: the known kinds must be exactly v1's plus those of the added
files, and every variant must appear somewhere in v1 or the added files. The added files are as
frozen as v1.

One gap remains. A unit value counts for every open enum its string decodes as, wherever it
sits: `completed` counts for `ToolCallStatus`, `PlanStatus` and `CompactionStatus` alike. So a
new unit variant spelled like a value the corpus already holds for another enum passes without an
entry of its own; give it its `added/` pair all the same.

A later schema adds its own corpus as `v2/` beside this one. v1 must keep decoding for as long as
Clauseo reads logs written at `SCHEMA_VERSION` 1: a change that breaks it needs a
`schema_version` bump and an upcaster, not a new corpus.
